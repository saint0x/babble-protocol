//! Bounded redo publication journal. The committed rename is the decision point;
//! no canonical record is touched before it, and recovery always rolls forward.
use super::*;
use std::{collections::BTreeSet, fs::File, sync::atomic::Ordering};

const MAX_RECORDS: usize = 256;
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const JOURNAL: &str = ".publication-committed";
const STAGING: &str = ".publication-prepared";

#[derive(Debug, thiserror::Error)]
pub enum PublicationError {
    #[error("publication not committed: {0}")]
    Precommit(CoreError),
    #[error(
        "publication {transaction} commit outcome uncertain; reopen store before retry: {source}"
    )]
    Uncertain {
        transaction: String,
        source: CoreError,
    },
    #[error("publication {transaction} committed; recovery required before retry: {source}")]
    Committed {
        transaction: String,
        source: CoreError,
    },
}

impl From<CoreError> for PublicationError {
    fn from(error: CoreError) -> Self {
        Self::Precommit(error)
    }
}

/// Validated records only. Object/edge/event IDs are immutable; judgments may be
/// refreshed (their IDs omit evaluation time). Existing values are journaled.
#[derive(Default)]
pub struct PublicationBatch {
    pub(super) records: Vec<Record>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub(super) collection: String,
    pub(super) id: String,
    pub(super) value: Value,
    pub(super) previous: Option<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    digest: Hash,
    payload: String,
}

impl PublicationBatch {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn object(&mut self, object: &Object, author: &Identity) -> Result<()> {
        object.verify(author)?;
        self.push("objects", object.id.as_str(), object)
    }

    pub fn edge(&mut self, edge: &Edge, author: &Identity) -> Result<()> {
        edge.verify(author)?;
        self.push("edges", edge.id.as_str(), edge)
    }

    pub fn event(&mut self, event: &Event, actor: &Identity) -> Result<()> {
        event.verify(actor)?;
        self.push("events", event.id.as_str(), event)
    }

    pub fn judgment(&mut self, judgment: &Judgment) -> Result<()> {
        judgment.id.validate()?;
        if !judgment.confidence.is_finite() || !(0.0..=1.0).contains(&judgment.confidence) {
            return Err(conflict("invalid publication judgment confidence"));
        }
        self.push("judgments", judgment.id.as_str(), judgment)
    }

    /// Stage both records or neither. Identical association retries are allowed;
    /// conflicting requests for an existing judgment ID are never overwritten.
    pub fn object_judgment(
        &mut self,
        input: &ObjectJudgmentInput,
        judgment: &Judgment,
    ) -> Result<()> {
        input.validate_judgment(judgment)?;
        if self.records.len() > MAX_RECORDS - 2 {
            return Err(conflict("publication record limit exceeded"));
        }
        if self.records.iter().any(|record| {
            record.id == judgment.id.as_str()
                && matches!(
                    record.collection.as_str(),
                    "judgments" | "object_judgment_inputs"
                )
        }) {
            return Err(conflict("duplicate publication object judgment"));
        }
        // Serialize both before mutating the batch, including on encoding failure.
        let input_value = serde_json::to_value(input).map_err(encoding)?;
        let judgment_value = serde_json::to_value(judgment).map_err(encoding)?;
        self.records.extend([
            Record {
                collection: "object_judgment_inputs".into(),
                id: input.judgment_id.to_string(),
                value: input_value,
                previous: None,
            },
            Record {
                collection: "judgments".into(),
                id: judgment.id.to_string(),
                value: judgment_value,
                previous: None,
            },
        ]);
        Ok(())
    }

    pub fn receipt(&mut self, receipt: &PublicationReceipt) -> Result<()> {
        receipt.validate()?;
        self.push("publication_receipts", receipt.request.id.as_str(), receipt)
    }

    pub(super) fn push(
        &mut self,
        collection: &str,
        id: &str,
        value: &impl Serialize,
    ) -> Result<()> {
        if self.records.len() >= MAX_RECORDS {
            return Err(conflict("publication record limit exceeded"));
        }
        let value = serde_json::to_value(value).map_err(encoding)?;
        self.records.push(Record {
            collection: collection.into(),
            id: id.into(),
            value,
            previous: None,
        });
        Ok(())
    }
}

impl FileStore {
    pub fn publication_receipt(&self, id: &Hash) -> Result<Option<PublicationReceipt>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        let Some(value) = read_value(&self.path("publication_receipts", id.as_str())?)? else {
            return Ok(None);
        };
        let receipt: PublicationReceipt = serde_json::from_value(value).map_err(encoding)?;
        receipt.validate()?;
        if &receipt.request.id != id {
            return Err(conflict("publication receipt ID mismatch"));
        }
        if receipt.is_event_only() {
            let event = read_value(&self.path("events", receipt.outcome.event.as_str())?)?
                .ok_or_else(|| conflict("consent receipt event missing"))?;
            validate_value("events", receipt.outcome.event.as_str(), &event)?;
            validate_consent_receipt(&receipt, &serde_json::from_value(event).map_err(encoding)?)?;
        }
        Ok(Some(receipt))
    }

    /// Reject use after an uncertain/committed I/O failure. Failed handles stay
    /// invalid after another handle recovers; their callers must reopen too.
    pub fn check_ready(&self) -> Result<()> {
        let _guard = self.publication_guard()?;
        Ok(())
    }

    /// The OS lock is held through checkpoint and journal retirement. Separate
    /// opens use separate file descriptions, so clones and processes serialize.
    fn publication_lock(&self) -> Result<File> {
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.root.join(".publication-lock"))
            .map_err(io_error)?;
        file.lock().map_err(io_error)?;
        Ok(file)
    }

    pub(crate) fn publication_guard(&self) -> Result<File> {
        let lock = self.publication_lock()?;
        if self.recovery_required.load(Ordering::Acquire)
            || self.root.join(JOURNAL).try_exists().map_err(io_error)?
        {
            return Err(conflict("publication recovery required; reopen store"));
        }
        Ok(lock)
    }

    pub fn commit_publication(
        &self,
        batch: PublicationBatch,
    ) -> std::result::Result<(), PublicationError> {
        self.commit_with_hook(batch, &mut |_| Ok(()))
    }

    fn commit_with_hook(
        &self,
        batch: PublicationBatch,
        hook: &mut impl FnMut(&str) -> Result<()>,
    ) -> std::result::Result<(), PublicationError> {
        let _lock = self.publication_guard()?;
        self.commit_publication_locked(batch, hook)
    }

    // Only callers holding publication_guard may enter here. Preparation uses
    // this to atomically look up the retry key and create its first phase.
    pub(super) fn commit_publication_locked(
        &self,
        mut batch: PublicationBatch,
        hook: &mut impl FnMut(&str) -> Result<()>,
    ) -> std::result::Result<(), PublicationError> {
        validate_records(&batch.records)?;
        self.validate_invocation_preimages(&batch.records, false)?;
        for record in &mut batch.records {
            let path = self.path(&record.collection, &record.id)?;
            let current = read_value(&path)?;
            if (matches!(
                record.collection.as_str(),
                "objects" | "edges" | "publication_receipts" | "invocations"
            ) && current.is_some())
                || (record.collection == "events"
                    && current.as_ref().is_some_and(|value| value != &record.value))
            {
                return Err(conflict(format!("publication conflict: {}", record.id)).into());
            }
            record.previous = current;
        }
        // A corrupt existing Judgment must not become an unrecoverable preimage
        // in a newly committed journal.
        validate_records(&batch.records)?;
        self.validate_existing_object_judgments(&batch.records)?;
        let payload = serde_json::to_string(&batch.records).map_err(encoding)?;
        let digest = Hash::from_bytes(payload.as_bytes());
        let transaction = digest.to_string();
        let bytes = serde_json::to_vec(&Journal {
            version: 1,
            digest,
            payload,
        })
        .map_err(encoding)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(conflict("publication byte limit exceeded").into());
        }
        hook("validated")?;
        sync_write(&self.root.join(STAGING), &bytes)?;
        hook("prepared")?;
        // A rename error is conservatively ambiguous (notably on remote filesystems).
        let uncertain = |source| {
            self.recovery_required.store(true, Ordering::Release);
            PublicationError::Uncertain {
                transaction: transaction.clone(),
                source,
            }
        };
        fs::rename(self.root.join(STAGING), self.root.join(JOURNAL))
            .map_err(io_error)
            .map_err(&uncertain)?;
        hook("renamed").map_err(&uncertain)?;
        sync_dir(&self.root).map_err(&uncertain)?;
        let committed = |source| {
            self.recovery_required.store(true, Ordering::Release);
            PublicationError::Committed {
                transaction: transaction.clone(),
                source,
            }
        };
        hook("committed").map_err(&committed)?;
        self.install_publication(&batch.records, hook)
            .map_err(&committed)?;
        self.update_judgment_index(
            batch
                .records
                .iter()
                .filter(|record| record.collection == "judgments")
                .map(|record| JudgmentId::new_unchecked(&record.id)),
            hook,
        )
        .map_err(&committed)?;
        self.retire_publication(hook).map_err(&committed)
    }

    pub(crate) fn recover_publication(&self) -> Result<()> {
        let _lock = self.publication_lock()?;
        let path = self.root.join(JOURNAL);
        if path.try_exists().map_err(io_error)? {
            let bytes = read_bounded(&path)?;
            let journal: Journal = serde_json::from_slice(&bytes).map_err(encoding)?;
            if journal.version != 1
                || Hash::from_bytes(journal.payload.as_bytes()) != journal.digest
            {
                return Err(conflict("corrupt publication journal version or digest"));
            }
            let records: Vec<Record> = serde_json::from_str(&journal.payload).map_err(encoding)?;
            validate_records(&records)?;
            self.verify_recovered_signatures(&records)?;
            self.install_publication(&records, &mut |_| Ok(()))?;
            // Recovery may start with an absent, obsolete, or partially updated
            // index. Rebuild only after all canonical pairs have been installed.
            self.rebuild_judgment_index()?;
            self.retire_publication(&mut |_| Ok(()))?;
        } else {
            self.rebuild_judgment_index()?;
        }
        // Staging has no commit decision and never changes canonical records.
        match fs::remove_file(self.root.join(STAGING)) {
            Ok(()) => sync_dir(&self.root)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(io_error(error)),
        }
        // Persist newly-created collection directories before any future commit.
        sync_dir(&self.root)?;
        if let Some(parent) = self
            .root
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
        {
            sync_dir(parent)?;
        }
        Ok(())
    }

    pub(super) fn verify_recovered_signatures(&self, records: &[Record]) -> Result<()> {
        let mut state = babble_state::MemoryState::default();
        for identity in read_all_json::<Identity>(&self.root.join("identities"))? {
            state.apply_identity(identity)?;
        }
        let mut transitions = read_all_json::<Event>(&self.root.join("events"))?;
        transitions.retain(|event| event.kind == babble_state::EventKind::IdentityKeyTransition);
        transitions.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        for event in transitions {
            state.apply_event(event)?;
        }
        for record in records {
            match record.collection.as_str() {
                "objects" => {
                    let object: Object =
                        serde_json::from_value(record.value.clone()).map_err(encoding)?;
                    object
                        .verify(&state.signing_identity_at(&object.author, object.created_at)?)?;
                }
                "edges" => {
                    let edge: Edge =
                        serde_json::from_value(record.value.clone()).map_err(encoding)?;
                    let author = edge.author.as_ref().ok_or(CoreError::UnsignedEdge)?;
                    edge.verify(&state.signing_identity_at(author, edge.created_at)?)?;
                }
                "events" => {
                    let event: Event =
                        serde_json::from_value(record.value.clone()).map_err(encoding)?;
                    event.verify(&state.signing_identity_at(&event.actor, event.created_at)?)?;
                }
                _ => (),
            }
        }
        Ok(())
    }

    fn install_publication(
        &self,
        records: &[Record],
        hook: &mut impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        self.validate_existing_object_judgments(records)?;
        self.validate_invocation_preimages(records, true)?;
        // Check the entire recovery batch first. Never overwrite divergent data.
        for record in records {
            let current = read_value(&self.path(&record.collection, &record.id)?)?;
            if current != record.previous && current.as_ref() != Some(&record.value) {
                return Err(conflict(format!(
                    "publication recovery conflict: {}",
                    record.id
                )));
            }
        }
        for (index, record) in records.iter().enumerate() {
            let path = self.path(&record.collection, &record.id)?;
            let bytes = serde_json::to_vec_pretty(&record.value).map_err(encoding)?;
            let temporary = path.with_extension("publication-tmp");
            sync_write(&temporary, &bytes)?;
            hook(&format!("record-{index}-prepared"))?;
            fs::rename(&temporary, &path).map_err(io_error)?;
            hook(&format!("record-{index}-renamed"))?;
        }
        for collection in records
            .iter()
            .map(|r| &r.collection)
            .collect::<BTreeSet<_>>()
        {
            sync_dir(&self.root.join(collection))?;
        }
        hook("installed")
    }

    fn validate_existing_object_judgments(&self, records: &[Record]) -> Result<()> {
        for record in records
            .iter()
            .filter(|record| record.collection == "judgments")
        {
            // Paired records and their preimages are checked by validate_records.
            if records
                .iter()
                .any(|input| input.collection == "object_judgment_inputs" && input.id == record.id)
            {
                continue;
            }
            if let Some(input) =
                self.object_judgment_input_unlocked(&JudgmentId::new_unchecked(&record.id))?
            {
                let judgment = serde_json::from_value(record.value.clone()).map_err(encoding)?;
                input.validate_judgment(&judgment)?;
                if let Some(previous) = &record.previous {
                    input.validate_judgment(
                        &serde_json::from_value(previous.clone()).map_err(encoding)?,
                    )?;
                }
            }
        }
        Ok(())
    }

    fn retire_publication(&self, hook: &mut impl FnMut(&str) -> Result<()>) -> Result<()> {
        fs::remove_file(self.root.join(JOURNAL)).map_err(io_error)?;
        hook("retired")?;
        sync_dir(&self.root)?;
        hook("finished")
    }
}

pub(super) fn validate_records(records: &[Record]) -> Result<()> {
    if records.is_empty() || records.len() > MAX_RECORDS {
        return Err(conflict("invalid publication record count"));
    }
    let mut seen = BTreeSet::new();
    for record in records {
        ensure_safe_component(&record.id)?;
        if !seen.insert((&record.collection, &record.id)) {
            return Err(conflict("duplicate publication record"));
        }
        validate_value(&record.collection, &record.id, &record.value)?;
        if let Some(previous) = &record.previous {
            validate_value(&record.collection, &record.id, previous)?;
            if record.collection != "judgments" && previous != &record.value {
                return Err(conflict("immutable publication record replacement"));
            }
        }
    }
    for record in records
        .iter()
        .filter(|record| record.collection == "object_judgment_inputs")
    {
        let input: ObjectJudgmentInput =
            serde_json::from_value(record.value.clone()).map_err(encoding)?;
        let counterpart = records
            .iter()
            .find(|candidate| candidate.collection == "judgments" && candidate.id == record.id)
            .ok_or_else(|| conflict("object judgment counterpart outside publication"))?;
        input.validate_judgment(
            &serde_json::from_value(counterpart.value.clone()).map_err(encoding)?,
        )?;
        if record.previous.is_some() && counterpart.previous.is_none() {
            return Err(conflict("object judgment preimage counterpart missing"));
        }
        if let Some(previous) = &counterpart.previous {
            input
                .validate_judgment(&serde_json::from_value(previous.clone()).map_err(encoding)?)?;
        }
    }
    // A receipt cannot commit independently of the records it promises to replay.
    for record in records
        .iter()
        .filter(|record| record.collection == "publication_receipts")
    {
        let receipt: PublicationReceipt =
            serde_json::from_value(record.value.clone()).map_err(encoding)?;
        let required = |collection: &str, id: &str| {
            records
                .iter()
                .find(|record| record.collection == collection && record.id == id)
                .map(|record| &record.value)
                .ok_or_else(|| conflict("receipt references a record outside its publication"))
        };
        let event: Event =
            serde_json::from_value(required("events", receipt.outcome.event.as_str())?.clone())
                .map_err(encoding)?;
        if event.actor != receipt.request.author {
            return Err(conflict("receipt actor mismatch"));
        }
        if let Some(id) = &receipt.outcome.object {
            let object: Object = serde_json::from_value(required("objects", id.as_str())?.clone())
                .map_err(encoding)?;
            if object.author != receipt.request.author
                || event.target != babble_state::EventTarget::Object(id.clone())
                || !matches!(
                    event.kind,
                    babble_state::EventKind::ObjectPublished
                        | babble_state::EventKind::ObjectForked
                        | babble_state::EventKind::ObjectRemixed
                )
            {
                return Err(conflict("receipt object mismatch"));
            }
        } else if receipt.is_event_only() {
            // A consent receipt promises exactly one event, with no hidden writes.
            if records.len() != 2 {
                return Err(conflict(
                    "consent publication requires exactly event and receipt",
                ));
            }
            validate_consent_receipt(&receipt, &event)?;
        } else if event.target != babble_state::EventTarget::Edge(receipt.outcome.edges[0].clone())
            || event.kind != babble_state::EventKind::EdgePublished
        {
            return Err(conflict("receipt edge event mismatch"));
        }
        let batch_edges: BTreeSet<_> = records
            .iter()
            .filter(|record| record.collection == "edges")
            .map(|record| record.id.as_str())
            .collect();
        let receipt_edges: BTreeSet<_> =
            receipt.outcome.edges.iter().map(|id| id.as_str()).collect();
        if batch_edges != receipt_edges {
            return Err(conflict("receipt does not describe all publication edges"));
        }
        for id in &receipt.outcome.edges {
            let edge: Edge = serde_json::from_value(required("edges", id.as_str())?.clone())
                .map_err(encoding)?;
            if edge.author.as_ref() != Some(&receipt.request.author) {
                return Err(conflict("receipt edge author mismatch"));
            }
            if receipt
                .outcome
                .object
                .as_ref()
                .is_some_and(|object| &edge.source != object)
            {
                return Err(conflict("receipt edge source mismatch"));
            }
        }
    }
    super::invocations::validate_invocation_batch(records)?;
    Ok(())
}

fn validate_consent_receipt(receipt: &PublicationReceipt, event: &Event) -> Result<()> {
    if event.actor != receipt.request.author || event.id != receipt.outcome.event {
        return Err(conflict("consent receipt event mismatch"));
    }
    super::receipts::validate_consent_event(event)
}

pub(super) fn validate_value(collection: &str, id: &str, value: &Value) -> Result<()> {
    let actual = match collection {
        "objects" => {
            let v: Object = serde_json::from_value(value.clone()).map_err(encoding)?;
            v.id.validate()?;
            v.id.to_string()
        }
        "edges" => {
            let v: Edge = serde_json::from_value(value.clone()).map_err(encoding)?;
            v.id.validate()?;
            v.id.to_string()
        }
        "events" => {
            let v: Event = serde_json::from_value(value.clone()).map_err(encoding)?;
            v.id.validate()?;
            v.id.to_string()
        }
        "judgments" => {
            let v: Judgment = serde_json::from_value(value.clone()).map_err(encoding)?;
            v.id.validate()?;
            if !v.confidence.is_finite() || !(0.0..=1.0).contains(&v.confidence) {
                return Err(conflict("invalid recovered judgment confidence"));
            }
            v.id.to_string()
        }
        "object_judgment_inputs" => {
            let input: ObjectJudgmentInput =
                serde_json::from_value(value.clone()).map_err(encoding)?;
            input.validate()?;
            input.judgment_id.to_string()
        }
        "publication_receipts" => {
            let receipt: PublicationReceipt =
                serde_json::from_value(value.clone()).map_err(encoding)?;
            receipt.validate()?;
            receipt.request.id.to_string()
        }
        "invocations" => {
            let phase: babble_capabilities::invocation::InvocationRecord =
                serde_json::from_value(value.clone()).map_err(encoding)?;
            phase.validate()?;
            phase.storage_id()?
        }
        _ => return Err(conflict("invalid publication collection")),
    };
    if actual != id {
        return Err(conflict("publication record ID mismatch"));
    }
    Ok(())
}

pub(crate) fn read_value(path: &Path) -> Result<Option<Value>> {
    #[cfg(test)]
    RECORD_READS.with(|reads| {
        if let Some(reads) = reads.borrow_mut().as_mut() {
            reads.push(path.to_path_buf());
        }
    });
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if !metadata.is_file() || metadata.len() > MAX_BYTES {
                return Err(conflict("invalid publication destination"));
            }
            serde_json::from_slice(&read_bounded(path)?)
                .map(Some)
                .map_err(encoding)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(error)),
    }
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(io_error)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(conflict("publication byte limit exceeded"));
    }
    Ok(bytes)
}

fn sync_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::create(path).map_err(io_error)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(io_error)?;
    }
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}

fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)
        .map_err(io_error)?
        .sync_all()
        .map_err(io_error)
}
fn conflict(message: impl Into<String>) -> CoreError {
    CoreError::Conflict(message.into())
}
fn io_error(error: std::io::Error) -> CoreError {
    conflict(format!("publication I/O: {error}"))
}
fn encoding(error: serde_json::Error) -> CoreError {
    CoreError::Canonical(format!("publication encoding: {error}"))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod consent_tests;

#[cfg(test)]
mod invocation_tests;

#[cfg(test)]
mod invocation_integrity_tests;
#[cfg(test)]
mod browser_invocation_tests;

#[cfg(test)]
mod object_judgment_tests;

#[cfg(test)]
mod judgment_index_tests;

#[cfg(test)]
thread_local! {
    pub(crate) static RECORD_READS: std::cell::RefCell<Option<Vec<PathBuf>>> = const { std::cell::RefCell::new(None) };
}

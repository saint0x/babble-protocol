//! Invocation phases share the publication journal and its OS writer lock.
//! There is no separate database commit or mutable current-state projection.
use super::publication::{Record, read_value, validate_records, validate_value};
use super::*;
use babel_capabilities::invocation::{
    InvocationAction, InvocationContext, InvocationIntent, InvocationOutcome, InvocationRecord,
    InvocationState, MAX_INVOCATION_REVISION,
};
use std::collections::BTreeSet;

impl PublicationBatch {
    /// Stage one legal next phase. Commit compares its immutable predecessor
    /// with the durable head under the writer lock. This is not a reservation.
    /// CompletePublication requires its receipt and effects in this same batch.
    /// The receipt request fingerprint must equal expected.intent().fingerprint().
    /// Publication/dispatch consumption is also the quota debit; commit checks
    /// aggregate usage across originating logins under the same writer lock.
    pub fn invocation_transition(
        &mut self,
        expected: &InvocationRecord,
        action: InvocationAction,
        context: &InvocationContext,
        now: Timestamp,
    ) -> Result<InvocationRecord> {
        let next = expected.transition(action, context, now)?;
        self.invocation_phase(&next)?;
        Ok(next)
    }

    fn invocation_phase(&mut self, phase: &InvocationRecord) -> Result<()> {
        if self
            .records
            .iter()
            .any(|record| record.collection == "invocations")
        {
            return Err(conflict("only one invocation transition per publication"));
        }
        phase.validate()?;
        self.push("invocations", &phase.storage_id()?, phase)
    }
}

impl FileStore {
    /// Exact retries return the original challenge/outcome, even after expiry.
    /// Authentication must precede lookup. Preserve original ingress timestamps
    /// on retry; a changed deadline is a changed intent and conflicts.
    pub fn prepare_invocation(
        &self,
        intent: InvocationIntent,
        now: Timestamp,
    ) -> std::result::Result<InvocationRecord, PublicationError> {
        intent.validate()?;
        let _guard = self.publication_guard()?;
        if let Some(existing) = self.invocation_unlocked(&intent.key()?)? {
            if existing.intent().fingerprint()? != intent.fingerprint()? {
                return Err(conflict("invocation key reused with changed intent").into());
            }
            return Ok(existing);
        }
        if babel_capabilities::invocation::is_one_use_invocation(intent.capability.as_str()) {
            let pending = self
                .list_invocations_unlocked()?
                .into_iter()
                .filter(|record| {
                    record.intent().context.actor == intent.context.actor
                        && babel_capabilities::invocation::is_one_use_invocation(
                            record.intent().capability.as_str(),
                        )
                        && matches!(
                            record.state(),
                            InvocationState::Pending | InvocationState::Approved
                        )
                        && record.intent().deadline > now
                })
                .count();
            if pending >= 32 {
                return Err(conflict("social invocation pending limit exceeded").into());
            }
        }
        let pending = InvocationRecord::pending(intent, now)?;
        let mut batch = PublicationBatch::new();
        batch.invocation_phase(&pending)?;
        self.commit_publication_locked(batch, &mut |_| Ok(()))?;
        Ok(pending)
    }

    /// Private history access; callers must authorize actor/login before exposing
    /// results, and must separately suppress bridge delivery to lost contexts.
    pub fn invocation(&self, key: &Hash) -> Result<Option<InvocationRecord>> {
        let _guard = self.publication_guard()?;
        self.invocation_unlocked(key)
    }

    pub fn list_invocations(&self) -> Result<Vec<InvocationRecord>> {
        let _guard = self.publication_guard()?;
        self.list_invocations_unlocked()
    }

    fn list_invocations_unlocked(&self) -> Result<Vec<InvocationRecord>> {
        let mut keys = BTreeSet::new();
        for entry in fs::read_dir(self.root.join("invocations")).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "publication-tmp") {
                continue;
            }
            if path.extension().is_none_or(|ext| ext != "json") {
                return Err(conflict("unrecognized invocation record file"));
            }
            let value = read_value(&path)?.ok_or_else(|| conflict("missing invocation record"))?;
            let phase = decode(&value)?;
            phase.validate()?;
            if path.file_stem().and_then(|s| s.to_str()) != Some(phase.storage_id()?.as_str()) {
                return Err(conflict("invocation record filename mismatch"));
            }
            keys.insert(phase.intent().key()?);
        }
        keys.into_iter()
            .map(|key| {
                self.invocation_unlocked(&key)?
                    .ok_or_else(|| conflict("missing invocation history"))
            })
            .collect()
    }

    pub fn transition_invocation(
        &self,
        expected: &InvocationRecord,
        action: InvocationAction,
        context: &InvocationContext,
        now: Timestamp,
    ) -> std::result::Result<InvocationRecord, PublicationError> {
        let mut batch = PublicationBatch::new();
        let next = batch.invocation_transition(expected, action, context, now)?;
        self.commit_publication(batch)?;
        Ok(next)
    }

    fn invocation_unlocked(&self, key: &Hash) -> Result<Option<InvocationRecord>> {
        key.validate()?;
        ensure_safe_component(key.as_str())?;
        let mut head: Option<InvocationRecord> = None;
        let mut gap = false;
        for revision in 0..=MAX_INVOCATION_REVISION {
            let id = format!("{key}-{revision}");
            let Some(value) = read_value(&self.path("invocations", &id)?)? else {
                gap = true;
                continue;
            };
            if gap {
                return Err(conflict("invocation history has a missing phase"));
            }
            validate_value("invocations", &id, &value)?;
            let phase = decode(&value)?;
            if let Some(previous) = &head {
                previous.validate_successor(&phase)?;
            }
            self.validate_invocation_outcome(&phase)?;
            head = Some(phase);
        }
        Ok(head)
    }

    pub(super) fn validate_invocation_preimages(
        &self,
        records: &[Record],
        recovering: bool,
    ) -> Result<()> {
        for record in records.iter().filter(|r| r.collection == "invocations") {
            let next = decode(&record.value)?;
            // Recovery may observe the new head already installed. Read only
            // earlier immutable phases here; the effect files may be partial.
            let mut previous: Option<InvocationRecord> = None;
            for revision in 0..next.revision() {
                let id = format!("{}-{revision}", next.intent().key()?);
                let value = read_value(&self.path("invocations", &id)?)?
                    .ok_or_else(|| conflict("invocation predecessor is missing"))?;
                validate_value("invocations", &id, &value)?;
                let phase = decode(&value)?;
                if let Some(prior) = &previous {
                    prior.validate_successor(&phase)?;
                }
                previous = Some(phase);
            }
            if let Some(previous) = &previous {
                previous.validate_successor(&next)?;
            }
            for revision in next.revision()..=MAX_INVOCATION_REVISION {
                let id = format!("{}-{revision}", next.intent().key()?);
                if let Some(current) = read_value(&self.path("invocations", &id)?)? {
                    if !recovering || revision != next.revision() || current != record.value {
                        return Err(conflict("stale invocation decision or conflicting history"));
                    }
                }
            }
            if record.previous.is_some() {
                return Err(conflict(
                    "invocation phases cannot replace existing records",
                ));
            }
            if !recovering && (publication_receipt_id(&next).is_some()
                || matches!(next.action(), Some(InvocationAction::Dispatch { .. }))) {
                self.check_social_invocation_quota_unlocked(next.intent(), next.updated_at())?;
            }
        }
        Ok(())
    }

    /// Advisory preflight. The same check runs under the commit writer lock.
    pub fn check_social_invocation_quota(
        &self,
        intent: &InvocationIntent,
        now: Timestamp,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        self.check_social_invocation_quota_unlocked(intent, now)
    }

    fn check_social_invocation_quota_unlocked(
        &self,
        intent: &InvocationIntent,
        now: Timestamp,
    ) -> Result<()> {
        if !babel_capabilities::invocation::is_one_use_invocation(intent.capability.as_str()) {
            return Ok(());
        }
        let definition = babel_capabilities::CapabilityBroker::babel_default()
            .definitions()
            .into_iter()
            .find(|d| d.id == intent.capability && d.version == intent.capability_version)
            .ok_or_else(|| conflict("social invocation capability unavailable"))?;
        let mut calls = 1u64;
        let mut bytes = intent.payload.canonical_bytes()?.len() as u64;
        for record in self.list_invocations_unlocked()? {
            let prior = record.intent();
            if prior.context.actor != intent.context.actor
                || prior.context.object_id != intent.context.object_id
                || prior.capability != intent.capability
                || !record
                    .consumed_at()
                    .is_some_and(|at| (now.0 - at.0).whole_nanoseconds() < 60_000_000_000)
            {
                continue;
            }
            calls = calls
                .checked_add(1)
                .ok_or_else(|| conflict("social quota overflow"))?;
            bytes = bytes
                .checked_add(prior.payload.canonical_bytes()?.len() as u64)
                .ok_or_else(|| conflict("social quota overflow"))?;
        }
        if calls > u64::from(definition.quota.calls_per_minute)
            || bytes > definition.quota.bytes_per_minute
        {
            return Err(conflict("social invocation aggregate quota exceeded"));
        }
        Ok(())
    }

    fn validate_invocation_outcome(&self, phase: &InvocationRecord) -> Result<()> {
        let Some(receipt_id) = publication_receipt_id(phase) else {
            return Ok(());
        };
        let value = read_value(&self.path("publication_receipts", receipt_id.as_str())?)?
            .ok_or_else(|| conflict("invocation publication receipt missing"))?;
        let receipt: PublicationReceipt =
            serde_json::from_value(value.clone()).map_err(encoding)?;
        validate_receipt_binding(phase, &receipt)?;
        let mut records = vec![Record {
            collection: "publication_receipts".into(),
            id: receipt_id.to_string(),
            value,
            previous: None,
        }];
        let mut required = vec![("events", receipt.outcome.event.to_string())];
        if let Some(object) = &receipt.outcome.object {
            required.push(("objects", object.to_string()));
        }
        required.extend(
            receipt
                .outcome
                .edges
                .iter()
                .map(|id| ("edges", id.to_string())),
        );
        for (collection, id) in required {
            let value = read_value(&self.path(collection, &id)?)?
                .ok_or_else(|| conflict("invocation publication effect missing"))?;
            records.push(Record {
                collection: collection.into(),
                id,
                value,
                previous: None,
            });
        }
        validate_records(&records)?;
        self.verify_recovered_signatures(&records)
    }
}

pub(super) fn validate_invocation_batch(records: &[Record]) -> Result<()> {
    let phases: Vec<_> = records
        .iter()
        .filter(|r| r.collection == "invocations")
        .collect();
    if phases.len() > 1 {
        return Err(conflict("multiple invocation phases in one publication"));
    }
    let Some(record) = phases.first() else {
        return Ok(());
    };
    let phase = decode(&record.value)?;
    let Some(receipt_id) = publication_receipt_id(&phase) else {
        if records.len() != 1 {
            return Err(conflict("unconsumed invocation cannot publish effects"));
        }
        return Ok(());
    };
    let receipts: Vec<_> = records
        .iter()
        .filter(|r| r.collection == "publication_receipts")
        .collect();
    if receipts.len() != 1 || receipts[0].id != receipt_id.as_str() {
        return Err(conflict(
            "invocation completion requires its receipt in the same publication",
        ));
    }
    let receipt: PublicationReceipt =
        serde_json::from_value(receipts[0].value.clone()).map_err(encoding)?;
    validate_receipt_binding(&phase, &receipt)?;
    for record in records {
        match record.collection.as_str() {
            "invocations" | "publication_receipts" | "objects" | "edges" | "events" => (),
            "judgments" => {
                if !records.iter().any(|input| {
                    input.collection == "object_judgment_inputs" && input.id == record.id
                }) {
                    return Err(conflict(
                        "invocation publication contains an unpaired judgment",
                    ));
                }
            }
            "object_judgment_inputs" => {
                let input: ObjectJudgmentInput =
                    serde_json::from_value(record.value.clone()).map_err(encoding)?;
                if receipt.outcome.object.as_ref() != Some(&input.object_id) {
                    return Err(conflict(
                        "invocation judgment belongs to an unreceipted object",
                    ));
                }
            }
            _ => {
                return Err(conflict(
                    "invocation publication contains an unrelated collection",
                ));
            }
        }
    }
    for record in records.iter().filter(|r| r.collection == "objects") {
        if receipt.outcome.object.as_ref().map(ObjectId::as_str) != Some(record.id.as_str()) {
            return Err(conflict(
                "invocation publication contains an unreceipted object",
            ));
        }
    }
    for record in records.iter().filter(|r| r.collection == "events") {
        let event: Event = serde_json::from_value(record.value.clone()).map_err(encoding)?;
        let promised = event.id == receipt.outcome.event
            || matches!(
                &event.target, babel_state::EventTarget::Edge(id) if receipt.outcome.edges.contains(id)
                    && event.kind == babel_state::EventKind::EdgePublished
            );
        if !promised || event.actor != phase.intent().context.actor {
            return Err(conflict(
                "invocation publication contains an unreceipted event",
            ));
        }
    }
    Ok(())
}

fn publication_receipt_id(phase: &InvocationRecord) -> Option<&Hash> {
    match phase.state() {
        InvocationState::Completed {
            outcome: InvocationOutcome::Publication { receipt },
        } => Some(receipt),
        _ => None,
    }
}

fn validate_receipt_binding(phase: &InvocationRecord, receipt: &PublicationReceipt) -> Result<()> {
    receipt.validate()?;
    if Some(&receipt.request.id) != publication_receipt_id(phase)
        || receipt.request.author != phase.intent().context.actor
        || receipt.request.fingerprint != phase.intent().fingerprint()?
        || receipt.is_event_only()
    {
        return Err(conflict(
            "invocation publication receipt actor/intent/effect mismatch",
        ));
    }
    Ok(())
}

fn decode(value: &Value) -> Result<InvocationRecord> {
    serde_json::from_value(value.clone()).map_err(encoding)
}
fn conflict(message: &str) -> CoreError {
    CoreError::Conflict(message.into())
}
fn encoding(error: serde_json::Error) -> CoreError {
    CoreError::Canonical(format!("invocation encoding: {error}"))
}
fn io_error(error: std::io::Error) -> CoreError {
    CoreError::Conflict(format!("invocation I/O: {error}"))
}

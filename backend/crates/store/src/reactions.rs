//! Durable public reaction registers, signed retry receipts, and constant-size summaries.
//! One node writer per root, as for publication/following. SQL CAS also serializes clones.
//! Histories and receipt keys are retained indefinitely; deleting them breaks retry safety.
use crate::FileStore;
use babble_graph::{
    Appreciation, Engagement, REACTION_MAX_REVISION, ReactionAction, ReactionReceipt,
    ReactionRecord, ReactionRequest, ReactionState, ReactionSummary, ReactionValue, Stance,
};
use babble_identity::Identity;
use babble_types::{Canonical, Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

fn db_error(_: rusqlite::Error) -> Error {
    Error::StorageUnavailable(
        "public reactions database failure; retry with the same request key".into(),
    )
}
fn encoding(_: serde_json::Error) -> Error {
    Error::StorageUnavailable("invalid public reaction record; restore the store".into())
}
fn encode(value: &impl serde::Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(encoding)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(encoding)
}

impl FileStore {
    fn reactions_connection(&self) -> Result<Connection> {
        let connection = Connection::open_with_flags(
            self.root().join("public_reactions/reactions.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(db_error)?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(db_error)?;
        connection
            .execute_batch(
                "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048;",
            )
            .map_err(db_error)?;
        Ok(connection)
    }

    pub(crate) fn initialize_reactions(&self) -> Result<()> {
        self.initialize_reactions_with_hook(|_| Ok(()))
    }

    fn initialize_reactions_with_hook(
        &self,
        mut checkpoint: impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let directory = self.root().join("public_reactions");
        let io_error = |_| {
            Error::StorageUnavailable(
                "initialize reactions database; retry opening the store".into(),
            )
        };
        if !directory.try_exists().map_err(io_error)? {
            // Only this unpublished staging directory can be discarded after interruption.
            // An established final directory with a missing database remains a recovery error.
            let staged = self.root().join(".public-reactions-install");
            if staged.try_exists().map_err(io_error)? {
                std::fs::remove_dir_all(&staged).map_err(io_error)?;
            }
            std::fs::create_dir(&staged).map_err(io_error)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o700))
                    .map_err(io_error)?;
            }
            checkpoint("directory")?;
            let db = Connection::open(staged.join("reactions.sqlite3")).map_err(db_error)?;
            checkpoint("database")?;
            db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;
                CREATE TABLE actions (
                    author TEXT NOT NULL, object TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                    id TEXT NOT NULL UNIQUE, record TEXT NOT NULL,
                    receipt_id TEXT NOT NULL REFERENCES receipts(id) DEFERRABLE INITIALLY DEFERRED,
                    PRIMARY KEY(author,object,revision));
                CREATE TABLE pairs (
                    author TEXT NOT NULL, object TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                    value TEXT NOT NULL, id TEXT NOT NULL REFERENCES actions(id), PRIMARY KEY(author,object));
                CREATE INDEX reactions_by_object ON pairs(object,author);
                CREATE TABLE summaries (object TEXT PRIMARY KEY, record TEXT NOT NULL);
                CREATE TABLE receipts (
                    author TEXT NOT NULL, object TEXT NOT NULL, key TEXT NOT NULL, record TEXT NOT NULL,
                    sequence INTEGER NOT NULL CHECK(sequence>0), id TEXT NOT NULL UNIQUE,
                    PRIMARY KEY(author,key), UNIQUE(author,sequence));
                CREATE INDEX receipt_pairs ON receipts(author,object,sequence);
                CREATE TABLE receipt_heads (
                    author TEXT PRIMARY KEY, sequence INTEGER NOT NULL CHECK(sequence>0),
                    id TEXT NOT NULL REFERENCES receipts(id));
                PRAGMA user_version=1; COMMIT;").map_err(db_error)?;
            checkpoint("schema")?;
            db.close().map_err(|(_, e)| db_error(e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    staged.join("reactions.sqlite3"),
                    std::fs::Permissions::from_mode(0o600),
                )
                .map_err(io_error)?;
            }
            std::fs::File::open(staged.join("reactions.sqlite3"))
                .and_then(|f| f.sync_all())
                .map_err(io_error)?;
            std::fs::File::open(&staged)
                .and_then(|f| f.sync_all())
                .map_err(io_error)?;
            checkpoint("synced")?;
            std::fs::rename(&staged, &directory).map_err(io_error)?;
            std::fs::File::open(self.root())
                .and_then(|f| f.sync_all())
                .map_err(io_error)?;
        }
        // Never silently replace a missing database or schema with an empty history.
        let db = self.reactions_connection()?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_error)?;
        if version != 1 {
            return Err(Error::StorageUnavailable(
                "unsupported reactions database version".into(),
            ));
        }
        Ok(())
    }

    pub fn reaction_state(&self, author: &IdentityId, object: &ObjectId) -> Result<ReactionState> {
        let _guard = self.publication_guard()?;
        pair(&self.reactions_connection()?, author, object).map(|(state, _)| state)
    }

    pub fn reaction_record(
        &self,
        author: &IdentityId,
        object: &ObjectId,
    ) -> Result<ReactionRecord> {
        let _guard = self.publication_guard()?;
        let mut db = self.reactions_connection()?;
        let tx = db.transaction().map_err(db_error)?;
        let (state, id) = pair(&tx, author, object)?;
        let action = id.map(|id| action_by_id(&tx, &id)).transpose()?;
        if action.as_ref().is_some_and(|a| a.payload.state != state) {
            return Err(Error::Signature);
        }
        Ok(ReactionRecord { state, action })
    }

    /// Indexed point lookup. Verification reconstructs summaries from signed history at reopen.
    pub fn reaction_summary(&self, object: &ObjectId) -> Result<ReactionSummary> {
        let _guard = self.publication_guard()?;
        summary(&self.reactions_connection()?, object)
    }

    pub fn commit_reaction(
        &self,
        request: &ReactionRequest,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
        sign: impl FnOnce(
            &ReactionState,
            Option<Hash>,
            bool,
            u64,
            Option<Hash>,
        ) -> Result<(Option<ReactionAction>, ReactionReceipt)>,
    ) -> Result<ReactionState> {
        request.validate()?;
        let _guard = self.publication_guard()?;
        let mut db = self.reactions_connection()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous: Option<(String, String, u64, String)> = tx
            .query_row(
                "SELECT record,id,sequence,object FROM receipts WHERE author=?1 AND key=?2",
                params![request.author_id.as_str(), request.idempotency_key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()
            .map_err(db_error)?;
        if let Some((previous, stored_id, sequence, object)) = previous {
            let receipt: ReactionReceipt = decode(&previous)?;
            receipt.verify(&identity_at(
                &receipt.payload.request.author_id,
                receipt.payload.created_at,
            )?)?;
            if receipt.id()?.as_str() != stored_id
                || receipt.payload.sequence != sequence
                || receipt.payload.request.object_id.as_str() != object
                || receipt.payload.request.author_id != request.author_id
                || receipt.payload.request.idempotency_key != request.idempotency_key
            {
                return Err(Error::Signature);
            }
            verify_receipt_state(&tx, &receipt, &identity_at)?;
            if receipt.payload.request != *request {
                return Err(Error::Conflict(
                    "idempotency key already used for another reaction intent".into(),
                ));
            }
            return Ok(receipt.payload.state);
        }
        let (mut state, previous_id) = pair(&tx, &request.author_id, &request.object_id)?;
        if state.revision != request.expected_revision {
            return Err(Error::Conflict(
                "reaction revision changed; refresh reaction state".into(),
            ));
        }
        let old_value = state.value.clone();
        let changed = state.value != request.value;
        if changed {
            state.value = request.value.clone();
            state.revision = next(state.revision)?;
        }
        let head: Option<(u64, String)> = tx
            .query_row(
                "SELECT sequence,id FROM receipt_heads WHERE author=?1",
                [request.author_id.as_str()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db_error)?;
        let sequence = next(head.as_ref().map_or(0, |(seq, _)| *seq))?;
        let receipt_previous_id = head.map(|(_, id)| Hash::new_unchecked(id));
        let (action, receipt) = sign(
            &state,
            previous_id.clone(),
            changed,
            sequence,
            receipt_previous_id.clone(),
        )?;
        if receipt.payload.request != *request
            || receipt.payload.state != state
            || receipt.payload.sequence != sequence
            || receipt.payload.previous_id != receipt_previous_id
            || action.is_some() != changed
        {
            return Err(Error::Signature);
        }
        if let Some(id) = &receipt_previous_id {
            let predecessor = receipt_by_id(&tx, id)?;
            if receipt.payload.created_at < predecessor.payload.created_at {
                return Err(Error::StorageUnavailable(
                    "clock precedes the last reaction request; retry with the same key".into(),
                ));
            }
        }
        let receipt_id = receipt.id()?;
        if let Some(action) = action {
            if action.payload.state != state
                || action.payload.previous_id != previous_id
                || action.payload.request_id != request.canonical_hash()?
                || action.payload.created_at != receipt.payload.created_at
            {
                return Err(Error::Signature);
            }
            if let Some(id) = &previous_id
                && action.payload.created_at < action_by_id(&tx, id)?.payload.created_at
            {
                return Err(Error::StorageUnavailable(
                    "clock precedes the last reaction action; retry with the same key".into(),
                ));
            }
            let mut totals = summary(&tx, &state.object_id)?;
            adjust(&mut totals, &old_value, false)?;
            adjust(&mut totals, &state.value, true)?;
            tx.execute("INSERT INTO actions(author,object,revision,id,record,receipt_id) VALUES (?1,?2,?3,?4,?5,?6)",
                params![state.author_id.as_str(), state.object_id.as_str(), state.revision, action.id.as_str(), encode(&action)?, receipt_id.as_str()]).map_err(db_error)?;
            tx.execute("INSERT INTO pairs(author,object,revision,value,id) VALUES (?1,?2,?3,?4,?5)
                ON CONFLICT(author,object) DO UPDATE SET revision=excluded.revision,value=excluded.value,id=excluded.id",
                params![state.author_id.as_str(), state.object_id.as_str(), state.revision, encode(&state.value)?, action.id.as_str()]).map_err(db_error)?;
            tx.execute("INSERT INTO summaries(object,record) VALUES (?1,?2) ON CONFLICT(object) DO UPDATE SET record=excluded.record",
                params![state.object_id.as_str(), encode(&totals)?]).map_err(db_error)?;
        }
        tx.execute(
            "INSERT INTO receipts(author,key,record,sequence,id,object) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                request.author_id.as_str(),
                request.idempotency_key,
                encode(&receipt)?,
                sequence,
                receipt_id.as_str(),
                request.object_id.as_str()
            ],
        )
        .map_err(db_error)?;
        tx.execute(
            "INSERT INTO receipt_heads(author,sequence,id) VALUES (?1,?2,?3)
            ON CONFLICT(author) DO UPDATE SET sequence=excluded.sequence,id=excluded.id",
            params![request.author_id.as_str(), sequence, receipt_id.as_str()],
        )
        .map_err(db_error)?;
        tx.commit().map_err(db_error)?;
        Ok(state)
    }

    /// Verifies historical signing keys, complete action/receipt chains and all projections.
    /// Streams rows using bounded SQLite cache; never loads the history into node memory.
    pub fn verify_reaction_records(
        &self,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
        object_at: impl Fn(&ObjectId, Timestamp) -> Result<()>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let mut db = self.reactions_connection()?;
        let tx = db.transaction().map_err(db_error)?;
        let broken: bool = tx
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if broken {
            return Err(Error::Signature);
        }
        let mut statement = tx.prepare("SELECT author,object,revision,id,record,receipt_id FROM actions ORDER BY author,object,revision").map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous: Option<ReactionAction> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let action: ReactionAction = decode(&row.get::<_, String>(4).map_err(db_error)?)?;
            let state = &action.payload.state;
            if row.get::<_, String>(0).map_err(db_error)? != state.author_id.as_str()
                || row.get::<_, String>(1).map_err(db_error)? != state.object_id.as_str()
                || row.get::<_, u64>(2).map_err(db_error)? != state.revision
                || row.get::<_, String>(3).map_err(db_error)? != action.id.as_str()
            {
                return Err(Error::Signature);
            }
            action.verify(&identity_at(&state.author_id, action.payload.created_at)?)?;
            object_at(&state.object_id, action.payload.created_at)?;
            let receipt_id = Hash::new_unchecked(row.get::<_, String>(5).map_err(db_error)?);
            let receipt = receipt_by_id(&tx, &receipt_id)?;
            if receipt.id()? != receipt_id
                || receipt.payload.state != *state
                || receipt.payload.request.canonical_hash()? != action.payload.request_id
                || receipt.payload.created_at != action.payload.created_at
                || receipt.payload.request.expected_revision.checked_add(1) != Some(state.revision)
            {
                return Err(Error::Signature);
            }
            let predecessor = previous.as_ref().filter(|p| {
                p.payload.state.author_id == state.author_id
                    && p.payload.state.object_id == state.object_id
            });
            match predecessor {
                Some(p)
                    if state.revision == p.payload.state.revision + 1
                        && state.value != p.payload.state.value
                        && action.payload.previous_id.as_ref() == Some(&p.id)
                        && action.payload.created_at >= p.payload.created_at => {}
                None if state.revision == 1
                    && !state.value.is_empty()
                    && action.payload.previous_id.is_none() => {}
                _ => return Err(Error::Signature),
            }
            previous = Some(action);
        }
        // Equality on decoded values avoids accepting SQL NULL comparison semantics for malformed JSON.
        let mut statement = tx
            .prepare("SELECT author,object,revision,value,id FROM pairs ORDER BY object,author")
            .map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut totals: Option<ReactionSummary> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let author = IdentityId::new_unchecked(row.get::<_, String>(0).map_err(db_error)?);
            let object = ObjectId::new_unchecked(row.get::<_, String>(1).map_err(db_error)?);
            let state = ReactionState {
                author_id: author,
                object_id: object.clone(),
                revision: row.get(2).map_err(db_error)?,
                value: decode(&row.get::<_, String>(3).map_err(db_error)?)?,
            };
            let action = action_by_id(
                &tx,
                &Hash::new_unchecked(row.get::<_, String>(4).map_err(db_error)?),
            )?;
            let latest: u64 = tx
                .query_row(
                    "SELECT max(revision) FROM actions WHERE author=?1 AND object=?2",
                    params![state.author_id.as_str(), object.as_str()],
                    |r| r.get(0),
                )
                .map_err(db_error)?;
            if action.payload.state != state || state.revision != latest {
                return Err(Error::Signature);
            }
            if totals.as_ref().is_some_and(|t| t.object_id != object) {
                verify_summary(&tx, totals.as_ref().expect("present"))?;
                totals = None;
            }
            adjust(
                totals.get_or_insert_with(|| ReactionSummary::empty(&object)),
                &state.value,
                true,
            )?;
        }
        if let Some(totals) = totals {
            verify_summary(&tx, &totals)?;
        }
        let missing: bool = tx.query_row("SELECT EXISTS(
            SELECT 1 FROM actions a WHERE NOT EXISTS(SELECT 1 FROM pairs p WHERE p.author=a.author AND p.object=a.object)
            UNION ALL SELECT 1 FROM summaries s WHERE NOT EXISTS(SELECT 1 FROM pairs p WHERE p.object=s.object))", [], |r| r.get(0)).map_err(db_error)?;
        if missing {
            return Err(Error::Signature);
        }
        let mut statement = tx.prepare("SELECT author,key,record,sequence,id,object FROM receipts ORDER BY author,sequence").map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous: Option<ReactionReceipt> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let receipt: ReactionReceipt = decode(&row.get::<_, String>(2).map_err(db_error)?)?;
            let payload = &receipt.payload;
            if row.get::<_, String>(0).map_err(db_error)? != payload.request.author_id.as_str()
                || row.get::<_, String>(1).map_err(db_error)? != payload.request.idempotency_key
                || row.get::<_, u64>(3).map_err(db_error)? != payload.sequence
                || row.get::<_, String>(4).map_err(db_error)? != receipt.id()?.as_str()
                || row.get::<_, String>(5).map_err(db_error)? != payload.request.object_id.as_str()
            {
                return Err(Error::Signature);
            }
            receipt.verify(&identity_at(
                &payload.request.author_id,
                payload.created_at,
            )?)?;
            object_at(&payload.request.object_id, payload.created_at)?;
            let predecessor = previous
                .as_ref()
                .filter(|p| p.payload.request.author_id == payload.request.author_id);
            match predecessor {
                Some(p)
                    if payload.sequence == p.payload.sequence + 1
                        && payload.previous_id.as_ref() == Some(&p.id()?)
                        && payload.created_at >= p.payload.created_at => {}
                None if payload.sequence == 1 && payload.previous_id.is_none() => {}
                _ => return Err(Error::Signature),
            }
            verify_receipt_state(&tx, &receipt, &identity_at)?;
            previous = Some(receipt);
        }
        let incomplete: bool = tx.query_row("SELECT EXISTS(
            SELECT 1 FROM receipt_heads h LEFT JOIN receipts r ON r.id=h.id WHERE r.id IS NULL
                OR r.author!=h.author OR r.sequence!=h.sequence OR h.sequence!=(SELECT count(*) FROM receipts WHERE author=h.author)
            UNION ALL SELECT 1 FROM receipts r WHERE NOT EXISTS(SELECT 1 FROM receipt_heads h WHERE h.author=r.author))", [], |r| r.get(0)).map_err(db_error)?;
        if incomplete {
            return Err(Error::Signature);
        }
        Ok(())
    }
}

fn next(value: u64) -> Result<u64> {
    value
        .checked_add(1)
        .filter(|v| *v <= REACTION_MAX_REVISION)
        .ok_or_else(|| Error::Conflict("reaction revision or receipt sequence exhausted".into()))
}
fn pair(
    db: &Connection,
    author: &IdentityId,
    object: &ObjectId,
) -> Result<(ReactionState, Option<Hash>)> {
    author.validate()?;
    object.validate()?;
    let row: Option<(String, u64, String)> = db
        .query_row(
            "SELECT value,revision,id FROM pairs WHERE author=?1 AND object=?2",
            params![author.as_str(), object.as_str()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    match row {
        Some((value, revision, id)) => {
            let state = ReactionState {
                author_id: author.clone(),
                object_id: object.clone(),
                value: decode(&value)?,
                revision,
            };
            state.validate()?;
            Ok((state, Some(Hash::new_unchecked(id))))
        }
        None => Ok((ReactionState::absent(author, object), None)),
    }
}
fn action_by_id(db: &Connection, id: &Hash) -> Result<ReactionAction> {
    let raw: String = db
        .query_row(
            "SELECT record FROM actions WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    let action: ReactionAction = decode(&raw)?;
    if action.id != *id {
        return Err(Error::Signature);
    }
    Ok(action)
}
fn receipt_by_id(db: &Connection, id: &Hash) -> Result<ReactionReceipt> {
    let raw: String = db
        .query_row(
            "SELECT record FROM receipts WHERE id=?1",
            [id.as_str()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    decode(&raw)
}
fn summary(db: &Connection, object: &ObjectId) -> Result<ReactionSummary> {
    object.validate()?;
    let raw: Option<String> = db
        .query_row(
            "SELECT record FROM summaries WHERE object=?1",
            [object.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    if raw.is_none() {
        let has_history: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pairs WHERE object=?1)",
                [object.as_str()],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        if has_history {
            return Err(Error::Signature);
        }
    }
    let result: ReactionSummary = raw
        .map(|r| decode(&r))
        .transpose()?
        .unwrap_or_else(|| ReactionSummary::empty(object));
    result.validate()?;
    if result.object_id != *object {
        return Err(Error::Signature);
    }
    Ok(result)
}
fn verify_summary(db: &Connection, expected: &ReactionSummary) -> Result<()> {
    // Require a persisted zero-count summary too: withdrawal history must not disappear.
    let raw: String = db
        .query_row(
            "SELECT record FROM summaries WHERE object=?1",
            [expected.object_id.as_str()],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if decode::<ReactionSummary>(&raw)? != *expected {
        return Err(Error::Signature);
    }
    Ok(())
}
fn adjust(summary: &mut ReactionSummary, value: &ReactionValue, add: bool) -> Result<()> {
    value.validate()?;
    for (count, present) in [
        (&mut summary.participants, !value.is_empty()),
        (
            &mut summary.likes,
            value.appreciation == Some(Appreciation::Like),
        ),
        (
            &mut summary.dislikes,
            value.appreciation == Some(Appreciation::Dislike),
        ),
        (
            &mut summary.engaging,
            value.engagement == Some(Engagement::Engaging),
        ),
        (
            &mut summary.not_engaging,
            value.engagement == Some(Engagement::NotEngaging),
        ),
        (&mut summary.support, value.stance == Some(Stance::Support)),
        (&mut summary.oppose, value.stance == Some(Stance::Oppose)),
        (
            &mut summary.uncertain,
            value.stance == Some(Stance::Uncertain),
        ),
        (&mut summary.certainty_responses, value.certainty.is_some()),
    ] {
        if present {
            *count = if add {
                count.checked_add(1)
            } else {
                count.checked_sub(1)
            }
            .filter(|n| *n <= REACTION_MAX_REVISION)
            .ok_or(Error::Signature)?;
        }
    }
    Ok(())
}

fn verify_receipt_state(
    db: &Connection,
    receipt: &ReactionReceipt,
    identity_at: &impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
) -> Result<()> {
    let payload = &receipt.payload;
    // Indexed predecessor by pair and signed sequence, not wall time. Verify it here too:
    // live receipt retries do not get the startup verifier's earlier-record guarantees.
    let previous: Option<(String, String, u64)> = db
        .query_row(
            "SELECT record,id,sequence FROM receipts
        WHERE author=?1 AND object=?2 AND sequence<?3 ORDER BY sequence DESC LIMIT 1",
            params![
                payload.state.author_id.as_str(),
                payload.state.object_id.as_str(),
                payload.sequence
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    let before = previous
        .map(|(raw, id, sequence)| {
            let previous: ReactionReceipt = decode(&raw)?;
            previous.verify(&identity_at(
                &previous.payload.request.author_id,
                previous.payload.created_at,
            )?)?;
            if previous.id()?.as_str() != id
                || previous.payload.sequence != sequence
                || previous.payload.state.author_id != payload.state.author_id
                || previous.payload.state.object_id != payload.state.object_id
                || previous.payload.created_at > payload.created_at
            {
                return Err(Error::Signature);
            }
            Ok(previous.payload.state)
        })
        .transpose()?
        .unwrap_or_else(|| {
            ReactionState::absent(&payload.state.author_id, &payload.state.object_id)
        });
    if before.revision != payload.request.expected_revision {
        return Err(Error::Signature);
    }
    let changed = before.value != payload.request.value;
    let expected = if changed {
        next(before.revision)?
    } else {
        before.revision
    };
    if payload.state.revision != expected {
        return Err(Error::Signature);
    }
    if payload.state.revision > 0 {
        let (raw, id, action_receipt): (String, String, String) = db
            .query_row(
                "SELECT record,id,receipt_id FROM actions WHERE author=?1 AND object=?2 AND revision=?3",
                params![
                    payload.state.author_id.as_str(),
                    payload.state.object_id.as_str(),
                    payload.state.revision
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_error)?;
        let action: ReactionAction = decode(&raw)?;
        action.verify(&identity_at(
            &action.payload.state.author_id,
            action.payload.created_at,
        )?)?;
        if action.id.as_str() != id
            || action.payload.state != payload.state
            || action.payload.created_at > payload.created_at
            || (changed && action.payload.request_id != payload.request.canonical_hash()?)
            || (changed
                && (action_receipt != receipt.id()?.as_str()
                    || action.payload.created_at != payload.created_at))
        {
            return Err(Error::Signature);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reactions_first_install_is_atomic_and_interrupted_staging_is_retryable() {
        for phase in ["directory", "database", "schema", "synced"] {
            let root = std::env::temp_dir().join(format!(
                "babble-reactions-install-{}-{}-{phase}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir(&root).unwrap();
            let store = FileStore {
                root: root.clone(),
                recovery_required: Default::default(),
            };
            assert!(
                store
                    .initialize_reactions_with_hook(|at| if at == phase {
                        Err(Error::StorageUnavailable("interrupted installation".into()))
                    } else {
                        Ok(())
                    })
                    .is_err()
            );
            assert!(!root.join("public_reactions").exists());
            assert!(root.join(".public-reactions-install").exists());
            let store = FileStore::open(&root).unwrap();
            store
                .verify_reaction_records(|_, _| Err(Error::Signature), |_, _| Err(Error::Signature))
                .unwrap();
            assert!(!root.join(".public-reactions-install").exists());
            std::fs::remove_file(root.join("public_reactions/reactions.sqlite3")).unwrap();
            assert!(FileStore::open(&root).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

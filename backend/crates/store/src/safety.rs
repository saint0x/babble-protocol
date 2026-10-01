//! SQLite owns the private action log, materialized pairs, and durable request receipts.
//! No connection/cache fields: each operation holds the existing root publication guard.
use crate::FileStore;
use babble_graph::{SafetyAction, SafetyReceipt, SafetyRequest, SafetyState};
use babble_identity::Identity;
use babble_types::{Canonical, Error, Hash, IdentityId, Result, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

fn db_error(_: rusqlite::Error) -> Error {
    Error::StorageUnavailable(
        "private safety database failure; retry with the same request key".into(),
    )
}
fn encoding(_: serde_json::Error) -> Error {
    Error::StorageUnavailable("invalid private safety record; restore the store".into())
}

impl FileStore {
    pub fn safety_blocked(&self, author: &IdentityId, target: &IdentityId) -> Result<bool> {
        author.validate()?;
        target.validate()?;
        let _guard = self.publication_guard()?;
        self.safety_connection()?.query_row("SELECT EXISTS(SELECT 1 FROM pairs WHERE blocked=1 AND ((author=?1 AND target=?2) OR (author=?2 AND target=?1)))",
            params![author.as_str(), target.as_str()], |r| r.get(0)).map_err(db_error)
    }
    fn safety_connection(&self) -> Result<Connection> {
        let path = self.root().join("private_safety").join("safety.sqlite3");
        let connection = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE)
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

    pub(crate) fn initialize_safety(&self) -> Result<()> {
        self.initialize_safety_with_hook(|_| Ok(()))
    }

    fn initialize_safety_with_hook(
        &self,
        mut checkpoint: impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let directory = self.root().join("private_safety");
        let io_error = |_| {
            Error::StorageUnavailable("initialize safety database; retry opening the store".into())
        };
        if !directory.try_exists().map_err(io_error)? {
            // Only this unpublished staging directory can be discarded after interruption.
            // An established final directory with a missing database remains a recovery error.
            let staged = self.root().join(".private-safety-install");
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
            let db = Connection::open(staged.join("safety.sqlite3")).map_err(db_error)?;
            checkpoint("database")?;
            db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;

            CREATE TABLE actions (
                author TEXT NOT NULL, target TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                id TEXT NOT NULL UNIQUE, record TEXT NOT NULL,
                receipt_id TEXT NOT NULL REFERENCES receipts(id) DEFERRABLE INITIALLY DEFERRED,
                PRIMARY KEY(author,target,revision));
            CREATE TABLE pairs (
                author TEXT NOT NULL, target TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                blocked INTEGER NOT NULL CHECK(blocked IN(0,1)), muted INTEGER NOT NULL CHECK(muted IN(0,1)), id TEXT NOT NULL REFERENCES actions(id),
                PRIMARY KEY(author,target));
            CREATE INDEX active_targets ON pairs(author,blocked,muted,target);
            CREATE TABLE versions (author TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version>0));
            CREATE TABLE receipts (
                author TEXT NOT NULL, target TEXT NOT NULL, key TEXT NOT NULL, record TEXT NOT NULL,
                sequence INTEGER NOT NULL CHECK(sequence>0), id TEXT NOT NULL UNIQUE,
                PRIMARY KEY(author,key), UNIQUE(author,sequence));
            CREATE TABLE receipt_heads (
                author TEXT PRIMARY KEY, sequence INTEGER NOT NULL CHECK(sequence>0),
                id TEXT NOT NULL REFERENCES receipts(id));
        CREATE INDEX receipt_pairs ON receipts(author,target,sequence);
 PRAGMA user_version=1; COMMIT;").map_err(db_error)?;
            checkpoint("schema")?;
            db.close().map_err(|(_, e)| db_error(e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(
                    staged.join("safety.sqlite3"),
                    std::fs::Permissions::from_mode(0o600),
                )
                .map_err(io_error)?;
            }
            std::fs::File::open(staged.join("safety.sqlite3"))
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
        let db = self.safety_connection()?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db_error)?;
        if version != 1 {
            return Err(Error::StorageUnavailable(
                "unsupported safety database version".into(),
            ));
        }
        Ok(())
    }

    pub fn safety_state(&self, author: &IdentityId, target: &IdentityId) -> Result<SafetyState> {
        let _guard = self.publication_guard()?;
        pair(&self.safety_connection()?, author, target).map(|(state, _)| state)
    }

    /// At most 1,000 active targets per author; snapshot reads share one transaction.
    pub fn safety_snapshot(&self, author: &IdentityId) -> Result<(u64, Vec<SafetyState>)> {
        let _guard = self.publication_guard()?;
        let mut db = self.safety_connection()?;
        let tx = db.transaction().map_err(db_error)?;
        let version = tx
            .query_row(
                "SELECT version FROM versions WHERE author=?1",
                [author.as_str()],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?
            .unwrap_or(0);
        let mut statement = tx.prepare("SELECT target FROM pairs WHERE author=?1 AND (blocked=1 OR muted=1) ORDER BY target LIMIT 1001").map_err(db_error)?;
        let targets = statement
            .query_map([author.as_str()], |row| row.get::<_, String>(0))
            .map_err(db_error)?
            .map(|row| row.map(IdentityId::new_unchecked).map_err(db_error))
            .collect::<Result<Vec<_>>>()?;
        if targets.len() > 1_000 {
            return Err(Error::Conflict("safety capacity exceeded".into()));
        }
        let states = targets
            .iter()
            .map(|target| pair(&tx, author, target).map(|(state, _)| state))
            .collect::<Result<Vec<_>>>()?;
        Ok((version, states))
    }

    /// The callback signs using the node's current key, inside the CAS transaction.
    /// Duplicate requests return before signing or updating any materialized state.
    pub fn commit_safety(
        &self,
        request: &SafetyRequest,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
        sign: impl FnOnce(
            &SafetyState,
            Option<Hash>,
            bool,
            u64,
            Option<Hash>,
        ) -> Result<(Option<SafetyAction>, SafetyReceipt)>,
    ) -> Result<SafetyState> {
        request.validate()?;
        let _guard = self.publication_guard()?;
        let mut db = self.safety_connection()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(db_error)?;
        let previous: Option<String> = tx
            .query_row(
                "SELECT record FROM receipts WHERE author=?1 AND key=?2",
                params![request.author_id.as_str(), request.idempotency_key],
                |row| row.get(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(previous) = previous {
            let receipt: SafetyReceipt = serde_json::from_str(&previous).map_err(encoding)?;
            receipt.verify(&identity_at(
                &request.author_id,
                receipt.payload.created_at,
            )?)?;
            verify_receipt_state(&tx, &receipt, &identity_at)?;
            if receipt.payload.request != *request {
                return Err(Error::Conflict(
                    "idempotency key already used for another safety intent".into(),
                ));
            }
            return Ok(receipt.payload.state);
        }
        let (mut state, previous_id) = pair(&tx, &request.author_id, &request.target_id)?;
        if state.revision != request.expected_revision {
            return Err(Error::Conflict(
                "safety revision changed; refresh safety state".into(),
            ));
        }
        let changed = state.blocked != request.blocked || state.muted != request.muted;
        if changed {
            if (request.blocked || request.muted) && !(state.blocked || state.muted) {
                let count: u64 = tx
                    .query_row(
                        "SELECT count(*) FROM pairs WHERE author=?1 AND (blocked=1 OR muted=1)",
                        [request.author_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                if count >= 1_000 {
                    return Err(Error::Conflict(
                        "maximum 1000 blocked or muted identities reached".into(),
                    ));
                }
            }
            state.blocked = request.blocked;
            state.muted = request.muted;
            state.revision = state
                .revision
                .checked_add(1)
                .filter(|v| *v <= 9_007_199_254_740_991)
                .ok_or_else(|| Error::Conflict("safety revision exhausted".into()))?;
        }
        let head: Option<(u64, String)> = tx
            .query_row(
                "SELECT sequence,id FROM receipt_heads WHERE author=?1",
                [request.author_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(db_error)?;
        let sequence = head
            .as_ref()
            .map_or(0, |(sequence, _)| *sequence)
            .checked_add(1)
            .filter(|sequence| *sequence <= 9_007_199_254_740_991)
            .ok_or_else(|| Error::Conflict("safety receipt sequence exhausted".into()))?;
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
            return Err(Error::Conflict("invalid safety transaction records".into()));
        }
        if let Some(id) = &receipt_previous_id {
            let record: String = tx
                .query_row(
                    "SELECT record FROM receipts WHERE id=?1",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            let predecessor: SafetyReceipt = serde_json::from_str(&record).map_err(encoding)?;
            if receipt.payload.created_at < predecessor.payload.created_at {
                return Err(Error::StorageUnavailable(
                    "clock precedes the last safety request; retry with the same key".into(),
                ));
            }
        }
        receipt.verify(&identity_at(
            &request.author_id,
            receipt.payload.created_at,
        )?)?;
        identity_at(&request.target_id, receipt.payload.created_at)?;
        let receipt_id = receipt.id()?;
        if let Some(action) = action {
            action.verify(&identity_at(&request.author_id, action.payload.created_at)?)?;
            if action.payload.state != state
                || action.payload.previous_id != previous_id
                || action.payload.request_id != request.canonical_hash()?
                || action.payload.created_at != receipt.payload.created_at
            {
                return Err(Error::Signature);
            }
            if let Some(id) = &previous_id {
                let record: String = tx
                    .query_row(
                        "SELECT record FROM actions WHERE id=?1",
                        [id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                let predecessor: SafetyAction = serde_json::from_str(&record).map_err(encoding)?;
                if action.payload.created_at < predecessor.payload.created_at {
                    return Err(Error::StorageUnavailable(
                        "clock precedes the last safety action; retry with the same key".into(),
                    ));
                }
            }
            tx.execute(
                "INSERT INTO actions(author,target,revision,id,record,receipt_id) VALUES (?1,?2,?3,?4,?5,?6)",
                params![
                    state.author_id.as_str(),
                    state.target_id.as_str(),
                    state.revision,
                    action.id.as_str(),
                    serde_json::to_string(&action).map_err(encoding)?,
                    receipt_id.as_str(),
                ],
            )
            .map_err(db_error)?;
            tx.execute("INSERT INTO pairs(author,target,revision,blocked,muted,id) VALUES (?1,?2,?3,?4,?5,?6)
                ON CONFLICT(author,target) DO UPDATE SET revision=excluded.revision,blocked=excluded.blocked,muted=excluded.muted,id=excluded.id",
                params![state.author_id.as_str(),state.target_id.as_str(),state.revision,state.blocked,state.muted,action.id.as_str()]).map_err(db_error)?;
            tx.execute("INSERT INTO versions(author,version) VALUES (?1,1) ON CONFLICT(author) DO UPDATE SET version=version+1", [state.author_id.as_str()]).map_err(db_error)?;
        }
        tx.execute(
            "INSERT INTO receipts(author,key,record,sequence,id,target) VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                request.author_id.as_str(),
                request.idempotency_key,
                serde_json::to_string(&receipt).map_err(encoding)?,
                sequence,
                receipt_id.as_str(),
                request.target_id.as_str(),
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

    /// Streaming verification uses one pair at a time, not an in-memory log copy.
    pub fn verify_safety_records(
        &self,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let mut connection = self.safety_connection()?;
        let db = connection.transaction().map_err(db_error)?;
        let broken: bool = db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM pragma_foreign_key_check)",
                [],
                |r| r.get(0),
            )
            .map_err(db_error)?;
        let oversized: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM pairs WHERE blocked=1 OR muted=1 GROUP BY author HAVING count(*)>1000)", [], |r| r.get(0)).map_err(db_error)?;
        if broken || oversized {
            return Err(Error::Signature);
        }
        let mut statement = db.prepare("SELECT author,target,revision,id,record,receipt_id FROM actions ORDER BY author,target,revision").map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous: Option<SafetyAction> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let action: SafetyAction =
                serde_json::from_str(&row.get::<_, String>(4).map_err(db_error)?)
                    .map_err(encoding)?;
            let state = &action.payload.state;
            if row.get::<_, String>(0).map_err(db_error)? != state.author_id.as_str()
                || row.get::<_, String>(1).map_err(db_error)? != state.target_id.as_str()
                || row.get::<_, u64>(2).map_err(db_error)? != state.revision
                || row.get::<_, String>(3).map_err(db_error)? != action.id.as_str()
            {
                return Err(Error::Signature);
            }
            action.verify(&identity_at(&state.author_id, action.payload.created_at)?)?;
            identity_at(&state.target_id, action.payload.created_at)?;
            let receipt_id: String = row.get(5).map_err(db_error)?;
            let record: String = db
                .query_row(
                    "SELECT record FROM receipts WHERE id=?1",
                    [&receipt_id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            let receipt: SafetyReceipt = serde_json::from_str(&record).map_err(encoding)?;
            if receipt.id()?.as_str() != receipt_id
                || receipt.payload.state != *state
                || receipt.payload.request.canonical_hash()? != action.payload.request_id
                || receipt.payload.created_at != action.payload.created_at
                || receipt.payload.request.expected_revision.checked_add(1) != Some(state.revision)
            {
                return Err(Error::Signature);
            }
            let predecessor = previous.as_ref().filter(|p| {
                p.payload.state.author_id == state.author_id
                    && p.payload.state.target_id == state.target_id
            });
            match predecessor {
                Some(p)
                    if state.revision == p.payload.state.revision + 1
                        && (state.blocked != p.payload.state.blocked
                            || state.muted != p.payload.state.muted)
                        && action.payload.previous_id.as_ref() == Some(&p.id)
                        && action.payload.created_at >= p.payload.created_at => {}
                None if state.revision == 1
                    && (state.blocked || state.muted)
                    && action.payload.previous_id.is_none() => {}
                _ => return Err(Error::Signature),
            }
            previous = Some(action);
        }
        let inconsistent: bool = db.query_row("SELECT EXISTS(
            SELECT 1 FROM pairs p LEFT JOIN actions a ON a.id=p.id
            WHERE a.id IS NULL OR a.author!=p.author OR a.target!=p.target OR a.revision!=p.revision
              OR json_extract(a.record,'$.payload.state.blocked') IS NOT p.blocked
              OR json_extract(a.record,'$.payload.state.muted') IS NOT p.muted
              OR p.revision!=(SELECT max(revision) FROM actions WHERE author=p.author AND target=p.target)
            UNION ALL SELECT 1 FROM actions a WHERE NOT EXISTS(SELECT 1 FROM pairs p WHERE p.author=a.author AND p.target=a.target)
            UNION ALL SELECT 1 FROM versions v WHERE v.version!=(SELECT count(*) FROM actions a WHERE a.author=v.author)
            UNION ALL SELECT 1 FROM actions a WHERE NOT EXISTS(SELECT 1 FROM versions v WHERE v.author=a.author))", [], |row| row.get(0)).map_err(db_error)?;
        if inconsistent {
            return Err(Error::Signature);
        }
        let mut statement = db
            .prepare("SELECT author,key,record,sequence,id,target FROM receipts ORDER BY author,sequence")
            .map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous_receipt: Option<SafetyReceipt> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let receipt: SafetyReceipt =
                serde_json::from_str(&row.get::<_, String>(2).map_err(db_error)?)
                    .map_err(encoding)?;
            let payload = &receipt.payload;
            if row.get::<_, String>(0).map_err(db_error)? != payload.request.author_id.as_str()
                || row.get::<_, String>(1).map_err(db_error)? != payload.request.idempotency_key
                || row.get::<_, u64>(3).map_err(db_error)? != payload.sequence
                || row.get::<_, String>(4).map_err(db_error)? != receipt.id()?.as_str()
                || row.get::<_, String>(5).map_err(db_error)? != payload.request.target_id.as_str()
            {
                return Err(Error::Signature);
            }
            receipt.verify(&identity_at(
                &payload.request.author_id,
                payload.created_at,
            )?)?;
            identity_at(&payload.request.target_id, payload.created_at)?;
            let predecessor = previous_receipt
                .as_ref()
                .filter(|previous| previous.payload.request.author_id == payload.request.author_id);
            match predecessor {
                Some(previous)
                    if payload.sequence == previous.payload.sequence + 1
                        && payload.previous_id.as_ref() == Some(&previous.id()?)
                        && payload.created_at >= previous.payload.created_at => {}
                None if payload.sequence == 1 && payload.previous_id.is_none() => {}
                _ => return Err(Error::Signature),
            }
            verify_receipt_state(&db, &receipt, &identity_at)?;
            previous_receipt = Some(receipt);
        }
        let incomplete: bool = db.query_row("SELECT EXISTS(
            SELECT 1 FROM receipt_heads h LEFT JOIN receipts r ON r.id=h.id
                WHERE r.id IS NULL OR r.author!=h.author OR r.sequence!=h.sequence
                OR h.sequence!=(SELECT count(*) FROM receipts WHERE author=h.author)
            UNION ALL SELECT 1 FROM receipts r WHERE NOT EXISTS(SELECT 1 FROM receipt_heads h WHERE h.author=r.author)
        )", [], |row| row.get(0)).map_err(db_error)?;
        if incomplete {
            return Err(Error::Signature);
        }
        Ok(())
    }
}

fn pair(
    db: &Connection,
    author: &IdentityId,
    target: &IdentityId,
) -> Result<(SafetyState, Option<Hash>)> {
    author.validate()?;
    target.validate()?;
    let pair: Option<(bool, bool, u64, String)> = db
        .query_row(
            "SELECT blocked,muted,revision,id FROM pairs WHERE author=?1 AND target=?2",
            params![author.as_str(), target.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match pair {
        Some((blocked, muted, revision, id)) => (
            SafetyState {
                author_id: author.clone(),
                target_id: target.clone(),
                blocked,
                muted,
                revision,
            },
            Some(Hash::new_unchecked(id)),
        ),
        None => (SafetyState::absent(author, target), None),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_first_install_is_atomic_and_interrupted_staging_is_retryable() {
        for phase in ["directory", "database", "schema", "synced"] {
            let root = std::env::temp_dir().join(format!(
                "babble-safety-install-{}-{}-{phase}",
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
                    .initialize_safety_with_hook(|at| if at == phase {
                        Err(Error::StorageUnavailable("interrupted installation".into()))
                    } else {
                        Ok(())
                    })
                    .is_err()
            );
            assert!(!root.join("private_safety").exists());
            assert!(root.join(".private-safety-install").exists());
            let store = FileStore::open(&root).unwrap();
            store
                .verify_safety_records(|_, _| Err(Error::Signature))
                .unwrap();
            assert!(!root.join(".private-safety-install").exists());
            std::fs::remove_file(root.join("private_safety/safety.sqlite3")).unwrap();
            assert!(FileStore::open(&root).is_err());
            std::fs::remove_dir_all(root).unwrap();
        }
    }
}

fn verify_receipt_state(
    db: &Connection,
    receipt: &SafetyReceipt,
    identity_at: &impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
) -> Result<()> {
    let payload = &receipt.payload;
    // Indexed predecessor by pair and signed sequence, not wall time. Verify it here too:
    // live receipt retries do not get the startup verifier's earlier-record guarantees.
    let previous: Option<(String, String, u64)> = db
        .query_row(
            "SELECT record,id,sequence FROM receipts
        WHERE author=?1 AND target=?2 AND sequence<?3 ORDER BY sequence DESC LIMIT 1",
            params![
                payload.state.author_id.as_str(),
                payload.state.target_id.as_str(),
                payload.sequence
            ],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    let before = previous
        .map(|(raw, id, sequence)| {
            let previous: SafetyReceipt = serde_json::from_str(&raw).map_err(encoding)?;
            previous.verify(&identity_at(
                &previous.payload.request.author_id,
                previous.payload.created_at,
            )?)?;
            if previous.id()?.as_str() != id
                || previous.payload.sequence != sequence
                || previous.payload.state.author_id != payload.state.author_id
                || previous.payload.state.target_id != payload.state.target_id
                || previous.payload.created_at > payload.created_at
            {
                return Err(Error::Signature);
            }
            Ok(previous.payload.state)
        })
        .transpose()?
        .unwrap_or_else(|| SafetyState::absent(&payload.state.author_id, &payload.state.target_id));
    if before.revision != payload.request.expected_revision {
        return Err(Error::Signature);
    }
    let changed =
        (before.blocked, before.muted) != (payload.request.blocked, payload.request.muted);
    let expected = if changed {
        before
            .revision
            .checked_add(1)
            .filter(|v| *v <= 9_007_199_254_740_991)
            .ok_or(Error::Signature)?
    } else {
        before.revision
    };
    if payload.state.revision != expected {
        return Err(Error::Signature);
    }
    if payload.state.revision > 0 {
        let (raw, id, action_receipt): (String, String, String) = db
            .query_row(
                "SELECT record,id,receipt_id FROM actions WHERE author=?1 AND target=?2 AND revision=?3",
                params![
                    payload.state.author_id.as_str(),
                    payload.state.target_id.as_str(),
                    payload.state.revision
                ],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(db_error)?;
        let action: SafetyAction = serde_json::from_str(&raw).map_err(encoding)?;
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

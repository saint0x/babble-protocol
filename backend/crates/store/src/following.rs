//! SQLite owns the private action log, materialized pairs, and durable request receipts.
//! No connection/cache fields: each operation holds the existing root publication guard.
use crate::FileStore;
use babel_graph::{FollowAction, FollowReceipt, FollowRequest, FollowState};
use babel_identity::Identity;
use babel_types::{Canonical, Error, Hash, IdentityId, Result, Timestamp};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

fn db_error(_: rusqlite::Error) -> Error {
    Error::StorageUnavailable(
        "private following database failure; retry with the same request key".into(),
    )
}
fn encoding(_: serde_json::Error) -> Error {
    Error::StorageUnavailable("invalid private following record; restore the store".into())
}

impl FileStore {
    fn following_connection(&self) -> Result<Connection> {
        let path = self
            .root()
            .join("private_following")
            .join("following.sqlite3");
        let connection = Connection::open(path).map_err(db_error)?;
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

    pub fn initialize_following(&self) -> Result<()> {
        let _guard = self.publication_guard()?;
        let directory = self.root().join("private_following");
        std::fs::create_dir_all(&directory)
            .map_err(|_| Error::Conflict("create private following directory".into()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| Error::Conflict("secure private following directory".into()))?;
        }
        let connection = self.following_connection()?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                directory.join("following.sqlite3"),
                std::fs::Permissions::from_mode(0o600),
            )
            .map_err(|_| Error::Conflict("secure private following database".into()))?;
        }
        connection.execute_batch("
            CREATE TABLE IF NOT EXISTS actions (
                author TEXT NOT NULL, target TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                id TEXT NOT NULL UNIQUE, record TEXT NOT NULL,
                receipt_id TEXT NOT NULL REFERENCES receipts(id) DEFERRABLE INITIALLY DEFERRED,
                PRIMARY KEY(author,target,revision));
            CREATE TABLE IF NOT EXISTS pairs (
                author TEXT NOT NULL, target TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
                following INTEGER NOT NULL CHECK(following IN(0,1)), id TEXT NOT NULL REFERENCES actions(id),
                PRIMARY KEY(author,target));
            CREATE INDEX IF NOT EXISTS followed_targets ON pairs(author,following,target);
            CREATE TABLE IF NOT EXISTS versions (author TEXT PRIMARY KEY, version INTEGER NOT NULL CHECK(version>0));
            CREATE TABLE IF NOT EXISTS receipts (
                author TEXT NOT NULL, key TEXT NOT NULL, record TEXT NOT NULL,
                sequence INTEGER NOT NULL CHECK(sequence>0), id TEXT NOT NULL UNIQUE,
                PRIMARY KEY(author,key), UNIQUE(author,sequence));
            CREATE TABLE IF NOT EXISTS receipt_heads (
                author TEXT PRIMARY KEY, sequence INTEGER NOT NULL CHECK(sequence>0),
                id TEXT NOT NULL REFERENCES receipts(id));
        ").map_err(db_error)?;
        Ok(())
    }

    pub fn following_state(&self, author: &IdentityId, target: &IdentityId) -> Result<FollowState> {
        let _guard = self.publication_guard()?;
        pair(&self.following_connection()?, author, target).map(|(state, _)| state)
    }

    /// At most 10,000 active follows per author bounds feed merge memory.
    pub fn following_snapshot(&self, author: &IdentityId) -> Result<(u64, Vec<IdentityId>)> {
        let _guard = self.publication_guard()?;
        let mut db = self.following_connection()?;
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
        let mut statement = tx.prepare("SELECT target FROM pairs WHERE author=?1 AND following=1 ORDER BY target LIMIT 10001").map_err(db_error)?;
        let targets = statement
            .query_map([author.as_str()], |row| row.get::<_, String>(0))
            .map_err(db_error)?
            .map(|row| row.map(IdentityId::new_unchecked).map_err(db_error))
            .collect::<Result<Vec<_>>>()?;
        if targets.len() > 10_000 {
            return Err(Error::Conflict("following capacity exceeded".into()));
        }
        Ok((version, targets))
    }

    /// The callback signs using the node's current key, inside the CAS transaction.
    /// Duplicate requests return before signing or updating any materialized state.
    pub fn commit_following(
        &self,
        request: &FollowRequest,
        sign: impl FnOnce(
            &FollowState,
            Option<Hash>,
            bool,
            u64,
            Option<Hash>,
        ) -> Result<(Option<FollowAction>, FollowReceipt)>,
    ) -> Result<FollowState> {
        request.validate()?;
        let _guard = self.publication_guard()?;
        let mut db = self.following_connection()?;
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
            let receipt: FollowReceipt = serde_json::from_str(&previous).map_err(encoding)?;
            if receipt.payload.request != *request {
                return Err(Error::Conflict(
                    "idempotency key already used for another follow intent".into(),
                ));
            }
            return Ok(receipt.payload.state);
        }
        let (mut state, previous_id) = pair(&tx, &request.author_id, &request.target_id)?;
        if state.revision != request.expected_revision {
            return Err(Error::Conflict(
                "follow revision changed; refresh follow state".into(),
            ));
        }
        let changed = state.following != request.following;
        if changed {
            if request.following {
                let count: u64 = tx
                    .query_row(
                        "SELECT count(*) FROM pairs WHERE author=?1 AND following=1",
                        [request.author_id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                if count >= 10_000 {
                    return Err(Error::Conflict(
                        "maximum 10000 followed identities reached".into(),
                    ));
                }
            }
            state.following = request.following;
            state.revision = state
                .revision
                .checked_add(1)
                .filter(|v| *v <= 9_007_199_254_740_991)
                .ok_or_else(|| Error::Conflict("follow revision exhausted".into()))?;
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
            .ok_or_else(|| Error::Conflict("follow receipt sequence exhausted".into()))?;
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
            return Err(Error::Conflict("invalid follow transaction records".into()));
        }
        if let Some(id) = &receipt_previous_id {
            let record: String = tx
                .query_row(
                    "SELECT record FROM receipts WHERE id=?1",
                    [id.as_str()],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            let predecessor: FollowReceipt = serde_json::from_str(&record).map_err(encoding)?;
            if receipt.payload.created_at < predecessor.payload.created_at {
                return Err(Error::StorageUnavailable(
                    "clock precedes the last follow request; retry with the same key".into(),
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
            if let Some(id) = &previous_id {
                let record: String = tx
                    .query_row(
                        "SELECT record FROM actions WHERE id=?1",
                        [id.as_str()],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                let predecessor: FollowAction = serde_json::from_str(&record).map_err(encoding)?;
                if action.payload.created_at < predecessor.payload.created_at {
                    return Err(Error::StorageUnavailable(
                        "clock precedes the last follow action; retry with the same key".into(),
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
            tx.execute("INSERT INTO pairs(author,target,revision,following,id) VALUES (?1,?2,?3,?4,?5)
                ON CONFLICT(author,target) DO UPDATE SET revision=excluded.revision,following=excluded.following,id=excluded.id",
                params![state.author_id.as_str(),state.target_id.as_str(),state.revision,state.following,action.id.as_str()]).map_err(db_error)?;
            tx.execute("INSERT INTO versions(author,version) VALUES (?1,1) ON CONFLICT(author) DO UPDATE SET version=version+1", [state.author_id.as_str()]).map_err(db_error)?;
        }
        tx.execute(
            "INSERT INTO receipts(author,key,record,sequence,id) VALUES (?1,?2,?3,?4,?5)",
            params![
                request.author_id.as_str(),
                request.idempotency_key,
                serde_json::to_string(&receipt).map_err(encoding)?,
                sequence,
                receipt_id.as_str(),
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
    pub fn verify_following_records(
        &self,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let db = self.following_connection()?;
        let mut statement = db.prepare("SELECT author,target,revision,id,record,receipt_id FROM actions ORDER BY author,target,revision").map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous: Option<FollowAction> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let action: FollowAction =
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
            let receipt: FollowReceipt = serde_json::from_str(&record).map_err(encoding)?;
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
                        && state.following != p.payload.state.following
                        && action.payload.previous_id.as_ref() == Some(&p.id)
                        && action.payload.created_at >= p.payload.created_at => {}
                None if state.revision == 1
                    && state.following
                    && action.payload.previous_id.is_none() => {}
                _ => return Err(Error::Signature),
            }
            previous = Some(action);
        }
        let inconsistent: bool = db.query_row("SELECT EXISTS(
            SELECT 1 FROM pairs p LEFT JOIN actions a ON a.id=p.id
            WHERE a.id IS NULL OR a.author!=p.author OR a.target!=p.target OR a.revision!=p.revision
              OR json_extract(a.record,'$.payload.state.following')!=p.following
              OR p.revision!=(SELECT max(revision) FROM actions WHERE author=p.author AND target=p.target)
            UNION ALL SELECT 1 FROM actions a WHERE NOT EXISTS(SELECT 1 FROM pairs p WHERE p.author=a.author AND p.target=a.target)
            UNION ALL SELECT 1 FROM versions v WHERE v.version!=(SELECT count(*) FROM actions a WHERE a.author=v.author)
            UNION ALL SELECT 1 FROM actions a WHERE NOT EXISTS(SELECT 1 FROM versions v WHERE v.author=a.author))", [], |row| row.get(0)).map_err(db_error)?;
        if inconsistent {
            return Err(Error::Signature);
        }
        let mut statement = db
            .prepare("SELECT author,key,record,sequence,id FROM receipts ORDER BY author,sequence")
            .map_err(db_error)?;
        let mut rows = statement.query([]).map_err(db_error)?;
        let mut previous_receipt: Option<FollowReceipt> = None;
        while let Some(row) = rows.next().map_err(db_error)? {
            let receipt: FollowReceipt =
                serde_json::from_str(&row.get::<_, String>(2).map_err(db_error)?)
                    .map_err(encoding)?;
            let payload = &receipt.payload;
            if row.get::<_, String>(0).map_err(db_error)? != payload.request.author_id.as_str()
                || row.get::<_, String>(1).map_err(db_error)? != payload.request.idempotency_key
                || row.get::<_, u64>(3).map_err(db_error)? != payload.sequence
                || row.get::<_, String>(4).map_err(db_error)? != receipt.id()?.as_str()
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
            if payload.state.revision == 0 {
                if payload.state.following {
                    return Err(Error::Signature);
                }
            } else {
                let record: String = db
                    .query_row(
                        "SELECT record FROM actions WHERE author=?1 AND target=?2 AND revision=?3",
                        params![
                            payload.state.author_id.as_str(),
                            payload.state.target_id.as_str(),
                            payload.state.revision
                        ],
                        |row| row.get(0),
                    )
                    .map_err(db_error)?;
                let action: FollowAction = serde_json::from_str(&record).map_err(encoding)?;
                if action.payload.state != payload.state {
                    return Err(Error::Signature);
                }
                if payload.state.revision == payload.request.expected_revision + 1
                    && action.payload.request_id != payload.request.canonical_hash()?
                {
                    return Err(Error::Signature);
                }
            }
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
) -> Result<(FollowState, Option<Hash>)> {
    let pair: Option<(bool, u64, String)> = db
        .query_row(
            "SELECT following,revision,id FROM pairs WHERE author=?1 AND target=?2",
            params![author.as_str(), target.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(db_error)?;
    Ok(match pair {
        Some((following, revision, id)) => (
            FollowState {
                author_id: author.clone(),
                target_id: target.clone(),
                following,
                revision,
            },
            Some(Hash::new_unchecked(id)),
        ),
        None => (FollowState::absent(author, target), None),
    })
}

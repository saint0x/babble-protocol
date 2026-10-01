//! Private signed audit, receipts, and projections share one durable transaction.
use crate::FileStore;
use babel_graph::moderation::*;
use babel_identity::Identity;
use babel_types::{Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use std::collections::{BTreeMap, BTreeSet};

fn unavailable(_: impl std::fmt::Display) -> Error {
    Error::StorageUnavailable("private moderation storage unavailable".into())
}
fn encode<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(unavailable)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(unavailable)
}
fn case(db: &Connection, id: &str) -> Result<Option<ModerationCase>> {
    db.query_row("SELECT record FROM cases WHERE id=?1", [id], |r| {
        r.get::<_, String>(0)
    })
    .optional()
    .map_err(unavailable)?
    .map(|s| decode(&s))
    .transpose()
}
fn head(db: &Connection) -> Result<(u64, Option<Hash>)> {
    Ok(db
        .query_row("SELECT sequence,id FROM head WHERE singleton=1", [], |r| {
            Ok((r.get::<_, u64>(0)?, r.get::<_, String>(1)?))
        })
        .optional()
        .map_err(unavailable)?
        .map_or((0, None), |(s, id)| (s, Some(Hash::new_unchecked(id)))))
}

impl FileStore {
    fn moderation_connection(&self) -> Result<Connection> {
        let db = Connection::open_with_flags(
            self.root().join("private_moderation/moderation.sqlite3"),
            OpenFlags::SQLITE_OPEN_READ_WRITE,
        )
        .map_err(unavailable)?;
        db.busy_timeout(std::time::Duration::from_secs(5))
            .map_err(unavailable)?;
        db.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA cache_size=-2048;",
        )
        .map_err(unavailable)?;
        Ok(db)
    }
    pub(crate) fn initialize_moderation(&self) -> Result<()> {
        let _guard = self.publication_guard()?;
        let directory = self.root().join("private_moderation");
        if !directory.try_exists().map_err(unavailable)? {
            let staged = self.root().join(".private-moderation-install");
            if staged.try_exists().map_err(unavailable)? {
                std::fs::remove_dir_all(&staged).map_err(unavailable)?;
            }
            std::fs::create_dir(&staged).map_err(unavailable)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o700))
                    .map_err(unavailable)?;
            }
            let file = staged.join("moderation.sqlite3");
            let db = Connection::open(&file).map_err(unavailable)?;
            db.execute_batch("PRAGMA synchronous=FULL; BEGIN IMMEDIATE;
                CREATE TABLE receipts(sequence INTEGER PRIMARY KEY CHECK(sequence>0), id TEXT UNIQUE NOT NULL, actor TEXT NOT NULL, key TEXT NOT NULL, record TEXT NOT NULL, UNIQUE(actor,key));
                CREATE TABLE head(singleton INTEGER PRIMARY KEY CHECK(singleton=1), sequence INTEGER NOT NULL, id TEXT NOT NULL REFERENCES receipts(id));
                CREATE TABLE cases(id TEXT PRIMARY KEY, sequence INTEGER UNIQUE NOT NULL, object TEXT NOT NULL, reporter TEXT NOT NULL, subject TEXT NOT NULL, status TEXT NOT NULL, restricted INTEGER NOT NULL CHECK(restricted IN(0,1)), record TEXT NOT NULL);
                CREATE INDEX mine ON cases(reporter,sequence DESC);
                CREATE INDEX affected ON cases(subject,sequence DESC);
                CREATE INDEX queue ON cases(status,sequence DESC);
                CREATE INDEX restrictions ON cases(object,restricted);
                PRAGMA user_version=1; COMMIT;").map_err(unavailable)?;
            db.close().map_err(|(_, e)| unavailable(e))?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600))
                    .map_err(unavailable)?;
            }
            for path in [&file, &staged] {
                std::fs::File::open(path)
                    .and_then(|f| f.sync_all())
                    .map_err(unavailable)?;
            }
            std::fs::rename(staged, &directory).map_err(unavailable)?;
            std::fs::File::open(self.root())
                .and_then(|f| f.sync_all())
                .map_err(unavailable)?;
        }
        let db = self.moderation_connection()?;
        let version: u32 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(unavailable)?;
        if version != 1 {
            return Err(unavailable("schema version"));
        }
        Ok(())
    }
    pub fn moderation_case(&self, id: &str) -> Result<Option<ModerationCase>> {
        canonical_id(id, "report_")?;
        let _guard = self.publication_guard()?;
        case(&self.moderation_connection()?, id)
    }
    pub fn moderation_restrictions(&self) -> Result<(u64, BTreeSet<ObjectId>)> {
        let _guard = self.publication_guard()?;
        let mut db = self.moderation_connection()?;
        let tx = db.transaction().map_err(unavailable)?;
        let sequence = head(&tx)?.0;
        let mut stmt = tx
            .prepare("SELECT DISTINCT object FROM cases WHERE restricted=1")
            .map_err(unavailable)?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(unavailable)?
            .map(|r| r.map(ObjectId::new_unchecked).map_err(unavailable))
            .collect::<Result<_>>()?;
        Ok((sequence, ids))
    }
    pub fn moderation_restricted(&self, object: &ObjectId) -> Result<bool> {
        let _guard = self.publication_guard()?;
        self.moderation_connection()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM cases WHERE object=?1 AND restricted=1)",
                [object.as_str()],
                |r| r.get(0),
            )
            .map_err(unavailable)
    }
    pub fn moderation_list(
        &self,
        actor: &IdentityId,
        scope: ModerationScope,
        before: Option<u64>,
        limit: usize,
    ) -> Result<ModerationPage> {
        if limit == 0 || limit > 100 || before.is_some_and(|n| n == 0 || n > MAX_SEQUENCE) {
            return Err(Error::Canonical("invalid moderation pagination".into()));
        }
        let _guard = self.publication_guard()?;
        let db = self.moderation_connection()?;
        let predicate = match scope {
            ModerationScope::Mine => "reporter=?1",
            ModerationScope::Affected => "subject=?1 AND status!='pending'",
            ModerationScope::Queue => "status IN ('pending','appealed') AND ?1 IS NOT NULL",
        };
        let mut stmt = db.prepare(&format!("SELECT record FROM cases WHERE {predicate} AND sequence<?2 ORDER BY sequence DESC LIMIT ?3")).map_err(unavailable)?;
        let mut items = stmt
            .query_map(
                params![
                    actor.as_str(),
                    before.unwrap_or(MAX_SEQUENCE + 1),
                    limit + 1
                ],
                |r| r.get::<_, String>(0),
            )
            .map_err(unavailable)?
            .map(|r| decode(&r.map_err(unavailable)?))
            .collect::<Result<Vec<ModerationCase>>>()?;
        let next_before = if items.len() > limit {
            items.truncate(limit);
            items.last().map(|c| c.sequence)
        } else {
            None
        };
        Ok(ModerationPage { items, next_before })
    }
    pub fn commit_moderation(
        &self,
        actor: &IdentityId,
        reviewers: &BTreeSet<IdentityId>,
        intent: &ModerationIntent,
        subject: &IdentityId,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
        sign: impl FnOnce(ModerationReceiptPayload) -> Result<ModerationReceipt>,
    ) -> Result<ModerationCase> {
        intent.validate()?;
        let _guard = self.publication_guard()?;
        let mut db = self.moderation_connection()?;
        let tx = db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(unavailable)?;
        let retry: Option<String> = tx
            .query_row(
                "SELECT record FROM receipts WHERE actor=?1 AND key=?2",
                params![actor.as_str(), intent.key()],
                |r| r.get(0),
            )
            .optional()
            .map_err(unavailable)?;
        if let Some(retry) = retry {
            let receipt: ModerationReceipt = decode(&retry)?;
            receipt.verify(&identity_at(actor, receipt.payload.result.updated_at)?)?;
            if receipt.payload.actor != *actor || receipt.payload.intent != *intent {
                return Err(Error::Conflict(
                    "idempotency key already used for another moderation intent".into(),
                ));
            }
            return Ok(receipt.payload.result);
        }
        let previous = case(&tx, &intent.case_id(actor)?)?;
        if matches!(intent, ModerationIntent::Report(_)) {
            let count: u64 = tx
                .query_row(
                    "SELECT count(*) FROM cases WHERE reporter=?1",
                    [actor.as_str()],
                    |r| r.get(0),
                )
                .map_err(unavailable)?;
            if count >= 1000 {
                return Err(Error::Conflict(REPORT_INTAKE_LIMIT.into()));
            }
        }
        let (n, previous_id) = head(&tx)?;
        let sequence = n
            .checked_add(1)
            .filter(|n| *n <= MAX_SEQUENCE)
            .ok_or_else(|| Error::Conflict("moderation sequence exhausted".into()))?;
        let at = Timestamp::now();
        let result = transition(previous, actor, reviewers, intent, subject, sequence, at)?;
        let payload = ModerationReceiptPayload {
            actor: actor.clone(),
            reviewers: reviewers.clone(),
            intent: intent.clone(),
            result: result.clone(),
            sequence,
            previous_id,
        };
        let receipt = sign(payload.clone())?;
        if receipt.payload != payload {
            return Err(Error::Signature);
        }
        receipt.verify(&identity_at(actor, at)?)?;
        let id = receipt.id()?;
        tx.execute(
            "INSERT INTO receipts(sequence,id,actor,key,record) VALUES(?1,?2,?3,?4,?5)",
            params![
                sequence,
                id.as_str(),
                actor.as_str(),
                intent.key(),
                encode(&receipt)?
            ],
        )
        .map_err(unavailable)?;
        tx.execute("INSERT INTO head(singleton,sequence,id) VALUES(1,?1,?2) ON CONFLICT(singleton) DO UPDATE SET sequence=excluded.sequence,id=excluded.id",params![sequence,id.as_str()]).map_err(unavailable)?;
        tx.execute("INSERT INTO cases(id,sequence,object,reporter,subject,status,restricted,record) VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(id) DO UPDATE SET status=excluded.status,restricted=excluded.restricted,record=excluded.record",params![result.id,result.sequence,result.object_id.as_str(),result.reporter_id.as_ref().ok_or(Error::Signature)?.as_str(),result.subject_author_id.as_str(),status(result.status),result.restricted(),encode(&result)?]).map_err(unavailable)?;
        tx.commit().map_err(unavailable)?;
        Ok(result)
    }
    /// Rebuild every projection from signed history and compare without repairing tampering.
    pub fn verify_moderation(
        &self,
        identity_at: impl Fn(&IdentityId, Timestamp) -> Result<Identity>,
        object_author: impl Fn(&ObjectId) -> Result<IdentityId>,
    ) -> Result<()> {
        let _guard = self.publication_guard()?;
        let mut db = self.moderation_connection()?;
        let tx = db.transaction().map_err(unavailable)?;
        let mut states = BTreeMap::new();
        let mut expected_head = (0, None);
        let mut stmt = tx
            .prepare("SELECT sequence,id,actor,key,record FROM receipts ORDER BY sequence")
            .map_err(unavailable)?;
        let mut rows = stmt.query([]).map_err(unavailable)?;
        while let Some(row) = rows.next().map_err(unavailable)? {
            let receipt: ModerationReceipt =
                decode(&row.get::<_, String>(4).map_err(unavailable)?)?;
            let p = &receipt.payload;
            receipt.verify(&identity_at(&p.actor, p.result.updated_at)?)?;
            if p.sequence != expected_head.0 + 1
                || p.previous_id != expected_head.1
                || p.sequence != row.get::<_, u64>(0).map_err(unavailable)?
                || receipt.id()?.as_str() != row.get::<_, String>(1).map_err(unavailable)?
                || p.actor.as_str() != row.get::<_, String>(2).map_err(unavailable)?
                || p.intent.key() != row.get::<_, String>(3).map_err(unavailable)?
            {
                return Err(Error::Signature);
            }
            let author = object_author(&p.result.object_id)?;
            if let ModerationIntent::Decision { request, .. } = &p.intent {
                for signal in &request.source_signals {
                    if !self
                        .object_judgment_input_unlocked(signal)?
                        .is_some_and(|i| i.object_id == p.result.object_id)
                    {
                        return Err(Error::Signature);
                    }
                }
            }
            let next = transition(
                states.remove(&p.result.id),
                &p.actor,
                &p.reviewers,
                &p.intent,
                &author,
                p.sequence,
                p.result.updated_at,
            )?;
            if next != p.result {
                return Err(Error::Signature);
            }
            states.insert(next.id.clone(), next);
            expected_head = (p.sequence, Some(receipt.id()?));
        }
        if head(&tx)? != expected_head {
            return Err(Error::Signature);
        }
        let mut stmt = tx
            .prepare(
                "SELECT id,sequence,object,reporter,subject,status,restricted,record FROM cases",
            )
            .map_err(unavailable)?;
        let mut rows = stmt.query([]).map_err(unavailable)?;
        while let Some(row) = rows.next().map_err(unavailable)? {
            let actual: ModerationCase = decode(&row.get::<_, String>(7).map_err(unavailable)?)?;
            let expected = states.remove(&actual.id).ok_or(Error::Signature)?;
            if actual != expected
                || actual.id != row.get::<_, String>(0).map_err(unavailable)?
                || actual.sequence != row.get::<_, u64>(1).map_err(unavailable)?
                || actual.object_id.as_str() != row.get::<_, String>(2).map_err(unavailable)?
                || actual
                    .reporter_id
                    .as_ref()
                    .ok_or(Error::Signature)?
                    .as_str()
                    != row.get::<_, String>(3).map_err(unavailable)?
                || actual.subject_author_id.as_str()
                    != row.get::<_, String>(4).map_err(unavailable)?
                || status(actual.status) != row.get::<_, String>(5).map_err(unavailable)?
                || actual.restricted() != row.get::<_, bool>(6).map_err(unavailable)?
            {
                return Err(Error::Signature);
            }
        }
        if !states.is_empty() {
            return Err(Error::Signature);
        }
        Ok(())
    }
}
fn status(s: ModerationStatus) -> &'static str {
    match s {
        ModerationStatus::Pending => "pending",
        ModerationStatus::Decided => "decided",
        ModerationStatus::Appealed => "appealed",
        ModerationStatus::Closed => "closed",
    }
}

//! Disposable lookup data. Canonical JSON pairs are validated on rebuild and
//! again on selection. Every caller holds the OS publication lock.
use super::*;
use babel_judgment::{DefinitionId, ProviderVersion};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

const INDEX: &str = "object-judgments.sqlite3";
const REBUILD: &str = ".object-judgments-rebuild.sqlite3";
const APPLICATION_ID: i64 = 0x424a4931;
const VERSION: i64 = 1;
const SCHEMA: &str = "
    PRAGMA application_id = 1112164657;
    PRAGMA user_version = 1;
    CREATE TABLE entries (
        id TEXT PRIMARY KEY NOT NULL,
        object_id TEXT NOT NULL,
        definition TEXT NOT NULL,
        provider TEXT NOT NULL,
        created_seconds INTEGER NOT NULL,
        created_nanos INTEGER NOT NULL CHECK(created_nanos BETWEEN 0 AND 999999999)
    ) STRICT;
    CREATE INDEX object_history ON entries(object_id, id);
    CREATE INDEX latest_input ON entries(
        object_id, definition, provider, created_seconds DESC, created_nanos DESC, id DESC
    );
    CREATE TABLE readiness (id INTEGER PRIMARY KEY CHECK(id=1), ready INTEGER NOT NULL CHECK(ready=1)) STRICT;
    INSERT INTO readiness VALUES (1, 1);
";

struct Entry {
    id: String,
    object_id: String,
    definition: String,
    provider: String,
    seconds: i64,
    nanos: u32,
}

impl Entry {
    fn read(row: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            object_id: row.get(1)?,
            definition: row.get(2)?,
            provider: row.get(3)?,
            seconds: row.get(4)?,
            nanos: row.get(5)?,
        })
    }

    fn validated_pair(&self, store: &FileStore) -> Result<(ObjectJudgmentInput, Judgment)> {
        let (input, judgment) = store
            .object_judgment_pair_unlocked(&JudgmentId::new_unchecked(&self.id))?
            .ok_or_else(|| index_error("indexed association missing"))?;
        if self.object_id != input.object_id.as_str()
            || self.definition != judgment.definition.as_str()
            || self.provider != provider_key(&judgment.provider)?
            || self.seconds != judgment.created_at.0.unix_timestamp()
            || self.nanos != judgment.created_at.0.nanosecond()
        {
            return Err(index_error("indexed metadata differs from canonical pair"));
        }
        Ok((input, judgment))
    }
}

impl FileStore {
    fn judgment_index_connection(&self, writable: bool) -> Result<Connection> {
        // Never create an empty index during a query or an incremental update.
        let flags = if writable {
            OpenFlags::SQLITE_OPEN_READ_WRITE
        } else {
            OpenFlags::SQLITE_OPEN_READ_ONLY
        };
        let db = Connection::open_with_flags(self.root.join(INDEX), flags).map_err(index_error)?;
        let application: i64 = db
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(index_error)?;
        let version: i64 = db
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(index_error)?;
        if application != APPLICATION_ID || version != VERSION {
            return Err(index_error("unknown index format; reopen store to rebuild"));
        }
        let ready: Option<i64> = db
            .query_row("SELECT ready FROM readiness WHERE id=1", [], |r| r.get(0))
            .optional()
            .map_err(index_error)?;
        if ready != Some(1) {
            return Err(index_error("incomplete index; reopen store to rebuild"));
        }
        if writable {
            db.pragma_update(None, "synchronous", "FULL")
                .map_err(index_error)?;
        }
        Ok(db)
    }

    pub(crate) fn rebuild_judgment_index(&self) -> Result<()> {
        let path = self.root.join(REBUILD);
        // Only this lock-protected, disposable rebuild database is removed.
        for stale in [path.clone(), self.root.join(format!("{REBUILD}-journal"))] {
            match fs::remove_file(stale) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(index_error(error)),
            }
        }
        let mut db = Connection::open(&path).map_err(index_error)?;
        db.execute_batch("PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;")
            .map_err(index_error)?;
        let tx = db.transaction().map_err(index_error)?;
        tx.execute_batch(SCHEMA).map_err(index_error)?;
        for entry in fs::read_dir(self.root.join("object_judgment_inputs")).map_err(index_error)? {
            let path = entry.map_err(index_error)?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let id = path
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| index_error("invalid canonical association filename"))?;
            let (input, judgment) = self
                .object_judgment_pair_unlocked(&JudgmentId::new_unchecked(id))?
                .ok_or_else(|| index_error("canonical association disappeared during rebuild"))?;
            upsert(&tx, &input, &judgment)?;
        }
        tx.commit().map_err(index_error)?;
        db.close().map_err(|(_, error)| index_error(error))?;
        fs::File::open(&path)
            .and_then(|file| file.sync_all())
            .map_err(index_error)?;
        // A process exit inside the old SQLite transaction can leave a hot
        // rollback journal. It must never be replayed against the replacement.
        for suffix in ["-journal", "-wal", "-shm"] {
            match fs::remove_file(self.root.join(format!("{INDEX}{suffix}"))) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(index_error(error)),
            }
        }
        fs::rename(&path, self.root.join(INDEX)).map_err(index_error)?;
        fs::File::open(&self.root)
            .and_then(|file| file.sync_all())
            .map_err(index_error)
    }

    pub(crate) fn update_judgment_index(
        &self,
        ids: impl Iterator<Item = JudgmentId>,
        hook: &mut impl FnMut(&str) -> Result<()>,
    ) -> Result<()> {
        let mut db = self.judgment_index_connection(true)?;
        let tx = db.transaction().map_err(index_error)?;
        for id in ids {
            if let Some((input, judgment)) = self.object_judgment_pair_unlocked(&id)? {
                upsert(&tx, &input, &judgment)?;
            } else {
                tx.execute("DELETE FROM entries WHERE id=?1", [id.as_str()])
                    .map_err(index_error)?;
            }
        }
        hook("index-updated")?;
        tx.commit().map_err(index_error)?;
        hook("indexed")
    }

    pub(crate) fn indexed_object_judgment_inputs(
        &self,
        object_id: &ObjectId,
    ) -> Result<Vec<ObjectJudgmentInput>> {
        let db = self.judgment_index_connection(false)?;
        let mut statement = db
            .prepare(
                "SELECT id,object_id,definition,provider,created_seconds,created_nanos
             FROM entries WHERE object_id=?1 ORDER BY id",
            )
            .map_err(index_error)?;
        let rows = statement
            .query_map([object_id.as_str()], Entry::read)
            .map_err(index_error)?;
        rows.map(|row| {
            let (input, _) = row.map_err(index_error)?.validated_pair(self)?;
            if &input.object_id != object_id {
                return Err(index_error("history row outside requested Object"));
            }
            Ok(input)
        })
        .collect()
    }

    pub(crate) fn latest_indexed_object_judgment_input(
        &self,
        object_id: &ObjectId,
        definition: &DefinitionId,
        provider: &ProviderVersion,
        reference: Timestamp,
    ) -> Result<Option<(ObjectJudgmentInput, Judgment)>> {
        let db = self.judgment_index_connection(false)?;
        let entry = db
            .query_row(
                "SELECT id,object_id,definition,provider,created_seconds,created_nanos
             FROM entries WHERE object_id=?1 AND definition=?2 AND provider=?3
               AND (created_seconds,created_nanos)<=(?4,?5)
             ORDER BY created_seconds DESC,created_nanos DESC,id DESC LIMIT 1",
                params![
                    object_id.as_str(),
                    definition.as_str(),
                    provider_key(provider)?,
                    reference.0.unix_timestamp(),
                    reference.0.nanosecond()
                ],
                Entry::read,
            )
            .optional()
            .map_err(index_error)?;
        entry
            .map(|entry| {
                let (input, judgment) = entry.validated_pair(self)?;
                if &input.object_id != object_id
                    || &judgment.definition != definition
                    || &judgment.provider != provider
                    || judgment.created_at > reference
                {
                    return Err(index_error("latest row outside requested scope"));
                }
                Ok((input, judgment))
            })
            .transpose()
    }
}

fn upsert(db: &Connection, input: &ObjectJudgmentInput, judgment: &Judgment) -> Result<()> {
    db.execute(
        "INSERT INTO entries(id,object_id,definition,provider,created_seconds,created_nanos)
         VALUES (?1,?2,?3,?4,?5,?6) ON CONFLICT(id) DO UPDATE SET
         object_id=excluded.object_id,definition=excluded.definition,provider=excluded.provider,
         created_seconds=excluded.created_seconds,created_nanos=excluded.created_nanos",
        params![
            judgment.id.as_str(),
            input.object_id.as_str(),
            judgment.definition.as_str(),
            provider_key(&judgment.provider)?,
            judgment.created_at.0.unix_timestamp(),
            judgment.created_at.0.nanosecond()
        ],
    )
    .map_err(index_error)?;
    Ok(())
}

fn provider_key(provider: &ProviderVersion) -> Result<String> {
    serde_json::to_string(provider).map_err(index_error)
}

fn index_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::Conflict(format!("object judgment index: {error}"))
}

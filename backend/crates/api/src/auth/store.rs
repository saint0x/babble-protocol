use super::{ApiError, Principal, random_token};
use rusqlite::{Connection, OptionalExtension, params};
use std::{fs, path::Path, time::Duration};
use time::OffsetDateTime;
mod documents;
mod security;
#[cfg(test)]
mod security_tests;

pub(crate) struct AuthStore {
    db: Connection,
}

pub(super) const CLEANUP_BATCH: usize = 128;

impl AuthStore {
    pub fn open(root: &Path) -> Result<Self, ApiError> {
        let directory = root.join("auth");
        fs::create_dir_all(&directory).map_err(storage_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
                .map_err(storage_error)?;
        }
        let path = directory.join("accounts.sqlite3");
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(0o600);
            let file = options.open(&path).map_err(storage_error)?;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .map_err(storage_error)?;
        }
        #[cfg(not(unix))]
        return Err(ApiError::unavailable(
            "private account storage requires Unix file permissions",
        ));
        let mut db = Connection::open(path).map_err(storage_error)?;
        db.busy_timeout(Duration::from_secs(5))
            .map_err(storage_error)?;
        db.execute_batch(
            "PRAGMA journal_mode=DELETE; PRAGMA synchronous=FULL;
             CREATE TABLE IF NOT EXISTS accounts (
               identity_id TEXT PRIMARY KEY, password_hash TEXT NOT NULL);
             CREATE TABLE IF NOT EXISTS sessions (
               token_hash TEXT PRIMARY KEY, identity_id TEXT NOT NULL,
               expires_at INTEGER NOT NULL);
             CREATE INDEX IF NOT EXISTS sessions_identity ON sessions(identity_id);
             CREATE TABLE IF NOT EXISTS surface_owners (
               session_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, object_id TEXT NOT NULL);",
        )
        .map_err(storage_error)?;
        // Old owners have no provable originating login. Keep their IDs reserved,
        // but never infer a credential binding from identity alone.
        let tx = db.transaction().map_err(storage_error)?;
        security::migrate_sessions(&tx)?;
        let columns: Vec<String> = {
            let mut statement = tx
                .prepare("PRAGMA table_info(surface_owners)")
                .map_err(storage_error)?;
            statement
                .query_map([], |row| row.get(1))
                .map_err(storage_error)?
                .collect::<Result<_, _>>()
                .map_err(storage_error)?
        };
        if !columns.iter().any(|column| column == "account_session") {
            tx.execute_batch(
                "ALTER TABLE surface_owners ADD COLUMN account_session TEXT;
                ALTER TABLE surface_owners ADD COLUMN retired INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(storage_error)?;
        }
        if !columns.iter().any(|column| column == "lease_expires_at") {
            // A prior host never promised liveness. Do not revive its sessions.
            tx.execute_batch("ALTER TABLE surface_owners ADD COLUMN lease_expires_at INTEGER NOT NULL DEFAULT 0;")
                .map_err(storage_error)?;
        }
        if !columns.iter().any(|column| column == "document_id") {
            tx.execute_batch("ALTER TABLE surface_owners ADD COLUMN document_id TEXT;")
                .map_err(storage_error)?;
        }
        tx.execute_batch("CREATE INDEX IF NOT EXISTS surface_account_session ON surface_owners(account_session);")
            .map_err(storage_error)?;
        tx.execute_batch("CREATE INDEX IF NOT EXISTS surface_cleanup_pending ON surface_owners(session_id) WHERE retired=0;
            CREATE INDEX IF NOT EXISTS surface_account_pending ON surface_owners(account_session, session_id) WHERE retired=0;")
            .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS host_documents (
            document_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL,
            account_session TEXT NOT NULL, object_id TEXT NOT NULL,
            epoch TEXT NOT NULL, lease_expires_at INTEGER NOT NULL,
            retired INTEGER NOT NULL DEFAULT 0);
            CREATE INDEX IF NOT EXISTS host_document_login ON host_documents(account_session);",
        )
        .map_err(storage_error)?;
        Ok(Self { db })
    }

    pub fn create_account(&mut self, identity: &str, hash: &str) -> Result<(), ApiError> {
        self.db
            .execute(
                "INSERT INTO accounts VALUES (?1, ?2)",
                params![identity, hash],
            )
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn password_hash(&self, identity: &str) -> Result<Option<String>, ApiError> {
        self.db
            .query_row(
                "SELECT password_hash FROM accounts WHERE identity_id=?1",
                [identity],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)
    }

    pub fn authenticate(&self, token: &str) -> Result<Principal, ApiError> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(ApiError::unauthorized());
        }
        self.authenticate_hash(&digest(token))
    }

    pub fn authenticate_hash(&self, hash: &str) -> Result<Principal, ApiError> {
        self.db.query_row(
            "SELECT identity_id, expires_at FROM sessions WHERE token_hash=?1 AND expires_at > ?2",
            params![hash, OffsetDateTime::now_utc().unix_timestamp()],
            |row| Ok(Principal { identity_id: row.get(0)?, expires_at: row.get(1)?, account_session: hash.to_owned() }),
        ).optional().map_err(storage_error)?.ok_or_else(ApiError::unauthorized)
    }

    pub fn revoke(&mut self, token: &str) -> Result<(), ApiError> {
        self.db
            .execute("DELETE FROM sessions WHERE token_hash=?1", [digest(token)])
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn reserve_surface(
        &mut self,
        session: &str,
        principal: &Principal,
        object: &str,
    ) -> Result<bool, ApiError> {
        let inserted = self
            .db
            .execute(
                "INSERT OR IGNORE INTO surface_owners (session_id, identity_id, object_id, account_session, lease_expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![session, principal.identity_id, object, principal.account_session, lease_deadline(principal, now_ms())],
            )
            .map_err(storage_error)?;
        if inserted != 1 && !self.owns_surface(session, principal, Some(object), false)? {
            return Err(ApiError::forbidden());
        }
        Ok(inserted == 1)
    }

    pub fn remove_surface(&mut self, session: &str) -> Result<(), ApiError> {
        self.db
            .execute("DELETE FROM surface_owners WHERE session_id=?1", [session])
            .map_err(storage_error)?;
        Ok(())
    }

    pub fn owns_surface(
        &self,
        session: &str,
        principal: &Principal,
        object: Option<&str>,
        allow_retired: bool,
    ) -> Result<bool, ApiError> {
        let found: Option<String> = self
            .db
            .query_row(
                "SELECT object_id FROM surface_owners WHERE session_id=?1 AND identity_id=?2 AND account_session=?3 AND (?4 OR (retired=0 AND lease_expires_at>?5))",
                params![session, principal.identity_id, principal.account_session, allow_retired, now_ms()],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage_error)?;
        Ok(found.is_some_and(|found| object.is_none_or(|object| object == found)))
    }

    pub fn has_surface(&self, session: &str) -> Result<bool, ApiError> {
        self.db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM surface_owners WHERE session_id=?1)",
                [session],
                |row| row.get(0),
            )
            .map_err(storage_error)
    }

    pub fn has_surface_document(&self, session: &str) -> Result<bool, ApiError> {
        self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM surface_owners WHERE session_id=?1 AND document_id IS NOT NULL)",
            [session], |row| row.get(0),
        ).map_err(storage_error)
    }

    pub fn bind_surface_document(
        &mut self,
        session: &str,
        principal: &Principal,
        document: &str,
    ) -> Result<(), ApiError> {
        self.authenticate_hash(&principal.account_session)?;
        let updated = self.db.execute(
            "UPDATE surface_owners SET document_id=?1 WHERE session_id=?2 AND identity_id=?3 AND account_session=?4 AND (document_id IS NULL OR document_id=?1) AND retired=0 AND lease_expires_at>?5 AND EXISTS(SELECT 1 FROM sessions WHERE token_hash=?4 AND identity_id=?3 AND expires_at>?6)",
            params![document, session, principal.identity_id, principal.account_session, now_ms(), OffsetDateTime::now_utc().unix_timestamp()],
        ).map_err(storage_error)?;
        if updated != 1 {
            self.authenticate_hash(&principal.account_session)?;
            if !self.owns_surface(session, principal, None, false)? {
                return Err(ApiError::forbidden());
            }
            return Err(ApiError::conflict(
                "Surface session already bound to another document",
            ));
        }
        Ok(())
    }

    pub fn matches_surface_document(
        &self,
        session: &str,
        principal: &Principal,
        document: &str,
    ) -> Result<bool, ApiError> {
        self.db.query_row(
            "SELECT EXISTS(SELECT 1 FROM surface_owners o JOIN sessions s ON s.token_hash=o.account_session AND s.identity_id=o.identity_id WHERE o.session_id=?1 AND o.identity_id=?2 AND o.account_session=?3 AND o.document_id=?4 AND o.retired=0 AND o.lease_expires_at>?5 AND s.expires_at>?6)",
            params![session, principal.identity_id, principal.account_session, document, now_ms(), OffsetDateTime::now_utc().unix_timestamp()],
            |row| row.get(0),
        ).map_err(storage_error)
    }

    pub fn renew_surface(
        &mut self,
        session: &str,
        principal: &Principal,
    ) -> Result<(i64, i64), ApiError> {
        let now = now_ms();
        let deadline = lease_deadline(principal, now);
        if deadline <= now + 2 {
            return Err(ApiError::unauthorized());
        }
        let updated = self.db.execute(
            "UPDATE surface_owners SET lease_expires_at=?1 WHERE session_id=?2 AND identity_id=?3 AND account_session=?4 AND retired=0 AND lease_expires_at>?5",
            params![deadline, session, principal.identity_id, principal.account_session, now],
        ).map_err(storage_error)?;
        if updated != 1 {
            return Err(ApiError::forbidden());
        }
        Ok((now, deadline))
    }

    pub fn surface_cleanup_batch(&self, after: &str) -> Result<Vec<(String, bool)>, ApiError> {
        let mut statement = self
            .db
            .prepare(
                "SELECT o.session_id, (s.token_hash IS NULL OR s.expires_at<=?1 OR o.lease_expires_at<=?4)
             FROM surface_owners o INDEXED BY surface_cleanup_pending LEFT JOIN sessions s
             ON o.account_session=s.token_hash AND o.identity_id=s.identity_id
             WHERE o.retired=0 AND o.session_id>?2 ORDER BY o.session_id LIMIT ?3",
            )
            .map_err(storage_error)?;
        statement
            .query_map(
                params![
                    OffsetDateTime::now_utc().unix_timestamp(),
                    after,
                    CLEANUP_BATCH as i64,
                    now_ms()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(storage_error)?
            .collect::<Result<_, _>>()
            .map_err(storage_error)
    }

    pub fn origin_surface_batch(&self, account_session: &str) -> Result<Vec<String>, ApiError> {
        let mut statement = self
            .db
            .prepare(
                "SELECT session_id FROM surface_owners INDEXED BY surface_account_pending
             WHERE retired=0 AND account_session=?1 ORDER BY session_id LIMIT ?2",
            )
            .map_err(storage_error)?;
        statement
            .query_map(params![account_session, CLEANUP_BATCH as i64], |row| {
                row.get(0)
            })
            .map_err(storage_error)?
            .collect::<Result<_, _>>()
            .map_err(storage_error)
    }

    pub fn retire_surface(&mut self, session: &str) -> Result<(), ApiError> {
        self.db
            .execute(
                "UPDATE surface_owners SET retired=1 WHERE session_id=?1",
                [session],
            )
            .map_err(storage_error)?;
        Ok(())
    }
}

const SURFACE_LEASE_MS: i64 = 60_000;

fn now_ms() -> i64 {
    (OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

fn lease_deadline(principal: &Principal, now: i64) -> i64 {
    (now + SURFACE_LEASE_MS).min(principal.expires_at.saturating_mul(1000))
}

pub(crate) fn digest(token: &str) -> String {
    blake3::hash(token.as_bytes()).to_hex().to_string()
}

fn storage_error(_: impl std::fmt::Display) -> ApiError {
    // SQLite diagnostics can contain SQL values; never expose credential material.
    ApiError::internal("account storage operation failed")
}

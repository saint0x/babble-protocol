use super::{ApiError, AuthStore, Principal, digest, random_token, storage_error};
use crate::auth::{AccountSessionInfo, format_expiry};
use rusqlite::{OptionalExtension, Transaction, TransactionBehavior, params};
use time::OffsetDateTime;

pub(super) fn migrate_sessions(tx: &Transaction<'_>) -> Result<(), ApiError> {
    let columns = tx
        .prepare("PRAGMA table_info(sessions)")
        .map_err(storage_error)?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(storage_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(storage_error)?;
    if columns.iter().any(|name| name == "public_id") {
        return Ok(());
    }
    // Rebuild atomically: credentials and their expiry remain byte-for-byte intact.
    // Legacy creation times are unknowable, and must not be fabricated.
    tx.execute_batch("CREATE TABLE sessions_security (
        token_hash TEXT PRIMARY KEY, identity_id TEXT NOT NULL, expires_at INTEGER NOT NULL,
        public_id TEXT NOT NULL UNIQUE CHECK(length(public_id)=72 AND substr(public_id,1,8)='account_' AND substr(public_id,9) NOT GLOB '*[^0-9a-f]*'),
        created_at INTEGER);").map_err(storage_error)?;
    {
        let mut statement = tx
            .prepare("SELECT token_hash, identity_id, expires_at FROM sessions")
            .map_err(storage_error)?;
        let mut rows = statement.query([]).map_err(storage_error)?;
        while let Some(row) = rows.next().map_err(storage_error)? {
            tx.execute(
                "INSERT INTO sessions_security VALUES (?1,?2,?3,?4,NULL)",
                params![
                    row.get::<_, String>(0).map_err(storage_error)?,
                    row.get::<_, String>(1).map_err(storage_error)?,
                    row.get::<_, i64>(2).map_err(storage_error)?,
                    public_id()?
                ],
            )
            .map_err(storage_error)?;
        }
    }
    tx.execute_batch(
        "DROP TABLE sessions; ALTER TABLE sessions_security RENAME TO sessions;
        CREATE INDEX sessions_identity ON sessions(identity_id);",
    )
    .map_err(storage_error)
}

fn public_id() -> Result<String, ApiError> {
    Ok(format!("account_{}", random_token()?))
}

fn now() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

fn require_live(tx: &Transaction<'_>, principal: &Principal) -> Result<(), ApiError> {
    let live: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM sessions WHERE token_hash=?1 AND identity_id=?2 AND expires_at>?3)",
        params![principal.account_session, principal.identity_id, now()], |row| row.get(0)).map_err(storage_error)?;
    if live {
        Ok(())
    } else {
        Err(ApiError::unauthorized())
    }
}

fn same_password(tx: &Transaction<'_>, identity: &str, observed: &str) -> Result<bool, ApiError> {
    let current: Option<String> = tx
        .query_row(
            "SELECT password_hash FROM accounts WHERE identity_id=?1",
            [identity],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    Ok(current.as_deref() == Some(observed))
}

fn insert_session(tx: &Transaction<'_>, identity: &str) -> Result<(String, i64), ApiError> {
    let created = now();
    let expires = created + 7 * 24 * 60 * 60;
    let token = random_token()?;
    let id = public_id()?;
    tx.execute("DELETE FROM sessions WHERE expires_at<=?1", [created])
        .map_err(storage_error)?;
    tx.execute("DELETE FROM sessions WHERE identity_id=?1 AND token_hash NOT IN
        (SELECT token_hash FROM sessions WHERE identity_id=?1 ORDER BY expires_at DESC, rowid DESC LIMIT 15)", [identity]).map_err(storage_error)?;
    tx.execute("INSERT INTO sessions (token_hash,identity_id,expires_at,public_id,created_at) VALUES (?1,?2,?3,?4,?5)",
        params![digest(&token), identity, expires, id, created]).map_err(storage_error)?;
    Ok((token, expires))
}

impl AuthStore {
    /// Password verification happens outside locks; issuance must compare its snapshot.
    pub fn issue_verified(
        &mut self,
        identity: &str,
        observed: &str,
    ) -> Result<(String, i64), ApiError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        if !same_password(&tx, identity, observed)? {
            return Err(ApiError::unauthorized());
        }
        let result = insert_session(&tx, identity)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    #[cfg(test)]
    pub fn issue(&mut self, identity: &str) -> Result<(String, i64), ApiError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let result = insert_session(&tx, identity)?;
        tx.commit().map_err(storage_error)?;
        Ok(result)
    }

    pub fn sessions(&mut self, principal: &Principal) -> Result<Vec<AccountSessionInfo>, ApiError> {
        let tx = self.db.transaction().map_err(storage_error)?;
        require_live(&tx, principal)?;
        let mut statement = tx.prepare("SELECT public_id, created_at, expires_at, token_hash=?2 FROM sessions
            WHERE identity_id=?1 AND expires_at>?3 ORDER BY (token_hash=?2) DESC, created_at DESC, rowid DESC LIMIT 16").map_err(storage_error)?;
        let rows = statement
            .query_map(
                params![principal.identity_id, principal.account_session, now()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<i64>>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, bool>(3)?,
                    ))
                },
            )
            .map_err(storage_error)?;
        rows.map(|row| {
            let (id, created, expires, current) = row.map_err(storage_error)?;
            Ok(AccountSessionInfo {
                id,
                created_at: created.map(format_expiry).transpose()?,
                expires_at: format_expiry(expires)?,
                current,
            })
        })
        .collect()
    }

    /// `None` revokes other logins. A public ID revokes exactly one owned login.
    pub fn revoke_sessions(
        &mut self,
        principal: &Principal,
        id: Option<&str>,
    ) -> Result<Vec<String>, ApiError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        require_live(&tx, principal)?;
        let hashes = {
            let mut statement = tx
                .prepare(
                    "SELECT token_hash FROM sessions WHERE identity_id=?1 AND
                ((?2 IS NULL AND token_hash!=?3) OR public_id=?2)",
                )
                .map_err(storage_error)?;
            statement
                .query_map(
                    params![principal.identity_id, id, principal.account_session],
                    |row| row.get::<_, String>(0),
                )
                .map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?
        };
        for hash in &hashes {
            tx.execute("DELETE FROM sessions WHERE token_hash=?1", [hash])
                .map_err(storage_error)?;
        }
        tx.commit().map_err(storage_error)?;
        Ok(hashes)
    }

    pub fn change_password(
        &mut self,
        principal: &Principal,
        observed: &str,
        replacement: Option<&str>,
    ) -> Result<Vec<String>, ApiError> {
        let tx = self
            .db
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        require_live(&tx, principal)?;
        if !same_password(&tx, &principal.identity_id, observed)? {
            return Err(ApiError::forbidden());
        }
        let replacement = replacement.ok_or_else(ApiError::forbidden)?;
        let hashes = {
            let mut statement = tx
                .prepare("SELECT token_hash FROM sessions WHERE identity_id=?1")
                .map_err(storage_error)?;
            statement
                .query_map([&principal.identity_id], |row| row.get::<_, String>(0))
                .map_err(storage_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(storage_error)?
        };
        tx.execute(
            "UPDATE accounts SET password_hash=?1 WHERE identity_id=?2 AND password_hash=?3",
            params![replacement, principal.identity_id, observed],
        )
        .map_err(storage_error)?;
        tx.execute(
            "DELETE FROM sessions WHERE identity_id=?1",
            [&principal.identity_id],
        )
        .map_err(storage_error)?;
        tx.commit().map_err(storage_error)?;
        Ok(hashes)
    }
}

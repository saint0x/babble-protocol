use super::{ApiError, AuthStore, Principal, storage_error};
use rusqlite::{OptionalExtension, params};
use time::OffsetDateTime;

impl AuthStore {
    pub fn register_host_document(
        &mut self,
        principal: &Principal,
        document: &str,
        object: &str,
        epoch: &str,
    ) -> Result<i64, ApiError> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let deadline = (now + 60).min(principal.expires_at);
        let existing: Option<(String, String, String, String, i64, bool)> = self
            .db
            .query_row(
                "SELECT identity_id, account_session, object_id, epoch, lease_expires_at, retired
             FROM host_documents WHERE document_id=?1",
                [document],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )
            .optional()
            .map_err(storage_error)?;
        if let Some((actor, login, bound_object, bound_epoch, expires, retired)) = existing {
            if actor != principal.identity_id
                || login != principal.account_session
                || bound_object != object
                || bound_epoch != epoch
                || retired
                || expires <= now
            {
                return Err(ApiError::conflict(
                    "host document binding is immutable or retired",
                ));
            }
            self.db
                .execute(
                    "UPDATE host_documents SET lease_expires_at=?2 WHERE document_id=?1",
                    params![document, deadline],
                )
                .map_err(storage_error)?;
        } else {
            let count: u32 = self
                .db
                .query_row(
                    "SELECT COUNT(*) FROM host_documents WHERE account_session=?1
                AND retired=0 AND lease_expires_at>?2",
                    params![principal.account_session, now],
                    |row| row.get(0),
                )
                .map_err(storage_error)?;
            if count >= 16 {
                return Err(ApiError::rate_limited());
            }
            self.db
                .execute(
                    "INSERT INTO host_documents
                (document_id,identity_id,account_session,object_id,epoch,lease_expires_at)
                VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        document,
                        principal.identity_id,
                        principal.account_session,
                        object,
                        epoch,
                        deadline
                    ],
                )
                .map_err(storage_error)?;
        }
        Ok(deadline)
    }

    pub fn require_host_document(
        &self,
        principal: &Principal,
        document: &str,
        object: &str,
        epoch: &str,
    ) -> Result<(), ApiError> {
        let valid: bool = self
            .db
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM host_documents
            WHERE document_id=?1 AND identity_id=?2 AND account_session=?3 AND object_id=?4
            AND epoch=?5 AND retired=0 AND lease_expires_at>?6)",
                params![
                    document,
                    principal.identity_id,
                    principal.account_session,
                    object,
                    epoch,
                    OffsetDateTime::now_utc().unix_timestamp()
                ],
                |row| row.get(0),
            )
            .map_err(storage_error)?;
        if valid {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }

    pub fn retire_host_document(
        &mut self,
        principal: &Principal,
        document: &str,
    ) -> Result<(), ApiError> {
        let changed = self
            .db
            .execute(
                "UPDATE host_documents SET retired=1 WHERE document_id=?1
            AND identity_id=?2 AND account_session=?3",
                params![document, principal.identity_id, principal.account_session],
            )
            .map_err(storage_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ApiError::forbidden())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_host_document_lease_is_immutable_bounded_and_cannot_revive() {
        let root = std::env::temp_dir().join(format!(
            "babel-host-documents-{}",
            crate::auth::random_token().unwrap()
        ));
        let mut store = AuthStore::open(&root).unwrap();
        let (token, _) = store.issue("actor").unwrap();
        let (other, _) = store.issue("actor").unwrap();
        let principal = store.authenticate(&token).unwrap();
        let other = store.authenticate(&other).unwrap();
        store
            .register_host_document(&principal, "document", "controller", "epoch")
            .unwrap();
        store
            .register_host_document(&principal, "document", "controller", "epoch")
            .unwrap();
        assert!(
            store
                .register_host_document(&other, "document", "controller", "epoch")
                .is_err()
        );
        assert!(
            store
                .register_host_document(&principal, "document", "changed", "epoch")
                .is_err()
        );
        assert!(
            store
                .register_host_document(&principal, "document", "controller", "restart")
                .is_err()
        );
        for index in 1..16 {
            store
                .register_host_document(
                    &principal,
                    &format!("document-{index}"),
                    "controller",
                    "epoch",
                )
                .unwrap();
        }
        assert!(
            store
                .register_host_document(&principal, "over-limit", "controller", "epoch")
                .is_err()
        );
        store
            .db
            .execute(
                "UPDATE host_documents SET lease_expires_at=0 WHERE document_id='document'",
                [],
            )
            .unwrap();
        assert!(
            store
                .require_host_document(&principal, "document", "controller", "epoch")
                .is_err()
        );
        assert!(
            store
                .register_host_document(&principal, "document", "controller", "epoch")
                .is_err()
        );
        store
            .register_host_document(&principal, "replacement", "controller", "epoch")
            .unwrap();
        store
            .retire_host_document(&principal, "replacement")
            .unwrap();
        assert!(
            store
                .register_host_document(&principal, "replacement", "controller", "epoch")
                .is_err()
        );
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }
}

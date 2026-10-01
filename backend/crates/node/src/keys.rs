use super::*;
use std::{fs, io::Write};

impl<P: JudgmentProvider> LocalNode<P> {
    // Key versions are immutable. Persist before the transition event so a crash
    // always leaves a key matching whichever transition reached the event store.
    pub(crate) fn persist_signing_key(&self, key: &Keypair) -> Result<()> {
        let root = self.store.root().join("signing_keys");
        fs::create_dir_all(&root).map_err(key_error)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(key_error)?;
        }
        let path = root.join(&key.public_key().bytes);
        if path.exists() {
            let existing = fs::read_to_string(&path).map_err(key_error)?;
            if existing != key.ed25519_secret_hex() {
                return Err(babble_types::Error::Signature);
            }
            return Ok(());
        }
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let temporary = root.join(format!("{}.pending", key.public_key().bytes));
        let mut file = options.open(&temporary).map_err(key_error)?;
        file.write_all(key.ed25519_secret_hex().as_bytes())
            .map_err(key_error)?;
        file.sync_all().map_err(key_error)?;
        fs::rename(temporary, path).map_err(key_error)?;
        fs::File::open(root)
            .and_then(|dir| dir.sync_all())
            .map_err(key_error)?;
        Ok(())
    }

    pub(crate) fn restore_signing_keys(&mut self) -> Result<()> {
        for identity in self.store.list_identities()? {
            let current = self.state.signing_identity(&identity.id)?;
            let path = self
                .store
                .root()
                .join("signing_keys")
                .join(&current.public_key.bytes);
            match fs::read_to_string(path) {
                Ok(secret) => {
                    let key = Keypair::from_ed25519_secret_hex(&secret)?;
                    self.attach_signing_keypair(&identity.id, key)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(key_error(error)),
            }
        }
        Ok(())
    }
}

fn key_error(error: std::io::Error) -> babble_types::Error {
    babble_types::Error::Conflict(format!("signing key storage: {error}"))
}

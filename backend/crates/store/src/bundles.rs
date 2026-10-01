use crate::{BlobReadError, FileStore};
use babble_identity::Identity;
use babble_object::{Object, SurfaceRole, bundle::BundleFile};
use babble_types::{Error, Hash, ObjectId, Result};
use std::{collections::BTreeMap, sync::Arc};

/// Immutable bytes verified against an Object and a trusted signing key. Construction is
/// private: a deserialized claim of readiness is never a verification receipt.
#[derive(Clone, Debug)]
pub struct VerifiedBundle {
    object_id: ObjectId,
    role: SurfaceRole,
    manifest_hash: Hash,
    entry_path: String,
    files: BTreeMap<String, VerifiedBundleFile>,
}

#[derive(Clone, Debug)]
pub struct VerifiedBundleFile {
    descriptor: BundleFile,
    bytes: Arc<[u8]>,
}

impl VerifiedBundle {
    pub fn object_id(&self) -> &ObjectId {
        &self.object_id
    }
    pub fn role(&self) -> &SurfaceRole {
        &self.role
    }
    pub fn manifest_hash(&self) -> &Hash {
        &self.manifest_hash
    }
    pub fn entry_path(&self) -> &str {
        &self.entry_path
    }
    pub fn files(&self) -> impl Iterator<Item = &VerifiedBundleFile> {
        self.files.values()
    }
    /// Logical paths are exact manifest keys, not URLs or filesystem paths.
    pub fn file(&self, path: &str) -> Option<&VerifiedBundleFile> {
        self.files.get(path)
    }
}

impl VerifiedBundleFile {
    pub fn descriptor(&self) -> &BundleFile {
        &self.descriptor
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

impl FileStore {
    /// Verify already-materialized blobs, including locally cached external
    /// resources. This operation performs no acquisition and no browser admission.
    /// `author` must be the trusted signing identity at the Object's creation
    /// time. LocalNode resolves that key through its verified transition history.
    pub fn verify_surface_bundle(
        &self,
        object: &Object,
        author: &Identity,
        role: SurfaceRole,
    ) -> Result<VerifiedBundle> {
        self.check_ready()?;
        object.verify(author)?;
        let mut surfaces = object
            .surfaces
            .iter()
            .filter(|surface| surface.role == role);
        let surface = surfaces
            .next()
            .ok_or_else(|| Error::NotFound("bundle Surface role".into()))?;
        if surfaces.next().is_some() {
            return Err(Error::Conflict("bundle Surface role is ambiguous".into()));
        }
        let manifest = surface.bundle.as_ref().ok_or_else(|| {
            Error::Conflict("Surface does not declare an executable bundle".into())
        })?;
        manifest.validate_surface(surface)?;
        let manifest_hash = manifest.hash()?;
        let mut files = BTreeMap::new();
        let mut blobs: BTreeMap<Hash, Arc<[u8]>> = BTreeMap::new();
        for descriptor in &manifest.files {
            let bytes = if let Some(bytes) = blobs.get(&descriptor.integrity) {
                Arc::clone(bytes)
            } else {
                let max_bytes = usize::try_from(descriptor.size_bytes)
                    .map_err(|_| Error::Conflict("bundle file size exceeds host limits".into()))?;
                let bytes = self
                    .get_blob_bounded(&descriptor.integrity, max_bytes)
                    .map_err(|error| match error {
                        BlobReadError::TooLarge { .. } => Error::Conflict(format!(
                            "bundle file exceeds committed size: {}",
                            descriptor.path
                        )),
                        BlobReadError::Storage(error) => error,
                    })?
                    .ok_or_else(|| {
                        Error::NotFound(format!(
                            "bundle file has not been materialized: {}",
                            descriptor.path
                        ))
                    })?;
                if bytes.len() != max_bytes {
                    return Err(Error::Conflict(format!(
                        "bundle file does not match committed size: {}",
                        descriptor.path
                    )));
                }
                let bytes: Arc<[u8]> = Arc::from(bytes);
                blobs.insert(descriptor.integrity.clone(), Arc::clone(&bytes));
                bytes
            };
            files.insert(
                descriptor.path.clone(),
                VerifiedBundleFile {
                    descriptor: descriptor.clone(),
                    bytes,
                },
            );
        }
        Ok(VerifiedBundle {
            object_id: object.id.clone(),
            role,
            manifest_hash,
            entry_path: manifest.entry_path.clone(),
            files,
        })
    }
}

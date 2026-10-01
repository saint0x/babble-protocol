use crate::{BlobUploadReport, CliError};
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_object::bundle::{MAX_BUNDLE_BYTES, MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_MANIFEST_BYTES};
use babble_types::Hash;
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub const MAX_INPUT_BYTES: u64 = MAX_BUNDLE_MANIFEST_BYTES as u64;
pub const MAX_FILE_BYTES: u64 = MAX_BUNDLE_FILE_BYTES;
pub const MAX_BUILD_BYTES: u64 = MAX_BUNDLE_BYTES;

#[derive(Clone, Debug)]
struct Artifact {
    path: PathBuf,
    bytes: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct CapturedArtifacts {
    blobs: BTreeMap<Hash, Artifact>,
    paths: BTreeMap<PathBuf, Hash>,
    total_bytes: u64,
}

impl CapturedArtifacts {
    pub fn capture(&mut self, path: &Path) -> Result<(Hash, u64), CliError> {
        let path = path
            .canonicalize()
            .map_err(|err| CliError::Io(format!("resolve {}: {err}", path.display())))?;
        if let Some(hash) = self.paths.get(&path) {
            return Ok((hash.clone(), self.blobs[hash].bytes.len() as u64));
        }
        let limit = MAX_FILE_BYTES.min(MAX_BUILD_BYTES - self.total_bytes);
        let bytes = read_bounded(&path, limit)?;
        self.total_bytes += bytes.len() as u64;
        let hash = Hash::from_bytes(&bytes);
        let size = bytes.len() as u64;
        self.paths.insert(path.clone(), hash.clone());
        self.blobs
            .entry(hash.clone())
            .or_insert(Artifact { path, bytes });
        Ok((hash, size))
    }

    pub fn upload(
        &self,
        node: &LocalNode<LocalProvider>,
    ) -> Result<Vec<BlobUploadReport>, CliError> {
        self.blobs
            .iter()
            .map(|(hash, artifact)| {
                let stored = node.store().put_blob(&artifact.bytes)?;
                if stored != *hash {
                    return Err(CliError::Invalid(format!(
                        "stored blob hash differs from captured hash {hash}"
                    )));
                }
                Ok(BlobUploadReport {
                    hash: stored,
                    path: artifact.path.clone(),
                    bytes: artifact.bytes.len() as u64,
                })
            })
            .collect()
    }
}

pub fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, CliError> {
    let file =
        File::open(path).map_err(|err| CliError::Io(format!("open {}: {err}", path.display())))?;
    let metadata = file
        .metadata()
        .map_err(|err| CliError::Io(format!("stat {}: {err}", path.display())))?;
    if !metadata.is_file() {
        return Err(CliError::Invalid(format!(
            "input must be a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(CliError::Invalid(format!(
            "input exceeds {limit} byte limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| CliError::Io(format!("read {}: {err}", path.display())))?;
    if bytes.len() as u64 > limit {
        return Err(CliError::Invalid(format!(
            "input exceeds {limit} byte limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

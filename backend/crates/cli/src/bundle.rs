use crate::{CliError, artifacts::CapturedArtifacts};
use babble_object::bundle::{
    BUNDLE_VERSION, BundleFile, BundleFileKind, BundleManifest, MAX_BUNDLE_FILES,
};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// CLI authoring input names local, already-built files. Signed inventories are output only.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBundle {
    entry_path: String,
    files: Vec<LocalBundleFile>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalBundleFile {
    path: String,
    file: PathBuf,
    media_type: String,
    kind: BundleFileKind,
}

impl LocalBundle {
    pub fn capture(
        self,
        base: &Path,
        artifacts: &mut CapturedArtifacts,
    ) -> Result<BundleManifest, CliError> {
        if self.files.is_empty() || self.files.len() > MAX_BUNDLE_FILES {
            return Err(CliError::Invalid(format!(
                "bundle requires between 1 and {MAX_BUNDLE_FILES} files"
            )));
        }
        let mut paths = BTreeSet::new();
        for file in &self.files {
            if !paths.insert(&file.path) {
                return Err(CliError::Invalid(format!(
                    "duplicate bundle path: {}",
                    file.path
                )));
            }
        }
        if !paths.contains(&self.entry_path) {
            return Err(CliError::Invalid(
                "bundle entry_path must name a declared file".into(),
            ));
        }
        let mut files = self
            .files
            .into_iter()
            .map(|input| {
                let (integrity, size_bytes) = artifacts.capture(&base.join(input.file))?;
                Ok(BundleFile {
                    path: input.path,
                    source_uri: format!("babble://blobs/{integrity}"),
                    integrity,
                    size_bytes,
                    media_type: input.media_type,
                    kind: input.kind,
                })
            })
            .collect::<Result<Vec<_>, CliError>>()?;
        files.sort_by(|a, b| a.path.cmp(&b.path));
        let bundle = BundleManifest {
            version: BUNDLE_VERSION,
            entry_path: self.entry_path,
            files,
        };
        bundle.validate()?;
        Ok(bundle)
    }
}

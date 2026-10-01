//! Signed bundle inventory admission. Acquisition must separately verify every file's bytes.

use crate::{Surface, SurfaceTarget, resource_uri::ResourceUri};
use babble_types::{Canonical, Error, Hash, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use url::Url;

pub const BUNDLE_VERSION: u32 = 1;
pub const MAX_BUNDLE_FILES: usize = 256;
pub const MAX_BUNDLE_MANIFEST_BYTES: usize = 256 * 1024;
pub const MAX_BUNDLE_FILE_BYTES: u64 = 8 * 1024 * 1024;
pub const MAX_BUNDLE_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_BUNDLE_PATH_BYTES: usize = 1024;
const MAX_PATH_SEGMENT_BYTES: usize = 255;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    pub version: u32,
    pub entry_path: String,
    pub files: Vec<BundleFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BundleFile {
    pub path: String,
    pub source_uri: String,
    pub integrity: Hash,
    pub size_bytes: u64,
    pub media_type: String,
    pub kind: BundleFileKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BundleFileKind {
    Document,
    Script,
    Stylesheet,
    Asset,
    Wasm,
}

impl BundleManifest {
    pub fn validate(&self) -> Result<()> {
        if self.version != BUNDLE_VERSION {
            return Err(invalid("unsupported manifest version"));
        }
        if self.files.is_empty() || self.files.len() > MAX_BUNDLE_FILES {
            return Err(invalid("requires between 1 and 256 files"));
        }
        validate_path(&self.entry_path)?;

        // Bound input strings before URL parsing or canonical serialization allocates copies.
        let mut string_bytes = self.entry_path.len();
        for file in &self.files {
            for value in [
                file.path.as_str(),
                file.source_uri.as_str(),
                file.integrity.as_str(),
                file.media_type.as_str(),
            ] {
                string_bytes = string_bytes
                    .checked_add(value.len())
                    .filter(|total| *total <= MAX_BUNDLE_MANIFEST_BYTES)
                    .ok_or_else(|| invalid("manifest exceeds 256 KiB"))?;
            }
        }

        let mut previous_path: Option<&str> = None;
        let mut metadata_by_hash = BTreeMap::new();
        let mut total_bytes = 0u64;
        for file in &self.files {
            validate_path(&file.path)?;
            if previous_path.is_some_and(|previous| previous >= file.path.as_str()) {
                return Err(invalid(
                    "file paths must be unique and sorted lexicographically",
                ));
            }
            previous_path = Some(&file.path);
            let source = ResourceUri::parse(&file.source_uri)?;
            source.validate_integrity(&file.integrity)?;
            if source.blob_hash().is_none()
                && !Url::parse(&file.source_uri)
                    .is_ok_and(|url| matches!(url.scheme(), "https" | "http"))
            {
                return Err(invalid("file sources must be absolute acquisition URIs"));
            }
            if !file.kind.accepts_media_type(&file.media_type) {
                return Err(invalid("unsupported file MIME/kind pair"));
            }
            if file.size_bytes > MAX_BUNDLE_FILE_BYTES {
                return Err(invalid("file exceeds 8 MiB"));
            }
            // Count every logical file, including repeated content, against the materialized limit.
            total_bytes = total_bytes
                .checked_add(file.size_bytes)
                .filter(|total| *total <= MAX_BUNDLE_BYTES)
                .ok_or_else(|| invalid("total file bytes exceed 32 MiB"))?;
            let metadata = (file.media_type.as_str(), file.kind, file.size_bytes);
            if metadata_by_hash
                .insert(&file.integrity, metadata)
                .is_some_and(|previous| previous != metadata)
            {
                return Err(invalid(
                    "identical hashes require identical MIME, kind and size",
                ));
            }
        }
        let entry = self.entry_file()?;
        if entry.kind != BundleFileKind::Document || entry.media_type != "text/html" {
            return Err(invalid("entry must be a Document with text/html MIME"));
        }
        if self.canonical_bytes()?.len() > MAX_BUNDLE_MANIFEST_BYTES {
            return Err(invalid("canonical manifest exceeds 256 KiB"));
        }
        Ok(())
    }

    pub fn validate_surface(&self, surface: &Surface) -> Result<()> {
        if !matches!(surface.target, SurfaceTarget::Web | SurfaceTarget::WebGpu) {
            return Err(invalid(
                "unsupported bundle surface profile; expected Web or WebGpu",
            ));
        }
        self.validate()?;
        let entry = self.entry_file()?;
        if surface.entry != entry.source_uri || surface.integrity.as_ref() != Some(&entry.integrity)
        {
            return Err(invalid(
                "surface entry and integrity must exactly match the bundle entry",
            ));
        }
        Ok(())
    }

    /// Return the declared entry; callers must validate the inventory before acquisition.
    pub fn entry_file(&self) -> Result<&BundleFile> {
        self.files
            .iter()
            .find(|file| file.path == self.entry_path)
            .ok_or_else(|| invalid("entry_path must identify a declared file"))
    }

    /// Hash only admitted inventories with Babble's versioned canonical encoding.
    pub fn hash(&self) -> Result<Hash> {
        self.validate()?;
        self.canonical_hash()
    }
}

impl BundleFileKind {
    fn accepts_media_type(self, media_type: &str) -> bool {
        match self {
            Self::Document => media_type == "text/html",
            Self::Script => matches!(media_type, "text/javascript" | "application/javascript"),
            Self::Stylesheet => media_type == "text/css",
            Self::Wasm => media_type == "application/wasm",
            Self::Asset => matches!(
                media_type,
                "application/json"
                    | "application/octet-stream"
                    | "text/plain"
                    | "image/png"
                    | "image/jpeg"
                    | "image/gif"
                    | "image/webp"
                    | "image/avif"
                    | "image/svg+xml"
                    | "image/x-icon"
                    | "image/vnd.microsoft.icon"
                    | "font/woff"
                    | "font/woff2"
                    | "font/ttf"
                    | "font/otf"
                    | "audio/mpeg"
                    | "audio/ogg"
                    | "audio/wav"
                    | "audio/webm"
                    | "audio/mp4"
                    | "video/mp4"
                    | "video/webm"
                    | "video/ogg"
            ),
        }
    }
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty()
        || path.len() > MAX_BUNDLE_PATH_BYTES
        || !path.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'-' | b'_' | b'~')
        })
        || path.split('/').any(|segment| {
            segment.is_empty()
                || segment == "."
                || segment == ".."
                || segment.len() > MAX_PATH_SEGMENT_BYTES
        })
    {
        return Err(invalid(
            "logical paths require nonempty ASCII URL-safe segments without traversal (1024 bytes total, 255 per segment)",
        ));
    }
    Ok(())
}

fn invalid(message: &str) -> Error {
    Error::Conflict(format!("invalid bundle: {message}"))
}

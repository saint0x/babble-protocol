use babble_object::bundle::{BundleFile, BundleFileKind, BundleManifest};
use babble_types::{Canonical, Hash};
use serde_json::{Value, json};

pub(crate) fn fixture() -> serde_json::Result<Value> {
    let mut files = Vec::new();
    for (path, bytes, media_type, kind) in [
        (
            "assets/main.js",
            &b"export const version = 1;"[..],
            "text/javascript",
            BundleFileKind::Script,
        ),
        (
            "index.html",
            &b"<!doctype html><script type=module src=assets/main.js></script>"[..],
            "text/html",
            BundleFileKind::Document,
        ),
    ] {
        let integrity = Hash::from_bytes(bytes);
        files.push(BundleFile {
            path: path.into(),
            source_uri: format!("babble://blobs/{integrity}"),
            integrity,
            size_bytes: bytes.len() as u64,
            media_type: media_type.into(),
            kind,
        });
    }
    let manifest = BundleManifest {
        version: 1,
        entry_path: "index.html".into(),
        files,
    };
    let hash = manifest.hash().map_err(super::serde_error)?;
    let bytes = manifest.canonical_bytes().map_err(super::serde_error)?;
    Ok(json!({
        "version": babble_types::CANONICAL_ENCODING_VERSION,
        "sample": manifest,
        "bytes_hex": hex::encode(bytes),
        "hash": hash,
    }))
}

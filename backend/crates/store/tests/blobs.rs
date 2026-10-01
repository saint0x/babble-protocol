use babble_store::{BlobReadError, FileStore};
use babble_types::Hash;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct StoreRoot(PathBuf);
impl StoreRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babble-bounded-blobs-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed),
            )))
    }
}
impl Drop for StoreRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn bounded_blobs_preserve_exact_bytes_and_limits_across_reopen() {
    let root = StoreRoot::new();
    let store = FileStore::open(&root.0).unwrap();
    let payload = vec![7; 32_769];
    let hash = store.put_blob(&payload).unwrap();
    for store in [store, FileStore::open(&root.0).unwrap()] {
        assert_eq!(
            store.get_blob_bounded(&hash, payload.len()).unwrap(),
            Some(payload.clone())
        );
        assert_eq!(
            store.get_blob_bounded(&hash, payload.len() + 1).unwrap(),
            Some(payload.clone())
        );
        assert!(matches!(
            store.get_blob_bounded(&hash, payload.len() - 1),
            Err(BlobReadError::TooLarge { .. })
        ));
        assert!(matches!(
            store.get_blob_bounded(&hash, 0),
            Err(BlobReadError::TooLarge { max_bytes: 0 })
        ));
    }
}

#[test]
fn bounded_blobs_distinguish_missing_invalid_and_tampered_bytes() {
    let root = StoreRoot::new();
    let store = FileStore::open(&root.0).unwrap();
    let hash = store.put_blob(b"exact bytes").unwrap();
    assert!(
        store
            .get_blob_bounded(&Hash::new_unchecked("invalid"), 100)
            .is_err()
    );
    for invalid in ["g".repeat(64), hash.as_str().to_uppercase(), "-".repeat(64)] {
        let invalid = Hash::new_unchecked(invalid);
        assert!(matches!(
            store.get_blob_bounded(&invalid, 100),
            Err(BlobReadError::Storage(babble_types::Error::Canonical(_)))
        ));
        assert!(matches!(
            store.get_blob(&invalid),
            Err(babble_types::Error::Canonical(_))
        ));
    }
    assert_eq!(
        store
            .get_blob_bounded(&Hash::from_bytes(b"missing"), 100)
            .unwrap(),
        None
    );
    fs::write(root.0.join("blobs").join(hash.as_str()), b"other bytes").unwrap();
    let error = store.get_blob_bounded(&hash, 100).unwrap_err();
    assert!(matches!(error, BlobReadError::Storage(_)));
    assert!(error.to_string().contains("integrity mismatch"));
    let empty = store.put_blob(b"").unwrap();
    assert_eq!(store.get_blob_bounded(&empty, 0).unwrap(), Some(vec![]));
}

#[test]
fn bounded_blobs_reject_sparse_oversized_files_before_reading_them() {
    let root = StoreRoot::new();
    let store = FileStore::open(&root.0).unwrap();
    let hash = Hash::from_bytes(b"sparse fixture");
    let file = fs::File::create(root.0.join("blobs").join(hash.as_str())).unwrap();
    file.set_len(1_u64 << 40).unwrap();
    assert!(matches!(
        store.get_blob_bounded(&hash, 1024),
        Err(BlobReadError::TooLarge { max_bytes: 1024 })
    ));
}

#[test]
fn bounded_blobs_reject_non_files() {
    let root = StoreRoot::new();
    let store = FileStore::open(&root.0).unwrap();
    let hash = Hash::from_bytes(b"directory fixture");
    fs::create_dir(root.0.join("blobs").join(hash.as_str())).unwrap();
    assert!(
        store
            .get_blob_bounded(&hash, 1024)
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );
}

#[test]
fn trusted_native_reads_keep_their_explicit_unbounded_policy() {
    let root = StoreRoot::new();
    let store = FileStore::open(&root.0).unwrap();
    let limit = 8 * 1024 * 1024;
    let payload = vec![42; limit + 1];
    let hash = store.put_blob(&payload).unwrap();
    assert!(matches!(
        store.get_blob_bounded(&hash, limit),
        Err(BlobReadError::TooLarge { .. })
    ));
    assert_eq!(store.get_blob(&hash).unwrap(), Some(payload));
}

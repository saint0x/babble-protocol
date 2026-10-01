use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_object::{
    Object, ObjectKind, Surface, SurfaceRole, SurfaceTarget,
    bundle::{BundleFile, BundleFileKind, BundleManifest},
};
use babel_store::FileStore;
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    store: FileStore,
    author: Identity,
    key: Keypair,
    surface: Surface,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-bundle-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = FileStore::open(&root).unwrap();
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Application, "bundle-author", &key).unwrap();
        let mut files = Vec::new();
        for (path, bytes, media_type, kind) in [
            (
                "assets/copy.js",
                &b"export const value = 42;"[..],
                "text/javascript",
                BundleFileKind::Script,
            ),
            (
                "assets/main.js",
                &b"export const value = 42;"[..],
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
            let integrity = store.put_blob(bytes).unwrap();
            files.push(BundleFile {
                path: path.into(),
                source_uri: format!("babel://blobs/{integrity}"),
                integrity,
                size_bytes: bytes.len() as u64,
                media_type: media_type.into(),
                kind,
            });
        }
        let entry = files.last().unwrap();
        let surface = Surface {
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: entry.source_uri.clone(),
            integrity: Some(entry.integrity.clone()),
            bundle: Some(BundleManifest {
                version: 1,
                entry_path: "index.html".into(),
                files,
            }),
        };
        Self {
            root,
            store,
            author,
            key,
            surface,
        }
    }

    fn object(&self, surface: Surface) -> Object {
        Object::create(
            &self.author,
            ObjectKind::new("test.bundle"),
            "test.bundle.v1",
            json!({"title":"Bundle"}),
        )
        .unwrap()
        .with_surfaces(vec![surface])
        .unwrap()
        .sign(&self.author, &self.key)
        .unwrap()
    }
    fn path(&self, file: &BundleFile) -> PathBuf {
        self.root.join("blobs").join(file.integrity.as_str())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn verified_snapshot_is_complete_exact_immutable_and_bound_to_object_and_role() {
    let fixture = Fixture::new();
    let object = fixture.object(fixture.surface.clone());
    let snapshot = fixture
        .store
        .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
        .unwrap();
    assert_eq!(snapshot.object_id(), &object.id);
    assert_eq!(snapshot.role(), &SurfaceRole::Feed);
    assert_eq!(
        snapshot.manifest_hash(),
        &fixture.surface.bundle.as_ref().unwrap().hash().unwrap()
    );
    assert_eq!(snapshot.entry_path(), "index.html");
    assert_eq!(snapshot.files().count(), 3);
    let script = snapshot.file("assets/main.js").unwrap();
    let copy = snapshot.file("assets/copy.js").unwrap();
    assert!(std::ptr::eq(script.bytes().as_ptr(), copy.bytes().as_ptr()));
    let original = script.bytes().to_vec();
    fs::write(
        fixture.path(script.descriptor()),
        b"changed after verification",
    )
    .unwrap();
    assert_eq!(snapshot.file("assets/main.js").unwrap().bytes(), original);
    assert!(
        fixture
            .store
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .is_err()
    );
    for path in [
        "../index.html",
        "/index.html",
        "index.html?x=1",
        "assets%2Fmain.js",
        "missing.js",
    ] {
        assert!(snapshot.file(path).is_none());
    }
}

#[test]
fn every_file_must_be_present_and_verified_before_any_receipt_is_returned() {
    let fixture = Fixture::new();
    let object = fixture.object(fixture.surface.clone());
    let files = &fixture.surface.bundle.as_ref().unwrap().files;
    for file in [&files[0], &files[2]] {
        let path = fixture.path(file);
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        let error = fixture
            .store
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .unwrap_err();
        assert!(error.to_string().contains("not been materialized"));
        fs::write(&path, vec![b'x'; original.len()]).unwrap();
        assert!(
            fixture
                .store
                .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
                .unwrap_err()
                .to_string()
                .contains("integrity mismatch")
        );
        fs::File::create(&path)
            .unwrap()
            .set_len(1_u64 << 40)
            .unwrap();
        assert!(
            fixture
                .store
                .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
                .unwrap_err()
                .to_string()
                .contains("exceeds committed size")
        );
        fs::write(path, original).unwrap();
    }
    assert!(
        fixture
            .store
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .is_ok()
    );
}

#[test]
fn signed_sizes_are_exact_and_external_sources_require_materialized_bytes() {
    let fixture = Fixture::new();
    let mut surface = fixture.surface.clone();
    let manifest = surface.bundle.as_mut().unwrap();
    for file in manifest
        .files
        .iter_mut()
        .filter(|file| file.kind == BundleFileKind::Script)
    {
        file.size_bytes += 1;
    }
    let object = fixture.object(surface);
    assert!(
        fixture
            .store
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .unwrap_err()
            .to_string()
            .contains("does not match committed size")
    );
    let mut surface = fixture.surface.clone();
    for file in &mut surface.bundle.as_mut().unwrap().files {
        file.source_uri = format!("https://publisher.example/{}", file.path);
    }
    surface.entry = "https://publisher.example/index.html".into();
    let object = fixture.object(surface);
    let reopened = FileStore::open(&fixture.root).unwrap();
    assert!(
        reopened
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .is_ok()
    );
    fs::remove_file(fixture.path(&fixture.surface.bundle.as_ref().unwrap().files[2])).unwrap();
    assert!(
        reopened
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Feed)
            .is_err()
    );
}

#[test]
fn signature_inventory_and_unambiguous_role_are_required() {
    let fixture = Fixture::new();
    let object = fixture.object(fixture.surface.clone());
    let mut changed = object.clone();
    changed.surfaces[0].bundle.as_mut().unwrap().files[0].source_uri =
        "https://publisher.example/changed.js".into();
    assert!(
        fixture
            .store
            .verify_surface_bundle(&changed, &fixture.author, SurfaceRole::Feed)
            .is_err()
    );
    let other = Identity::create(IdentityKind::Application, "other", &Keypair::generate()).unwrap();
    assert!(
        fixture
            .store
            .verify_surface_bundle(&object, &other, SurfaceRole::Feed)
            .is_err()
    );
    assert!(
        fixture
            .store
            .verify_surface_bundle(&object, &fixture.author, SurfaceRole::Expanded)
            .is_err()
    );
    let duplicate = object
        .clone()
        .with_surfaces(vec![fixture.surface.clone(), fixture.surface.clone()])
        .unwrap()
        .sign(&fixture.author, &fixture.key)
        .unwrap();
    assert!(
        fixture
            .store
            .verify_surface_bundle(&duplicate, &fixture.author, SurfaceRole::Feed)
            .unwrap_err()
            .to_string()
            .contains("ambiguous")
    );
    let mut unsigned = object;
    unsigned.signature = None;
    assert!(
        fixture
            .store
            .verify_surface_bundle(&unsigned, &fixture.author, SurfaceRole::Feed)
            .is_err()
    );
}

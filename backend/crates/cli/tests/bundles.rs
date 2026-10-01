mod support;

use babel_authoring::ObjectDraft;
use babel_identity::{Identity, IdentityKeyScope, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_object::{
    Object,
    bundle::{BundleManifest, MAX_BUNDLE_FILE_BYTES, MAX_BUNDLE_MANIFEST_BYTES},
};
use babel_store::FileStore;
use babel_types::Hash;
use serde_json::{Value, json};
use std::{collections::BTreeSet, fs};
use support::*;

#[test]
fn bundle_binary_build_sign_publish_reopen_and_verify() {
    let fixture = Fixture::new();
    let manifest = str_path(&fixture.manifest);
    let build = ok(["build", manifest]);
    assert!(build.get("artifacts").is_none());
    assert!(build["draft"]["resources"].as_array().unwrap().is_empty());
    let bundle: BundleManifest =
        serde_json::from_value(build["draft"]["surfaces"][0]["bundle"].clone()).unwrap();
    bundle.validate().unwrap();
    let entry = bundle.entry_file().unwrap();
    assert_eq!(build["draft"]["surfaces"][0]["entry"], entry.source_uri);
    assert_eq!(
        build["draft"]["surfaces"][0]["integrity"],
        entry.integrity.as_str()
    );
    let expected: BTreeSet<_> = bundle
        .files
        .iter()
        .map(|file| file.integrity.as_str())
        .collect();
    let required: BTreeSet<_> = build["report"]["required_blobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value.as_str().unwrap())
        .collect();
    assert_eq!(required, expected);
    for file in &bundle.files {
        let bytes = fs::read(fixture.root.join("dist").join(&file.path)).unwrap();
        assert_eq!(Hash::from_bytes(&bytes), file.integrity);
        assert_eq!(file.size_bytes, bytes.len() as u64);
        assert_eq!(file.source_uri, format!("babel://blobs/{}", file.integrity));
    }

    let draft_file = fixture.root.join("draft.json");
    ok(["build", manifest, "--out", str_path(&draft_file)]);
    let draft: Value = serde_json::from_slice(&fs::read(draft_file).unwrap()).unwrap();
    assert_eq!(draft, build["draft"]);
    let (identity, key) = fixture.identity();
    let object_file = fixture.root.join("object.json");
    let signed = ok([
        "sign",
        str_path(&identity),
        str_path(&key),
        manifest,
        "--out",
        str_path(&object_file),
    ]);
    assert_eq!(
        signed["report"]["draft_hash"],
        build["report"]["draft_hash"]
    );
    let object: Object = serde_json::from_slice(&fs::read(object_file).unwrap()).unwrap();
    let author: Value = serde_json::from_slice(&fs::read(&identity).unwrap()).unwrap();
    let author: Identity = serde_json::from_value(author["identity"].clone()).unwrap();
    object.verify(&author).unwrap();
    assert_eq!(object.surfaces[0].bundle.as_ref(), Some(&bundle));
    let mut forged = object.clone();
    forged.surfaces[0].bundle.as_mut().unwrap().files[0].size_bytes += 1;
    assert!(forged.verify(&author).is_err());

    let store_root = fixture.root.join("store");
    let published = ok([
        "publish",
        str_path(&store_root),
        str_path(&identity),
        str_path(&key),
        manifest,
    ]);
    assert_eq!(
        published["report"]["draft_hash"],
        build["report"]["draft_hash"]
    );
    let uploaded: BTreeSet<_> = published["uploaded_blobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|file| file["hash"].as_str().unwrap())
        .collect();
    assert_eq!(uploaded, expected);
    let object_id = published["object"].as_str().unwrap();
    let inspected = ok(["inspect", "object", str_path(&store_root), object_id]);
    assert_eq!(inspected["object"]["surfaces"], draft["surfaces"]);
    let report = ok([
        "inspect",
        "bundle",
        str_path(&store_root),
        object_id,
        "Feed",
    ]);
    assert_eq!(report["manifest_hash"], bundle.hash().unwrap().as_str());
    assert_eq!(report["entry_path"], bundle.entry_path);
    assert_eq!(report["verified"], true);
    assert_eq!(
        report["files"].as_array().unwrap().len(),
        bundle.files.len()
    );
    let store = FileStore::open(&store_root).unwrap();
    for (file, verified) in bundle.files.iter().zip(report["files"].as_array().unwrap()) {
        let bytes = store.get_blob(&file.integrity).unwrap().unwrap();
        assert_eq!(
            bytes,
            fs::read(fixture.root.join("dist").join(&file.path)).unwrap()
        );
        assert_eq!(verified["path"], file.path);
        assert_eq!(verified["integrity"], file.integrity.as_str());
        assert_eq!(verified["size_bytes"], bytes.len() as u64);
        assert!(verified.get("bytes").is_none());
    }
    fails(
        [
            "inspect",
            "bundle",
            str_path(&store_root),
            object_id,
            "Fullscreen",
        ],
        "role",
    );
    fails(
        [
            "inspect",
            "bundle",
            str_path(&store_root),
            object_id,
            "typo",
        ],
        "unknown surface role",
    );

    let file = &bundle.files[0];
    let blob_path = store_root.join("blobs").join(file.integrity.as_str());
    fs::write(&blob_path, vec![b'!'; file.size_bytes as usize]).unwrap();
    fails(
        [
            "inspect",
            "bundle",
            str_path(&store_root),
            object_id,
            "feed",
        ],
        "integrity mismatch",
    );
    fs::remove_file(&blob_path).unwrap();
    fails(
        [
            "inspect",
            "bundle",
            str_path(&store_root),
            object_id,
            "feed",
        ],
        "not been materialized",
    );
}

#[test]
fn bundle_build_is_reproducible_across_roots_and_file_order() {
    let first = Fixture::new();
    let second = Fixture::new();
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["files"]
        .as_array_mut()
        .unwrap()
        .reverse();
    second.write(&input);
    let a = ok(["build", str_path(&first.manifest)]);
    let b = ok(["build", str_path(&second.manifest)]);
    assert_eq!(a["draft"], b["draft"]);
    assert_eq!(a["report"]["draft_hash"], b["report"]["draft_hash"]);
    assert_eq!(a["report"]["required_blobs"], b["report"]["required_blobs"]);
}

#[test]
fn bundle_rejects_ambiguous_inputs_and_invalid_inventory() {
    let fixture = Fixture::new();
    for field in ["entry", "path", "integrity"] {
        let mut input = Fixture::input();
        input["surfaces"][0][field] = json!("conflict");
        fixture.write(&input);
        fails(["build", str_path(&fixture.manifest)], "cannot be combined");
    }
    for field in ["source_uri", "integrity", "size_bytes"] {
        let mut input = Fixture::input();
        input["surfaces"][0]["bundle"]["files"][0][field] = json!("not local input");
        fixture.write(&input);
        fails(["build", str_path(&fixture.manifest)], "unknown field");
    }
    for path in [
        "../escape.css",
        "/absolute.css",
        "app//style.css",
        "app/./style.css",
        "app/%2e%2e/style.css",
        "app\\style.css",
    ] {
        let mut input = Fixture::input();
        input["surfaces"][0]["bundle"]["files"][0]["path"] = json!(path);
        fixture.write(&input);
        fails(["build", str_path(&fixture.manifest)], "logical paths");
    }
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["files"][0]["path"] = json!("app/index.html");
    fixture.write(&input);
    fails(
        ["build", str_path(&fixture.manifest)],
        "duplicate bundle path",
    );
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["entry_path"] = json!("missing.html");
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "entry_path");
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["files"][0]["file"] = json!("missing.css");
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "missing.css");
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["files"][0]["media_type"] = json!("text/html");
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "MIME/kind");
    let mut input = Fixture::input();
    input["surfaces"][0]["bundle"]["files"] = json!([]);
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "between 1 and 256");
    let mut input = Fixture::input();
    let file = input["surfaces"][0]["bundle"]["files"][0].clone();
    input["surfaces"][0]["bundle"]["files"] = json!(vec![file; 257]);
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "between 1 and 256");
}

#[test]
fn bounded_reads_cover_manifests_files_and_multi_surface_builds() {
    let fixture = Fixture::new();
    fs::File::create(&fixture.manifest)
        .unwrap()
        .set_len(MAX_BUNDLE_MANIFEST_BYTES as u64 + 1)
        .unwrap();
    fails(["build", str_path(&fixture.manifest)], "byte limit");
    fixture.write(&Fixture::input());
    fs::File::create(fixture.root.join("dist/app/scripts/main.js"))
        .unwrap()
        .set_len(MAX_BUNDLE_FILE_BYTES + 1)
        .unwrap();
    fails(["build", str_path(&fixture.manifest)], "byte limit");

    let fixture = Fixture::new();
    let mut input = Fixture::input();
    let mut surfaces = Vec::new();
    for index in 0..5 {
        let path = format!("large-{index}.html");
        fs::File::create(fixture.root.join(&path))
            .unwrap()
            .set_len(MAX_BUNDLE_FILE_BYTES)
            .unwrap();
        surfaces.push(json!({"role":"Feed", "target":"Web", "bundle": {
            "entry_path":"index.html", "files":[{"path":"index.html", "file":path, "media_type":"text/html", "kind":"document"}]
        }}));
    }
    input["surfaces"] = json!(&surfaces[..4]);
    fixture.write(&input);
    ok(["build", str_path(&fixture.manifest)]);
    input["surfaces"] = json!(surfaces);
    fixture.write(&input);
    fails(["build", str_path(&fixture.manifest)], "byte limit");
}

#[test]
fn legacy_declared_integrity_mismatch_fails_before_publish() {
    let fixture = Fixture::new();
    let mut input = Fixture::input();
    input["surfaces"] = json!([]);
    input["resources"] = json!([{"path":"dist/app/index.html", "integrity":Hash::from_bytes(b"changed"), "media_type":"text/html"}]);
    fixture.write(&input);
    let (identity, key) = fixture.identity();
    let store = fixture.root.join("must-not-exist");
    fails(
        [
            "publish",
            str_path(&store),
            str_path(&identity),
            str_path(&key),
            str_path(&fixture.manifest),
        ],
        "integrity mismatch",
    );
    assert!(!store.exists());
}

#[test]
fn inspect_bundle_uses_historical_keys_after_rotation_and_reopen() {
    let fixture = Fixture::new();
    let build = ok(["build", str_path(&fixture.manifest)]);
    let draft: ObjectDraft = serde_json::from_value(build["draft"].clone()).unwrap();
    let store_root = fixture.root.join("store");
    let mut node = LocalNode::open(&store_root, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Application, "rotating-author")
        .unwrap();
    for file in &draft.surfaces[0].bundle.as_ref().unwrap().files {
        let bytes = fs::read(fixture.root.join("dist").join(&file.path)).unwrap();
        node.store().put_blob(&bytes).unwrap();
    }
    let original = node.publish_draft(&author.id, draft.clone()).unwrap();
    node.rotate_identity_key(
        &author.id,
        IdentityKeyScope::Root,
        None,
        "CLI verification regression",
    )
    .unwrap();
    let rotated = node.publish_draft(&author.id, draft).unwrap();
    assert!(rotated.verify(&author).is_err());
    drop(node);
    for object in [original, rotated] {
        let report = ok([
            "inspect",
            "bundle",
            str_path(&store_root),
            object.id.as_str(),
            "feed",
        ]);
        assert_eq!(report["object_id"], object.id.as_str());
        assert_eq!(report["verified"], true);
        assert_eq!(report["files"].as_array().unwrap().len(), 5);
    }
}

#[test]
fn canonical_bundle_manifest_bound_applies_after_local_input_expands() {
    let fixture = Fixture::new();
    fs::write(fixture.root.join("asset.bin"), b"asset").unwrap();
    let mut input = Fixture::input();
    let mut files = vec![input["surfaces"][0]["bundle"]["files"][2].clone()];
    let prefix = std::iter::repeat_n("a".repeat(200), 4)
        .collect::<Vec<_>>()
        .join("/");
    for index in 0..255 {
        files.push(json!({"path":format!("{prefix}/file-{index}.bin"), "file":"asset.bin", "kind":"asset", "media_type":"application/octet-stream"}));
    }
    input["surfaces"][0]["bundle"]["files"] = json!(files);
    assert!(serde_json::to_vec(&input).unwrap().len() < MAX_BUNDLE_MANIFEST_BYTES);
    fixture.write(&input);
    fails(
        ["build", str_path(&fixture.manifest)],
        "manifest exceeds 256 KiB",
    );
}

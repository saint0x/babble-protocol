use super::*;
use serde_json::json;

#[test]
fn publish_uses_captured_files_after_inputs_are_replaced_and_removed() {
    let keypair = Keypair::generate();
    let author = Identity::create(IdentityKind::Application, "capture-test", &keypair).unwrap();
    let root = std::env::temp_dir().join(format!("babel-cli-capture-{}", author.id));
    fs::create_dir_all(&root).unwrap();
    let html = b"<!doctype html><script src='./app.js'></script>";
    let script = b"document.body.dataset.ready = 'true';";
    let image = b"legacy-image";
    fs::write(root.join("index.html"), html).unwrap();
    fs::write(root.join("app.js"), script).unwrap();
    fs::write(root.join("image.bin"), image).unwrap();
    let manifest_path = root.join("manifest.json");
    fs::write(&manifest_path, serde_json::to_vec(&json!({
        "kind":"babel.text", "schema":"babel.schema.text.v1",
        "payload":{"text":"Capture once", "metadata":{}},
        "resources":[{"path":"image.bin", "media_type":"application/octet-stream"}],
        "surfaces":[
            {"role":"Feed", "target":"Web", "bundle":{
                "entry_path":"index.html", "files":[
                    {"path":"index.html", "file":"index.html", "media_type":"text/html", "kind":"document"},
                    {"path":"app.js", "file":"app.js", "media_type":"text/javascript", "kind":"script"}
                ]
            }},
            {"role":"Expanded", "target":"Web", "path":"app.js"}
        ]
    })).unwrap()).unwrap();
    let output = build_manifest(&manifest_path).unwrap();
    let draft_hash = output.report.draft_hash.clone();
    let captured_bundle = output.draft.surfaces[0].bundle.clone().unwrap();
    fs::write(root.join("index.html"), b"tampered after capture").unwrap();
    fs::remove_file(root.join("app.js")).unwrap();
    fs::remove_file(root.join("image.bin")).unwrap();
    fs::write(&manifest_path, b"not even JSON anymore").unwrap();
    let signed = output
        .draft
        .build_unsigned(&author)
        .unwrap()
        .sign(&author, &keypair)
        .unwrap();
    signed.verify(&author).unwrap();
    assert_eq!(signed.surfaces[0].bundle.as_ref(), Some(&captured_bundle));
    let store_root = root.join("store");
    let report = publish_build(&store_root, author, keypair, output).unwrap();
    assert_eq!(report.report.draft_hash, draft_hash);
    assert_eq!(report.uploaded_blobs.len(), 3);
    let store = FileStore::open(&store_root).unwrap();
    for original in [html.as_slice(), script.as_slice(), image.as_slice()] {
        assert_eq!(
            store
                .get_blob(&Hash::from_bytes(original))
                .unwrap()
                .unwrap(),
            original
        );
    }
    let verified = inspect_bundle(&store_root, report.object.as_str(), SurfaceRole::Feed).unwrap();
    assert_eq!(verified.manifest_hash, captured_bundle.hash().unwrap());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_reader_rejects_non_file_inputs() {
    assert!(
        read_bounded(Path::new("."), MAX_INPUT_BYTES)
            .unwrap_err()
            .to_string()
            .contains("regular file")
    );
}

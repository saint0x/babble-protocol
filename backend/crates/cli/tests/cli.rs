use babble_graph::{EdgeOrigin, Relation};
use babble_identity::IdentityKind;
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn validate_and_build_resolve_manifest_resource_hashes() {
    let root = unique_root("manifest");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("surface.js"),
        "export default function mount() {}\n",
    )
    .unwrap();
    fs::write(root.join("image.bin"), b"image-bytes").unwrap();

    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        r#"{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "Manifest-backed object", "metadata": {}},
  "resources": [
    {"media_type": "application/octet-stream", "path": "image.bin"}
  ],
  "surfaces": [
    {"role": "Feed", "target": "Web", "entry": "surface.js", "path": "surface.js"}
  ],
  "capabilities": [
    {"id": "babble.realtime.join", "version": 1, "scope": {"room": "self"}}
  ]
}"#,
    )
    .unwrap();

    let validate = assert_ok(command(["validate", manifest.to_str().unwrap()]));
    let report = json_output(&validate);
    assert_eq!(report["kind"], "babble.text");
    assert_eq!(report["resources"].as_array().unwrap().len(), 1);
    assert_eq!(report["surfaces"].as_array().unwrap().len(), 1);
    assert_eq!(report["capabilities"][0]["id"], "babble.realtime.join");

    let draft_path = root.join("draft.json");
    let build = assert_ok(command([
        "build",
        manifest.to_str().unwrap(),
        "--out",
        draft_path.to_str().unwrap(),
    ]));
    let build_report = json_output(&build);
    assert_eq!(build_report["draft"], draft_path.to_str().unwrap());

    let draft: Value = serde_json::from_slice(&fs::read(&draft_path).unwrap()).unwrap();
    assert_eq!(
        draft["resources"][0]["uri"],
        format!(
            "babble://blobs/{}",
            draft["resources"][0]["integrity"].as_str().unwrap()
        )
    );
    assert_eq!(
        draft["surfaces"][0]["integrity"].as_str().unwrap().len(),
        64
    );
    assert_eq!(
        draft["surfaces"][0]["entry"],
        format!(
            "babble://blobs/{}",
            draft["surfaces"][0]["integrity"].as_str().unwrap()
        )
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validate_rejects_executable_surface_without_integrity_material() {
    let root = unique_root("invalid-surface");
    fs::create_dir_all(&root).unwrap();
    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        r#"{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "Missing surface integrity", "metadata": {}},
  "surfaces": [
    {"role": "Feed", "target": "Web", "entry": "surface.js"}
  ]
}"#,
    )
    .unwrap();

    let output = command(["validate", manifest.to_str().unwrap()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("requires integrity or path"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preview_reports_runtime_sandbox_and_permission_diagnostics() {
    let root = unique_root("preview");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("surface.js"),
        "export default function mount() { return 'interactive'; }\n",
    )
    .unwrap();

    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        r#"{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "Previewed Object", "metadata": {}},
  "resources": [
    {"media_type": "text/javascript", "path": "surface.js"}
  ],
  "surfaces": [
    {"role": "Feed", "target": "Web", "entry": "surface.js", "path": "surface.js"}
  ],
  "capabilities": [
    {"id": "babble.realtime.join", "version": 1, "scope": {"room": "self"}}
  ]
}"#,
    )
    .unwrap();

    let preview = assert_ok(command(["preview", manifest.to_str().unwrap()]));
    let preview = json_output(&preview);
    assert!(preview["object_id"].as_str().unwrap().starts_with("obj_"));
    assert_eq!(preview["summary"]["surface_count"], 1);
    assert_eq!(preview["summary"]["needs_permission"], 1);
    assert_eq!(preview["summary"]["blocked"], 0);
    assert_eq!(preview["surfaces"][0]["admission"], "needs_permission");
    assert_eq!(
        preview["surfaces"][0]["plan"]["sandbox"]["isolated_origin"],
        true
    );
    assert_eq!(
        preview["surfaces"][0]["plan"]["capability_decisions"][0]["status"],
        "requires_user"
    );

    let dev = assert_ok(command(["dev", manifest.to_str().unwrap()]));
    let dev = json_output(&dev);
    assert_eq!(dev["mode"], "local-dev-host");
    assert_eq!(dev["diagnostics"]["permission_prompt_count"], 1);
    assert!(
        dev["diagnostics"]["notes"][0]
            .as_str()
            .unwrap()
            .contains("permission-gated")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn preview_blocks_executable_surface_without_declared_resource() {
    let root = unique_root("preview-blocked");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("surface.js"),
        "export default function mount() {}\n",
    )
    .unwrap();

    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        r#"{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "Blocked preview Object", "metadata": {}},
  "surfaces": [
    {"role": "Feed", "target": "Web", "entry": "surface.js", "path": "surface.js"}
  ]
}"#,
    )
    .unwrap();

    let preview = assert_ok(command(["preview", manifest.to_str().unwrap()]));
    let preview = json_output(&preview);
    assert_eq!(preview["summary"]["blocked"], 1);
    assert_eq!(preview["surfaces"][0]["admission"], "blocked");
    assert!(
        preview["summary"]["blocked_reasons"][0]
            .as_str()
            .unwrap()
            .contains("surface integrity must match a declared resource")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn inspect_and_graph_read_signed_store_records() {
    let root = unique_root("store");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let source = node
        .publish_text(&alice.id, "CLI graph source Object")
        .unwrap();
    let target = node
        .publish_text(&alice.id, "CLI graph target Object")
        .unwrap();
    let edge = node
        .publish_edge(
            &alice.id,
            source.id.clone(),
            target.id.clone(),
            Relation::References,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();

    let store = assert_ok(command(["inspect", "store", root.to_str().unwrap()]));
    let store_report = json_output(&store);
    assert_eq!(store_report["counts"]["identities"], 1);
    assert_eq!(store_report["counts"]["objects"], 2);
    assert_eq!(store_report["counts"]["edges"], 1);

    let inspected = assert_ok(command([
        "inspect",
        "object",
        root.to_str().unwrap(),
        source.id.as_str(),
    ]));
    let inspected = json_output(&inspected);
    assert_eq!(inspected["verified"], true);
    assert_eq!(inspected["object"]["id"], source.id.as_str());

    let graph = assert_ok(command([
        "graph",
        "object",
        root.to_str().unwrap(),
        source.id.as_str(),
    ]));
    let graph = json_output(&graph);
    assert_eq!(graph["counts"]["outgoing"], 1);
    assert_eq!(graph["outgoing"][0]["id"], edge.id.as_str());
    assert_eq!(graph["outgoing_by_relation"]["references"], 1);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn identity_sign_and_publish_manifest_round_trip_through_store() {
    let root = unique_root("publish");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("surface.js"),
        "export default function mount() { return 'ok'; }\n",
    )
    .unwrap();
    fs::write(root.join("image.bin"), b"published-image-bytes").unwrap();

    let manifest = root.join("manifest.json");
    fs::write(
        &manifest,
        r#"{
  "kind": "babble.text",
  "schema": "babble.schema.text.v1",
  "payload": {"text": "CLI published Object", "metadata": {"source": "test"}},
  "resources": [
    {"media_type": "application/octet-stream", "path": "image.bin"}
  ],
  "surfaces": [
    {"role": "Feed", "target": "Web", "entry": "surface.js", "path": "surface.js"}
  ]
}"#,
    )
    .unwrap();

    let identity_path = root.join("identity.json");
    let key_path = root.join("key.json");
    let identity = assert_ok(command([
        "identity",
        "new",
        identity_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
        "Person",
        "alice",
    ]));
    let identity_report = json_output(&identity);
    assert!(
        identity_report["identity"]
            .as_str()
            .unwrap()
            .starts_with("id_")
    );
    assert!(identity_path.exists());
    assert!(key_path.exists());

    let object_path = root.join("object.json");
    let signed = assert_ok(command([
        "sign",
        identity_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
        manifest.to_str().unwrap(),
        "--out",
        object_path.to_str().unwrap(),
    ]));
    let signed_report = json_output(&signed);
    assert_eq!(signed_report["object"], object_path.to_str().unwrap());
    let signed_object: Value = serde_json::from_slice(&fs::read(&object_path).unwrap()).unwrap();
    assert_eq!(signed_object["kind"], "babble.text");
    assert_eq!(
        signed_object["surfaces"][0]["entry"],
        format!(
            "babble://blobs/{}",
            signed_object["surfaces"][0]["integrity"].as_str().unwrap()
        )
    );

    let store_root = root.join("store");
    let publish = assert_ok(command([
        "publish",
        store_root.to_str().unwrap(),
        identity_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
        manifest.to_str().unwrap(),
    ]));
    let publish_report = json_output(&publish);
    assert_eq!(
        publish_report["uploaded_blobs"].as_array().unwrap().len(),
        2
    );
    let object_id = publish_report["object"].as_str().unwrap();

    let store = assert_ok(command(["inspect", "store", store_root.to_str().unwrap()]));
    let store_report = json_output(&store);
    assert_eq!(store_report["counts"]["identities"], 1);
    assert_eq!(store_report["counts"]["objects"], 1);
    assert_eq!(store_report["counts"]["events"], 2);
    assert_eq!(store_report["counts"]["blobs"], 2);

    let inspected = assert_ok(command([
        "inspect",
        "object",
        store_root.to_str().unwrap(),
        object_id,
    ]));
    let inspected = json_output(&inspected);
    assert_eq!(inspected["verified"], true);
    assert_eq!(inspected["object"]["id"], object_id);

    let republish = assert_ok(command([
        "publish",
        store_root.to_str().unwrap(),
        identity_path.to_str().unwrap(),
        key_path.to_str().unwrap(),
        manifest.to_str().unwrap(),
    ]));
    let republish_report = json_output(&republish);
    assert!(
        republish_report["object"]
            .as_str()
            .unwrap()
            .starts_with("obj_")
    );

    fs::remove_dir_all(root).unwrap();
}

fn command<const N: usize>(args: [&str; N]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_babble"))
        .args(args)
        .output()
        .unwrap()
}

fn assert_ok(output: Output) -> Output {
    if !output.status.success() {
        panic!(
            "command failed\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    output
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babble-cli-{name}-{nanos}"))
}

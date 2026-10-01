use babble_capabilities::{CapabilityBroker, GrantDecision};
use babble_crypto::Keypair;
use babble_identity::{Identity, IdentityKind};
use babble_object::{
    CapabilityRequest, Object, Surface, SurfaceRole, SurfaceTarget,
    bundle::{BundleFile, BundleFileKind, BundleManifest},
};
use babble_runtime::{RuntimeAdmissionStatus, SurfaceRuntime, SurfaceSessionPlan};
use babble_store::{FileStore, VerifiedBundle};
use babble_types::{Hash, Timestamp};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    store: FileStore,
    identity: Identity,
    key: Keypair,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-runtime-bundles-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = FileStore::open(&root).unwrap();
        let key = Keypair::from_ed25519_secret_hex(&"31".repeat(32)).unwrap();
        let identity = Identity::create(IdentityKind::Application, "runtime-bundle", &key).unwrap();
        Self {
            root,
            store,
            identity,
            key,
        }
    }

    fn object(&self, target: SurfaceTarget, capabilities: Vec<CapabilityRequest>) -> Object {
        let bytes = b"<!doctype html><p>Verified</p>";
        let integrity = self.store.put_blob(bytes).unwrap();
        let source_uri = format!("babble://blobs/{integrity}");
        Object::text(&self.identity, "bundle policy")
            .unwrap()
            .with_surfaces(vec![Surface {
                role: SurfaceRole::Feed,
                target,
                entry: source_uri.clone(),
                integrity: Some(integrity.clone()),
                bundle: Some(BundleManifest {
                    version: 1,
                    entry_path: "index.html".into(),
                    files: vec![BundleFile {
                        path: "index.html".into(),
                        source_uri,
                        integrity,
                        size_bytes: bytes.len() as u64,
                        media_type: "text/html".into(),
                        kind: BundleFileKind::Document,
                    }],
                }),
            }])
            .unwrap()
            .with_capabilities(capabilities)
            .unwrap()
            .sign(&self.identity, &self.key)
            .unwrap()
    }

    fn receipt(&self, object: &Object) -> VerifiedBundle {
        self.store
            .verify_surface_bundle(object, &self.identity, SurfaceRole::Feed)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn request() -> CapabilityRequest {
    CapabilityRequest {
        id: "babble.network.fetch".into(),
        version: 1,
        scope: json!({"origins":["https://example.com"]}),
    }
}

#[test]
fn verified_policy_requires_a_receipt_and_retains_the_legacy_blocker() {
    let fixture = Fixture::new();
    let object = fixture.object(SurfaceTarget::Web, vec![]);
    let receipt = fixture.receipt(&object);
    let runtime = SurfaceRuntime::babble_default();
    let legacy = runtime
        .prepare_surface(&object, SurfaceRole::Feed, &[])
        .unwrap();
    assert_eq!(legacy.admission, RuntimeAdmissionStatus::Blocked);
    assert!(legacy.bundle_verification.is_none());
    assert!(legacy.sandbox.iframe_sandbox.is_none());
    let plan = runtime
        .prepare_verified_surface(&object, SurfaceRole::Feed, &[], &receipt)
        .unwrap();
    assert_eq!(plan.admission, RuntimeAdmissionStatus::Ready);
    assert!(plan.verified_mount.is_none());
    assert_eq!(plan.bundle_verification.as_ref().unwrap().policy_version, 1);
    assert_eq!(
        &plan.bundle_verification.as_ref().unwrap().manifest_hash,
        receipt.manifest_hash()
    );
    assert_eq!(
        plan.sandbox.iframe_sandbox.as_deref(),
        Some("allow-scripts allow-same-origin")
    );
    assert!(plan.sandbox.isolated_origin);
    assert!(!plan.sandbox.host_cookies);
    assert!(!plan.sandbox.top_navigation);
    for directive in [
        "worker-src 'none'",
        "frame-src 'none'",
        "object-src 'none'",
        "base-uri 'none'",
        "form-action 'none'",
        "connect-src 'self'",
    ] {
        assert!(plan.sandbox.csp.contains(directive));
    }
    for forbidden in [
        "unsafe-eval",
        "wasm-unsafe-eval",
        "unsafe-inline",
        "blob:",
        "data:",
    ] {
        assert!(!plan.sandbox.csp.contains(forbidden));
    }
}

#[test]
fn byte_readiness_does_not_grant_permissions_or_override_host_policy() {
    let fixture = Fixture::new();
    let object = fixture.object(SurfaceTarget::Web, vec![request()]);
    let receipt = fixture.receipt(&object);
    let broker = CapabilityBroker::babble_default();
    let runtime = SurfaceRuntime::new(broker.clone());
    let legacy = runtime
        .prepare_surface(&object, SurfaceRole::Feed, &[])
        .unwrap();
    let plan = runtime
        .prepare_verified_surface(&object, SurfaceRole::Feed, &[], &receipt)
        .unwrap();
    assert_eq!(plan.admission, RuntimeAdmissionStatus::NeedsPermission);
    assert_eq!(plan.capability_decisions, legacy.capability_decisions);
    assert_eq!(plan.budget, legacy.budget);
    assert!(runtime.start_session(plan, None).is_err());
    let grant = broker
        .issue_grant(object.id.clone(), request(), GrantDecision::Approved, None)
        .unwrap();
    assert_eq!(
        runtime
            .prepare_verified_surface(&object, SurfaceRole::Feed, &[grant.clone()], &receipt)
            .unwrap()
            .admission,
        RuntimeAdmissionStatus::Ready
    );
    let mut revoked = grant.clone();
    revoked.revoked_at = Some(Timestamp::now());
    let mut expired = grant.clone();
    expired.expires_at = Some(Timestamp::now());
    let mut unrelated = grant;
    unrelated.object_id = babble_types::ObjectId::from_hash(&Hash::from_bytes(b"other"));
    for grant in [revoked, expired, unrelated] {
        assert_ne!(
            runtime
                .prepare_verified_surface(&object, SurfaceRole::Feed, &[grant], &receipt)
                .unwrap()
                .admission,
            RuntimeAdmissionStatus::Ready
        );
    }
    for capability in [
        CapabilityRequest {
            id: "app.unavailable".into(),
            version: 1,
            scope: json!({}),
        },
        CapabilityRequest {
            version: 99,
            ..request()
        },
    ] {
        let object = fixture.object(SurfaceTarget::Web, vec![capability]);
        let receipt = fixture.receipt(&object);
        assert_eq!(
            runtime
                .prepare_verified_surface(&object, SurfaceRole::Feed, &[], &receipt)
                .unwrap()
                .admission,
            RuntimeAdmissionStatus::Blocked
        );
    }
}

#[test]
fn receipt_binding_covers_object_commitment_role_and_manifest() {
    let fixture = Fixture::new();
    let mut object = fixture.object(SurfaceTarget::Web, vec![request()]);
    let mut surfaces = object.surfaces.clone();
    let mut expanded = surfaces[0].clone();
    expanded.role = SurfaceRole::Expanded;
    surfaces.push(expanded);
    object = object
        .with_surfaces(surfaces)
        .unwrap()
        .sign(&fixture.identity, &fixture.key)
        .unwrap();
    let receipt = fixture.receipt(&object);
    let runtime = SurfaceRuntime::babble_default();
    assert!(
        runtime
            .prepare_verified_surface(&object, SurfaceRole::Expanded, &[], &receipt)
            .is_err()
    );
    let other = fixture.object(SurfaceTarget::Web, vec![]);
    assert!(
        runtime
            .prepare_verified_surface(&other, SurfaceRole::Feed, &[], &receipt)
            .is_err()
    );
    let mut changed = object.clone();
    changed.capabilities.clear();
    assert!(
        runtime
            .prepare_verified_surface(&changed, SurfaceRole::Feed, &[], &receipt)
            .is_err()
    );
    let mut changed = object.clone();
    changed.payload["text"] = json!("tampered");
    assert!(
        runtime
            .prepare_verified_surface(&changed, SurfaceRole::Feed, &[], &receipt)
            .is_err()
    );
    let mut changed = object.clone();
    changed.signature = None;
    assert!(
        runtime
            .prepare_verified_surface(&changed, SurfaceRole::Feed, &[], &receipt)
            .is_err()
    );
    let mut surfaces = object.surfaces.clone();
    surfaces[0].bundle.as_mut().unwrap().files[0].source_uri =
        "https://example.com/index.html".into();
    surfaces[0].entry = "https://example.com/index.html".into();
    let changed = object
        .with_surfaces(surfaces)
        .unwrap()
        .sign(&fixture.identity, &fixture.key)
        .unwrap();
    assert!(
        runtime
            .prepare_verified_surface(&changed, SurfaceRole::Feed, &[], &receipt)
            .is_err()
    );
}

#[test]
fn verified_policy_keeps_webgpu_and_background_capability_requirements() {
    let fixture = Fixture::new();
    let runtime = SurfaceRuntime::babble_default();
    let object = fixture.object(SurfaceTarget::WebGpu, vec![]);
    let receipt = fixture.receipt(&object);
    let plan = runtime
        .prepare_verified_surface(&object, SurfaceRole::Feed, &[], &receipt)
        .unwrap();
    assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked);
    assert!(
        plan.blocked_reasons
            .iter()
            .any(|reason| reason.contains("babble.graphics.webgpu"))
    );
    let object = fixture.object(SurfaceTarget::Web, vec![]);
    let mut surfaces = object.surfaces.clone();
    surfaces[0].role = SurfaceRole::Background;
    let object = object
        .with_surfaces(surfaces)
        .unwrap()
        .sign(&fixture.identity, &fixture.key)
        .unwrap();
    let receipt = fixture
        .store
        .verify_surface_bundle(&object, &fixture.identity, SurfaceRole::Background)
        .unwrap();
    let plan = runtime
        .prepare_verified_surface(&object, SurfaceRole::Background, &[], &receipt)
        .unwrap();
    assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked);
    assert!(
        plan.blocked_reasons
            .iter()
            .any(|reason| reason.contains("background surfaces"))
    );
}

#[test]
fn wasm_inventory_and_unsupported_targets_cannot_use_verified_web_policy() {
    let fixture = Fixture::new();
    let object = fixture.object(SurfaceTarget::Web, vec![]);
    let receipt = fixture.receipt(&object);
    let runtime = SurfaceRuntime::babble_default();
    for target in [
        SurfaceTarget::Wasm,
        SurfaceTarget::Static,
        SurfaceTarget::NativeTrusted,
    ] {
        let mut changed = object.clone();
        changed.surfaces[0].target = target;
        assert!(
            runtime
                .prepare_verified_surface(&changed, SurfaceRole::Feed, &[], &receipt)
                .is_err()
        );
    }
    let bytes = b"\0asm\x01\0\0\0";
    let integrity = fixture.store.put_blob(bytes).unwrap();
    let mut surfaces = object.surfaces.clone();
    surfaces[0].bundle.as_mut().unwrap().files.push(BundleFile {
        path: "module.wasm".into(),
        source_uri: format!("babble://blobs/{integrity}"),
        integrity,
        size_bytes: bytes.len() as u64,
        media_type: "application/wasm".into(),
        kind: BundleFileKind::Wasm,
    });
    let object = object
        .with_surfaces(surfaces)
        .unwrap()
        .sign(&fixture.identity, &fixture.key)
        .unwrap();
    let receipt = fixture.receipt(&object);
    assert!(
        runtime
            .prepare_verified_surface(&object, SurfaceRole::Feed, &[], &receipt)
            .unwrap_err()
            .to_string()
            .contains("WASM")
    );
}

#[test]
fn serialized_metadata_does_not_enable_the_legacy_path_and_old_plans_roundtrip() {
    let fixture = Fixture::new();
    let object = fixture.object(SurfaceTarget::Web, vec![]);
    let runtime = SurfaceRuntime::babble_default();
    let legacy = runtime
        .prepare_surface(&object, SurfaceRole::Feed, &[])
        .unwrap();
    let value = serde_json::to_value(&legacy).unwrap();
    assert!(value.get("bundle_verification").is_none());
    assert!(value.get("verified_mount").is_none());
    assert!(value["sandbox"].get("iframe_sandbox").is_none());
    let decoded: SurfaceSessionPlan = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    let mut forged = value;
    forged["bundle_verification"] = json!({"policy_version":1,"manifest_hash":object.surfaces[0].bundle.as_ref().unwrap().hash().unwrap()});
    let decoded: SurfaceSessionPlan = serde_json::from_value(forged).unwrap();
    assert!(runtime.start_session(decoded, None).is_err());
    assert_eq!(
        runtime
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap()
            .admission,
        RuntimeAdmissionStatus::Blocked
    );
}

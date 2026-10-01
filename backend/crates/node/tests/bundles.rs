use babel_authoring::ObjectDraft;
use babel_capabilities::{CapabilityGrant, GrantDecision};
use babel_identity::{Identity, IdentityKeyScope, IdentityKeyTransition, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{ImportBundle, LocalNode};
use babel_object::{
    CapabilityRequest, Surface, SurfaceRole, SurfaceTarget,
    bundle::{BundleFile, BundleFileKind, BundleManifest},
};
use babel_runtime::{RuntimeAdmissionStatus, SurfaceLifecycle, SurfaceSessionId};
use babel_types::{Canonical, Hash};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babel-node-bundle-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn draft(node: &LocalNode<LocalProvider>) -> ObjectDraft {
    let mut files = Vec::new();
    for (path, bytes, media_type, kind) in [
        (
            "index.html",
            &b"<!doctype html><script src=main.js></script>"[..],
            "text/html",
            BundleFileKind::Document,
        ),
        (
            "main.js",
            &b"document.body.dataset.version = 'committed';"[..],
            "text/javascript",
            BundleFileKind::Script,
        ),
    ] {
        let integrity = node.store().put_blob(bytes).unwrap();
        files.push(BundleFile {
            path: path.into(),
            source_uri: format!("babel://blobs/{integrity}"),
            integrity,
            size_bytes: bytes.len() as u64,
            media_type: media_type.into(),
            kind,
        });
    }
    let entry = &files[0];
    ObjectDraft::text("Verified bundle publication")
        .unwrap()
        .with_surface(Surface {
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: entry.source_uri.clone(),
            integrity: Some(entry.integrity.clone()),
            bundle: Some(BundleManifest {
                version: 1,
                entry_path: "index.html".into(),
                files,
            }),
        })
        .unwrap()
}

#[test]
fn publication_verification_and_reopen_resolve_the_historical_signing_key() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Application, "bundle-publisher")
        .unwrap();
    let draft = draft(&node);
    let original = node.publish_draft(&author.id, draft.clone()).unwrap();
    node.rotate_identity_key(
        &author.id,
        IdentityKeyScope::Root,
        None,
        "bundle regression",
    )
    .unwrap();
    let rotated = node.publish_draft(&author.id, draft).unwrap();
    assert!(
        rotated.verify(&author).is_err(),
        "root identity key cannot verify the rotated publication"
    );
    for object in [&original, &rotated] {
        let bundle = node
            .verify_surface_bundle(&object.id, SurfaceRole::Feed)
            .unwrap();
        assert_eq!(bundle.object_id(), &object.id);
        assert_eq!(bundle.files().count(), 2);
        assert_eq!(
            Hash::from_bytes(bundle.file("index.html").unwrap().bytes()),
            object.surfaces[0].integrity.clone().unwrap()
        );
        let plan = node.prepare_surface(&object.id, SurfaceRole::Feed).unwrap();
        assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            plan.blocked_reasons
                .iter()
                .any(|reason| reason.contains("verified bundle execution gateway"))
        );
        assert!(
            node.start_surface_session(&object.id, SurfaceRole::Feed, None)
                .is_err()
        );
        let verified = node
            .prepare_verified_surface_for_identity(
                &object.id,
                SurfaceRole::Feed,
                Some(&author.id),
                &bundle,
            )
            .unwrap();
        assert_eq!(verified.admission, RuntimeAdmissionStatus::Ready);
        assert!(verified.verified_mount.is_none());
        let id = SurfaceSessionId::from_material(object.id.as_str());
        let session = node
            .start_verified_surface_session_for_identity(
                &object.id,
                SurfaceRole::Feed,
                id.clone(),
                &author.id,
                &bundle,
            )
            .unwrap();
        assert_eq!(session.lifecycle, SurfaceLifecycle::Prefetched);
        assert_eq!(session.events.len(), 1);
        assert!(
            node.start_verified_surface_session_for_identity(
                &object.id,
                SurfaceRole::Feed,
                id.clone(),
                &author.id,
                &bundle,
            )
            .is_err()
        );
        assert_eq!(node.surface_session(&id).unwrap(), session);
        let (active, _) = node
            .transition_surface_session(&id, SurfaceLifecycle::Active, "verified activation")
            .unwrap();
        assert_eq!(active.lifecycle, SurfaceLifecycle::Active);
    }
    drop(node);
    let mut reopened = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    for object in [original, rotated] {
        let receipt = reopened
            .verify_surface_bundle(&object.id, SurfaceRole::Feed)
            .unwrap();
        assert_eq!(
            reopened
                .prepare_verified_surface_for_identity(
                    &object.id,
                    SurfaceRole::Feed,
                    Some(&author.id),
                    &receipt,
                )
                .unwrap()
                .admission,
            RuntimeAdmissionStatus::Ready
        );
        reopened
            .start_verified_surface_session_for_identity(
                &object.id,
                SurfaceRole::Feed,
                SurfaceSessionId::from_material(object.id.as_str()),
                &author.id,
                &receipt,
            )
            .unwrap();
    }
}

#[test]
fn every_local_bundle_dependency_is_required_before_publication() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Person, "bundle-missing")
        .unwrap();
    let draft = draft(&node);
    let missing = &draft.surfaces[0].bundle.as_ref().unwrap().files[1].integrity;
    assert!(draft.required_blob_hashes().contains(missing));
    fs::remove_file(root.0.join("blobs").join(missing.as_str())).unwrap();
    assert!(node.publish_draft(&author.id, draft).is_err());
    assert!(node.store().list_objects().unwrap().is_empty());
}

#[test]
fn verified_start_rechecks_identity_grants_and_revocation_after_preparation() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let alice = node
        .create_identity(IdentityKind::Person, "bundle-alice")
        .unwrap();
    let bob = node
        .create_identity(IdentityKind::Person, "bundle-bob")
        .unwrap();
    let request = CapabilityRequest {
        id: "babel.network.fetch".into(),
        version: 1,
        scope: serde_json::json!({"origins":["https://example.com"]}),
    };
    let draft = draft(&node).with_capability(request.clone()).unwrap();
    let object = node.publish_draft(&alice.id, draft).unwrap();
    let receipt = node
        .verify_surface_bundle(&object.id, SurfaceRole::Feed)
        .unwrap();
    let grant = node
        .grant_capability(&alice.id, &object.id, request, GrantDecision::Approved)
        .unwrap();
    let grant: CapabilityGrant = serde_json::from_value(grant.payload["grant"].clone()).unwrap();
    assert_eq!(
        node.prepare_verified_surface_for_identity(
            &object.id,
            SurfaceRole::Feed,
            Some(&alice.id),
            &receipt
        )
        .unwrap()
        .admission,
        RuntimeAdmissionStatus::Ready
    );
    for identity in [None, Some(&bob.id)] {
        assert_eq!(
            node.prepare_verified_surface_for_identity(
                &object.id,
                SurfaceRole::Feed,
                identity,
                &receipt
            )
            .unwrap()
            .admission,
            RuntimeAdmissionStatus::NeedsPermission
        );
    }
    let bob_id = SurfaceSessionId::from_material(b"bob-attempt");
    assert!(
        node.start_verified_surface_session_for_identity(
            &object.id,
            SurfaceRole::Feed,
            bob_id.clone(),
            &bob.id,
            &receipt
        )
        .is_err()
    );
    assert!(node.surface_session(&bob_id).is_err());
    node.revoke_capability(&alice.id, &object.id, &grant.id)
        .unwrap();
    let id = SurfaceSessionId::from_material(b"revoked-after-ready");
    assert!(
        node.start_verified_surface_session_for_identity(
            &object.id,
            SurfaceRole::Feed,
            id.clone(),
            &alice.id,
            &receipt
        )
        .is_err()
    );
    assert!(node.surface_session(&id).is_err());
}

#[test]
fn verified_sessions_reject_cross_object_role_unknown_identity_and_bad_signatures() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Application, "bundle-bindings")
        .unwrap();
    let mut draft = draft(&node);
    let mut expanded = draft.surfaces[0].clone();
    expanded.role = SurfaceRole::Expanded;
    draft = draft.with_surface(expanded).unwrap();
    let object = node.publish_draft(&author.id, draft.clone()).unwrap();
    let other = node.publish_draft(&author.id, draft).unwrap();
    let receipt = node
        .verify_surface_bundle(&object.id, SurfaceRole::Feed)
        .unwrap();
    for (object_id, role) in [
        (&other.id, SurfaceRole::Feed),
        (&object.id, SurfaceRole::Expanded),
    ] {
        assert!(
            node.prepare_verified_surface_for_identity(
                object_id,
                role.clone(),
                Some(&author.id),
                &receipt
            )
            .is_err()
        );
        assert!(
            node.start_verified_surface_session_for_identity(
                object_id,
                role,
                SurfaceSessionId::from_material(b"invalid-binding"),
                &author.id,
                &receipt
            )
            .is_err()
        );
    }
    let unknown = babel_types::IdentityId::from_hash(&Hash::from_bytes(b"unknown"));
    assert!(
        node.prepare_verified_surface_for_identity(
            &object.id,
            SurfaceRole::Feed,
            Some(&unknown),
            &receipt
        )
        .is_err()
    );
    let mut forged = object.clone();
    forged.signature = Some(babel_crypto::Keypair::generate().sign(b"untrusted"));
    assert!(
        node.import_bundle(ImportBundle {
            objects: vec![forged],
            ..Default::default()
        })
        .is_err()
    );
    assert_eq!(
        node.prepare_verified_surface_for_identity(
            &object.id,
            SurfaceRole::Feed,
            Some(&author.id),
            &receipt
        )
        .unwrap()
        .admission,
        RuntimeAdmissionStatus::Ready
    );
}

#[test]
fn verified_admission_consumes_the_immutable_receipt_without_reopening_blobs() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Application, "bundle-snapshot")
        .unwrap();
    let object = node.publish_draft(&author.id, draft(&node)).unwrap();
    let receipt = node
        .verify_surface_bundle(&object.id, SurfaceRole::Feed)
        .unwrap();
    for file in receipt.files() {
        fs::write(
            root.0
                .join("blobs")
                .join(file.descriptor().integrity.as_str()),
            b"corrupted disk",
        )
        .unwrap();
    }
    assert!(
        node.verify_surface_bundle(&object.id, SurfaceRole::Feed)
            .is_err()
    );
    let session = node
        .start_verified_surface_session_for_identity(
            &object.id,
            SurfaceRole::Feed,
            SurfaceSessionId::from_material(b"immutable"),
            &author.id,
            &receipt,
        )
        .unwrap();
    assert_eq!(session.plan.admission, RuntimeAdmissionStatus::Ready);
    assert!(session.plan.verified_mount.is_none());
    assert_eq!(
        Hash::from_bytes(receipt.file("index.html").unwrap().bytes()),
        object.surfaces[0].integrity.clone().unwrap()
    );
}

#[test]
fn verified_start_rechecks_signing_history_after_receipt_creation() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let key = babel_crypto::Keypair::generate();
    let author = Identity::create(IdentityKind::Application, "historical-authority", &key).unwrap();
    node.import_signing_identity(author.clone(), key.clone())
        .unwrap();
    let object = node.publish_draft(&author.id, draft(&node)).unwrap();
    let receipt = node
        .verify_surface_bundle(&object.id, SurfaceRole::Feed)
        .unwrap();
    assert_eq!(
        node.prepare_verified_surface_for_identity(
            &object.id,
            SurfaceRole::Feed,
            Some(&author.id),
            &receipt,
        )
        .unwrap()
        .admission,
        RuntimeAdmissionStatus::Ready
    );

    // A subsequently imported, valid transition changes the authoritative key
    // at this Object's timestamp. Its previous receipt must not authorize start.
    let next = babel_crypto::Keypair::generate();
    let mut transition = IdentityKeyTransition::create(
        author.id.clone(),
        1,
        IdentityKeyScope::Root,
        &key,
        &next,
        None,
        "historical update",
    )
    .unwrap();
    transition.effective_at = object.created_at;
    let mut commitment = serde_json::to_value(&transition).unwrap();
    commitment
        .as_object_mut()
        .unwrap()
        .remove("previous_signature");
    commitment.as_object_mut().unwrap().remove("next_signature");
    let bytes = commitment.canonical_bytes().unwrap();
    transition.previous_signature = key.sign(&bytes);
    transition.next_signature = next.sign(&bytes);
    let event = babel_state::Event::new(
        &author,
        babel_state::EventKind::IdentityKeyTransition,
        babel_state::EventTarget::Identity(author.id.clone()),
        serde_json::to_value(transition).unwrap(),
        vec![],
    )
    .unwrap()
    .sign(&author, &key)
    .unwrap();
    node.import_bundle(ImportBundle {
        events: vec![event],
        ..Default::default()
    })
    .unwrap();
    assert!(
        node.prepare_verified_surface_for_identity(
            &object.id,
            SurfaceRole::Feed,
            Some(&author.id),
            &receipt,
        )
        .is_err()
    );
    let id = SurfaceSessionId::from_material(b"changed-signing-authority");
    assert!(
        node.start_verified_surface_session_for_identity(
            &object.id,
            SurfaceRole::Feed,
            id.clone(),
            &author.id,
            &receipt,
        )
        .is_err()
    );
    assert!(node.surface_session(&id).is_err());
}

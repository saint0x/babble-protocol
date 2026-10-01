use babel_authoring::ObjectDraft;
use babel_capabilities::{CapabilityGrant, GrantDecision};
use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{ImportBundle, LocalNode};
use babel_object::{CapabilityRequest, Surface, SurfaceRole, SurfaceTarget};
use babel_runtime::{SurfaceLifecycle, SurfaceSessionId};
use babel_state::{Event, EventKind, EventTarget};
use babel_types::{IdentityId, ObjectId};
use serde_json::json;
use std::{fs, path::PathBuf};

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!(
            "babel-session-permissions-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        )))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn capability() -> CapabilityRequest {
    CapabilityRequest {
        id: "babel.storage.local".into(),
        version: 1,
        scope: json!({"namespace":"self"}),
    }
}

fn object(node: &mut LocalNode<LocalProvider>, actor: &IdentityId, text: &str) -> ObjectId {
    let blob = node
        .put_media_blob("text/html", b"<!doctype html><p>Permission lifecycle</p>")
        .unwrap();
    node.publish_draft(
        actor,
        ObjectDraft::text(text)
            .unwrap()
            .with_resource(blob.resource())
            .unwrap()
            .with_surface(Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: blob.uri,
                integrity: Some(blob.integrity),
            })
            .unwrap()
            .with_capability(capability())
            .unwrap(),
    )
    .unwrap()
    .id
}

fn approve(
    node: &mut LocalNode<LocalProvider>,
    actor: &IdentityId,
    object: &ObjectId,
) -> CapabilityGrant {
    let event = node
        .grant_capability(actor, object, capability(), GrantDecision::Approved)
        .unwrap();
    serde_json::from_value(event.payload["grant"].clone()).unwrap()
}

fn start(
    node: &mut LocalNode<LocalProvider>,
    actor: &IdentityId,
    object: &ObjectId,
    name: &str,
    lifecycle: SurfaceLifecycle,
) -> SurfaceSessionId {
    let id = SurfaceSessionId::from_material(name);
    node.start_surface_session_for_identity(object, SurfaceRole::Feed, id.clone(), actor)
        .unwrap();
    if lifecycle != SurfaceLifecycle::Prefetched {
        node.transition_surface_session(&id, SurfaceLifecycle::Active, "test activation")
            .unwrap();
        if lifecycle != SurfaceLifecycle::Active {
            node.transition_surface_session(&id, lifecycle, "test suspension")
                .unwrap();
        }
    }
    id
}

#[test]
fn revocation_evicts_every_dependent_session_without_substituting_another_grant() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let bob = node.create_identity(IdentityKind::Person, "bob").unwrap();
    let app = object(&mut node, &alice.id, "shared app");
    let other = object(&mut node, &alice.id, "unrelated app");
    let first = approve(&mut node, &alice.id, &app);
    let second = approve(&mut node, &alice.id, &app);
    approve(&mut node, &bob.id, &app);
    approve(&mut node, &alice.id, &other);
    let dependent = [
        start(
            &mut node,
            &alice.id,
            &app,
            "prefetched",
            SurfaceLifecycle::Prefetched,
        ),
        start(
            &mut node,
            &alice.id,
            &app,
            "active",
            SurfaceLifecycle::Active,
        ),
        start(
            &mut node,
            &alice.id,
            &app,
            "suspended",
            SurfaceLifecycle::Suspended,
        ),
    ];
    let selected = node
        .surface_session(&dependent[0])
        .unwrap()
        .plan
        .capability_decisions[0]
        .grant
        .as_ref()
        .unwrap()
        .id
        .clone();
    let remaining = if first.id == selected {
        second.id
    } else {
        first.id
    };
    let bob_session = start(
        &mut node,
        &bob.id,
        &app,
        "other actor",
        SurfaceLifecycle::Active,
    );
    let other_session = start(
        &mut node,
        &alice.id,
        &other,
        "other object",
        SurfaceLifecycle::Active,
    );
    assert!(node.revoke_capability(&bob.id, &app, &selected).is_err());
    assert_eq!(
        node.surface_session(&dependent[1]).unwrap().lifecycle,
        SurfaceLifecycle::Active
    );
    node.revoke_capability(&alice.id, &app, &selected).unwrap();
    for id in &dependent {
        let session = node.surface_session(id).unwrap();
        assert_eq!(session.lifecycle, SurfaceLifecycle::Evicted);
        assert_eq!(
            session.events.last().unwrap().reason,
            "Surface permission revoked, expired or unavailable"
        );
        assert!(
            node.transition_surface_session(id, SurfaceLifecycle::Active, "cannot resurrect")
                .is_err()
        );
        assert!(
            node.checkpoint_surface_state(id, json!({"must":"not write"}), "revoked")
                .is_err()
        );
        node.reconcile_surface_permissions().unwrap();
        assert_eq!(
            node.surface_session(id).unwrap().events.len(),
            session.events.len()
        );
    }
    for id in [bob_session, other_session] {
        assert_eq!(
            node.surface_session(&id).unwrap().lifecycle,
            SurfaceLifecycle::Active
        );
    }
    let replacement = start(
        &mut node,
        &alice.id,
        &app,
        "new explicit session",
        SurfaceLifecycle::Active,
    );
    assert_eq!(
        node.surface_session(&replacement)
            .unwrap()
            .plan
            .capability_decisions[0]
            .grant
            .as_ref()
            .unwrap()
            .id,
        remaining
    );
}

#[test]
fn imported_owner_revocation_is_terminal_but_foreign_revocation_is_not() {
    let root = Root::new();
    let mut node = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    let key = Keypair::generate();
    let alice = Identity::create(IdentityKind::Person, "alice", &key).unwrap();
    node.import_signing_identity(alice.clone(), key.clone())
        .unwrap();
    let foreign_key = Keypair::generate();
    let bob = Identity::create(IdentityKind::Person, "bob", &foreign_key).unwrap();
    node.import_signing_identity(bob.clone(), foreign_key.clone())
        .unwrap();
    let app = object(&mut node, &alice.id, "federated permission");
    let grant = approve(&mut node, &alice.id, &app);
    let session = start(
        &mut node,
        &alice.id,
        &app,
        "remote revocation",
        SurfaceLifecycle::Active,
    );
    let event = |actor: &Identity, signing_key: &Keypair| {
        Event::new(
            actor,
            EventKind::CapabilityRevoked,
            EventTarget::Object(app.clone()),
            json!({"grant_id":grant.id,"capability":grant.capability,"version":grant.version}),
            vec![],
        )
        .unwrap()
        .sign(actor, signing_key)
        .unwrap()
    };
    node.import_bundle(ImportBundle {
        events: vec![event(&bob, &foreign_key)],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(
        node.surface_session(&session).unwrap().lifecycle,
        SurfaceLifecycle::Active
    );
    let revoked = event(&alice, &key);
    node.import_bundle(ImportBundle {
        events: vec![revoked.clone()],
        ..Default::default()
    })
    .unwrap();
    let retired = node.surface_session(&session).unwrap();
    assert_eq!(retired.lifecycle, SurfaceLifecycle::Evicted);
    node.import_bundle(ImportBundle {
        events: vec![revoked],
        ..Default::default()
    })
    .unwrap();
    assert_eq!(node.surface_session(&session).unwrap(), retired);
    drop(node);
    let mut reopened = LocalNode::open(&root.0, LocalProvider::default()).unwrap();
    assert!(
        reopened
            .start_surface_session_for_identity(
                &app,
                SurfaceRole::Feed,
                SurfaceSessionId::from_material("restart"),
                &alice.id
            )
            .is_err()
    );
}

use babble_authoring::ObjectDraft;
use babble_capabilities::{CapabilityGrant, GrantDecision};
use babble_crypto::Keypair;
use babble_identity::{Identity, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_node::{ImportBundle, LocalNode};
use babble_object::CapabilityRequest;
use babble_state::{Event, EventKind, EventTarget};
use serde_json::json;

#[test]
fn imported_revocation_cannot_revoke_another_actors_grant() {
    let root = std::env::temp_dir().join(format!(
        "babble-grant-auth-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let key = Keypair::generate();
    let bob = Identity::create(IdentityKind::Person, "bob", &key).unwrap();
    node.import_signing_identity(bob.clone(), key.clone())
        .unwrap();
    let capability: CapabilityRequest = serde_json::from_value(
        json!({"id":"babble.storage.object","version":1,"scope":{"namespace":"self"}}),
    )
    .unwrap();
    let object = node
        .publish_draft(
            &alice.id,
            ObjectDraft::text("shared app")
                .unwrap()
                .with_capability(capability.clone())
                .unwrap(),
        )
        .unwrap();
    let issued = node
        .grant_capability(&alice.id, &object.id, capability, GrantDecision::Approved)
        .unwrap();
    let grant: CapabilityGrant = serde_json::from_value(issued.payload["grant"].clone()).unwrap();
    let forged = Event::new(
        &bob,
        EventKind::CapabilityRevoked,
        EventTarget::Object(object.id.clone()),
        json!({"grant_id":grant.id,"capability":grant.capability,"version":grant.version}),
        vec![issued.id.clone()],
    )
    .unwrap()
    .sign(&bob, &key)
    .unwrap();
    node.import_bundle(ImportBundle {
        events: vec![forged],
        ..Default::default()
    })
    .unwrap();
    let grants = node
        .capability_grants_for_identity(&object.id, &alice.id)
        .unwrap();
    assert_eq!(grants.len(), 1);
    assert!(grants[0].revoked_at.is_none());
    assert!(
        node.capability_grants(&object.id).unwrap()[0]
            .revoked_at
            .is_none()
    );
    assert!(
        node.capability_grants_for_identity(&object.id, &bob.id)
            .unwrap()
            .is_empty()
    );
    drop(node);
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    assert!(
        node.capability_grants_for_identity(&object.id, &alice.id)
            .unwrap()[0]
            .revoked_at
            .is_none()
    );
    node.revoke_capability(&alice.id, &object.id, &grant.id)
        .unwrap();
    assert!(
        node.capability_grants_for_identity(&object.id, &alice.id)
            .unwrap()[0]
            .revoked_at
            .is_some()
    );
    drop(node);
    std::fs::remove_dir_all(root).unwrap();
}

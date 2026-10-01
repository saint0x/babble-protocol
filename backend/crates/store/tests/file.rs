use babble_crypto::Keypair;
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_identity::{Identity, IdentityKind};
use babble_judgment::{
    ConstantProvider, DefinitionId, JudgmentCache, JudgmentRequest, JudgmentState,
};
use babble_object::Object;
use babble_state::{Event, EventKind, EventTarget};
use babble_store::FileStore;
use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn file_store_round_trips_protocol_records_and_blobs() {
    let root = unique_root("roundtrip");
    let store = FileStore::open(&root).unwrap();

    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    let object = Object::text(&identity, "persistent Objects remain content addressed")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let evidence = Object::text(&identity, "storage separates protocol metadata from blobs")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let edge = Edge::new(
        evidence.id.clone(),
        object.id.clone(),
        Relation::EvidenceFor,
        EdgeOrigin::HumanAssertion,
        Some(identity.id.clone()),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let event = Event::new(
        &identity,
        EventKind::ObjectPublished,
        EventTarget::Object(object.id.clone()),
        serde_json::json!({ "kind": "babble.text" }),
        Vec::new(),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();

    let provider = ConstantProvider::default();
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: object.id.to_string(),
            context: BTreeMap::from([(
                "text".to_string(),
                serde_json::json!("storage separates protocol metadata from blobs"),
            )]),
        },
        parameters: BTreeMap::new(),
    };
    let mut cache = JudgmentCache::default();
    let judgment = cache.get_or_evaluate(&provider, &request).unwrap();

    store.put_identity(&identity).unwrap();
    store.put_object(&evidence, &identity).unwrap();
    store.put_object(&object, &identity).unwrap();
    store.put_edge(&edge, &identity).unwrap();
    store.put_event(&event, &identity).unwrap();
    store.put_judgment(&judgment).unwrap();

    let blob_hash = store
        .put_blob(b"large resources are stored by hash")
        .unwrap();

    assert_eq!(store.get_identity(&identity.id).unwrap().unwrap(), identity);
    assert_eq!(store.get_object(&object.id).unwrap().unwrap(), object);
    assert_eq!(store.get_edge(&edge.id).unwrap().unwrap(), edge);
    assert_eq!(store.get_event(&event.id).unwrap().unwrap(), event);
    assert_eq!(store.get_judgment(&judgment.id).unwrap().unwrap(), judgment);
    assert_eq!(store.list_identities().unwrap(), vec![identity]);
    let objects = store.list_objects().unwrap();
    assert_eq!(objects.len(), 2);
    assert!(objects.contains(&evidence));
    assert!(objects.contains(&object));
    assert_eq!(store.list_edges().unwrap(), vec![edge]);
    assert_eq!(store.list_events().unwrap(), vec![event]);
    assert_eq!(store.list_judgments().unwrap(), vec![judgment]);
    assert_eq!(
        store.get_blob(&blob_hash).unwrap().unwrap(),
        b"large resources are stored by hash"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn blob_integrity_mismatch_is_rejected() {
    let root = unique_root("blob-integrity");
    let store = FileStore::open(&root).unwrap();

    let hash = store.put_blob(b"truthful bytes").unwrap();
    fs::write(root.join("blobs").join(hash.as_str()), b"tampered bytes").unwrap();

    assert!(store.get_blob(&hash).is_err());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn object_storage_round_trips_scoped_entries() {
    let root = unique_root("object-storage");
    let store = FileStore::open(&root).unwrap();

    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    let first = Object::text(&identity, "first storage owner")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let second = Object::text(&identity, "second storage owner")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();

    let first_record = store
        .put_object_storage(
            &first.id,
            "profile/theme",
            serde_json::json!({"mode": "dark"}),
        )
        .unwrap();
    store
        .put_object_storage(&first.id, "profile/name", serde_json::json!("alice"))
        .unwrap();
    store
        .put_object_storage(
            &second.id,
            "profile/theme",
            serde_json::json!({"mode": "light"}),
        )
        .unwrap();

    assert_eq!(first_record.object_id, first.id);
    assert_eq!(
        store
            .get_object_storage(&first.id, "profile/theme")
            .unwrap()
            .unwrap()
            .value,
        serde_json::json!({"mode": "dark"})
    );

    let profile_entries = store
        .list_object_storage(&first.id, Some("profile/"), 10)
        .unwrap();
    assert_eq!(
        profile_entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["profile/name", "profile/theme"]
    );

    assert_eq!(
        store
            .delete_object_storage(&first.id, "profile/theme")
            .unwrap()
            .unwrap()
            .value,
        serde_json::json!({"mode": "dark"})
    );
    assert!(
        store
            .get_object_storage(&first.id, "profile/theme")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store
            .get_object_storage(&second.id, "profile/theme")
            .unwrap()
            .unwrap()
            .value,
        serde_json::json!({"mode": "light"})
    );
    assert!(
        store
            .put_object_storage(&first.id, "../escape", serde_json::json!(true))
            .is_err()
    );

    fs::remove_dir_all(root).unwrap();
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babble-store-{name}-{nanos}"))
}

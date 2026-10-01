use super::*;
use babble_crypto::Keypair;
use babble_graph::{EdgeOrigin, Relation};
use babble_identity::IdentityKind;
use babble_judgment::{DefinitionId, ProviderVersion};
use babble_state::{EventKind, EventTarget};
use std::{process::Command, sync::atomic::AtomicU64};

pub(super) struct Root(pub(super) PathBuf);
impl Root {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "babble-atomic-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

pub(super) fn fixture(store: &FileStore) -> PublicationBatch {
    let keypair = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "atomic-author", &keypair).unwrap();
    store.put_identity(&author).unwrap();
    let source = Object::text(&author, "source")
        .unwrap()
        .sign(&author, &keypair)
        .unwrap();
    store.put_object(&source, &author).unwrap();
    let object = Object::text(&author, "publication")
        .unwrap()
        .sign(&author, &keypair)
        .unwrap();
    let edge = Edge::new(
        object.id.clone(),
        source.id,
        Relation::ReplyTo,
        EdgeOrigin::HumanAssertion,
        Some(author.id.clone()),
    )
    .unwrap()
    .sign(&author, &keypair)
    .unwrap();
    let event = Event::new(
        &author,
        EventKind::ObjectPublished,
        EventTarget::Object(object.id.clone()),
        serde_json::json!({"kind": object.kind.as_str()}),
        vec![],
    )
    .unwrap()
    .sign(&author, &keypair)
    .unwrap();
    let edge_event = Event::new(
        &author,
        EventKind::EdgePublished,
        EventTarget::Edge(edge.id.clone()),
        serde_json::json!({"source": edge.source, "target": edge.target}),
        vec![event.id.clone()],
    )
    .unwrap()
    .sign(&author, &keypair)
    .unwrap();
    let judgment = Judgment {
        id: JudgmentId::from_hash(&Hash::from_bytes(b"fixture-judgment")),
        definition: DefinitionId::spam_v1(),
        provider: ProviderVersion {
            provider: "fixture".into(),
            version: "1".into(),
            model: "fixture".into(),
        },
        input_hash: Hash::from_bytes(b"fixture-input"),
        output: serde_json::json!({"score": 0.1}),
        confidence: 0.9,
        created_at: Timestamp::now(),
    };
    let mut batch = PublicationBatch::new();
    batch.object(&object, &author).unwrap();
    batch.event(&event, &author).unwrap();
    batch.edge(&edge, &author).unwrap();
    batch.event(&edge_event, &author).unwrap();
    batch.judgment(&judgment).unwrap();
    batch
        .receipt(&PublicationReceipt {
            request: PublicationRequest {
                id: Hash::from_bytes(b"fixture-retry"),
                fingerprint: Hash::from_bytes(b"fixture-request"),
                author: author.id.clone(),
            },
            outcome: PublicationOutcome {
                object: Some(object.id.clone()),
                edges: vec![edge.id.clone()],
                event: event.id.clone(),
            },
        })
        .unwrap();
    batch
}

fn phases() -> Vec<String> {
    let mut phases = ["validated", "prepared", "renamed", "committed"]
        .map(str::to_string)
        .to_vec();
    for index in 0..6 {
        phases.extend([
            format!("record-{index}-prepared"),
            format!("record-{index}-renamed"),
        ]);
    }
    phases.extend(["installed", "retired", "finished"].map(str::to_string));
    phases
}

pub(super) fn assert_records(store: &FileStore, expected: &[Record], committed: bool) {
    for record in expected {
        let actual = read_value(&store.path(&record.collection, &record.id).unwrap()).unwrap();
        assert_eq!(
            actual.as_ref(),
            if committed { Some(&record.value) } else { None },
            "{}",
            record.collection
        );
    }
    assert!(!store.root.join(JOURNAL).exists());
    assert!(!store.root.join(STAGING).exists());
}

#[test]
fn publication_errors_at_every_phase_recover_all_or_none() {
    for phase in phases() {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let batch = fixture(&store);
        let expected = batch.records.clone();
        let error = store
            .commit_with_hook(batch, &mut |at| {
                if at == phase {
                    Err(conflict(format!("injected {at}")))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let committed = !matches!(phase.as_str(), "validated" | "prepared");
        match phase.as_str() {
            "validated" | "prepared" => {
                assert!(matches!(error, PublicationError::Precommit(_)));
                assert!(store.list_objects().is_ok());
            }
            "renamed" => assert!(matches!(error, PublicationError::Uncertain { .. })),
            _ => assert!(matches!(error, PublicationError::Committed { .. })),
        }
        if committed {
            assert!(store.list_objects().is_err());
            assert!(store.clone().list_events().is_err());
        }
        let recovered = FileStore::open(&root.0).unwrap();
        assert_records(&recovered, &expected, committed);
        assert_records(&FileStore::open(&root.0).unwrap(), &expected, committed);
        // The failed handle stays invalid even when another handle recovered disk.
        if committed {
            assert!(store.list_judgments().is_err());
        }
    }
}

#[test]
fn publication_crash_child() {
    let Some(root) = std::env::var_os("BABBLE_ATOMIC_CRASH_ROOT") else {
        return;
    };
    let phase = std::env::var("BABBLE_ATOMIC_CRASH_PHASE").unwrap();
    let store = FileStore::open(PathBuf::from(root)).unwrap();
    let batch = fixture(&store);
    sync_write(
        &store.root.join("expected.fixture"),
        &serde_json::to_vec(&batch.records).unwrap(),
    )
    .unwrap();
    store
        .commit_with_hook(batch, &mut |at| {
            if at == phase {
                std::process::exit(73);
            }
            Ok(())
        })
        .unwrap();
    panic!("crash phase was not reached");
}

#[test]
fn publication_process_exit_at_every_phase_recovers_after_restart() {
    for phase in phases() {
        let root = Root::new();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "publication::tests::publication_crash_child",
                "--nocapture",
            ])
            .env("BABBLE_ATOMIC_CRASH_ROOT", &root.0)
            .env("BABBLE_ATOMIC_CRASH_PHASE", &phase)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let expected: Vec<Record> =
            serde_json::from_slice(&fs::read(root.0.join("expected.fixture")).unwrap()).unwrap();
        let store = FileStore::open(&root.0).unwrap();
        assert_records(
            &store,
            &expected,
            !matches!(phase.as_str(), "validated" | "prepared"),
        );
    }
}

#[test]
fn publication_conflicts_and_bounds_never_write() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let batch = fixture(&store);
    let expected = batch.records.clone();
    store.commit_publication(batch).unwrap();
    let conflict = store
        .commit_publication(PublicationBatch {
            records: expected.clone(),
        })
        .unwrap_err();
    assert!(matches!(conflict, PublicationError::Precommit(_)));
    assert_records(&store, &expected, true);
    let duplicate = vec![expected[0].clone(), expected[0].clone()];
    assert!(
        store
            .commit_publication(PublicationBatch { records: duplicate })
            .is_err()
    );
    assert!(store.commit_publication(PublicationBatch::new()).is_err());
    assert!(
        store
            .commit_publication(PublicationBatch {
                records: vec![expected[0].clone(); MAX_RECORDS + 1]
            })
            .is_err()
    );
    let mut judgment = expected[4].clone();
    judgment.value["output"] = Value::String("x".repeat(MAX_BYTES as usize));
    assert!(
        store
            .commit_publication(PublicationBatch {
                records: vec![judgment]
            })
            .is_err()
    );
    assert!(!store.root.join(STAGING).exists());
}

fn leave_committed(store: &FileStore) -> Vec<Record> {
    let batch = fixture(store);
    let expected = batch.records.clone();
    assert!(matches!(
        store.commit_with_hook(batch, &mut |at| {
            if at == "committed" {
                Err(conflict("stop"))
            } else {
                Ok(())
            }
        }),
        Err(PublicationError::Committed { .. })
    ));
    expected
}

#[test]
fn publication_corrupt_journal_and_conflicting_recovery_fail_closed() {
    for corruption in [
        "truncated",
        "digest",
        "version",
        "path",
        "duplicate",
        "conflict",
        "id",
        "signature",
        "receipt_missing_object",
        "receipt_actor",
        "receipt_missing_edge",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let expected = leave_committed(&store);
        let path = root.0.join(JOURNAL);
        let bytes = fs::read(&path).unwrap();
        let mut journal: Journal = serde_json::from_slice(&bytes).unwrap();
        match corruption {
            "truncated" => fs::write(&path, &bytes[..bytes.len() / 2]).unwrap(),
            "digest" => {
                journal.digest = Hash::from_bytes(b"wrong");
                fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
            }
            "version" => {
                journal.version = 2;
                fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
            }
            "conflict" => {
                let mut divergent = expected[4].value.clone();
                divergent["confidence"] = serde_json::json!(0.2);
                fs::write(
                    store.path("judgments", &expected[4].id).unwrap(),
                    serde_json::to_vec(&divergent).unwrap(),
                )
                .unwrap();
            }
            _ => {
                let mut records = expected.clone();
                match corruption {
                    "path" => records[0].collection = "../objects".into(),
                    "duplicate" => records.push(records[0].clone()),
                    "id" => records[0].id = records[2].id.clone(),
                    "signature" => records[0].value["signature"] = Value::Null,
                    "receipt_missing_object" => {
                        records[5].value["outcome"]["object"] =
                            serde_json::to_value(ObjectId::from_hash(&Hash::from_bytes(b"missing")))
                                .unwrap()
                    }
                    "receipt_actor" => {
                        records[5].value["request"]["author"] =
                            serde_json::to_value(IdentityId::from_hash(&Hash::from_bytes(b"wrong")))
                                .unwrap()
                    }
                    "receipt_missing_edge" => {
                        records[5].value["outcome"]["edges"] = serde_json::json!([])
                    }
                    _ => unreachable!(),
                }
                journal.payload = serde_json::to_string(&records).unwrap();
                journal.digest = Hash::from_bytes(journal.payload.as_bytes());
                fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
            }
        }
        assert!(FileStore::open(&root.0).is_err(), "{corruption}");
        assert!(path.exists(), "journal must remain for diagnosis");
        assert!(
            !store.path("objects", &expected[0].id).unwrap().exists(),
            "recovery must preflight all records"
        );
    }
}

#[test]
fn publication_judgment_refresh_recovers_previous_or_new_value() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let batch = fixture(&store);
    let mut record = batch.records[4].clone();
    store.commit_publication(batch).unwrap();
    record.value["confidence"] = serde_json::json!(0.5);
    let expected = record.value.clone();
    let error = store
        .commit_with_hook(
            PublicationBatch {
                records: vec![record.clone()],
            },
            &mut |at| {
                if at == "record-0-renamed" {
                    Err(conflict("stop"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert!(matches!(error, PublicationError::Committed { .. }));
    let store = FileStore::open(&root.0).unwrap();
    assert_eq!(
        read_value(&store.path("judgments", &record.id).unwrap()).unwrap(),
        Some(expected)
    );
}

#[test]
fn publication_rejects_corrupt_judgment_preimage_before_commit() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let batch = fixture(&store);
    let object = batch.records[0].clone();
    let judgment = &batch.records[4];
    fs::write(
        store.path("judgments", &judgment.id).unwrap(),
        b"{\"invalid\":true}",
    )
    .unwrap();
    assert!(matches!(
        store.commit_publication(batch),
        Err(PublicationError::Precommit(_))
    ));
    assert!(store.check_ready().is_ok());
    assert!(!store.path("objects", &object.id).unwrap().exists());
    assert!(!root.0.join(JOURNAL).exists());
    assert!(!root.0.join(STAGING).exists());
}

#[test]
fn publication_readers_wait_until_entire_batch_is_installed() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let batch = fixture(&store);
    let reader = store.clone();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        start_rx.recv().unwrap();
        let count = reader.list_objects().unwrap().len();
        done_tx.send(count).unwrap();
    });
    store
        .commit_with_hook(batch, &mut |at| {
            if at == "record-0-renamed" {
                start_tx.send(()).unwrap();
                assert!(
                    done_rx
                        .recv_timeout(std::time::Duration::from_millis(30))
                        .is_err()
                );
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(done_rx.recv().unwrap(), 2);
    thread.join().unwrap();
}

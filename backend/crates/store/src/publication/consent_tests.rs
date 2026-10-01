use super::tests::{Root, assert_records};
use super::*;
use babel_crypto::Keypair;
use babel_identity::IdentityKind;
use babel_state::{EventKind, EventTarget};
use babel_types::CapabilityGrantId;
use serde_json::json;
use std::process::Command;

fn fixture(store: &FileStore, kind: EventKind) -> (Identity, Keypair, PublicationBatch) {
    let keypair = Keypair::generate();
    let actor = Identity::create(IdentityKind::Person, "consent-author", &keypair).unwrap();
    store.put_identity(&actor).unwrap();
    let object = ObjectId::from_hash(&Hash::from_bytes(b"existing-consent-object"));
    let grant = CapabilityGrantId::from_hash(&Hash::from_bytes(b"consent-grant"));
    let payload = match kind {
        EventKind::CapabilityGranted => json!({"grant": {
            "id": grant,
            "object_id": object,
            "capability": "babel.storage.local",
            "version": 1,
            "scope": {"namespace": "self"},
            "decision": "approved",
            "quota": {
                "calls_per_minute": 20, "bytes_per_minute": 1024,
                "persistent_bytes": 4096, "realtime_connections": 0,
                "max_call_ms": 1000, "background_allowed": false
            },
            "created_at": Timestamp::now(),
            "expires_at": null,
            "revoked_at": null
        }}),
        EventKind::CapabilityRevoked => json!({
            "grant_id": grant, "capability": "babel.storage.local", "version": 1
        }),
        _ => unreachable!(),
    };
    let event = Event::new(&actor, kind, EventTarget::Object(object), payload, vec![])
        .unwrap()
        .sign(&actor, &keypair)
        .unwrap();
    let mut batch = PublicationBatch::new();
    batch.event(&event, &actor).unwrap();
    batch
        .receipt(&PublicationReceipt {
            request: PublicationRequest {
                id: Hash::from_bytes(b"consent-request"),
                fingerprint: Hash::from_bytes(b"consent-fingerprint"),
                author: actor.id.clone(),
            },
            outcome: PublicationOutcome {
                object: None,
                edges: vec![],
                event: event.id,
            },
        })
        .unwrap();
    (actor, keypair, batch)
}

fn kinds() -> [EventKind; 2] {
    [EventKind::CapabilityGranted, EventKind::CapabilityRevoked]
}

fn phases() -> Vec<String> {
    [
        "validated",
        "prepared",
        "renamed",
        "committed",
        "record-0-prepared",
        "record-0-renamed",
        "record-1-prepared",
        "record-1-renamed",
        "installed",
        "retired",
        "finished",
    ]
    .map(str::to_string)
    .to_vec()
}

fn receipt(records: &[Record]) -> PublicationReceipt {
    serde_json::from_value(
        records
            .iter()
            .find(|record| record.collection == "publication_receipts")
            .unwrap()
            .value
            .clone(),
    )
    .unwrap()
}

fn assert_replay(store: &FileStore, records: &[Record], committed: bool) {
    assert_records(store, records, committed);
    let expected = receipt(records);
    let actual = store.publication_receipt(&expected.request.id).unwrap();
    assert_eq!(actual, committed.then_some(expected));
    assert_eq!(store.list_events().unwrap().len(), usize::from(committed));
}

#[test]
fn consent_publication_faults_recover_original_event_and_receipt() {
    for kind in kinds() {
        for phase in phases() {
            let root = Root::new();
            let store = FileStore::open(&root.0).unwrap();
            let (_, _, batch) = fixture(&store, kind.clone());
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
                    assert!(matches!(error, PublicationError::Precommit(_)))
                }
                "renamed" => assert!(matches!(error, PublicationError::Uncertain { .. })),
                _ => assert!(matches!(error, PublicationError::Committed { .. })),
            }
            if committed {
                assert!(
                    store
                        .publication_receipt(&receipt(&expected).request.id)
                        .is_err()
                );
                assert!(store.clone().list_events().is_err());
            }
            let reopened = FileStore::open(&root.0).unwrap();
            assert_replay(&reopened, &expected, committed);
            assert_replay(&FileStore::open(&root.0).unwrap(), &expected, committed);
            if committed {
                assert!(store.check_ready().is_err());
                assert!(matches!(
                    reopened.commit_publication(PublicationBatch {
                        records: expected.clone()
                    }),
                    Err(PublicationError::Precommit(_))
                ));
                assert_replay(&reopened, &expected, true);
            } else {
                store
                    .commit_publication(PublicationBatch {
                        records: expected.clone(),
                    })
                    .unwrap();
                assert_replay(&store, &expected, true);
            }
        }
    }
}

#[test]
fn consent_publication_crash_child() {
    let Some(root) = std::env::var_os("BABEL_CONSENT_CRASH_ROOT") else {
        return;
    };
    let phase = std::env::var("BABEL_CONSENT_CRASH_PHASE").unwrap();
    let kind = serde_json::from_str(&std::env::var("BABEL_CONSENT_CRASH_KIND").unwrap()).unwrap();
    let store = FileStore::open(PathBuf::from(root)).unwrap();
    let (_, _, batch) = fixture(&store, kind);
    sync_write(
        &store.root.join("consent.fixture"),
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
fn consent_publication_process_exit_at_every_phase_recovers_after_restart() {
    for kind in kinds() {
        for phase in phases() {
            let root = Root::new();
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "publication::consent_tests::consent_publication_crash_child",
                    "--nocapture",
                ])
                .env("BABEL_CONSENT_CRASH_ROOT", &root.0)
                .env("BABEL_CONSENT_CRASH_PHASE", &phase)
                .env(
                    "BABEL_CONSENT_CRASH_KIND",
                    serde_json::to_string(&kind).unwrap(),
                )
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(73),
                "{kind:?}/{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let expected = serde_json::from_slice::<Vec<Record>>(
                &fs::read(root.0.join("consent.fixture")).unwrap(),
            )
            .unwrap();
            let committed = !matches!(phase.as_str(), "validated" | "prepared");
            assert_replay(&FileStore::open(&root.0).unwrap(), &expected, committed);
            assert_replay(&FileStore::open(&root.0).unwrap(), &expected, committed);
        }
    }
}

fn replace_event(
    batch: &mut PublicationBatch,
    actor: &Identity,
    keypair: &Keypair,
    change: impl FnOnce(&mut Event),
) {
    let mut event: Event = serde_json::from_value(batch.records[0].value.clone()).unwrap();
    change(&mut event);
    let event = Event::new(
        actor,
        event.kind,
        event.target,
        event.payload,
        event.parents,
    )
    .unwrap()
    .sign(actor, keypair)
    .unwrap();
    batch.records[0].id = event.id.to_string();
    batch.records[0].value = serde_json::to_value(&event).unwrap();
    batch.records[1].value["outcome"]["event"] = json!(event.id);
}

fn assert_precommit_rejected(store: &FileStore, batch: PublicationBatch, label: &str) {
    assert!(
        matches!(
            store.commit_publication(batch),
            Err(PublicationError::Precommit(_))
        ),
        "{label}"
    );
    assert!(store.check_ready().is_ok());
    assert!(store.list_events().unwrap().is_empty());
    assert!(
        read_all_json::<PublicationReceipt>(&store.root.join("publication_receipts"))
            .unwrap()
            .is_empty()
    );
    assert!(!store.root.join(JOURNAL).exists());
    assert!(!store.root.join(STAGING).exists());
}

#[test]
fn consent_publication_rejects_unrelated_or_misleading_records() {
    for kind in kinds() {
        for invalid in [
            "orphan",
            "missing-event",
            "actor",
            "object-outcome",
            "edge-outcome",
            "extra-event",
            "extra-object",
            "extra-edge",
            "extra-judgment",
            "extra-receipt",
            "target",
            "bad-target-id",
            "unrelated-kind",
        ] {
            let root = Root::new();
            let store = FileStore::open(&root.0).unwrap();
            let (actor, keypair, mut batch) = fixture(&store, kind.clone());
            match invalid {
                "orphan" => {
                    batch.records.remove(0);
                }
                "missing-event" => {
                    batch.records[1].value["outcome"]["event"] =
                        json!(EventId::from_hash(&Hash::from_bytes(b"missing")))
                }
                "actor" => {
                    batch.records[1].value["request"]["author"] =
                        json!(IdentityId::from_hash(&Hash::from_bytes(b"other")))
                }
                "object-outcome" => {
                    batch.records[1].value["outcome"]["object"] =
                        batch.records[0].value["target"]["Object"].clone()
                }
                "edge-outcome" => {
                    batch.records[1].value["outcome"]["edges"] =
                        json!([EdgeId::from_hash(&Hash::from_bytes(b"edge"))])
                }
                "extra-receipt" => {
                    let mut extra = batch.records[1].clone();
                    let id = Hash::from_bytes(b"second-request");
                    extra.id = id.to_string();
                    extra.value["request"]["id"] = json!(id);
                    batch.records.push(extra);
                }
                "extra-event" | "extra-object" | "extra-edge" | "extra-judgment" => {
                    let other = super::tests::fixture(&store);
                    let index = match invalid {
                        "extra-object" => 0,
                        "extra-event" => 1,
                        "extra-edge" => 2,
                        _ => 4,
                    };
                    batch.records.push(other.records[index].clone());
                }
                "target" => replace_event(&mut batch, &actor, &keypair, |event| {
                    event.target = EventTarget::Network
                }),
                "bad-target-id" => replace_event(&mut batch, &actor, &keypair, |event| {
                    event.target = EventTarget::Object(ObjectId::new_unchecked("invalid"))
                }),
                "unrelated-kind" => replace_event(&mut batch, &actor, &keypair, |event| {
                    event.kind = EventKind::ConsensusCheckpoint
                }),
                _ => unreachable!(),
            }
            assert_precommit_rejected(&store, batch, invalid);
        }
    }
}

#[test]
fn consent_publication_rejects_signed_malformed_payloads() {
    let invalid_grants = [
        ("/grant", Value::Null),
        ("/grant/id", json!("invalid")),
        (
            "/grant/object_id",
            json!(ObjectId::from_hash(&Hash::from_bytes(b"other-object"))),
        ),
        ("/grant/capability", json!("invalid")),
        ("/grant/version", json!(0)),
        ("/grant/scope", json!("invalid")),
        ("/grant/decision", json!("maybe")),
        ("/grant/quota", json!({})),
        ("/grant/quota/calls_per_minute", json!(-1)),
        ("/grant/created_at", json!("invalid")),
        ("/grant/expires_at", json!(false)),
        ("/grant/revoked_at", json!(Timestamp::now())),
    ];
    for (pointer, invalid) in invalid_grants {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (actor, keypair, mut batch) = fixture(&store, EventKind::CapabilityGranted);
        replace_event(&mut batch, &actor, &keypair, |event| {
            *event.payload.pointer_mut(pointer).unwrap() = invalid
        });
        assert_precommit_rejected(&store, batch, pointer);
    }
    for payload in [
        Value::Null,
        json!({}),
        json!({"grant_id": 12}),
        json!({"grant_id": "invalid"}),
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (actor, keypair, mut batch) = fixture(&store, EventKind::CapabilityRevoked);
        replace_event(&mut batch, &actor, &keypair, |event| {
            event.payload = payload
        });
        assert_precommit_rejected(&store, batch, "revocation payload");
    }
}

#[test]
fn consent_publication_unsigned_and_wrong_signatures_cannot_be_staged() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (actor, _, batch) = fixture(&store, EventKind::CapabilityGranted);
    let mut event: Event = serde_json::from_value(batch.records[0].value.clone()).unwrap();
    event.signature = None;
    assert!(PublicationBatch::new().event(&event, &actor).is_err());
    let event = event.sign(&actor, &Keypair::generate()).unwrap();
    assert!(PublicationBatch::new().event(&event, &actor).is_err());
}

#[test]
fn consent_publication_corrupt_committed_journal_fails_before_install() {
    for kind in kinds() {
        for invalid in [
            "orphan",
            "actor",
            "target",
            "kind",
            "payload",
            "signature",
            "event-id",
            "extra-object",
            "conflicting-disk-event",
        ] {
            let root = Root::new();
            let store = FileStore::open(&root.0).unwrap();
            let (actor, keypair, batch) = fixture(&store, kind.clone());
            let mut records = batch.records.clone();
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
            let event_path = store.path("events", &records[0].id).unwrap();
            let receipt_path = store.path("publication_receipts", &records[1].id).unwrap();
            match invalid {
                "orphan" => {
                    records.remove(0);
                }
                "actor" => {
                    records[1].value["request"]["author"] =
                        json!(IdentityId::from_hash(&Hash::from_bytes(b"other")))
                }
                "signature" => records[0].value["signature"] = Value::Null,
                "event-id" => {
                    records[0].value["id"] = json!(EventId::from_hash(&Hash::from_bytes(b"other")))
                }
                "extra-object" => records.push(Record {
                    collection: "objects".into(),
                    id: "invalid".into(),
                    value: json!({}),
                    previous: None,
                }),
                "conflicting-disk-event" => {
                    let mut divergent = records[0].value.clone();
                    divergent["payload"] = json!({"different": true});
                    fs::write(&event_path, serde_json::to_vec(&divergent).unwrap()).unwrap();
                }
                _ => {
                    let mut batch = PublicationBatch { records };
                    replace_event(&mut batch, &actor, &keypair, |event| match invalid {
                        "target" => event.target = EventTarget::Network,
                        "kind" => event.kind = EventKind::ObjectPublished,
                        "payload" => event.payload = json!({}),
                        _ => unreachable!(),
                    });
                    records = batch.records;
                }
            }
            let payload = serde_json::to_string(&records).unwrap();
            let journal = Journal {
                version: 1,
                digest: Hash::from_bytes(payload.as_bytes()),
                payload,
            };
            fs::write(root.0.join(JOURNAL), serde_json::to_vec(&journal).unwrap()).unwrap();
            assert!(FileStore::open(&root.0).is_err(), "{kind:?}/{invalid}");
            assert!(root.0.join(JOURNAL).exists());
            assert!(
                !receipt_path.exists(),
                "receipt installed despite {invalid}"
            );
            assert_eq!(
                fs::read_dir(root.0.join("events")).unwrap().count(),
                usize::from(invalid == "conflicting-disk-event")
            );
        }
    }
}

#[test]
fn consent_receipt_reads_reject_missing_or_mismatched_events() {
    for kind in kinds() {
        for invalid in ["missing", "id", "actor", "kind", "payload", "unsigned"] {
            let root = Root::new();
            let store = FileStore::open(&root.0).unwrap();
            let (_, _, batch) = fixture(&store, kind.clone());
            let expected = receipt(&batch.records);
            let path = store.path("events", &batch.records[0].id).unwrap();
            let mut value = batch.records[0].value.clone();
            store.commit_publication(batch).unwrap();
            match invalid {
                "missing" => fs::remove_file(&path).unwrap(),
                _ => {
                    match invalid {
                        "id" => {
                            value["id"] = json!(EventId::from_hash(&Hash::from_bytes(b"other")))
                        }
                        "actor" => {
                            value["actor"] =
                                json!(IdentityId::from_hash(&Hash::from_bytes(b"other")))
                        }
                        "kind" => value["kind"] = json!("consensus_checkpoint"),
                        "payload" => value["payload"] = json!({}),
                        "unsigned" => value["signature"] = Value::Null,
                        _ => unreachable!(),
                    }
                    fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
                }
            }
            let reopened = FileStore::open(&root.0).unwrap();
            assert!(
                reopened.publication_receipt(&expected.request.id).is_err(),
                "{kind:?}/{invalid}"
            );
        }
    }
}

#[test]
fn consent_extension_preserves_single_edge_receipts_and_shape_limits() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let old = super::tests::fixture(&store);
    let mut edge_receipt = receipt(&old.records);
    edge_receipt.outcome.object = None;
    edge_receipt.outcome.event = serde_json::from_value::<Event>(old.records[3].value.clone())
        .unwrap()
        .id;
    let mut batch = PublicationBatch {
        records: vec![old.records[2].clone(), old.records[3].clone()],
    };
    batch.receipt(&edge_receipt).unwrap();
    store.commit_publication(batch).unwrap();
    assert_eq!(
        FileStore::open(&root.0)
            .unwrap()
            .publication_receipt(&edge_receipt.request.id)
            .unwrap(),
        Some(edge_receipt.clone())
    );
    edge_receipt
        .outcome
        .edges
        .push(EdgeId::from_hash(&Hash::from_bytes(b"second-edge")));
    assert!(edge_receipt.validate().is_err());
    edge_receipt.outcome.object = Some(ObjectId::from_hash(&Hash::from_bytes(b"object")));
    edge_receipt.outcome.edges[1] = edge_receipt.outcome.edges[0].clone();
    assert!(edge_receipt.validate().is_err());
    edge_receipt.outcome.edges = (0..126)
        .map(|i| EdgeId::from_hash(&Hash::from_bytes(format!("edge-{i}").as_bytes())))
        .collect();
    assert!(edge_receipt.validate().is_err());
}

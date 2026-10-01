use super::tests::{Root, fixture};
use super::*;
use babble_capabilities::{CapabilityId, invocation::*};
use serde_json::json;
use std::{
    process::Command,
    sync::{Arc, Barrier},
};

pub(super) fn at(seconds: u32) -> Timestamp {
    serde_json::from_value(json!(format!(
        "2026-09-30T00:{:02}:{:02}Z",
        seconds / 60,
        seconds % 60
    )))
    .unwrap()
}

fn intent(actor: IdentityId) -> InvocationIntent {
    InvocationIntent {
        request_key: "stable-user-action".into(),
        context: InvocationContext {
            actor,
            login_id: "originating-login".into(),
            object_id: ObjectId::from_hash(&Hash::from_bytes(b"controller")),
            object_version: Hash::from_bytes(b"immutable-version"),
            origin: InvocationOrigin::Surface {
                session_id: "admitted-session".into(),
                document_id: "admitted-document".into(),
                role: "card".into(),
                entry: "index.html".into(),
                resource_digest: Hash::from_bytes(b"resource"),
            },
            policy_revision: Hash::from_bytes(b"policy"),
            context_epoch: Hash::from_bytes(b"server-epoch"),
        },
        method: "babble.social.reply".into(),
        method_version: 2,
        capability: CapabilityId::new("babble.social.reply").unwrap(),
        capability_version: 1,
        scope: json!({"target": "stored-resource"}),
        executor: InvocationExecutor::LocalPublication,
        payload: json!({"text": "exact content", "target": "stored-resource"}),
        created_at: at(0),
        deadline: at(120),
    }
}

pub(super) fn bare_intent() -> InvocationIntent {
    intent(IdentityId::from_hash(&Hash::from_bytes(b"actor")))
}

pub(super) fn approve(store: &FileStore, intent: InvocationIntent) -> InvocationRecord {
    let pending = store.prepare_invocation(intent, at(0)).unwrap();
    store
        .transition_invocation(
            &pending,
            InvocationAction::Approve,
            &pending.intent().context,
            at(1),
        )
        .unwrap()
}

pub(super) fn completion(
    store: &FileStore,
) -> (PublicationBatch, InvocationRecord, InvocationRecord) {
    let mut batch = fixture(store);
    batch.records.retain(|r| r.collection != "judgments");
    let receipt: PublicationReceipt =
        serde_json::from_value(batch.records.last().unwrap().value.clone()).unwrap();
    let approved = approve(store, intent(receipt.request.author.clone()));
    batch
        .records
        .iter_mut()
        .find(|r| r.collection == "publication_receipts")
        .unwrap()
        .value["request"]["fingerprint"] = json!(approved.intent().fingerprint().unwrap());
    let complete = batch
        .invocation_transition(
            &approved,
            InvocationAction::CompletePublication {
                receipt: receipt.request.id,
            },
            &approved.intent().context,
            at(2),
        )
        .unwrap();
    (batch, approved, complete)
}

#[test]
fn invocation_social_quota_serializes_competing_effects_under_writer_lock() {
    use babble_crypto::Keypair;
    use babble_graph::{EdgeOrigin, Relation};
    use babble_identity::IdentityKind;
    use babble_state::{EventKind, EventTarget};
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let keypair = Keypair::generate();
    let actor = Identity::create(IdentityKind::Person, "quota", &keypair).unwrap();
    store.put_identity(&actor).unwrap();
    let object = Object::text(&actor, "controller").unwrap().sign(&actor, &keypair).unwrap();
    store.put_object(&object, &actor).unwrap();
    let mut batches = Vec::new();
    for n in 0..11 {
        let mut intent = intent(actor.id.clone());
        intent.request_key = format!("quota-{n}");
        intent.context.object_id = object.id.clone();
        intent.context.login_id = format!("login-{n}");
        intent.capability = CapabilityId::new("babble.social.follow").unwrap();
        intent.method = "babble.social.follow".into();
        intent.payload = json!({"target_object_id":object.id,"text":null,"media":null});
        let approved = approve(&store, intent);
        let edge = Edge::new(object.id.clone(), object.id.clone(), Relation::Follows,
            EdgeOrigin::HumanAssertion, Some(actor.id.clone())).unwrap().sign(&actor, &keypair).unwrap();
        let event = Event::new(&actor, EventKind::EdgePublished, EventTarget::Edge(edge.id.clone()),
            json!({"source":object.id,"target":object.id}), vec![]).unwrap().sign(&actor, &keypair).unwrap();
        let receipt = PublicationReceipt {
            request: PublicationRequest { id: Hash::from_bytes(format!("quota-receipt-{n}").as_bytes()),
                author: actor.id.clone(), fingerprint: approved.intent().fingerprint().unwrap() },
            outcome: PublicationOutcome { object: None, edges: vec![edge.id.clone()], event: event.id.clone() },
        };
        let mut batch = PublicationBatch::new();
        batch.edge(&edge, &actor).unwrap(); batch.event(&event, &actor).unwrap(); batch.receipt(&receipt).unwrap();
        batch.invocation_transition(&approved, InvocationAction::CompletePublication { receipt: receipt.request.id },
            &approved.intent().context, at(2)).unwrap();
        batches.push(batch);
    }
    let racing = batches.split_off(9);
    for batch in batches { store.commit_publication(batch).unwrap(); }
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = racing.into_iter().map(|batch| {
        let store = FileStore::open(&root.0).unwrap();
        let barrier = barrier.clone();
        std::thread::spawn(move || { barrier.wait(); store.commit_publication(batch) })
    }).collect();
    let outcomes: Vec<_> = workers.into_iter().map(|worker| worker.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(outcomes.iter().find_map(|r| r.as_ref().err()).unwrap().to_string().contains("quota"));
    assert_eq!(store.list_edges().unwrap().len(), 10);
    assert_eq!(store.list_invocations().unwrap().iter().filter(|record| record.consumed_at().is_some()).count(), 10);
    assert_eq!(FileStore::open(&root.0).unwrap().list_edges().unwrap().len(), 10);
}

#[test]
fn invocation_retry_preserves_challenge_and_terminal_result_without_extending_deadline() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let original = bare_intent();
    let pending = store.prepare_invocation(original.clone(), at(0)).unwrap();
    assert_eq!(
        store.prepare_invocation(original.clone(), at(1)).unwrap(),
        pending
    );
    for changed in [
        "payload", "deadline", "scope", "method", "policy", "document", "executor",
    ] {
        let mut other = original.clone();
        match changed {
            "payload" => other.payload = json!({"text": "changed"}),
            "deadline" => other.deadline = at(121),
            "scope" => other.scope = json!({"target": "other"}),
            "method" => other.method = "social.share".into(),
            "policy" => other.context.policy_revision = Hash::from_bytes(b"other"),
            "document" => {
                other.context.origin = InvocationOrigin::HostAction {
                    document_id: "different".into(),
                }
            }
            "executor" => {
                other.executor = InvocationExecutor::External {
                    provider: "remote".into(),
                    version: "1".into(),
                }
            }
            _ => unreachable!(),
        }
        assert!(store.prepare_invocation(other, at(1)).is_err(), "{changed}");
    }
    let denied = store
        .transition_invocation(&pending, InvocationAction::Deny, &original.context, at(1))
        .unwrap();
    assert_eq!(store.prepare_invocation(original, at(200)).unwrap(), denied);
    assert!(
        store
            .transition_invocation(
                &denied,
                InvocationAction::Approve,
                &denied.intent().context,
                at(2)
            )
            .is_err()
    );
    assert_eq!(
        FileStore::open(&root.0)
            .unwrap()
            .invocation(&denied.intent().key().unwrap())
            .unwrap(),
        Some(denied)
    );
}

#[test]
fn invocation_context_deadline_and_payload_bounds_are_enforced() {
    let original = bare_intent();
    let pending = InvocationRecord::pending(original.clone(), at(0)).unwrap();
    for changed in [
        "actor", "login", "object", "version", "origin", "policy", "epoch", "session", "document",
        "role", "entry", "bundle",
    ] {
        let mut context = original.context.clone();
        match changed {
            "actor" => context.actor = IdentityId::from_hash(&Hash::from_bytes(b"other")),
            "login" => context.login_id = "other-login".into(),
            "object" => context.object_id = ObjectId::from_hash(&Hash::from_bytes(b"other")),
            "version" => context.object_version = Hash::from_bytes(b"other"),
            "origin" => {
                context.origin = InvocationOrigin::HostAction {
                    document_id: "admitted-document".into(),
                }
            }
            "policy" => context.policy_revision = Hash::from_bytes(b"other"),
            "epoch" => context.context_epoch = Hash::from_bytes(b"other"),
            _ => {
                if let InvocationOrigin::Surface {
                    session_id,
                    document_id,
                    role,
                    entry,
                    resource_digest,
                } = &mut context.origin
                {
                    match changed {
                        "session" => *session_id = "other".into(),
                        "document" => *document_id = "other".into(),
                        "role" => *role = "other".into(),
                        "entry" => *entry = "other".into(),
                        "bundle" => *resource_digest = Hash::from_bytes(b"other"),
                        _ => unreachable!(),
                    }
                }
            }
        }
        assert!(
            pending
                .transition(InvocationAction::Approve, &context, at(1))
                .is_err(),
            "{changed}"
        );
    }
    assert!(
        pending
            .transition(InvocationAction::Approve, &original.context, at(120))
            .is_err()
    );
    assert!(
        pending
            .transition(InvocationAction::Expire, &original.context, at(119))
            .is_err()
    );
    let expired = pending
        .transition(InvocationAction::Expire, &original.context, at(120))
        .unwrap();
    assert!(expired.state().is_terminal());
    let approved = pending
        .transition(InvocationAction::Approve, &original.context, at(1))
        .unwrap();
    assert!(
        approved
            .transition(
                InvocationAction::CompletePublication {
                    receipt: Hash::from_bytes(b"receipt")
                },
                &original.context,
                at(120)
            )
            .is_err()
    );
    for invalid in [
        "payload",
        "scope",
        "depth",
        "ttl",
        "ttl-fraction",
        "zero-version",
        "invalid-hash",
        "identifier",
    ] {
        let mut intent = original.clone();
        match invalid {
            "payload" => intent.payload = json!("x".repeat(MAX_INVOCATION_PAYLOAD_BYTES)),
            "scope" => intent.scope = json!("x".repeat(MAX_INVOCATION_SCOPE_BYTES)),
            "depth" => {
                for _ in 0..34 {
                    intent.payload = json!([intent.payload]);
                }
            }
            "ttl" => intent.deadline = at(301),
            "ttl-fraction" => {
                intent.deadline =
                    serde_json::from_value(json!("2026-09-30T00:05:00.000000001Z")).unwrap()
            }
            "zero-version" => intent.method_version = 0,
            "invalid-hash" => intent.context.object_version = Hash::new_unchecked("g".repeat(64)),
            "identifier" => intent.request_key = "x".repeat(257),
            _ => unreachable!(),
        }
        assert!(intent.validate().is_err(), "{invalid}");
    }
    assert!(InvocationRecord::pending(original.clone(), at(120)).is_err());
    let mut reordered = original.clone();
    reordered.payload = json!({"target": "stored-resource", "text": "exact content"});
    assert_eq!(
        original.fingerprint().unwrap(),
        reordered.fingerprint().unwrap()
    );
    assert_eq!(
        original.payload_hash().unwrap(),
        reordered.payload_hash().unwrap()
    );
}

#[test]
fn invocation_external_dispatch_never_reports_an_undispatched_effect() {
    for outcome in ["complete", "unknown", "failed"] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let mut intent = bare_intent();
        intent.executor = InvocationExecutor::External {
            provider: "provider".into(),
            version: "1".into(),
        };
        let approved = approve(&store, intent);
        let context = &approved.intent().context;
        let dispatch_id = Hash::from_bytes(b"provider-idempotency-key");
        assert!(
            approved
                .transition(
                    InvocationAction::CompleteExternal {
                        dispatch_id: dispatch_id.clone(),
                        result: json!({})
                    },
                    context,
                    at(2)
                )
                .is_err()
        );
        assert!(
            approved
                .transition(
                    InvocationAction::CompletePublication {
                        receipt: Hash::from_bytes(b"receipt")
                    },
                    context,
                    at(2)
                )
                .is_err()
        );
        let running = store
            .transition_invocation(
                &approved,
                InvocationAction::Dispatch {
                    dispatch_id: dispatch_id.clone(),
                },
                context,
                at(2),
            )
            .unwrap();
        assert_eq!(running.consumed_at(), Some(at(2)));
        assert!(
            store
                .transition_invocation(&running, InvocationAction::Cancel, context, at(3))
                .is_err()
        );
        assert!(
            running
                .transition(
                    InvocationAction::CompleteExternal {
                        dispatch_id: Hash::from_bytes(b"other"),
                        result: json!({})
                    },
                    context,
                    at(3)
                )
                .is_err()
        );
        let reopened = FileStore::open(&root.0).unwrap();
        assert_eq!(
            reopened
                .invocation(&running.intent().key().unwrap())
                .unwrap(),
            Some(running.clone())
        );
        let action = match outcome {
            "complete" => InvocationAction::CompleteExternal {
                dispatch_id: dispatch_id.clone(),
                result: json!({"provider_receipt": "acknowledged"}),
            },
            "unknown" => InvocationAction::MarkUnknown {
                dispatch_id: dispatch_id.clone(),
            },
            _ => InvocationAction::Fail {
                code: "provider_rejected".into(),
            },
        };
        let terminal = reopened
            .transition_invocation(&running, action, context, at(200))
            .unwrap();
        assert_eq!(terminal.state().is_terminal(), outcome != "unknown");
        assert!(
            terminal
                .transition(
                    InvocationAction::Dispatch {
                        dispatch_id: dispatch_id.clone()
                    },
                    context,
                    at(201)
                )
                .is_err()
        );
        assert_eq!(
            reopened
                .prepare_invocation(terminal.intent().clone(), at(201))
                .unwrap(),
            terminal
        );
        if outcome == "unknown" {
            let reconciled = reopened
                .transition_invocation(
                    &terminal,
                    InvocationAction::CompleteExternal {
                        dispatch_id,
                        result: json!({"provider_receipt": "reconciled"}),
                    },
                    context,
                    at(201),
                )
                .unwrap();
            assert!(reconciled.state().is_terminal());
            assert_eq!(reconciled.consumed_at(), running.consumed_at());
            assert_eq!(reconciled.revision(), 4);
            FileStore::open(&root.0).unwrap();
        }
    }
}

#[test]
fn invocation_cancellation_expiry_and_restart_invalidation_cannot_revive() {
    for from_approved in [false, true] {
        for action in [
            InvocationAction::Cancel,
            InvocationAction::Expire,
            InvocationAction::Invalidate {
                reason: InvocationInvalidation::Restart,
            },
        ] {
            let root = Root::new();
            let store = FileStore::open(&root.0).unwrap();
            let original = bare_intent();
            let record = if from_approved {
                approve(&store, original.clone())
            } else {
                store.prepare_invocation(original.clone(), at(0)).unwrap()
            };
            let terminal = store
                .transition_invocation(&record, action, &original.context, at(120))
                .unwrap();
            assert_eq!(terminal.consumed_at(), None);
            assert!(
                store
                    .transition_invocation(
                        &record,
                        InvocationAction::Approve,
                        &original.context,
                        at(121)
                    )
                    .is_err()
            );
            assert_eq!(
                store.prepare_invocation(original, at(200)).unwrap(),
                terminal
            );
            FileStore::open(&root.0).unwrap();
        }
    }
}

#[test]
fn invocation_concurrent_stale_decisions_and_duplicate_prepare_have_one_winner() {
    let root = Root::new();
    let barrier = Arc::new(Barrier::new(8));
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let store = FileStore::open(&root.0).unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.prepare_invocation(bare_intent(), at(0)).unwrap()
            })
        })
        .collect();
    let initial: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(initial.iter().all(|record| record == &initial[0]));
    let barrier = Arc::new(Barrier::new(3));
    let handles: Vec<_> = [
        InvocationAction::Approve,
        InvocationAction::Deny,
        InvocationAction::Cancel,
    ]
    .into_iter()
    .map(|action| {
        let store = FileStore::open(&root.0).unwrap();
        let expected = initial[0].clone();
        let barrier = barrier.clone();
        std::thread::spawn(move || {
            barrier.wait();
            store.transition_invocation(&expected, action, &expected.intent().context, at(1))
        })
    })
    .collect();
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    let store = FileStore::open(&root.0).unwrap();
    assert_eq!(store.list_invocations().unwrap().len(), 1);
    assert_eq!(
        store
            .invocation(&initial[0].intent().key().unwrap())
            .unwrap()
            .unwrap()
            .revision(),
        1
    );
}

#[test]
fn invocation_expected_preimage_is_not_replaced_by_commit_disk_loading() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let pending = store.prepare_invocation(bare_intent(), at(0)).unwrap();
    let mut forged = serde_json::to_value(&pending).unwrap();
    forged["id"] = json!(Hash::from_bytes(b"another-challenge"));
    let forged: InvocationRecord = serde_json::from_value(forged).unwrap();
    forged.validate().unwrap();
    let mut batch = PublicationBatch::new();
    batch
        .invocation_transition(
            &forged,
            InvocationAction::Approve,
            &forged.intent().context,
            at(1),
        )
        .unwrap();
    assert!(matches!(
        store.commit_publication(batch),
        Err(PublicationError::Precommit(_))
    ));
    let mut approve = PublicationBatch::new();
    approve
        .invocation_transition(
            &pending,
            InvocationAction::Approve,
            &pending.intent().context,
            at(1),
        )
        .unwrap();
    let cancelled = store
        .transition_invocation(
            &pending,
            InvocationAction::Cancel,
            &pending.intent().context,
            at(1),
        )
        .unwrap();
    assert!(matches!(
        store.commit_publication(approve),
        Err(PublicationError::Precommit(_))
    ));
    assert_eq!(
        store.invocation(&pending.intent().key().unwrap()).unwrap(),
        Some(cancelled)
    );
}

#[test]
fn invocation_local_completion_requires_same_batch_receipt_and_real_actor_effect() {
    for invalid in [
        "missing-receipt",
        "missing-object",
        "missing-event",
        "missing-edge",
        "actor",
        "receipt-id",
        "receipt-only",
        "unrelated-event",
        "phase-preimage",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (mut batch, approved, _) = completion(&store);
        match invalid {
            "missing-receipt" => batch
                .records
                .retain(|r| r.collection != "publication_receipts"),
            "missing-object" => batch.records.retain(|r| r.collection != "objects"),
            "missing-event" => batch.records.retain(|r| r.collection != "events"),
            "missing-edge" => batch.records.retain(|r| r.collection != "edges"),
            "actor" => {
                batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "publication_receipts")
                    .unwrap()
                    .value["request"]["author"] =
                    json!(IdentityId::from_hash(&Hash::from_bytes(b"other-actor")))
            }
            "receipt-id" => {
                let r = batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "publication_receipts")
                    .unwrap();
                r.id = Hash::from_bytes(b"other-receipt").to_string();
                r.value["request"]["id"] = json!(r.id);
            }
            "receipt-only" => batch.records.retain(|r| {
                matches!(
                    r.collection.as_str(),
                    "publication_receipts" | "invocations"
                )
            }),
            "unrelated-event" => {
                batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "events")
                    .unwrap()
                    .value["kind"] = json!("capability_revoked")
            }
            "phase-preimage" => {
                batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "invocations")
                    .unwrap()
                    .previous = Some(json!({}))
            }
            _ => unreachable!(),
        }
        assert!(
            matches!(
                store.commit_publication(batch),
                Err(PublicationError::Precommit(_))
            ),
            "{invalid}"
        );
        assert_eq!(
            store.invocation(&approved.intent().key().unwrap()).unwrap(),
            Some(approved)
        );
        assert_eq!(store.list_objects().unwrap().len(), 1);
        assert_eq!(store.list_events().unwrap().len(), 0);
    }
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (batch, approved, complete) = completion(&store);
    store.commit_publication(batch).unwrap();
    let key = complete.intent().key().unwrap();
    assert_eq!(store.invocation(&key).unwrap(), Some(complete.clone()));
    assert_eq!(complete.consumed_at(), Some(at(2)));
    assert!(
        store
            .transition_invocation(
                &approved,
                InvocationAction::Cancel,
                &approved.intent().context,
                at(3)
            )
            .is_err()
    );
    assert_eq!(
        store
            .prepare_invocation(complete.intent().clone(), at(200))
            .unwrap(),
        complete
    );
    assert_eq!(
        FileStore::open(&root.0).unwrap().invocation(&key).unwrap(),
        Some(complete)
    );
    assert_eq!(store.list_objects().unwrap().len(), 2);
}

fn phases() -> Vec<String> {
    let mut phases = ["validated", "prepared", "renamed", "committed"]
        .map(str::to_owned)
        .to_vec();
    for index in 0..6 {
        phases.extend([
            format!("record-{index}-prepared"),
            format!("record-{index}-renamed"),
        ]);
    }
    phases.extend(["installed", "retired", "finished"].map(str::to_owned));
    phases
}

#[test]
fn invocation_fault_at_every_publication_phase_recovers_consumption_and_effect_together() {
    for phase in phases() {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (batch, approved, complete) = completion(&store);
        let error = store
            .commit_with_hook(batch, &mut |at| {
                if at == phase {
                    Err(conflict("injected"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let committed = !matches!(phase.as_str(), "validated" | "prepared");
        assert_eq!(matches!(error, PublicationError::Precommit(_)), !committed);
        if committed {
            assert!(store.invocation(&approved.intent().key().unwrap()).is_err());
        }
        for _ in 0..2 {
            let reopened = FileStore::open(&root.0).unwrap();
            assert_eq!(
                reopened
                    .invocation(&approved.intent().key().unwrap())
                    .unwrap(),
                Some(if committed {
                    complete.clone()
                } else {
                    approved.clone()
                }),
                "{phase}"
            );
            assert_eq!(
                reopened.list_objects().unwrap().len(),
                if committed { 2 } else { 1 },
                "{phase}"
            );
            assert_eq!(
                reopened.list_events().unwrap().len(),
                if committed { 2 } else { 0 },
                "{phase}"
            );
        }
    }
}

#[test]
fn invocation_crash_child() {
    let Some(root) = std::env::var_os("BABBLE_INVOCATION_CRASH_ROOT") else {
        return;
    };
    let phase = std::env::var("BABBLE_INVOCATION_CRASH_PHASE").unwrap();
    let store = FileStore::open(PathBuf::from(root)).unwrap();
    let (batch, approved, complete) = completion(&store);
    sync_write(
        &store.root.join("expected.fixture"),
        &serde_json::to_vec(&(approved, complete)).unwrap(),
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
    panic!("crash phase not reached");
}

#[test]
fn invocation_process_exit_at_every_phase_recovers_after_restart() {
    for phase in phases() {
        let root = Root::new();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "publication::invocation_tests::invocation_crash_child",
                "--nocapture",
            ])
            .env("BABBLE_INVOCATION_CRASH_ROOT", &root.0)
            .env("BABBLE_INVOCATION_CRASH_PHASE", &phase)
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{phase}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let (approved, complete): (InvocationRecord, InvocationRecord) =
            serde_json::from_slice(&fs::read(root.0.join("expected.fixture")).unwrap()).unwrap();
        let committed = !matches!(phase.as_str(), "validated" | "prepared");
        let store = FileStore::open(&root.0).unwrap();
        assert_eq!(
            store.invocation(&approved.intent().key().unwrap()).unwrap(),
            Some(if committed { complete } else { approved })
        );
        assert_eq!(
            store.list_objects().unwrap().len(),
            if committed { 2 } else { 1 }
        );
    }
}

#[test]
fn invocation_reopen_fails_closed_on_illegal_history_and_missing_effects() {
    for invalid in [
        "intent",
        "predecessor",
        "missing-initial",
        "missing-approved",
        "illegal-transition",
        "name",
        "receipt",
        "object",
        "edge",
        "event",
        "actor",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (batch, approved, complete) = completion(&store);
        let records = batch.records.clone();
        store.commit_publication(batch).unwrap();
        let phase_path = store
            .path("invocations", &complete.storage_id().unwrap())
            .unwrap();
        let mut value = serde_json::to_value(&complete).unwrap();
        match invalid {
            "intent" => value["intent"]["payload"] = json!({"changed": true}),
            "predecessor" => value["previous"] = json!(Hash::from_bytes(b"wrong")),
            "illegal-transition" => {
                value["action"] = json!({"kind": "approve"});
                value["state"] = json!({"kind": "approved"});
            }
            "missing-initial" => fs::remove_file(
                store
                    .path(
                        "invocations",
                        &format!("{}-0", approved.intent().key().unwrap()),
                    )
                    .unwrap(),
            )
            .unwrap(),
            "missing-approved" => fs::remove_file(
                store
                    .path("invocations", &approved.storage_id().unwrap())
                    .unwrap(),
            )
            .unwrap(),
            "name" => {
                fs::rename(&phase_path, phase_path.with_file_name("bad-name.json")).unwrap();
            }
            "actor" => {
                let r = records
                    .iter()
                    .find(|r| r.collection == "publication_receipts")
                    .unwrap();
                let mut receipt = r.value.clone();
                receipt["request"]["author"] =
                    json!(IdentityId::from_hash(&Hash::from_bytes(b"other")));
                fs::write(
                    store.path(&r.collection, &r.id).unwrap(),
                    serde_json::to_vec(&receipt).unwrap(),
                )
                .unwrap();
            }
            _ => {
                let collection = match invalid {
                    "receipt" => "publication_receipts",
                    "object" => "objects",
                    "edge" => "edges",
                    "event" => "events",
                    _ => unreachable!(),
                };
                let r = records.iter().find(|r| r.collection == collection).unwrap();
                fs::remove_file(store.path(&r.collection, &r.id).unwrap()).unwrap();
            }
        }
        if matches!(invalid, "intent" | "predecessor" | "illegal-transition") {
            fs::write(&phase_path, serde_json::to_vec(&value).unwrap()).unwrap();
        }
        assert!(FileStore::open(&root.0).is_err(), "{invalid}");
    }
}

#[test]
fn invocation_recovery_rejects_rehashed_journal_with_wrong_preimage_or_absent_effect() {
    for invalid in [
        "preimage",
        "missing-receipt",
        "missing-object",
        "changed-intent",
        "illegal-transition",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (batch, _, _) = completion(&store);
        let mut records = batch.records.clone();
        assert!(
            store
                .commit_with_hook(batch, &mut |at| if at == "committed" {
                    Err(conflict("stop"))
                } else {
                    Ok(())
                })
                .is_err()
        );
        match invalid {
            "preimage" => {
                records
                    .iter_mut()
                    .find(|r| r.collection == "invocations")
                    .unwrap()
                    .value["previous"] = json!(Hash::from_bytes(b"wrong"))
            }
            "missing-receipt" => records.retain(|r| r.collection != "publication_receipts"),
            "missing-object" => records.retain(|r| r.collection != "objects"),
            "changed-intent" => {
                records
                    .iter_mut()
                    .find(|r| r.collection == "invocations")
                    .unwrap()
                    .value["intent"]["method"] = json!("other.method")
            }
            "illegal-transition" => {
                let r = records
                    .iter_mut()
                    .find(|r| r.collection == "invocations")
                    .unwrap();
                r.value["action"] = json!({"kind": "approve"});
                r.value["state"] = json!({"kind": "approved"});
            }
            _ => unreachable!(),
        }
        let payload = serde_json::to_string(&records).unwrap();
        let journal = Journal {
            version: 1,
            digest: Hash::from_bytes(payload.as_bytes()),
            payload,
        };
        fs::write(root.0.join(JOURNAL), serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(FileStore::open(&root.0).is_err(), "{invalid}");
        assert!(root.0.join(JOURNAL).exists());
        assert_eq!(fs::read_dir(root.0.join("events")).unwrap().count(), 0);
    }
}

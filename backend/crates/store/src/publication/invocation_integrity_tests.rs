use super::invocation_tests::{approve, at, bare_intent, completion};
use super::tests::{Root, fixture};
use super::*;
use babel_capabilities::invocation::*;
use serde_json::json;
use std::sync::{Arc, Barrier};

#[test]
fn invocation_cancel_and_publication_compete_for_one_durable_preimage() {
    for _ in 0..8 {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (batch, approved, complete) = completion(&store);
        let barrier = Arc::new(Barrier::new(2));
        let publisher = {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_publication(batch)
            })
        };
        let canceller = {
            let store = FileStore::open(&root.0).unwrap();
            let approved = approved.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.transition_invocation(
                    &approved,
                    InvocationAction::Cancel,
                    &approved.intent().context,
                    at(2),
                )
            })
        };
        let published = publisher.join().unwrap().is_ok();
        let cancelled = canceller.join().unwrap().is_ok();
        assert_ne!(published, cancelled);
        let reopened = FileStore::open(&root.0).unwrap();
        let outcome = reopened
            .invocation(&approved.intent().key().unwrap())
            .unwrap()
            .unwrap();
        if published {
            assert_eq!(outcome, complete);
        } else {
            assert_eq!(outcome.state(), &InvocationState::Cancelled);
        }
        assert_eq!(
            reopened.list_objects().unwrap().len(),
            if published { 2 } else { 1 }
        );
        assert_eq!(outcome.consumed_at().is_some(), published);
    }
}

#[test]
fn invocation_publication_receipt_with_valid_other_actor_effect_is_rejected() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let mut batch = fixture(&store);
    let receipt: PublicationReceipt =
        serde_json::from_value(batch.records.last().unwrap().value.clone()).unwrap();
    let approved = approve(&store, bare_intent());
    assert_ne!(receipt.request.author, approved.intent().context.actor);
    batch.records.retain(|r| r.collection != "judgments");
    batch
        .records
        .iter_mut()
        .find(|r| r.collection == "publication_receipts")
        .unwrap()
        .value["request"]["fingerprint"] = json!(approved.intent().fingerprint().unwrap());
    batch
        .invocation_transition(
            &approved,
            InvocationAction::CompletePublication {
                receipt: receipt.request.id,
            },
            &approved.intent().context,
            at(2),
        )
        .unwrap();
    assert!(matches!(
        store.commit_publication(batch),
        Err(PublicationError::Precommit(_))
    ));
    assert_eq!(store.list_objects().unwrap().len(), 1);
}

#[test]
fn invocation_cannot_consume_an_already_committed_receipt() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (mut batch, approved, _) = completion(&store);
    batch.records.retain(|r| r.collection != "invocations");
    let receipt: PublicationReceipt = serde_json::from_value(
        batch
            .records
            .iter()
            .find(|r| r.collection == "publication_receipts")
            .unwrap()
            .value
            .clone(),
    )
    .unwrap();
    store.commit_publication(batch).unwrap();
    assert!(matches!(
        store.transition_invocation(
            &approved,
            InvocationAction::CompletePublication {
                receipt: receipt.request.id
            },
            &approved.intent().context,
            at(2)
        ),
        Err(PublicationError::Precommit(_))
    ));
    assert_eq!(
        store.invocation(&approved.intent().key().unwrap()).unwrap(),
        Some(approved)
    );
}

#[test]
fn invocation_publication_rejects_unrelated_same_actor_intent_and_extra_records() {
    for invalid in ["fingerprint", "edge", "judgment", "event-only-receipt"] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (mut batch, approved, _) = completion(&store);
        match invalid {
            "fingerprint" => {
                batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "publication_receipts")
                    .unwrap()
                    .value["request"]["fingerprint"] =
                    json!(Hash::from_bytes(b"unrelated-same-actor-operation"))
            }
            "edge" => {
                let mut extra = batch
                    .records
                    .iter()
                    .find(|r| r.collection == "edges")
                    .unwrap()
                    .clone();
                extra.id = EdgeId::from_hash(&Hash::from_bytes(b"unreceipted-edge")).to_string();
                extra.value["id"] = json!(extra.id);
                batch.records.push(extra);
            }
            "judgment" => {
                let extra = fixture(&store)
                    .records
                    .into_iter()
                    .find(|r| r.collection == "judgments")
                    .unwrap();
                batch.records.push(extra);
            }
            "event-only-receipt" => {
                let receipt = batch
                    .records
                    .iter_mut()
                    .find(|r| r.collection == "publication_receipts")
                    .unwrap();
                receipt.value["outcome"]["object"] = Value::Null;
                receipt.value["outcome"]["edges"] = json!([]);
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
        assert!(store.list_events().unwrap().is_empty());
    }
}

#[test]
fn invocation_reopen_cryptographically_validates_promised_effects() {
    for collection in ["objects", "edges", "events"] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (batch, _, _) = completion(&store);
        let record = batch
            .records
            .iter()
            .find(|r| r.collection == collection)
            .unwrap()
            .clone();
        store.commit_publication(batch).unwrap();
        let mut corrupt = record.value;
        corrupt["signature"] = Value::Null;
        fs::write(
            store.path(collection, &record.id).unwrap(),
            serde_json::to_vec(&corrupt).unwrap(),
        )
        .unwrap();
        assert!(FileStore::open(&root.0).is_err(), "{collection}");
    }
}

#[test]
fn invocation_initial_intent_corruption_and_terminal_revival_are_rejected() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let pending = store.prepare_invocation(bare_intent(), at(0)).unwrap();
    let denied = store
        .transition_invocation(
            &pending,
            InvocationAction::Deny,
            &pending.intent().context,
            at(1),
        )
        .unwrap();
    let mut forged = serde_json::to_value(&denied).unwrap();
    forged["state"] = json!({"kind": "approved"});
    forged["action"] = json!({"kind": "approve"});
    let forged: InvocationRecord = serde_json::from_value(forged).unwrap();
    // Even a shape-valid alternate decision with the same revision cannot act
    // as the expected preimage for consuming the actual durable denial.
    let mut batch = PublicationBatch::new();
    batch
        .invocation_transition(
            &forged,
            InvocationAction::Cancel,
            &forged.intent().context,
            at(2),
        )
        .unwrap();
    assert!(matches!(
        store.commit_publication(batch),
        Err(PublicationError::Precommit(_))
    ));
    let mut corrupt = serde_json::to_value(&pending).unwrap();
    corrupt["intent"]["context"]["origin"]["resource_digest"] =
        json!(Hash::from_bytes(b"other-resource"));
    fs::write(
        store
            .path("invocations", &pending.storage_id().unwrap())
            .unwrap(),
        serde_json::to_vec(&corrupt).unwrap(),
    )
    .unwrap();
    assert!(FileStore::open(&root.0).is_err());
}

#[test]
fn invocation_unknown_can_reconcile_failure_but_never_cancel_or_redispatch() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let mut intent = bare_intent();
    intent.executor = InvocationExecutor::External {
        provider: "provider".into(),
        version: "1".into(),
    };
    let approved = approve(&store, intent);
    let context = &approved.intent().context;
    let dispatch_id = Hash::from_bytes(b"dispatch");
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
    let unknown = store
        .transition_invocation(
            &running,
            InvocationAction::MarkUnknown {
                dispatch_id: dispatch_id.clone(),
            },
            context,
            at(200),
        )
        .unwrap();
    for action in [
        InvocationAction::Cancel,
        InvocationAction::Expire,
        InvocationAction::Dispatch {
            dispatch_id: dispatch_id.clone(),
        },
        InvocationAction::CompleteExternal {
            dispatch_id: Hash::from_bytes(b"wrong"),
            result: json!({}),
        },
    ] {
        assert!(
            store
                .transition_invocation(&unknown, action, context, at(201))
                .is_err()
        );
    }
    let failed = store
        .transition_invocation(
            &unknown,
            InvocationAction::Fail {
                code: "provider_confirmed_failure".into(),
            },
            context,
            at(201),
        )
        .unwrap();
    assert!(failed.state().is_terminal());
    assert_eq!(failed.consumed_at(), running.consumed_at());
    assert_eq!(
        store
            .prepare_invocation(failed.intent().clone(), at(201))
            .unwrap(),
        failed
    );
    FileStore::open(&root.0).unwrap();
}

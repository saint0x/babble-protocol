use super::invocation_tests::{approve, at, bare_intent};
use super::tests::Root;
use super::*;
use babble_capabilities::{CapabilityId, invocation::*};
use serde_json::json;
use std::sync::{Arc, Barrier};

fn browser_intent(key: &str) -> InvocationIntent {
    let mut intent = bare_intent();
    intent.request_key = key.into();
    intent.method = "babble.fullscreen.enter".into();
    intent.capability = CapabilityId::new("babble.fullscreen.enter").unwrap();
    intent.scope = json!({});
    intent.payload = json!({"target_hint":null,"navigation_ui":"auto"});
    intent.executor = InvocationExecutor::External {
        provider: browser::BROWSER_EXECUTOR.into(),
        version: "1".into(),
    };
    intent
}

fn dispatch_batch(record: &InvocationRecord) -> (PublicationBatch, InvocationRecord) {
    let mut batch = PublicationBatch::new();
    let running = batch
        .invocation_transition(
            record,
            InvocationAction::Dispatch {
                dispatch_id: Hash::from_bytes(record.intent().request_key.as_bytes()),
            },
            &record.intent().context,
            at(2),
        )
        .unwrap();
    (batch, running)
}

#[test]
fn browser_quota_is_atomic_across_competing_store_handles_and_logins() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let mut batches = Vec::new();
    for i in 0..11 {
        let mut intent = browser_intent(&format!("quota-{i}"));
        intent.context.login_id = format!("different-login-{i}");
        let approved = approve(&store, intent);
        batches.push(dispatch_batch(&approved).0);
    }
    let racing = batches.split_off(9);
    for batch in batches {
        store.commit_publication(batch).unwrap();
    }
    let barrier = Arc::new(Barrier::new(2));
    let workers: Vec<_> = racing
        .into_iter()
        .map(|batch| {
            let store = FileStore::open(&root.0).unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.commit_publication(batch)
            })
        })
        .collect();
    let outcomes: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        outcomes
            .iter()
            .find_map(|r| r.as_ref().err())
            .unwrap()
            .to_string()
            .contains("quota")
    );
    assert_eq!(
        store
            .list_invocations()
            .unwrap()
            .iter()
            .filter(|r| r.consumed_at().is_some())
            .count(),
        10
    );
}

#[test]
fn browser_cancel_and_dispatch_compete_for_one_preimage() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let approved = approve(&store, browser_intent("race"));
    let (batch, _) = dispatch_batch(&approved);
    let barrier = Arc::new(Barrier::new(2));
    let other = FileStore::open(&root.0).unwrap();
    let signal = barrier.clone();
    let worker = std::thread::spawn(move || {
        signal.wait();
        other.commit_publication(batch)
    });
    barrier.wait();
    let cancelled = store.transition_invocation(
        &approved,
        InvocationAction::Cancel,
        &approved.intent().context,
        at(2),
    );
    let dispatched = worker.join().unwrap();
    assert_ne!(cancelled.is_ok(), dispatched.is_ok());
    let record = store
        .invocation(&approved.intent().key().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(record.consumed_at().is_some(), dispatched.is_ok());
}

#[test]
fn browser_crash_child() {
    let Some(root) = std::env::var_os("BABBLE_BROWSER_CRASH_ROOT") else {
        return;
    };
    let phase = std::env::var("BABBLE_BROWSER_CRASH_PHASE").unwrap();
    let operation = std::env::var("BABBLE_BROWSER_CRASH_OPERATION").unwrap();
    let store = FileStore::open(PathBuf::from(root)).unwrap();
    let approved = approve(&store, browser_intent("crash"));
    let (mut batch, running) = dispatch_batch(&approved);
    let (before, after) = if operation == "ack" {
        store.commit_publication(batch).unwrap();
        batch = PublicationBatch::new();
        let dispatch_id = match running.state() {
            InvocationState::Running { dispatch_id } => dispatch_id.clone(),
            _ => unreachable!(),
        };
        let completed = batch
            .invocation_transition(
                &running,
                InvocationAction::CompleteExternal {
                    dispatch_id,
                    result: json!({"kind":"fullscreen_enter","entered":true}),
                },
                &running.intent().context,
                at(3),
            )
            .unwrap();
        (running, completed)
    } else {
        (approved, running)
    };
    sync_write(
        &store.root.join("browser.fixture"),
        &serde_json::to_vec(&(before, after)).unwrap(),
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
fn browser_dispatch_and_ack_crash_at_each_journal_boundary() {
    for operation in ["dispatch", "ack"] {
        for phase in [
            "validated",
            "prepared",
            "renamed",
            "committed",
            "record-0-prepared",
            "record-0-renamed",
            "installed",
            "retired",
            "finished",
        ] {
            let root = Root::new();
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "publication::browser_invocation_tests::browser_crash_child",
                    "--nocapture",
                ])
                .env("BABBLE_BROWSER_CRASH_ROOT", &root.0)
                .env("BABBLE_BROWSER_CRASH_PHASE", phase)
                .env("BABBLE_BROWSER_CRASH_OPERATION", operation)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(73),
                "{operation}/{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let (before, after): (InvocationRecord, InvocationRecord) =
                serde_json::from_slice(&fs::read(root.0.join("browser.fixture")).unwrap()).unwrap();
            let store = FileStore::open(&root.0).unwrap();
            let current = store
                .invocation(&before.intent().key().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(
                current,
                if matches!(phase, "validated" | "prepared") {
                    before
                } else {
                    after
                }
            );
            if current.consumed_at().is_some() {
                assert!(
                    store
                        .transition_invocation(
                            &current,
                            InvocationAction::Dispatch {
                                dispatch_id: Hash::from_bytes(b"redispatch")
                            },
                            &current.intent().context,
                            at(4)
                        )
                        .is_err()
                );
            }
        }
    }
}

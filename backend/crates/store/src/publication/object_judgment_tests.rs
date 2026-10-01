use super::*;
use babble_judgment::{
    ConstantProvider, DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState,
};
use std::{process::Command, sync::atomic::AtomicU64};

pub(super) struct Root(pub(super) PathBuf);
impl Root {
    pub(super) fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "babble-object-judgments-{}-{}",
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

pub(super) fn fixture() -> (ObjectJudgmentInput, Judgment) {
    let object_id = ObjectId::from_hash(&Hash::from_bytes(b"evaluated-object"));
    let request = JudgmentRequest {
        definition: DefinitionId::relevance_v1(),
        state: JudgmentState {
            subject: object_id.to_string(),
            context: [
                ("text".into(), serde_json::json!("provider-visible text")),
                ("public_signals".into(), serde_json::json!({"count": 3})),
            ]
            .into(),
        },
        parameters: [("query".into(), serde_json::json!("provider-visible query"))].into(),
    };
    let mut judgment = ConstantProvider::default().judge(&request).unwrap();
    judgment.created_at =
        serde_json::from_value(serde_json::json!("2026-01-01T00:00:00Z")).unwrap();
    (
        ObjectJudgmentInput {
            object_id,
            judgment_id: judgment.id.clone(),
            request,
        },
        judgment,
    )
}

pub(super) fn batch(input: &ObjectJudgmentInput, judgment: &Judgment) -> PublicationBatch {
    let mut batch = PublicationBatch::new();
    batch.object_judgment(input, judgment).unwrap();
    batch
}

fn assert_pair(
    store: &FileStore,
    input: &ObjectJudgmentInput,
    judgment: &Judgment,
    committed: bool,
) {
    assert_eq!(
        store.get_object_judgment_input(&input.judgment_id).unwrap(),
        committed.then(|| input.clone())
    );
    assert_eq!(
        store.get_judgment(&judgment.id).unwrap(),
        committed.then(|| judgment.clone())
    );
    assert_eq!(
        store.object_judgment_inputs(&input.object_id).unwrap(),
        if committed {
            vec![input.clone()]
        } else {
            vec![]
        }
    );
    assert_eq!(
        store
            .latest_object_judgment_input(
                &input.object_id,
                &judgment.definition,
                &judgment.provider,
                judgment.created_at,
            )
            .unwrap(),
        committed.then(|| (input.clone(), judgment.clone()))
    );
}

#[test]
fn provider_scoped_requests_survive_restart_retry_and_metadata_refresh() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, mut judgment) = fixture();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    assert_pair(&store, &input, &judgment, true);
    judgment.created_at =
        serde_json::from_value(serde_json::json!("2026-01-02T00:00:00Z")).unwrap();
    judgment.confidence = 0.7;
    store.commit_publication(batch(&input, &judgment)).unwrap();
    assert_pair(&FileStore::open(&root.0).unwrap(), &input, &judgment, true);
}

#[test]
fn history_is_sorted_by_judgment_id_and_filters_objects() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, _) = fixture();
    let mut expected = Vec::new();
    for index in 0..8 {
        let mut next = input.clone();
        next.request
            .parameters
            .insert("query".into(), serde_json::json!(format!("query-{index}")));
        if index == 7 {
            next.object_id = ObjectId::from_hash(&Hash::from_bytes(b"other-object"));
            next.request.state.subject = next.object_id.to_string();
        }
        if index % 2 == 0 {
            next.request.definition = DefinitionId::evidence_quality_v1();
        }
        let judgment = ConstantProvider::default().judge(&next.request).unwrap();
        next.judgment_id = judgment.id.clone();
        store.commit_publication(batch(&next, &judgment)).unwrap();
        if index != 7 {
            expected.push(next);
        }
    }
    expected.sort_by(|left, right| left.judgment_id.cmp(&right.judgment_id));
    let store = FileStore::open(&root.0).unwrap();
    assert_eq!(
        store.object_judgment_inputs(&input.object_id).unwrap(),
        expected
    );
    assert!(
        store
            .object_judgment_inputs(&ObjectId::new_unchecked("invalid"))
            .is_err()
    );
    assert!(
        store
            .get_object_judgment_input(&JudgmentId::new_unchecked("invalid"))
            .is_err()
    );
}

#[test]
fn invalid_bindings_never_stage_either_record() {
    for fault in [
        "object",
        "subject",
        "request",
        "hash",
        "definition",
        "id",
        "provider",
        "parameters",
        "output",
        "output_schema",
        "confidence",
        "nan",
    ] {
        let (mut input, mut judgment) = fixture();
        match fault {
            "object" => input.object_id = ObjectId::new_unchecked("../bad"),
            "subject" => input.request.state.subject = "other-subject".into(),
            "request" => {
                input.request.state.context.remove("text");
            }
            "hash" => judgment.input_hash = Hash::from_bytes(b"wrong"),
            "definition" => judgment.definition = DefinitionId::spam_v1(),
            "id" => {
                judgment.id = JudgmentId::from_hash(&Hash::from_bytes(b"wrong"));
                input.judgment_id = judgment.id.clone();
            }
            "provider" => judgment.provider.version = "changed".into(),
            "parameters" => {
                input
                    .request
                    .parameters
                    .insert("query".into(), serde_json::json!("changed"));
            }
            "output" => judgment.output["score"] = serde_json::json!(0.7),
            "output_schema" => judgment.output["score"] = serde_json::json!(2.0),
            "confidence" => judgment.confidence = 1.1,
            "nan" => judgment.confidence = f64::NAN,
            _ => unreachable!(),
        }
        let mut batch = PublicationBatch::new();
        assert!(batch.object_judgment(&input, &judgment).is_err(), "{fault}");
        assert!(batch.records.is_empty(), "{fault}");
    }
    let (input, mut judgment) = fixture();
    judgment.output["score"] = serde_json::json!(-1.0);
    judgment.id = JudgmentId::from_hash(
        &(
            &judgment.definition,
            &judgment.provider,
            &judgment.input_hash,
            &input.request.parameters,
            &judgment.output,
        )
            .canonical_hash()
            .unwrap(),
    );
    let input = ObjectJudgmentInput {
        judgment_id: judgment.id.clone(),
        ..input
    };
    assert!(
        PublicationBatch::new()
            .object_judgment(&input, &judgment)
            .is_err()
    );
    let mut unknown = serde_json::to_value(&input).unwrap();
    unknown["unexpected"] = Value::Bool(true);
    assert!(serde_json::from_value::<ObjectJudgmentInput>(unknown).is_err());
}

#[test]
fn source_agreement_results_bind_to_the_actual_request_even_with_a_valid_commitment() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (mut input, _) = fixture();
    input.request.definition = DefinitionId::source_agreement_v1();
    input.request.parameters.clear();
    input.request.state.context.insert(
        "source_agreement".into(),
        serde_json::json!({
            "reference_time": 1234.0,
            "previous_score": null,
            "sources": []
        }),
    );
    let output = serde_json::json!({
        "kind": "source_agreement",
        "confidence": 0.0,
        "confidence_status": "uncalibrated",
        "reference_time": 1234.0,
        "source_ids": [],
        "content_id": input.object_id,
        "consensus_score": 0.0,
        "reliability_score": 0.0,
        "validation_count": 0,
        "state": "insufficient",
        "temporal_weight": 0.0,
        "term_agreement": 0.0,
        "fact_agreement": 0.0,
        "user_contributions": {},
        "limitations": ["No sources supplied"]
    });
    for fault in [
        None,
        Some("content_id"),
        Some("reference_time"),
        Some("source_ids"),
        Some("record_confidence"),
    ] {
        let mut output = output.clone();
        match fault {
            Some("content_id") => output["content_id"] = serde_json::json!("another-object"),
            Some("reference_time") => output["reference_time"] = serde_json::json!(1235.0),
            Some("source_ids") => {
                output["source_ids"] = serde_json::json!(["unsupplied-source"]);
                output["validation_count"] = serde_json::json!(1);
            }
            _ => (),
        }
        let provider = ConstantProvider::new(ConstantProvider::default().version(), output, 0.0);
        let mut judgment = provider.judge(&input.request).unwrap();
        if fault == Some("record_confidence") {
            judgment.confidence = 0.5;
        }
        input.judgment_id = judgment.id.clone();
        let mut batch = PublicationBatch::new();
        let result = batch.object_judgment(&input, &judgment);
        if fault.is_some() {
            assert!(result.is_err(), "{fault:?}");
            assert!(batch.records.is_empty());
        } else {
            result.unwrap();
            store.commit_publication(batch).unwrap();
            assert_pair(&FileStore::open(&root.0).unwrap(), &input, &judgment, true);
        }
    }
}

#[test]
fn pair_staging_preserves_record_limits_and_rejects_in_batch_duplicates() {
    let (input, judgment) = fixture();
    let mut batch = batch(&input, &judgment);
    assert!(batch.object_judgment(&input, &judgment).is_err());
    assert_eq!(batch.records.len(), 2);
    batch
        .records
        .resize(MAX_RECORDS - 1, batch.records[0].clone());
    let count = batch.records.len();
    assert!(batch.object_judgment(&input, &judgment).is_err());
    assert_eq!(batch.records.len(), count);
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
        "index-updated",
        "indexed",
        "retired",
        "finished",
    ]
    .map(str::to_string)
    .to_vec()
}

#[test]
fn errors_at_every_boundary_recover_both_records_and_block_failed_handles() {
    for phase in phases() {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        let error = store
            .commit_with_hook(batch(&input, &judgment), &mut |at| {
                if at == phase {
                    Err(conflict("injected failure"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        let committed = !matches!(phase.as_str(), "validated" | "prepared");
        assert_eq!(matches!(error, PublicationError::Precommit(_)), !committed);
        if committed {
            assert!(store.get_object_judgment_input(&input.judgment_id).is_err());
            assert!(store.object_judgment_inputs(&input.object_id).is_err());
        }
        for _ in 0..2 {
            assert_pair(
                &FileStore::open(&root.0).unwrap(),
                &input,
                &judgment,
                committed,
            );
        }
    }
}

#[test]
fn object_judgment_crash_child() {
    let Some(root) = std::env::var_os("BABBLE_OBJECT_JUDGMENT_CRASH_ROOT") else {
        return;
    };
    let phase = std::env::var("BABBLE_OBJECT_JUDGMENT_CRASH_PHASE").unwrap();
    let store = FileStore::open(PathBuf::from(root)).unwrap();
    let (input, mut judgment) = fixture();
    if std::env::var_os("BABBLE_OBJECT_JUDGMENT_REFRESH").is_some() {
        store.commit_publication(batch(&input, &judgment)).unwrap();
        judgment.confidence = 0.7;
        judgment.created_at =
            serde_json::from_value(serde_json::json!("2026-01-02T00:00:00Z")).unwrap();
    }
    store
        .commit_with_hook(batch(&input, &judgment), &mut |at| {
            if at == phase {
                std::process::exit(73);
            }
            Ok(())
        })
        .unwrap();
    panic!("crash phase not reached");
}

#[test]
fn process_exit_and_refresh_recover_atomically_at_every_boundary() {
    for refresh in [false, true] {
        for phase in phases() {
            let root = Root::new();
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args([
                    "--exact",
                    "publication::object_judgment_tests::object_judgment_crash_child",
                    "--nocapture",
                ])
                .env("BABBLE_OBJECT_JUDGMENT_CRASH_ROOT", &root.0)
                .env("BABBLE_OBJECT_JUDGMENT_CRASH_PHASE", &phase);
            if refresh {
                command.env("BABBLE_OBJECT_JUDGMENT_REFRESH", "1");
            }
            let output = command.output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(73),
                "{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let (input, mut judgment) = fixture();
            let committed = !matches!(phase.as_str(), "validated" | "prepared");
            if refresh && committed {
                judgment.confidence = 0.7;
                judgment.created_at =
                    serde_json::from_value(serde_json::json!("2026-01-02T00:00:00Z")).unwrap();
            }
            let store = FileStore::open(&root.0).unwrap();
            assert_pair(&store, &input, &judgment, committed || refresh);
            if committed || refresh {
                store.commit_publication(batch(&input, &judgment)).unwrap();
            }
            assert_pair(
                &FileStore::open(&root.0).unwrap(),
                &input,
                &judgment,
                committed || refresh,
            );
        }
    }
}

#[test]
fn corrupt_or_missing_counterparts_and_misnamed_associations_fail_closed() {
    for fault in [
        "missing",
        "corrupt",
        "judgment_id",
        "hash",
        "output",
        "confidence",
        "input",
        "input_id",
        "filename",
        "unknown",
        "oversized",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        store.commit_publication(batch(&input, &judgment)).unwrap();
        let judgment_path = store.path("judgments", judgment.id.as_str()).unwrap();
        let input_path = store
            .path("object_judgment_inputs", judgment.id.as_str())
            .unwrap();
        let mut judgment_value = serde_json::to_value(&judgment).unwrap();
        let mut input_value = serde_json::to_value(&input).unwrap();
        match fault {
            "missing" => fs::remove_file(&judgment_path).unwrap(),
            "corrupt" => fs::write(&judgment_path, b"{").unwrap(),
            "judgment_id" => {
                judgment_value["id"] =
                    serde_json::json!(JudgmentId::from_hash(&Hash::from_bytes(b"wrong")))
            }
            "hash" => judgment_value["input_hash"] = serde_json::json!(Hash::from_bytes(b"wrong")),
            "output" => judgment_value["output"]["score"] = serde_json::json!(0.3),
            "confidence" => judgment_value["confidence"] = serde_json::json!(-1),
            "input" => input_value["request"]["parameters"]["query"] = serde_json::json!("changed"),
            "input_id" => {
                input_value["judgment_id"] =
                    serde_json::json!(JudgmentId::from_hash(&Hash::from_bytes(b"wrong")))
            }
            "unknown" => input_value["extra"] = Value::Bool(true),
            "filename" => {
                fs::rename(
                    &input_path,
                    store
                        .path(
                            "object_judgment_inputs",
                            JudgmentId::from_hash(&Hash::from_bytes(b"wrong")).as_str(),
                        )
                        .unwrap(),
                )
                .unwrap();
            }
            "oversized" => fs::write(&input_path, vec![b' '; MAX_BYTES as usize + 1]).unwrap(),
            _ => unreachable!(),
        }
        if matches!(fault, "judgment_id" | "hash" | "output" | "confidence") {
            fs::write(&judgment_path, serde_json::to_vec(&judgment_value).unwrap()).unwrap();
        }
        if matches!(fault, "input" | "input_id" | "unknown") {
            fs::write(&input_path, serde_json::to_vec(&input_value).unwrap()).unwrap();
        }
        assert!(
            store.object_judgment_inputs(&input.object_id).is_err(),
            "{fault}"
        );
        assert!(
            store
                .latest_object_judgment_input(
                    &input.object_id,
                    &judgment.definition,
                    &judgment.provider,
                    judgment.created_at
                )
                .is_err(),
            "{fault}"
        );
        if fault != "filename" {
            assert!(
                store.get_object_judgment_input(&judgment.id).is_err(),
                "{fault}"
            );
        }
        // Opening now validates all canonical pairs while rebuilding the index.
        assert!(FileStore::open(&root.0).is_err(), "{fault}");
    }
}

#[test]
fn legacy_judgments_remain_readable_and_can_gain_valid_associations() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    store.put_judgment(&judgment).unwrap();
    assert_eq!(
        store.get_judgment(&judgment.id).unwrap(),
        Some(judgment.clone())
    );
    assert_eq!(store.list_judgments().unwrap(), vec![judgment.clone()]);
    assert_eq!(store.get_object_judgment_input(&judgment.id).unwrap(), None);
    assert!(
        store
            .object_judgment_inputs(&input.object_id)
            .unwrap()
            .is_empty()
    );
    store.commit_publication(batch(&input, &judgment)).unwrap();
    assert_pair(&store, &input, &judgment, true);
}

#[test]
fn conflicting_preimages_and_existing_associations_are_never_overwritten() {
    for fault in [
        "request",
        "missing_judgment",
        "invalid_judgment",
        "hash",
        "unknown",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        store.commit_publication(batch(&input, &judgment)).unwrap();
        let input_path = store
            .path("object_judgment_inputs", judgment.id.as_str())
            .unwrap();
        let judgment_path = store.path("judgments", judgment.id.as_str()).unwrap();
        let mut value = serde_json::to_value(&input).unwrap();
        match fault {
            "request" => value["request"]["parameters"]["query"] = serde_json::json!("conflict"),
            "unknown" => value["extra"] = Value::Bool(true),
            "missing_judgment" => fs::remove_file(&judgment_path).unwrap(),
            "invalid_judgment" => fs::write(&judgment_path, b"{}").unwrap(),
            "hash" => {
                let mut old = judgment.clone();
                old.input_hash = Hash::from_bytes(b"wrong");
                fs::write(&judgment_path, serde_json::to_vec(&old).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        fs::write(&input_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let before = fs::read(&input_path).unwrap();
        assert!(
            matches!(
                store.commit_publication(batch(&input, &judgment)),
                Err(PublicationError::Precommit(_))
            ),
            "{fault}"
        );
        assert_eq!(fs::read(&input_path).unwrap(), before);
        assert!(!store.root.join(JOURNAL).exists());
        assert!(store.check_ready().is_ok());
    }
}

#[test]
fn old_write_apis_preserve_existing_association_binding() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    let mut wrong = judgment.clone();
    wrong.output["score"] = serde_json::json!(0.8);
    assert!(store.put_judgment(&wrong).is_err());
    let mut update = PublicationBatch::new();
    update.judgment(&wrong).unwrap();
    assert!(store.commit_publication(update).is_err());
    assert_pair(&store, &input, &judgment, true);
    let mut refreshed = judgment.clone();
    refreshed.confidence = 0.8;
    store.put_judgment(&refreshed).unwrap();
    let mut update = PublicationBatch::new();
    update.judgment(&refreshed).unwrap();
    store.commit_publication(update).unwrap();
    assert_pair(&store, &input, &refreshed, true);
}

#[test]
fn journal_pair_and_preimage_corruption_fail_before_any_install() {
    for fault in [
        "missing_pair",
        "subject",
        "output",
        "previous_request",
        "previous_hash",
        "missing_previous_judgment",
        "current_conflict",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        store.commit_publication(batch(&input, &judgment)).unwrap();
        let mut refreshed = judgment.clone();
        refreshed.confidence = 0.7;
        assert!(matches!(
            store.commit_with_hook(batch(&input, &refreshed), &mut |at| {
                if at == "committed" {
                    Err(conflict("stop"))
                } else {
                    Ok(())
                }
            }),
            Err(PublicationError::Committed { .. })
        ));
        let path = root.0.join(JOURNAL);
        let mut journal: Journal = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let mut records: Vec<Record> = serde_json::from_str(&journal.payload).unwrap();
        match fault {
            "missing_pair" => {
                records.pop();
            }
            "subject" => {
                records[0].value["request"]["state"]["subject"] = serde_json::json!("wrong")
            }
            "output" => records[1].value["output"]["score"] = serde_json::json!(0.9),
            "previous_request" => {
                records[0].previous.as_mut().unwrap()["request"]["parameters"]["query"] =
                    serde_json::json!("wrong")
            }
            "previous_hash" => {
                records[1].previous.as_mut().unwrap()["input_hash"] =
                    serde_json::json!(Hash::from_bytes(b"wrong"))
            }
            "missing_previous_judgment" => records[1].previous = None,
            "current_conflict" => {
                let mut value = serde_json::to_value(&input).unwrap();
                value["request"]["parameters"]["query"] = serde_json::json!("wrong");
                fs::write(
                    store
                        .path("object_judgment_inputs", judgment.id.as_str())
                        .unwrap(),
                    serde_json::to_vec(&value).unwrap(),
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        journal.payload = serde_json::to_string(&records).unwrap();
        journal.digest = Hash::from_bytes(journal.payload.as_bytes());
        fs::write(&path, serde_json::to_vec(&journal).unwrap()).unwrap();
        assert!(FileStore::open(&root.0).is_err(), "{fault}");
        assert!(path.exists());
        assert_eq!(
            read_json::<Judgment>(&store.path("judgments", judgment.id.as_str()).unwrap()).unwrap(),
            Some(judgment)
        );
    }
}

#[test]
fn association_readers_wait_for_the_counterpart_install() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    let reader = store.clone();
    let id = input.judgment_id.clone();
    let (start_tx, start_rx) = std::sync::mpsc::channel();
    let (done_tx, done_rx) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        start_rx.recv().unwrap();
        done_tx.send(reader.get_object_judgment_input(&id)).unwrap();
    });
    store
        .commit_with_hook(batch(&input, &judgment), &mut |at| {
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
    assert_eq!(done_rx.recv().unwrap().unwrap(), Some(input));
    thread.join().unwrap();
}

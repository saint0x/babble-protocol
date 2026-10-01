use super::object_judgment_tests::{Root, batch, fixture};
use super::*;
use babel_judgment::{ConstantProvider, DefinitionId, JudgmentProvider};
use rusqlite::{Connection, params};

const INDEX: &str = "object-judgments.sqlite3";

fn time(value: &str) -> Timestamp {
    serde_json::from_value(serde_json::json!(value)).unwrap()
}

fn variant(label: &str) -> (ObjectJudgmentInput, Judgment) {
    let (mut input, _) = fixture();
    input
        .request
        .parameters
        .insert("query".into(), serde_json::json!(label));
    let mut judgment = ConstantProvider::default().judge(&input.request).unwrap();
    judgment.created_at = time("2026-01-01T00:00:00Z");
    input.judgment_id = judgment.id.clone();
    (input, judgment)
}

fn latest(
    store: &FileStore,
    input: &ObjectJudgmentInput,
    judgment: &Judgment,
    reference: Timestamp,
) -> Option<(ObjectJudgmentInput, Judgment)> {
    store
        .latest_object_judgment_input(
            &input.object_id,
            &judgment.definition,
            &judgment.provider,
            reference,
        )
        .unwrap()
}

#[test]
fn open_migrates_flat_pairs_and_repairs_missing_corrupt_unknown_or_incomplete_indexes() {
    for fault in [
        "missing",
        "corrupt",
        "version",
        "application",
        "schema",
        "readiness",
        "omitted",
        "misindexed",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        // Existing flat-file data predates the derived SQLite index.
        write_json(
            &store
                .path("object_judgment_inputs", input.judgment_id.as_str())
                .unwrap(),
            &input,
        )
        .unwrap();
        write_json(
            &store.path("judgments", judgment.id.as_str()).unwrap(),
            &judgment,
        )
        .unwrap();
        let path = root.0.join(INDEX);
        match fault {
            "missing" => fs::remove_file(&path).unwrap(),
            "corrupt" => fs::write(&path, b"not sqlite").unwrap(),
            _ => {
                let db = Connection::open(&path).unwrap();
                db.execute_batch(match fault {
                    "version" => "PRAGMA user_version=999;",
                    "application" => "PRAGMA application_id=0;",
                    "schema" => "DROP TABLE entries;",
                    "readiness" => "DELETE FROM readiness;",
                    "omitted" => "DELETE FROM entries;",
                    "misindexed" => {
                        "INSERT INTO entries VALUES ('wrong','wrong','wrong','wrong',0,0);"
                    }
                    _ => unreachable!(),
                })
                .unwrap();
            }
        }
        if !matches!(fault, "omitted" | "misindexed") {
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
        }
        let reopened = FileStore::open(&root.0).unwrap();
        assert_eq!(
            reopened.object_judgment_inputs(&input.object_id).unwrap(),
            vec![input.clone()],
            "{fault}"
        );
        assert_eq!(
            latest(&reopened, &input, &judgment, judgment.created_at),
            Some((input.clone(), judgment.clone())),
            "{fault}"
        );
        // Older handles acquire a fresh connection after the atomic replacement.
        assert_eq!(
            latest(&store, &input, &judgment, judgment.created_at),
            Some((input, judgment))
        );
    }
}

#[test]
fn latest_filters_exact_scope_and_orders_full_precision_time_then_id() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    let reference = time("2026-01-01T00:00:00.000000002Z");
    assert_eq!(latest(&store, &input, &judgment, reference), None);
    let mut candidates = Vec::new();
    for (label, timestamp) in [
        ("old", "1960-01-01T00:00:00Z"),
        ("nanosecond-1", "2026-01-01T00:00:00.000000001Z"),
        ("tie-a", "2026-01-01T00:00:00.000000002Z"),
        ("tie-b", "2026-01-01T01:00:00.000000002+01:00"),
        ("future", "2026-01-01T00:00:00.000000003Z"),
    ] {
        let (input, mut judgment) = variant(label);
        judgment.created_at = time(timestamp);
        store.commit_publication(batch(&input, &judgment)).unwrap();
        candidates.push((input, judgment));
    }
    for scope in ["object", "definition", "provider", "model", "version"] {
        let (mut other, _) = variant(scope);
        let mut provider = judgment.provider.clone();
        match scope {
            "object" => {
                other.object_id = ObjectId::from_hash(&Hash::from_bytes(b"other"));
                other.request.state.subject = other.object_id.to_string();
            }
            "definition" => other.request.definition = DefinitionId::evidence_quality_v1(),
            "provider" => provider.provider.push_str("other"),
            "model" => provider.model.push_str("other"),
            "version" => provider.version.push_str("other"),
            _ => unreachable!(),
        }
        let mut result =
            ConstantProvider::new(provider, judgment.output.clone(), judgment.confidence)
                .judge(&other.request)
                .unwrap();
        result.created_at = reference;
        other.judgment_id = result.id.clone();
        store.commit_publication(batch(&other, &result)).unwrap();
    }
    let expected = candidates
        .iter()
        .filter(|(_, j)| j.created_at <= reference)
        .max_by_key(|(_, j)| (j.created_at, &j.id))
        .unwrap()
        .clone();
    assert_eq!(latest(&store, &input, &judgment, reference), Some(expected));
    assert_eq!(
        latest(&store, &input, &judgment, time("1959-12-31T23:59:59Z")),
        None
    );
    assert_eq!(
        latest(&store, &input, &judgment, time("1960-01-01T00:00:00Z")),
        Some(candidates[0].clone())
    );
    assert!(
        store
            .latest_object_judgment_input(
                &ObjectId::new_unchecked("bad"),
                &judgment.definition,
                &judgment.provider,
                reference
            )
            .is_err()
    );
}

#[test]
fn separate_handles_observe_publication_legacy_refresh_and_rebuilt_index() {
    let root = Root::new();
    let first = FileStore::open(&root.0).unwrap();
    let second = FileStore::open(&root.0).unwrap();
    let (input, mut judgment) = fixture();
    first.commit_publication(batch(&input, &judgment)).unwrap();
    let old = judgment.created_at;
    assert!(latest(&second, &input, &judgment, old).is_some());
    judgment.created_at = time("2026-01-02T00:00:00Z");
    judgment.confidence = 0.8;
    second.put_judgment(&judgment).unwrap();
    let third = FileStore::open(&root.0).unwrap();
    for store in [&first, &second, &third] {
        assert_eq!(latest(store, &input, &judgment, old), None);
        assert_eq!(
            latest(store, &input, &judgment, judgment.created_at),
            Some((input.clone(), judgment.clone()))
        );
    }
    let (other, result) = variant("second-writer");
    first.commit_publication(batch(&other, &result)).unwrap();
    for store in [&second, &third] {
        let history = store.object_judgment_inputs(&input.object_id).unwrap();
        assert_eq!(history.len(), 2);
        assert!(
            history
                .windows(2)
                .all(|v| v[0].judgment_id < v[1].judgment_id)
        );
    }
}

#[test]
fn failed_sqlite_update_rolls_back_all_rows_and_keeps_journal_until_rebuilt() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let observer = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    let (other, result) = variant("second-row");
    let db = Connection::open(root.0.join(INDEX)).unwrap();
    db.execute_batch(&format!(
        "CREATE TRIGGER fail_second BEFORE INSERT ON entries WHEN NEW.id='{}' BEGIN SELECT RAISE(ABORT, 'injected SQLite failure'); END;", result.id
    )).unwrap();
    let mut publication = batch(&input, &judgment);
    publication.object_judgment(&other, &result).unwrap();
    assert!(matches!(
        store.commit_publication(publication),
        Err(PublicationError::Committed { .. })
    ));
    assert_eq!(
        db.query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(db);
    assert!(root.0.join(JOURNAL).exists());
    assert!(observer.object_judgment_inputs(&input.object_id).is_err());
    let recovered = FileStore::open(&root.0).unwrap();
    assert_eq!(
        recovered
            .object_judgment_inputs(&input.object_id)
            .unwrap()
            .len(),
        2
    );
    assert!(latest(&recovered, &input, &judgment, judgment.created_at).is_some());
    assert!(!root.0.join(JOURNAL).exists());
    assert!(store.check_ready().is_err());
    assert_eq!(
        observer
            .object_judgment_inputs(&input.object_id)
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn legacy_refresh_index_failure_is_recovered_from_its_journal() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, mut judgment) = fixture();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    Connection::open(root.0.join(INDEX)).unwrap().execute_batch(
        "CREATE TRIGGER fail_refresh BEFORE UPDATE ON entries BEGIN SELECT RAISE(ABORT, 'injected refresh failure'); END;"
    ).unwrap();
    let old = judgment.created_at;
    judgment.created_at = time("2026-01-02T00:00:00Z");
    assert!(store.put_judgment(&judgment).is_err());
    assert!(root.0.join(JOURNAL).exists());
    let recovered = FileStore::open(&root.0).unwrap();
    assert_eq!(latest(&recovered, &input, &judgment, old), None);
    assert_eq!(
        latest(&recovered, &input, &judgment, judgment.created_at),
        Some((input, judgment))
    );
}

#[test]
fn selected_index_metadata_must_match_canonical_pair() {
    for column in [
        "object_id",
        "definition",
        "provider",
        "created_seconds",
        "created_nanos",
        "id",
    ] {
        let root = Root::new();
        let store = FileStore::open(&root.0).unwrap();
        let (input, judgment) = fixture();
        store.commit_publication(batch(&input, &judgment)).unwrap();
        let mut object_id = input.object_id.clone();
        let mut definition = judgment.definition.clone();
        let mut provider = judgment.provider.clone();
        let db = Connection::open(root.0.join(INDEX)).unwrap();
        match column {
            "object_id" => {
                object_id = ObjectId::from_hash(&Hash::from_bytes(b"misindexed-object"));
                db.execute("UPDATE entries SET object_id=?1", [object_id.as_str()])
                    .unwrap();
            }
            "definition" => {
                definition = DefinitionId::evidence_quality_v1();
                db.execute("UPDATE entries SET definition=?1", [definition.as_str()])
                    .unwrap();
            }
            "provider" => {
                provider.version.push_str("changed");
                db.execute(
                    "UPDATE entries SET provider=?1",
                    [serde_json::to_string(&provider).unwrap()],
                )
                .unwrap();
            }
            "created_seconds" => {
                db.execute("UPDATE entries SET created_seconds=created_seconds-1", [])
                    .unwrap();
            }
            "created_nanos" => {
                db.execute("UPDATE entries SET created_nanos=1", [])
                    .unwrap();
            }
            "id" => {
                db.execute(
                    "UPDATE entries SET id=?1",
                    [JudgmentId::from_hash(&Hash::from_bytes(b"absent")).as_str()],
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        drop(db);
        assert!(
            store.object_judgment_inputs(&object_id).is_err(),
            "{column}"
        );
        assert!(
            store
                .latest_object_judgment_input(
                    &object_id,
                    &definition,
                    &provider,
                    time("2026-01-02T00:00:00Z")
                )
                .is_err(),
            "{column}"
        );
        let repaired = FileStore::open(&root.0).unwrap();
        assert_eq!(
            latest(&repaired, &input, &judgment, judgment.created_at),
            Some((input, judgment))
        );
    }
}

#[test]
fn queries_read_only_requested_canonical_pairs_and_latest_reads_exactly_one() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    let (older, mut older_result) = variant("older");
    older_result.created_at = time("2025-01-01T00:00:00Z");
    store
        .commit_publication(batch(&older, &older_result))
        .unwrap();
    let (mut unrelated, _) = variant("unrelated");
    unrelated.object_id = ObjectId::from_hash(&Hash::from_bytes(b"unrelated"));
    unrelated.request.state.subject = unrelated.object_id.to_string();
    let result = ConstantProvider::default()
        .judge(&unrelated.request)
        .unwrap();
    unrelated.judgment_id = result.id.clone();
    store
        .commit_publication(batch(&unrelated, &result))
        .unwrap();
    // Instrument the actual canonical read boundary, not a timing proxy.
    RECORD_READS.with(|reads| *reads.borrow_mut() = Some(Vec::new()));
    let selected = latest(&store, &input, &judgment, judgment.created_at);
    let reads = RECORD_READS.with(|reads| reads.borrow_mut().take().unwrap());
    assert_eq!(selected, Some((input.clone(), judgment.clone())));
    assert_eq!(
        reads,
        vec![
            store
                .path("object_judgment_inputs", judgment.id.as_str())
                .unwrap(),
            store.path("judgments", judgment.id.as_str()).unwrap(),
        ]
    );
    RECORD_READS.with(|reads| *reads.borrow_mut() = Some(Vec::new()));
    assert_eq!(
        store
            .object_judgment_inputs(&input.object_id)
            .unwrap()
            .len(),
        2
    );
    let reads = RECORD_READS.with(|reads| reads.borrow_mut().take().unwrap());
    assert_eq!(reads.len(), 4);
    assert!(reads.iter().all(|path| {
        let id = path.file_stem().unwrap().to_str().unwrap();
        id == judgment.id.as_str() || id == older_result.id.as_str()
    }));
    // Unrelated corruption does not expand a targeted query; reopen audits all.
    fs::write(store.path("judgments", result.id.as_str()).unwrap(), b"{").unwrap();
    assert_eq!(
        store
            .object_judgment_inputs(&input.object_id)
            .unwrap()
            .len(),
        2
    );
    assert!(store.object_judgment_inputs(&unrelated.object_id).is_err());
    assert!(FileStore::open(&root.0).is_err());
    // A corrupt selected pair must error, never fall back to an older result.
    fs::write(store.path("judgments", judgment.id.as_str()).unwrap(), b"{").unwrap();
    assert!(
        store
            .latest_object_judgment_input(
                &input.object_id,
                &judgment.definition,
                &judgment.provider,
                judgment.created_at
            )
            .is_err()
    );
}

#[test]
fn latest_sqlite_plan_uses_covering_range_index_without_sort_or_scan() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    store.commit_publication(batch(&input, &judgment)).unwrap();
    let db = Connection::open(root.0.join(INDEX)).unwrap();
    let mut statement = db.prepare("EXPLAIN QUERY PLAN SELECT id,object_id,definition,provider,created_seconds,created_nanos FROM entries
        WHERE object_id=?1 AND definition=?2 AND provider=?3 AND (created_seconds,created_nanos)<=(?4,?5)
        ORDER BY created_seconds DESC,created_nanos DESC,id DESC LIMIT 1").unwrap();
    let plan = statement
        .query_map(
            params![
                input.object_id.as_str(),
                judgment.definition.as_str(),
                serde_json::to_string(&judgment.provider).unwrap(),
                judgment.created_at.0.unix_timestamp(),
                0
            ],
            |r| r.get::<_, String>(3),
        )
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap()
        .join("\n");
    assert!(
        plan.contains("SEARCH entries USING COVERING INDEX latest_input"),
        "{plan}"
    );
    assert!(
        !plan.contains("TEMP B-TREE") && !plan.contains("SCAN"),
        "{plan}"
    );
}

#[test]
fn recovery_rebuild_failure_retains_committed_journal() {
    let root = Root::new();
    let store = FileStore::open(&root.0).unwrap();
    let (input, judgment) = fixture();
    assert!(matches!(
        store.commit_with_hook(batch(&input, &judgment), &mut |phase| {
            if phase == "committed" {
                Err(conflict("injected stop"))
            } else {
                Ok(())
            }
        }),
        Err(PublicationError::Committed { .. })
    ));
    let bad = store
        .path(
            "object_judgment_inputs",
            JudgmentId::from_hash(&Hash::from_bytes(b"bad")).as_str(),
        )
        .unwrap();
    fs::write(&bad, b"{").unwrap();
    assert!(FileStore::open(&root.0).is_err());
    assert!(root.0.join(JOURNAL).exists());
    fs::remove_file(bad).unwrap();
    let recovered = FileStore::open(&root.0).unwrap();
    assert_eq!(
        latest(&recovered, &input, &judgment, judgment.created_at),
        Some((input, judgment))
    );
    assert!(!root.0.join(JOURNAL).exists());
}

use super::*;
use crate::{RepliesListQuery, object_provider_judgment_state};
use babble_authoring::ObjectDraft;
use babble_identity::{Identity, IdentityKind};
use babble_judgment::ProviderVersion;
use babble_judgment_local::LocalProvider;
use babble_media::MediaBlob;
use babble_object::CapabilityRequest;
use babble_state::EventKind;
use babble_types::{Canonical, Hash, ObjectId, Timestamp};
use serde_json::json;
use std::{
    cell::RefCell,
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[derive(Clone, Copy, Debug)]
enum Fault {
    Unavailable,
    Definition,
    Provider,
    InputHash,
    Output,
    Confidence,
    NonFiniteConfidence,
    Id,
}

#[derive(Default)]
struct FaultProvider {
    calls: RefCell<Vec<JudgmentRequest>>,
    fault: RefCell<Option<(usize, Fault)>>,
    install_block: RefCell<Option<PathBuf>>,
}

impl JudgmentProvider for FaultProvider {
    fn version(&self) -> ProviderVersion {
        LocalProvider::default().version()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        let mut calls = self.calls.borrow_mut();
        calls.push(request.clone());
        let fault = self.fault.borrow().filter(|(at, _)| *at == calls.len());
        if matches!(fault, Some((_, Fault::Unavailable))) {
            return Err(Error::ProviderUnavailable("test worker unavailable".into()));
        }
        let mut judgment = LocalProvider::default().judge(request)?;
        if calls.len() == 4
            && let Some(root) = self.install_block.borrow().as_ref()
        {
            fs::create_dir(
                root.join("judgments")
                    .join(format!("{}.publication-tmp", judgment.id)),
            )
            .unwrap();
        }
        match fault.map(|(_, fault)| fault) {
            Some(Fault::Definition) => judgment.definition = DefinitionId::relevance_v1(),
            Some(Fault::Provider) => judgment.provider.version = "wrong-version".into(),
            Some(Fault::InputHash) => judgment.input_hash = Hash::from_bytes(b"wrong input"),
            Some(Fault::Output) => judgment.output = json!({"invalid": true}),
            Some(Fault::Confidence) => judgment.confidence = 1.5,
            Some(Fault::NonFiniteConfidence) => judgment.confidence = f64::NAN,
            Some(Fault::Id) => judgment.id = babble_types::JudgmentId::new_unchecked("invalid"),
            _ => {}
        }
        Ok(judgment)
    }
}

impl FaultProvider {
    fn arm(&self, at: usize, fault: Fault) {
        self.calls.borrow_mut().clear();
        *self.fault.borrow_mut() = Some((at, fault));
    }

    fn recover(&self) {
        self.calls.borrow_mut().clear();
        *self.fault.borrow_mut() = None;
    }
}

struct TestRoot(PathBuf);

impl TestRoot {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-node-publication-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

impl Drop for TestRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[derive(Clone, Copy, Debug)]
enum Publication {
    Text,
    Record,
    Draft,
    Media,
    MediaDraft,
    Reply,
    Share,
    Fork,
    Remix,
}

struct Context {
    author: Identity,
    source: Object,
    target: Object,
    record: Object,
    media: MediaBlob,
}

fn setup(node: &mut LocalNode<FaultProvider>) -> Context {
    let author = node
        .create_identity(IdentityKind::Person, "publisher")
        .unwrap();
    let owner = node
        .create_identity(IdentityKind::Person, "source-owner")
        .unwrap();
    let target = node
        .publish_text(&owner.id, "Source evidence and methodology")
        .unwrap();
    let capabilities = ["babble.social.reply", "babble.social.share"].map(|id| CapabilityRequest {
        id: id.into(),
        version: 1,
        scope: json!({"object_id": target.id}),
    });
    let mut draft = ObjectDraft::text("Social source app").unwrap();
    for capability in &capabilities {
        draft = draft.with_capability(capability.clone()).unwrap();
    }
    let source = node.publish_draft(&owner.id, draft).unwrap();
    let record = Object::text(&author, "Signed publication with evidence")
        .unwrap()
        .sign(&author, node.local_keypair(&author.id).unwrap())
        .unwrap();
    let media = node
        .put_media_blob("text/plain", b"publication media evidence")
        .unwrap();
    node.judgment_provider.recover();
    Context {
        author,
        source,
        target,
        record,
        media,
    }
}

fn publish(
    node: &mut LocalNode<FaultProvider>,
    context: &Context,
    path: Publication,
) -> Result<Object> {
    let author = &context.author.id;
    match path {
        Publication::Text => node.publish_text(author, "Publication with source evidence"),
        Publication::Record => node.publish_object_record(author, context.record.clone()),
        Publication::Draft => node.publish_draft(author, ObjectDraft::text("Draft evidence")?),
        Publication::Media => {
            node.publish_media_object(author, "Evidence media", None, vec![context.media.clone()])
        }
        Publication::MediaDraft => node.publish_draft(
            author,
            ObjectDraft::media("Evidence media draft", None, vec![context.media.clone()])?,
        ),
        Publication::Reply => crate::invocations::tests::invoke(node, author, &context.source,
            &context.target.id, "reply", Some("Reply with evidence"), None)
            .map(|publication| publication.object.unwrap()),
        Publication::Share => crate::invocations::tests::invoke(node, author, &context.source,
            &context.target.id, "share", Some("Share with evidence"), None)
            .map(|publication| publication.object.unwrap()),
        Publication::Fork => node
            .fork_object(
                author,
                &context.target.id,
                ObjectDraft::text("Forked evidence")?,
            )
            .map(|publication| publication.object),
        Publication::Remix => node
            .remix_object(
                author,
                vec![context.target.id.clone(), context.source.id.clone()],
                ObjectDraft::text("Remixed evidence")?,
            )
            .map(|publication| publication.object),
    }
}

fn assert_cache_unchanged(node: &LocalNode<FaultProvider>, requests: &[JudgmentRequest]) {
    for request in requests {
        let key = cache_key(&node.judgment_provider.version(), request).unwrap();
        assert!(
            node.judgment_cache.entry(&key).is_none(),
            "partial cache for {}",
            request.definition.as_str()
        );
    }
}

fn publication_failure_case(path: Publication) {
    for failure_at in 1..=4 {
        let root = TestRoot::new();
        let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        let context = setup(&mut node);
        let objects_before = node.store.list_objects().unwrap();
        let events_before = node.store.list_events().unwrap();
        let edges_before = node.store.list_edges().unwrap();
        let judgments_before = node.store.list_judgments().unwrap();
        let heads_before = node.latest_event_ids(100).unwrap();
        node.judgment_provider.arm(failure_at, Fault::Unavailable);
        let error = publish(&mut node, &context, path).unwrap_err();
        assert!(
            matches!(error, Error::ProviderUnavailable(_)),
            "{path:?}: {error}"
        );
        let requests = node.judgment_provider.calls.borrow().clone();
        assert_eq!(requests.len(), failure_at);
        let failed_id = ObjectId::new_unchecked(requests[0].state.subject.clone());
        assert!(node.object(&failed_id).is_none());
        assert_eq!(node.latest_event_ids(100).unwrap(), heads_before);
        assert_eq!(node.store.list_objects().unwrap(), objects_before);
        assert_eq!(node.store.list_events().unwrap(), events_before);
        assert_eq!(node.store.list_edges().unwrap(), edges_before);
        assert_eq!(node.store.list_judgments().unwrap(), judgments_before);
        assert_cache_unchanged(&node, &requests);
        assert!(
            node.list_replies(&RepliesListQuery {
                object_id: context.target.id.clone(),
                cursor: None,
                limit: 50
            })
            .unwrap()
            .replies
            .is_empty()
        );
        let reopened = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        assert!(reopened.object(&failed_id).is_none());
        drop(reopened);

        node.judgment_provider.recover();
        let object = publish(&mut node, &context, path).unwrap();
        assert_eq!(node.judgment_provider.calls.borrow().len(), 4);
        object.verify(&context.author).unwrap();
        assert_eq!(node.object(&object.id), Some(&object));
        assert_eq!(
            node.store.list_objects().unwrap().len(),
            objects_before.len() + 1
        );
        let extra_edges = match path {
            Publication::Remix => 2,
            Publication::Reply | Publication::Share | Publication::Fork => 1,
            _ => 0,
        };
        assert_eq!(
            node.store.list_edges().unwrap().len(),
            edges_before.len() + extra_edges
        );
        assert_eq!(
            node.store.list_events().unwrap().len(),
            events_before.len() + 1 + extra_edges
        );
        for event in node.store.list_events().unwrap() {
            event
                .verify(
                    &node
                        .state
                        .signing_identity_at(&event.actor, event.created_at)
                        .unwrap(),
                )
                .unwrap();
        }
        let expected_kind = match path {
            Publication::Fork => EventKind::ObjectForked,
            Publication::Remix => EventKind::ObjectRemixed,
            _ => EventKind::ObjectPublished,
        };
        assert!(
            node.store
                .list_events()
                .unwrap()
                .iter()
                .any(|event| event.kind == expected_kind
                    && event.target == babble_state::EventTarget::Object(object.id.clone()))
        );
        let judgments = node.object_judgments(&object.id).unwrap();
        assert_eq!(judgments.len(), 4);
        for judgment in &judgments {
            assert_eq!(
                judgment.input_hash,
                object_provider_judgment_state(&object)
                    .canonical_hash()
                    .unwrap()
            );
            assert_eq!(judgment.provider, node.judgment_provider.version());
            let cached = node
                .judge_object_orchestrated(&object.id, judgment.definition.clone(), BTreeMap::new())
                .unwrap();
            assert!(cached.cache_hit);
            let persisted: Judgment =
                serde_json::from_slice(&serde_json::to_vec(&cached.judgment).unwrap()).unwrap();
            assert_eq!(&persisted, judgment);
            assert_eq!(
                cached.decisions[0].privacy.provider_input_hash,
                judgment.input_hash
            );
            assert_eq!(
                cached.decisions[0].privacy.original_input_hash,
                object_judgment_state(&object).canonical_hash().unwrap()
            );
        }
        assert_eq!(node.judgment_provider.calls.borrow().len(), 4);
        drop(node);
        let mut reopened = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        assert_eq!(reopened.object(&object.id), Some(&object));
        assert_eq!(reopened.object_judgments(&object.id).unwrap(), judgments);
        if matches!(path, Publication::Reply) {
            let replies = reopened
                .list_replies(&RepliesListQuery {
                    object_id: context.target.id.clone(),
                    cursor: None,
                    limit: 50,
                })
                .unwrap();
            assert_eq!(replies.replies.len(), 1);
            assert_eq!(replies.replies[0].object, object);
        }
        assert_eq!(
            reopened.ingest_object_judgments(&object.id).unwrap().len(),
            4
        );
    }
}

#[test]
fn publication_text_failure_is_atomic() {
    publication_failure_case(Publication::Text);
}
#[test]
fn publication_record_failure_is_atomic() {
    publication_failure_case(Publication::Record);
}
#[test]
fn publication_draft_failure_is_atomic() {
    publication_failure_case(Publication::Draft);
}
#[test]
fn publication_media_failure_is_atomic() {
    publication_failure_case(Publication::Media);
}
#[test]
fn publication_media_draft_failure_is_atomic() {
    publication_failure_case(Publication::MediaDraft);
}
#[test]
fn publication_reply_failure_is_atomic() {
    publication_failure_case(Publication::Reply);
}
#[test]
fn publication_share_failure_is_atomic() {
    publication_failure_case(Publication::Share);
}
#[test]
fn publication_fork_failure_is_atomic() {
    publication_failure_case(Publication::Fork);
}
#[test]
fn publication_remix_failure_is_atomic() {
    publication_failure_case(Publication::Remix);
}

#[test]
fn publication_rejects_invalid_judgments_without_cache_poisoning() {
    for fault in [
        Fault::Definition,
        Fault::Provider,
        Fault::InputHash,
        Fault::Output,
        Fault::Confidence,
        Fault::NonFiniteConfidence,
        Fault::Id,
    ] {
        for at in [1, 4] {
            let root = TestRoot::new();
            let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
            let context = setup(&mut node);
            let before = (
                node.store.list_objects().unwrap(),
                node.store.list_events().unwrap(),
                node.store.list_judgments().unwrap(),
            );
            node.judgment_provider.arm(at, fault);
            assert!(
                node.publish_object_record(&context.author.id, context.record.clone())
                    .is_err(),
                "{fault:?}"
            );
            assert_eq!(
                (
                    node.store.list_objects().unwrap(),
                    node.store.list_events().unwrap(),
                    node.store.list_judgments().unwrap()
                ),
                before
            );
            assert!(node.object(&context.record.id).is_none());
            assert_cache_unchanged(&node, &node.judgment_provider.calls.borrow());
            node.judgment_provider.recover();
            node.publish_object_record(&context.author.id, context.record.clone())
                .unwrap();
            assert_eq!(node.judgment_provider.calls.borrow().len(), 4, "{fault:?}");
        }
    }
}

#[test]
fn publication_reingestion_failure_preserves_existing_cache_hits_and_records() {
    let root = TestRoot::new();
    let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
    let context = setup(&mut node);
    let object = node
        .publish_object_record(&context.author.id, context.record)
        .unwrap();
    drop(node);
    let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
    node.judge_object(&object.id, DefinitionId::spam_v1(), BTreeMap::new())
        .unwrap();
    let request = node.judgment_provider.calls.borrow()[0].clone();
    let key = cache_key(&node.judgment_provider.version(), &request).unwrap();
    let cached_before = node.judgment_cache.entry(&key).unwrap().clone();
    let records_before = node.store.list_judgments().unwrap();
    node.judgment_provider.arm(3, Fault::Unavailable);
    assert!(node.ingest_object_judgments(&object.id).is_err());
    assert_eq!(node.judgment_cache.entry(&key), Some(&cached_before));
    assert_eq!(node.store.list_judgments().unwrap(), records_before);
    assert_cache_unchanged(&node, &node.judgment_provider.calls.borrow());
    node.judgment_provider.recover();
    assert_eq!(node.ingest_object_judgments(&object.id).unwrap().len(), 4);
    assert_eq!(node.judgment_provider.calls.borrow().len(), 3);
    assert_eq!(
        node.judgment_cache.entry(&key).unwrap().hits,
        cached_before.hits + 1
    );
}

#[test]
fn publication_disk_precommit_failure_preserves_all_indexes_and_cache() {
    for path in [
        Publication::Text,
        Publication::Record,
        Publication::Draft,
        Publication::Media,
        Publication::MediaDraft,
        Publication::Reply,
        Publication::Share,
        Publication::Fork,
        Publication::Remix,
    ] {
        let root = TestRoot::new();
        let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        let context = setup(&mut node);
        let before = (
            node.store.list_objects().unwrap(),
            node.store.list_events().unwrap(),
            node.store.list_edges().unwrap(),
            node.store.list_judgments().unwrap(),
        );
        let invocation = if matches!(path, Publication::Reply | Publication::Share) {
            let auth = crate::invocations::tests::host_context(&node, &context.author.id, &context.source);
            let action = if matches!(path, Publication::Reply) { "reply" } else { "share" };
            let record = node.prepare_social_invocation(auth.clone(), "precommit", &format!("babble.social.{action}"),
                crate::SocialInvocationPayload { target_object_id: context.target.id.clone(), text: Some("evidence".into()), media: None },
                Timestamp(Timestamp::now().0 + time::Duration::seconds(60))).unwrap();
            node.decide_social_invocation(&auth, "precommit", record.id(), true).unwrap();
            Some((auth, record))
        } else { None };
        fs::create_dir(root.0.join(".publication-prepared")).unwrap();
        let error = match invocation {
            Some((auth, record)) => node.execute_social_invocation(&auth, "precommit", record.id()).map(|result| result.object.unwrap()),
            None => publish(&mut node, &context, path),
        }.unwrap_err();
        assert!(
            error.to_string().contains("not committed"),
            "{path:?}: {error}"
        );
        let requests = node.judgment_provider.calls.borrow().clone();
        assert_eq!(requests.len(), 4);
        let id = ObjectId::new_unchecked(requests[0].state.subject.clone());
        assert!(node.object(&id).is_none());
        assert!(node.state.graph().outgoing(&id).is_empty());
        assert_cache_unchanged(&node, &requests);
        assert_eq!(
            (
                node.store.list_objects().unwrap(),
                node.store.list_events().unwrap(),
                node.store.list_edges().unwrap(),
                node.store.list_judgments().unwrap()
            ),
            before
        );
        assert!(
            node.list_replies(&RepliesListQuery {
                object_id: context.target.id.clone(),
                cursor: None,
                limit: 50
            })
            .unwrap()
            .replies
            .is_empty()
        );
        fs::remove_dir(root.0.join(".publication-prepared")).unwrap();
        drop(node);
        let node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        assert!(node.object(&id).is_none());
    }
}

#[test]
fn publication_disk_postcommit_failure_recovers_social_and_provenance_atomically() {
    for path in [
        Publication::Text,
        Publication::Record,
        Publication::Draft,
        Publication::Media,
        Publication::MediaDraft,
        Publication::Reply,
        Publication::Share,
        Publication::Fork,
        Publication::Remix,
    ] {
        let root = TestRoot::new();
        let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        let context = setup(&mut node);
        let objects_before = node.store.list_objects().unwrap().len();
        let events_before = node.store.list_events().unwrap().len();
        let edges_before = node.store.list_edges().unwrap().len();
        *node.judgment_provider.install_block.borrow_mut() = Some(root.0.clone());
        let error = publish(&mut node, &context, path).unwrap_err();
        assert!(
            error.to_string().contains("committed; recovery required"),
            "{path:?}: {error}"
        );
        let requests = node.judgment_provider.calls.borrow().clone();
        let id = ObjectId::new_unchecked(requests[0].state.subject.clone());
        assert!(node.object(&id).is_none());
        assert!(node.state.graph().outgoing(&id).is_empty());
        assert_cache_unchanged(&node, &requests);
        assert!(node.store.list_objects().is_err());
        assert!(node.store.list_events().is_err());
        assert!(node.check_ready().is_err());
        assert!(
            node.list_replies(&RepliesListQuery {
                object_id: context.target.id.clone(),
                cursor: None,
                limit: 50,
            })
            .is_err()
        );
        assert!(
            node.grants_belong_to(&context.source.id, &context.author.id, &[])
                .is_err()
        );
        assert!(
            node.put_media_blob("text/plain", b"must not write while poisoned")
                .is_err()
        );
        let keypair = babble_crypto::Keypair::generate();
        let later_identity =
            Identity::create(IdentityKind::Person, "must-not-apply", &keypair).unwrap();
        assert!(
            node.import_signing_identity(later_identity.clone(), keypair)
                .is_err()
        );
        assert!(node.identity(&later_identity.id).is_none());
        assert!(
            node.create_identity(IdentityKind::Person, "must-not-create")
                .is_err()
        );
        assert!(LocalNode::open(&root.0, FaultProvider::default()).is_err());
        for entry in fs::read_dir(root.0.join("judgments")).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() && path.extension().is_some_and(|ext| ext == "publication-tmp") {
                fs::remove_dir(path).unwrap();
            }
        }
        drop(node);
        let reopened = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
        let object = reopened.object(&id).unwrap();
        let expected_edges = match path {
            Publication::Remix => 2,
            Publication::Reply | Publication::Share | Publication::Fork => 1,
            _ => 0,
        };
        assert_eq!(
            reopened.store.list_objects().unwrap().len(),
            objects_before + 1
        );
        assert_eq!(
            reopened.store.list_events().unwrap().len(),
            events_before + 1 + expected_edges
        );
        assert_eq!(
            reopened.store.list_edges().unwrap().len(),
            edges_before + expected_edges
        );
        assert_eq!(reopened.object_judgments(&id).unwrap().len(), 4);
        let edges = reopened.state.graph().outgoing(&id);
        assert_eq!(edges.len(), expected_edges);
        for edge in edges {
            assert_eq!(edge.source, object.id);
            assert!(reopened.object(&edge.target).is_some());
            edge.verify(&context.author).unwrap();
            match path {
                Publication::Reply => assert_eq!(edge.relation, babble_graph::Relation::ReplyTo),
                Publication::Share => assert_eq!(edge.relation, babble_graph::Relation::Quotes),
                Publication::Fork => {
                    assert_eq!(edge.relation, babble_graph::Relation::Forks);
                    assert_eq!(object.provenance.forked_from, Some(edge.target.clone()));
                }
                Publication::Remix => {
                    assert_eq!(edge.relation, babble_graph::Relation::Remixes);
                    assert!(object.provenance.remixed_from.contains(&edge.target));
                }
                _ => unreachable!(),
            }
        }
        let replies = reopened
            .list_replies(&RepliesListQuery {
                object_id: context.target.id.clone(),
                cursor: None,
                limit: 50,
            })
            .unwrap();
        assert_eq!(
            replies.replies.len(),
            usize::from(matches!(path, Publication::Reply))
        );
        for event in reopened.store.list_events().unwrap() {
            event
                .verify(
                    &reopened
                        .state
                        .signing_identity_at(&event.actor, event.created_at)
                        .unwrap(),
                )
                .unwrap();
        }
    }
}

#[test]
fn publication_duplicate_record_does_not_change_cache_or_events() {
    let root = TestRoot::new();
    let mut node = LocalNode::open(&root.0, FaultProvider::default()).unwrap();
    let context = setup(&mut node);
    let object = node
        .publish_object_record(&context.author.id, context.record)
        .unwrap();
    let events = node.store.list_events().unwrap();
    node.judgment_provider.recover();
    assert!(
        node.publish_object_record(&context.author.id, object)
            .is_err()
    );
    assert!(node.judgment_provider.calls.borrow().is_empty());
    assert_eq!(node.store.list_events().unwrap(), events);
}

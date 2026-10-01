use babel_graph::{EdgeOrigin, Relation};
use babel_identity::IdentityKind;
use babel_judgment_local::LocalProvider;
use babel_node::{LocalNode, QuotesListQuery, RepliesListQuery, moderation::*};
use babel_types::{IdentityId, ObjectId, Timestamp};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: IdentityId,
    reporter: IdentityId,
    reviewer: IdentityId,
    appeal_reviewer: IdentityId,
    post: ObjectId,
    replies: Vec<ObjectId>,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-moderation-projections-{}-{}-{}",
            std::process::id(),
            Timestamp::now().0.unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap()
            .id;
        let reporter = node
            .create_identity(IdentityKind::Person, "reporter")
            .unwrap()
            .id;
        let reviewer = node
            .create_identity(IdentityKind::Person, "reviewer")
            .unwrap()
            .id;
        let appeal_reviewer = node
            .create_identity(IdentityKind::Person, "appeal-reviewer")
            .unwrap()
            .id;
        node.configure_moderators(&format!("{reviewer},{appeal_reviewer}"))
            .unwrap();
        let post = node
            .publish_text(&author, "A discussion with signed replies")
            .unwrap()
            .id;
        for n in 0..4 {
            let reply = node
                .publish_text(&author, &format!("Signed discussion reply number {n}"))
                .unwrap()
                .id;
            node.publish_edge(
                &author,
                reply,
                post.clone(),
                Relation::ReplyTo,
                EdgeOrigin::HumanAssertion,
            )
            .unwrap();
        }
        let replies = node
            .list_replies(&RepliesListQuery {
                object_id: post.clone(),
                cursor: None,
                limit: 50,
            })
            .unwrap()
            .replies
            .into_iter()
            .map(|r| r.object.id)
            .collect();
        Self {
            root,
            node,
            author,
            reporter,
            reviewer,
            appeal_reviewer,
            post,
            replies,
        }
    }

    fn restrict(&mut self, object: &ObjectId, key: &str) -> ModerationCase {
        let report = self
            .node
            .moderation_report(
                &self.reporter,
                ReportRequest {
                    object_id: object.clone(),
                    reason: ModerationReason::Harassment,
                    details: "A report about this particular signed reply.".into(),
                    idempotency_key: format!("report-{key}"),
                },
            )
            .unwrap();
        self.node
            .moderation_decide(
                &self.reviewer,
                report.id,
                decision(ModerationOutcome::Restrict, 1, key),
            )
            .unwrap()
    }

    fn reverse(&mut self, case: &ModerationCase) {
        self.node
            .moderation_appeal(
                &self.author,
                case.id.clone(),
                AppealRequest {
                    details: "Please reconsider the evidence and context for this reply.".into(),
                    expected_revision: 2,
                    idempotency_key: format!("appeal-{}", case.id),
                },
            )
            .unwrap();
        self.node
            .moderation_decide(
                &self.appeal_reviewer,
                case.id.clone(),
                decision(
                    ModerationOutcome::NoAction,
                    3,
                    &format!("reverse-{}", case.id),
                ),
            )
            .unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn decision(outcome: ModerationOutcome, expected_revision: u64, key: &str) -> DecisionRequest {
    DecisionRequest {
        outcome,
        reason: ModerationReason::Harassment,
        explanation: "An independent reviewer examined the reported content and context.".into(),
        policy_version: POLICY.into(),
        source_signals: vec![],
        expected_revision,
        idempotency_key: key.into(),
    }
}

#[test]
fn restricted_replies_are_filtered_before_page_limits_and_restore_after_appeal() {
    let mut f = Fixture::new();
    let signed = f.node.object(&f.replies[0]).unwrap().clone();
    let events = f.node.store().list_events().unwrap();
    let query = RepliesListQuery {
        object_id: f.post.clone(),
        cursor: None,
        limit: 1,
    };
    let old_bookmark = f.node.list_replies(&query).unwrap().next_cursor;
    let first = f.restrict(&f.replies[0].clone(), "first");
    f.restrict(&f.replies[2].clone(), "third");
    let page = f.node.list_replies(&query).unwrap();
    assert_eq!(page.replies.len(), 1);
    assert_eq!(page.replies[0].object.id, f.replies[1]);
    let last = f
        .node
        .list_replies(&RepliesListQuery {
            cursor: page.next_cursor,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(last.replies[0].object.id, f.replies[3]);
    assert!(last.next_cursor.is_none());
    let resumed = f
        .node
        .list_replies(&RepliesListQuery {
            cursor: old_bookmark,
            ..query.clone()
        })
        .unwrap();
    assert_eq!(
        resumed.replies[0].object.id, f.replies[1],
        "a bookmark into signed history remains valid when its reply is restricted"
    );
    assert_eq!(f.node.object(&signed.id).unwrap(), &signed);
    assert_eq!(f.node.store().list_events().unwrap(), events);
    f.reverse(&first);
    assert_eq!(
        f.node.list_replies(&query).unwrap().replies[0].object.id,
        f.replies[0]
    );
    let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    let result = reopened
        .list_replies(&RepliesListQuery { limit: 50, ..query })
        .unwrap();
    assert_eq!(
        result
            .replies
            .iter()
            .map(|r| r.object.id.clone())
            .collect::<Vec<_>>(),
        vec![
            f.replies[0].clone(),
            f.replies[1].clone(),
            f.replies[3].clone()
        ]
    );
}

#[test]
fn quoted_previews_preserve_edges_and_pagination_without_redistributing_restricted_content() {
    let mut f = Fixture::new();
    for target in f.replies.iter().take(2) {
        f.node
            .publish_edge(
                &f.author,
                f.post.clone(),
                target.clone(),
                Relation::Quotes,
                EdgeOrigin::HumanAssertion,
            )
            .unwrap();
    }
    let query = QuotesListQuery {
        object_id: f.post.clone(),
        cursor: None,
        limit: 1,
    };
    let original = f.node.list_quotes(&query).unwrap();
    let target = original.quotes[0].edge.target.clone();
    let case = f.restrict(&target, "quoted");
    let hidden = f.node.list_quotes(&query).unwrap();
    assert_eq!(hidden.quotes[0].edge, original.quotes[0].edge);
    assert!(hidden.quotes[0].object.is_none());
    assert_eq!(hidden.next_cursor, original.next_cursor);
    let next = f
        .node
        .list_quotes(&QuotesListQuery {
            cursor: hidden.next_cursor,
            ..query.clone()
        })
        .unwrap();
    assert!(next.quotes[0].object.is_some());
    assert!(next.next_cursor.is_none());
    assert_eq!(f.node.object(&target), original.quotes[0].object.as_ref());
    f.reverse(&case);
    assert_eq!(f.node.list_quotes(&query).unwrap(), original);
}

#[test]
fn entirely_restricted_threads_are_empty_and_storage_failure_never_serves_unfiltered_previews() {
    let mut f = Fixture::new();
    for (n, target) in f.replies.clone().iter().enumerate() {
        f.restrict(target, &format!("reply-{n}"));
    }
    let query = RepliesListQuery {
        object_id: f.post.clone(),
        cursor: None,
        limit: 1,
    };
    let page = f.node.list_replies(&query).unwrap();
    assert!(page.replies.is_empty());
    assert!(page.next_cursor.is_none());
    let path = f.root.join("private_moderation/moderation.sqlite3");
    let held = path.with_extension("held");
    std::fs::rename(&path, &held).unwrap();
    assert!(f.node.list_replies(&query).is_err());
    assert!(
        f.node
            .list_quotes(&QuotesListQuery {
                object_id: f.post.clone(),
                cursor: None,
                limit: 1
            })
            .is_err()
    );
    std::fs::rename(held, path).unwrap();
    assert!(f.node.list_replies(&query).unwrap().replies.is_empty());
}

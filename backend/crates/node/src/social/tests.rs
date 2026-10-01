use super::*;
use babel_authoring::ObjectDraft;
use babel_graph::Relation;
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_types::Hash;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: Identity,
    target: Object,
    controller: Object,
}

#[derive(Debug)]
struct SocialTestPublication {
    object: Object,
    edge: Edge,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-social-media-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "social author")
            .unwrap();
        let target = node.publish_text(&author.id, "parent").unwrap();
        let capabilities: Vec<babel_object::CapabilityRequest> = ["reply", "share"].into_iter().map(|action| {
            serde_json::from_value(json!({"id":format!("babel.social.{action}"),"version":1,"scope":{"object_id":target.id}})).unwrap()
        }).collect();
        let mut draft = ObjectDraft::text("controller").unwrap();
        for cap in &capabilities {
            draft = draft.with_capability(cap.clone()).unwrap();
        }
        let controller = node.publish_draft(&author.id, draft).unwrap();
        Self {
            root,
            node,
            author,
            target,
            controller,
        }
    }

    fn media(&self) -> SocialMediaAttachment {
        SocialMediaAttachment {
            title: " Attached media ".into(),
            resources: vec![
                self.node
                    .put_media_blob("image/png", b"image bytes")
                    .unwrap(),
            ],
        }
    }

    fn publish(
        &mut self,
        action: &str,
        text: &str,
        media: Option<&SocialMediaAttachment>,
    ) -> Result<SocialTestPublication> {
        crate::invocations::tests::invoke(&mut self.node, &self.author.id, &self.controller,
            &self.target.id, action, Some(text), media)
            .map(|result| SocialTestPublication { object: result.object.unwrap(), edge: result.edge })
    }

    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.node.store().list_objects().unwrap().len(),
            self.node.store().list_edges().unwrap().len(),
            self.node.store().list_events().unwrap().len(),
            self.node.store().list_judgments().unwrap().len(),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn social_media_reply_and_share_publish_signed_objects_and_edges_with_optional_caption() {
    let mut f = Fixture::new();
    let mut media = f.media();
    media
        .resources
        .push(f.node.put_media_blob("audio/wav", b"audio bytes").unwrap());
    for (action, caption, relation) in [
        ("reply", "  hello  ", Relation::ReplyTo),
        ("share", "  ", Relation::Quotes),
    ] {
        let before = f.counts();
        let result = f.publish(action, caption, Some(&media)).unwrap();
        assert_eq!(result.object.kind.as_str(), "babel.media");
        assert_eq!(result.object.payload["title"], "Attached media");
        assert_eq!(
            result.object.payload["description"],
            if caption.trim().is_empty() {
                Value::Null
            } else {
                json!(caption.trim())
            }
        );
        assert_eq!(result.object.payload["resources"], json!(media.resources));
        assert_eq!(
            result.object.resources,
            media
                .resources
                .iter()
                .map(MediaBlob::resource)
                .collect::<Vec<_>>()
        );
        assert!(result.object.capabilities.is_empty());
        assert!(result.object.surfaces.is_empty());
        result.object.verify(&f.author).unwrap();
        result.edge.verify(&f.author).unwrap();
        assert_eq!(result.edge.source, result.object.id);
        assert_eq!(result.edge.target, f.target.id);
        assert_eq!(result.edge.relation, relation);
        assert_eq!(f.counts().0, before.0 + 1);
        assert_eq!(f.counts().1, before.1 + 1);
        assert_eq!(f.counts().2, before.2 + 2);
    }
    let replies = f
        .node
        .list_replies(&crate::RepliesListQuery {
            object_id: f.target.id.clone(),
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(replies.replies.len(), 1);
    assert_eq!(replies.replies[0].object.kind.as_str(), "babel.media");
    let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert_eq!(
        reopened
            .list_replies(&crate::RepliesListQuery {
                object_id: f.target.id.clone(),
                cursor: None,
                limit: 50
            })
            .unwrap(),
        replies
    );
    for action in ["reply", "share"] {
        assert!(f.publish(action, " \n ", None).is_err());
        assert_eq!(
            f.publish(action, " text ", None).unwrap().object.payload["text"],
            "text"
        );
    }
}

#[test]
fn social_media_invalid_resources_never_write_publication_or_receipt() {
    let mut f = Fixture::new();
    let valid = f.media();
    let mut invalid = Vec::new();
    let mut media = valid.clone();
    media.title = " ".into();
    invalid.push(media);
    let mut media = valid.clone();
    media.resources.clear();
    invalid.push(media);
    let mut media = valid.clone();
    media.resources.push(media.resources[0].clone());
    invalid.push(media);
    for field in [
        "uri",
        "mime",
        "size-small",
        "size-large",
        "zero",
        "oversize",
        "missing",
        "hash",
    ] {
        let mut media = valid.clone();
        let blob = &mut media.resources[0];
        match field {
            "uri" => blob.uri = "https://example.test/image.png".into(),
            "mime" => blob.media_type = "Image/PNG".into(),
            "size-small" => blob.size_bytes -= 1,
            "size-large" => blob.size_bytes += 1,
            "zero" => blob.size_bytes = 0,
            "oversize" => blob.size_bytes = MAX_ATTACHMENT_BLOB_BYTES as u64 + 1,
            "missing" => *blob = MediaBlob::from_bytes("image/png", b"absent").unwrap(),
            "hash" => blob.integrity = Hash::new_unchecked("bad"),
            _ => unreachable!(),
        }
        invalid.push(media);
    }
    // A valid first resource must not publish if a later resource is missing.
    let mut media = valid.clone();
    media
        .resources
        .push(MediaBlob::from_bytes("audio/wav", b"absent audio").unwrap());
    invalid.push(media);
    let before = f.counts();
    for action in ["reply", "share"] {
        for media in &invalid {
            let result = f.publish(action, "caption", Some(media));
            assert!(result.is_err(), "accepted {media:?}");
            assert_eq!(f.counts(), before);
            assert_eq!(
                fs::read_dir(f.root.join("publication_receipts"))
                    .unwrap()
                    .count(),
                0
            );
        }
    }
    let path = f
        .root
        .join("blobs")
        .join(valid.resources[0].integrity.as_str());
    assert!(path.is_file(), "{}", path.display());
    fs::write(&path, b"wrong bytes").unwrap();
    assert!(
        f.publish("reply", "", Some(&valid))
            .unwrap_err()
            .to_string()
            .contains("integrity")
    );
    assert_eq!(f.counts(), before);
}

#[test]
fn social_media_reference_quota_counts_metadata_but_not_uploaded_blob_again() {
    let mut f = Fixture::new();
    let media = SocialMediaAttachment {
        title: "Video".into(),
        resources: vec![
            f.node
                .put_media_blob("video/mp4", &vec![42; MAX_ATTACHMENT_BLOB_BYTES])
                .unwrap(),
        ],
    };
    f.publish("share", "caption", Some(&media)).unwrap();
    let records = f.node.store().list_invocations().unwrap();
    assert_eq!(records.len(), 1);
    assert!(records[0].consumed_at().is_some());
    assert!(serde_json::to_vec(&records[0].intent().payload).unwrap().len() < 16 * 1024);
    let mut excessive = media.clone();
    excessive.title = "x".repeat(16 * 1024);
    let before = f.counts();
    assert!(
        f.publish("share", "", Some(&excessive))
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    assert_eq!(f.counts(), before);
}

#[test]
fn social_media_preserves_manifest_scope_source_and_target_checks() {
    let mut f = Fixture::new();
    let media = f.media();
    let before = f.counts();
    let missing = ObjectId::from_hash(&Hash::from_bytes(b"missing"));
    for (source, target) in [(f.controller.clone(), missing),
        (f.controller.clone(), f.controller.id.clone()), (f.target.clone(), f.target.id.clone())] {
        assert!(crate::invocations::tests::invoke(&mut f.node, &f.author.id, &source, &target,
            "reply", Some(""), Some(&media)).is_err());
    }
    assert_eq!(f.counts(), before);
}

use crate::LocalNode;
use babel_capabilities::CapabilityReceipt;
use babel_graph::Edge;
use babel_judgment::JudgmentProvider;
use babel_media::MediaBlob;
use babel_object::Object;
use babel_types::{IdentityId, ObjectId, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub(crate) const MAX_ATTACHMENT_BLOB_BYTES: usize = 8 * 1024 * 1024;

#[cfg(test)]
mod tests;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocialMediaAttachment {
    pub title: String,
    pub resources: Vec<MediaBlob>,
}

/// Legacy return type retained for callers migrating to SocialInvocationResult.
#[derive(Clone, Debug, PartialEq)]
pub struct SocialTextPublication {
    pub object: Object,
    pub edge: Edge,
    pub receipt: CapabilityReceipt,
}

// These compatibility entry points deliberately cannot publish. Reusable grant
// bindings omit the actor/login/document and exact one-use approved payload.
impl<P: JudgmentProvider> LocalNode<P> {
    pub fn social_follow(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _grant_ids: &[String],
    ) -> Result<(Edge, CapabilityReceipt)> {
        self.check_ready()?;
        Err(invocation_required())
    }

    pub fn social_unfollow(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _grant_ids: &[String],
    ) -> Result<(Edge, CapabilityReceipt)> {
        self.check_ready()?;
        Err(invocation_required())
    }

    pub fn social_reply(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _text: &str,
        _grant_ids: &[String],
    ) -> Result<SocialTextPublication> {
        self.check_ready()?;
        Err(invocation_required())
    }

    pub fn social_reply_with_media(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _text: &str,
        _media: Option<&SocialMediaAttachment>,
        _grant_ids: &[String],
    ) -> Result<SocialTextPublication> {
        self.check_ready()?;
        Err(invocation_required())
    }

    pub fn social_share(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _text: &str,
        _grant_ids: &[String],
    ) -> Result<SocialTextPublication> {
        self.check_ready()?;
        Err(invocation_required())
    }

    pub fn social_share_with_media(
        &mut self,
        _author_id: &IdentityId,
        _source_object_id: &ObjectId,
        _target_object_id: &ObjectId,
        _text: &str,
        _media: Option<&SocialMediaAttachment>,
        _grant_ids: &[String],
    ) -> Result<SocialTextPublication> {
        self.check_ready()?;
        Err(invocation_required())
    }
}

fn invocation_required() -> babel_types::Error {
    babel_types::Error::Conflict(
        "social effects require prepare/approve/execute with one-use invocation authority".into(),
    )
}

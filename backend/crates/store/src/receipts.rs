use super::*;

/// Private host retry metadata, committed in the same journal as its publication.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationRequest {
    pub id: Hash,
    pub fingerprint: Hash,
    pub author: IdentityId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationOutcome {
    pub object: Option<ObjectId>,
    pub edges: Vec<EdgeId>,
    pub event: EventId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PublicationReceipt {
    pub request: PublicationRequest,
    pub outcome: PublicationOutcome,
}

impl PublicationRequest {
    pub fn validate(&self) -> Result<()> {
        self.id.validate()?;
        self.fingerprint.validate()?;
        self.author.validate()
    }
}

impl PublicationReceipt {
    pub fn validate(&self) -> Result<()> {
        self.request.validate()?;
        self.outcome.event.validate()?;
        if let Some(object) = &self.outcome.object {
            object.validate()?;
        } else if self.outcome.edges.len() > 1 {
            return Err(CoreError::Conflict(
                "receipt without object requires one edge or a consent event".into(),
            ));
        }
        if self.outcome.edges.len() > 125 {
            return Err(CoreError::Conflict(
                "publication receipt edge limit exceeded".into(),
            ));
        }
        let mut unique = std::collections::BTreeSet::new();
        for edge in &self.outcome.edges {
            edge.validate()?;
            if !unique.insert(edge) {
                return Err(CoreError::Conflict("duplicate receipt edge".into()));
            }
        }
        Ok(())
    }

    pub(crate) fn is_event_only(&self) -> bool {
        self.outcome.object.is_none() && self.outcome.edges.is_empty()
    }
}

// Decode the persisted consent envelope without pulling the capability broker
// into storage. Authorization and grant projection remain node responsibilities.
pub(crate) fn validate_consent_event(event: &Event) -> Result<()> {
    use babble_state::{EventKind, EventTarget};
    use babble_types::CapabilityGrantId;

    #[derive(Deserialize)]
    struct Grant {
        id: CapabilityGrantId,
        object_id: ObjectId,
        capability: String,
        version: u32,
        scope: Value,
        #[serde(rename = "decision")]
        _decision: Decision,
        #[serde(rename = "quota")]
        _quota: Quota,
        #[serde(rename = "created_at")]
        _created_at: Timestamp,
        #[serde(rename = "expires_at")]
        _expires_at: Option<Timestamp>,
        revoked_at: Option<Timestamp>,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum Decision {
        Approved,
        Denied,
    }
    #[derive(Deserialize)]
    struct Quota {
        #[serde(rename = "calls_per_minute")]
        _calls_per_minute: u32,
        #[serde(rename = "bytes_per_minute")]
        _bytes_per_minute: u64,
        #[serde(rename = "persistent_bytes")]
        _persistent_bytes: u64,
        #[serde(rename = "realtime_connections")]
        _realtime_connections: u32,
        #[serde(rename = "max_call_ms")]
        _max_call_ms: u64,
        #[serde(rename = "background_allowed")]
        _background_allowed: bool,
    }
    #[derive(Deserialize)]
    struct Granted {
        grant: Grant,
    }
    #[derive(Deserialize)]
    struct Revoked {
        grant_id: CapabilityGrantId,
    }
    let EventTarget::Object(target) = &event.target else {
        return Err(CoreError::Conflict(
            "consent event requires object target".into(),
        ));
    };
    target.validate()?;
    if event.signature.is_none() {
        return Err(CoreError::UnsignedEvent);
    }
    let decode =
        |error: serde_json::Error| CoreError::Canonical(format!("consent event payload: {error}"));
    match event.kind {
        EventKind::CapabilityGranted => {
            let Granted { grant } =
                serde_json::from_value(event.payload.clone()).map_err(decode)?;
            grant.id.validate()?;
            grant.object_id.validate()?;
            if &grant.object_id != target || grant.revoked_at.is_some() {
                return Err(CoreError::Conflict(
                    "consent grant target or revocation mismatch".into(),
                ));
            }
            babble_object::validate_capability_request(&babble_object::CapabilityRequest {
                id: grant.capability,
                version: grant.version,
                scope: grant.scope,
            })?;
        }
        EventKind::CapabilityRevoked => {
            let Revoked { grant_id } =
                serde_json::from_value(event.payload.clone()).map_err(decode)?;
            grant_id.validate()?;
        }
        _ => {
            return Err(CoreError::Conflict(
                "event-only receipt requires consent event".into(),
            ));
        }
    }
    Ok(())
}

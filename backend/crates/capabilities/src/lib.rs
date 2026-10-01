use babble_object::{CapabilityRequest, Object, validate_capability_request};
use babble_types::{Canonical, CapabilityGrantId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub mod invocation;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct CapabilityId(String);

impl CapabilityId {
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        validate_capability_id(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    ImplicitSafe,
    AskOnce,
    AskEachTime,
    DeniedByDefault,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityQuota {
    pub calls_per_minute: u32,
    pub bytes_per_minute: u64,
    pub persistent_bytes: u64,
    pub realtime_connections: u32,
    pub max_call_ms: u64,
    pub background_allowed: bool,
}

impl CapabilityQuota {
    pub fn restrictive() -> Self {
        Self {
            calls_per_minute: 0,
            bytes_per_minute: 0,
            persistent_bytes: 0,
            realtime_connections: 0,
            max_call_ms: 1000,
            background_allowed: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityDefinition {
    pub id: CapabilityId,
    pub version: u32,
    pub request_schema: Value,
    pub response_schema: Value,
    pub permission: PermissionMode,
    pub quota: CapabilityQuota,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GrantDecision {
    Approved,
    Denied,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityGrant {
    pub id: CapabilityGrantId,
    pub object_id: ObjectId,
    pub capability: CapabilityId,
    pub version: u32,
    pub scope: Value,
    pub decision: GrantDecision,
    pub quota: CapabilityQuota,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub revoked_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct CapabilityGrantCommitment {
    pub object_id: ObjectId,
    pub capability: CapabilityId,
    pub version: u32,
    pub scope: Value,
    pub decision: GrantDecision,
    pub quota: CapabilityQuota,
    pub created_at: Timestamp,
    pub expires_at: Option<Timestamp>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityManifest {
    pub object_id: ObjectId,
    pub requests: Vec<CapabilityRequest>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityDecisionStatus {
    Granted,
    RequiresUser,
    Denied,
    Unavailable,
    VersionUnsupported,
    Revoked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityDecision {
    pub request: CapabilityRequest,
    pub status: CapabilityDecisionStatus,
    pub reason: String,
    pub definition: Option<CapabilityDefinition>,
    pub grant: Option<CapabilityGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityCall {
    pub object_id: ObjectId,
    pub capability: CapabilityId,
    pub version: u32,
    pub scope: Value,
    pub requested_bytes: u64,
    pub realtime_connections: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityReceipt {
    pub grant_id: CapabilityGrantId,
    pub capability: CapabilityId,
    pub version: u32,
    pub scope: Value,
    pub quota: CapabilityQuota,
    pub remaining_calls_per_minute: u32,
    pub remaining_bytes_per_minute: u64,
    pub remaining_realtime_connections: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityUsageWindow {
    pub grant_id: CapabilityGrantId,
    pub window_started_at: Timestamp,
    pub calls: u32,
    pub bytes: u64,
    pub realtime_connections: u32,
}

#[derive(Clone, Debug, Default)]
pub struct CapabilityBroker {
    definitions: BTreeMap<(CapabilityId, u32), CapabilityDefinition>,
}

impl CapabilityBroker {
    pub fn new(definitions: Vec<CapabilityDefinition>) -> Result<Self> {
        let mut by_key = BTreeMap::new();
        for definition in definitions {
            if definition.version == 0 {
                return Err(babble_types::Error::Conflict(format!(
                    "capability version must be positive: {}",
                    definition.id.as_str()
                )));
            }
            by_key.insert((definition.id.clone(), definition.version), definition);
        }
        Ok(Self {
            definitions: by_key,
        })
    }

    pub fn babble_default() -> Self {
        Self::new(default_capabilities()).expect("default capabilities are valid")
    }

    pub fn definitions(&self) -> Vec<CapabilityDefinition> {
        self.definitions.values().cloned().collect()
    }

    pub fn manifest(&self, object: &Object) -> CapabilityManifest {
        CapabilityManifest {
            object_id: object.id.clone(),
            requests: object.capabilities.clone(),
        }
    }

    pub fn evaluate_object(
        &self,
        object: &Object,
        grants: &[CapabilityGrant],
    ) -> Vec<CapabilityDecision> {
        object
            .capabilities
            .iter()
            .cloned()
            .map(|request| self.evaluate_request(object.id.clone(), request, grants))
            .collect()
    }

    pub fn evaluate_request(
        &self,
        object_id: ObjectId,
        request: CapabilityRequest,
        grants: &[CapabilityGrant],
    ) -> CapabilityDecision {
        if validate_capability_request(&request).is_err() {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Denied,
                reason: "invalid capability request".to_string(),
                definition: None,
                grant: None,
            };
        }
        let Ok(capability_id) = CapabilityId::new(request.id.clone()) else {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Denied,
                reason: "invalid capability id".to_string(),
                definition: None,
                grant: None,
            };
        };
        let Some(definition) = self
            .definitions
            .get(&(capability_id.clone(), request.version))
            .cloned()
        else {
            let known = self.definitions.keys().any(|(id, _)| id == &capability_id);
            return CapabilityDecision {
                request,
                status: if known {
                    CapabilityDecisionStatus::VersionUnsupported
                } else {
                    CapabilityDecisionStatus::Unavailable
                },
                reason: if known {
                    "capability version is unsupported".to_string()
                } else {
                    "capability is unavailable on this host".to_string()
                },
                definition: None,
                grant: None,
            };
        };

        if definition.permission == PermissionMode::Unavailable {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Unavailable,
                reason: "capability is unavailable on this host".to_string(),
                definition: Some(definition),
                grant: None,
            };
        }
        if definition.permission == PermissionMode::DeniedByDefault {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Denied,
                reason: "capability is denied by host policy".to_string(),
                definition: Some(definition),
                grant: None,
            };
        }

        if definition.permission == PermissionMode::AskEachTime
            && invocation::is_one_use_invocation(capability_id.as_str())
        {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::RequiresUser,
                reason: "effect requires one-use invocation approval".into(),
                definition: Some(definition),
                grant: None,
            };
        }

        if let Some(grant) = active_grant_at(
            grants,
            &object_id,
            &capability_id,
            request.version,
            &request.scope,
            Timestamp::now(),
        ) {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Granted,
                reason: "active grant satisfies request".to_string(),
                definition: Some(definition),
                grant: Some(grant.clone()),
            };
        }

        if definition.permission == PermissionMode::ImplicitSafe {
            return CapabilityDecision {
                request,
                status: CapabilityDecisionStatus::Granted,
                reason: "implicit-safe capability is available without prompting".to_string(),
                definition: Some(definition),
                grant: None,
            };
        }

        CapabilityDecision {
            request,
            status: CapabilityDecisionStatus::RequiresUser,
            reason: "capability requires trusted host approval".to_string(),
            definition: Some(definition),
            grant: None,
        }
    }

    pub fn issue_grant(
        &self,
        object_id: ObjectId,
        request: CapabilityRequest,
        decision: GrantDecision,
        expires_at: Option<Timestamp>,
    ) -> Result<CapabilityGrant> {
        validate_capability_request(&request)?;
        let capability = CapabilityId::new(request.id)?;
        let definition = self
            .definitions
            .get(&(capability.clone(), request.version))
            .ok_or_else(|| {
                babble_types::Error::NotFound(format!(
                    "capability {}@{}",
                    capability.as_str(),
                    request.version
                ))
            })?;
        if definition.permission == PermissionMode::Unavailable
            || definition.permission == PermissionMode::DeniedByDefault
        {
            return Err(babble_types::Error::Conflict(format!(
                "capability cannot be granted by user decision: {}",
                capability.as_str()
            )));
        }
        if definition.permission == PermissionMode::AskEachTime
            && invocation::is_one_use_invocation(capability.as_str())
            && decision == GrantDecision::Approved
        {
            return Err(babble_types::Error::Conflict("social effects require one-use invocation approval".into()));
        }
        let commitment = CapabilityGrantCommitment {
            object_id,
            capability,
            version: request.version,
            scope: request.scope,
            decision,
            quota: definition.quota.clone(),
            created_at: Timestamp::now(),
            expires_at,
        };
        Ok(CapabilityGrant {
            id: CapabilityGrantId::from_hash(&commitment.canonical_hash()?),
            object_id: commitment.object_id,
            capability: commitment.capability,
            version: commitment.version,
            scope: commitment.scope,
            decision: commitment.decision,
            quota: commitment.quota,
            created_at: commitment.created_at,
            expires_at: commitment.expires_at,
            revoked_at: None,
        })
    }

    pub fn authorize_call(
        &self,
        call: CapabilityCall,
        grants: &[CapabilityGrant],
    ) -> Result<CapabilityReceipt> {
        self.authorize_call_with_usage(call, grants, &[], Timestamp::now())
    }

    pub fn authorize_call_with_usage(
        &self,
        call: CapabilityCall,
        grants: &[CapabilityGrant],
        usage_windows: &[CapabilityUsageWindow],
        now: Timestamp,
    ) -> Result<CapabilityReceipt> {
        let definition = self
            .definitions
            .get(&(call.capability.clone(), call.version))
            .ok_or_else(|| {
                babble_types::Error::NotFound(format!(
                    "capability {}@{}",
                    call.capability.as_str(),
                    call.version
                ))
            })?;
        if definition.permission == PermissionMode::AskEachTime
            && invocation::is_one_use_invocation(call.capability.as_str())
        {
            return Err(babble_types::Error::Conflict("social effects require one-use invocation approval".into()));
        }
        if definition.permission == PermissionMode::Unavailable
            || definition.permission == PermissionMode::DeniedByDefault
        {
            return Err(babble_types::Error::Conflict(format!(
                "capability call denied by host policy: {}",
                call.capability.as_str()
            )));
        }
        if definition.permission == PermissionMode::ImplicitSafe {
            let grant = self.issue_grant(
                call.object_id.clone(),
                CapabilityRequest {
                    id: call.capability.as_str().to_string(),
                    version: call.version,
                    scope: call.scope.clone(),
                },
                GrantDecision::Approved,
                None,
            )?;
            return metered_receipt(grant, &call, usage_windows, now);
        }
        let grant = active_grant_at(
            grants,
            &call.object_id,
            &call.capability,
            call.version,
            &call.scope,
            now,
        )
        .ok_or_else(|| {
            babble_types::Error::Conflict(format!(
                "missing active grant for {}@{}",
                call.capability.as_str(),
                call.version
            ))
        })?;
        metered_receipt(grant.clone(), &call, usage_windows, now)
    }
}

pub fn revoke_grant(mut grant: CapabilityGrant) -> CapabilityGrant {
    grant.revoked_at = Some(Timestamp::now());
    grant
}

fn active_grant_at<'a>(
    grants: &'a [CapabilityGrant],
    object_id: &ObjectId,
    capability: &CapabilityId,
    version: u32,
    scope: &Value,
    now: Timestamp,
) -> Option<&'a CapabilityGrant> {
    grants.iter().rev().find(|grant| {
        &grant.object_id == object_id
            && &grant.capability == capability
            && grant.version == version
            && grant.scope == *scope
            && grant.decision == GrantDecision::Approved
            && grant.revoked_at.is_none()
            && grant
                .expires_at
                .map(|expires_at| expires_at > now)
                .unwrap_or(true)
    })
}

fn metered_receipt(
    grant: CapabilityGrant,
    call: &CapabilityCall,
    usage_windows: &[CapabilityUsageWindow],
    now: Timestamp,
) -> Result<CapabilityReceipt> {
    if call.requested_bytes > grant.quota.bytes_per_minute {
        return Err(babble_types::Error::Conflict(format!(
            "capability {}@{} byte request exceeds per-minute quota",
            grant.capability.as_str(),
            grant.version
        )));
    }
    if call.realtime_connections > grant.quota.realtime_connections {
        return Err(babble_types::Error::Conflict(format!(
            "capability {}@{} realtime connection request exceeds quota",
            grant.capability.as_str(),
            grant.version
        )));
    }
    let current = usage_for_grant(&grant.id, usage_windows, now)?;
    let used_calls = current.calls;
    let used_bytes = current.bytes;
    let used_realtime = current.realtime_connections;
    let next_calls = used_calls.checked_add(1).ok_or_else(|| {
        babble_types::Error::Conflict("capability call count overflow".to_string())
    })?;
    let next_bytes = used_bytes
        .checked_add(call.requested_bytes)
        .ok_or_else(|| {
            babble_types::Error::Conflict("capability byte usage overflow".to_string())
        })?;
    let next_realtime = used_realtime
        .checked_add(call.realtime_connections)
        .ok_or_else(|| {
            babble_types::Error::Conflict("capability realtime usage overflow".to_string())
        })?;
    if next_calls > grant.quota.calls_per_minute {
        return Err(babble_types::Error::Conflict(format!(
            "capability {}@{} call quota exhausted",
            grant.capability.as_str(),
            grant.version
        )));
    }
    if next_bytes > grant.quota.bytes_per_minute {
        return Err(babble_types::Error::Conflict(format!(
            "capability {}@{} byte quota exhausted",
            grant.capability.as_str(),
            grant.version
        )));
    }
    if next_realtime > grant.quota.realtime_connections {
        return Err(babble_types::Error::Conflict(format!(
            "capability {}@{} realtime connection quota exhausted",
            grant.capability.as_str(),
            grant.version
        )));
    }

    Ok(CapabilityReceipt {
        grant_id: grant.id,
        capability: grant.capability,
        version: grant.version,
        scope: grant.scope,
        remaining_calls_per_minute: grant.quota.calls_per_minute - next_calls,
        remaining_bytes_per_minute: grant.quota.bytes_per_minute - next_bytes,
        remaining_realtime_connections: grant.quota.realtime_connections - next_realtime,
        quota: grant.quota,
    })
}

fn usage_for_grant(
    grant_id: &CapabilityGrantId,
    usage_windows: &[CapabilityUsageWindow],
    now: Timestamp,
) -> Result<CapabilityUsageWindow> {
    let mut aggregate = CapabilityUsageWindow {
        grant_id: grant_id.clone(),
        window_started_at: now,
        calls: 0,
        bytes: 0,
        realtime_connections: 0,
    };
    for window in usage_windows
        .iter()
        .filter(|window| &window.grant_id == grant_id)
        .filter(|window| {
            now.0 >= window.window_started_at.0
                && now.0 - window.window_started_at.0 < time::Duration::minutes(1)
        })
    {
        aggregate.calls = aggregate.calls.checked_add(window.calls).ok_or_else(|| {
            babble_types::Error::Conflict("capability call usage overflow".to_string())
        })?;
        aggregate.bytes = aggregate.bytes.checked_add(window.bytes).ok_or_else(|| {
            babble_types::Error::Conflict("capability byte usage overflow".to_string())
        })?;
        aggregate.realtime_connections = aggregate
            .realtime_connections
            .checked_add(window.realtime_connections)
            .ok_or_else(|| {
                babble_types::Error::Conflict("capability realtime usage overflow".to_string())
            })?;
    }
    Ok(aggregate)
}

fn default_capabilities() -> Vec<CapabilityDefinition> {
    vec![
        definition(
            "babble.identity.current",
            PermissionMode::AskOnce,
            20,
            4 * 1024,
            false,
        ),
        definition(
            "babble.social.follow",
            PermissionMode::AskEachTime,
            10,
            4 * 1024,
            false,
        ),
        definition(
            "babble.social.unfollow",
            PermissionMode::AskEachTime,
            10,
            4 * 1024,
            false,
        ),
        definition(
            "babble.social.share",
            PermissionMode::AskEachTime,
            20,
            16 * 1024,
            false,
        ),
        definition(
            "babble.social.reply",
            PermissionMode::AskEachTime,
            30,
            32 * 1024,
            false,
        ),
        definition(
            "babble.storage.local",
            PermissionMode::AskOnce,
            120,
            128 * 1024,
            false,
        )
        .with_persistent_bytes(10 * 1024 * 1024),
        definition(
            "babble.storage.object",
            PermissionMode::AskOnce,
            120,
            128 * 1024,
            false,
        )
        .with_persistent_bytes(10 * 1024 * 1024),
        definition(
            "babble.realtime.join",
            PermissionMode::AskOnce,
            30,
            32 * 1024,
            false,
        )
        .with_realtime_connections(2),
        definition(
            "babble.realtime.send",
            PermissionMode::AskOnce,
            120,
            256 * 1024,
            false,
        ),
        definition(
            "babble.realtime.leave",
            PermissionMode::ImplicitSafe,
            120,
            4 * 1024,
            false,
        ),
        definition(
            "babble.payments.checkout",
            PermissionMode::AskEachTime,
            4,
            16 * 1024,
            false,
        ),
        definition(
            "babble.ai.judge",
            PermissionMode::AskOnce,
            20,
            256 * 1024,
            false,
        ),
        definition(
            "babble.ai.generate",
            PermissionMode::AskEachTime,
            10,
            512 * 1024,
            false,
        ),
        definition(
            "babble.ai.embed",
            PermissionMode::AskOnce,
            60,
            512 * 1024,
            false,
        ),
        definition(
            "babble.ai.transcribe",
            PermissionMode::AskEachTime,
            6,
            16 * 1024 * 1024,
            false,
        ),
        definition(
            "babble.media.camera",
            PermissionMode::AskEachTime,
            4,
            16 * 1024 * 1024,
            false,
        ),
        definition(
            "babble.media.microphone",
            PermissionMode::AskEachTime,
            4,
            16 * 1024 * 1024,
            false,
        ),
        definition(
            "babble.graphics.webgpu",
            PermissionMode::AskOnce,
            60,
            8 * 1024 * 1024,
            false,
        ),
        definition(
            "babble.notifications.request",
            PermissionMode::AskOnce,
            4,
            4 * 1024,
            false,
        ),
        definition(
            "babble.clipboard.write",
            PermissionMode::AskEachTime,
            20,
            64 * 1024,
            false,
        ),
        definition(
            "babble.fullscreen.enter",
            PermissionMode::AskEachTime,
            10,
            4 * 1024,
            false,
        ),
        definition(
            "babble.network.fetch",
            PermissionMode::AskOnce,
            60,
            2 * 1024 * 1024,
            false,
        ),
        definition(
            "babble.location",
            PermissionMode::DeniedByDefault,
            0,
            0,
            false,
        ),
        definition("babble.files", PermissionMode::DeniedByDefault, 0, 0, false),
    ]
}

fn definition(
    id: &str,
    permission: PermissionMode,
    calls_per_minute: u32,
    bytes_per_minute: u64,
    background_allowed: bool,
) -> CapabilityDefinition {
    CapabilityDefinition {
        id: CapabilityId::new(id).expect("static capability id is valid"),
        version: 1,
        request_schema: json!({"type": "object"}),
        response_schema: json!({"type": "object"}),
        permission,
        quota: CapabilityQuota {
            calls_per_minute,
            bytes_per_minute,
            persistent_bytes: 0,
            realtime_connections: 0,
            max_call_ms: if invocation::is_one_use_invocation(id) { 60_000 } else { 5000 },
            background_allowed,
        },
    }
}

trait QuotaBuilder {
    fn with_persistent_bytes(self, value: u64) -> Self;
    fn with_realtime_connections(self, value: u32) -> Self;
}

impl QuotaBuilder for CapabilityDefinition {
    fn with_persistent_bytes(mut self, value: u64) -> Self {
        self.quota.persistent_bytes = value;
        self
    }

    fn with_realtime_connections(mut self, value: u32) -> Self {
        self.quota.realtime_connections = value;
        self
    }
}

fn validate_capability_id(value: &str) -> Result<()> {
    let parts = value.split('.').collect::<Vec<_>>();
    let valid = parts.len() >= 2
        && parts.iter().all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        });
    if valid {
        Ok(())
    } else {
        Err(babble_types::Error::Conflict(format!(
            "invalid capability id: {value}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_crypto::Keypair;
    use babble_identity::{Identity, IdentityKind};
    use babble_object::Object;

    #[test]
    fn imported_social_and_browser_grants_never_satisfy_one_use_consent() {
        let broker = CapabilityBroker::babble_default();
        let object_id = ObjectId::from_hash(&babble_types::Hash::from_bytes(b"controller"));
        for definition in broker.definitions().into_iter().filter(|d| invocation::is_one_use_invocation(d.id.as_str())) {
            let request = CapabilityRequest { id: definition.id.as_str().into(), version: 1, scope: json!({}) };
            let grant = CapabilityGrant {
                id: CapabilityGrantId::from_hash(&babble_types::Hash::from_bytes(definition.id.as_str().as_bytes())),
                object_id: object_id.clone(), capability: definition.id.clone(), version: 1,
                scope: json!({}), decision: GrantDecision::Approved, quota: definition.quota,
                created_at: Timestamp::now(), expires_at: None, revoked_at: None,
            };
            assert_eq!(broker.evaluate_request(object_id.clone(), request.clone(), std::slice::from_ref(&grant)).status,
                CapabilityDecisionStatus::RequiresUser);
            assert!(broker.authorize_call(CapabilityCall { object_id: object_id.clone(), capability: definition.id,
                version: 1, scope: json!({}), requested_bytes: 0, realtime_connections: 0 }, &[grant]).is_err());
            assert!(broker.issue_grant(object_id.clone(), request, GrantDecision::Approved, None).is_err());
        }
    }

    #[test]
    fn broker_requires_grants_for_sensitive_capabilities_and_authorizes_active_scope() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let object = Object::text(&identity, "hello")
            .unwrap()
            .with_capabilities(vec![CapabilityRequest {
                id: "babble.network.fetch".to_string(),
                version: 1,
                scope: json!({"origins": ["https://example.com"]}),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let broker = CapabilityBroker::babble_default();
        let request = object.capabilities[0].clone();

        let before = broker.evaluate_object(&object, &[]);
        assert_eq!(before[0].status, CapabilityDecisionStatus::RequiresUser);

        let grant = broker
            .issue_grant(
                object.id.clone(),
                request.clone(),
                GrantDecision::Approved,
                None,
            )
            .unwrap();
        let after = broker.evaluate_object(&object, std::slice::from_ref(&grant));
        assert_eq!(after[0].status, CapabilityDecisionStatus::Granted);
        let receipt = broker
            .authorize_call(
                CapabilityCall {
                    object_id: object.id.clone(),
                    capability: CapabilityId::new("babble.network.fetch").unwrap(),
                    version: 1,
                    scope: request.scope,
                    requested_bytes: 1024,
                    realtime_connections: 0,
                },
                std::slice::from_ref(&grant),
            )
            .unwrap();
        assert_eq!(receipt.capability.as_str(), "babble.network.fetch");
        assert_eq!(receipt.remaining_calls_per_minute, 59);
        assert_eq!(receipt.remaining_bytes_per_minute, (2 * 1024 * 1024) - 1024);

        let exhausted = broker.authorize_call_with_usage(
            CapabilityCall {
                object_id: object.id.clone(),
                capability: CapabilityId::new("babble.network.fetch").unwrap(),
                version: 1,
                scope: json!({"origins": ["https://example.com"]}),
                requested_bytes: 1,
                realtime_connections: 0,
            },
            std::slice::from_ref(&grant),
            &[CapabilityUsageWindow {
                grant_id: grant.id.clone(),
                window_started_at: Timestamp::now(),
                calls: 60,
                bytes: 0,
                realtime_connections: 0,
            }],
            Timestamp::now(),
        );
        assert!(exhausted.is_err());
    }

    #[test]
    fn broker_rejects_unavailable_or_denied_capability_grants() {
        let broker = CapabilityBroker::babble_default();
        let object_id = ObjectId::new_unchecked(format!("obj_{}", "0".repeat(64)));
        let denied = broker.issue_grant(
            object_id,
            CapabilityRequest {
                id: "babble.location".to_string(),
                version: 1,
                scope: json!({}),
            },
            GrantDecision::Approved,
            None,
        );

        assert!(denied.is_err());
    }

    #[test]
    fn broker_ignores_expired_grants() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let object = Object::text(&identity, "hello")
            .unwrap()
            .with_capabilities(vec![CapabilityRequest {
                id: "babble.ai.judge".to_string(),
                version: 1,
                scope: json!({"definition": "babble.judgment.relevance.v1"}),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let broker = CapabilityBroker::babble_default();
        let request = object.capabilities[0].clone();
        let expired_at = Timestamp(Timestamp::now().0 - time::Duration::seconds(1));
        let grant = broker
            .issue_grant(
                object.id.clone(),
                request.clone(),
                GrantDecision::Approved,
                Some(expired_at),
            )
            .unwrap();

        let decisions = broker.evaluate_object(&object, std::slice::from_ref(&grant));
        assert_eq!(decisions[0].status, CapabilityDecisionStatus::RequiresUser);
        let authorized = broker.authorize_call(
            CapabilityCall {
                object_id: object.id,
                capability: CapabilityId::new("babble.ai.judge").unwrap(),
                version: 1,
                scope: request.scope,
                requested_bytes: 128,
                realtime_connections: 0,
            },
            &[grant],
        );
        assert!(authorized.is_err());
    }
}

use babel_capabilities::{
    CapabilityBroker, CapabilityDecision, CapabilityDecisionStatus, CapabilityGrant,
};
use babel_object::{Object, Surface, SurfaceRole, SurfaceTarget};
use babel_types::{Hash, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

mod bundles;
pub use bundles::{BUNDLE_POLICY_VERSION, BundleVerification, VerifiedSurfaceMount};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceLifecycle {
    Cold,
    Prefetched,
    Warm,
    Active,
    Suspended,
    Evicted,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ResourceBudget {
    pub memory_bytes: u64,
    pub cpu_ms_per_minute: u64,
    pub gpu_expected: bool,
    pub network_bytes_per_minute: u64,
    pub persistent_storage_bytes: u64,
    pub realtime_connections: u32,
    pub background_eligible: bool,
}

impl ResourceBudget {
    pub fn for_surface(role: &SurfaceRole, target: &SurfaceTarget) -> Self {
        let mut budget = match role {
            SurfaceRole::Preview => Self {
                memory_bytes: 8 * 1024 * 1024,
                cpu_ms_per_minute: 250,
                gpu_expected: false,
                network_bytes_per_minute: 0,
                persistent_storage_bytes: 0,
                realtime_connections: 0,
                background_eligible: false,
            },
            SurfaceRole::Feed => Self {
                memory_bytes: 32 * 1024 * 1024,
                cpu_ms_per_minute: 1500,
                gpu_expected: false,
                network_bytes_per_minute: 512 * 1024,
                persistent_storage_bytes: 1024 * 1024,
                realtime_connections: 1,
                background_eligible: false,
            },
            SurfaceRole::Expanded => Self {
                memory_bytes: 96 * 1024 * 1024,
                cpu_ms_per_minute: 6000,
                gpu_expected: false,
                network_bytes_per_minute: 4 * 1024 * 1024,
                persistent_storage_bytes: 10 * 1024 * 1024,
                realtime_connections: 2,
                background_eligible: false,
            },
            SurfaceRole::Fullscreen => Self {
                memory_bytes: 256 * 1024 * 1024,
                cpu_ms_per_minute: 30000,
                gpu_expected: false,
                network_bytes_per_minute: 32 * 1024 * 1024,
                persistent_storage_bytes: 64 * 1024 * 1024,
                realtime_connections: 4,
                background_eligible: false,
            },
            SurfaceRole::Background => Self {
                memory_bytes: 32 * 1024 * 1024,
                cpu_ms_per_minute: 1000,
                gpu_expected: false,
                network_bytes_per_minute: 1024 * 1024,
                persistent_storage_bytes: 10 * 1024 * 1024,
                realtime_connections: 1,
                background_eligible: true,
            },
        };
        if matches!(target, SurfaceTarget::WebGpu) {
            budget.gpu_expected = true;
            budget.memory_bytes = budget.memory_bytes.max(128 * 1024 * 1024);
        }
        budget
    }

    pub fn validate_reduction_from(&self, current: &Self) -> Result<()> {
        let raised = self.memory_bytes > current.memory_bytes
            || self.cpu_ms_per_minute > current.cpu_ms_per_minute
            || self.network_bytes_per_minute > current.network_bytes_per_minute
            || self.persistent_storage_bytes > current.persistent_storage_bytes
            || self.realtime_connections > current.realtime_connections
            || (self.gpu_expected && !current.gpu_expected)
            || (self.background_eligible && !current.background_eligible);
        if raised {
            return Err(babel_types::Error::Conflict(
                "Surface resource budget updates may only preserve or lower host budgets"
                    .to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceLifecycleCounts {
    pub cold: u32,
    pub prefetched: u32,
    pub warm: u32,
    pub active: u32,
    pub suspended: u32,
    pub evicted: u32,
}

impl SurfaceLifecycleCounts {
    fn add(&mut self, lifecycle: &SurfaceLifecycle) {
        match lifecycle {
            SurfaceLifecycle::Cold => self.cold += 1,
            SurfaceLifecycle::Prefetched => self.prefetched += 1,
            SurfaceLifecycle::Warm => self.warm += 1,
            SurfaceLifecycle::Active => self.active += 1,
            SurfaceLifecycle::Suspended => self.suspended += 1,
            SurfaceLifecycle::Evicted => self.evicted += 1,
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceResourceTotals {
    pub memory_bytes: u64,
    pub cpu_ms_per_minute: u64,
    pub gpu_expected_sessions: u32,
    pub network_bytes_per_minute: u64,
    pub persistent_storage_bytes: u64,
    pub realtime_connections: u32,
    pub background_eligible_sessions: u32,
    pub zero_cpu_sessions: u32,
}

impl SurfaceResourceTotals {
    fn add(&mut self, budget: &ResourceBudget) {
        self.memory_bytes = self.memory_bytes.saturating_add(budget.memory_bytes);
        self.cpu_ms_per_minute = self
            .cpu_ms_per_minute
            .saturating_add(budget.cpu_ms_per_minute);
        self.network_bytes_per_minute = self
            .network_bytes_per_minute
            .saturating_add(budget.network_bytes_per_minute);
        self.persistent_storage_bytes = self
            .persistent_storage_bytes
            .saturating_add(budget.persistent_storage_bytes);
        self.realtime_connections = self
            .realtime_connections
            .saturating_add(budget.realtime_connections);
        if budget.gpu_expected {
            self.gpu_expected_sessions += 1;
        }
        if budget.background_eligible {
            self.background_eligible_sessions += 1;
        }
        if budget.cpu_ms_per_minute == 0 {
            self.zero_cpu_sessions += 1;
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSessionHealth {
    pub session_id: SurfaceSessionId,
    pub object_id: ObjectId,
    pub role: SurfaceRole,
    pub target: SurfaceTarget,
    pub lifecycle: SurfaceLifecycle,
    pub admission: RuntimeAdmissionStatus,
    pub budget: ResourceBudget,
    pub event_count: u64,
    pub last_event_reason: Option<String>,
    pub granted_capability_count: u32,
    pub blocked_reason_count: u32,
    pub zero_cpu_required: bool,
    pub updated_at: Timestamp,
}

impl SurfaceSessionHealth {
    pub fn from_session(session: &SurfaceSession) -> Self {
        let granted_capability_count = session
            .plan
            .capability_decisions
            .iter()
            .filter(|decision| decision.status == CapabilityDecisionStatus::Granted)
            .count() as u32;
        Self {
            session_id: session.id.clone(),
            object_id: session.plan.object_id.clone(),
            role: session.plan.surface.role.clone(),
            target: session.plan.surface.target.clone(),
            lifecycle: session.lifecycle.clone(),
            admission: session.plan.admission.clone(),
            budget: session.budget.clone(),
            event_count: session.events.len() as u64,
            last_event_reason: session.events.last().map(|event| event.reason.clone()),
            granted_capability_count,
            blocked_reason_count: session.plan.blocked_reasons.len() as u32,
            zero_cpu_required: matches!(
                session.lifecycle,
                SurfaceLifecycle::Suspended | SurfaceLifecycle::Evicted
            ) || session.budget.cpu_ms_per_minute == 0,
            updated_at: session.updated_at,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceRuntimeHealthSnapshot {
    pub at: Timestamp,
    pub session_count: u32,
    pub lifecycle_counts: SurfaceLifecycleCounts,
    pub totals: SurfaceResourceTotals,
    pub sessions: Vec<SurfaceSessionHealth>,
}

impl SurfaceRuntimeHealthSnapshot {
    pub fn from_sessions<'a>(sessions: impl IntoIterator<Item = &'a SurfaceSession>) -> Self {
        let mut lifecycle_counts = SurfaceLifecycleCounts::default();
        let mut totals = SurfaceResourceTotals::default();
        let mut sessions = sessions
            .into_iter()
            .map(|session| {
                lifecycle_counts.add(&session.lifecycle);
                if session.lifecycle != SurfaceLifecycle::Evicted {
                    totals.add(&session.budget);
                }
                SurfaceSessionHealth::from_session(session)
            })
            .collect::<Vec<_>>();
        sessions.sort_by(|left, right| left.session_id.as_str().cmp(right.session_id.as_str()));
        Self {
            at: Timestamp::now(),
            session_count: sessions.len() as u32,
            lifecycle_counts,
            totals,
            sessions,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeDeviceClass {
    LowPowerMobile,
    Mobile,
    Tablet,
    Desktop,
    Workstation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePressureLevel {
    Normal,
    Elevated,
    Critical,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSchedulingInput {
    pub viewport_distance_px: u32,
    pub approaching_viewport: bool,
    pub interaction_score: u16,
    pub memory_pressure: RuntimePressureLevel,
    pub gpu_pressure: RuntimePressureLevel,
    pub battery_saver: bool,
    pub metered_network: bool,
    pub device_class: RuntimeDeviceClass,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceScheduleDecision {
    pub lifecycle: SurfaceLifecycle,
    pub budget: ResourceBudget,
    pub should_serialize_state: bool,
    pub release_gpu_resources: bool,
    pub zero_cpu_required: bool,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAdmissionStatus {
    Ready,
    NeedsPermission,
    Blocked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SandboxPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iframe_sandbox: Option<String>,
    pub isolated_origin: bool,
    pub csp: String,
    pub host_cookies: bool,
    pub top_navigation: bool,
    pub wasi_filesystem: bool,
    pub wasi_network: bool,
    pub capability_bridge: bool,
}

impl SandboxPolicy {
    pub fn for_target(target: &SurfaceTarget) -> Self {
        match target {
            SurfaceTarget::Static => Self {
                iframe_sandbox: None,
                isolated_origin: false,
                csp: "default-src 'none'; img-src 'self' data:; style-src 'self'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'".to_string(),
                host_cookies: false,
                top_navigation: false,
                wasi_filesystem: false,
                wasi_network: false,
                capability_bridge: false,
            },
            SurfaceTarget::Wasm => Self {
                iframe_sandbox: None,
                isolated_origin: true,
                csp: "default-src 'none'; connect-src 'none'; script-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'".to_string(),
                host_cookies: false,
                top_navigation: false,
                wasi_filesystem: false,
                wasi_network: false,
                capability_bridge: true,
            },
            SurfaceTarget::Web | SurfaceTarget::WebGpu => Self {
                iframe_sandbox: None,
                isolated_origin: true,
                csp: "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'".to_string(),
                host_cookies: false,
                top_navigation: false,
                wasi_filesystem: false,
                wasi_network: false,
                capability_bridge: true,
            },
            SurfaceTarget::NativeTrusted => Self {
                iframe_sandbox: None,
                isolated_origin: true,
                csp: "native-trusted-disabled-for-untrusted-objects".to_string(),
                host_cookies: false,
                top_navigation: false,
                wasi_filesystem: false,
                wasi_network: false,
                capability_bridge: false,
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WasmCapabilityImport {
    pub capability: String,
    pub version: u32,
    pub module: String,
    pub function: String,
    pub grant_id: Option<String>,
    pub max_call_ms: u64,
    pub bytes_per_minute: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WasmExecutionPolicy {
    pub isolated_store: bool,
    pub max_memory_bytes: u64,
    pub fuel_per_activation: u64,
    pub epoch_deadline_ms: u64,
    pub max_host_calls_per_activation: u32,
    pub max_host_call_bytes: u64,
    pub wasi_filesystem: bool,
    pub wasi_network: bool,
    pub allowed_imports: Vec<WasmCapabilityImport>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSessionPlan {
    pub object_id: ObjectId,
    pub surface: Surface,
    pub lifecycle: SurfaceLifecycle,
    pub admission: RuntimeAdmissionStatus,
    pub budget: ResourceBudget,
    pub sandbox: SandboxPolicy,
    pub wasm_execution: Option<WasmExecutionPolicy>,
    pub capability_decisions: Vec<CapabilityDecision>,
    pub blocked_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle_verification: Option<BundleVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verified_mount: Option<VerifiedSurfaceMount>,
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct SurfaceSessionId(String);

impl SurfaceSessionId {
    pub const PREFIX: &'static str = "surf_";

    pub fn from_material(material: impl AsRef<[u8]>) -> Self {
        Self(format!(
            "{}{}",
            Self::PREFIX,
            Hash::from_bytes(material.as_ref()).as_str()
        ))
    }

    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let id = Self(value);
        id.validate()?;
        Ok(id)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(&self) -> Result<()> {
        let Some(suffix) = self.0.strip_prefix(Self::PREFIX) else {
            return Err(babel_types::Error::InvalidPrefix {
                expected: Self::PREFIX,
                actual: self.0.clone(),
            });
        };
        if suffix.len() != 64 {
            return Err(babel_types::Error::InvalidHashLength {
                expected: 64,
                actual: suffix.len(),
            });
        }
        Ok(())
    }
}

impl std::fmt::Display for SurfaceSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceRuntimeEventKind {
    LifecycleTransition,
    BudgetChanged,
    StateCheckpointed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceRuntimeEvent {
    pub sequence: u64,
    pub session_id: SurfaceSessionId,
    pub object_id: ObjectId,
    pub kind: SurfaceRuntimeEventKind,
    pub previous_lifecycle: Option<SurfaceLifecycle>,
    pub lifecycle: SurfaceLifecycle,
    pub previous_budget: Option<ResourceBudget>,
    pub budget: ResourceBudget,
    pub reason: String,
    pub at: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceStateCheckpoint {
    pub session_id: SurfaceSessionId,
    pub object_id: ObjectId,
    pub lifecycle: SurfaceLifecycle,
    pub reason: String,
    pub state: serde_json::Value,
    pub state_hash: Hash,
    pub size_bytes: u64,
    pub created_at: Timestamp,
}

impl SurfaceStateCheckpoint {
    pub fn new(
        session_id: SurfaceSessionId,
        object_id: ObjectId,
        lifecycle: SurfaceLifecycle,
        reason: impl Into<String>,
        state: serde_json::Value,
    ) -> Result<Self> {
        let state_bytes = serde_json::to_vec(&state).map_err(|err| {
            babel_types::Error::Canonical(format!("encode Surface state checkpoint: {err}"))
        })?;
        Ok(Self {
            session_id,
            object_id,
            lifecycle,
            reason: clean_reason(reason),
            state,
            state_hash: Hash::from_bytes(&state_bytes),
            size_bytes: state_bytes.len() as u64,
            created_at: Timestamp::now(),
        })
    }

    pub fn verify(&self) -> Result<()> {
        self.session_id.validate()?;
        self.object_id.validate()?;
        let state_bytes = serde_json::to_vec(&self.state).map_err(|err| {
            babel_types::Error::Canonical(format!("encode Surface state checkpoint: {err}"))
        })?;
        let actual_hash = Hash::from_bytes(&state_bytes);
        let actual_size = state_bytes.len() as u64;
        if actual_hash != self.state_hash || actual_size != self.size_bytes {
            return Err(babel_types::Error::Conflict(format!(
                "Surface state checkpoint integrity mismatch for session {}",
                self.session_id
            )));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSession {
    pub id: SurfaceSessionId,
    pub plan: SurfaceSessionPlan,
    pub lifecycle: SurfaceLifecycle,
    pub budget: ResourceBudget,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub events: Vec<SurfaceRuntimeEvent>,
}

impl SurfaceSession {
    pub fn start(id: SurfaceSessionId, plan: SurfaceSessionPlan, reason: &str) -> Result<Self> {
        if plan.admission != RuntimeAdmissionStatus::Ready {
            return Err(babel_types::Error::Conflict(format!(
                "cannot start Surface session with admission {:?}",
                plan.admission
            )));
        }
        let now = Timestamp::now();
        let mut session = Self {
            id,
            lifecycle: plan.lifecycle.clone(),
            budget: plan.budget.clone(),
            plan,
            created_at: now,
            updated_at: now,
            events: Vec::new(),
        };
        if session.lifecycle != SurfaceLifecycle::Prefetched {
            session.transition(SurfaceLifecycle::Prefetched, reason)?;
        }
        Ok(session)
    }

    pub fn transition(
        &mut self,
        next: SurfaceLifecycle,
        reason: impl Into<String>,
    ) -> Result<SurfaceRuntimeEvent> {
        let reason = clean_reason(reason);
        if next == self.lifecycle {
            return self.record_event(
                SurfaceRuntimeEventKind::LifecycleTransition,
                Some(self.lifecycle.clone()),
                next,
                None,
                self.budget.clone(),
                reason,
            );
        }
        if !lifecycle_transition_allowed(&self.lifecycle, &next) {
            return Err(babel_types::Error::Conflict(format!(
                "invalid Surface lifecycle transition: {:?} -> {:?}",
                self.lifecycle, next
            )));
        }
        let previous = self.lifecycle.clone();
        self.lifecycle = next.clone();
        self.record_event(
            SurfaceRuntimeEventKind::LifecycleTransition,
            Some(previous),
            next,
            None,
            self.budget.clone(),
            reason,
        )
    }

    pub fn reduce_budget(
        &mut self,
        budget: ResourceBudget,
        reason: impl Into<String>,
    ) -> Result<SurfaceRuntimeEvent> {
        budget.validate_reduction_from(&self.budget)?;
        let previous = self.budget.clone();
        self.budget = budget.clone();
        self.record_event(
            SurfaceRuntimeEventKind::BudgetChanged,
            None,
            self.lifecycle.clone(),
            Some(previous),
            budget,
            clean_reason(reason),
        )
    }

    pub fn checkpoint_state(
        &mut self,
        checkpoint: &SurfaceStateCheckpoint,
    ) -> Result<SurfaceRuntimeEvent> {
        checkpoint.verify()?;
        if checkpoint.session_id != self.id || checkpoint.object_id != self.plan.object_id {
            return Err(babel_types::Error::Conflict(format!(
                "Surface state checkpoint {} is not bound to session {}",
                checkpoint.state_hash, self.id
            )));
        }
        self.record_event(
            SurfaceRuntimeEventKind::StateCheckpointed,
            None,
            self.lifecycle.clone(),
            None,
            self.budget.clone(),
            checkpoint.reason.clone(),
        )
    }

    fn record_event(
        &mut self,
        kind: SurfaceRuntimeEventKind,
        previous_lifecycle: Option<SurfaceLifecycle>,
        lifecycle: SurfaceLifecycle,
        previous_budget: Option<ResourceBudget>,
        budget: ResourceBudget,
        reason: String,
    ) -> Result<SurfaceRuntimeEvent> {
        let at = Timestamp::now();
        let sequence = self.events.len() as u64 + 1;
        let event = SurfaceRuntimeEvent {
            sequence,
            session_id: self.id.clone(),
            object_id: self.plan.object_id.clone(),
            kind,
            previous_lifecycle,
            lifecycle,
            previous_budget,
            budget,
            reason,
            at,
        };
        self.updated_at = at;
        self.events.push(event.clone());
        Ok(event)
    }
}

#[derive(Clone, Debug)]
pub struct SurfaceRuntime {
    broker: CapabilityBroker,
}

#[derive(Clone, Debug, Default)]
pub struct SurfaceScheduler;

impl SurfaceScheduler {
    pub fn new() -> Self {
        Self
    }

    pub fn recommend(
        &self,
        plan: &SurfaceSessionPlan,
        input: &SurfaceSchedulingInput,
    ) -> SurfaceScheduleDecision {
        let mut lifecycle = scheduled_lifecycle(plan, input);
        if plan.lifecycle == SurfaceLifecycle::Evicted {
            lifecycle = SurfaceLifecycle::Evicted;
        }
        if plan.admission != RuntimeAdmissionStatus::Ready {
            lifecycle = SurfaceLifecycle::Cold;
        }
        let budget = scheduled_budget(&plan.budget, &plan.surface.target, &lifecycle, input);
        let zero_cpu_required = matches!(
            lifecycle,
            SurfaceLifecycle::Suspended | SurfaceLifecycle::Evicted
        );
        SurfaceScheduleDecision {
            lifecycle: lifecycle.clone(),
            budget,
            should_serialize_state: matches!(
                lifecycle,
                SurfaceLifecycle::Suspended | SurfaceLifecycle::Evicted
            ),
            release_gpu_resources: plan.budget.gpu_expected
                && (matches!(
                    lifecycle,
                    SurfaceLifecycle::Suspended | SurfaceLifecycle::Evicted
                ) || input.gpu_pressure != RuntimePressureLevel::Normal),
            zero_cpu_required,
            reason: schedule_reason(plan, &lifecycle),
        }
    }
}

impl SurfaceRuntime {
    pub fn new(broker: CapabilityBroker) -> Self {
        Self { broker }
    }

    pub fn babel_default() -> Self {
        Self::new(CapabilityBroker::babel_default())
    }

    pub fn prepare_surface(
        &self,
        object: &Object,
        role: SurfaceRole,
        grants: &[CapabilityGrant],
    ) -> Result<SurfaceSessionPlan> {
        self.prepare_surface_with_bundle(object, role, grants, None)
    }

    fn prepare_surface_with_bundle(
        &self,
        object: &Object,
        role: SurfaceRole,
        grants: &[CapabilityGrant],
        receipt: Option<&babel_store::VerifiedBundle>,
    ) -> Result<SurfaceSessionPlan> {
        object.verify_unsigned_commitment_shape()?;
        let surface = object
            .surfaces
            .iter()
            .find(|surface| surface.role == role)
            .cloned()
            .ok_or_else(|| {
                babel_types::Error::NotFound(format!("surface {:?} for object {}", role, object.id))
            })?;
        let bundle_verification = receipt
            .map(|receipt| bundles::validate_receipt(object, &surface, receipt))
            .transpose()?;
        let mut blocked_reasons = executable_blockers(object, &surface, receipt.is_some());
        if object
            .surfaces
            .iter()
            .filter(|candidate| candidate.role == role)
            .count()
            > 1
        {
            blocked_reasons.push(format!(
                "object declares multiple {:?} surfaces; role selection must be unambiguous",
                role
            ));
        }
        let capability_decisions = self.broker.evaluate_object(object, grants);
        if surface.target == SurfaceTarget::WebGpu
            && !object
                .capabilities
                .iter()
                .any(|capability| capability.id == "babel.graphics.webgpu")
        {
            blocked_reasons
                .push("WebGPU surfaces must declare babel.graphics.webgpu capability".to_string());
        }
        let permission_blocked = capability_decisions.iter().any(|decision| {
            decision.status == CapabilityDecisionStatus::RequiresUser
                && !promptable_invocation_decision(decision)
        });
        for decision in &capability_decisions {
            if matches!(
                decision.status,
                CapabilityDecisionStatus::Denied
                    | CapabilityDecisionStatus::Unavailable
                    | CapabilityDecisionStatus::VersionUnsupported
                    | CapabilityDecisionStatus::Revoked
            ) {
                blocked_reasons.push(format!(
                    "capability {}@{}: {}",
                    decision.request.id, decision.request.version, decision.reason
                ));
            }
        }

        let admission = if !blocked_reasons.is_empty() {
            RuntimeAdmissionStatus::Blocked
        } else if permission_blocked {
            RuntimeAdmissionStatus::NeedsPermission
        } else {
            RuntimeAdmissionStatus::Ready
        };

        let budget = ResourceBudget::for_surface(&surface.role, &surface.target);
        let sandbox = if receipt.is_some() {
            bundles::verified_sandbox()
        } else {
            SandboxPolicy::for_target(&surface.target)
        };
        let wasm_execution = wasm_execution_policy(&surface, &budget, &capability_decisions);

        Ok(SurfaceSessionPlan {
            object_id: object.id.clone(),
            surface: surface.clone(),
            lifecycle: SurfaceLifecycle::Cold,
            admission,
            budget,
            sandbox,
            wasm_execution,
            capability_decisions,
            blocked_reasons,
            bundle_verification,
            verified_mount: None,
        })
    }

    pub fn start_session(
        &self,
        plan: SurfaceSessionPlan,
        requested_id: Option<SurfaceSessionId>,
    ) -> Result<SurfaceSession> {
        let id = requested_id.unwrap_or_else(|| {
            SurfaceSessionId::from_material(format!(
                "{}:{}:{:?}:{}",
                plan.object_id,
                plan.surface.entry,
                plan.surface.role,
                Timestamp::now().0
            ))
        });
        SurfaceSession::start(id, plan, "Surface session admitted by host runtime")
    }
}

/// One-use consent is requested for exact intent after the Surface opens.
fn promptable_invocation_decision(decision: &CapabilityDecision) -> bool {
    decision.request.version == 1
        && decision.definition.as_ref().is_some_and(|definition| {
            definition.permission == babel_capabilities::PermissionMode::AskEachTime
        })
        && babel_capabilities::invocation::is_one_use_invocation(decision.request.id.as_str())
}

trait ObjectRuntimeValidation {
    fn verify_unsigned_commitment_shape(&self) -> Result<()>;
}

impl ObjectRuntimeValidation for Object {
    fn verify_unsigned_commitment_shape(&self) -> Result<()> {
        self.id.validate()?;
        Ok(())
    }
}

fn executable_blockers(object: &Object, surface: &Surface, verified_bundle: bool) -> Vec<String> {
    let mut reasons = Vec::new();
    let entry = surface.entry.as_str();
    if entry.is_empty() {
        reasons.push("surface entry must not be empty".to_string());
    }
    let parsed_entry = babel_object::resource_uri::ResourceUri::parse(entry);
    if parsed_entry.is_err() {
        reasons.push(
            "surface entry must be a safe relative or integrity-addressed HTTP(S) resource"
                .to_string(),
        );
    }
    if let (Ok(uri), Some(integrity)) = (&parsed_entry, &surface.integrity) {
        if uri.validate_integrity(integrity).is_err() {
            reasons.push("surface entry requires matching canonical integrity".to_string());
        }
    }
    if surface.target == SurfaceTarget::NativeTrusted {
        reasons.push("native trusted targets are not admitted for untrusted Objects".to_string());
    }
    if surface.role == SurfaceRole::Background {
        let has_background_capability = object.capabilities.iter().any(|capability| {
            matches!(
                capability.id.as_str(),
                "babel.realtime.join" | "babel.realtime.send" | "babel.storage.local"
            )
        });
        if !has_background_capability {
            reasons.push(
                "background surfaces must declare a storage or realtime capability".to_string(),
            );
        }
    }
    if let Some(bundle) = &surface.bundle {
        if let Err(error) = bundle.validate_surface(surface) {
            reasons.push(format!("invalid executable bundle: {error}"));
        }
        if !verified_bundle {
            reasons.push("executable bundles require a verified bundle execution gateway".into());
        }
        return reasons;
    }
    if matches!(
        surface.target,
        SurfaceTarget::Wasm | SurfaceTarget::Web | SurfaceTarget::WebGpu
    ) {
        let Some(integrity) = &surface.integrity else {
            reasons.push("executable surfaces require integrity-addressed resources".to_string());
            return reasons;
        };
        let mut has_integrity_match = false;
        let mut has_uri_match = false;
        for resource in object
            .resources
            .iter()
            .filter(|resource| &resource.integrity == integrity)
        {
            has_integrity_match = true;
            if parsed_entry
                .as_ref()
                .is_ok_and(|uri| uri.matches_resource(resource))
            {
                has_uri_match = true;
                if surface_target_accepts_media_type(&surface.target, &resource.media_type) {
                    return reasons;
                }
            }
        }
        if !has_integrity_match {
            reasons.push("surface integrity must match a declared resource".to_string());
        } else if !has_uri_match {
            reasons
                .push("surface entry must match the integrity-addressed resource URI".to_string());
        } else {
            reasons.push(format!(
                "surface target {:?} does not accept the media type of any matching resource",
                surface.target
            ));
        }
    }
    reasons
}

fn surface_target_accepts_media_type(target: &SurfaceTarget, media_type: &str) -> bool {
    match target {
        SurfaceTarget::Static => matches!(
            media_type,
            "text/html" | "text/plain" | "image/png" | "image/jpeg" | "image/gif" | "image/webp"
        ),
        SurfaceTarget::Wasm => media_type == "application/wasm",
        SurfaceTarget::Web | SurfaceTarget::WebGpu => matches!(
            media_type,
            "text/html" | "text/javascript" | "application/javascript" | "text/css"
        ),
        SurfaceTarget::NativeTrusted => false,
    }
}

fn wasm_execution_policy(
    surface: &Surface,
    budget: &ResourceBudget,
    decisions: &[CapabilityDecision],
) -> Option<WasmExecutionPolicy> {
    if surface.target != SurfaceTarget::Wasm {
        return None;
    }
    let mut allowed_imports = decisions
        .iter()
        .filter_map(wasm_capability_import)
        .collect::<Vec<_>>();
    allowed_imports.sort_by(|left, right| {
        left.module
            .cmp(&right.module)
            .then_with(|| left.function.cmp(&right.function))
    });
    Some(WasmExecutionPolicy {
        isolated_store: true,
        max_memory_bytes: budget.memory_bytes,
        fuel_per_activation: wasm_fuel_for_budget(budget),
        epoch_deadline_ms: budget.cpu_ms_per_minute.clamp(50, 5_000),
        max_host_calls_per_activation: wasm_host_call_limit(budget),
        max_host_call_bytes: budget.network_bytes_per_minute.max(4 * 1024),
        wasi_filesystem: false,
        wasi_network: false,
        allowed_imports,
    })
}

fn wasm_capability_import(decision: &CapabilityDecision) -> Option<WasmCapabilityImport> {
    if decision.status != CapabilityDecisionStatus::Granted {
        return None;
    }
    let definition = decision.definition.as_ref()?;
    let capability = definition.id.as_str();
    Some(WasmCapabilityImport {
        capability: capability.to_string(),
        version: definition.version,
        module: format!("babel:capability/{}@{}", capability, definition.version),
        function: capability_import_function(capability),
        grant_id: decision.grant.as_ref().map(|grant| grant.id.to_string()),
        max_call_ms: definition.quota.max_call_ms,
        bytes_per_minute: definition.quota.bytes_per_minute,
    })
}

fn capability_import_function(capability: &str) -> String {
    capability
        .strip_prefix("babel.")
        .unwrap_or(capability)
        .replace(['.', '-'], "_")
}

fn wasm_fuel_for_budget(budget: &ResourceBudget) -> u64 {
    budget
        .cpu_ms_per_minute
        .saturating_mul(10_000)
        .clamp(500_000, 300_000_000)
}

fn wasm_host_call_limit(budget: &ResourceBudget) -> u32 {
    (budget.cpu_ms_per_minute / 50).clamp(4, 1_000) as u32
}

fn scheduled_lifecycle(
    plan: &SurfaceSessionPlan,
    input: &SurfaceSchedulingInput,
) -> SurfaceLifecycle {
    if input.memory_pressure == RuntimePressureLevel::Critical
        && (input.viewport_distance_px > 1_200 || input.interaction_score < 600)
    {
        return SurfaceLifecycle::Evicted;
    }
    if input.gpu_pressure == RuntimePressureLevel::Critical
        && plan.budget.gpu_expected
        && input.viewport_distance_px > 600
    {
        return SurfaceLifecycle::Evicted;
    }
    if input.viewport_distance_px <= 80 && input.interaction_score >= 650 {
        return SurfaceLifecycle::Active;
    }
    if input.viewport_distance_px <= 400 || input.interaction_score >= 700 {
        return SurfaceLifecycle::Warm;
    }
    if input.viewport_distance_px <= prefetch_distance(input)
        && (input.approaching_viewport || input.interaction_score >= 300)
    {
        return SurfaceLifecycle::Prefetched;
    }
    if input.battery_saver
        || input.memory_pressure != RuntimePressureLevel::Normal
        || (plan.budget.gpu_expected && input.gpu_pressure != RuntimePressureLevel::Normal)
    {
        return SurfaceLifecycle::Suspended;
    }
    SurfaceLifecycle::Cold
}

fn scheduled_budget(
    base: &ResourceBudget,
    target: &SurfaceTarget,
    lifecycle: &SurfaceLifecycle,
    input: &SurfaceSchedulingInput,
) -> ResourceBudget {
    let mut budget = base.clone();
    apply_device_budget(&mut budget, input);
    if input.metered_network {
        budget.network_bytes_per_minute = budget.network_bytes_per_minute.min(256 * 1024);
    }
    if input.memory_pressure == RuntimePressureLevel::Elevated {
        budget.memory_bytes /= 2;
    }
    if input.memory_pressure == RuntimePressureLevel::Critical {
        budget.memory_bytes /= 4;
    }
    if input.battery_saver {
        budget.cpu_ms_per_minute /= 2;
        budget.gpu_expected = false;
    }
    if input.gpu_pressure != RuntimePressureLevel::Normal && matches!(target, SurfaceTarget::WebGpu)
    {
        budget.gpu_expected = false;
    }
    match lifecycle {
        SurfaceLifecycle::Active => budget,
        SurfaceLifecycle::Warm => ResourceBudget {
            cpu_ms_per_minute: budget.cpu_ms_per_minute.min(base.cpu_ms_per_minute / 2),
            network_bytes_per_minute: budget
                .network_bytes_per_minute
                .min(base.network_bytes_per_minute / 2),
            realtime_connections: budget.realtime_connections.min(base.realtime_connections),
            ..budget
        },
        SurfaceLifecycle::Prefetched => ResourceBudget {
            memory_bytes: budget.memory_bytes.min(base.memory_bytes / 2),
            cpu_ms_per_minute: budget.cpu_ms_per_minute.min(250),
            network_bytes_per_minute: budget.network_bytes_per_minute.min(64 * 1024),
            persistent_storage_bytes: budget
                .persistent_storage_bytes
                .min(base.persistent_storage_bytes),
            realtime_connections: 0,
            gpu_expected: false,
            background_eligible: false,
        },
        SurfaceLifecycle::Cold => ResourceBudget {
            memory_bytes: budget.memory_bytes.min(4 * 1024 * 1024),
            cpu_ms_per_minute: 0,
            gpu_expected: false,
            network_bytes_per_minute: 0,
            persistent_storage_bytes: budget
                .persistent_storage_bytes
                .min(base.persistent_storage_bytes),
            realtime_connections: 0,
            background_eligible: false,
        },
        SurfaceLifecycle::Suspended => ResourceBudget {
            memory_bytes: budget.memory_bytes.min(2 * 1024 * 1024),
            cpu_ms_per_minute: 0,
            gpu_expected: false,
            network_bytes_per_minute: 0,
            persistent_storage_bytes: budget
                .persistent_storage_bytes
                .min(base.persistent_storage_bytes),
            realtime_connections: if base.background_eligible {
                budget.realtime_connections.min(1)
            } else {
                0
            },
            background_eligible: base.background_eligible,
        },
        SurfaceLifecycle::Evicted => ResourceBudget {
            memory_bytes: 0,
            cpu_ms_per_minute: 0,
            gpu_expected: false,
            network_bytes_per_minute: 0,
            persistent_storage_bytes: 0,
            realtime_connections: 0,
            background_eligible: false,
        },
    }
}

fn apply_device_budget(budget: &mut ResourceBudget, input: &SurfaceSchedulingInput) {
    match input.device_class {
        RuntimeDeviceClass::LowPowerMobile => {
            budget.memory_bytes /= 4;
            budget.cpu_ms_per_minute /= 4;
            budget.network_bytes_per_minute /= 4;
            budget.realtime_connections = budget.realtime_connections.min(1);
        }
        RuntimeDeviceClass::Mobile => {
            budget.memory_bytes /= 2;
            budget.cpu_ms_per_minute /= 2;
            budget.network_bytes_per_minute /= 2;
            budget.realtime_connections = budget.realtime_connections.min(2);
        }
        RuntimeDeviceClass::Tablet => {
            budget.memory_bytes = budget.memory_bytes.saturating_mul(3) / 4;
            budget.cpu_ms_per_minute = budget.cpu_ms_per_minute.saturating_mul(3) / 4;
        }
        RuntimeDeviceClass::Desktop => {}
        RuntimeDeviceClass::Workstation => {}
    }
}

fn prefetch_distance(input: &SurfaceSchedulingInput) -> u32 {
    match input.device_class {
        RuntimeDeviceClass::LowPowerMobile => 700,
        RuntimeDeviceClass::Mobile => 1_000,
        RuntimeDeviceClass::Tablet => 1_400,
        RuntimeDeviceClass::Desktop => 2_000,
        RuntimeDeviceClass::Workstation => 2_800,
    }
}

fn schedule_reason(plan: &SurfaceSessionPlan, lifecycle: &SurfaceLifecycle) -> String {
    if plan.admission != RuntimeAdmissionStatus::Ready {
        return "surface is not admitted by the runtime".to_string();
    }
    match lifecycle {
        SurfaceLifecycle::Active => "surface is visible and likely to be used".to_string(),
        SurfaceLifecycle::Warm => {
            "surface is near viewport or has high interaction likelihood".to_string()
        }
        SurfaceLifecycle::Prefetched => {
            "surface is approaching viewport within device prefetch distance".to_string()
        }
        SurfaceLifecycle::Cold => "surface is distant and can remain cold".to_string(),
        SurfaceLifecycle::Suspended => {
            "surface is offscreen under pressure and must release active work".to_string()
        }
        SurfaceLifecycle::Evicted => {
            "surface is offscreen under critical pressure and must be evicted".to_string()
        }
    }
}

fn lifecycle_transition_allowed(current: &SurfaceLifecycle, next: &SurfaceLifecycle) -> bool {
    matches!(
        (current, next),
        (SurfaceLifecycle::Cold, SurfaceLifecycle::Prefetched)
            | (SurfaceLifecycle::Cold, SurfaceLifecycle::Warm)
            | (SurfaceLifecycle::Cold, SurfaceLifecycle::Active)
            | (SurfaceLifecycle::Cold, SurfaceLifecycle::Suspended)
            | (SurfaceLifecycle::Cold, SurfaceLifecycle::Evicted)
            | (SurfaceLifecycle::Prefetched, SurfaceLifecycle::Warm)
            | (SurfaceLifecycle::Prefetched, SurfaceLifecycle::Active)
            | (SurfaceLifecycle::Prefetched, SurfaceLifecycle::Suspended)
            | (SurfaceLifecycle::Prefetched, SurfaceLifecycle::Evicted)
            | (SurfaceLifecycle::Warm, SurfaceLifecycle::Active)
            | (SurfaceLifecycle::Warm, SurfaceLifecycle::Suspended)
            | (SurfaceLifecycle::Warm, SurfaceLifecycle::Evicted)
            | (SurfaceLifecycle::Active, SurfaceLifecycle::Suspended)
            | (SurfaceLifecycle::Active, SurfaceLifecycle::Evicted)
            | (SurfaceLifecycle::Suspended, SurfaceLifecycle::Warm)
            | (SurfaceLifecycle::Suspended, SurfaceLifecycle::Active)
            | (SurfaceLifecycle::Suspended, SurfaceLifecycle::Evicted)
    )
}

fn clean_reason(reason: impl Into<String>) -> String {
    let reason = reason.into();
    let reason = reason.trim();
    if reason.is_empty() {
        "host runtime state changed".to_string()
    } else {
        reason.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_capabilities::{CapabilityBroker, GrantDecision};
    use babel_crypto::Keypair;
    use babel_identity::{Identity, IdentityKind};
    use babel_object::{CapabilityRequest, Object, Resource, Surface, SurfaceRole, SurfaceTarget};
    use babel_types::Hash;
    use serde_json::json;

    #[test]
    fn invocation_lazy_social_admission_preserves_integrity_and_other_permission_gates() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "invocation", &keypair).unwrap();
        let digest = Hash::from_bytes(b"surface");
        for capability in [
            "babel.social.follow",
            "babel.social.unfollow",
            "babel.social.share",
            "babel.social.reply",
            "babel.clipboard.write",
            "babel.fullscreen.enter",
            "babel.media.camera",
        ] {
            let object = Object::text(&identity, "controller")
                .unwrap()
                .with_resources(vec![Resource {
                    uri: "app.js".into(),
                    media_type: "text/javascript".into(),
                    integrity: digest.clone(),
                }])
                .unwrap()
                .with_surfaces(vec![Surface {
                    role: SurfaceRole::Feed,
                    target: SurfaceTarget::Web,
                    entry: "app.js".into(),
                    integrity: Some(digest.clone()),
                    bundle: None,
                }])
                .unwrap()
                .with_capabilities(vec![CapabilityRequest {
                    id: capability.into(),
                    version: 1,
                    scope: if capability == "babel.media.camera" {
                        json!({"modes":["photo"],"media_types":["image/png"]})
                    } else {
                        json!({})
                    },
                }])
                .unwrap()
                .sign(&identity, &keypair)
                .unwrap();
            let runtime = SurfaceRuntime::babel_default();
            let plan = runtime
                .prepare_surface(&object, SurfaceRole::Feed, &[])
                .unwrap();
            assert_eq!(
                plan.admission,
                if babel_capabilities::invocation::is_one_use_invocation(capability) {
                    RuntimeAdmissionStatus::Ready
                } else {
                    RuntimeAdmissionStatus::NeedsPermission
                }
            );
            assert_eq!(
                plan.capability_decisions[0].status,
                CapabilityDecisionStatus::RequiresUser
            );
            assert!(plan.capability_decisions[0].grant.is_none());
        }
    }

    #[test]
    fn runtime_blocks_executable_surface_without_integrity() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "index.html".to_string(),
                integrity: None,
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();

        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();

        assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            plan.blocked_reasons
                .iter()
                .any(|reason| reason.contains("integrity"))
        );
    }

    #[test]
    fn runtime_blocks_executable_surface_resource_abuse() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let bundle_hash = Hash::from_bytes(b"console.log('babel')");
        let image_hash = Hash::from_bytes(b"not executable");
        let base = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![
                Resource {
                    uri: "bundle.js".to_string(),
                    media_type: "text/javascript".to_string(),
                    integrity: bundle_hash.clone(),
                },
                Resource {
                    uri: "image.png".to_string(),
                    media_type: "image/png".to_string(),
                    integrity: image_hash.clone(),
                },
            ])
            .unwrap();

        let unsafe_entry = base
            .clone()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "file:///tmp/secret.js".to_string(),
                integrity: Some(bundle_hash.clone()),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let unsafe_plan = SurfaceRuntime::babel_default()
            .prepare_surface(&unsafe_entry, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(unsafe_plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            unsafe_plan.blocked_reasons.iter().any(|reason| {
                reason.contains("safe relative") || reason.contains("resource URI")
            })
        );

        let wrong_media = base
            .clone()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "image.png".to_string(),
                integrity: Some(image_hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let media_plan = SurfaceRuntime::babel_default()
            .prepare_surface(&wrong_media, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(media_plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            media_plan
                .blocked_reasons
                .iter()
                .any(|reason| reason.contains("media type"))
        );

        let traversal = base
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "../bundle.js".to_string(),
                integrity: Some(bundle_hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let traversal_plan = SurfaceRuntime::babel_default()
            .prepare_surface(&traversal, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(traversal_plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            traversal_plan
                .blocked_reasons
                .iter()
                .any(|reason| reason.contains("safe relative"))
        );
    }

    #[test]
    fn runtime_accepts_integrity_addressed_babel_blob_surface_entries() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let uri = format!("babel://blobs/{hash}");
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: uri.clone(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: uri.clone(),
                integrity: Some(hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();

        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();

        assert_eq!(plan.admission, RuntimeAdmissionStatus::Ready);
        assert_eq!(plan.surface.entry, uri);
        assert!(plan.blocked_reasons.is_empty());
    }

    #[test]
    fn runtime_blocks_ambiguous_surface_roles_and_webgpu_without_capability() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: "bundle.js".to_string(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![
                Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: SurfaceTarget::WebGpu,
                    entry: "bundle.js".to_string(),
                    integrity: Some(hash.clone()),
                },
                Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: SurfaceTarget::Web,
                    entry: "bundle.js".to_string(),
                    integrity: Some(hash),
                },
            ])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();

        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();

        assert_eq!(plan.admission, RuntimeAdmissionStatus::Blocked);
        assert!(
            plan.blocked_reasons
                .iter()
                .any(|reason| reason.contains("multiple Feed surfaces"))
        );
        assert!(
            plan.blocked_reasons
                .iter()
                .any(|reason| reason.contains("babel.graphics.webgpu"))
        );
    }

    #[test]
    fn runtime_admits_surface_after_required_grant_exists() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let request = CapabilityRequest {
            id: "babel.network.fetch".to_string(),
            version: 1,
            scope: json!({"origins": ["https://example.com"]}),
        };
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: "bundle.js".to_string(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "bundle.js".to_string(),
                integrity: Some(hash),
            }])
            .unwrap()
            .with_capabilities(vec![request.clone()])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let broker = CapabilityBroker::babel_default();
        let grant = broker
            .issue_grant(object.id.clone(), request, GrantDecision::Approved, None)
            .unwrap();

        let plan = SurfaceRuntime::new(broker)
            .prepare_surface(&object, SurfaceRole::Feed, &[grant])
            .unwrap();

        assert_eq!(plan.admission, RuntimeAdmissionStatus::Ready);
        assert!(plan.sandbox.isolated_origin);
        assert!(plan.sandbox.capability_bridge);
        assert!(plan.sandbox.csp.contains("worker-src 'none'"));
        assert!(plan.sandbox.csp.contains("object-src 'none'"));
        assert!(plan.sandbox.csp.contains("base-uri 'none'"));
        assert!(plan.sandbox.csp.contains("form-action 'none'"));
    }

    #[test]
    fn runtime_wasm_policy_uses_isolated_store_and_granted_capability_imports() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"\0asm babel module");
        let request = CapabilityRequest {
            id: "babel.storage.object".to_string(),
            version: 1,
            scope: json!({"namespace": "state"}),
        };
        let object = Object::text(&identity, "wasm object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: format!("babel://blobs/{hash}"),
                media_type: "application/wasm".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Wasm,
                entry: format!("babel://blobs/{hash}"),
                integrity: Some(hash),
            }])
            .unwrap()
            .with_capabilities(vec![request.clone()])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();

        let pending = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        assert_eq!(pending.admission, RuntimeAdmissionStatus::NeedsPermission);
        let pending_policy = pending
            .wasm_execution
            .expect("WASM surfaces must include an execution policy");
        assert!(pending_policy.isolated_store);
        assert!(!pending_policy.wasi_filesystem);
        assert!(!pending_policy.wasi_network);
        assert!(pending_policy.allowed_imports.is_empty());

        let broker = CapabilityBroker::babel_default();
        let grant = broker
            .issue_grant(object.id.clone(), request, GrantDecision::Approved, None)
            .unwrap();
        let ready = SurfaceRuntime::new(broker)
            .prepare_surface(&object, SurfaceRole::Feed, &[grant.clone()])
            .unwrap();

        assert_eq!(ready.admission, RuntimeAdmissionStatus::Ready);
        let policy = ready
            .wasm_execution
            .expect("ready WASM surfaces must include an execution policy");
        assert_eq!(policy.max_memory_bytes, ready.budget.memory_bytes);
        assert!(policy.fuel_per_activation >= 500_000);
        assert!(policy.max_host_calls_per_activation >= 4);
        assert!(!policy.wasi_filesystem);
        assert!(!policy.wasi_network);
        assert_eq!(policy.allowed_imports.len(), 1);
        assert_eq!(policy.allowed_imports[0].capability, "babel.storage.object");
        assert_eq!(
            policy.allowed_imports[0].module,
            "babel:capability/babel.storage.object@1"
        );
        assert_eq!(policy.allowed_imports[0].function, "storage_object");
        assert_eq!(
            policy.allowed_imports[0].grant_id.as_deref(),
            Some(grant.id.as_str())
        );
    }

    #[test]
    fn runtime_sandbox_policies_block_workers_for_all_untrusted_targets() {
        for target in [
            SurfaceTarget::Static,
            SurfaceTarget::Wasm,
            SurfaceTarget::Web,
            SurfaceTarget::WebGpu,
        ] {
            let policy = SandboxPolicy::for_target(&target);
            assert!(
                policy.csp.contains("worker-src 'none'"),
                "{target:?} policy must explicitly deny worker creation"
            );
            assert!(
                policy.csp.contains("object-src 'none'"),
                "{target:?} policy must explicitly deny plugin/object content"
            );
            assert!(
                policy.csp.contains("base-uri 'none'"),
                "{target:?} policy must block base URL rewriting"
            );
            assert!(
                policy.csp.contains("form-action 'none'"),
                "{target:?} policy must block form exfiltration"
            );
        }
    }

    #[test]
    fn runtime_surface_sessions_track_lifecycle_and_budget_events() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: "bundle.js".to_string(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "bundle.js".to_string(),
                integrity: Some(hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();

        let runtime = SurfaceRuntime::babel_default();
        let plan = runtime
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        let mut session = runtime
            .start_session(
                plan,
                Some(
                    SurfaceSessionId::new(
                        "surf_0000000000000000000000000000000000000000000000000000000000000000",
                    )
                    .unwrap(),
                ),
            )
            .unwrap();

        assert_eq!(session.lifecycle, SurfaceLifecycle::Prefetched);
        assert_eq!(session.events.len(), 1);

        let active = session
            .transition(SurfaceLifecycle::Warm, "viewport nearing")
            .unwrap();
        assert_eq!(active.sequence, 2);
        session
            .transition(SurfaceLifecycle::Active, "visible")
            .unwrap();
        assert!(
            session
                .transition(SurfaceLifecycle::Prefetched, "invalid rewind")
                .is_err()
        );

        let lowered = ResourceBudget {
            memory_bytes: session.budget.memory_bytes / 2,
            cpu_ms_per_minute: session.budget.cpu_ms_per_minute / 2,
            gpu_expected: false,
            network_bytes_per_minute: session.budget.network_bytes_per_minute,
            persistent_storage_bytes: session.budget.persistent_storage_bytes,
            realtime_connections: session.budget.realtime_connections,
            background_eligible: false,
        };
        let budget = session
            .reduce_budget(lowered.clone(), "thermal pressure")
            .unwrap();
        assert_eq!(budget.kind, SurfaceRuntimeEventKind::BudgetChanged);
        assert_eq!(session.budget, lowered);

        let raised = ResourceBudget {
            memory_bytes: session.budget.memory_bytes + 1,
            ..session.budget.clone()
        };
        assert!(session.reduce_budget(raised, "raise").is_err());
        let live = SurfaceRuntimeHealthSnapshot::from_sessions([&session]);
        assert_eq!(live.totals.memory_bytes, lowered.memory_bytes);
        session
            .transition(SurfaceLifecycle::Evicted, "host lease expired")
            .unwrap();
        let retired = SurfaceRuntimeHealthSnapshot::from_sessions([&session]);
        assert_eq!(retired.session_count, 1);
        assert_eq!(retired.sessions[0].lifecycle, SurfaceLifecycle::Evicted);
        assert_eq!(retired.totals, SurfaceResourceTotals::default());
    }

    #[test]
    fn scheduler_suspends_offscreen_surfaces_with_zero_cpu_under_pressure() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: "bundle.js".to_string(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "bundle.js".to_string(),
                integrity: Some(hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        let input = SurfaceSchedulingInput {
            viewport_distance_px: 4_000,
            approaching_viewport: false,
            interaction_score: 50,
            memory_pressure: RuntimePressureLevel::Elevated,
            gpu_pressure: RuntimePressureLevel::Normal,
            battery_saver: true,
            metered_network: true,
            device_class: RuntimeDeviceClass::Mobile,
        };

        let decision = SurfaceScheduler::new().recommend(&plan, &input);

        assert_eq!(decision.lifecycle, SurfaceLifecycle::Suspended);
        assert!(decision.should_serialize_state);
        assert!(decision.zero_cpu_required);
        assert_eq!(decision.budget.cpu_ms_per_minute, 0);
        assert_eq!(decision.budget.network_bytes_per_minute, 0);
        assert_eq!(decision.budget.realtime_connections, 0);
        assert!(!decision.budget.gpu_expected);
        decision
            .budget
            .validate_reduction_from(&plan.budget)
            .unwrap();
    }

    #[test]
    fn scheduler_prefetches_approaching_surfaces_and_evicts_under_critical_pressure() {
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        let hash = Hash::from_bytes(b"console.log('babel')");
        let object = Object::text(&identity, "active object")
            .unwrap()
            .with_resources(vec![Resource {
                uri: "bundle.js".to_string(),
                media_type: "text/javascript".to_string(),
                integrity: hash.clone(),
            }])
            .unwrap()
            .with_surfaces(vec![Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "bundle.js".to_string(),
                integrity: Some(hash),
            }])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let plan = SurfaceRuntime::babel_default()
            .prepare_surface(&object, SurfaceRole::Feed, &[])
            .unwrap();
        let scheduler = SurfaceScheduler::new();
        let approaching = SurfaceSchedulingInput {
            viewport_distance_px: 1_500,
            approaching_viewport: true,
            interaction_score: 350,
            memory_pressure: RuntimePressureLevel::Normal,
            gpu_pressure: RuntimePressureLevel::Normal,
            battery_saver: false,
            metered_network: false,
            device_class: RuntimeDeviceClass::Desktop,
        };
        let prefetched = scheduler.recommend(&plan, &approaching);
        assert_eq!(prefetched.lifecycle, SurfaceLifecycle::Prefetched);
        assert_eq!(prefetched.budget.realtime_connections, 0);
        prefetched
            .budget
            .validate_reduction_from(&plan.budget)
            .unwrap();

        let critical = SurfaceSchedulingInput {
            memory_pressure: RuntimePressureLevel::Critical,
            viewport_distance_px: 2_000,
            interaction_score: 100,
            ..approaching
        };
        let evicted = scheduler.recommend(&plan, &critical);
        assert_eq!(evicted.lifecycle, SurfaceLifecycle::Evicted);
        assert!(evicted.should_serialize_state);
        assert!(evicted.zero_cpu_required);
        assert_eq!(evicted.budget.memory_bytes, 0);
        assert_eq!(evicted.budget.persistent_storage_bytes, 0);
        evicted
            .budget
            .validate_reduction_from(&plan.budget)
            .unwrap();
    }
}

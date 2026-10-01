//! Private node moderation contracts and deterministic case transitions.
use babel_types::{Canonical, Error, Hash, IdentityId, JudgmentId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const POLICY: &str = "babel.integrity.v1";
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;
pub const REPORT_INTAKE_LIMIT: &str =
    "maximum 1000 reports per account reached; existing cases can still be reviewed or appealed";

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModerationReason {
    Spam,
    Malware,
    Fraud,
    Harassment,
    IllegalContent,
    OtherIntegrity,
}
pub const REASONS: [ModerationReason; 6] = [
    ModerationReason::Spam,
    ModerationReason::Malware,
    ModerationReason::Fraud,
    ModerationReason::Harassment,
    ModerationReason::IllegalContent,
    ModerationReason::OtherIntegrity,
];
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModerationOutcome {
    NoAction,
    Restrict,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModerationStatus {
    Pending,
    Decided,
    Appealed,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportRequest {
    pub object_id: ObjectId,
    pub reason: ModerationReason,
    pub details: String,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub outcome: ModerationOutcome,
    pub reason: ModerationReason,
    pub explanation: String,
    pub policy_version: String,
    pub source_signals: Vec<JudgmentId>,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppealRequest {
    pub details: String,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
    pub idempotency_key: String,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModerationDecision {
    pub reviewer_id: IdentityId,
    pub outcome: ModerationOutcome,
    pub reason: ModerationReason,
    pub explanation: String,
    pub policy_version: String,
    pub source_signals: Vec<JudgmentId>,
    pub created_at: Timestamp,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["appellant_id", "details", "created_at"]))]
pub struct ModerationAppeal {
    pub appellant_id: Option<IdentityId>,
    pub details: Option<String>,
    pub created_at: Timestamp,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["id", "sequence", "object_id", "subject_author_id", "reporter_id", "reason", "details", "created_at", "updated_at", "revision", "status", "decisions", "appeal"]))]
pub struct ModerationCase {
    #[schemars(regex(pattern = "^report_[0-9a-f]{64}$"))]
    pub id: String,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub sequence: u64,
    pub object_id: ObjectId,
    pub subject_author_id: IdentityId,
    pub reporter_id: Option<IdentityId>,
    pub reason: Option<ModerationReason>,
    pub details: Option<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    pub status: ModerationStatus,
    #[schemars(length(max = 2))]
    pub decisions: Vec<ModerationDecision>,
    pub appeal: Option<ModerationAppeal>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModerationAccess {
    pub actor_id: IdentityId,
    pub can_review: bool,
    pub policy_version: String,
    pub reasons: Vec<ModerationReason>,
}
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["items", "next_before"]))]
pub struct ModerationPage {
    pub items: Vec<ModerationCase>,
    pub next_before: Option<u64>,
}
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ModerationScope {
    Mine,
    Affected,
    Queue,
}

pub fn canonical_id(value: &str, prefix: &str) -> Result<()> {
    if value.strip_prefix(prefix).is_none_or(|s| {
        s.len() != 64
            || !s
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(Error::Canonical(
            "invalid canonical moderation identifier".into(),
        ));
    }
    Ok(())
}
pub fn parse_reviewers(value: &str) -> Result<BTreeSet<IdentityId>> {
    if value.is_empty() {
        return Ok(BTreeSet::new());
    }
    if value.len() > 67 * 100 + 100 {
        return Err(Error::Canonical("too many moderators".into()));
    }
    let mut result = BTreeSet::new();
    for raw in value.split(',') {
        canonical_id(raw, "id_")?;
        if !result.insert(IdentityId::new_unchecked(raw)) {
            return Err(Error::Canonical("duplicate moderator".into()));
        }
    }
    if result.len() > 100 {
        return Err(Error::Canonical("too many moderators".into()));
    }
    Ok(result)
}
pub fn text(value: &str) -> Result<String> {
    if value.len() > 16_000 || !(20..=4000).contains(&value.trim().chars().count()) {
        return Err(Error::Canonical(
            "moderation text must contain 20..4000 characters and at most 16000 bytes".into(),
        ));
    }
    Ok(value.trim().into())
}
pub fn key(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 256 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(Error::Canonical(
            "idempotency key must be 1..256 visible ASCII bytes".into(),
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModerationIntent {
    Report(ReportRequest),
    Decision {
        case_id: String,
        request: DecisionRequest,
    },
    Appeal {
        case_id: String,
        request: AppealRequest,
    },
}
impl ModerationIntent {
    pub fn key(&self) -> &str {
        match self {
            Self::Report(r) => &r.idempotency_key,
            Self::Decision { request: r, .. } => &r.idempotency_key,
            Self::Appeal { request: r, .. } => &r.idempotency_key,
        }
    }
    pub fn case_id(&self, actor: &IdentityId) -> Result<String> {
        Ok(match self {
            Self::Report(_) => format!(
                "report_{}",
                ("babel.private.moderation.case.v1", actor, self.key()).canonical_hash()?
            ),
            Self::Decision { case_id, .. } | Self::Appeal { case_id, .. } => {
                canonical_id(case_id, "report_")?;
                case_id.clone()
            }
        })
    }
    pub fn validate(&self) -> Result<()> {
        key(self.key())?;
        match self {
            Self::Report(r) => {
                canonical_id(r.object_id.as_str(), "obj_")?;
                text(&r.details)?;
            }
            Self::Appeal {
                case_id,
                request: r,
            } => {
                canonical_id(case_id, "report_")?;
                text(&r.details)?;
                revision(r.expected_revision)?;
            }
            Self::Decision {
                case_id,
                request: r,
            } => {
                canonical_id(case_id, "report_")?;
                text(&r.explanation)?;
                revision(r.expected_revision)?;
                if r.policy_version != POLICY
                    || r.source_signals.len() > 20
                    || r.source_signals.iter().collect::<BTreeSet<_>>().len()
                        != r.source_signals.len()
                {
                    return Err(Error::Canonical(
                        "invalid moderation policy or signals".into(),
                    ));
                }
                for signal in &r.source_signals {
                    canonical_id(signal.as_str(), "jud_")?;
                }
            }
        }
        Ok(())
    }
}
fn revision(n: u64) -> Result<()> {
    if n == 0 || n > MAX_SEQUENCE {
        Err(Error::Canonical("invalid moderation revision".into()))
    } else {
        Ok(())
    }
}

impl ModerationCase {
    pub fn restricted(&self) -> bool {
        self.decisions
            .last()
            .is_some_and(|d| d.outcome == ModerationOutcome::Restrict)
    }
    pub fn view(mut self, actor: &IdentityId, reviewer: bool) -> Result<Self> {
        let reporter = self.reporter_id.as_ref() == Some(actor);
        if !reviewer
            && !reporter
            && !(self.subject_author_id == *actor && !self.decisions.is_empty())
        {
            return Err(Error::NotFound("moderation case".into()));
        }
        if !reviewer && !reporter {
            self.reporter_id = None;
            self.reason = None;
            self.details = None;
        }
        if let Some(appeal) = &mut self.appeal {
            if !reviewer && appeal.appellant_id.as_ref() != Some(actor) {
                appeal.appellant_id = None;
                appeal.details = None;
            }
        }
        Ok(self)
    }
}

pub fn transition(
    previous: Option<ModerationCase>,
    actor: &IdentityId,
    reviewers: &BTreeSet<IdentityId>,
    intent: &ModerationIntent,
    subject: &IdentityId,
    sequence: u64,
    at: Timestamp,
) -> Result<ModerationCase> {
    intent.validate()?;
    canonical_id(actor.as_str(), "id_")?;
    canonical_id(subject.as_str(), "id_")?;
    let id = intent.case_id(actor)?;
    if let ModerationIntent::Report(r) = intent {
        if previous.is_some() {
            return Err(Error::Conflict("report already exists".into()));
        }
        revision(sequence)?;
        return Ok(ModerationCase {
            id,
            sequence,
            object_id: r.object_id.clone(),
            subject_author_id: subject.clone(),
            reporter_id: Some(actor.clone()),
            reason: Some(r.reason),
            details: Some(text(&r.details)?),
            created_at: at,
            updated_at: at,
            revision: 1,
            status: ModerationStatus::Pending,
            decisions: vec![],
            appeal: None,
        });
    }
    let mut case = previous.ok_or_else(|| Error::NotFound("moderation case".into()))?;
    if case.id != id || at < case.updated_at {
        return Err(Error::Conflict("moderation state or clock changed".into()));
    }
    let expected = match intent {
        ModerationIntent::Decision { request: r, .. } => {
            if !reviewers.contains(actor)
                || case.reporter_id.as_ref() == Some(actor)
                || case.subject_author_id == *actor
                || case
                    .appeal
                    .as_ref()
                    .is_some_and(|a| a.appellant_id.as_ref() == Some(actor))
                || case.decisions.iter().any(|d| d.reviewer_id == *actor)
            {
                return Err(Error::Conflict(
                    "independent authorized reviewer required".into(),
                ));
            }
            if !matches!(
                case.status,
                ModerationStatus::Pending | ModerationStatus::Appealed
            ) {
                return Err(Error::Conflict("case is not awaiting a decision".into()));
            }
            case.decisions.push(ModerationDecision {
                reviewer_id: actor.clone(),
                outcome: r.outcome,
                reason: r.reason,
                explanation: text(&r.explanation)?,
                policy_version: r.policy_version.clone(),
                source_signals: r.source_signals.clone(),
                created_at: at,
            });
            case.status = if case.appeal.is_some() {
                ModerationStatus::Closed
            } else {
                ModerationStatus::Decided
            };
            r.expected_revision
        }
        ModerationIntent::Appeal { request: r, .. } => {
            if case.status != ModerationStatus::Decided || case.appeal.is_some() {
                return Err(Error::Conflict("case cannot be appealed".into()));
            }
            let allowed = if case.restricted() {
                &case.subject_author_id == actor
            } else {
                case.reporter_id.as_ref() == Some(actor)
            };
            if !allowed {
                return Err(Error::Conflict(
                    "account cannot appeal this decision".into(),
                ));
            }
            case.appeal = Some(ModerationAppeal {
                appellant_id: Some(actor.clone()),
                details: Some(text(&r.details)?),
                created_at: at,
            });
            case.status = ModerationStatus::Appealed;
            r.expected_revision
        }
        _ => unreachable!(),
    };
    if case.revision != expected {
        return Err(Error::Conflict("moderation revision changed".into()));
    }
    case.revision += 1;
    case.updated_at = at;
    Ok(case)
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModerationReceiptPayload {
    pub actor: IdentityId,
    pub reviewers: BTreeSet<IdentityId>,
    pub intent: ModerationIntent,
    pub result: ModerationCase,
    pub sequence: u64,
    pub previous_id: Option<Hash>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModerationReceipt {
    pub payload: ModerationReceiptPayload,
    pub signature: babel_crypto::Signature,
}
impl ModerationReceipt {
    pub fn id(&self) -> Result<Hash> {
        ("babel.private.moderation.receipt.v1", &self.payload).canonical_hash()
    }
    pub fn sign(payload: ModerationReceiptPayload, key: &babel_crypto::Keypair) -> Result<Self> {
        let signature =
            key.sign(&("babel.private.moderation.receipt.v1", &payload).canonical_bytes()?);
        Ok(Self { payload, signature })
    }
    pub fn verify(&self, identity: &babel_identity::Identity) -> Result<()> {
        if identity.id != self.payload.actor {
            return Err(Error::Signature);
        }
        identity.public_key.verify(
            &("babel.private.moderation.receipt.v1", &self.payload).canonical_bytes()?,
            &self.signature,
        )
    }
}

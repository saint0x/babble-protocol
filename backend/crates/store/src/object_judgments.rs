use super::*;
use babel_judgment::{
    DefinitionId, JudgmentRegistry, JudgmentRequest, ProviderVersion,
    validate_source_agreement_result,
};
use schemars::JsonSchema;

/// The actual provider-scoped request that produced an Object's Judgment.
/// Immutable by judgment ID; evaluation metadata may be refreshed separately.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObjectJudgmentInput {
    pub object_id: ObjectId,
    pub judgment_id: JudgmentId,
    pub request: JudgmentRequest,
}

impl ObjectJudgmentInput {
    pub(crate) fn validate(&self) -> Result<()> {
        self.object_id.validate()?;
        self.judgment_id.validate()?;
        if self.request.state.subject != self.object_id.as_str() {
            return Err(CoreError::Conflict(
                "object judgment subject mismatch".into(),
            ));
        }
        JudgmentRegistry::babel_core().validate_request(&self.request)?;
        Ok(())
    }

    pub(crate) fn validate_judgment(&self, judgment: &Judgment) -> Result<()> {
        self.validate()?;
        if self.judgment_id != judgment.id
            || self.request.definition != judgment.definition
            || self.request.state.canonical_hash()? != judgment.input_hash
        {
            return Err(CoreError::Conflict(
                "object judgment binding mismatch".into(),
            ));
        }
        if !judgment.confidence.is_finite() || !(0.0..=1.0).contains(&judgment.confidence) {
            return Err(CoreError::Conflict(
                "invalid object judgment confidence".into(),
            ));
        }
        JudgmentRegistry::babel_core().validate_output(&judgment.definition, &judgment.output)?;
        if self.request.definition == DefinitionId::source_agreement_v1() {
            if judgment.confidence != 0.0 {
                return Err(CoreError::Conflict(
                    "source agreement confidence must remain uncalibrated".into(),
                ));
            }
            validate_source_agreement_result(&self.request, &judgment.output)?;
        }
        let commitment = (
            &judgment.definition,
            &judgment.provider,
            &judgment.input_hash,
            &self.request.parameters,
            &judgment.output,
        );
        if JudgmentId::from_hash(&commitment.canonical_hash()?) != judgment.id {
            return Err(CoreError::Conflict(
                "object judgment commitment mismatch".into(),
            ));
        }
        Ok(())
    }
}

impl FileStore {
    pub fn get_object_judgment_input(
        &self,
        id: &JudgmentId,
    ) -> Result<Option<ObjectJudgmentInput>> {
        let _guard = self.publication_guard()?;
        self.object_judgment_input_unlocked(id)
    }

    /// Return validated associations in ascending judgment-ID order.
    pub fn object_judgment_inputs(&self, object_id: &ObjectId) -> Result<Vec<ObjectJudgmentInput>> {
        let _guard = self.publication_guard()?;
        object_id.validate()?;
        self.indexed_object_judgment_inputs(object_id)
    }

    /// Latest validated pair for this exact definition and provider version with
    /// `created_at <= reference`, breaking timestamp ties by greatest Judgment ID.
    /// Reads at most one canonical association and its Judgment.
    pub fn latest_object_judgment_input(
        &self,
        object_id: &ObjectId,
        definition: &DefinitionId,
        provider: &ProviderVersion,
        reference: Timestamp,
    ) -> Result<Option<(ObjectJudgmentInput, Judgment)>> {
        let _guard = self.publication_guard()?;
        object_id.validate()?;
        self.latest_indexed_object_judgment_input(object_id, definition, provider, reference)
    }

    // Callers hold the publication lock throughout association/counterpart reads.
    pub(crate) fn object_judgment_input_unlocked(
        &self,
        id: &JudgmentId,
    ) -> Result<Option<ObjectJudgmentInput>> {
        Ok(self
            .object_judgment_pair_unlocked(id)?
            .map(|(input, _)| input))
    }

    pub(crate) fn object_judgment_pair_unlocked(
        &self,
        id: &JudgmentId,
    ) -> Result<Option<(ObjectJudgmentInput, Judgment)>> {
        id.validate()?;
        let Some(value) =
            publication::read_value(&self.path("object_judgment_inputs", id.as_str())?)?
        else {
            return Ok(None);
        };
        let input: ObjectJudgmentInput = serde_json::from_value(value)
            .map_err(|err| CoreError::Canonical(format!("decode object judgment input: {err}")))?;
        if &input.judgment_id != id {
            return Err(CoreError::Conflict(
                "object judgment input ID mismatch".into(),
            ));
        }
        let value = publication::read_value(&self.path("judgments", id.as_str())?)?
            .ok_or_else(|| CoreError::Conflict("object judgment counterpart missing".into()))?;
        let judgment: Judgment = serde_json::from_value(value)
            .map_err(|err| CoreError::Canonical(format!("decode object judgment: {err}")))?;
        input.validate_judgment(&judgment)?;
        Ok(Some((input, judgment)))
    }
}

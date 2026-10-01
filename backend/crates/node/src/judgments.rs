use crate::{LocalNode, ingestion_definitions, object_judgment_state};
use babble_judgment::{
    CacheKey, DefinitionId, Judgment, JudgmentCache, JudgmentOrchestrator, JudgmentProvider,
    JudgmentRequest, OrchestratedJudgment, cache_key,
};
use babble_object::Object;
use babble_types::{Error, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;

#[cfg(test)]
mod tests;

pub(crate) struct PreparedJudgment {
    key: CacheKey,
    deadline: Option<Instant>,
    pub(crate) input: Option<babble_store::ObjectJudgmentInput>,
    pub(crate) orchestration: OrchestratedJudgment,
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn prepare_object_judgments(
        &self,
        object: &Object,
    ) -> Result<Vec<PreparedJudgment>> {
        ingestion_definitions()
            .into_iter()
            .map(|definition| self.prepare_object_judgment(object, definition, BTreeMap::new()))
            .collect()
    }

    pub(crate) fn prepare_object_judgment(
        &self,
        object: &Object,
        definition: DefinitionId,
        parameters: BTreeMap<String, Value>,
    ) -> Result<PreparedJudgment> {
        let deadline = (definition == DefinitionId::source_agreement_v1())
            .then(|| Instant::now() + crate::agreement::EVALUATION_BUDGET);
        let state = if definition == DefinitionId::source_agreement_v1() {
            if !parameters.is_empty() {
                return Err(Error::Conflict(
                    "source agreement parameters are host-owned".into(),
                ));
            }
            if !self
                .judgment_provider
                .supported_definitions()
                .contains(&definition)
            {
                return Err(Error::ProviderUnavailable(
                    "source agreement provider is unavailable".into(),
                ));
            }
            self.source_agreement_state_before(
                object,
                babble_types::Timestamp::now(),
                deadline.unwrap(),
            )?
        } else {
            object_judgment_state(object)
        };
        let request = JudgmentRequest {
            definition,
            state,
            parameters,
        };
        self.prepare_judgment_request_before(object, request, deadline)
    }

    pub(crate) fn prepare_judgment_request_before(
        &self,
        object: &Object,
        request: JudgmentRequest,
        deadline: Option<Instant>,
    ) -> Result<PreparedJudgment> {
        check_deadline(deadline)?;
        let provider = self.judgment_provider.version();
        let (scoped, _) = self.judgment_provider.privacy_policy().apply(&request)?;
        let key = cache_key(&provider, &scoped)?;
        check_deadline(deadline)?;

        // Stage only this request's cache entry. Failed batches must not insert entries
        // or increment hit counters in the live cache.
        let mut staged_cache = JudgmentCache::default();
        if let Some(judgment) = self.judgment_cache.get(&key) {
            staged_cache.insert(key.clone(), judgment.clone());
        } else if request.definition == DefinitionId::source_agreement_v1() {
            if let Some((input, judgment)) = self.store.latest_object_judgment_input(
                &object.id,
                &request.definition,
                &provider,
                babble_types::Timestamp::now(),
            )? && input.request == scoped
            {
                staged_cache.insert(key.clone(), judgment);
            }
        }
        check_deadline(deadline)?;
        let orchestrator = JudgmentOrchestrator::single(&self.judgment_provider);
        let orchestration = match deadline {
            Some(deadline) => {
                orchestrator.evaluate_before(&mut staged_cache, &request, deadline)?
            }
            None => orchestrator.evaluate(&mut staged_cache, &request)?,
        };
        let judgment = &orchestration.judgment;
        if request.definition == DefinitionId::source_agreement_v1() {
            babble_judgment::validate_source_agreement_result(&scoped, &judgment.output)?;
        }
        judgment.id.validate()?;
        if judgment.definition != request.definition
            || judgment.provider != provider
            || judgment.input_hash != key.input_hash
            || !judgment.confidence.is_finite()
            || !(0.0..=1.0).contains(&judgment.confidence)
        {
            return Err(Error::Conflict(format!(
                "invalid provider Judgment binding or confidence for {}",
                request.definition.as_str()
            )));
        }
        let input = (scoped.state.subject == object.id.as_str()).then(|| {
            babble_store::ObjectJudgmentInput {
                object_id: object.id.clone(),
                judgment_id: judgment.id.clone(),
                request: scoped,
            }
        });
        check_deadline(deadline)?;
        Ok(PreparedJudgment {
            key,
            deadline,
            input,
            orchestration,
        })
    }

    pub(crate) fn commit_object_judgments(
        &mut self,
        prepared: Vec<PreparedJudgment>,
    ) -> Result<Vec<Judgment>> {
        let mut batch = babble_store::PublicationBatch::new();
        for entry in &prepared {
            check_deadline(entry.deadline)?;
            if let Some(input) = &entry.input {
                batch.object_judgment(input, &entry.orchestration.judgment)?;
            } else {
                batch.judgment(&entry.orchestration.judgment)?;
            }
        }
        for entry in &prepared {
            check_deadline(entry.deadline)?;
        }
        self.store
            .commit_publication(batch)
            .map_err(crate::publication::publication_error)?;
        Ok(self.cache_object_judgments(prepared))
    }

    pub(crate) fn cache_object_judgments(
        &mut self,
        prepared: Vec<PreparedJudgment>,
    ) -> Vec<Judgment> {
        prepared
            .into_iter()
            .map(|entry| {
                if entry.orchestration.cache_hit && self.judgment_cache.get(&entry.key).is_some() {
                    self.judgment_cache.record_hit(&entry.key);
                } else {
                    self.judgment_cache
                        .insert(entry.key, entry.orchestration.judgment.clone());
                }
                entry.orchestration.judgment
            })
            .collect()
    }
}

pub(crate) fn check_deadline(deadline: Option<Instant>) -> Result<()> {
    if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
        return Err(Error::ProviderUnavailable(
            "judgment evaluation work budget exceeded".into(),
        ));
    }
    Ok(())
}

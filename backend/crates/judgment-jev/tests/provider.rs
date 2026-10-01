use babble_judgment::{
    DefinitionId, JudgmentCache, JudgmentOrchestrator, JudgmentProvider, JudgmentRequest,
    JudgmentState, ProviderRole,
};
use babble_judgment_jev::{JevConfig, JevProvider, JevRequest, JevResponse, JevTransport};
use babble_types::Result;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct ScriptedTransport {
    seen: Arc<Mutex<Vec<JevRequest>>>,
}

#[test]
fn jev_provider_uses_remote_privacy_when_orchestrated() {
    let transport = ScriptedTransport::default();
    let seen = transport.seen.clone();
    let provider = JevProvider::with_transport(
        JevConfig::new("https://jev.example.test/judge", "jev-default", "1"),
        transport,
    );
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_sensitive".to_string(),
            context: BTreeMap::from([
                ("object".to_string(), serde_json::json!({"private": true})),
                (
                    "text".to_string(),
                    serde_json::json!("according to source-backed evidence"),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    };

    let result = JudgmentOrchestrator::single(&provider)
        .evaluate(&mut JudgmentCache::default(), &request)
        .unwrap();

    assert_eq!(result.decisions[0].role, ProviderRole::Remote);
    assert!(
        result.decisions[0]
            .privacy
            .redacted_context_keys
            .contains(&"object".to_string())
    );
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_ne!(seen[0].state.subject, "obj_sensitive");
    assert!(!seen[0].state.context.contains_key("object"));
    assert!(seen[0].state.context.contains_key("text"));
}

impl JevTransport for ScriptedTransport {
    fn evaluate(&self, _config: &JevConfig, request: &JevRequest) -> Result<JevResponse> {
        self.seen.lock().unwrap().push(request.clone());
        Ok(JevResponse {
            output: serde_json::json!({
                "kind": "bounded_score",
                "score": 0.82,
                "label": "evidence_quality"
            }),
            confidence: 0.91,
            model: Some("jev-scripted".to_string()),
            model_version: Some("2026-09-27".to_string()),
        })
    }
}

#[test]
fn jev_provider_maps_babble_request_without_leaking_jev_types_to_callers() {
    let transport = ScriptedTransport::default();
    let seen = transport.seen.clone();
    let provider = JevProvider::with_transport(
        JevConfig::new("https://jev.example.test/judge", "jev-default", "1"),
        transport,
    );
    let mut context = BTreeMap::new();
    context.insert(
        "text".to_string(),
        serde_json::json!("source-backed evidence"),
    );
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_123".to_string(),
            context,
        },
        parameters: BTreeMap::new(),
    };

    let judgment = provider.judge(&request).unwrap();

    assert_eq!(judgment.provider.provider, "jev");
    assert_eq!(judgment.provider.model, "jev-scripted");
    assert_eq!(judgment.confidence, 0.91);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0].definition,
        DefinitionId::evidence_quality_v1().as_str()
    );
    assert!(!seen[0].input_hash.is_empty());
}

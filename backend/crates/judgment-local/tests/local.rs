use babble_judgment::{DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState};
use babble_judgment_local::LocalProvider;
use std::collections::BTreeMap;

#[test]
fn local_provider_scores_evidence_quality_without_remote_model() {
    let provider = LocalProvider::default();
    let mut context = BTreeMap::new();
    context.insert(
        "text".to_string(),
        serde_json::json!("According to the dataset and published methodology, the result reproduced across studies."),
    );
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_evidence".to_string(),
            context,
        },
        parameters: BTreeMap::new(),
    };

    let judgment = provider.judge(&request).unwrap();

    assert_eq!(judgment.provider.provider, "babble-local");
    assert!(judgment.output["score"].as_f64().unwrap() > 0.5);
    assert!(judgment.confidence > 0.0);
}

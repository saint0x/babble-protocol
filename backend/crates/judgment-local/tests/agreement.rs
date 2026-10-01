use babel_judgment::{DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState};
use babel_judgment_local::LocalProvider;
use babel_types::Error;
use serde_json::json;
use std::collections::BTreeMap;

#[test]
fn local_provider_does_not_advertise_or_fabricate_source_agreement() {
    let provider = LocalProvider::default();
    assert_eq!(provider.supported_definitions().len(), 6);
    assert!(
        !provider
            .supported_definitions()
            .contains(&DefinitionId::source_agreement_v1())
    );
    let request = JudgmentRequest {
        definition: DefinitionId::source_agreement_v1(),
        state: JudgmentState {
            subject: "obj_test".into(),
            context: BTreeMap::from([
                ("text".into(), json!("Subject text")),
                (
                    "source_agreement".into(),
                    json!({
                        "reference_time": 200.0, "previous_score": null, "sources": []
                    }),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    };
    assert!(matches!(
        provider.judge(&request),
        Err(Error::ProviderUnavailable(_))
    ));
}

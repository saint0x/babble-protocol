use babel_judgment_python::contract;
use serde_json::json;

#[test]
fn fixture_schemas_are_current() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/algorithms/v1");
    let checked: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("schema-bundle.json")).unwrap()).unwrap();
    assert_json_eq(&checked, &contract::schemas(), "");
    let fixtures: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("fixtures.json")).unwrap()).unwrap();
    for response in fixtures["responses"].as_array().unwrap() {
        contract::decode(&serde_json::to_vec(response).unwrap()).unwrap();
    }
    for request in fixtures["requests"].as_array().unwrap() {
        let parsed: contract::Request = serde_json::from_value(request.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), *request);
    }
}

fn assert_json_eq(actual: &serde_json::Value, expected: &serde_json::Value, path: &str) {
    match (actual, expected) {
        (serde_json::Value::Object(actual), serde_json::Value::Object(expected)) => {
            assert_eq!(actual.len(), expected.len(), "schema keys at {path}");
            for (key, expected) in expected {
                assert_json_eq(&actual[key], expected, &format!("{path}/{key}"));
            }
        }
        (serde_json::Value::Array(actual), serde_json::Value::Array(expected)) => {
            assert_eq!(actual.len(), expected.len(), "schema items at {path}");
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_json_eq(actual, expected, &format!("{path}/{index}"));
            }
        }
        _ => assert_eq!(
            actual, expected,
            "schema drift at {path}; regenerate with example export"
        ),
    }
}

#[test]
fn strict_json_and_envelopes() {
    assert!(
        serde_json::from_value::<contract::Request>(
            json!({"protocol":"babel.algorithms.v1","id":1,"method":"future"})
        )
        .is_err()
    );
    for bytes in [
        br#"{"protocol":"babel.algorithms.v1","id":1,"result":null,"error":null}"#.as_slice(),
        br#"{"protocol":"babel.algorithms.v1","id":1,"id":2,"result":null,"error":{"code":"algorithm_failure","message":"failed"}}"#,
        br#"{"protocol":"babel.algorithms.v1","id":1,"error":{"code":"algorithm_failure","message":"failed"}}"#,
    ] { assert!(contract::decode(bytes).is_err()); }
    for id in [
        json!(0),
        json!(-1),
        json!(9007199254740992_u64),
        json!(1.5),
        json!(true),
    ] {
        let response = json!({"protocol":"babel.algorithms.v1","id":id,"result":null,"error":{"code":"algorithm_failure","message":"failed"}});
        assert!(contract::decode(&serde_json::to_vec(&response).unwrap()).is_err());
    }
}

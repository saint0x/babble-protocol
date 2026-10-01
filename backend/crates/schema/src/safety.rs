use babel_crypto::Keypair;
use babel_graph::{
    SafetyAction, SafetyActionPayload, SafetyEntry, SafetyReceipt, SafetyReceiptPayload,
    SafetyRequest, SafetySnapshot, SafetyState,
};
use babel_identity::{Identity, IdentityKind};
use babel_types::{Canonical, IdentityId, Timestamp};
use serde_json::{Value, json};

pub(crate) fn fixtures() -> babel_types::Result<Value> {
    // Fixed public test identities keep schema exports byte-for-byte reproducible.
    let key = Keypair::from_ed25519_secret_hex(&"31".repeat(32))?;
    let created_at: Timestamp = serde_json::from_value(json!("2026-09-30T00:00:00Z"))
        .map_err(|e| babel_types::Error::Canonical(e.to_string()))?;
    let identity = |handle: &str| -> babel_types::Result<Identity> {
        let commitment = json!({"kind":"Person","handle":handle,"public_key":key.public_key(),"created_at":created_at});
        let result = Identity {
            id: IdentityId::from_hash(&commitment.canonical_hash()?),
            kind: IdentityKind::Person,
            handle: handle.into(),
            public_key: key.public_key(),
            created_at,
            signature: key.sign(&commitment.canonical_bytes()?),
        };
        result.verify()?;
        Ok(result)
    };
    let author = identity("safety-owner")?;
    let target = identity("safety-target")?;
    let absent = SafetyState::absent(&author.id, &target.id);
    let state = SafetyState {
        blocked: true,
        muted: true,
        revision: 1,
        ..absent.clone()
    };
    let request = SafetyRequest {
        author_id: author.id.clone(),
        target_id: target.id.clone(),
        blocked: true,
        muted: true,
        expected_revision: 0,
        idempotency_key: "safety-fixture-1".into(),
    };
    let action = SafetyAction::sign(
        SafetyActionPayload {
            state: state.clone(),
            previous_id: None,
            created_at,
            request_id: request.canonical_hash()?,
        },
        &key,
    )?;
    let receipt = SafetyReceipt::sign(
        SafetyReceiptPayload {
            request,
            state: state.clone(),
            created_at,
            sequence: 1,
            previous_id: None,
        },
        &key,
    )?;
    action.verify(&author)?;
    receipt.verify(&author)?;
    let snapshot = SafetySnapshot {
        author_id: author.id.clone(),
        revision: 1,
        entries: vec![SafetyEntry {
            identity: target,
            state: state.clone(),
        }],
    };
    Ok(
        json!({"author":author,"absent":absent,"state":state,"snapshot":snapshot,
        "set_request":{"blocked":true,"muted":true,"expected_revision":0,"idempotency_key":"safety-fixture-1"},
        "action":action,"receipt":receipt,"state_hash":state.canonical_hash()?}),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn safety_fixtures_and_schema_are_exact_and_reproducible() {
        assert_eq!(super::fixtures().unwrap(), super::fixtures().unwrap());
        let schemas = crate::protocol_schemas().unwrap();
        for (name, fields) in [
            (
                "node.SafetyState",
                vec!["author_id", "target_id", "blocked", "muted", "revision"],
            ),
            (
                "node.SafetySnapshot",
                vec!["author_id", "revision", "entries"],
            ),
            (
                "api.SetSafetyRequest",
                vec!["blocked", "muted", "expected_revision", "idempotency_key"],
            ),
        ] {
            let schema = &schemas[name];
            assert_eq!(schema["additionalProperties"], false);
            assert_eq!(schema["required"].as_array().unwrap().len(), fields.len());
            for field in fields {
                assert!(
                    schema["required"]
                        .as_array()
                        .unwrap()
                        .contains(&serde_json::json!(field))
                );
            }
        }
        assert_eq!(
            schemas["node.SafetySnapshot"]["properties"]["entries"]["maxItems"],
            1000
        );
    }
}

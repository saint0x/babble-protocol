use babel_crypto::Keypair;
use babel_graph::{
    SafetyAction, SafetyActionPayload, SafetyReceipt, SafetyReceiptPayload, SafetyRequest,
    SafetyState,
};
use babel_identity::{Identity, IdentityKind};
use babel_types::{Hash, Timestamp};

#[test]
fn safety_signed_action_commits_every_field_and_signer() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &Keypair::generate()).unwrap();
    let action = SafetyAction::sign(
        SafetyActionPayload {
            state: SafetyState {
                author_id: author.id.clone(),
                target_id: other.id.clone(),
                blocked: true,
                muted: false,
                revision: 1,
            },
            previous_id: None,
            created_at: Timestamp::now(),
            request_id: Hash::from_bytes(b"request"),
        },
        &key,
    )
    .unwrap();
    action.verify(&author).unwrap();
    assert!(action.verify(&other).is_err());
    let mut tampered = action.clone();
    tampered.payload.state.blocked = false;
    assert!(tampered.verify(&author).is_err());
    let mut tampered = action.clone();
    tampered.payload.state.revision = 2;
    assert!(tampered.verify(&author).is_err());
    let mut tampered = action;
    tampered.payload.state.target_id = author.id.clone();
    assert!(tampered.verify(&author).is_err());
}

#[test]
fn safety_receipt_binds_key_intent_and_original_result() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &Keypair::generate()).unwrap();
    let request = SafetyRequest {
        author_id: author.id.clone(),
        target_id: other.id.clone(),
        blocked: false,
        muted: false,
        expected_revision: 0,
        idempotency_key: "request-1".into(),
    };
    let receipt = SafetyReceipt::sign(
        SafetyReceiptPayload {
            state: SafetyState::absent(&author.id, &other.id),
            request,
            created_at: Timestamp::now(),
            sequence: 1,
            previous_id: None,
        },
        &key,
    )
    .unwrap();
    receipt.verify(&author).unwrap();
    let mut tampered = receipt;
    tampered.payload.request.idempotency_key = "request-2".into();
    assert!(tampered.verify(&author).is_err());
}

#[test]
fn safety_domain_rejects_self_safety_unbounded_keys_and_unknown_fields() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &key).unwrap();
    let mut request = SafetyRequest {
        author_id: author.id.clone(),
        target_id: other.id,
        blocked: true,
        muted: false,
        expected_revision: 0,
        idempotency_key: "k".repeat(256),
    };
    request.validate().unwrap();
    request.idempotency_key.push('k');
    assert!(request.validate().is_err());
    request.idempotency_key = "ok".into();
    request.target_id = author.id;
    assert!(request.validate().is_err());
    let mut json = serde_json::to_value(&request).unwrap();
    json["extra"] = true.into();
    assert!(serde_json::from_value::<SafetyRequest>(json).is_err());
}

use babel_crypto::Keypair;
use babel_graph::{
    FollowAction, FollowActionPayload, FollowReceipt, FollowReceiptPayload, FollowRequest,
    FollowState,
};
use babel_identity::{Identity, IdentityKind};
use babel_types::{Hash, Timestamp};

#[test]
fn following_signed_action_commits_every_field_and_signer() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &Keypair::generate()).unwrap();
    let action = FollowAction::sign(
        FollowActionPayload {
            state: FollowState {
                author_id: author.id.clone(),
                target_id: other.id.clone(),
                following: true,
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
    tampered.payload.state.following = false;
    assert!(tampered.verify(&author).is_err());
    let mut tampered = action.clone();
    tampered.payload.state.revision = 2;
    assert!(tampered.verify(&author).is_err());
    let mut tampered = action;
    tampered.payload.state.target_id = author.id.clone();
    assert!(tampered.verify(&author).is_err());
}

#[test]
fn following_receipt_binds_key_intent_and_original_result() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &Keypair::generate()).unwrap();
    let request = FollowRequest {
        author_id: author.id.clone(),
        target_id: other.id.clone(),
        following: false,
        expected_revision: 0,
        idempotency_key: "request-1".into(),
    };
    let receipt = FollowReceipt::sign(
        FollowReceiptPayload {
            state: FollowState::absent(&author.id, &other.id),
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
fn following_domain_rejects_self_follow_unbounded_keys_and_unknown_fields() {
    let key = Keypair::generate();
    let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
    let other = Identity::create(IdentityKind::Person, "other", &key).unwrap();
    let mut request = FollowRequest {
        author_id: author.id.clone(),
        target_id: other.id,
        following: true,
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
    assert!(serde_json::from_value::<FollowRequest>(json).is_err());
}

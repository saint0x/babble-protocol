use super::*;
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use serde_json::{Value, json};

#[test]
fn security_schema_preserves_required_nullable_creation_time_and_bounds() {
    let schema = serde_json::to_value(schemars::schema_for!(AccountSessionInfo)).unwrap();
    assert_eq!(
        schema["properties"]["id"]["pattern"],
        "^account_[a-f0-9]{64}$"
    );
    assert!(
        schema["required"]
            .as_array()
            .unwrap()
            .contains(&json!("created_at"))
    );
    assert!(
        schema["properties"]["created_at"]["type"]
            .as_array()
            .unwrap()
            .contains(&json!("null"))
    );
    assert_eq!(schema["properties"]["created_at"]["format"], "date-time");
    assert_eq!(schema["properties"]["expires_at"]["format"], "date-time");
    let schema = serde_json::to_value(schemars::schema_for!(AccountSessionsResponse)).unwrap();
    assert_eq!(schema["properties"]["sessions"]["maxItems"], 16);
    assert!(
        serde_json::from_value::<AccountSessionInfo>(
            json!({"id":"account", "expires_at":"date", "current":true})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<AccountSessionInfo>(
            json!({"id":"account", "created_at":null, "expires_at":"date", "current":true})
        )
        .is_ok()
    );
    assert!(
        serde_json::from_value::<ChangePasswordRequest>(
            json!({"current_password":"a","new_password":"b","identity_id":"spoof"})
        )
        .is_err()
    );
}

#[test]
fn security_password_policy_counts_unicode_scalars_and_utf8_bytes_without_normalization() {
    assert!(password_policy(&"a".repeat(14)).is_err());
    assert!(password_policy(&"a".repeat(15)).is_ok());
    assert!(password_policy(&"\u{1f9ed}".repeat(14)).is_err());
    assert!(password_policy(&"\u{1f9ed}".repeat(15)).is_ok());
    assert!(password_policy(&"\u{1f9ed}".repeat(256)).is_ok());
    assert!(password_policy(&"\u{1f9ed}".repeat(257)).is_err());
    assert!(password_policy(&"a".repeat(1024)).is_ok());
    assert!(password_policy(&"a".repeat(1025)).is_err());
    assert!(password_policy(&" ".repeat(15)).is_ok());
    assert!(password_policy(&"e\u{301}".repeat(8)).is_ok());
}

#[tokio::test]
async fn security_legacy_password_and_new_password_are_verified_exactly() {
    let root = std::env::temp_dir().join(format!(
        "babel-security-password-{}",
        random_token().unwrap()
    ));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let identity = node
        .create_identity(IdentityKind::Person, "legacy-login")
        .unwrap();
    let state = ApiState::new(node);
    let legacy = " old-pass-12 ";
    let salt = SaltString::generate(&mut OsRng);
    let hash = password_engine()
        .hash_password(legacy.as_bytes(), &salt)
        .unwrap()
        .to_string();
    state
        .auth
        .with_store(|store| store.create_account(identity.id.as_str(), &hash))
        .unwrap();
    let signed_in = login(
        State(state.clone()),
        Json(LoginRequest {
            identity_id: identity.id.to_string(),
            password: legacy.into(),
        }),
    )
    .await
    .unwrap()
    .0;
    assert!(
        login(
            State(state.clone()),
            Json(LoginRequest {
                identity_id: identity.id.to_string(),
                password: legacy.trim().into()
            })
        )
        .await
        .is_err()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        header::AUTHORIZATION,
        format!("Bearer {}", signed_in.token).parse().unwrap(),
    );
    let new = " new-password-with-space ";
    security::change_password(
        State(state.clone()),
        headers.clone(),
        Json(ChangePasswordRequest {
            current_password: legacy.into(),
            new_password: new.into(),
        }),
    )
    .await
    .unwrap();
    assert!(state.auth.principal(&headers).is_err());
    assert!(
        login(
            State(state.clone()),
            Json(LoginRequest {
                identity_id: identity.id.to_string(),
                password: legacy.into()
            })
        )
        .await
        .is_err()
    );
    assert!(
        login(
            State(state.clone()),
            Json(LoginRequest {
                identity_id: identity.id.to_string(),
                password: new.trim().into()
            })
        )
        .await
        .is_err()
    );
    assert!(
        login(
            State(state.clone()),
            Json(LoginRequest {
                identity_id: identity.id.to_string(),
                password: new.into()
            })
        )
        .await
        .is_ok()
    );
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn security_revocation_invalidates_previously_admitted_execution() {
    let root = std::env::temp_dir().join(format!(
        "babel-security-admission-{}",
        random_token().unwrap()
    ));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let identity = node
        .create_identity(IdentityKind::Person, "admitted")
        .unwrap();
    let state = ApiState::new(node);
    let (actor, peer) = state
        .auth
        .with_store(|store| {
            store.create_account(identity.id.as_str(), "hash")?;
            let token = store.issue_verified(identity.id.as_str(), "hash")?.0;
            let peer = store.issue_verified(identity.id.as_str(), "hash")?.0;
            Ok((store.authenticate(&token)?, store.authenticate(&peer)?))
        })
        .unwrap();
    let admitted = surface::ExecutionAuthorization {
        principal: actor.clone(),
        surface: None,
    };
    {
        let _node = lock_node(&state).unwrap();
        state
            .auth
            .with_store(|store| store.revoke_sessions(&peer, None))
            .unwrap();
    }
    surface::EXECUTION
        .scope(admitted, async {
            assert!(lock_node(&state).is_err());
        })
        .await;
    let admitted = surface::ExecutionAuthorization {
        principal: peer.clone(),
        surface: None,
    };
    {
        let _node = lock_node(&state).unwrap();
        state
            .auth
            .with_store(|store| store.change_password(&peer, "hash", Some("new-hash")))
            .unwrap();
    }
    surface::EXECUTION
        .scope(admitted, async {
            assert!(lock_node(&state).is_err());
        })
        .await;
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn security_account_methods_are_not_rpc_or_surface_operations() {
    let catalog = babel_rpc::babel_rpc_catalog().unwrap();
    for method in catalog.methods {
        let name = method.method.as_str();
        assert!(!name.starts_with("babel.auth."));
        assert!(!name.contains("password"));
    }
    let response = AccountSessionsResponse {
        sessions: vec![AccountSessionInfo {
            id: format!("account_{}", "a".repeat(64)),
            created_at: None,
            expires_at: "2026-10-01T00:00:00Z".into(),
            current: true,
        }],
    };
    let value: Value = serde_json::to_value(response).unwrap();
    let fields = value["sessions"][0].as_object().unwrap();
    assert_eq!(fields.len(), 4);
    for key in ["id", "created_at", "expires_at", "current"] {
        assert!(fields.contains_key(key));
    }
}

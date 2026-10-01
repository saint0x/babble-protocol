use super::*;

#[tokio::test]
async fn security_revoke_others_rejects_bodies_without_changing_sessions() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let peer = login(&app, &account, PASSWORD).await;
    let before = sessions(&app, &account).await;
    for body in [
        "{}".to_owned(),
        "null".to_owned(),
        json!({"author_id":account.id}).to_string(),
        "{".to_owned(),
    ] {
        assert_eq!(
            raw(
                &app,
                "POST",
                "/auth/sessions/revoke-others",
                Some(&account.token),
                Some(&body)
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            raw(
                &app,
                "POST",
                "/auth/sessions/revoke-others",
                None,
                Some(&body)
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(sessions(&app, &account).await, before);
    status(&app, &peer, StatusCode::OK).await;
}

#[tokio::test]
async fn security_every_management_route_requires_authentication() {
    let fixture = Fixture::new();
    let app = fixture.app();
    for token in [None, Some("forged")] {
        for (method, uri, body) in [
            ("GET", "/auth/sessions".to_owned(), Value::Null),
            (
                "DELETE",
                format!("/auth/sessions/account_{}", "0".repeat(64)),
                Value::Null,
            ),
            (
                "POST",
                "/auth/sessions/revoke-others".to_owned(),
                Value::Null,
            ),
            (
                "POST",
                "/auth/password".to_owned(),
                json!({"current_password":PASSWORD,"new_password":REPLACEMENT}),
            ),
        ] {
            assert_eq!(
                request(&app, method, &uri, token, body).await.0,
                StatusCode::UNAUTHORIZED,
                "{method} {uri}"
            );
        }
    }
}

#[tokio::test]
async fn security_password_body_is_strict_and_failures_do_not_revoke_or_leak_secrets() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let before = sessions(&app, &account).await;
    for body in [
        json!({}),
        json!({"current_password":PASSWORD}),
        json!({"new_password":REPLACEMENT}),
        json!({"current_password":null,"new_password":REPLACEMENT}),
        json!({"current_password":PASSWORD,"new_password":12}),
        json!({"current_password":PASSWORD,"new_password":REPLACEMENT,"identity_id":account.id}),
        json!({"current_password":PASSWORD,"new_password":REPLACEMENT,"author_id":"someone-else"}),
        json!({"current_password":PASSWORD,"new_password":REPLACEMENT,"token":account.token}),
    ] {
        let result = request(&app, "POST", "/auth/password", Some(&account.token), body).await;
        assert!(
            [StatusCode::BAD_REQUEST, StatusCode::UNPROCESSABLE_ENTITY].contains(&result.0),
            "{result:?}"
        );
        let response = result.1.to_string();
        for secret in [PASSWORD, REPLACEMENT, &account.token] {
            assert!(!response.contains(secret));
        }
    }
    let malformed = raw(
        &app,
        "POST",
        "/auth/password",
        Some(&account.token),
        Some("{\"current_password\":"),
    )
    .await;
    assert_eq!(malformed.0, StatusCode::BAD_REQUEST);
    let duplicate = format!(
        "{{\"current_password\":\"{PASSWORD}\",\"current_password\":\"{PASSWORD}\",\"new_password\":\"{REPLACEMENT}\"}}"
    );
    assert!(
        [StatusCode::BAD_REQUEST, StatusCode::UNPROCESSABLE_ENTITY].contains(
            &raw(
                &app,
                "POST",
                "/auth/password",
                Some(&account.token),
                Some(&duplicate)
            )
            .await
            .0
        )
    );
    for new in [
        PASSWORD.to_owned(),
        "a".repeat(14),
        "\u{00e9}".repeat(14),
        "a".repeat(1025),
        "\u{1f642}".repeat(257),
    ] {
        let result = change(&app, &account, PASSWORD, &new).await;
        assert_eq!(result.0, StatusCode::BAD_REQUEST, "{result:?}");
        assert!(!result.1.to_string().contains(PASSWORD));
    }
    assert_eq!(sessions(&app, &account).await, before);
    login(&app, &account, PASSWORD).await;
}

#[tokio::test]
async fn security_registration_and_change_count_scalars_not_bytes_without_composition_rules() {
    let fixture = Fixture::new();
    let app = fixture.app();
    for password in [
        "a".repeat(14),
        "\u{00e9}".repeat(14),
        "\u{1f642}".repeat(257),
        "a".repeat(1025),
    ] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/auth/register",
                None,
                json!({"kind":"Person","handle":"invalid","password":password})
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    // Fifteen identical scalars and the 1024-byte boundary are valid; entropy
    // composition heuristics and byte-based minimums are not this policy.
    for password in [
        "a".repeat(15),
        " ".repeat(15),
        "\u{00e9}".repeat(15),
        "\u{1f642}".repeat(256),
    ] {
        let account = register_with(&app, &password).await;
        login(&app, &account, &password).await;
        assert_eq!(
            change(&app, &account, &password, REPLACEMENT).await.0,
            StatusCode::NO_CONTENT
        );
        let fresh = login(&app, &account, REPLACEMENT).await;
        assert_eq!(
            change(&app, &fresh, REPLACEMENT, &password).await.0,
            StatusCode::NO_CONTENT
        );
        login(&app, &account, &password).await;
    }
}

#[tokio::test]
async fn security_passwords_are_not_normalized_or_trimmed() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let composed = "\u{00e9}".repeat(15);
    let decomposed = "e\u{0301}".repeat(15);
    let account = register_with(&app, &composed).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/auth/login",
            None,
            json!({"identity_id":account.id,"password":decomposed})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        change(&app, &account, &composed, &decomposed).await.0,
        StatusCode::NO_CONTENT
    );
    let fresh = login(&app, &account, &decomposed).await;
    let spaced = " leading and trailing spaces ";
    assert_eq!(
        change(&app, &fresh, &decomposed, spaced).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/auth/login",
            None,
            json!({"identity_id":account.id,"password":spaced.trim()})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    login(&app, &account, spaced).await;
}

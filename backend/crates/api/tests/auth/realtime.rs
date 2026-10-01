use super::*;

#[tokio::test]
async fn auth_private_realtime_room_reads_and_session_mutations_obey_membership() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let object = publish(&app, &alice, "private realtime object").await;
    let (status, room) = request(&app, "POST", "/realtime/rooms", Some(&alice.token), json!({
        "author_id":alice.id,"object_id":object,"name":"private-room",
        "schema":"babel.realtime.state.v1","membership":{"allow_list":[alice.id]},"persistence":"durable_messages"
    })).await;
    assert_eq!(status, StatusCode::OK, "{room}");
    let room_id = room["room"]["id"].as_str().unwrap();
    let uri = format!("/realtime/rooms/{room_id}");
    assert_eq!(
        request(&app, "GET", &uri, Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "GET", &uri, Some(&bob.token), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &uri, None, Value::Null).await.0,
        StatusCode::UNAUTHORIZED
    );
    let (status, started) = request(
        &app,
        "POST",
        "/realtime/sessions",
        Some(&alice.token),
        json!({"author_id":alice.id,"room_id":room_id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    let session = started["session"]["id"].as_str().unwrap();
    let uri = format!("/realtime/sessions/{session}");
    let payload = json!({"author_id":alice.id,"object_id":object,"session_id":session});
    assert_eq!(
        request(&app, "DELETE", &uri, Some(&bob.token), payload.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "DELETE", &uri, Some(&alice.token), payload)
            .await
            .0,
        StatusCode::OK
    );
}

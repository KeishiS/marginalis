#[tokio::test]
async fn oidc_rejects_a_subject_without_server_users_membership() {
    let server = TestServer::start().await;
    let response = login_response(
        &server,
        "non-member-subject",
        &["unrelated-group"],
        "rejected-login-code",
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(cookie(&response, "marginalis_session").is_none());
}


//! Integration test: refresh() POSTs JSON to the configured token_url.
//!
//! We can't easily redirect `claude_pro_max().token_url` to a mock server
//! (it's a `&'static str`), so this test exercises the same code path by
//! calling `motosan_ai_oauth::refresh` directly with a mock-server config.

use mockito::Matcher;
use motosan_ai_oauth::{OAuthConfig, StateStrategy, TokenBodyFormat};

#[tokio::test]
async fn refresh_posts_json_body_when_token_body_is_json() {
    let mut server = mockito::Server::new_async().await;
    let token_url: &'static str = Box::leak(format!("{}/token", server.url()).into_boxed_str());

    let mock = server
        .mock("POST", "/token")
        .match_header("content-type", Matcher::Regex("application/json".into()))
        .match_body(Matcher::JsonString(
            r#"{"grant_type":"refresh_token","refresh_token":"OLD_REFRESH","client_id":"test-client"}"#
                .into(),
        ))
        .with_status(200)
        .with_body(r#"{"access_token":"NEW_AT","refresh_token":"NEW_RT","expires_in":3600}"#)
        .create_async()
        .await;

    let cfg = OAuthConfig {
        client_id: "test-client",
        client_secret: None,
        auth_url: "https://unused/",
        token_url,
        scopes: &[],
        redirect_port: None,
        callback_path: "/callback",
        redirect_uri_host: "localhost",
        token_body: TokenBodyFormat::Json,
        extra_auth_params: &[],
        state_strategy: StateStrategy::Random,
    };

    let token = motosan_ai_oauth::refresh(&cfg, "OLD_REFRESH")
        .await
        .expect("refresh ok");
    assert_eq!(token.access_token, "NEW_AT");
    assert_eq!(token.refresh_token, "NEW_RT");
    mock.assert_async().await;
}

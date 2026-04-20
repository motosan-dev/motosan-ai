use serde::Deserialize;

use crate::{error::Error, unix_now, OAuthConfig, Token};

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    /// Servers may omit `refresh_token` on a refresh grant (RFC 6749 §6).
    /// Callers must pass their existing refresh token as fallback.
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    id_token: Option<String>,
    expires_in: u64,
}

impl RawTokenResponse {
    fn into_token(self, fallback_refresh: Option<&str>) -> Token {
        Token {
            access_token: self.access_token,
            refresh_token: self
                .refresh_token
                .or_else(|| fallback_refresh.map(str::to_owned))
                .unwrap_or_default(),
            id_token: self.id_token,
            expires_in: self.expires_in,
            issued_at: unix_now(),
        }
    }
}

async fn post_token(
    token_url: &str,
    params: Vec<(&str, &str)>,
    fallback_refresh: Option<&str>,
) -> Result<Token, Error> {
    let client = reqwest::Client::new();
    let resp = client
        .post(token_url)
        .header("User-Agent", "Mozilla/5.0 (compatible; motosan-ai-oauth)")
        .form(&params)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::TokenExchange(format!("HTTP {status}: {body}")));
    }

    Ok(resp
        .json::<RawTokenResponse>()
        .await?
        .into_token(fallback_refresh))
}

pub async fn exchange_code(
    config: &OAuthConfig,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<Token, Error> {
    let mut params = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
        ("client_id", config.client_id),
    ];
    if let Some(secret) = config.client_secret {
        params.push(("client_secret", secret));
    }
    post_token(config.token_url, params, None).await
}

pub async fn refresh_token(config: &OAuthConfig, refresh_token: &str) -> Result<Token, Error> {
    let mut params = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", config.client_id),
    ];
    if let Some(secret) = config.client_secret {
        params.push(("client_secret", secret));
    }
    post_token(config.token_url, params, Some(refresh_token)).await
}

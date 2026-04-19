use serde::Deserialize;

use crate::{error::Error, unix_now, Token, CLIENT_ID, REDIRECT_URI, TOKEN_URL};

#[derive(Deserialize)]
struct RawTokenResponse {
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_in: u64,
}

impl RawTokenResponse {
    fn into_token(self) -> Token {
        Token {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            id_token: self.id_token,
            expires_in: self.expires_in,
            issued_at: unix_now(),
        }
    }
}

async fn post_token(params: &[(&str, &str)]) -> Result<Token, Error> {
    let client = reqwest::Client::new();
    let resp = client
        .post(TOKEN_URL)
        .header("User-Agent", "Mozilla/5.0 (compatible; codex-oauth)")
        .form(params)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(Error::TokenExchange(format!("HTTP {status}: {body}")));
    }

    Ok(resp.json::<RawTokenResponse>().await?.into_token())
}

pub async fn exchange_code(code: &str, verifier: &str) -> Result<Token, Error> {
    post_token(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", verifier),
        ("client_id", CLIENT_ID),
    ])
    .await
}

pub async fn refresh_token(refresh_token: &str) -> Result<Token, Error> {
    post_token(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", CLIENT_ID),
    ])
    .await
}

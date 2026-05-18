mod error;
mod exchange;
mod pkce;
pub mod providers;
mod server;

pub use error::Error;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::RngCore as _;

const LOGIN_TIMEOUT_SECS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenBodyFormat {
    Form,
    Json,
}

#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub client_id: &'static str,
    pub client_secret: Option<&'static str>,
    pub auth_url: &'static str,
    pub token_url: &'static str,
    pub scopes: &'static [&'static str],
    pub redirect_port: Option<u16>,
    pub extra_auth_params: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Token {
    pub access_token: String,
    pub refresh_token: String,
    pub id_token: Option<String>,
    pub expires_in: u64,
    pub issued_at: u64,
}

impl Token {
    pub fn is_expired(&self) -> bool {
        unix_now() + 60 >= self.issued_at + self.expires_in
    }
}

pub async fn login(config: &OAuthConfig) -> Result<Token, Error> {
    let pkce = pkce::Pkce::generate();

    let mut state_bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut state_bytes);
    let state = URL_SAFE_NO_PAD.encode(state_bytes);

    let server = server::bind(config.redirect_port).await?;
    let redirect_uri = format!("http://127.0.0.1:{}/auth/callback", server.port);

    let auth_url = build_auth_url(config, &pkce.challenge, &state, &redirect_uri);

    println!("Open this URL to log in:\n\n  {auth_url}\n");
    let _ = open_browser(&auth_url);

    let (code, returned_state) = tokio::time::timeout(
        std::time::Duration::from_secs(LOGIN_TIMEOUT_SECS),
        server::wait_for_callback(server),
    )
    .await
    .map_err(|_| {
        Error::Callback(format!(
            "timed out waiting for browser callback ({LOGIN_TIMEOUT_SECS}s)"
        ))
    })??;

    if returned_state != state {
        return Err(Error::StateMismatch);
    }

    exchange::exchange_code(config, &code, &pkce.verifier, &redirect_uri).await
}

pub async fn refresh(config: &OAuthConfig, refresh_token: &str) -> Result<Token, Error> {
    exchange::refresh_token(config, refresh_token).await
}

pub(crate) fn build_auth_url(
    config: &OAuthConfig,
    challenge: &str,
    state: &str,
    redirect_uri: &str,
) -> String {
    let mut url = reqwest::Url::parse(config.auth_url).expect("auth_url must be valid");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("client_id", config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", redirect_uri)
            .append_pair("scope", &config.scopes.join(" "))
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        for (k, v) in config.extra_auth_params {
            q.append_pair(k, v);
        }
    }
    url.to_string()
}

pub(crate) fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn open_browser(url: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(url).spawn()?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(url).spawn()?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("cmd")
        .args(["/c", "start", "", url])
        .spawn()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_config() -> OAuthConfig {
        OAuthConfig {
            client_id: "test-client",
            client_secret: None,
            auth_url: "https://auth.example.com/oauth/authorize",
            token_url: "https://auth.example.com/oauth/token",
            scopes: &["openid", "profile"],
            redirect_port: None,
            extra_auth_params: &[],
        }
    }

    #[test]
    fn build_auth_url_contains_required_params() {
        let config = dummy_config();
        let url = build_auth_url(
            &config,
            "challenge123",
            "state456",
            "http://127.0.0.1:9999/auth/callback",
        );
        assert!(url.contains("client_id=test-client"));
        assert!(url.contains("response_type=code"));
        assert!(url.contains("code_challenge=challenge123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("state=state456"));
        assert!(url.contains("redirect_uri="));
        assert!(url.contains("scope="));
    }

    #[test]
    fn build_auth_url_includes_scopes_joined() {
        let config = dummy_config();
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        assert!(url.contains("scope=openid+profile") || url.contains("scope=openid%20profile"));
    }

    #[test]
    fn build_auth_url_parses_as_valid_url() {
        let config = dummy_config();
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        reqwest::Url::parse(&url).expect("auth URL must be valid");
    }

    #[test]
    fn build_auth_url_appends_extra_auth_params() {
        let config = OAuthConfig {
            extra_auth_params: &[("foo", "bar"), ("baz", "qux")],
            ..dummy_config()
        };
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        assert!(url.contains("foo=bar"));
        assert!(url.contains("baz=qux"));
    }

    #[test]
    fn build_auth_url_no_longer_hardcodes_access_type() {
        // Empty extra_auth_params should produce no access_type query pair.
        let config = OAuthConfig {
            extra_auth_params: &[],
            ..dummy_config()
        };
        let url = build_auth_url(&config, "c", "s", "http://127.0.0.1:1234/auth/callback");
        assert!(!url.contains("access_type="));
    }

    #[test]
    fn token_is_expired_when_issued_at_zero() {
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: None,
            expires_in: 3600,
            issued_at: 0,
        };
        assert!(token.is_expired());
    }

    #[test]
    fn token_not_expired_when_just_issued() {
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: None,
            expires_in: 3600,
            issued_at: unix_now(),
        };
        assert!(!token.is_expired());
    }

    #[test]
    fn token_within_buffer_is_considered_expired() {
        let token = Token {
            access_token: "a".into(),
            refresh_token: "r".into(),
            id_token: None,
            expires_in: 30, // expires in 30s, within the 60s buffer
            issued_at: unix_now(),
        };
        assert!(token.is_expired());
    }
}

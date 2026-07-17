//! Live >1h token-refresh smoke for the `chatgpt-codex` provider (F5).
//!
//! Proves a refreshing [`TokenSource`] carries a chat loop across the ~1h
//! ChatGPT access-token expiry: one chat call every 10 minutes for 70
//! minutes, each attempt resolving the bearer token through the source and
//! refreshing via `codex-oauth` when the token is (about to be) expired.
//!
//! Requires a valid `~/.codex/auth.json` with `tokens.refresh_token` and
//! `tokens.account_id` (mint via `codex login`). Discovery is through the
//! `HOME` env var only, mirroring `tests/chatgpt_codex_live.rs`. Token
//! material is never printed.
//!
//! Run manually (takes ~70 minutes):
//! `cargo test --features chatgpt-codex --test live_chatgpt_codex_token_refresh -- --ignored --nocapture`

#![cfg(feature = "chatgpt-codex")]

use motosan_ai::auth::{async_trait, TokenSource};
use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, MotosanError, StopReason};
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{env, fs};

const MODEL: &str = "gpt-5.5";
const CALL_INTERVAL: Duration = Duration::from_secs(600); // 10 minutes
const CALLS: u32 = 8; // t = 0, 10, ..., 70 minutes
const TEST_SPAN_SECS: u64 = 4200; // 70 minutes

/// Refreshing TokenSource built locally on the workspace `codex-oauth`
/// crate (`Token::is_expired()` + `refresh()`). Lives in the test, not the
/// SDK, keeping the SDK decoupled from the oauth crates (F5).
struct RefreshingSource {
    token: tokio::sync::Mutex<codex_oauth::Token>,
    refreshes: AtomicUsize,
}

impl std::fmt::Debug for RefreshingSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print token material.
        f.debug_struct("RefreshingSource").finish_non_exhaustive()
    }
}

#[async_trait]
impl TokenSource for RefreshingSource {
    async fn access_token(&self) -> Result<String, MotosanError> {
        let mut token = self.token.lock().await;
        if token.is_expired() {
            eprintln!("[refresh] access token expired — refreshing via codex-oauth");
            let refreshed = codex_oauth::refresh(&token.refresh_token)
                .await
                .map_err(|e| MotosanError::Auth {
                    message: format!("codex token refresh failed: {e}"),
                    status_code: None,
                    retry_after: None,
                    request_id: None,
                })?;
            *token = refreshed;
            self.refreshes.fetch_add(1, Ordering::SeqCst);
        }
        Ok(token.access_token.clone())
    }
}

/// Pull `tokens.refresh_token` + `tokens.account_id` from
/// `~/.codex/auth.json` (same discovery as
/// `tests/chatgpt_codex_live.rs::load_codex_auth`).
fn load_codex_refresh_auth() -> Option<(String, String)> {
    let home = env::var("HOME").ok()?;
    let path = std::path::Path::new(&home).join(".codex/auth.json");
    let raw = fs::read_to_string(path).ok()?;
    let auth: Value = serde_json::from_str(&raw).ok()?;
    let tokens = auth.get("tokens")?;
    let refresh_token = tokens.get("refresh_token")?.as_str()?.to_string();
    let account_id = tokens.get("account_id")?.as_str()?.to_string();
    Some((refresh_token, account_id))
}

#[tokio::test]
#[ignore = "live: ~70 minutes of real chatgpt.com calls; needs ~/.codex/auth.json"]
async fn live_refreshing_token_source_survives_token_expiry() {
    let Some((refresh_token, account_id)) = load_codex_refresh_auth() else {
        eprintln!("skipping live test: missing ~/.codex/auth.json tokens.refresh_token/account_id");
        return;
    };

    // Mint a fresh, fully-populated Token up front so issued_at/expires_in
    // are authoritative (auth.json does not persist them in oauth Token
    // form).
    let initial = codex_oauth::refresh(&refresh_token)
        .await
        .expect("initial refresh must succeed");
    let initial_expires_in = initial.expires_in;
    eprintln!("[setup] initial token expires_in = {initial_expires_in}s");

    let source = Arc::new(RefreshingSource {
        token: tokio::sync::Mutex::new(initial),
        refreshes: AtomicUsize::new(0),
    });

    // new() takes a static token we immediately override — pass "" so no
    // real credential ever sits in the (redacted) StaticTokenSource.
    let provider =
        ChatGptCodexProvider::new("", account_id, MODEL, None).with_token_source(source.clone());

    for call in 1..=CALLS {
        let elapsed_min = (call - 1) * 10;
        eprintln!("[t+{elapsed_min:>2}m] chat call {call}/{CALLS} ...");
        let response = provider
            .chat(
                ChatRequest::builder()
                    .message(Message::user("Reply with the single word: pong"))
                    .build(),
            )
            .await
            .unwrap_or_else(|e| panic!("call {call}/{CALLS} at t+{elapsed_min}m failed: {e}"));
        assert!(
            !response.content.is_empty(),
            "call {call}: empty response content"
        );
        assert_eq!(response.stop_reason, StopReason::EndTurn, "call {call}");
        eprintln!(
            "[t+{elapsed_min:>2}m] ok: {:?} (refreshes so far: {})",
            response.content.trim(),
            source.refreshes.load(Ordering::SeqCst)
        );
        if call < CALLS {
            tokio::time::sleep(CALL_INTERVAL).await;
        }
    }

    let refreshes = source.refreshes.load(Ordering::SeqCst);
    eprintln!("[done] {CALLS} calls over 70 minutes; {refreshes} refresh(es)");
    // If the initial token expires inside the 70-minute window (~1h
    // lifetime), at least one refresh must have happened.
    if initial_expires_in < TEST_SPAN_SECS {
        assert!(
            refreshes >= 1,
            "token lifetime {initial_expires_in}s < {TEST_SPAN_SECS}s test span, \
             yet no refresh happened"
        );
    }
}

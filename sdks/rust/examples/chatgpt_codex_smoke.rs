//! Live end-to-end smoke for the `chatgpt-codex` provider.
//!
//! This drives the **real** serializer + **real** HTTP + **real** SSE adapter
//! against the live ChatGPT backend (`chatgpt.com/backend-api/codex/responses`)
//! using the codex token you already minted with `codex login`. It is a manual
//! smoke — running it makes one authenticated network call.
//!
//! ## Run
//!
//! ```sh
//! cargo run --example chatgpt_codex_smoke --features chatgpt-codex
//! ```
//!
//! Expected: a header line, then "say hi in exactly three words" streamed back
//! token-by-token, then a summary line with the stop reason and (if reported)
//! token usage.
//!
//! Requires a valid `~/.codex/auth.json`. The access token is read but **never**
//! printed.

use std::process::ExitCode;

use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::types::{ChatRequest, Message, StreamEventType};
use serde_json::Value;
use tokio_stream::StreamExt;

/// The model to smoke. Override by editing this constant.
const MODEL: &str = "gpt-5.5";

// `current_thread` because the `chatgpt-codex` feature only enables tokio's
// single-threaded `rt` (no `rt-multi-thread`); a bare `#[tokio::main]` would
// fail to build against this feature set.
#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    eprintln!("== chatgpt-codex live smoke ==");

    // 1. Load the codex token from ~/.codex/auth.json.
    let (access_token, account_id) = match load_codex_auth() {
        Ok(pair) => pair,
        Err(msg) => {
            eprintln!("error: {msg}");
            eprintln!("hint: run `codex login` first to mint ~/.codex/auth.json");
            return ExitCode::FAILURE;
        }
    };
    // NOTE: never print `access_token`.
    eprintln!("loaded codex auth (account_id = {account_id}), model = {MODEL}");

    // 2. Build the provider against the real chatgpt.com endpoint (base_url=None
    //    => the default CHATGPT_CODEX_URL).
    let provider = ChatGptCodexProvider::new(access_token, account_id, MODEL, None);

    // 3. One user message.
    let req: ChatRequest = ChatRequest::builder()
        .message(Message::user("say hi in exactly three words"))
        .build();

    // 4. Stream and print deltas inline.
    eprintln!("-- streaming response --");
    let mut stream = match provider.stream(req).await {
        Ok(s) => s,
        Err(e) => {
            // The error may carry a redacted upstream body — that's fine.
            eprintln!("\nstream request failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut stop_reason = None;
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;
    let mut got_text = false;

    while let Some(item) = stream.next().await {
        let event = match item {
            Ok(ev) => ev,
            Err(e) => {
                eprintln!("\nstream error: {e}");
                return ExitCode::FAILURE;
            }
        };

        if let Some(usage) = &event.usage {
            input_tokens = usage.input_tokens;
            output_tokens = usage.output_tokens;
        }
        if event.done {
            stop_reason = event.stop_reason.clone();
            break;
        }
        if event.event_type == StreamEventType::Text {
            print!("{}", event.content);
            // Flush so the streamed text appears live, not buffered to the end.
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
            got_text = true;
        }
    }

    println!();
    eprintln!(
        "-- done -- stop_reason={:?} usage: {} in / {} out tokens{}",
        stop_reason,
        input_tokens,
        output_tokens,
        if got_text {
            ""
        } else {
            " (no text deltas received)"
        },
    );

    ExitCode::SUCCESS
}

/// Read `~/.codex/auth.json` and pull out `tokens.access_token` and
/// `tokens.account_id`. Returns a human-readable error string on any failure.
fn load_codex_auth() -> Result<(String, String), String> {
    let home = std::env::var("HOME").map_err(|_| "HOME env var is not set".to_string())?;
    let path = format!("{home}/.codex/auth.json");

    let raw = std::fs::read_to_string(&path).map_err(|e| format!("could not read {path}: {e}"))?;
    let json: Value =
        serde_json::from_str(&raw).map_err(|e| format!("{path} is not valid JSON: {e}"))?;

    let tokens = json
        .get("tokens")
        .ok_or_else(|| format!("{path} has no `tokens` object"))?;

    let access_token = tokens
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{path} is missing `tokens.access_token`"))?
        .to_string();

    let account_id = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| format!("{path} is missing `tokens.account_id`"))?
        .to_string();

    Ok((access_token, account_id))
}

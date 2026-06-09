//! Parser for Gemini CLI's `--output-format stream-json` NDJSON events.
//!
//! Gemini CLI emits four event shapes, all on their own line:
//! - `{"type":"init",...}` — session metadata; `session_id` is surfaced for resume.
//! - `{"type":"message","role":"user","content":"..."}` — echo of the
//!   stdin prompt; ignored.
//! - `{"type":"message","role":"assistant","content":"...","delta":true}`
//!   — one streamed chunk of the model's answer. Multiple of these
//!   arrive per turn and must be concatenated.
//! - `{"type":"result","status":"success|...","stats":{...}}` — terminal
//!   event carrying the aggregated token usage for the turn.
//!
//! See the spike notes in the commit that introduced this module for
//! sample payloads.

use serde::Deserialize;

use crate::types::{StreamEvent, Usage};

/// Minimal typed view of the NDJSON events we care about. Unknown `type`
/// values land in [`GeminiStreamEvent::Other`] so the parser never panics
/// on future additions.
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum GeminiStreamEvent {
    #[serde(rename = "init")]
    Init {
        #[serde(default)]
        session_id: Option<String>,
    },

    #[serde(rename = "message")]
    Message {
        role: String,
        #[serde(default)]
        content: String,
        /// Gemini flags streamed chunks with `delta: true`. Final
        /// non-delta messages (if any future version emits them) are
        /// skipped to avoid double-counting against the accumulated
        /// chunks.
        #[serde(default)]
        delta: bool,
    },

    #[serde(rename = "result")]
    Result {
        #[serde(default)]
        status: Option<String>,
        #[serde(default)]
        stats: Option<GeminiStats>,
    },

    #[serde(other)]
    Other,
}

/// Token usage block from the terminal `result` event.
///
/// Gemini exposes top-level aggregated counts on `stats` plus a per-model
/// breakdown in `stats.models`. We take the aggregated numbers — they
/// already sum across the utility router and the main model.
#[derive(Deserialize)]
pub struct GeminiStats {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Cached-input token count. Maps to `cache_read_input_tokens` in
    /// our [`Usage`] struct.
    #[serde(default)]
    pub cached: Option<u64>,
}

/// Action produced by parsing a single NDJSON line.
pub enum NdjsonAction {
    /// An assistant content chunk to forward to the consumer.
    Text(StreamEvent),
    /// The `init` event announced the session id used by `--resume <id>`.
    SessionStarted(StreamEvent),
    /// Terminal `result` event. `usage` is populated when `stats` was
    /// present, and `done` is always emitted.
    Result {
        usage: Option<StreamEvent>,
        done: StreamEvent,
    },
    /// Terminal error surfaced from a non-success `result` status.
    Error(String),
}

/// Parse a single NDJSON line into an action.
///
/// Returns `None` for:
/// - Malformed JSON (ignored, consistent with sibling providers).
/// - `init` events.
/// - `message` events from the `user` role (stdin echo).
/// - `message` events with `delta: false` or empty content.
/// - Any future event type that lands in [`GeminiStreamEvent::Other`].
pub fn parse_ndjson_line(line: &str) -> Option<NdjsonAction> {
    let event: GeminiStreamEvent = serde_json::from_str(line).ok()?;
    match event {
        GeminiStreamEvent::Message {
            role,
            content,
            delta,
        } if role == "assistant" && delta && !content.is_empty() => {
            Some(NdjsonAction::Text(StreamEvent::text(content)))
        }
        GeminiStreamEvent::Init { session_id } => session_id
            .filter(|s| !s.is_empty())
            .map(|id| NdjsonAction::SessionStarted(StreamEvent::session_started(id))),
        GeminiStreamEvent::Result { status, stats } => {
            let status_ref = status.as_deref().unwrap_or("success");
            if status_ref != "success" {
                return Some(NdjsonAction::Error(format!(
                    "gemini CLI result status: {status_ref}"
                )));
            }
            let usage_event = stats.map(|s| {
                StreamEvent::usage(Usage {
                    input_tokens: s.input_tokens.unwrap_or(0) as u32,
                    output_tokens: s.output_tokens.unwrap_or(0) as u32,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: s.cached.map(|c| c as u32),
                })
            });
            Some(NdjsonAction::Result {
                usage: usage_event,
                done: StreamEvent::done(),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_assistant_delta_yields_text() {
        let line = r#"{"type":"message","timestamp":"t","role":"assistant","content":"Hello","delta":true}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Text(event) => {
                assert_eq!(event.content, "Hello");
                assert!(!event.done);
            }
            _ => panic!("expected Text action"),
        }
    }

    #[test]
    fn skip_user_message_echo() {
        let line = r#"{"type":"message","role":"user","content":"hi","delta":false}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn skip_assistant_without_delta_flag() {
        // Final non-delta message (hypothetical future snapshot) must
        // not double-count against accumulated chunks.
        let line = r#"{"type":"message","role":"assistant","content":"full answer"}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn skip_empty_assistant_delta() {
        let line = r#"{"type":"message","role":"assistant","content":"","delta":true}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn init_event_captures_session_id() {
        let line = r#"{"type":"init","session_id":"sess_42","model":"auto-gemini-3"}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::SessionStarted(event) => {
                assert_eq!(event.session_id.as_deref(), Some("sess_42"));
            }
            _ => panic!("expected SessionStarted"),
        }
    }

    #[test]
    fn init_event_without_session_id_is_ignored() {
        assert!(parse_ndjson_line(r#"{"type":"init","model":"auto-gemini-3"}"#).is_none());
    }

    #[test]
    fn skip_init_event() {
        let line = r#"{"type":"init","session_id":"abc","model":"auto-gemini-3"}"#;
        match parse_ndjson_line(line) {
            Some(NdjsonAction::SessionStarted(event)) => {
                assert_eq!(event.session_id.as_deref(), Some("abc"));
            }
            _ => panic!("expected SessionStarted"),
        }
    }

    #[test]
    fn parse_result_with_stats() {
        let line = r#"{"type":"result","status":"success","stats":{"total_tokens":120,"input_tokens":100,"output_tokens":20,"cached":40}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Result { usage, done } => {
                let usage = usage.expect("usage should be present");
                let u = usage.usage.expect("usage field should exist");
                assert_eq!(u.input_tokens, 100);
                assert_eq!(u.output_tokens, 20);
                assert_eq!(u.cache_read_input_tokens, Some(40));
                assert!(done.done);
            }
            _ => panic!("expected Result action"),
        }
    }

    #[test]
    fn parse_result_without_stats() {
        let line = r#"{"type":"result","status":"success"}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Result { usage, done } => {
                assert!(usage.is_none());
                assert!(done.done);
            }
            _ => panic!("expected Result action"),
        }
    }

    #[test]
    fn non_success_result_becomes_error() {
        let line = r#"{"type":"result","status":"failed"}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Error(msg) => assert!(msg.contains("failed")),
            _ => panic!("expected Error action"),
        }
    }

    #[test]
    fn ignore_unknown_event_type() {
        let line = r#"{"type":"progress","percent":50}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn handle_malformed_json() {
        assert!(parse_ndjson_line("not json at all").is_none());
        assert!(parse_ndjson_line("{").is_none());
        assert!(parse_ndjson_line("").is_none());
    }
}

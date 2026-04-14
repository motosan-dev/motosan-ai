//! JSONL event schema for `codex exec --json` and the bridge that maps it to
//! crate-level [`StreamEvent`]s.
//!
//! Codex emits newline-delimited JSON events with a `type` discriminator:
//! - `thread.started` / `turn.started` — ignored (lifecycle bookkeeping)
//! - `item.started` — ignored (partial items, we wait for completion)
//! - `item.completed` — surfaced only when `item.type == "agent_message"`
//! - `turn.completed` — produces a usage event plus a terminal done event
//! - `turn.failed` / `error` — mapped to [`NdjsonAction::Error`]
//! - Anything else — silently dropped via [`CodexStreamEvent::Other`]
//!
//! Only the fields we actually read are modeled; unknown fields are
//! ignored by serde.

use serde::Deserialize;

use crate::types::{StreamEvent, Usage};

/// Top-level event from `codex exec --json`.
///
/// Codex emits JSONL with a `type` discriminator. We only model the
/// variants the provider needs; everything else falls through to
/// [`Other`](Self::Other).
#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum CodexStreamEvent {
    /// An item finished (agent message, command execution, reasoning, …).
    /// Only `agent_message` items are surfaced downstream.
    #[serde(rename = "item.completed")]
    ItemCompleted { item: CodexItem },

    /// The turn finished successfully. Carries token usage when available.
    #[serde(rename = "turn.completed")]
    TurnCompleted {
        #[serde(default)]
        usage: Option<CodexUsage>,
    },

    /// The turn failed. The `error` payload is opaque (Codex may return
    /// any JSON shape), so we stringify it for the error message.
    #[serde(rename = "turn.failed")]
    TurnFailed {
        #[serde(default)]
        error: Option<serde_json::Value>,
    },

    /// A top-level error outside of a turn (auth, config, spawn).
    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        message: Option<String>,
    },

    /// Any unmodeled event (`thread.started`, `item.started`, etc.).
    #[serde(other)]
    Other,
}

/// A completed item inside an [`ItemCompleted`](CodexStreamEvent::ItemCompleted)
/// event.
///
/// Codex emits many item subtypes (`agent_message`, `reasoning`,
/// `command_execution`, `file_changes`, `mcp_tool_calls`, `web_searches`,
/// `plan_updates`); we inspect [`item_type`](Self::item_type) to decide
/// whether to surface the item.
#[derive(Deserialize)]
pub struct CodexItem {
    /// The item subtype string, e.g. `"agent_message"`.
    #[serde(rename = "type")]
    pub item_type: String,
    /// Text payload for text-bearing items. Absent for most non-message
    /// item types.
    #[serde(default)]
    pub text: Option<String>,
}

/// Token usage reported on `turn.completed`.
///
/// All fields are optional because Codex does not guarantee that every
/// field is present on every turn (e.g. cached token counts only appear
/// when prompt caching kicked in).
#[derive(Deserialize)]
pub struct CodexUsage {
    #[serde(default)]
    pub input_tokens: Option<u64>,
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Subset of `input_tokens` served from the prompt cache. Maps to
    /// [`Usage::cache_read_input_tokens`] downstream.
    #[serde(default)]
    pub cached_input_tokens: Option<u64>,
}

/// Result of parsing a single JSONL line.
///
/// Lifted out of the parser so both the blocking and streaming paths can
/// share the same translation logic.
pub enum NdjsonAction {
    /// An `agent_message` item with non-empty text, already converted to a
    /// [`StreamEvent::text`] event.
    Text(StreamEvent),
    /// Terminal: the turn completed. `usage` is `Some` when the
    /// `turn.completed` event included token counts. `done` is always a
    /// [`StreamEvent::done`] marker.
    Done {
        usage: Option<StreamEvent>,
        done: StreamEvent,
    },
    /// The turn failed or the CLI emitted a top-level `error`. The string
    /// is a human-readable message suitable for a
    /// [`MotosanError::ProviderError`](crate::error::MotosanError::ProviderError).
    Error(String),
}

/// Parse a single JSONL line from `codex exec --json`.
///
/// Returns `None` when the line is malformed JSON, an unmodeled event,
/// a non-`agent_message` item, or an empty-text agent message — all
/// cases that should be silently skipped by the caller's event loop.
pub fn parse_ndjson_line(line: &str) -> Option<NdjsonAction> {
    let event: CodexStreamEvent = serde_json::from_str(line).ok()?;
    match event {
        CodexStreamEvent::ItemCompleted { item } => {
            if item.item_type == "agent_message" {
                let text = item.text.unwrap_or_default();
                if text.is_empty() {
                    None
                } else {
                    Some(NdjsonAction::Text(StreamEvent::text(text)))
                }
            } else {
                None
            }
        }
        CodexStreamEvent::TurnCompleted { usage } => {
            let usage_event = usage.map(|u| {
                StreamEvent::usage(Usage {
                    input_tokens: u.input_tokens.unwrap_or(0) as u32,
                    output_tokens: u.output_tokens.unwrap_or(0) as u32,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: u.cached_input_tokens.map(|t| t as u32),
                })
            });
            Some(NdjsonAction::Done {
                usage: usage_event,
                done: StreamEvent::done(),
            })
        }
        CodexStreamEvent::TurnFailed { error } => {
            let msg = error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "codex turn failed".to_string());
            Some(NdjsonAction::Error(msg))
        }
        CodexStreamEvent::Error { message } => Some(NdjsonAction::Error(
            message.unwrap_or_else(|| "codex error".to_string()),
        )),
        CodexStreamEvent::Other => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_agent_message() {
        let line = r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"Hello"}}"#;
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
    fn ignore_non_agent_item() {
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"reasoning","text":"thinking"}}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn ignore_empty_agent_message() {
        let line =
            r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":""}}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn parse_turn_completed_with_usage() {
        let line = r#"{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Done { usage, done } => {
                let usage = usage.expect("usage should be present");
                let u = usage.usage.expect("usage field should exist");
                assert_eq!(u.input_tokens, 24763);
                assert_eq!(u.output_tokens, 122);
                assert_eq!(u.cache_read_input_tokens, Some(24448));
                assert!(done.done);
            }
            _ => panic!("expected Done action"),
        }
    }

    #[test]
    fn parse_turn_completed_without_usage() {
        let line = r#"{"type":"turn.completed"}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Done { usage, done } => {
                assert!(usage.is_none());
                assert!(done.done);
            }
            _ => panic!("expected Done action"),
        }
    }

    #[test]
    fn parse_turn_failed() {
        let line = r#"{"type":"turn.failed","error":{"code":"rate_limit"}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        assert!(matches!(action, NdjsonAction::Error(_)));
    }

    #[test]
    fn parse_error_event() {
        let line = r#"{"type":"error","message":"boom"}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Error(msg) => assert_eq!(msg, "boom"),
            _ => panic!("expected Error action"),
        }
    }

    #[test]
    fn ignore_unknown_event() {
        let line = r#"{"type":"thread.started","thread_id":"abc"}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn handle_malformed_json() {
        assert!(parse_ndjson_line("not json").is_none());
        assert!(parse_ndjson_line("{").is_none());
        assert!(parse_ndjson_line("").is_none());
    }
}

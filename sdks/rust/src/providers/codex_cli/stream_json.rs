//! JSONL event schema for `codex exec --json` and the bridge that maps it to
//! crate-level [`StreamEvent`]s.
//!
//! Codex emits newline-delimited JSON events with a `type` discriminator:
//! - `thread.started` — surfaced as [`StreamEvent::session_id`] for resume
//! - `turn.started` — ignored (lifecycle bookkeeping)
//! - `item.started` — ignored (partial items, we wait for completion)
//! - `item.completed` — surfaced when `item.type == "agent_message"` or a tool item
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
    /// Agent messages and tool items are surfaced downstream.
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

    /// Emitted once at the start of a turn. Carries the persistent thread id
    /// used to resume via `codex exec resume <thread_id>`.
    #[serde(rename = "thread.started")]
    ThreadStarted {
        #[serde(default)]
        thread_id: Option<String>,
    },

    /// Any unmodeled event (`item.started`, etc.).
    #[serde(other)]
    Other,
}

/// A completed item inside an [`ItemCompleted`](CodexStreamEvent::ItemCompleted)
/// event.
///
/// Codex emits many item subtypes (`agent_message`, `reasoning`,
/// `command_execution`, `file_changes`, `mcp_tool_call`, `web_searches`,
/// `plan_updates`); we inspect [`item_type`](Self::item_type) to decide
/// whether to surface the item.
#[derive(Deserialize)]
pub struct CodexItem {
    /// The item subtype string, e.g. `"agent_message"`.
    #[serde(rename = "type")]
    pub item_type: String,
    /// Item id used as the tool-call id when surfacing tool items.
    #[serde(default)]
    pub id: Option<String>,
    /// Text payload for text-bearing items. Absent for most non-message
    /// item types.
    #[serde(default)]
    pub text: Option<String>,
    /// MCP tool name for `mcp_tool_call` items.
    #[serde(default)]
    pub tool: Option<String>,
    /// MCP server name for `mcp_tool_call` items; used to qualify the surfaced
    /// tool name as `server/tool`.
    #[serde(default)]
    pub server: Option<String>,
    /// Shell command for `command_execution` items.
    #[serde(default)]
    pub command: Option<String>,
    /// Arguments payload for MCP tool calls.
    #[serde(default)]
    pub arguments: Option<serde_json::Value>,
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
    /// A command/MCP tool item, already expanded to start/args/end events.
    ToolCalls(Vec<StreamEvent>),
    /// The turn announced its persistent thread id (`thread.started`),
    /// already converted to a [`StreamEvent::session_started`] event.
    SessionStarted(StreamEvent),
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
        CodexStreamEvent::ItemCompleted { item } => match item.item_type.as_str() {
            "agent_message" => {
                let text = item.text.unwrap_or_default();
                if text.is_empty() {
                    None
                } else {
                    Some(NdjsonAction::Text(StreamEvent::text(text)))
                }
            }
            "command_execution" | "mcp_tool_call" => {
                let id = item.id.clone().unwrap_or_default();
                // MCP calls carry server + tool → surface as `server/tool`;
                // command_execution has no `tool` and falls back to the item type.
                let name = match (item.server.as_deref(), item.tool.as_deref()) {
                    (Some(server), Some(tool)) => format!("{server}/{tool}"),
                    (None, Some(tool)) => tool.to_string(),
                    _ => item.item_type.clone(),
                };
                let args = match item.arguments {
                    Some(ref v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
                    None => match item.command {
                        Some(ref cmd) => serde_json::json!({ "command": cmd }).to_string(),
                        None => "{}".to_string(),
                    },
                };
                Some(NdjsonAction::ToolCalls(vec![
                    StreamEvent::tool_call_start(id.clone(), name),
                    StreamEvent::tool_call_args_with_id(id.clone(), args),
                    StreamEvent::tool_call_end_with_id(id),
                ]))
            }
            _ => None,
        },
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
        CodexStreamEvent::ThreadStarted { thread_id } => thread_id
            .filter(|id| !id.is_empty())
            .map(|id| NdjsonAction::SessionStarted(StreamEvent::session_started(id))),
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
    fn command_execution_item_yields_tool_call_events() {
        use crate::types::StreamEventType;
        // VERIFIED against real `codex exec --json` (codex-cli 0.130.0, 2026-06-10):
        // the item is `command_execution` (singular) carrying id + command, plus the
        // folded-in RESULT fields (aggregated_output / exit_code / status) which we
        // drop — `#[serde(default)]` makes the extra fields ignored.
        let line = r#"{"type":"item.completed","item":{"id":"item_0","type":"command_execution","command":"ls -la","aggregated_output":"total 8\n","exit_code":0,"status":"completed"}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].tool_call_id.as_deref(), Some("item_0"));
        assert_eq!(
            events[0].tool_call_name.as_deref(),
            Some("command_execution")
        );
        assert_eq!(
            events[1].tool_call_args_delta.as_deref(),
            Some(r#"{"command":"ls -la"}"#)
        );
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
    }

    #[test]
    fn mcp_tool_call_item_uses_server_qualified_tool_name() {
        use crate::types::StreamEventType;
        // VERIFIED against a real `codex exec --json` MCP turn (codex-cli 0.130.0,
        // 2026-06-10): `mcp_tool_call` (singular) carries id / server / tool /
        // arguments, plus a folded-in result/status we drop. Name is `server/tool`.
        let line = r#"{"type":"item.completed","item":{"id":"item_2","type":"mcp_tool_call","server":"node_repl","tool":"js","arguments":{"code":"nodeRepl.write(String(2 + 2));","title":"Evaluate 2 + 2"},"result":{"content":[{"type":"text","text":"4"}]},"status":"completed"}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events[0].tool_call_id.as_deref(), Some("item_2"));
        assert_eq!(events[0].tool_call_name.as_deref(), Some("node_repl/js"));
        assert!(events[1]
            .tool_call_args_delta
            .as_deref()
            .unwrap()
            .contains("nodeRepl.write"));
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
    }

    #[test]
    fn non_tool_non_message_item_still_dropped() {
        let line =
            r#"{"type":"item.completed","item":{"id":"item_1","type":"file_changes","text":""}}"#;
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
    fn thread_started_captures_thread_id() {
        let line = r#"{"type":"thread.started","thread_id":"th_abc123"}"#;
        match parse_ndjson_line(line).expect("thread.started should now parse") {
            NdjsonAction::SessionStarted(event) => {
                assert_eq!(event.session_id.as_deref(), Some("th_abc123"));
                assert!(!event.done);
                assert_eq!(event.content, "");
            }
            _ => panic!("expected SessionStarted action"),
        }
    }

    #[test]
    fn thread_started_without_id_is_ignored() {
        assert!(parse_ndjson_line(r#"{"type":"thread.started"}"#).is_none());
    }

    #[test]
    fn ignore_unknown_event() {
        let line = r#"{"type":"item.started","item_id":"abc"}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn handle_malformed_json() {
        assert!(parse_ndjson_line("not json").is_none());
        assert!(parse_ndjson_line("{").is_none());
        assert!(parse_ndjson_line("").is_none());
    }
}

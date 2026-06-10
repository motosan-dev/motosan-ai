use serde::Deserialize;

use crate::types::{StreamEvent, Usage};

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeStreamEvent {
    // Legacy shape emitted by claude < 2.1.x. Kept for backward compatibility
    // with older binaries; never observed under current claude releases.
    #[serde(rename = "text")]
    Text { text: String },

    // Current shape (claude ≥ 2.1.x): assistant turns ship one event per
    // turn with a `message.content` array of typed blocks. We pull text out
    // of the `text` blocks and surface `tool_use` blocks as tool-call stream
    // events.
    #[serde(rename = "assistant")]
    Assistant { message: AssistantMessage },

    #[serde(rename = "result")]
    Result {
        // The `result` field on a terminal `result` event duplicates what
        // we already yielded as Text from the preceding `assistant` event.
        // Kept parsed so the variant matches the wire shape, and used as the
        // provider error message when `is_error` is true.
        result: String,
        #[serde(default)]
        usage: Option<ClaudeStreamUsage>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        is_error: bool,
        #[serde(default)]
        subtype: Option<String>,
    },

    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct AssistantMessage {
    pub content: Vec<AssistantContentBlock>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum AssistantContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },

    // `thinking`, etc — ignored.
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
pub struct ClaudeStreamUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Action produced by parsing a single NDJSON line.
pub enum NdjsonAction {
    Text(StreamEvent),
    ToolCalls(Vec<StreamEvent>),
    Result {
        usage: Option<StreamEvent>,
        done: StreamEvent,
        session_id: Option<String>,
    },
    Error(String),
}

/// Parse a single NDJSON line into an action.
/// Returns `None` for unrecognized or malformed events.
pub fn parse_ndjson_line(line: &str) -> Option<NdjsonAction> {
    let event: ClaudeStreamEvent = serde_json::from_str(line).ok()?;
    match event {
        ClaudeStreamEvent::Text { text } if !text.is_empty() => {
            Some(NdjsonAction::Text(StreamEvent::text(text)))
        }
        ClaudeStreamEvent::Assistant { message } => {
            let mut events: Vec<StreamEvent> = Vec::new();
            let mut text = String::new();
            let mut had_tool = false;

            for block in &message.content {
                match block {
                    AssistantContentBlock::Text { text: t } => text.push_str(t),
                    AssistantContentBlock::ToolUse { id, name, input } => {
                        if !text.is_empty() {
                            events.push(StreamEvent::text(std::mem::take(&mut text)));
                        }
                        had_tool = true;
                        events.push(StreamEvent::tool_call_start(id.clone(), name.clone()));
                        let args =
                            serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                        events.push(StreamEvent::tool_call_args_with_id(id.clone(), args));
                        events.push(StreamEvent::tool_call_end_with_id(id.clone()));
                    }
                    AssistantContentBlock::Other => {}
                }
            }

            if !text.is_empty() {
                events.push(StreamEvent::text(text));
            }

            if had_tool {
                Some(NdjsonAction::ToolCalls(events))
            } else {
                events.pop().map(NdjsonAction::Text)
            }
        }
        ClaudeStreamEvent::Result {
            result,
            usage,
            session_id,
            is_error,
            subtype,
        } => {
            if is_error {
                let msg = if result.is_empty() {
                    subtype.unwrap_or_else(|| "claude CLI provider error".to_string())
                } else {
                    result
                };
                return Some(NdjsonAction::Error(msg));
            }
            let usage_event = usage.map(|u| {
                StreamEvent::usage(Usage {
                    input_tokens: u.input_tokens.unwrap_or(0) as u32,
                    output_tokens: u.output_tokens.unwrap_or(0) as u32,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                })
            });
            Some(NdjsonAction::Result {
                usage: usage_event,
                done: StreamEvent::done(),
                session_id: session_id.filter(|s| !s.is_empty()),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text_event() {
        let line = r#"{"type":"text","text":"Hello"}"#;
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
    fn parse_result_with_usage() {
        let line =
            r#"{"type":"result","result":"done","usage":{"input_tokens":12,"output_tokens":8}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Result { usage, done, .. } => {
                let usage = usage.expect("usage should be present");
                let u = usage.usage.expect("usage field should exist");
                assert_eq!(u.input_tokens, 12);
                assert_eq!(u.output_tokens, 8);
                assert!(done.done);
            }
            _ => panic!("expected Result action"),
        }
    }

    #[test]
    fn result_event_surfaces_session_id() {
        let line = r#"{"type":"result","result":"done","session_id":"sess_99"}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::Result { session_id, .. } => {
                assert_eq!(session_id.as_deref(), Some("sess_99"));
            }
            _ => panic!("expected Result action"),
        }
    }

    #[test]
    fn result_event_with_is_error_surfaces_error_action() {
        let line = r#"{"type":"result","subtype":"success","is_error":true,"result":"bad model"}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::Error(msg) => assert!(msg.contains("bad model")),
            _ => panic!("expected Error action"),
        }
    }

    #[test]
    fn result_event_empty_session_id_is_none() {
        // An empty/absent session_id must filter to None (no empty session_started).
        for line in [
            r#"{"type":"result","result":"done","session_id":""}"#,
            r#"{"type":"result","result":"done"}"#,
        ] {
            match parse_ndjson_line(line).expect("parse") {
                NdjsonAction::Result { session_id, .. } => {
                    assert_eq!(session_id, None, "line={line}");
                }
                _ => panic!("expected Result action"),
            }
        }
    }

    #[test]
    fn parse_result_without_usage() {
        let line = r#"{"type":"result","result":"done"}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Result { usage, done, .. } => {
                assert!(usage.is_none());
                assert!(done.done);
            }
            _ => panic!("expected Result action"),
        }
    }

    #[test]
    fn ignore_unknown_event() {
        let line = r#"{"type":"progress","percent":50}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn handle_malformed_json() {
        assert!(parse_ndjson_line("not json at all").is_none());
        assert!(parse_ndjson_line("{").is_none());
        assert!(parse_ndjson_line("").is_none());
    }

    #[test]
    fn ignore_empty_text() {
        let line = r#"{"type":"text","text":""}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn parse_assistant_event_with_single_text_block() {
        // Wire shape emitted by claude ≥ 2.1.x in stream-json mode.
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Text(event) => {
                assert_eq!(event.content, "pong");
                assert!(!event.done);
            }
            _ => panic!("expected Text action"),
        }
    }

    #[test]
    fn assistant_event_skips_thinking_keeps_text() {
        // Mixed content with a thinking block before the text block. Only the
        // text portion should reach motosan-ai's stream consumers.
        let line = r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"hmm"},{"type":"text","text":"pong"}]}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Text(event) => {
                assert_eq!(event.content, "pong");
            }
            _ => panic!("expected Text action"),
        }
    }

    #[test]
    fn tool_use_block_yields_tool_call_events() {
        use crate::types::StreamEventType;
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"path":"/tmp/x"}}]}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, StreamEventType::ToolCallStart);
        assert_eq!(events[0].tool_call_id.as_deref(), Some("toolu_01"));
        assert_eq!(events[0].tool_call_name.as_deref(), Some("Read"));
        assert_eq!(
            events[1].tool_call_args_delta.as_deref(),
            Some(r#"{"path":"/tmp/x"}"#)
        );
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
        assert_eq!(events[2].tool_call_id.as_deref(), Some("toolu_01"));
    }

    #[test]
    fn assistant_text_only_still_yields_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::Text(event) => assert_eq!(event.content, "pong"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn assistant_text_and_tool_use_preserves_text() {
        use crate::types::StreamEventType;
        // Mixed turn: narration text + a tool_use block in one content[].
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"let me read it"},{"type":"tool_use","id":"toolu_2","name":"Read","input":{"path":"/x"}}]}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        // Full shape: text first, then the complete start→args→end triplet.
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].event_type, StreamEventType::Text);
        assert_eq!(events[0].content, "let me read it");
        assert_eq!(events[1].event_type, StreamEventType::ToolCallStart);
        assert_eq!(events[1].tool_call_name.as_deref(), Some("Read"));
        assert_eq!(events[2].event_type, StreamEventType::ToolCallArgs);
        assert_eq!(events[3].event_type, StreamEventType::ToolCallEnd);
    }

    #[test]
    fn assistant_event_with_only_non_text_blocks_is_dropped() {
        // Unknown-only assistant turn: nothing to surface as text.
        let line =
            r#"{"type":"assistant","message":{"content":[{"type":"thinking","thinking":"x"}]}}"#;
        assert!(parse_ndjson_line(line).is_none());
    }

    #[test]
    fn assistant_event_concatenates_multiple_text_blocks_no_separator() {
        // If claude ever ships an assistant turn with multiple text blocks in
        // a single event, they are concatenated verbatim (no separator). Locks
        // in the silent-concat behavior so a future refactor doesn't drift.
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"foo"},{"type":"text","text":"bar"}]}}"#;
        let action = parse_ndjson_line(line).expect("should parse");
        match action {
            NdjsonAction::Text(event) => {
                assert_eq!(event.content, "foobar");
            }
            _ => panic!("expected Text action"),
        }
    }

    #[test]
    fn assistant_event_with_empty_content_array_is_dropped() {
        // Defensive: an empty content array (zero blocks of any kind) yields
        // nothing rather than an empty Text event that would confuse consumers.
        let line = r#"{"type":"assistant","message":{"content":[]}}"#;
        assert!(parse_ndjson_line(line).is_none());
    }
}

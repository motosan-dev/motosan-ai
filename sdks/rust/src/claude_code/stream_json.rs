use serde::Deserialize;

use crate::types::{StreamEvent, Usage};

#[derive(Deserialize)]
#[serde(tag = "type")]
pub enum ClaudeStreamEvent {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "result")]
    Result {
        #[allow(dead_code)]
        result: String,
        #[serde(default)]
        usage: Option<ClaudeStreamUsage>,
    },

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
    Result {
        usage: Option<StreamEvent>,
        done: StreamEvent,
    },
}

/// Parse a single NDJSON line into an action.
/// Returns `None` for unrecognized or malformed events.
pub fn parse_ndjson_line(line: &str) -> Option<NdjsonAction> {
    let event: ClaudeStreamEvent = serde_json::from_str(line).ok()?;
    match event {
        ClaudeStreamEvent::Text { text } if !text.is_empty() => {
            Some(NdjsonAction::Text(StreamEvent::text(text)))
        }
        ClaudeStreamEvent::Result { usage, .. } => {
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
            NdjsonAction::Result { usage, done } => {
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
    fn parse_result_without_usage() {
        let line = r#"{"type":"result","result":"done"}"#;
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
}

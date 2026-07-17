//! Shared helpers for the CLI backends (claude-code / codex-cli / gemini-cli),
//! compiled ONCE behind the private `_cli` umbrella feature.

/// Terminal stop reason for a CLI turn.
///
/// NOTE: scheduled for retirement in M4 Task 3 (F4 — CLI backends always
/// report `EndTurn`). Moved here unchanged first so this diff stays
/// mechanical.
pub(crate) fn cli_terminal_stop_reason(saw_tool_call: bool) -> crate::types::StopReason {
    if saw_tool_call {
        crate::types::StopReason::ToolUse
    } else {
        crate::types::StopReason::EndTurn
    }
}

#[cfg(test)]
mod cli_terminal_tests {
    use crate::stream::{collect_stream, BoxStream};
    use crate::types::{StopReason, StreamEvent};
    use tokio_stream::iter;

    #[tokio::test]
    async fn tool_call_terminal_reason_collects_as_tool_use() {
        // Direct truth table for both branches (the false→EndTurn arm was untested).
        assert_eq!(super::cli_terminal_stop_reason(false), StopReason::EndTurn);
        assert_eq!(super::cli_terminal_stop_reason(true), StopReason::ToolUse);
        let events = vec![
            StreamEvent::tool_call_start("call_1", "Read"),
            StreamEvent::tool_call_args_with_id("call_1", r#"{"path":"/tmp/x"}"#),
            StreamEvent::tool_call_end_with_id("call_1"),
            StreamEvent::done_with_stop_reason(super::cli_terminal_stop_reason(true)),
        ];
        let stream: BoxStream = Box::pin(iter(events.into_iter().map(Ok)));
        let resp = collect_stream(stream).await.expect("collect");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }
}

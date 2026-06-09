pub use crate::types::StreamEvent;
use futures_core::Stream;
use std::pin::Pin;

pub type BoxStream = Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

/// Collect a streaming response into a single [`ChatResponse`].
///
/// This eliminates the boilerplate of manually matching each
/// [`StreamEventType`] variant and accumulating text, tool calls, and usage
/// tokens.  The returned `ChatResponse` has `model` set to an empty string
/// because the stream events do not carry model information; callers that
/// need the model name should fill it in after the call.
///
/// # Example
///
/// ```ignore
/// let stream = client.stream(messages).await?;
/// let response = motosan_ai::collect_stream(stream).await;
/// println!("{}", response.content);
/// ```
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
))]
pub async fn collect_stream(mut stream: BoxStream) -> crate::types::ChatResponse {
    use crate::types::{ChatResponse, StopReason, StreamEventType, ToolCall, Usage};
    use tokio_stream::StreamExt;

    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut current_tc_id = String::new();
    let mut current_tc_name = String::new();
    let mut current_tc_args = String::new();
    let mut input_tokens: u32 = 0;
    let mut output_tokens: u32 = 0;
    let mut cache_creation_input_tokens: Option<u32> = None;
    let mut cache_read_input_tokens: Option<u32> = None;
    let mut explicit_stop_reason: Option<StopReason> = None;
    let mut session_id: Option<String> = None;
    // Thinking accumulation. `thinking_delta_buf` collects every
    // ThinkingDelta as a fallback in case the provider does not emit
    // ThinkingDone. `thinking_done_buf` holds the explicit final text
    // from the most recent ThinkingDone and takes priority on assembly.
    let mut thinking_delta_buf = String::new();
    let mut thinking_done_buf: Option<String> = None;

    while let Some(event) = stream.next().await {
        if event.session_id.is_some() {
            session_id = event.session_id.clone();
        }
        if event.done {
            // The terminal event may carry a provider-reported stop reason
            // (Anthropic message_delta.stop_reason, OpenAI finish_reason).
            // Capture it before exiting the loop so the heuristic below
            // becomes a fallback only.
            if let Some(reason) = event.stop_reason {
                explicit_stop_reason = Some(reason);
            }
            break;
        }
        match event.event_type {
            StreamEventType::Text => {
                content.push_str(&event.content);
            }
            StreamEventType::Usage => {
                if let Some(ref usage) = event.usage {
                    input_tokens += usage.input_tokens;
                    output_tokens += usage.output_tokens;
                    if let Some(v) = usage.cache_creation_input_tokens {
                        *cache_creation_input_tokens.get_or_insert(0) += v;
                    }
                    if let Some(v) = usage.cache_read_input_tokens {
                        *cache_read_input_tokens.get_or_insert(0) += v;
                    }
                }
            }
            StreamEventType::ToolCallStart => {
                current_tc_id = event.tool_call_id.unwrap_or_default();
                current_tc_name = event.tool_call_name.unwrap_or_default();
                current_tc_args.clear();
            }
            StreamEventType::ToolCallArgs => {
                if let Some(delta) = &event.tool_call_args_delta {
                    current_tc_args.push_str(delta);
                }
            }
            StreamEventType::ToolCallEnd => {
                let input: serde_json::Value = serde_json::from_str(&current_tc_args)
                    .unwrap_or_else(|_| serde_json::json!({}));
                tool_calls.push(ToolCall {
                    id: std::mem::take(&mut current_tc_id),
                    name: std::mem::take(&mut current_tc_name),
                    input,
                });
                current_tc_args.clear();
            }
            StreamEventType::ThinkingDelta => {
                thinking_delta_buf.push_str(&event.content);
            }
            StreamEventType::ThinkingDone => {
                // ThinkingDone carries the full text. Prefer it over the
                // delta accumulator when available — the provider knows
                // the authoritative concatenation. Also clear the delta
                // buffer so a second thinking block starts fresh.
                thinking_done_buf = Some(event.content.clone());
                thinking_delta_buf.clear();
            }
        }
    }

    let stop_reason = match explicit_stop_reason {
        Some(reason) => reason,
        None if tool_calls.is_empty() => StopReason::EndTurn,
        None => StopReason::ToolUse,
    };

    let thinking = match thinking_done_buf {
        Some(text) if !text.is_empty() => Some(text),
        Some(_) => None, // explicit empty thinking block -> treat as none
        None if !thinking_delta_buf.is_empty() => Some(thinking_delta_buf),
        None => None,
    };

    ChatResponse {
        content,
        thinking,
        model: String::new(),
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
        },
        stop_reason,
        session_id,
        tool_calls,
    }
}

#[cfg(test)]
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
))]
mod thinking_collect_tests {
    use super::*;
    use crate::types::StreamEvent;
    use tokio_stream::iter;

    #[tokio::test]
    async fn collect_stream_accumulates_thinking_into_response_thinking() {
        let events = vec![
            StreamEvent::thinking_delta("Let me "),
            StreamEvent::thinking_delta("think..."),
            StreamEvent::thinking_done("Let me think..."),
            StreamEvent::text("Answer: "),
            StreamEvent::text("42"),
            StreamEvent::done(),
        ];
        let stream: BoxStream = Box::pin(iter(events));
        let resp = collect_stream(stream).await;
        assert_eq!(resp.content, "Answer: 42");
        assert_eq!(
            resp.thinking.as_deref(),
            Some("Let me think..."),
            "thinking field must come from ThinkingDone (or accumulated deltas if no Done)"
        );
    }

    #[tokio::test]
    async fn collect_stream_no_thinking_keeps_thinking_none() {
        let events = vec![StreamEvent::text("hello"), StreamEvent::done()];
        let stream: BoxStream = Box::pin(iter(events));
        let resp = collect_stream(stream).await;
        assert_eq!(resp.content, "hello");
        assert!(resp.thinking.is_none());
    }

    #[tokio::test]
    async fn collect_stream_falls_back_to_accumulated_deltas_if_no_done() {
        // Defensive: if a provider somehow emits ThinkingDelta but skips
        // ThinkingDone, collect_stream still produces a thinking field
        // from the accumulated deltas.
        let events = vec![
            StreamEvent::thinking_delta("A "),
            StreamEvent::thinking_delta("B"),
            StreamEvent::text("ok"),
            StreamEvent::done(),
        ];
        let stream: BoxStream = Box::pin(iter(events));
        let resp = collect_stream(stream).await;
        assert_eq!(resp.thinking.as_deref(), Some("A B"));
        assert_eq!(resp.content, "ok");
    }
}

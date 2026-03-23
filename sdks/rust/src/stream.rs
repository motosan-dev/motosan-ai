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
    feature = "ollama_native"
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

    while let Some(event) = stream.next().await {
        if event.done {
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
        }
    }

    let stop_reason = if tool_calls.is_empty() {
        StopReason::EndTurn
    } else {
        StopReason::ToolUse
    };

    ChatResponse {
        content,
        thinking: None,
        model: String::new(),
        usage: Usage {
            input_tokens,
            output_tokens,
        },
        stop_reason,
        tool_calls,
    }
}

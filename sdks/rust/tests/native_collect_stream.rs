#![cfg(feature = "chatgpt-codex")]

use motosan_ai::{collect_model_stream, ModelStreamDelta, ModelToolCall, StopReason, Usage};
use tokio_stream::iter;

#[tokio::test]
async fn native_collect_stream_preserves_freeform_tool_call() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::FreeformInput {
                call_id: "call_js".to_string(),
                delta: "console.".to_string(),
            },
            ModelStreamDelta::FreeformInput {
                call_id: "call_js".to_string(),
                delta: "log(1);".to_string(),
            },
            ModelStreamDelta::ToolCallDone {
                call: ModelToolCall::Freeform {
                    id: "call_js".to_string(),
                    name: "exec".to_string(),
                    input: "console.log(1);".to_string(),
                },
            },
            ModelStreamDelta::Usage {
                usage: Usage {
                    input_tokens: 2,
                    output_tokens: 3,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream)
        .await
        .expect("native stream should collect");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "console.log(1);".to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
    assert_eq!(response.usage.output_tokens, 3);
}

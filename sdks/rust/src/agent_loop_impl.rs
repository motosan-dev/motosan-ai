//! Implements [`motosan_agent_loop::LlmClient`] for [`Client`] so that the
//! motosan-ai multi-provider client can drive an [`AgentLoop`] directly.

use motosan_agent_loop::llm::{LlmClient, LlmResponse, ToolCallItem};
use motosan_agent_loop::message::{Message as LoopMessage, Role as LoopRole};
use motosan_agent_loop::AgentError;
use motosan_agent_tool::ToolDef;

use crate::client::Client;
use crate::types::{ChatRequest, Message as AiMessage, ToolCall as AiToolCall};

#[async_trait::async_trait]
impl LlmClient for Client {
    async fn chat(
        &self,
        messages: Vec<LoopMessage>,
        tools: &[ToolDef],
    ) -> motosan_agent_loop::Result<LlmResponse> {
        // Convert loop Messages to motosan-ai Messages.
        let ai_messages: Vec<AiMessage> = messages
            .into_iter()
            .map(loop_message_to_ai_message)
            .collect::<motosan_agent_loop::Result<Vec<_>>>()?;

        // Build ChatRequest, attaching tool definitions when provided.
        let mut req = ChatRequest::builder().messages(ai_messages);
        if !tools.is_empty() {
            req = req.tool_defs(tools);
        }

        // Dispatch to the underlying provider.
        let response = self
            .chat_with(req.build())
            .await
            .map_err(|e| AgentError::Llm(e.to_string()))?;

        // Convert ChatResponse to LlmResponse.
        if !response.tool_calls.is_empty() {
            let items: Vec<ToolCallItem> = response
                .tool_calls
                .into_iter()
                .map(|tc| ToolCallItem {
                    id: tc.id,
                    name: tc.name,
                    args: tc.input,
                })
                .collect();
            Ok(LlmResponse::ToolCalls(items))
        } else {
            Ok(LlmResponse::Message(response.content))
        }
    }
}

/// Convert a [`motosan_agent_loop::Message`] into a [`motosan_ai::Message`].
///
/// Returns an error if a `Tool` message is missing its `tool_call_id`.
fn loop_message_to_ai_message(msg: LoopMessage) -> motosan_agent_loop::Result<AiMessage> {
    match msg.role {
        LoopRole::User => Ok(AiMessage::user(&msg.content)),
        LoopRole::System => Ok(AiMessage::system(&msg.content)),
        LoopRole::Tool => {
            let tool_call_id = msg
                .tool_call_id
                .ok_or_else(|| AgentError::Llm("Tool message missing tool_call_id".to_string()))?;
            Ok(AiMessage::tool_result(tool_call_id, &msg.content))
        }
        LoopRole::Assistant => {
            if msg.tool_calls.is_empty() {
                Ok(AiMessage::assistant(&msg.content))
            } else {
                let tcs: Vec<AiToolCall> = msg
                    .tool_calls
                    .into_iter()
                    .map(|tc| AiToolCall {
                        id: tc.id,
                        name: tc.name,
                        // `ToolCallRef` carries only `id` and `name`; the original
                        // arguments are not preserved in the loop message, so we
                        // use `Value::Null` as a placeholder.
                        input: serde_json::Value::Null,
                    })
                    .collect();
                Ok(AiMessage::assistant_with_tool_calls(&msg.content, tcs))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use motosan_agent_loop::message::ToolCallRef;
    use std::sync::Arc;

    // -----------------------------------------------------------
    // Compile-time check: Client satisfies `dyn LlmClient`.
    // -----------------------------------------------------------
    #[allow(dead_code)]
    fn assert_client_is_llm_client(_c: Arc<dyn LlmClient>) {}

    // -----------------------------------------------------------
    // Message conversion: User
    // -----------------------------------------------------------
    #[test]
    fn convert_user_message() {
        let loop_msg = LoopMessage::user("hello");
        let ai_msg = loop_message_to_ai_message(loop_msg).unwrap();

        assert_eq!(ai_msg.role, crate::types::Role::User);
        assert_eq!(ai_msg.content, "hello");
        assert!(ai_msg.tool_call_id.is_none());
        assert!(ai_msg.tool_calls.is_empty());
    }

    // -----------------------------------------------------------
    // Message conversion: System
    // -----------------------------------------------------------
    #[test]
    fn convert_system_message() {
        let loop_msg = LoopMessage::system("You are helpful.");
        let ai_msg = loop_message_to_ai_message(loop_msg).unwrap();

        assert_eq!(ai_msg.role, crate::types::Role::System);
        assert_eq!(ai_msg.content, "You are helpful.");
    }

    // -----------------------------------------------------------
    // Message conversion: Assistant (text only)
    // -----------------------------------------------------------
    #[test]
    fn convert_assistant_message() {
        let loop_msg = LoopMessage::assistant("Sure, let me help.");
        let ai_msg = loop_message_to_ai_message(loop_msg).unwrap();

        assert_eq!(ai_msg.role, crate::types::Role::Assistant);
        assert_eq!(ai_msg.content, "Sure, let me help.");
        assert!(ai_msg.tool_calls.is_empty());
    }

    // -----------------------------------------------------------
    // Message conversion: Assistant with tool calls
    // -----------------------------------------------------------
    #[test]
    fn convert_assistant_with_tool_calls() {
        let loop_msg = LoopMessage::assistant_with_tool_calls(
            "Let me search.",
            vec![
                ToolCallRef {
                    id: "call_1".to_string(),
                    name: "search".to_string(),
                },
                ToolCallRef {
                    id: "call_2".to_string(),
                    name: "fetch".to_string(),
                },
            ],
        );
        let ai_msg = loop_message_to_ai_message(loop_msg).unwrap();

        assert_eq!(ai_msg.role, crate::types::Role::Assistant);
        assert_eq!(ai_msg.content, "Let me search.");
        assert_eq!(ai_msg.tool_calls.len(), 2);
        assert_eq!(ai_msg.tool_calls[0].id, "call_1");
        assert_eq!(ai_msg.tool_calls[0].name, "search");
        assert_eq!(ai_msg.tool_calls[0].input, serde_json::Value::Null);
        assert_eq!(ai_msg.tool_calls[1].id, "call_2");
        assert_eq!(ai_msg.tool_calls[1].name, "fetch");
    }

    // -----------------------------------------------------------
    // Message conversion: Tool result
    // -----------------------------------------------------------
    #[test]
    fn convert_tool_result_message() {
        let loop_msg = LoopMessage::tool_result("call_abc", "result data");
        let ai_msg = loop_message_to_ai_message(loop_msg).unwrap();

        assert_eq!(ai_msg.role, crate::types::Role::Tool);
        assert_eq!(ai_msg.content, "result data");
        assert_eq!(ai_msg.tool_call_id.as_deref(), Some("call_abc"));
        assert!(ai_msg.tool_calls.is_empty());
    }

    // -----------------------------------------------------------
    // Message conversion: Tool message without tool_call_id → error
    // -----------------------------------------------------------
    #[test]
    fn convert_tool_message_missing_id_returns_error() {
        let loop_msg = LoopMessage {
            role: LoopRole::Tool,
            content: "result".to_string(),
            tool_call_id: None,
            tool_calls: vec![],
        };
        let result = loop_message_to_ai_message(loop_msg);
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("tool_call_id"),
            "error should mention tool_call_id: {err_msg}"
        );
    }
}

use crate::retry::RetryPolicy;
use crate::types::{ChatRequest, Role};
use reqwest::Client;
use serde_json::{json, Value};

/// Default endpoint for the ChatGPT-backend Responses API.
const CHATGPT_CODEX_URL: &str = "https://chatgpt.com/backend-api/codex/responses";
/// `originator` header value codex's CLI sends; settled GREEN by the spike.
const ORIGINATOR: &str = "codex_cli_rs";

// NOTE: `#[allow(dead_code)]` is for Task 1 ONLY (no `ProviderImpl` impl yet, so
// the fields/methods are not all read). It is removed in Task 4 when `stream`/`chat`
// land and consume them.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ChatGptCodexProvider {
    http: Client,
    access_token: String,
    account_id: String,
    model: String,
    base_url: String,
    retry_policy: RetryPolicy,
}

#[allow(dead_code)]
impl ChatGptCodexProvider {
    pub fn new(
        access_token: impl Into<String>,
        account_id: impl Into<String>,
        model: impl Into<String>,
        base_url: Option<String>,
    ) -> Self {
        Self {
            http: Client::new(),
            access_token: access_token.into(),
            account_id: account_id.into(),
            model: model.into(),
            base_url: base_url.unwrap_or_else(|| CHATGPT_CODEX_URL.to_string()),
            retry_policy: RetryPolicy::default(),
        }
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = policy;
        self
    }

    fn apply_auth(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request
            .header("authorization", format!("Bearer {}", self.access_token))
            .header("chatgpt-account-id", &self.account_id)
            .header("originator", ORIGINATOR)
            .header("openai-beta", "responses=experimental")
            .header("accept", "text/event-stream")
    }

    fn url(&self) -> String {
        self.base_url.clone()
    }

    /// Serialize a [`ChatRequest`] into a ChatGPT-backend **Responses API** body.
    ///
    /// Shape settled by the route-B spike (scope §1) + PI's `buildRequestBody`:
    /// `store:false` is hard-required, the system prompt goes in `instructions`
    /// (NOT into `input`), non-system messages convert to Responses **input
    /// items**, and reasoning encrypted content is requested via `include`.
    ///
    /// ## Content-block coverage
    ///
    /// The motosan `Message`/`ContentBlock` model carries `Text`/`Image`/
    /// `Document` blocks plus flat `content` text and `tool_calls`. This
    /// serializer maps the cases the ChatGPT-backend transport needs:
    /// - user/assistant flat text -> `message` items with `input_text`/`output_text`;
    /// - assistant `tool_calls` -> `function_call` items;
    /// - `Role::Tool` results -> `function_call_output` items.
    ///
    /// Multimodal `ContentBlock::Image`/`Document` blocks are **not** emitted
    /// here — v1 of this provider is text-only (`ProviderCapabilities::text_only`),
    /// and a request carrying them would be rejected upstream by validation.
    /// See `// TODO(phase2)` below for the image passthrough hook.
    pub fn build_responses_body(&self, req: &ChatRequest) -> Value {
        let model = req.model.clone().unwrap_or_else(|| self.model.clone());

        // Instructions = the system prompt. `system_blocks` (joined) takes
        // priority over the `system` string, then any `Role::System` message.
        // Mirrors openai.rs's `chat_via_responses` precedence.
        let mut instructions_parts: Vec<String> = Vec::new();
        if let Some(blocks) = &req.system_blocks {
            for b in blocks {
                let trimmed = b.text.trim();
                if !trimmed.is_empty() {
                    instructions_parts.push(trimmed.to_string());
                }
            }
        } else if let Some(system) = &req.system {
            let trimmed = system.trim();
            if !trimmed.is_empty() {
                instructions_parts.push(trimmed.to_string());
            }
        }

        let mut input: Vec<Value> = Vec::new();
        for message in &req.messages {
            match message.role {
                // System messages go into `instructions`, never `input`.
                Role::System => {
                    let trimmed = message.content.trim();
                    if !trimmed.is_empty() {
                        instructions_parts.push(trimmed.to_string());
                    }
                }
                Role::User => {
                    // TODO(phase2): emit `input_image` blocks for
                    // `message.content_blocks` (Image/Document) — text-only v1.
                    input.push(json!({
                        "type": "message",
                        "role": "user",
                        "content": [{"type": "input_text", "text": message.content}],
                    }));
                }
                Role::Assistant => {
                    // Assistant text (if any) becomes an `output_text` message item.
                    if !message.content.is_empty() {
                        input.push(json!({
                            "type": "message",
                            "role": "assistant",
                            "content": [{"type": "output_text", "text": message.content}],
                        }));
                    }
                    // Each tool call becomes its own `function_call` item.
                    for tool_call in &message.tool_calls {
                        input.push(json!({
                            "type": "function_call",
                            "call_id": tool_call.id,
                            "name": tool_call.name,
                            "arguments": serde_json::to_string(&tool_call.input)
                                .unwrap_or_else(|_| "{}".to_string()),
                        }));
                    }
                }
                Role::Tool => {
                    if let Some(call_id) = &message.tool_call_id {
                        input.push(json!({
                            "type": "function_call_output",
                            "call_id": call_id,
                            "output": message.content,
                        }));
                    }
                }
            }
        }

        let instructions = if instructions_parts.is_empty() {
            "You are a helpful assistant.".to_string()
        } else {
            instructions_parts.join("\n\n")
        };

        let mut body = json!({
            "model": model,
            "store": false,
            "stream": true,
            "instructions": instructions,
            "input": input,
            "include": ["reasoning.encrypted_content"],
            "tool_choice": "auto",
            "parallel_tool_calls": true,
        });

        // Conditional: tools (Responses flat tool shape, `strict: null`).
        if let Some(tools) = &req.tools {
            let mapped: Vec<Value> = tools
                .iter()
                .map(|tool| {
                    json!({
                        "type": "function",
                        "name": tool.schema.name,
                        "description": tool.schema.description,
                        "parameters": tool.schema.input_schema,
                        "strict": Value::Null,
                    })
                })
                .collect();
            if !mapped.is_empty() {
                body["tools"] = json!(mapped);
            }
        }

        // Conditional: reasoning `{effort, summary:"auto"}` when an effort is
        // supplied via provider_options (no first-class field on ChatRequest).
        if let Some(effort) = req
            .provider_options
            .as_ref()
            .and_then(|opts| opts.get("reasoning_effort"))
            .and_then(Value::as_str)
        {
            body["reasoning"] = json!({"effort": effort, "summary": "auto"});
        }

        // Conditional: temperature.
        if let Some(temperature) = req.temperature {
            body["temperature"] = json!(temperature);
        }

        body
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ChatRequest, Message, ToolCall};
    use serde_json::json;

    fn test_provider() -> ChatGptCodexProvider {
        ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None)
    }

    fn simple_user_request(text: &str) -> ChatRequest {
        ChatRequest::builder().message(Message::user(text)).build()
    }

    #[test]
    fn body_has_required_codex_fields() {
        let p = test_provider();
        let req = simple_user_request("hi");
        let body = p.build_responses_body(&req);

        assert_eq!(body["store"], json!(false)); // REQUIRED
        assert_eq!(body["stream"], json!(true));
        assert_eq!(body["model"], json!("gpt-5.5"));
        assert!(body["instructions"].is_string()); // system prompt here, not a message
        assert!(body["input"].is_array()); // converted Responses items
        assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
        assert_eq!(body["tool_choice"], json!("auto"));
        assert_eq!(body["parallel_tool_calls"], json!(true));

        // The single user message becomes one Responses `message` input item
        // with an `input_text` content block — system text is NOT in `input`.
        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["type"], "message");
        assert_eq!(input[0]["role"], "user");
        assert_eq!(input[0]["content"][0]["type"], "input_text");
        assert_eq!(input[0]["content"][0]["text"], "hi");

        // No tools / reasoning / temperature on a bare request.
        assert!(body.get("tools").is_none());
        assert!(body.get("reasoning").is_none());
        assert!(body.get("temperature").is_none());
    }

    #[test]
    fn system_message_goes_to_instructions_not_input() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::system("You are a pirate."))
            .message(Message::user("hi"))
            .build();
        let body = p.build_responses_body(&req);

        assert_eq!(body["instructions"], json!("You are a pirate."));
        let input = body["input"].as_array().unwrap();
        // Only the user message survives into `input`.
        assert_eq!(input.len(), 1);
        assert_eq!(input[0]["role"], "user");
    }

    #[test]
    fn assistant_text_becomes_output_text_item() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::user("hi"))
            .message(Message::assistant("hello there"))
            .build();
        let body = p.build_responses_body(&req);

        let input = body["input"].as_array().unwrap();
        assert_eq!(input.len(), 2);
        assert_eq!(input[1]["type"], "message");
        assert_eq!(input[1]["role"], "assistant");
        assert_eq!(input[1]["content"][0]["type"], "output_text");
        assert_eq!(input[1]["content"][0]["text"], "hello there");
    }

    #[test]
    fn tool_call_and_result_serialize_as_function_items() {
        let p = test_provider();
        let tool = motosan_agent_primitives::ToolSchema::new(
            "get_weather",
            "Fetch the weather",
            json!({"type": "object", "properties": {"city": {"type": "string"}}}),
        );
        let req = ChatRequest::builder()
            .tool_schemas(&[tool])
            .message(Message::user("weather in Paris?"))
            .message(Message::assistant_with_tool_calls(
                "",
                vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "get_weather".to_string(),
                    input: json!({"city": "Paris"}),
                }],
            ))
            .message(Message::tool_result("call_1", "sunny, 21C"))
            .build();
        let body = p.build_responses_body(&req);

        // Tools serialize to the Responses flat tool shape (strict: null).
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["name"], "get_weather");
        assert_eq!(tools[0]["description"], "Fetch the weather");
        assert!(tools[0]["parameters"].is_object());
        assert_eq!(tools[0]["strict"], json!(null));

        let input = body["input"].as_array().unwrap();
        // user message, function_call, function_call_output.
        assert_eq!(input.len(), 3);

        // Assistant tool call -> function_call item.
        let fc = &input[1];
        assert_eq!(fc["type"], "function_call");
        assert_eq!(fc["call_id"], "call_1");
        assert_eq!(fc["name"], "get_weather");
        // arguments are a JSON-encoded string.
        let args: serde_json::Value =
            serde_json::from_str(fc["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(args, json!({"city": "Paris"}));

        // Tool result -> function_call_output item.
        let out = &input[2];
        assert_eq!(out["type"], "function_call_output");
        assert_eq!(out["call_id"], "call_1");
        assert_eq!(out["output"], "sunny, 21C");
    }

    #[test]
    fn reasoning_and_temperature_are_conditional() {
        let p = test_provider();
        let req = ChatRequest::builder()
            .message(Message::user("hi"))
            .temperature(0.3)
            .provider_options(json!({"reasoning_effort": "high"}))
            .build();
        let body = p.build_responses_body(&req);

        // `temperature` is an f32 on ChatRequest; serde_json widens it to f64
        // so compare against the same widening rather than the f64 literal 0.3.
        assert_eq!(body["temperature"], json!(0.3_f32));
        assert_eq!(body["reasoning"]["effort"], json!("high"));
        assert_eq!(body["reasoning"]["summary"], json!("auto"));
    }
}

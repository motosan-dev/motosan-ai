#[cfg(feature = "_http")]
use crate::types::ModelStreamDelta;
use crate::types::{
    ChatResponse, ContentBlock, FunctionCallOutputPayload, ImageSource, Message, ModelChatRequest,
    ModelChatResponse, ModelContextItem, ModelToolCall, ModelToolOutput, ModelToolSpec, Role,
    StopReason, ToolCall, ToolChoice, Usage,
};
use serde_json::{json, Value};

pub fn encode_tools(tool_specs: &[ModelToolSpec]) -> Vec<Value> {
    tool_specs.iter().map(encode_tool_spec).collect()
}

pub fn encode_tool_spec(tool_spec: &ModelToolSpec) -> Value {
    serde_json::to_value(tool_spec).expect("model tool specs serialize to JSON")
}

pub fn encode_input(context: &[ModelContextItem]) -> Vec<Value> {
    context.iter().flat_map(encode_context_item).collect()
}

pub fn encode_context_item(item: &ModelContextItem) -> Vec<Value> {
    match item {
        ModelContextItem::Message(message) => encode_message(message),
        ModelContextItem::ToolCall(call) => vec![encode_tool_call(call)],
        ModelContextItem::ToolOutput(output) => vec![encode_tool_output(output)],
    }
}

pub fn encode_tool_call(call: &ModelToolCall) -> Value {
    serde_json::to_value(call).expect("model tool calls serialize to JSON")
}

pub fn encode_tool_output(output: &ModelToolOutput) -> Value {
    match output {
        ModelToolOutput::Function { call_id, output } => json!({
            "type": "function_call_output",
            "call_id": call_id,
            "output": output,
        }),
        ModelToolOutput::Custom {
            call_id, output, ..
        } => json!({
            "type": "custom_tool_call_output",
            "call_id": call_id,
            "output": output,
        }),
    }
}

pub fn decode_tool_call(item: &Value) -> Option<ModelToolCall> {
    serde_json::from_value(item.clone()).ok()
}

pub fn decode_tool_output(item: &Value) -> Option<ModelToolOutput> {
    serde_json::from_value(item.clone()).ok()
}

pub fn build_model_request_body(
    req: &ModelChatRequest,
    default_model: &str,
    stream: bool,
    default_instructions: Option<&str>,
) -> Value {
    let model = req
        .model
        .clone()
        .unwrap_or_else(|| default_model.to_string());
    let mut input_context = Vec::new();
    let mut instructions_parts = Vec::new();

    if let Some(blocks) = &req.system_blocks {
        for block in blocks {
            let trimmed = block.text.trim();
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

    for item in &req.context {
        match item {
            ModelContextItem::Message(message) if message.role == Role::System => {
                let trimmed = message.content.trim();
                if !trimmed.is_empty() {
                    instructions_parts.push(trimmed.to_string());
                }
            }
            other => input_context.push(other.clone()),
        }
    }

    let mut body = json!({
        "model": model,
        "input": encode_input(&input_context),
    });

    if stream {
        body["stream"] = json!(true);
    }
    if !req.tool_specs.is_empty() {
        body["tools"] = json!(encode_tools(&req.tool_specs));
    }
    let instructions = if instructions_parts.is_empty() {
        default_instructions.map(str::to_string)
    } else {
        Some(instructions_parts.join("\n\n"))
    };
    if let Some(instructions) = instructions {
        body["instructions"] = json!(instructions);
    }
    if let Some(temperature) = req.temperature {
        body["temperature"] = json!(temperature);
    }
    if let Some(max_tokens) = req.max_tokens {
        body["max_output_tokens"] = json!(max_tokens);
    }
    if let Some(tool_choice) = &req.tool_choice {
        body["tool_choice"] = encode_tool_choice(tool_choice);
    }
    if let Some(stop_sequences) = &req.stop_sequences {
        if !stop_sequences.is_empty() {
            body["stop"] = json!(stop_sequences);
        }
    }
    if let Some(provider_options) = &req.provider_options {
        if let Some(map) = provider_options.as_object() {
            for (key, value) in map {
                body[key] = value.clone();
            }
        }
    }

    body
}

fn encode_tool_choice(tool_choice: &ToolChoice) -> Value {
    match tool_choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Required => json!("required"),
        ToolChoice::None => json!("none"),
        ToolChoice::Tool { name } => json!({"type": "function", "name": name}),
    }
}

pub fn decode_output_text(item: &Value) -> Option<String> {
    if item.get("type").and_then(Value::as_str) != Some("message") {
        return None;
    }

    let text = item
        .get("content")
        .and_then(Value::as_array)?
        .iter()
        .filter(|content| content.get("type").and_then(Value::as_str) == Some("output_text"))
        .filter_map(|content| content.get("text").and_then(Value::as_str))
        .collect::<String>();

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

pub fn model_chat_response_from_output(payload: &Value, default_model: &str) -> ModelChatResponse {
    let mut content = String::new();
    let mut thinking = None;
    let mut tool_calls = Vec::new();

    if let Some(output_text) = payload.get("output_text").and_then(Value::as_str) {
        content.push_str(output_text);
    }

    if let Some(output_items) = payload.get("output").and_then(Value::as_array) {
        for item in output_items {
            if let Some(text) = decode_output_text(item) {
                content.push_str(&text);
            }
            if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                if let Some(summary) = item
                    .get("summary")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|summary| {
                                summary
                                    .get("text")
                                    .or_else(|| summary.get("content"))
                                    .and_then(Value::as_str)
                            })
                            .collect::<String>()
                    })
                    .filter(|summary| !summary.is_empty())
                {
                    thinking = Some(summary);
                }
            }
            if let Some(call) = decode_tool_call(item) {
                tool_calls.push(call);
            }
        }
    }

    let status = payload.get("status").and_then(Value::as_str);
    let stop_reason = stop_reason_from_status(status, !tool_calls.is_empty());
    let usage = decode_usage(payload.get("usage"));
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(default_model)
        .to_string();

    ModelChatResponse {
        content,
        thinking,
        tool_calls,
        model,
        usage,
        stop_reason,
        session_id: None,
    }
}

pub fn decode_usage(value: Option<&Value>) -> Usage {
    let input_tokens = value
        .and_then(|usage| {
            usage
                .get("input_tokens")
                .or_else(|| usage.get("prompt_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let output_tokens = value
        .and_then(|usage| {
            usage
                .get("output_tokens")
                .or_else(|| usage.get("completion_tokens"))
        })
        .and_then(Value::as_u64)
        .unwrap_or(0) as u32;
    let cache_read_input_tokens = value
        .and_then(|usage| usage.get("input_tokens_details"))
        .and_then(|details| details.get("cached_tokens"))
        .and_then(Value::as_u64)
        .map(|tokens| tokens as u32)
        .filter(|tokens| *tokens > 0);

    Usage {
        input_tokens,
        output_tokens,
        cache_creation_input_tokens: None,
        cache_read_input_tokens,
    }
}

pub fn stop_reason_from_status(status: Option<&str>, has_tool_calls: bool) -> StopReason {
    if has_tool_calls {
        return StopReason::ToolUse;
    }

    match status {
        Some("incomplete") => StopReason::MaxTokens,
        Some("completed") | None => StopReason::EndTurn,
        Some("failed") => StopReason::Other,
        Some(_) => StopReason::Other,
    }
}

pub fn chat_response_from_output(
    payload: &Value,
    default_model: &str,
    fallback_stop_reason: StopReason,
) -> ChatResponse {
    let mut content = String::new();
    let mut tool_calls = Vec::new();

    if let Some(output_text) = payload.get("output_text").and_then(Value::as_str) {
        content.push_str(output_text);
    }

    if let Some(output_items) = payload.get("output").and_then(Value::as_array) {
        for item in output_items {
            if let Some(text) = decode_output_text(item) {
                content.push_str(&text);
            }
            if let Some(ModelToolCall::Function {
                id,
                name,
                arguments,
            }) = decode_tool_call(item)
            {
                let input = serde_json::from_str(&arguments).unwrap_or(Value::String(arguments));
                tool_calls.push(ToolCall { id, name, input });
            }
        }
    }

    let usage = decode_usage(payload.get("usage"));
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or(default_model)
        .to_string();

    ChatResponse {
        content,
        thinking: None,
        tool_calls,
        model,
        usage,
        stop_reason: fallback_stop_reason,
        session_id: None,
    }
}

fn encode_message(message: &Message) -> Vec<Value> {
    match message.role {
        Role::System => Vec::new(),
        Role::User => vec![json!({
            "type": "message",
            "role": "user",
            "content": encode_user_content(message),
        })],
        Role::Assistant => {
            let mut items = Vec::new();
            if !message.content.is_empty() {
                items.push(json!({
                    "type": "message",
                    "role": "assistant",
                    "content": [{"type": "output_text", "text": message.content}],
                }));
            }
            for tool_call in &message.tool_calls {
                items.push(encode_tool_call(&ModelToolCall::Function {
                    id: tool_call.id.clone(),
                    name: tool_call.name.clone(),
                    arguments: serde_json::to_string(&tool_call.input)
                        .unwrap_or_else(|_| "{}".to_string()),
                }));
            }
            items
        }
        Role::Tool => message
            .tool_call_id
            .as_ref()
            .map(|call_id| {
                encode_tool_output(&ModelToolOutput::Function {
                    call_id: call_id.clone(),
                    output: FunctionCallOutputPayload::Text(message.content.clone()),
                })
            })
            .into_iter()
            .collect(),
    }
}

fn encode_user_content(message: &Message) -> Vec<Value> {
    if message.content_blocks.is_empty() {
        return vec![json!({"type": "input_text", "text": message.content})];
    }

    let mut content = Vec::new();
    for block in &message.content_blocks {
        match block {
            ContentBlock::Text { text } => {
                content.push(json!({"type": "input_text", "text": text}));
            }
            ContentBlock::Image {
                source: ImageSource::Base64 { media_type, data },
            } => {
                content.push(json!({
                    "type": "input_image",
                    "image_url": format!("data:{media_type};base64,{data}"),
                }));
            }
            ContentBlock::Image {
                source: ImageSource::Url { url },
            } => {
                content.push(json!({
                    "type": "input_image",
                    "image_url": url,
                }));
            }
            ContentBlock::Document { .. } => {}
        }
    }

    if content.is_empty() {
        content.push(json!({"type": "input_text", "text": message.content}));
    }

    content
}

#[cfg(feature = "_http")]
pub fn model_stream_adapter<S>(sse: S) -> crate::stream::BoxModelStream
where
    S: futures_core::Stream<
            Item = Result<
                eventsource_stream::Event,
                eventsource_stream::EventStreamError<reqwest::Error>,
            >,
        > + Send
        + 'static,
{
    Box::pin(ResponsesModelStreamAdapter {
        inner: Box::pin(sse),
        pending: std::collections::VecDeque::new(),
        item_to_call_id: std::collections::HashMap::new(),
        saw_tool_call: false,
        error: None,
        saw_terminal: false,
    })
}

#[cfg(feature = "_http")]
struct ResponsesModelStreamAdapter {
    inner: std::pin::Pin<
        Box<
            dyn futures_core::Stream<
                    Item = Result<
                        eventsource_stream::Event,
                        eventsource_stream::EventStreamError<reqwest::Error>,
                    >,
                > + Send,
        >,
    >,
    pending: std::collections::VecDeque<ModelStreamDelta>,
    item_to_call_id: std::collections::HashMap<String, String>,
    saw_tool_call: bool,
    error: Option<String>,
    saw_terminal: bool,
}

#[cfg(feature = "_http")]
impl ResponsesModelStreamAdapter {
    fn handle_event(&mut self, data: &Value) {
        match data.get("type").and_then(Value::as_str).unwrap_or("") {
            "response.output_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.pending.push_back(ModelStreamDelta::Text {
                            delta: delta.to_string(),
                        });
                    }
                }
            }
            "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        self.pending.push_back(ModelStreamDelta::ThinkingDelta {
                            delta: delta.to_string(),
                        });
                    }
                }
            }
            "response.reasoning_text.done" | "response.reasoning_summary_text.done" => {
                if let Some(thinking) = data
                    .get("text")
                    .or_else(|| data.get("delta"))
                    .and_then(Value::as_str)
                {
                    self.pending.push_back(ModelStreamDelta::ThinkingDone {
                        thinking: thinking.to_string(),
                    });
                }
            }
            "response.output_item.added" => {
                if let Some(item) = data.get("item") {
                    self.remember_output_item(item);
                }
            }
            "response.function_call_arguments.delta" => {
                let call_id = self.call_id_from_event(data);
                if let (Some(call_id), Some(delta)) =
                    (call_id, data.get("delta").and_then(Value::as_str))
                {
                    self.pending.push_back(ModelStreamDelta::FunctionArguments {
                        call_id,
                        delta: delta.to_string(),
                    });
                }
            }
            "response.custom_tool_call_input.delta" => {
                let call_id = self.call_id_from_event(data);
                if let (Some(call_id), Some(delta)) =
                    (call_id, data.get("delta").and_then(Value::as_str))
                {
                    self.pending.push_back(ModelStreamDelta::FreeformInput {
                        call_id,
                        delta: delta.to_string(),
                    });
                }
            }
            "response.output_item.done" => {
                if let Some(item) = data.get("item") {
                    self.remember_output_item(item);
                    if let Some(call) = decode_tool_call(item) {
                        self.saw_tool_call = true;
                        self.pending
                            .push_back(ModelStreamDelta::ToolCallDone { call });
                    }
                }
            }
            "response.completed" | "response.incomplete" => {
                let response = data.get("response");
                let usage = decode_usage(response.and_then(|response| response.get("usage")));
                if usage.input_tokens != 0
                    || usage.output_tokens != 0
                    || usage.cache_creation_input_tokens.is_some()
                    || usage.cache_read_input_tokens.is_some()
                {
                    self.pending.push_back(ModelStreamDelta::Usage { usage });
                }
                let status = response
                    .and_then(|response| response.get("status"))
                    .and_then(Value::as_str);
                self.pending.push_back(ModelStreamDelta::Done {
                    stop_reason: stop_reason_from_status(status, self.saw_tool_call),
                });
                self.saw_terminal = true;
            }
            "error" | "response.failed" => {
                let msg = data
                    .get("message")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        data.get("response")
                            .and_then(|response| response.get("error"))
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                    })
                    .or_else(|| {
                        data.get("error")
                            .and_then(|error| error.get("message"))
                            .and_then(Value::as_str)
                    })
                    .unwrap_or("responses stream error")
                    .to_string();
                self.error = Some(msg);
            }
            _ => {}
        }
    }

    fn remember_output_item(&mut self, item: &Value) {
        let item_type = item.get("type").and_then(Value::as_str);
        if !matches!(item_type, Some("function_call" | "custom_tool_call")) {
            return;
        }
        let Some(call_id) = item.get("call_id").and_then(Value::as_str) else {
            return;
        };
        self.saw_tool_call = true;
        if let Some(item_id) = item.get("id").and_then(Value::as_str) {
            if !item_id.is_empty() {
                self.item_to_call_id
                    .insert(item_id.to_string(), call_id.to_string());
            }
        }
    }

    fn call_id_from_event(&self, data: &Value) -> Option<String> {
        data.get("call_id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                data.get("item_id")
                    .and_then(Value::as_str)
                    .and_then(|item_id| self.item_to_call_id.get(item_id))
                    .cloned()
            })
            .or_else(|| {
                data.get("item_id")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    }
}

#[cfg(feature = "_http")]
impl futures_core::Stream for ResponsesModelStreamAdapter {
    type Item = Result<ModelStreamDelta, crate::error::MotosanError>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;

        if let Some(delta) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(delta)));
        }
        if let Some(msg) = self.error.take() {
            self.saw_terminal = true;
            return Poll::Ready(Some(Err(crate::error::MotosanError::Stream(msg))));
        }

        loop {
            match self.inner.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(event))) => {
                    let data = event.data.trim();
                    if data.is_empty() || data == "[DONE]" {
                        continue;
                    }
                    let value: Value = match serde_json::from_str(data) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    self.handle_event(&value);
                    if let Some(delta) = self.pending.pop_front() {
                        return Poll::Ready(Some(Ok(delta)));
                    }
                    if let Some(msg) = self.error.take() {
                        self.saw_terminal = true;
                        return Poll::Ready(Some(Err(crate::error::MotosanError::Stream(msg))));
                    }
                }
                Poll::Ready(Some(Err(error))) => {
                    self.saw_terminal = true;
                    return Poll::Ready(Some(Err(crate::error::MotosanError::Stream(
                        error.to_string(),
                    ))));
                }
                Poll::Ready(None) => {
                    if !self.saw_terminal {
                        self.saw_terminal = true;
                        return Poll::Ready(Some(Err(
                            crate::error::MotosanError::IncompleteStream(
                                "responses stream ended without a terminal event".to_string(),
                            ),
                        )));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

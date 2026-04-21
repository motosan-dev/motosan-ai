# Tool Use & Multi-turn Loop

## Define Tools

**Python:**
```python
from motosan_ai import Tool

tools = [Tool(
    name="get_weather",
    description="Get current weather for a city",
    input_schema={
        "type": "object",
        "properties": {"city": {"type": "string", "description": "City name"}},
        "required": ["city"],
    },
)]
```

**Rust:**
```rust
use motosan_ai::Tool;
use serde_json::json;

let tools = vec![Tool {
    name: "get_weather".into(),
    description: Some("Get current weather for a city".into()),
    input_schema: Some(json!({
        "type": "object",
        "properties": {"city": {"type": "string"}},
        "required": ["city"]
    })),
}];
```

## ToolCall Fields

```
tool_call.id    — unique call ID (required for tool_result)
tool_call.name  — which tool was called
tool_call.input — parsed arguments (Python: dict, Rust: serde_json::Value)
```

## Multi-turn Tool Loop (Python)

```python
from motosan_ai import Client, Message, StopReason
import json

async def agent_loop(client, user_input: str, tools):
    messages = [Message.user(user_input)]

    while True:
        resp = await client.chat(messages, tools=tools)

        if resp.stop_reason != StopReason.tool_use or not resp.tool_calls:
            return resp.content  # done — final text response

        # Append assistant message with tool calls
        messages.append(Message.assistant_with_tool_calls(
            content=resp.content,
            tool_calls=resp.tool_calls,
        ))

        # Execute each tool and append results
        for tc in resp.tool_calls:
            result = await execute_tool(tc.name, tc.input)
            messages.append(Message.tool_result(
                tool_call_id=tc.id,
                content=json.dumps(result),
            ))

async def execute_tool(name: str, input: dict):
    if name == "get_weather":
        return {"temp": "22C", "condition": "sunny", "city": input["city"]}
    raise ValueError(f"Unknown tool: {name}")
```

## Multi-turn Tool Loop (Rust)

```rust
use motosan_ai::{Client, Message, ChatRequest, StopReason, Tool, MotosanError};

async fn agent_loop(
    client: &Client,
    input: &str,
    tools: Vec<Tool>,
) -> Result<String, MotosanError> {
    let mut messages = vec![Message::user(input)];

    loop {
        let request = ChatRequest::builder()
            .messages(messages.clone())
            .tools(tools.clone())
            .build();
        let resp = client.chat_with(request).await?;

        if resp.stop_reason != StopReason::ToolUse || resp.tool_calls.is_empty() {
            return Ok(resp.content);
        }

        messages.push(Message::assistant_with_tool_calls(
            &resp.content,
            resp.tool_calls.clone(),
        ));

        for tc in &resp.tool_calls {
            let result = execute_tool(&tc.name, &tc.input).await?;
            messages.push(Message::tool_result(&tc.id, &result));
        }
    }
}
```

## Streaming Tool Events

Event sequence for a single tool call:

```
tool_call_start  → tool_call_id + tool_call_name set
tool_call_args   → tool_call_args_delta (may arrive in multiple chunks)
tool_call_args   → ...
tool_call_end    → tool_call_id set, args complete
```

Buffer `tool_call_args_delta` until `tool_call_end`, then `json.loads()` / `serde_json::from_str()`.

Text events can be interleaved with tool call events — the model may emit text before/between tool calls.

### Streaming Tool Use (Python)

```python
import json

pending = {}  # tool_call_id -> {"name": str, "args": str}

async for event in client.stream(
    request.messages,
    tools=request.tools,
    system=request.system,
):
    match event.event_type:
        case "text":
            print(event.content, end="", flush=True)
        case "tool_call_start":
            pending[event.tool_call_id] = {
                "name": event.tool_call_name,
                "args": "",
            }
        case "tool_call_args":
            pending[event.tool_call_id]["args"] += event.tool_call_args_delta
        case "tool_call_end":
            tc = pending.pop(event.tool_call_id)
            args = json.loads(tc["args"])
            result = await execute_tool(tc["name"], args)
    if event.done:
        break
```

### Streaming Tool Use (Rust)

```rust
use std::collections::HashMap;
use motosan_ai::StreamEventType;
use futures_util::StreamExt;

let mut pending_name: HashMap<String, String> = HashMap::new();
let mut pending_args: HashMap<String, String> = HashMap::new();

let mut stream = client.stream_with(request).await?;
while let Some(event) = stream.next().await {
    match event.event_type {
        StreamEventType::Text => {
            print!("{}", event.content);
        }
        StreamEventType::ToolCallStart => {
            let id = event.tool_call_id.unwrap_or_default();
            pending_name.insert(id.clone(), event.tool_call_name.unwrap_or_default());
            pending_args.insert(id, String::new());
        }
        StreamEventType::ToolCallArgs => {
            let id = event.tool_call_id.unwrap_or_default();
            if let Some(args) = pending_args.get_mut(&id) {
                args.push_str(&event.tool_call_args_delta.unwrap_or_default());
            }
        }
        StreamEventType::ToolCallEnd => {
            let id = event.tool_call_id.unwrap_or_default();
            let name = pending_name.remove(&id).unwrap_or_default();
            let args_str = pending_args.remove(&id).unwrap_or_default();
            let args: serde_json::Value = serde_json::from_str(&args_str)?;
            // execute tool with name + args
        }
    }
    if event.done { break; }
}
```

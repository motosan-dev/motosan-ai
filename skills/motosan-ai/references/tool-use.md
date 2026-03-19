# Tool Use & Multi-turn Loop

## Define a Tool

**Python:**
```python
from motosan_ai import Tool

tools = [
    Tool(
        name="get_weather",
        description="Get current weather for a city",
        input_schema={
            "type": "object",
            "properties": {
                "city": {"type": "string", "description": "City name"}
            },
            "required": ["city"]
        }
    )
]
```

**Rust:**
```rust
use motosan_ai::{Tool, serde_json::json};

let tools = vec![Tool {
    name: "get_weather".into(),
    description: Some("Get current weather for a city".into()),
    input_schema: Some(json!({
        "type": "object",
        "properties": {
            "city": {"type": "string"}
        },
        "required": ["city"]
    })),
}];
```

## Multi-turn Tool Loop (Python)

```python
from motosan_ai import Client, Message, ChatRequest
import json

async def run_with_tools(client, user_input: str, tools):
    messages = [Message.user(user_input)]

    while True:
        resp = await client.chat_with(ChatRequest(
            messages=messages,
            tools=tools,
        ))

        if resp.stop_reason != "tool_use" or not resp.tool_calls:
            return resp.content   # done

        # Append assistant message with tool calls
        messages.append(Message.assistant_with_tool_calls(
            content=resp.content,
            tool_calls=resp.tool_calls,
        ))

        # Execute each tool and append results
        for tool_call in resp.tool_calls:
            result = await execute_tool(tool_call.name, tool_call.input)
            messages.append(Message.tool_result(
                tool_call_id=tool_call.id,
                content=json.dumps(result),
            ))

async def execute_tool(name: str, input: dict):
    if name == "get_weather":
        return {"temp": "22°C", "condition": "sunny", "city": input["city"]}
    raise ValueError(f"Unknown tool: {name}")
```

## Multi-turn Tool Loop (Rust)

```rust
use motosan_ai::{Client, Message, ChatRequest, StopReason};
use serde_json::json;

async fn run_with_tools(client: &Client, user_input: &str, tools: Vec<Tool>)
    -> Result<String, Box<dyn std::error::Error>>
{
    let mut messages = vec![Message::user(user_input)];

    loop {
        let resp = client.chat_with(ChatRequest {
            messages: messages.clone(),
            tools: Some(tools.clone()),
            ..Default::default()
        }).await?;

        if resp.stop_reason != StopReason::ToolUse || resp.tool_calls.is_empty() {
            return Ok(resp.content);
        }

        // Append assistant turn
        messages.push(Message::assistant_with_tool_calls(&resp.content, resp.tool_calls.clone()));

        // Execute tools and append results
        for tool_call in &resp.tool_calls {
            let result = execute_tool(&tool_call.name, &tool_call.input).await?;
            messages.push(Message::tool_result(&tool_call.id, &result.to_string()));
        }
    }
}
```

## ToolCall Fields

```python
tool_call.id      # str — unique call id (required for tool_result)
tool_call.name    # str — which tool to call
tool_call.input   # dict[str, Any] — parsed arguments
```

## Streaming Tool Events (event_type sequence)

```
tool_call_start  → event.tool_call_id, event.tool_call_name set
tool_call_args   → event.tool_call_args_delta accumulates JSON string
tool_call_end    → full args received, tool_call_id set
```

Buffer `tool_call_args_delta` until `tool_call_end`, then `json.loads()`.

# Streaming & ThinkStripper

## Basic Streaming (Python)

```python
from motosan_ai import Client, Message

client = Client.anthropic()

full_text = ""
async for event in client.stream([Message.user("Tell me a story")]):
    if event.event_type == "text" and event.content:
        print(event.content, end="", flush=True)
        full_text += event.content
    if event.done:
        break
print()
```

## Basic Streaming (Rust)

```rust
use motosan_ai::{Client, Provider, Message};
use futures_util::StreamExt;

let mut stream = client.stream(vec![Message::user("Tell me a story")]).await?;
let mut full_text = String::new();

while let Some(event) = stream.next().await {
    if !event.content.is_empty() {
        print!("{}", event.content);
        full_text.push_str(&event.content);
    }
    if event.done { break; }
}
```

**Note**: Rust `BoxStream` yields `StreamEvent` directly (not `Result<StreamEvent>`).

## StreamEvent Fields

| Field | Type | Set on |
|-------|------|--------|
| `event_type` | `"text"` / `"tool_call_start"` / `"tool_call_args"` / `"tool_call_end"` | all events |
| `content` | String | `text` events |
| `done` | bool | last event (True) |
| `tool_call_id` | Optional String | `tool_call_start`, `tool_call_args` (some providers), `tool_call_end` |
| `tool_call_name` | Optional String | `tool_call_start` |
| `tool_call_args_delta` | Optional String | `tool_call_args` |

## Streaming with Tools (Python)

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
            # append to messages and continue
    if event.done:
        break
```

## Streaming with Tools (Rust)

```rust
use motosan_ai::{ChatRequest, StreamEventType};
use futures_util::StreamExt;

let request = ChatRequest::builder()
    .messages(messages)
    .tools(tools)
    .build();

let mut stream = client.stream_with(request).await?;
while let Some(event) = stream.next().await {
    match event.event_type {
        StreamEventType::Text => print!("{}", event.content),
        StreamEventType::ToolCallStart => {
            let id = event.tool_call_id.unwrap_or_default();
            let name = event.tool_call_name.unwrap_or_default();
            // start accumulating args for this tool call
        },
        StreamEventType::ToolCallArgs => {
            let delta = event.tool_call_args_delta.unwrap_or_default();
            // append delta to accumulated args
        },
        StreamEventType::ToolCallEnd => {
            // json parse accumulated args, execute tool
        },
    }
    if event.done { break; }
}
```

## ThinkStripper

`<think>...</think>` blocks from reasoning models (DeepSeek, QwQ, MiniMax) are stripped **automatically** at `stream()` / `stream_with()` level. No manual setup needed.

### Why stateful?

`<think>` tags can span multiple streaming chunks:
```
chunk 1: "<thi"
chunk 2: "nk>reasoning"
chunk 3: "...</think>actual reply"
```
Per-chunk regex fails. `ThinkStripper` buffers internally and only emits safe text.

### Manual use (Python)

```python
from motosan_ai import ThinkStripper

stripper = ThinkStripper()

async for event in stream:
    if event.event_type == "text":
        clean = stripper.feed(event.content)
        if clean:
            print(clean, end="")

tail = stripper.flush()  # flush remaining buffer after stream ends
if tail:
    print(tail)
```

### Manual use (Rust)

```rust
use motosan_ai::think_stripper::ThinkStripper;

let mut stripper = ThinkStripper::new();

while let Some(event) = stream.next().await {
    let clean = stripper.feed(&event.content);
    if !clean.is_empty() {
        print!("{}", clean);
    }
}
let tail = stripper.flush();
if !tail.is_empty() {
    print!("{}", tail);
}
```

## Provider-Specific Streaming Notes

- **Anthropic**: `tool_call_id` comes on `content_block_start`, NOT on `content_block_delta` — SDK normalizes this so `tool_call_args` events may or may not carry `tool_call_id` depending on the provider. Use `tool_call_start`'s id as the canonical id.
- **Anthropic OAuth**: `chat()` auto-redirects to `stream()` (streaming required for OAuth). SDK collects stream result into `ChatResponse` transparently.
- **MiniMax**: May emit reasoning text before tool calls — ThinkStripper handles this automatically.
- **Ollama**: Streaming uses NDJSON (not SSE). SDK handles the format difference transparently.
- **OpenAI**: Standard SSE streaming. `tool_call_id` is present on all tool-related events.

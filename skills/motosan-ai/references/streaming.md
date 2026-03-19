# Streaming & ThinkStripper

## Basic Streaming (Python)

```python
from motosan_ai import Client, Message

client = Client.anthropic()
stream = await client.stream([Message.user("Tell me a story")])

full_text = ""
async for event in stream:
    if event.event_type == "text" and event.content:
        print(event.content, end="", flush=True)
        full_text += event.content
    if event.done:
        break

print()  # newline after stream
```

## event_type Values

| event_type | Meaning | Fields set |
|------------|---------|-----------|
| `"text"` | Text delta | `content` |
| `"tool_call_start"` | Tool call begins | `tool_call_id`, `tool_call_name` |
| `"tool_call_args"` | Tool args delta | `tool_call_args_delta`, `tool_call_id` |
| `"tool_call_end"` | Tool call complete | `tool_call_id` |

## Streaming with Tools (Python)

```python
pending_args: dict[str, str] = {}  # tool_call_id → accumulated args
pending_name: dict[str, str] = {}

async for event in await client.stream_with(req):
    match event.event_type:
        case "text":
            print(event.content, end="", flush=True)
        case "tool_call_start":
            pending_name[event.tool_call_id] = event.tool_call_name
            pending_args[event.tool_call_id] = ""
        case "tool_call_args":
            pending_args[event.tool_call_id] += event.tool_call_args_delta
        case "tool_call_end":
            call_id = event.tool_call_id
            args = json.loads(pending_args[call_id])
            result = await execute_tool(pending_name[call_id], args)
            # append to messages and continue loop
    if event.done:
        break
```

## Basic Streaming (Rust)

```rust
use futures_util::StreamExt;

let mut stream = client.stream(vec![Message::user("Tell me a story")]).await?;
let mut full_text = String::new();

while let Some(event) = stream.next().await {
    let event = event?;
    if event.is_text() && !event.content.is_empty() {
        print!("{}", event.content);
        full_text.push_str(&event.content);
    }
    if event.done { break; }
}
```

## ThinkStripper

`<think>` blocks in LLM output (e.g. from DeepSeek, QwQ) are stripped **automatically** at `client.stream()` level.

If you need manual control:

```python
from motosan_ai import ThinkStripper

stripper = ThinkStripper()

async for event in stream:
    if event.event_type == "text":
        clean = stripper.feed(event.content)  # strips <think>...</think>
        if clean:
            print(clean, end="")

# After stream ends — flush any remaining buffer
tail = stripper.flush()
if tail:
    print(tail)
```

**Why stateful?** `<think>` tags can span multiple chunks:
```
chunk 1: "<thi"
chunk 2: "nk>reasoning"
chunk 3: "...</think>actual reply"
```
Per-chunk regex fails here. `ThinkStripper` buffers internally and only emits safe text.

## Ollama / MiniMax Notes

- **Ollama**: streaming uses NDJSON (not SSE). SDK handles automatically.
- **MiniMax**: may emit reasoning text before tool calls — ThinkStripper handles this.
- **Anthropic**: `tool_call_id` comes on `content_block_start`, NOT on `content_block_delta` — SDK normalizes this.

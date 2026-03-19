# motosan-ai Python API Reference

## Client Construction

```python
from motosan_ai import Client, Provider

# Factory methods (read env var automatically)
client = Client.anthropic(model="claude-3-5-sonnet-20241022")
client = Client.openai(model="gpt-4o")
client = Client.minimax(model="MiniMax-Text-01")
client = Client.ollama(model="llama3.2", base_url="http://localhost:11434")
```

## Core Methods

### `chat()` — single-turn

```python
resp = await client.chat([Message.user("Hello")])
# resp.content: str
# resp.usage.input_tokens: int
# resp.usage.output_tokens: int
# resp.stop_reason: StopReason
```

### `chat_with()` — full control

```python
from motosan_ai import ChatRequest, Message

req = ChatRequest(
    messages=[Message.user("Hello")],
    model="claude-3-5-sonnet-20241022",   # override default
    system="You are a helpful assistant.",
    temperature=0.7,
    max_tokens=1024,
    tools=[...],                           # see tool-use.md
    provider_options={"extra_key": "val"}, # escape hatch
)
resp = await client.chat_with(req)
```

### `stream()` — streaming text

```python
async for event in await client.stream([Message.user("Tell me a story")]):
    if event.event_type == "text":
        print(event.content, end="", flush=True)
    if event.done:
        break
```

### `stream_with()` — streaming + tools + full control

```python
async for event in await client.stream_with(ChatRequest(
    messages=messages,
    tools=tools,
    system="...",
)):
    # handle events — see streaming.md
```

## Message Helpers

```python
from motosan_ai import Message, Role

Message.user("Hello")
Message.assistant("Hi there")
Message.system("You are a helpful assistant")
Message.assistant_with_tool_calls("", tool_calls=[...])
Message.tool_result(tool_call_id="call_123", content='{"result": 42}')

# Manual construction
Message(role=Role.user, content="Hello")
```

## StreamEvent Fields

```python
event.event_type   # "text" | "tool_call_start" | "tool_call_args" | "tool_call_end"
event.content      # str — text delta or empty
event.done         # bool — True on last event
event.tool_call_id    # str | None
event.tool_call_name  # str | None (on tool_call_start)
event.tool_call_args_delta  # str | None (on tool_call_args)
```

## RetryPolicy

```python
from motosan_ai import RetryPolicy

client = Client.anthropic(
    retry_policy=RetryPolicy(
        max_retries=3,
        base_delay_ms=500,
        max_delay_ms=10000,
        jitter=True,
    )
)
```

## ThinkStripper

Applied automatically at `client.stream()` level — `<think>` blocks are stripped transparently.

Manual use:
```python
from motosan_ai import ThinkStripper

stripper = ThinkStripper()
clean = stripper.feed("<think>reasoning...</think>Hello")  # → "Hello"
tail  = stripper.flush()  # call at stream end to flush buffer
```

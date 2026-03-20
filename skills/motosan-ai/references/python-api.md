# motosan-ai Python API Reference

## Install

```bash
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[anthropic,openai,ollama,minimax]"
```

## Public Exports

```python
from motosan_ai import (
    Client, Provider,
    Message, Role, Tool, ToolCall,
    ChatRequest, ChatResponse,
    Usage, StopReason, StreamEvent,
    MotosanError, AuthError, RateLimitError,
    InvalidRequestError, ConfigError, ProviderError,
    NetworkError, StreamError,
)
```

## Client Construction

```python
from motosan_ai import Client

# Factory methods (read env var automatically)
client = Client.anthropic()                           # ANTHROPIC_API_KEY
client = Client.anthropic(model="claude-opus-4-6")    # override model
client = Client.openai(model="gpt-4o")                # OPENAI_API_KEY
client = Client.minimax()                              # MINIMAX_API_KEY
client = Client.ollama(model="llama3.2")               # local, no key needed
client = Client.ollama(model="qwq", base_url="http://localhost:11434")
```

## Core Methods

### `chat()` — single turn

```python
resp = await client.chat([Message.user("Hello")])
resp.content         # str
resp.stop_reason     # StopReason: "end_turn" | "max_tokens" | "tool_use" | "stop"
resp.usage           # Usage: input_tokens, output_tokens
resp.tool_calls      # list[ToolCall] — empty if no tool use
resp.model           # str — model used
```

### `chat_with()` — full control

```python
from motosan_ai import ChatRequest, Message

resp = await client.chat_with(ChatRequest(
    messages=[Message.user("Hello")],
    model="claude-sonnet-4-6",             # optional override
    system="You are a helpful assistant.",   # optional
    temperature=0.7,                        # optional
    max_tokens=1024,                        # optional
    tools=[...],                            # optional — see tool-use.md
    provider_options={"key": "val"},        # optional escape hatch
))
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
    system="You are helpful",
)):
    # handle events — see streaming.md
```

## Message Helpers

```python
from motosan_ai import Message

Message.user("Hello")
Message.assistant("Hi there")
Message.system("You are a helpful assistant")
Message.assistant_with_tool_calls("Let me check", tool_calls=[...])
Message.tool_result(tool_call_id="call_123", content='{"result": 42}')

# Manual construction
from motosan_ai import Role
Message(role=Role.user, content="Hello")
```

## StreamEvent Fields

```python
event.event_type           # "text" | "tool_call_start" | "tool_call_args" | "tool_call_end"
event.content              # str — text delta or empty
event.done                 # bool — True on last event
event.tool_call_id         # str | None
event.tool_call_name       # str | None (on tool_call_start)
event.tool_call_args_delta # str | None (on tool_call_args)
```

## RetryPolicy

```python
from motosan_ai import RetryPolicy

client = Client.anthropic(
    retry_policy=RetryPolicy(
        max_retries=3,        # default 3
        base_delay_ms=500,    # default 100
        max_delay_ms=10000,   # default 2000
        jitter=True,          # default True
    )
)
```

Retries on: 429, 5xx, timeout/connect errors. Exponential backoff with jitter.

## Error Handling

```python
from motosan_ai import MotosanError, AuthError, RateLimitError

try:
    resp = await client.chat([Message.user("Hi")])
except RateLimitError as e:
    print(f"Rate limited: {e}")
except AuthError as e:
    print(f"Auth failed: {e}")
except MotosanError as e:
    print(f"Error: {e}")
```

Error hierarchy: `MotosanError` is the base class.
Subclasses: `AuthError`, `RateLimitError`, `InvalidRequestError`, `ConfigError`, `ProviderError`, `NetworkError`, `StreamError`

## ThinkStripper

Applied automatically in `stream()` / `stream_with()`. Manual use:

```python
from motosan_ai import ThinkStripper

stripper = ThinkStripper()
clean = stripper.feed("<think>reasoning...</think>Hello")  # → "Hello"
tail = stripper.flush()  # call at stream end to flush buffer
```

Handles `<think>` tags split across multiple chunks (stateful buffer).

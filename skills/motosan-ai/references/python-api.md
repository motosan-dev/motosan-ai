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

# Claude Code CLI backend (local binary, no API key)
from motosan_ai import ClaudeCodeClient
claude = ClaudeCodeClient().model("sonnet").permission_mode("plan")
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

### `chat()` — full control via keyword args

```python
resp = await client.chat(
    [Message.user("Hello")],
    system="You are a helpful assistant.",
    temperature=0.7,
    max_tokens=1024,
    tools=[...],
    provider_options={"key": "val"},
)
```

### `stream()` — streaming text

```python
async for event in client.stream([Message.user("Tell me a story")]):
    if event.event_type == "text":
        print(event.content, end="", flush=True)
    if event.done:
        break
```

### `stream()` with tools

```python
async for event in client.stream(
    messages,
    tools=tools,
    system="You are helpful",
):
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
event.event_type           # "text" | "usage" | "thinking" | "tool_call_start" | "tool_call_args" | "tool_call_end"
event.content              # str — text delta or empty
event.done                 # bool — True on last event
event.tool_call_id         # str | None
event.tool_call_name       # str | None (on tool_call_start)
event.tool_call_args_delta # str | None (on tool_call_args)
event.usage                # Usage | None — emitted by Anthropic/Gemini and Claude Code stream result events
event.stop_reason          # StopReason | None
```

## Claude Code CLI Backend

```python
from motosan_ai import ChatRequest, ClaudeCodeClient, Message

client = (
    ClaudeCodeClient()
    .model("sonnet")
    .system_prompt("Be concise.")
    .permission_mode("plan")
    .effort("low")
    .allow_tool("Read")
    .max_budget_usd(2.5)
)

resp = await client.chat(ChatRequest(messages=[Message.user("Hi")]))

async for event in client.stream(ChatRequest(messages=[Message.user("Count to 3")])):
    if event.event_type == "usage":
        print(event.usage)
```

Builder methods added in Python v0.9.0: `bare`, `system_prompt`, `permission_mode`, `effort`, `fallback_model`, `add_dir(s)`, `allow_tool` / `allowed_tools`, `disallow_tool` / `disallowed_tools`, `mcp_config(s)`, `strict_mcp_config`, `settings`, `setting_source(s)`, `session_id`, `resume`, `continue_latest`, `fork_session`, `plugin_dir(s)`, `agent`, `no_session_persistence`, and `max_budget_usd`.

Wire notes: `system_prompt(...)` maps to `--system-prompt`; request/system messages map to `--append-system-prompt`. Tool allow/deny lists and MCP configs are variadic CLI arguments, not comma-joined. Streams emit a `usage` event before terminal `done` when Claude Code reports usage.

## Retry

```python
# default retries = 3
client = Client.anthropic(api_key="...", max_retries=3)

# disable retries
client = Client.anthropic(api_key="...", max_retries=0)
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

Applied automatically in `stream()`. Manual use:

```python
from motosan_ai import ThinkStripper

stripper = ThinkStripper()
clean = stripper.feed("<think>reasoning...</think>Hello")  # → "Hello"
tail = stripper.flush()  # call at stream end to flush buffer
```

Handles `<think>` tags split across multiple chunks (stateful buffer).

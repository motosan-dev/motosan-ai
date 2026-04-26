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
    ApprovalMode, CodexCliClient, ClaudeCodeClient, GeminiCliClient,
    SandboxMode, LocalProvider,
    Message, Role, Tool, ToolCall,
    ChatRequest, ChatResponse,
    Usage, StopReason, StreamEvent,
    collect_stream,
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
client = Client.gemini_code_assist(access_token="ya29...", project_id="gcp-project")

# CLI backends (local binary, no API key)
from motosan_ai import ApprovalMode, ClaudeCodeClient, CodexCliClient, GeminiCliClient, SandboxMode
claude = ClaudeCodeClient().model("sonnet").permission_mode("plan")
codex = CodexCliClient().sandbox(SandboxMode.workspace_write).model("gpt-5.1-codex")
gemini_cli = GeminiCliClient().approval_mode(ApprovalMode.plan).model("gemini-2.5-pro")
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

### `chat_with(request)` — full ChatRequest passthrough

```python
from motosan_ai import ChatRequest, Message, ToolChoice

req = (
    ChatRequest.builder()
    .message(Message.user("Hello"))
    .thinking(1024)
    .tool_choice(ToolChoice.auto())
    .build()
)
resp = await client.chat_with(req)
```

Use when you need fields not exposed by `chat()` kwargs: `thinking`,
`tool_choice`, `mcp_servers`, `system_blocks`, `stop_sequences`. If
`request.model` is unset, `client.model` is used.

### `stream_with(request)` — full ChatRequest streaming

```python
async for event in client.stream_with(req):
    if event.content:
        print(event.content, end="")
```

Same use case and model fallback as `chat_with()`, with the same retry behavior
as `stream()`.

### `stream_collect()` — stream into ChatResponse

```python
resp = await client.stream_collect(
    [Message.user("Hello")],
    system="You are concise.",
    max_tokens=256,
)
```

Convenience wrapper around `stream()` plus `collect_stream()`. Returns a full
`ChatResponse` with text, thinking, tool calls, usage, stop reason, and model
fallback from `client.model`.

### `stream_collect_with(request)` — full ChatRequest stream collection

```python
req = ChatRequest.builder().message(Message.user("Hello")).thinking(1024).build()
resp = await client.stream_collect_with(req)
```

Use for stream-to-response assembly with the full `ChatRequest` surface.
Response `model` falls back to `request.model or client.model` when the stream
omits it.

### `collect_stream(events)` — top-level helper

```python
from motosan_ai import collect_stream

resp = await collect_stream(event_iterator)
```

Collects any `AsyncIterator[StreamEvent]` into `ChatResponse`; handles text,
thinking, tool call start/args/end, usage, and terminal stop reason.

> `Client.chat_sync()` is deprecated in Python v0.10.0 and will be removed in
> v0.11.0. Use `asyncio.run(client.chat(...))` instead.

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
event.usage                # Usage | None — emitted by HTTP providers and CLI terminal result events
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

## Codex CLI Backend

```python
from motosan_ai import ChatRequest, CodexCliClient, Message, SandboxMode

client = (
    CodexCliClient()
    .sandbox(SandboxMode.workspace_write)
    .model("gpt-5.1-codex")
    .profile("work")
    .config_override("approval_policy", "never")
)

resp = await client.chat(ChatRequest(messages=[Message.user("Hi")]))

async for event in client.stream(ChatRequest(messages=[Message.user("Count to 3")])):
    if event.event_type == "usage":
        print(event.usage)
```

Builder methods added in Python v0.9.1: `agent_mode`, `dangerously_bypass_approvals_and_sandbox`, `oss`, `ephemeral`, `sandbox`, `local_provider`, `model`, `profile`, `cd`, `add_dir`, `enable_feature`, `disable_feature`, and `config_override`.

Wire notes: `CodexCliClient` runs `codex exec --json --skip-git-repo-check ... -`, uses `CODEX_PATH` for binary resolution, does not require an SDK API key, and maps `turn.completed.usage.cached_input_tokens` to `Usage.cache_read_input_tokens`.

## Gemini CLI Backend

```python
from motosan_ai import ApprovalMode, ChatRequest, GeminiCliClient, Message

client = (
    GeminiCliClient()
    .model("gemini-2.5-pro")
    .approval_mode(ApprovalMode.plan)
    .include_dir("/tmp/workspace")
)

resp = await client.chat(ChatRequest(messages=[Message.user("Hi")]))

async for event in client.stream(ChatRequest(messages=[Message.user("Count to 3")])):
    if event.event_type == "usage":
        print(event.usage)
```

Builder methods added in Python v0.9.2: `model`, `yolo`, `sandbox`, `approval_mode`, `include_dir(s)`, `extension(s)`, `allowed_mcp_server(s)`, and `resume`.

Wire notes: `GeminiCliClient` runs `gemini -p "" -o stream-json ...` with no trailing `-`; uses `GEMINI_CLI_PATH`; merges system prompts into stdin via `\n\n`; and maps `result.stats.cached` to `Usage.cache_read_input_tokens`.

## Gemini Code Assist + OAuth

```python
from motosan_ai import ChatRequest, Client, Message
from motosan_ai.oauth import google_gemini_config, login, save_token

# One-time browser login; cache is written 0600.
token = await login(google_gemini_config())
save_token(token)

client = Client.gemini_code_assist(
    access_token=token.access_token,
    project_id="my-gcp-project",
)
resp = await client.chat([Message.user("Hi")])
```

`GeminiCodeAssistProvider` wraps the normal Gemini request body in the Code Assist envelope and maps `cachedContentTokenCount` to `Usage.cache_read_input_tokens` after subtracting it from `input_tokens`.

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

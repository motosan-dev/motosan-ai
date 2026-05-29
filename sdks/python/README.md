# motosan-ai (Python SDK)

Multi-provider Python SDK for Anthropic, OpenAI, MiniMax, Ollama, Gemini, Gemini Code Assist, and CLI backends.
All HTTP providers use `httpx` directly — no official provider SDKs required.
Also includes `ClaudeCodeClient`, `CodexCliClient`, and `GeminiCliClient` backends that shell out to local CLI binaries.

## Installation

```bash
pip install motosan-ai
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[openai]"
pip install "motosan-ai[minimax]"
pip install "motosan-ai[ollama]"
pip install "motosan-ai[gemini]"
pip install "motosan-ai[full]"
```

## Quick Start

```python
import asyncio

from motosan_ai import Client


async def main() -> None:
    client = Client.anthropic(api_key="sk-ant-...", model="claude-sonnet-4-6")
    response = await client.chat([
        {"role": "user", "content": "Hello"},
    ])
    print(response.content)


asyncio.run(main())
```

## Tool Use (Multi-turn)

```python
import asyncio

from motosan_ai import Client, Message, Tool


def get_weather(city: str) -> str:
    return f"Sunny in {city}"


async def main() -> None:
    client = Client.anthropic(api_key="sk-ant-...")

    tools = [
        Tool(
            name="get_weather",
            description="Get current weather",
            input_schema={
                "type": "object",
                "properties": {"city": {"type": "string"}},
                "required": ["city"],
            },
        )
    ]

    messages = [Message.user("What's the weather in Tokyo?")]
    response = await client.chat(messages, tools=tools)

    if response.tool_calls:
        tc = response.tool_calls[0]
        result = get_weather(tc.input["city"])

        messages += [
            Message.assistant_with_tool_calls("", response.tool_calls),
            Message.tool_result(tc.id, result),
        ]
        final = await client.chat(messages, tools=tools)
        print(final.content)


asyncio.run(main())
```

## Streaming

```python
import asyncio

from motosan_ai import Client, Message


async def main() -> None:
    client = Client.openai(api_key="sk-...", model="gpt-4o")

    async for event in client.stream([Message.user("Write a haiku about rain")]):
        if event.content:
            print(event.content, end="")
        if event.done:
            break


asyncio.run(main())
```

## Full `ChatRequest` Control

`Client.chat()` exposes the common kwargs (`tools`, `system`, `temperature`,
`max_tokens`, `provider_options`). For fields like `tool_choice`, `thinking`,
`mcp_servers`, `system_blocks`, or `stop_sequences`, use `chat_with()` or
`stream_with()` with `ChatRequest.builder()`:

```python
from motosan_ai import ChatRequest, Client, Message, ToolChoice

client = Client.anthropic()

req = (
    ChatRequest.builder()
    .message(Message.user("Solve: 13 * 17"))
    .thinking(2048)
    .tool_choice(ToolChoice.auto())
    .system_cached("Show concise reasoning.")
    .build()
)
resp = await client.chat_with(req)
print(resp.thinking)
print(resp.content)

async for event in client.stream_with(req):
    if event.content:
        print(event.content, end="")
```

## Streaming → Assembled Response

`stream_collect()` and `stream_collect_with()` drive a stream to completion and
return a `ChatResponse`. Use them when a provider path is stream-first or when
you want a complete response while preserving streaming transport behavior.

```python
from motosan_ai import ChatRequest, Client, Message

client = Client.anthropic()

# Convenience kwargs path
resp = await client.stream_collect([Message.user("hi")])

# Full ChatRequest path
req = ChatRequest.builder().message(Message.user("hi")).thinking(1024).build()
resp = await client.stream_collect_with(req)
```

The lower-level helper is also exported for custom stream callers:

```python
from motosan_ai import collect_stream

resp = await collect_stream(event_iterator)
```

## Retry

All API calls automatically retry on transient errors (429 rate limit, 5xx server errors, network timeouts). Default: 3 retries with exponential backoff (100ms, 200ms, 400ms).

```python
# Default: 3 retries
client = Client.anthropic(api_key="...")

# Disable retry
client = Client.anthropic(api_key="...", max_retries=0)

# Custom retry count
client = Client.anthropic(api_key="...", max_retries=5)
```

Respects `Retry-After` header when present.

## Sync usage

The SDK is async-only. Wrap the call in `asyncio.run`:

```python
import asyncio
from motosan_ai import Client, Message

client = Client.minimax(api_key="...")
response = asyncio.run(client.chat([Message.user("Hello from sync")]))
print(response.content)
```

## Providers

### Anthropic

```python
from motosan_ai import Client

client = Client.anthropic(api_key="sk-ant-...", model="claude-sonnet-4-6")
opus = Client.anthropic(api_key="sk-ant-...", model="claude-opus-4-8")
```

For Opus 4.8/4.7/4.6, `ThinkingConfig` uses Anthropic adaptive thinking (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) instead of the older budget-token shape, matching pi. Budget-based models still send `display: "summarized"` for OAuth thinking streams.

### OpenAI

```python
from motosan_ai import Client

client = Client.openai(api_key="sk-...", model="gpt-4o")
```

### MiniMax

```python
from motosan_ai import Client

client = Client.minimax(api_key="...", model="MiniMax-M1")
```

### Ollama

```python
from motosan_ai import Client

# OpenAI-compatible mode (default)
client = Client.ollama(model="llama3.2")

# Native Ollama API mode (supports think/keep_alive/num_ctx)
client = Client.ollama(model="llama3.2", native=True, think=True)
```

## Claude Code CLI Backend

```python
from motosan_ai import ChatRequest, ClaudeCodeClient, Message

client = (
    ClaudeCodeClient()
    .model("sonnet")
    .system_prompt("Be concise.")          # --system-prompt
    .permission_mode("plan")               # --permission-mode plan
    .effort("low")                         # --effort low
    .allow_tool("Read")                    # --allowed-tools Read
    .max_budget_usd(2.5)                   # --max-budget-usd 2.5
)

response = await client.chat(
    ChatRequest(messages=[Message.user("Hello from claude CLI")])
)
print(response.content)

async for event in client.stream(
    ChatRequest(messages=[Message.user("Stream a short poem")])
):
    if event.event_type == "usage":
        print(f"\nusage={event.usage}")
    elif event.content:
        print(event.content, end="")
    if event.done:
        break
```

Notes:
- Uses `CLAUDE_CODE_PATH` env var or `claude` in `PATH`.
- Live tests are opt-in: set `MOTOSAN_RUN_CLAUDE_CODE_LIVE=1`.
- `tool_calls` is always empty (tools run inside CLI).
- `agent_mode(True)` enables `--dangerously-skip-permissions` + JSON output parsing.
- Python v0.9.0 adds full Rust-compatible Claude Code flag coverage: `bare`, `system_prompt`, `permission_mode`, `effort`, `fallback_model`, `add_dir(s)`, `allow_tool` / `allowed_tools`, `disallow_tool` / `disallowed_tools`, `mcp_config(s)`, `strict_mcp_config`, `settings`, `setting_source(s)`, `session_id`, `resume`, `continue_latest`, `fork_session`, `plugin_dir(s)`, `agent`, `no_session_persistence`, and `max_budget_usd`.
- `system_prompt(...)` maps to `--system-prompt`; system messages / `ChatRequest.system` are appended with `--append-system-prompt`.
- `allowed_tools`, `disallowed_tools`, and `mcp_configs` are variadic CLI arguments, matching Rust (`--allowed-tools Read Bash`, not comma-joined).
- Streaming emits `StreamEvent(event_type="usage")` before the terminal `done` event when Claude Code includes token usage in the NDJSON `result` event.

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

response = await client.chat(ChatRequest(messages=[Message.user("Hello from codex CLI")]))
print(response.content)

async for event in client.stream(ChatRequest(messages=[Message.user("Stream a short answer")])):
    if event.event_type == "usage":
        print(event.usage)
    elif event.content:
        print(event.content, end="")
```

Notes:
- Uses `CODEX_PATH` env var or `codex` in `PATH`.
- No API key is required by the SDK; the `codex` binary handles its own auth.
- Live tests are opt-in: set `MOTOSAN_RUN_CODEX_LIVE=1`; override the live-test model with `MOTOSAN_CODEX_MODEL` (default `gpt-5.1-codex`).
- Available through both direct `CodexCliClient()` and unified `Client.codex_cli()` / `Provider.codex_cli` dispatch.
- Python v0.9.1 adds Rust-compatible flag coverage: `agent_mode`, `dangerously_bypass_approvals_and_sandbox`, `oss`, `ephemeral`, `sandbox`, `local_provider`, `model`, `profile`, `cd`, `add_dir`, `enable_feature`, `disable_feature`, and `config_override`.
- Streaming emits `StreamEvent(event_type="usage")` before terminal `done` when Codex includes token usage in `turn.completed`; `cached_input_tokens` maps to `Usage.cache_read_input_tokens`.

## Gemini CLI Backend

```python
from motosan_ai import ApprovalMode, ChatRequest, GeminiCliClient, Message

client = (
    GeminiCliClient()
    .model("gemini-2.5-pro")
    .approval_mode(ApprovalMode.plan)
    .include_dir("/tmp/workspace")
)

response = await client.chat(ChatRequest(messages=[Message.user("Hello from gemini CLI")]))
print(response.content)

async for event in client.stream(ChatRequest(messages=[Message.user("Stream a short answer")])):
    if event.event_type == "usage":
        print(event.usage)
    elif event.content:
        print(event.content, end="")
```

Notes:
- Uses `GEMINI_CLI_PATH` env var or `gemini` in `PATH`.
- No API key is required by the SDK; the `gemini` binary handles its own auth.
- Available through both direct `GeminiCliClient()` and unified `Client.gemini_cli()` / `Provider.gemini_cli` dispatch.
- Python v0.9.2 adds Rust-compatible flag coverage: `model`, `yolo`, `sandbox`, `approval_mode`, `include_dir(s)`, `extension(s)`, `allowed_mcp_server(s)`, and `resume`.
- Gemini CLI takes prompt input via stdin with no trailing `-` argv marker; system prompts are prepended to stdin with a blank line.
- Live tests are opt-in: set `MOTOSAN_RUN_GEMINI_CLI_LIVE=1`.

## Gemini Code Assist + Google OAuth

```python
from motosan_ai import ChatRequest, Client, Message

client = Client.gemini_code_assist(
    access_token="ya29...",
    project_id="my-gcp-project",
)
resp = await client.chat([Message.user("Hello from Code Assist")])
```

OAuth helpers are available under `motosan_ai.oauth`:

```python
import asyncio
from motosan_ai.oauth import gemini_config, login, save_token

async def main():
    token = await login(gemini_config())
    save_token(token)

asyncio.run(main())
```

### Anthropic OAuth (Claude Pro/Max)

`claude_pro_max_config()` drives a PKCE login against `claude.ai` and returns
an `sk-ant-oat01-*` token usable directly with `AnthropicProvider`:

```python
import asyncio
from motosan_ai import AnthropicProvider
from motosan_ai.oauth import claude_pro_max_config, login

async def main():
    token = await login(claude_pro_max_config())
    provider = AnthropicProvider(api_key=token.access_token)
    # AnthropicProvider auto-detects the sk-ant-oat01- prefix.

asyncio.run(main())
```

**⚠️ ToS disclosure:** this uses the OAuth `client_id` registered by
Anthropic's Claude Code CLI. The resulting token authenticates **as a Claude
Code CLI session**. Anthropic has not published this `client_id` for
third-party use; usage for purposes other than running `claude` may be
subject to change, rate limited, or in violation of Anthropic's terms. You
are responsible for compliance. If you have an `sk-ant-api*` key, prefer it.

Notes:
- `GeminiCodeAssistProvider` targets `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`.
- The provider takes an access token + `project_id`; OAuth helpers are separate and reusable.
- Token cache path: `~/.config/motosan-ai/google-tokens.json`, written with `0600` permissions.
- Live tests are opt-in: set `MOTOSAN_RUN_CODE_ASSIST_LIVE=1` and `GOOGLE_PROJECT_ID`, with a cached token present.

## Anthropic Auth Matrix

- `sk-ant-api*` or regular Anthropic API key → `x-api-key` header
- `sk-ant-oat01*` OAuth token → OAuth mode:
  - `Authorization: Bearer <token>` header (via httpx directly)
  - `anthropic-beta: claude-code-20250219,oauth-2025-04-20,...` headers
  - `user-agent: claude-code/<version>` + `x-app: cli` identity headers
  - System prompt sent as array of blocks (prefix + user system)
  - Claude Code system prompt prefix auto-injected
  - `chat()` auto-redirects to `stream()` and collects result (including tool_calls)

The SDK auto-detects token type by prefix — pass either into `Client.anthropic(api_key=...)`.

```python
from motosan_ai import Client

# Standard API key
client = Client.anthropic(api_key="sk-ant-api03-...")

# OAuth token (auto-detected, same interface)
client = Client.anthropic(api_key="sk-ant-oat01-...")
```

## HTTP Client

All providers use `httpx` directly — no official provider SDKs (`anthropic`, `openai`) required.
This keeps the dependency tree minimal and gives full control over auth, headers, and SSE parsing.

## Requirements

- Python 3.11+
- One provider API key:
  - `ANTHROPIC_API_KEY` (standard API key or OAuth token)
  - `OPENAI_API_KEY`
  - `MINIMAX_API_KEY`
  - Ollama: no key needed (local)

## Testing

```bash
# Unit tests (mock, no API needed)
uv run pytest sdks/python/tests/ -q --ignore=sdks/python/tests/integration/

# Live integration tests (requires ANTHROPIC_API_KEY)
ANTHROPIC_API_KEY=... uv run pytest sdks/python/tests/integration/test_anthropic_live.py -v
```

## Publishing

Automated via `publish-python.yml` on `python-v*` tag push → PyPI.

```bash
# Tag and push to trigger publish
git tag -a python-vX.Y.Z -m "python-vX.Y.Z — summary"
git push origin python-vX.Y.Z

# Manual (emergency)
uv build --out-dir dist && uv publish dist/*
```

Rust and Python SDKs are versioned independently.

## Development

```bash
uv sync --extra full --extra dev
uv run ruff check motosan_ai/
uv run pytest -q
```

## For AI Agents

If you're an AI coding assistant, fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt) for a quick-start guide with API examples, tool use patterns, and streaming setup.

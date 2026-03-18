# motosan-ai (Python SDK)

Multi-provider Python SDK for Anthropic, OpenAI, MiniMax, and Ollama.
All providers use `httpx` directly — no official provider SDKs required.

## Installation

```bash
pip install motosan-ai
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[openai]"
pip install "motosan-ai[minimax]"
pip install "motosan-ai[ollama]"
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

## Sync Wrapper

```python
from motosan_ai import Client, Message

client = Client.minimax(api_key="...")
response = client.chat_sync([Message.user("Hello from sync")])
print(response.content)
```

## Providers

### Anthropic

```python
from motosan_ai import Client

client = Client.anthropic(api_key="sk-ant-...", model="claude-sonnet-4-6")
```

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
client = Client.ollama(model="llama3.2", ollama_native=True, ollama_think=True)
```

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

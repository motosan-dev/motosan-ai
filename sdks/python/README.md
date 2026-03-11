# motosan-ai (Python SDK)

Multi-provider Python SDK for Anthropic, OpenAI, and MiniMax.

## Installation

```bash
pip install motosan-ai
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[openai]"
pip install "motosan-ai[minimax]"
pip install "motosan-ai[full]"
```

## Quick Start

```python
import asyncio

from motosan_ai import Client


async def main() -> None:
    client = Client.anthropic(api_key="sk-ant-...", model="claude-3-5-sonnet-latest")
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
    client = Client.openai(api_key="sk-...", model="gpt-4.1-mini")

    async for event in client.stream([Message.user("Write a haiku about rain")]):
        if event.content:
            print(event.content, end="")
        if event.done:
            break


asyncio.run(main())
```

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

client = Client.anthropic(api_key="sk-ant-...", model="claude-3-5-sonnet-latest")
```

### OpenAI

```python
from motosan_ai import Client

client = Client.openai(api_key="sk-...", model="gpt-4.1-mini")
```

### MiniMax

```python
from motosan_ai import Client

client = Client.minimax(api_key="...", model="MiniMax-M1")
```

## Requirements

- Python 3.11+
- One provider API key:
  - `ANTHROPIC_API_KEY`
  - `OPENAI_API_KEY`
  - `MINIMAX_API_KEY`

## Development

```bash
uv sync --extra full --extra dev
uv run ruff check motosan_ai/
uv run pytest -q
uv build --out-dir dist
uv publish --dry-run dist/*
```

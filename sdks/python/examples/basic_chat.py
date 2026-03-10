"""
Basic chat example.

Usage:
    ANTHROPIC_API_KEY=sk-ant-... python examples/basic_chat.py
    OPENAI_API_KEY=sk-...       python examples/basic_chat.py openai
"""
import asyncio
import sys
from motosan_ai import Client

async def main():
    provider = sys.argv[1] if len(sys.argv) > 1 else "anthropic"

    client = Client(provider=provider)
    response = await client.chat([
        {"role": "user", "content": "What is the capital of France? Answer in one sentence."}
    ])

    print(f"Response: {response.content}")
    print(f"Model: {response.model}")
    print(f"Tokens: {response.usage.input_tokens} in, {response.usage.output_tokens} out")

asyncio.run(main())

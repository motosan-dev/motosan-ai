"""
Streaming example.

Usage:
    ANTHROPIC_API_KEY=sk-ant-... python examples/streaming.py
"""
import asyncio
from motosan_ai import Client

async def main():
    client = Client(provider="anthropic")

    print("Streaming: ", end="", flush=True)
    async for event in client.stream([
        {"role": "user", "content": "Count from 1 to 10, one number per line."}
    ]):
        if not event.done:
            print(event.content, end="", flush=True)
    print()

asyncio.run(main())

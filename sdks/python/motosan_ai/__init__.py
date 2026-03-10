"""
motosan-ai — Multi-provider AI SDK.

Quick start::

    from motosan_ai import Client

    client = Client(provider="anthropic")
    response = await client.chat([{"role": "user", "content": "Hello"}])
    print(response.content)
"""

from motosan_ai.client import Client
from motosan_ai.types import (
    ChatRequest,
    ChatResponse,
    Message,
    Role,
    StopReason,
    StreamEvent,
    Tool,
    Usage,
)
from motosan_ai.error import (
    MotosanError,
    AuthError,
    RateLimitError,
    InvalidRequestError,
    ProviderError,
    NetworkError,
    StreamError,
)

__version__ = "0.1.0"

__all__ = [
    "Client",
    "ChatRequest", "ChatResponse", "Message", "Role", "StopReason",
    "StreamEvent", "Tool", "Usage",
    "MotosanError", "AuthError", "RateLimitError", "InvalidRequestError",
    "ProviderError", "NetworkError", "StreamError",
]

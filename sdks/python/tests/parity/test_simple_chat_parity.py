from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message
from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


@respx.mock
@pytest.mark.asyncio
async def test_simple_user_message_body(provider_under_test: ProviderUnderTest):
    body = await capture_chat_body(
        provider_under_test, ChatRequest(messages=[Message.user("Hello")])
    )
    assert_snapshot(f"simple_user_{provider_under_test.name}", body)


@respx.mock
@pytest.mark.asyncio
async def test_multi_turn_with_system(provider_under_test: ProviderUnderTest):
    req = ChatRequest(
        messages=[Message.user("q1"), Message.assistant("a1"), Message.user("q2")],
        system="Be concise.",
        temperature=0.2,
        max_tokens=100,
    )
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"multi_turn_system_{provider_under_test.name}", body)

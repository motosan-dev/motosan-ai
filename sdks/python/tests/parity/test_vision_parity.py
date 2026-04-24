from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message
from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


@respx.mock
@pytest.mark.asyncio
async def test_vision_base64_image(provider_under_test: ProviderUnderTest):
    if provider_under_test.name == "minimax":
        pytest.skip("MiniMax vision coverage is a separate concern")
    req = ChatRequest(messages=[Message.user_with_image("describe", "JVBER", "image/png")])
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"vision_base64_{provider_under_test.name}", body)

from __future__ import annotations

import pytest
import respx

from motosan_ai.types import ChatRequest, Message, Tool, ToolChoice
from tests._snapshots import assert_snapshot
from tests.parity.conftest import ProviderUnderTest, capture_chat_body


def _dummy_tool() -> Tool:
    return Tool(
        name="get_weather",
        description="Get weather for a city",
        input_schema={
            "type": "object",
            "properties": {"city": {"type": "string"}},
            "required": ["city"],
        },
    )


@pytest.mark.parametrize(
    "choice_name,choice",
    [
        ("auto", ToolChoice.auto()),
        ("required", ToolChoice.required()),
        ("none", ToolChoice.none()),
        ("tool_named", ToolChoice.tool("get_weather")),
    ],
    ids=["auto", "required", "none", "tool_named"],
)
@respx.mock
@pytest.mark.asyncio
async def test_tool_choice_matrix(
    provider_under_test: ProviderUnderTest,
    choice_name: str,
    choice: ToolChoice,
):
    req = ChatRequest(
        messages=[Message.user("what is the weather?")], tools=[_dummy_tool()], tool_choice=choice
    )
    body = await capture_chat_body(provider_under_test, req)
    assert_snapshot(f"tool_choice_{choice_name}_{provider_under_test.name}", body)

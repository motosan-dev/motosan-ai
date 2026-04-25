from __future__ import annotations

import asyncio

import httpx
import pytest

from motosan_ai.oauth._callback_server import bind, wait_for_callback


@pytest.mark.asyncio
async def test_bind_returns_port_in_loopback_range():
    server = await bind(port=None)
    try:
        assert 1024 <= server.port <= 65535
    finally:
        server.close()


@pytest.mark.asyncio
async def test_callback_captures_code_and_state():
    server = await bind(port=None)
    port = server.port

    async def fire_callback() -> None:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            await client.get(
                f"http://127.0.0.1:{port}/auth/callback",
                params={"code": "auth-code-xyz", "state": "state-abc"},
            )

    fire_task = asyncio.create_task(fire_callback())
    code, state = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "auth-code-xyz"
    assert state == "state-abc"


@pytest.mark.asyncio
async def test_callback_serves_success_html():
    server = await bind(port=None)
    port = server.port

    async def fire_then_check_response() -> str:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            r = await client.get(
                f"http://127.0.0.1:{port}/auth/callback",
                params={"code": "c", "state": "s"},
            )
            return r.text

    response_task = asyncio.create_task(fire_then_check_response())
    await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    assert "complete" in (await response_task).lower()

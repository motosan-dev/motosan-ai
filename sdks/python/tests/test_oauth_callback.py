from __future__ import annotations

import asyncio

import httpx
import pytest

from motosan_ai.oauth._callback_server import bind, wait_for_callback


@pytest.mark.asyncio
async def test_bind_returns_port_in_loopback_range():
    server = await bind(port=None, callback_path="/auth/callback")
    try:
        assert 1024 <= server.port <= 65535
    finally:
        server.close()


@pytest.mark.asyncio
async def test_callback_captures_code_and_state():
    server = await bind(port=None, callback_path="/auth/callback")
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
    server = await bind(port=None, callback_path="/auth/callback")
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


@pytest.mark.asyncio
async def test_callback_matches_configured_path():
    server = await bind(port=None, callback_path="/callback")
    port = server.port

    async def fire() -> None:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            await client.get(
                f"http://127.0.0.1:{port}/callback",
                params={"code": "c", "state": "s"},
            )

    fire_task = asyncio.create_task(fire())
    code, state = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "c"
    assert state == "s"


@pytest.mark.asyncio
async def test_callback_ignores_non_configured_path():
    server = await bind(port=None, callback_path="/callback")
    port = server.port

    async def hit_wrong_then_right() -> None:
        await asyncio.sleep(0.1)
        async with httpx.AsyncClient() as client:
            # Wrong path: must 404 and not resolve the future.
            r = await client.get(f"http://127.0.0.1:{port}/auth/callback")
            assert r.status_code == 404
            # Right path: resolves.
            await client.get(
                f"http://127.0.0.1:{port}/callback",
                params={"code": "ok", "state": "st"},
            )

    fire_task = asyncio.create_task(hit_wrong_then_right())
    code, _ = await asyncio.wait_for(wait_for_callback(server), timeout=5.0)
    await fire_task
    assert code == "ok"

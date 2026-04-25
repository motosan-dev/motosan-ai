from __future__ import annotations

import time

import httpx
import pytest
import respx

from motosan_ai.oauth.google import (
    Token,
    ensure_fresh_token,
    exchange_code,
    google_gemini_config,
    load_cached_token,
    login,
    refresh_token,
    save_token,
)


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_posts_to_token_url():
    cfg = google_gemini_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.new",
                "refresh_token": "1//ref",
                "expires_in": 3600,
                "id_token": "eyJ...",
            },
        )
    )
    token = await exchange_code(
        cfg, code="auth-code", verifier="ver", redirect_uri="http://127.0.0.1:9999/auth/callback"
    )
    assert token.access_token == "ya29.new"
    assert token.refresh_token == "1//ref"
    assert token.id_token == "eyJ..."
    assert abs(token.issued_at - int(time.time())) < 5
    body = route.calls[0].request.content.decode()
    assert "grant_type=authorization_code" in body
    assert "code=auth-code" in body
    assert "code_verifier=ver" in body


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_400_raises():
    from motosan_ai.error import AuthError

    cfg = google_gemini_config()
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(400, json={"error": "invalid_grant"})
    )
    with pytest.raises(AuthError, match="invalid_grant"):
        await exchange_code(cfg, code="bad", verifier="v", redirect_uri="http://127.0.0.1:0/cb")


@respx.mock
@pytest.mark.asyncio
async def test_refresh_token_uses_refresh_grant_type():
    cfg = google_gemini_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "ya29.refreshed", "expires_in": 3600}
        )
    )
    token = await refresh_token(cfg, refresh_token_value="old-refresh")
    assert token.access_token == "ya29.refreshed"
    assert token.refresh_token == "old-refresh"
    assert "grant_type=refresh_token" in route.calls[0].request.content.decode()


@respx.mock
@pytest.mark.asyncio
async def test_refresh_token_uses_returned_refresh_when_present():
    cfg = google_gemini_config()
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200,
            json={
                "access_token": "ya29.refreshed",
                "refresh_token": "1//new-ref",
                "expires_in": 3600,
            },
        )
    )
    token = await refresh_token(cfg, refresh_token_value="old-refresh")
    assert token.refresh_token == "1//new-ref"


@respx.mock
@pytest.mark.asyncio
async def test_login_full_flow_with_mocked_browser_and_callback():
    cfg = google_gemini_config()

    async def fake_open_and_callback(auth_url: str, redirect_uri: str) -> None:
        import asyncio as _asyncio
        from urllib.parse import parse_qs, urlencode, urlparse
        from urllib.request import urlopen

        state = parse_qs(urlparse(auth_url).query)["state"][0]
        await _asyncio.sleep(0.1)
        callback = f"{redirect_uri}?{urlencode({'code': 'test-code', 'state': state})}"
        await _asyncio.to_thread(lambda: urlopen(callback, timeout=5).read())

    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "ya29.new", "refresh_token": "1//ref", "expires_in": 3600}
        )
    )
    token = await login(cfg, _open_browser=fake_open_and_callback)
    assert token.access_token == "ya29.new"


@pytest.mark.asyncio
async def test_login_surfaces_mock_browser_errors_immediately():
    from motosan_ai.error import AuthError

    async def failing_browser(auth_url: str, redirect_uri: str) -> None:
        raise RuntimeError("browser helper exploded")

    with pytest.raises(AuthError, match="browser helper exploded"):
        await login(google_gemini_config(), _open_browser=failing_browser)


@pytest.mark.asyncio
async def test_login_rejects_state_mismatch():
    from motosan_ai.error import AuthError

    cfg = google_gemini_config()

    async def fire_wrong_state(auth_url: str, redirect_uri: str) -> None:
        import asyncio as _asyncio
        from urllib.parse import urlencode
        from urllib.request import urlopen

        await _asyncio.sleep(0.1)
        callback = f"{redirect_uri}?{urlencode({'code': 'c', 'state': 'WRONG'})}"
        await _asyncio.to_thread(lambda: urlopen(callback, timeout=5).read())

    with pytest.raises(AuthError, match="state"):
        await login(cfg, _open_browser=fire_wrong_state)


@respx.mock
@pytest.mark.asyncio
async def test_ensure_fresh_token_returns_cached_when_valid(tmp_path):
    cfg = google_gemini_config()
    cache = tmp_path / "tokens.json"
    fresh = Token("ok", "r", None, 3600, int(time.time()))
    save_token(fresh, path=cache)
    token = await ensure_fresh_token(cfg, cache_path=cache)
    assert token.access_token == "ok"


@respx.mock
@pytest.mark.asyncio
async def test_ensure_fresh_token_refreshes_when_expired(tmp_path):
    cfg = google_gemini_config()
    cache = tmp_path / "tokens.json"
    save_token(Token("old", "ref", None, 10, 0), path=cache)
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(200, json={"access_token": "ya29.new", "expires_in": 3600})
    )
    token = await ensure_fresh_token(cfg, cache_path=cache)
    assert token.access_token == "ya29.new"
    assert (load_cached_token(path=cache) or token).access_token == "ya29.new"


@pytest.mark.asyncio
async def test_ensure_fresh_token_raises_when_no_cache(tmp_path):
    from motosan_ai.error import AuthError

    with pytest.raises(AuthError, match="login"):
        await ensure_fresh_token(google_gemini_config(), cache_path=tmp_path / "missing.json")

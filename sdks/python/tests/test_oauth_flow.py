from __future__ import annotations

from urllib.parse import parse_qs, urlparse

from motosan_ai.oauth._flow import OAuthConfig, _build_auth_url


def _cfg(**overrides) -> OAuthConfig:
    base = dict(
        client_id="test-client",
        client_secret=None,
        auth_url="https://auth.example.com/authorize",
        token_url="https://auth.example.com/token",
        scopes=("openid",),
    )
    base.update(overrides)
    return OAuthConfig(**base)


def test_build_auth_url_appends_extra_auth_params():
    cfg = _cfg(extra_auth_params=(("foo", "bar"), ("baz", "qux")))
    url = _build_auth_url(cfg, "challenge", "state", "http://127.0.0.1:9999/cb")
    qs = parse_qs(urlparse(url).query)
    assert qs["foo"] == ["bar"]
    assert qs["baz"] == ["qux"]


def test_build_auth_url_no_extra_params_has_no_access_type():
    cfg = _cfg(extra_auth_params=())
    url = _build_auth_url(cfg, "challenge", "state", "http://127.0.0.1:9999/cb")
    qs = parse_qs(urlparse(url).query)
    assert "access_type" not in qs


import httpx
import pytest
import respx

from motosan_ai.oauth._flow import StateStrategy
from motosan_ai.oauth._flow import TokenBodyFormat, exchange_code


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_form_body():
    cfg = _cfg(token_body=TokenBodyFormat.FORM)
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )
    await exchange_code(cfg, code="C", state="S", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/x-www-form-urlencoded")
    body = req.content.decode()
    assert "grant_type=authorization_code" in body
    assert "code=C" in body
    assert "state=S" in body


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_json_body():
    cfg = _cfg(token_body=TokenBodyFormat.JSON)
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )
    await exchange_code(cfg, code="C", state="S", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/json")
    import json as _json

    payload = _json.loads(req.content)
    assert payload["grant_type"] == "authorization_code"
    assert payload["code"] == "C"
    assert payload["state"] == "S"


def test_build_auth_url_state_is_caller_provided():
    # _build_auth_url echoes whatever state string it is given; the
    # RANDOM vs EQUALS_VERIFIER choice is made in login(), tested below.
    cfg = _cfg()
    url = _build_auth_url(cfg, "challenge", "STATEVALUE", "http://127.0.0.1/cb")
    qs = parse_qs(urlparse(url).query)
    assert qs["state"] == ["STATEVALUE"]


@respx.mock
@pytest.mark.asyncio
async def test_login_equals_verifier_uses_verifier_as_state():
    from motosan_ai.oauth._flow import login

    cfg = _cfg(
        token_url="https://auth.example.com/token",
        state_strategy=StateStrategy.EQUALS_VERIFIER,
        callback_path="/auth/callback",
    )
    respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )

    captured: dict[str, str] = {}

    async def fake_browser(auth_url: str, redirect_uri: str) -> None:
        import asyncio as _a
        from urllib.parse import parse_qs as _pq, urlencode, urlparse as _up
        from urllib.request import urlopen

        q = _pq(_up(auth_url).query)
        captured["state"] = q["state"][0]
        captured["challenge"] = q["code_challenge"][0]
        await _a.sleep(0.1)
        cb = f"{redirect_uri}?{urlencode({'code': 'c', 'state': q['state'][0]})}"
        await _a.to_thread(lambda: urlopen(cb, timeout=5).read())

    await login(cfg, _open_browser=fake_browser)
    # Under EQUALS_VERIFIER, state must be the PKCE verifier — i.e. the
    # SHA256(state) base64url-encoded must equal the code_challenge.
    import base64
    import hashlib

    expected_challenge = (
        base64.urlsafe_b64encode(hashlib.sha256(captured["state"].encode()).digest())
        .rstrip(b"=")
        .decode()
    )
    assert expected_challenge == captured["challenge"]


@pytest.mark.asyncio
async def test_login_wraps_bind_oserror(monkeypatch):
    from motosan_ai.error import AuthError
    from motosan_ai.oauth import _flow

    async def fail_bind(port: int | None, callback_path: str):
        raise OSError("port in use")

    monkeypatch.setattr(_flow, "bind", fail_bind)

    with pytest.raises(AuthError, match="callback server failed to bind"):
        await _flow.login(_cfg())

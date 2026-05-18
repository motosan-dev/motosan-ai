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
    await exchange_code(cfg, code="C", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/x-www-form-urlencoded")
    body = req.content.decode()
    assert "grant_type=authorization_code" in body
    assert "code=C" in body


@respx.mock
@pytest.mark.asyncio
async def test_exchange_code_json_body():
    cfg = _cfg(token_body=TokenBodyFormat.JSON)
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "AT", "refresh_token": "RT", "expires_in": 3600}
        )
    )
    await exchange_code(cfg, code="C", verifier="V", redirect_uri="http://127.0.0.1/cb")
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/json")
    import json as _json

    payload = _json.loads(req.content)
    assert payload["grant_type"] == "authorization_code"
    assert payload["code"] == "C"

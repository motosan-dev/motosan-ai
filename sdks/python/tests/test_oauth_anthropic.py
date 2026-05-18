from __future__ import annotations

import httpx
import pytest
import respx

from motosan_ai.oauth import claude_pro_max_config
from motosan_ai.oauth._flow import StateStrategy, TokenBodyFormat, refresh_token


def test_claude_pro_max_client_id():
    assert claude_pro_max_config().client_id == "9d1c250a-e61b-44d9-88ed-5944d1962f5e"


def test_claude_pro_max_no_client_secret():
    assert claude_pro_max_config().client_secret is None


def test_claude_pro_max_redirect_port():
    assert claude_pro_max_config().redirect_port == 53692


def test_claude_pro_max_callback_path():
    assert claude_pro_max_config().callback_path == "/callback"


def test_claude_pro_max_redirect_uri_host_is_localhost():
    assert claude_pro_max_config().redirect_uri_host == "localhost"


def test_claude_pro_max_token_body_is_json():
    assert claude_pro_max_config().token_body is TokenBodyFormat.JSON


def test_claude_pro_max_state_strategy_is_equals_verifier():
    assert claude_pro_max_config().state_strategy is StateStrategy.EQUALS_VERIFIER


def test_claude_pro_max_extra_auth_params_has_code_true():
    assert ("code", "true") in tuple(claude_pro_max_config().extra_auth_params)


def test_claude_pro_max_scopes_include_claude_code_session():
    assert "user:sessions:claude_code" in tuple(claude_pro_max_config().scopes)


def test_claude_pro_max_auth_url_is_claude_ai():
    assert "claude.ai" in claude_pro_max_config().auth_url


@respx.mock
@pytest.mark.asyncio
async def test_refresh_posts_json_body():
    cfg = claude_pro_max_config()
    route = respx.post(cfg.token_url).mock(
        return_value=httpx.Response(
            200, json={"access_token": "NEW_AT", "refresh_token": "NEW_RT", "expires_in": 3600}
        )
    )
    token = await refresh_token(cfg, refresh_token_value="OLD_RT")
    assert token.access_token == "NEW_AT"
    assert token.refresh_token == "NEW_RT"
    req = route.calls[0].request
    assert req.headers["content-type"].startswith("application/json")
    import json as _json

    payload = _json.loads(req.content)
    assert payload["grant_type"] == "refresh_token"
    assert payload["refresh_token"] == "OLD_RT"

"""Live login smoke test for Anthropic OAuth.

Skipped by default — running it opens a real browser, requires a
Claude Pro/Max account, and binds to port 53692 on localhost. Run it
manually before releasing a new Python SDK version:

    cd sdks/python
    MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 uv run pytest \\
        tests/integration/test_anthropic_oauth_live.py -v -s

Success criterion: the returned token's access_token starts with the
"sk-ant-oat01-" prefix and the refresh token is non-empty.
"""

from __future__ import annotations

import os

import pytest

from motosan_ai.oauth import claude_pro_max_config, login

_LIVE = os.environ.get("MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE") == "1"


@pytest.mark.skipif(not _LIVE, reason="set MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 to run")
@pytest.mark.asyncio
async def test_live_login_returns_setup_token():
    token = await login(claude_pro_max_config())
    assert token.access_token.startswith("sk-ant-oat01-"), (
        f"unexpected token prefix: {token.access_token[:20]}"
    )
    assert token.refresh_token, "refresh_token must be non-empty"
    assert token.expires_in > 0, f"expires_in must be positive: {token.expires_in}"
    print(f"\nLive login OK. expires_in={token.expires_in}s")

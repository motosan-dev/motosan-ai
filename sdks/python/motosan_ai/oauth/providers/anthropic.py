"""Anthropic Claude Pro/Max OAuth provider config.

The ``client_id`` below is extracted from Anthropic's Claude Code CLI
authentication flow; the same value is used by the Rust ``anthropic-oauth``
crate and the reference implementation ``@earendil-works/pi-ai``. Anthropic
has not published this client_id as a public app registration for
third-party use — see the ToS disclosure in the Python SDK README.
"""

from __future__ import annotations

from motosan_ai.oauth._flow import OAuthConfig, StateStrategy, TokenBodyFormat


def claude_pro_max_config() -> OAuthConfig:
    return OAuthConfig(
        client_id="9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        client_secret=None,
        auth_url="https://claude.ai/oauth/authorize",
        token_url="https://platform.claude.com/v1/oauth/token",
        scopes=(
            "org:create_api_key",
            "user:profile",
            "user:inference",
            "user:sessions:claude_code",
            "user:mcp_servers",
            "user:file_upload",
        ),
        redirect_port=53692,
        callback_path="/callback",
        redirect_uri_host="localhost",
        token_body=TokenBodyFormat.JSON,
        extra_auth_params=(("code", "true"),),
        state_strategy=StateStrategy.EQUALS_VERIFIER,
    )

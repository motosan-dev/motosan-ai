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

from __future__ import annotations

import os
import stat
import time

from motosan_ai.oauth import (
    DEFAULT_CACHE_PATH,
    OAuthConfig,
    StateStrategy,
    Token,
    TokenBodyFormat,
    gemini_config,
    load_cached_token,
    save_token,
)


def test_token_not_expired_when_just_issued():
    t = Token("a", "r", None, 3600, int(time.time()))
    assert not t.is_expired()


def test_token_expired_when_within_buffer():
    assert Token("a", "r", None, 30, int(time.time())).is_expired()


def test_token_expired_when_issued_at_zero():
    assert Token("a", "r", None, 3600, 0).is_expired()


def test_default_cache_path_under_home_config():
    assert DEFAULT_CACHE_PATH.parent.name == "motosan-ai"
    assert DEFAULT_CACHE_PATH.name == "google-tokens.json"


def test_save_and_load_roundtrip(tmp_path):
    cache_path = tmp_path / "tokens.json"
    t = Token("abc", "ref", "id", 3600, 12345)
    save_token(t, path=cache_path)
    assert load_cached_token(path=cache_path) == t


def test_load_cached_token_missing_returns_none(tmp_path):
    assert load_cached_token(path=tmp_path / "none.json") is None


def test_save_creates_parent_directory(tmp_path):
    nested = tmp_path / "deeply" / "nested" / "tokens.json"
    save_token(Token("a", "r", None, 1, 1), path=nested)
    assert nested.exists()


def test_save_token_chmod_user_only(tmp_path):
    cache_path = tmp_path / "tokens.json"
    save_token(Token("a", "r", None, 1, 1), path=cache_path)
    assert stat.S_IMODE(os.stat(cache_path).st_mode) == 0o600


def test_gemini_config_has_public_client_id():
    assert "681255809395" in gemini_config().client_id


def test_gemini_config_has_client_secret():
    assert gemini_config().client_secret is not None


def test_gemini_config_auth_url_is_google():
    assert "accounts.google.com" in gemini_config().auth_url


def test_gemini_config_token_url_is_google():
    assert gemini_config().token_url == "https://oauth2.googleapis.com/token"


def test_gemini_config_scopes_include_cloud_platform():
    assert any("cloud-platform" in s for s in gemini_config().scopes)


def test_oauth_config_type_constructible():
    cfg = OAuthConfig("id", None, "auth", "token", ("scope",))
    assert cfg.client_id == "id"


def test_public_oauth_exports_enums():
    assert TokenBodyFormat.FORM.value == "form"
    assert StateStrategy.RANDOM.value == "random"

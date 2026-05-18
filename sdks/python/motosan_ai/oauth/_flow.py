from __future__ import annotations

import asyncio
import base64
import contextlib
import json
import os
import secrets
import time
import webbrowser
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import asdict, dataclass
from pathlib import Path
from urllib.parse import urlencode

import httpx

from motosan_ai.error import AuthError, NetworkError
from motosan_ai.oauth._callback_server import bind, wait_for_callback
from motosan_ai.oauth._pkce import Pkce

DEFAULT_CACHE_PATH = Path.home() / ".config" / "motosan-ai" / "google-tokens.json"
_EXPIRY_BUFFER_SECS = 60
_LOGIN_TIMEOUT_SECS = 120


@dataclass(frozen=True)
class Token:
    access_token: str
    refresh_token: str
    id_token: str | None
    expires_in: int
    issued_at: int

    def is_expired(self) -> bool:
        return int(time.time()) + _EXPIRY_BUFFER_SECS >= self.issued_at + self.expires_in


@dataclass(frozen=True)
class OAuthConfig:
    client_id: str
    client_secret: str | None
    auth_url: str
    token_url: str
    scopes: Sequence[str]
    redirect_port: int | None = None


def save_token(token: Token, *, path: Path = DEFAULT_CACHE_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    try:
        os.fchmod(fd, 0o600)
        with os.fdopen(fd, "w") as f:
            fd = -1  # fd ownership transferred to file object
            json.dump(asdict(token), f, indent=2)
    finally:
        if fd != -1:
            os.close(fd)


def load_cached_token(*, path: Path = DEFAULT_CACHE_PATH) -> Token | None:
    if not path.exists():
        return None
    data = json.loads(path.read_text())
    return Token(**data)


async def _post_token(config: OAuthConfig, data: dict[str, str]) -> Token:
    async with httpx.AsyncClient(timeout=30.0) as client:
        try:
            resp = await client.post(
                config.token_url,
                data=data,
                headers={"content-type": "application/x-www-form-urlencoded"},
            )
        except httpx.HTTPError as exc:
            raise NetworkError(f"OAuth token request failed: {exc}") from exc

    if resp.status_code != 200:
        try:
            err = resp.json()
            msg = err.get("error_description") or err.get("error") or resp.text
        except Exception:
            msg = resp.text
        raise AuthError(f"OAuth token exchange failed ({resp.status_code}): {msg}")

    payload = resp.json()
    return Token(
        access_token=payload["access_token"],
        refresh_token=payload.get("refresh_token", ""),
        id_token=payload.get("id_token"),
        expires_in=int(payload.get("expires_in", 3600)),
        issued_at=int(time.time()),
    )


async def exchange_code(
    config: OAuthConfig, *, code: str, verifier: str, redirect_uri: str
) -> Token:
    data = {
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "code_verifier": verifier,
        "client_id": config.client_id,
    }
    if config.client_secret:
        data["client_secret"] = config.client_secret
    return await _post_token(config, data)


async def refresh_token(config: OAuthConfig, *, refresh_token_value: str) -> Token:
    data = {
        "grant_type": "refresh_token",
        "refresh_token": refresh_token_value,
        "client_id": config.client_id,
    }
    if config.client_secret:
        data["client_secret"] = config.client_secret
    token = await _post_token(config, data)
    if not token.refresh_token:
        token = Token(
            access_token=token.access_token,
            refresh_token=refresh_token_value,
            id_token=token.id_token,
            expires_in=token.expires_in,
            issued_at=token.issued_at,
        )
    return token


OpenBrowserFn = Callable[[str, str], Awaitable[None]]


def _build_auth_url(config: OAuthConfig, challenge: str, state: str, redirect_uri: str) -> str:
    params = {
        "client_id": config.client_id,
        "response_type": "code",
        "redirect_uri": redirect_uri,
        "scope": " ".join(config.scopes),
        "state": state,
        "code_challenge": challenge,
        "code_challenge_method": "S256",
        "access_type": "offline",
    }
    return f"{config.auth_url}?{urlencode(params)}"


async def login(config: OAuthConfig, *, _open_browser: OpenBrowserFn | None = None) -> Token:
    pkce = Pkce.generate()
    state = base64.urlsafe_b64encode(secrets.token_bytes(16)).rstrip(b"=").decode("ascii")
    server = await bind(config.redirect_port)
    redirect_uri = f"http://127.0.0.1:{server.port}/auth/callback"
    auth_url = _build_auth_url(config, pkce.challenge, state, redirect_uri)

    callback_task = asyncio.create_task(wait_for_callback(server))
    browser_task: asyncio.Task[None] | None = None
    if _open_browser is not None:
        browser_task = asyncio.create_task(_open_browser(auth_url, redirect_uri))
    else:
        print(f"Open this URL to log in:\n\n  {auth_url}\n")
        webbrowser.open(auth_url)

    try:
        if browser_task is None:
            code, returned_state = await asyncio.wait_for(
                callback_task, timeout=_LOGIN_TIMEOUT_SECS
            )
        else:
            done, pending = await asyncio.wait(
                {callback_task, browser_task},
                timeout=_LOGIN_TIMEOUT_SECS,
                return_when=asyncio.FIRST_COMPLETED,
            )
            if not done:
                raise TimeoutError
            if browser_task in done:
                exc = browser_task.exception()
                if exc is not None:
                    callback_task.cancel()
                    with contextlib.suppress(asyncio.CancelledError):
                        await callback_task
                    raise AuthError(f"OAuth browser callback helper failed: {exc}") from exc
            code, returned_state = await asyncio.wait_for(
                callback_task, timeout=_LOGIN_TIMEOUT_SECS
            )
            for task in pending:
                task.cancel()
                with contextlib.suppress(asyncio.CancelledError):
                    await task
    except TimeoutError as exc:
        callback_task.cancel()
        with contextlib.suppress(asyncio.CancelledError):
            await callback_task
        raise AuthError(f"OAuth login timed out after {_LOGIN_TIMEOUT_SECS}s") from exc

    if returned_state != state:
        raise AuthError(f"OAuth state mismatch: sent {state!r}, got {returned_state!r}")

    return await exchange_code(config, code=code, verifier=pkce.verifier, redirect_uri=redirect_uri)


async def ensure_fresh_token(
    config: OAuthConfig, *, cache_path: Path = DEFAULT_CACHE_PATH
) -> Token:
    cached = load_cached_token(path=cache_path)
    if cached is None:
        raise AuthError(f"no cached OAuth token at {cache_path}; run login() first")
    if not cached.is_expired():
        return cached
    fresh = await refresh_token(config, refresh_token_value=cached.refresh_token)
    save_token(fresh, path=cache_path)
    return fresh

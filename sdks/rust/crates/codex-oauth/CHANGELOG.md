# Changelog

All notable changes to `codex-oauth` are documented here.

## [0.1.0] - 2026-04-20

### Added
- **`login()`** — browser-based PKCE OAuth flow against `auth.openai.com`. Opens the browser automatically (macOS/Linux/Windows) and falls back to printing the URL. Listens on `http://localhost:1455/auth/callback` for the redirect. Times out after 120 seconds.
- **`refresh(refresh_token)`** — exchanges a stored refresh token for a new `Token`.
- **`Token`** struct with `access_token`, `refresh_token`, `id_token`, `expires_in`, `issued_at` fields. Implements `Serialize`/`Deserialize` for persistence and `is_expired()` for expiry detection.
- **`Error`** enum covering IO, HTTP, callback parsing, state mismatch, and token exchange failures.

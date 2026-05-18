# Changelog

All notable changes to `motosan-ai-oauth` are documented in this file.

## [0.2.0] - 2026-05-18

### Added
- `TokenBodyFormat` enum (`Form` | `Json`) controlling the body encoding
  used by `exchange_code` and `refresh_token`.
- `OAuthConfig::callback_path` — the HTTP path the callback server matches
  against. Defaults vary per provider config (codex/gemini: `/auth/callback`,
  Anthropic: `/callback`).
- `OAuthConfig::redirect_uri_host` — the host string used inside the
  `redirect_uri` query parameter. Separate from the bind address (which is
  always `127.0.0.1`) so providers like Anthropic that register their
  redirect URI as `http://localhost:...` work without changing the listen
  socket.
- `OAuthConfig::token_body` — selects `TokenBodyFormat::Form` (existing
  behavior) or `Json`.
- `OAuthConfig::extra_auth_params` — replaces the previously hardcoded
  `access_type=offline` query parameter with a config-driven list.
- `anthropic` feature flag exposing `providers::anthropic::claude_pro_max()`.

### Changed
- **Breaking (for out-of-tree consumers constructing `OAuthConfig` literals
  directly):** four new required fields on `OAuthConfig`. The built-in
  provider configs (`providers::codex`, `providers::gemini`) are updated to
  set values that preserve previous behavior; consumers using them are
  unaffected.

## [0.1.0] - 2026-04-20

Initial release: generic PKCE OAuth login + refresh with built-in Codex
and Gemini provider configs.

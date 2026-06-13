# Changelog

All notable changes to `motosan-ai-oauth` are documented in this file.

## [Unreleased]

### Fixed
- `exchange_code` now sends `state` to the token endpoint only for
  `StateStrategy::EqualsVerifier` (Anthropic). `StateStrategy::Random`
  providers omit it to avoid token endpoints that reject unexpected fields.

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
- `OAuthConfig::state_strategy` (`StateStrategy::Random` | `EqualsVerifier`)
  — controls how the OAuth `state` CSRF nonce is derived. Anthropic's
  `claude.ai/oauth/authorize` empirically rejects random state values with
  "Invalid request format"; Anthropic uses `EqualsVerifier` (reuse PKCE
  verifier as state), matching Claude Code CLI behavior. Codex/Gemini stay
  on `Random`.
- `anthropic` feature flag exposing `providers::anthropic::claude_pro_max()`.

### Changed
- **Breaking (for out-of-tree consumers constructing `OAuthConfig` literals
  directly):** five new required fields on `OAuthConfig`. The built-in
  provider configs (`providers::codex`, `providers::gemini`) are updated to
  set values that preserve previous behavior; consumers using them are
  unaffected.
- Token POST behavior aligned with `@earendil-works/pi-ai`'s reference impl
  after empirical validation against `platform.claude.com/v1/oauth/token`:
  - `state` is now echoed in the token-endpoint POST body (after `code`).
    RFC 6749 §4.1.3 does not require this; most servers ignore extra
    fields. Anthropic empirically requires it (a missing `state` field
    returns HTTP 429 shaped as `rate_limit_error`). Codex/Gemini tolerate
    the extra field.
  - The previous `User-Agent: Mozilla/5.0 (compatible; motosan-ai-oauth)`
    override is removed (reqwest's default is used instead — fake-Mozilla
    UA strings can trip anti-abuse heuristics).
  - `Accept: application/json` is now set explicitly on the token POST.

## [0.1.0] - 2026-04-20

Initial release: generic PKCE OAuth login + refresh with built-in Codex
and Gemini provider configs.

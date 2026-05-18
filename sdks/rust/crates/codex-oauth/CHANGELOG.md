# Changelog

All notable changes to `codex-oauth` are documented in this file.

## [0.1.1] - 2026-05-18

### Changed
- Internal: provider config updated for `motosan-ai-oauth` v0.2.0's
  expanded `OAuthConfig` (new `callback_path`, `redirect_uri_host`,
  `token_body`, `extra_auth_params` fields). Values preserve previous
  Codex behavior (form-encoded token body, `/auth/callback` path,
  `127.0.0.1` redirect host, `access_type=offline` auth param). Public
  API of `codex-oauth` is unchanged.

## [0.1.0] - 2026-04-19

Initial release.

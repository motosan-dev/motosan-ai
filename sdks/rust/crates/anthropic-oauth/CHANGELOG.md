# Changelog

All notable changes to `anthropic-oauth` are documented in this file.

## [0.1.0] - 2026-05-18

Initial release. PKCE OAuth login and refresh for Anthropic Claude
Pro/Max. The resulting `sk-ant-oat01-*` access token is consumed
directly by `motosan-ai`'s `AnthropicProvider` (the setup-token code
path applies Bearer auth + Claude Code identity headers
automatically).

See the project README for the ToS disclosure regarding use of
Anthropic's Claude Code OAuth `client_id`.

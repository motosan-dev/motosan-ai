# Changelog

## [0.6.0] — 2026-04-05

### Added

- **ClaudeCodeClient** — new backend that shells out to the `claude` CLI binary, gated behind `--features claude-code`.
  - `chat()` — blocking subprocess call, returns `ChatResponse`.
  - `stream()` — NDJSON streaming via `--output-format stream-json`.
  - No API key required; uses the locally authenticated Claude Code session.
  - Limitation: no tool calling support (`tool_calls` is always empty).
- Documented all three Rust backends (API key, OAuth token, Claude Code CLI) in README.

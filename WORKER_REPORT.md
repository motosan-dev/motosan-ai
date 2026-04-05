# Worker Report: Issue #159

## Summary
Documented the three LlmClient backends (API key, OAuth, Claude Code CLI) in README and created CHANGELOG for v0.6.0.

## Changes

### `README.md`
- Added "Backends (Rust)" section with examples for all three backends
- Added `claude-code` to the feature list comment in Install section
- Added "Claude Code Backend" bullet to Features list
- Documented ClaudeCodeClient limitations (no tool calling, requires CLI)

### `CHANGELOG.md` (new)
- Created with `[0.6.0]` entry documenting ClaudeCodeClient and its limitations

## Acceptance Criteria
- [x] README updated with three-backend example
- [x] Feature flag instructions documented (`--features claude-code`)
- [x] Limitations of ClaudeCodeClient noted (no tool calling)
- [x] CHANGELOG updated

## Concerns
- None. This is a docs-only change.

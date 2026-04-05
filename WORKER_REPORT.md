# Worker Report: Issue #166 — ClaudeCodeClient (Python)

## What was done

Ported the Rust `ClaudeCodeClient` to idiomatic async Python, following the same design and CLI invocation patterns.

### New files
- `sdks/python/motosan_ai/providers/claude_code.py` — Full implementation including:
  - `ClaudeCodeClient` class with `__init__`, `with_path`, `model`, `agent_mode` builder methods
  - `chat()` — invokes `claude --print`, collects output, returns `ChatResponse`
  - `stream()` — invokes `claude --print --output-format stream-json`, yields `StreamEvent` from NDJSON
  - `_model_to_forward()` — mirrors Rust logic (skip empty/whitespace/"default")
  - `_messages_to_prompt()` — flattens multi-turn messages for CLI stdin
  - `_parse_agent_json()` — extracts result + usage from agent mode JSON
  - `_parse_ndjson_line()` — parses individual NDJSON stream events
- `sdks/python/tests/test_claude_code.py` — 31 unit tests covering model forwarding, message flattening, JSON parsing, NDJSON parsing, client construction, and argument building
- `sdks/python/tests/integration/test_claude_code_live.py` — Integration tests for chat and stream round-trips (auto-skipped when `claude` CLI not in PATH)

### Modified files
- `sdks/python/motosan_ai/providers/__init__.py` — Added `ClaudeCodeClient` export
- `sdks/python/motosan_ai/__init__.py` — Added `ClaudeCodeClient` to top-level exports and `__all__`

## Post-review fixes (2026-04-05)

### Fix 1 (CRITICAL): Stream NDJSON parsing updated for real CLI output

The `claude --print --output-format stream-json --verbose` CLI emits events with:
- `type: "assistant"` — text nested in `message.content[]` blocks with `type == "text"`
- `type: "result"` — final summary with `subtype: "success"`, `result`, and `usage`
- `type: "system"` — hook/session events (ignored)

Previous code expected `{"type":"text","text":"..."}` which doesn't match the actual CLI.

Changes:
- `_parse_ndjson_line()` now extracts text from `assistant` events' `message.content[]` array
- `_build_args()` adds `--verbose` when `output_format == "stream-json"` (required by CLI with `--print`)
- Unit tests updated to use the real event shapes
- Stream integration test now passes against live CLI (collected text correctly)

### Fix 2 (MINOR): Integration test skip guard respects CLAUDE_CODE_PATH

Changed `shutil.which("claude")` to `shutil.which(os.environ.get("CLAUDE_CODE_PATH", "claude"))`.

## Test results

```
106 passed, 7 skipped in 15.18s (34 claude_code tests + 72 existing)
```

All tests pass including live integration tests (chat + stream round-trips).

## Design decisions

- Used `asyncio.create_subprocess_exec` for subprocess management
- 300-second timeout matches Rust implementation
- Builder methods return `self` for chaining (Pythonic equivalent of Rust's consuming builder)
- No provider logic in `client.py` per CLAUDE.md rules
- No sync wrappers per CLAUDE.md rules — callers use `asyncio.run()`

## Concerns

The Rust `stream_json.rs` has the same incorrect event shapes (`{"type":"text","text":"..."}`) but that's a separate issue — tracked but not fixed here.

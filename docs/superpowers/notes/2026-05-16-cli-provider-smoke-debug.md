# CLI Provider Empty-Output Debug — 2026-05-16

**Investigation per** `docs/superpowers/specs/2026-05-16-motosan-ai-followups.md` §2.

**Symptom (from capo PR #11 manual smoke):** `cargo run -- --provider claude-code -p "Reply with exactly the word: pong"` and the codex equivalent both exit 0 but emit no final assistant text. capo prints its `[thinking]` preamble, then nothing.

## TL;DR

| Provider | Verdict | Fix lives in |
|----------|---------|--------------|
| `Provider::ClaudeCode` `.stream()` | 🚨 **motosan-ai bug** — missing `--verbose` flag | motosan-ai (this repo) |
| `Provider::ClaudeCode` `.chat()` | ✅ Clean | n/a |
| `Provider::CodexCli` `.stream()` / `.chat()` | ✅ Clean in isolation — if capo still sees empty output, the bug is downstream (capo's invocation) or in a parser shape mismatch we didn't surface here | downstream / further investigation |

## How the bug surfaces (claude_code `.stream()`)

`sdks/rust/src/providers/claude_code/mod.rs:396` spawns:

```rust
cmd.arg("--print").arg("--output-format").arg("stream-json");
```

`claude` 2.1.143 (current) **rejects this combination** at startup:

```
$ echo "Reply with exactly the word: pong" | claude --print --output-format stream-json -
Error: When using --print, --output-format=stream-json requires --verbose
```

Claude exits non-zero, stdout is empty, motosan-ai's stream code at `mod.rs:427-449` reads zero lines from stdout, the `async_stream::stream!` block falls through without yielding any events, and the stream closes cleanly with no events and no error. capo's consumer loop receives nothing.

The `child.wait().await` at `mod.rs:452` *would* see the non-zero exit code, but its return value is `_`-discarded. So motosan-ai swallows the failure silently.

## Verification

```bash
# Direct invocation matching motosan-ai's exact spawn args — empty output:
echo "Reply with exactly the word: pong" | claude --print --output-format stream-json -
# → Error: When using --print, --output-format=stream-json requires --verbose

# With --verbose added — works:
echo "Reply with exactly the word: pong" | claude --print --output-format stream-json --verbose -
# → Proper NDJSON including:
#   {"type":"assistant","message":{...,"content":[{"type":"text","text":"pong"}],...}}
#   {"type":"result","subtype":"success","result":"pong",...}

# .chat() path (no stream-json) — already works without --verbose:
echo "Reply with exactly the word: pong" | claude --print -
# → pong
```

## Codex side (acquitted)

`sdks/rust/src/providers/codex_cli/mod.rs:385` and `spawn.rs:227` spawn:

```rust
cmd.arg("exec").arg("--json").arg("--skip-git-repo-check");
```

Direct invocation:

```bash
echo "Reply with exactly the word: pong" | codex exec --json --skip-git-repo-check -
# → Proper NDJSON:
#   {"type":"thread.started",...}
#   {"type":"turn.started"}
#   {"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"pong"}}
#   {"type":"turn.completed","usage":{...}}
```

Codex with `codex-cli 0.130.0` emits the expected events under motosan-ai's exact spawn args. If capo continues to see empty output for `--provider codex-cli` after the claude fix lands, the next investigation step is parser shape: do motosan-ai's `codex_cli/mod.rs` and `codex_cli/event.rs` correctly map `item.completed` events with `item.type=="agent_message"` to a `StreamEvent::Text`? That's an in-repo audit worth ~30 minutes; out of scope for this round.

## Fix scope (claude_code)

Single-line change in `claude_code/mod.rs:396`:

```diff
- cmd.arg("--print").arg("--output-format").arg("stream-json");
+ cmd.arg("--print").arg("--output-format").arg("stream-json").arg("--verbose");
```

`--verbose` has been a stable flag in the `claude` CLI long before the requirement was added, so this is backwards-compatible with older binaries.

Also worth fixing in the same commit:
- `mod.rs:452`: `let _ = child.wait().await;` silently swallows non-zero exit codes. Should at minimum log or yield a `StreamEvent::Error` when the child exits non-zero AND no events were emitted. (Separate paragraph in the fix; not blocking.)

## Test plan

Add a `tests/claude_code_smoke.rs` integration test gated behind `#[ignore]` (requires `claude` binary + auth, like the existing codex live test) that:
1. Builds a `Client` with `Provider::ClaudeCode`
2. Calls `.stream()` with a "Reply with exactly the word: pong" prompt
3. Asserts at least one `StreamEvent::Text` arrives with non-empty text

This locks in the regression — if a future refactor drops `--verbose`, the test fails immediately.

Unit-test alternative (no live calls): add a test that spawns `claude --print --output-format stream-json --verbose -` directly and asserts the argv printed in `format!("{:?}", cmd)` contains `"--verbose"`. Cheaper but less safe than the live test.

## Release classification

Per `docs/superpowers/specs/2026-05-16-motosan-ai-followups.md` §190 ("§2 might add additional spawn-arg changes"):

- This is a **bug fix**, additive flag, no API surface change → **0.14.3 patch**.
- Bundle with §4 (CLI docs) which also touches claude_code/mod.rs, and §5a (unused field cleanup). Keep §3 (Ollama, breaking) and §5b (clippy mass cleanup) for 0.15.0 minor as the spec already plans.

## Capo coordination

Once 0.14.3 lands on crates.io, capo can bump its `motosan-ai` dep and re-run the manual smoke. If `--provider claude-code` now produces text but `--provider codex-cli` still doesn't, hand off the codex side to a separate investigation per the "Codex side (acquitted)" section above.

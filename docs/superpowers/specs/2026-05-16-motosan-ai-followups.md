# motosan-ai Rust SDK Follow-ups — 2026-05-16

**Status:** In progress — §1 shipped 2026-05-16 (motosan-ai 0.14.2 on crates.io); §2–§5 open. Ready to hand off to a fresh session running inside `~/Projects/wade/motosan-ai/`.

**Context:** Surfaced during capo's M2 manual-smoke side-quest (capo PR #11 — "multi-provider LLM dispatch"). capo wanted to use the user's already-authenticated `claude` / `codex` CLIs as LLM providers via `motosan-ai`'s `Provider::ClaudeCode` / `Provider::CodexCli`. The dispatch wiring worked, but capo's print-mode smoke discovered the final assistant text never reaches capo. An audit of motosan-ai's Rust SDK surfaced four additional items worth addressing.

This spec collects all five into one document so they can be triaged + executed in a single motosan-ai-side session.

---

## 1. ✅ DONE — publish 0.14.2

**Shipped** (merged to `main`, commits `7847136` + `ac03b16`, merge `f13d1fa`):
- `ClientBuilder::anthropic_base_url(...)` setter
- `Client::anthropic_base_url() -> Option<&str>` getter
- Threading through `build_anthropic_provider` (was hardcoded `None`)
- Round-trip test + mockito wire-through test in `tests/client_builder.rs`
- Bumped `version = "0.14.2"` in `sdks/rust/Cargo.toml`
- `CHANGELOG.md` entry
- Release-checklist docs (AGENTS.md, llms.txt, SKILL.md, READMEs, rust-api.md)

**Release** (completed 2026-05-16):
1. ✅ Pre-push gate passed (4 stages, incl. 9 Rust live tests against Anthropic API)
2. ✅ PR [#173](https://github.com/motosan-dev/motosan-ai/pull/173) merged via `gh pr merge --merge` → `f13d1fa`
3. ✅ Tag `rust-v0.14.2` pushed → triggered `publish-rust.yml` ([run 25956826592](https://github.com/motosan-dev/motosan-ai/actions/runs/25956826592))
4. ✅ Workflow green (fmt + clippy + test --all-features + publish)
5. ✅ crates.io: `motosan-ai 0.14.2` live at `2026-05-16T08:03:41Z`, not yanked

**Downstream (capo) — out of scope for this repo:** capo has a ~20 LOC follow-up patch waiting (bump `motosan-ai = "0.14.2"` + add `Settings::anthropic.base_url` + env overlay + chain `.anthropic_base_url()` in `build_anthropic` when non-default). Tracked on the capo side.

---

## 2. CRITICAL — CLI providers (`ClaudeCode`, `CodexCli`) emit no text under capo's invocation

**Empirical symptom (from capo PR #11 manual smoke):**
```bash
cd ~/Projects/wade/capo
cargo run --release -p capo -- --provider claude-code -p "Reply with exactly the word: pong"
# Exits 0
# Prints: "provider: claude-code | model: claude-sonnet-4-6"
# Prints: "[thinking]"
# Prints: <NO FINAL TEXT>
```
Same for `--provider codex-cli`.

**What's been ruled out** (per Explore agent audit on 2026-05-16):
- `claude_code/mod.rs:386-456` `.stream()` correctly parses NDJSON into `StreamEventType::Text` events.
- `codex_cli/mod.rs:375-448` `.stream()` correctly emits `agent_message` as `Text` events.
- `claude_code/mod.rs:363-379` `.chat()` returns `ChatResponse { content: stdout.trim().to_string(), ... }`.
- `motosan-agent-loop 0.18.2`'s bridge (`motosan_ai_impl.rs::impl LlmClient for motosan_ai::Client`) correctly maps `ChatResponse.content` → `LlmResponse::Message(content)`.
- capo's `map_event` in `crates/capo-agent/src/app.rs` correctly handles `AgentMessageComplete`.

**What's left as plausible root causes:**
1. **The `claude` / `codex` binary itself produces empty stdout under capo's invocation pattern.** Possibly a flag mismatch — e.g. `max_tokens=8192` being passed as a CLI arg the binary doesn't recognise and silently swallows.
2. **`MotosanAiClient::with_max_tokens(8192)` interacts weirdly with CLI providers** — the wrapper might be telling the CLI to limit output in a way that produces nothing.
3. **`spawn::invoke_cli()` in `claude_code/spawn.rs` collects empty stdout** because of subprocess buffering / stdin handling edge case.

**Investigation steps** (do these in order; stop at first hit):

### Step 2.1 — Verify the binaries work in isolation

```bash
# Direct invocation of claude — what does --print mode emit?
echo "Reply with exactly the word: pong" | claude --print -
# Should print "pong" or similar

# Direct invocation of codex
echo "Reply with exactly the word: pong" | codex exec --json -
# Should print NDJSON ending in agent_message + turn.completed
```

If either is empty: the bug is upstream of motosan-ai (Claude/Codex CLI behaviour change). Report to those projects.

### Step 2.2 — Verify motosan-ai's spawn args

Run a minimal Rust reproducer in `~/Projects/wade/motosan-ai/sdks/rust/examples/` (create one if missing):

```rust
// examples/cli_provider_smoke.rs
use motosan_ai::{Client, Message, Provider};

#[tokio::main]
async fn main() {
    let client = Client::builder()
        .provider(Provider::ClaudeCode)
        .api_key("cli-managed")
        .model("claude-sonnet-4-6")
        .build()
        .expect("build");
    let response = client
        .chat(vec![Message::user("Reply with exactly the word: pong")])
        .await
        .expect("chat");
    println!("content={:?}", response.content);
    println!("usage={:?}", response.usage);
}
```

```bash
cd ~/Projects/wade/motosan-ai/sdks/rust
cargo run --features claude-code,anthropic --example cli_provider_smoke
```

Expected: `content="pong"` or similar.
If empty: the bug is in `claude_code/spawn.rs::invoke_cli` — capture the exact `Command` it builds (`format!("{:?}", cmd)`) and inspect.

### Step 2.3 — If step 2.2 succeeds, the bug is in capo's invocation

The motosan-ai layer is clean. Hand back to capo to investigate:
- Whether capo's `MotosanAiClient::with_max_tokens(8192)` is causing the issue.
- Whether capo's `Engine` is calling `LlmClient::chat()` or some streaming variant.

### Step 2.4 — Document findings

Whatever step pinpoints the root cause, add a short note to `docs/superpowers/notes/2026-05-16-cli-provider-smoke-debug.md` (create the dir if needed) capturing:
- Which step (2.1 / 2.2 / 2.3) surfaced the bug
- The fix (one-line summary)
- Whether the fix lives in motosan-ai (this repo) or upstream (binary) or downstream (capo)

---

## 3. HIGH — Ollama HTTP path silently ignores three builder fields

**Files:** `sdks/rust/src/client.rs` lines 20-24, 235-240, 578-580.

**Bug:** Three `ClientBuilder` setters accept values that are only wired to the **native** Ollama provider, not the HTTP-based default. Anyone calling these without also calling `.ollama_native(true)` gets silent no-ops:

- `ollama_think: Option<String>`
- `ollama_keep_alive: Option<String>`
- `ollama_num_ctx: Option<u32>`

`client.rs:578-580` shows `build_ollama_native_provider` calls `.with_think()` / `.with_keep_alive()` / `.with_num_ctx()`. But `build_ollama_provider` (the HTTP path used when `ollama_native == false`, the **default**) at `client.rs:235-240` doesn't accept these — they're silently dropped.

**Fix options** (pick one):

| | Approach | Effort | Downside |
|---|---|---|---|
| **A** | Wire the 3 fields through `build_ollama_provider` too (HTTP path) | ~30 LOC + tests | Need to check whether Ollama's HTTP API accepts equivalents (likely yes via `options` field) |
| **B** | In `ClientBuilder::build()`, return `Err(MotosanError::Config(...))` if any of the 3 are set without `ollama_native(true)` | ~10 LOC | Breaking change for anyone currently calling them on HTTP path (silent no-op → error) |
| **C** | `tracing::warn!` at build time when the combination is detected | ~10 LOC | Non-breaking, but warnings don't always reach the developer |

**Recommend (A)** if Ollama HTTP API supports the relevant params; **(C)** as a quick fix if (A) is out of scope.

**capo impact:** capo doesn't use Ollama. This is upstream housekeeping, not a capo blocker.

---

## 4. MEDIUM — CLI providers under-documented

**Files:** `sdks/rust/src/providers/claude_code/mod.rs`, `sdks/rust/src/providers/codex_cli/mod.rs`.

The module-level docs describe the high-level "shells out to the CLI" model, but don't tell callers:

- `.chat()` spawns the CLI in **blocking, non-streaming mode** (e.g. `claude --print`) — collects subprocess stdout, returns single `ChatResponse`. No `StreamEvent::Text` events emitted.
- `.stream()` spawns the CLI in **streaming JSON mode** (e.g. `claude --print --output-format stream-json`) — yields `StreamEvent` items as the subprocess writes NDJSON.

This asymmetry vs HTTP providers (where chat/stream paths are essentially the same engine wrapped two ways) is **correct by design** but easy to miss.

**Action:** Add a `## Streaming vs Blocking` section near the top of each of:
- `claude_code/mod.rs` (after the existing "How it works" section)
- `codex_cli/mod.rs` (after the existing "How it works" section)

~15 lines of prose each. No code change.

---

## 5. LOW — Style cleanup

**5a. Unused `result` field** in `sdks/rust/src/providers/claude_code/stream_json.rs:13`, currently annotated `#[allow(dead_code)]`. Either delete it or replace the `#[allow]` with a comment explaining why it's kept (e.g. "kept for forward-compat with future Claude --print --output-format stream-json schema additions").

**5b. 10+ `unneeded return` clippy warnings** across the codebase (pre-existing, surfaced when running `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings`). Pure style cleanup. Not blocking but worth a single grep+sed cleanup commit.

**5c. Three `--all-features` clippy errors** about `ollama_native, ollama_think, ollama_keep_alive, ollama_num_ctx` "never read" — this overlaps with #3 above. Resolving #3 (option A) also clears these.

---

## Suggested release sequencing

| Release | Includes | Trigger |
|---|---|---|
| **0.14.2** ✅ published 2026-05-16 | `anthropic_base_url` setter/getter | Live on crates.io; capo follow-up unblocked |
| **0.14.3 (patch)** | #4 docs + #5a unused-field cleanup | Bundle whenever convenient; non-breaking |
| **0.15.0 (minor)** | #3 (option A or B — both arguably breaking) + #5b clippy cleanup | Cut after #2 investigation closes; #2 might add additional spawn-arg changes |
| **(no release needed)** | #2 investigation if it concludes "bug is upstream binary / downstream capo" | If neither side is motosan-ai, file findings in the notes doc and move on |

---

## Done criteria for this spec

- [x] Section 1: 0.14.2 tagged + published to crates.io.
- [ ] Section 2: investigation completed, root cause documented in `docs/superpowers/notes/2026-05-16-cli-provider-smoke-debug.md`. If fix lives in motosan-ai, it's merged.
- [ ] Section 3: Ollama HTTP gap addressed via option A / B / C. Decision recorded in the commit message.
- [ ] Section 4: docs added to both CLI provider modules.
- [ ] Section 5: clippy cleanup commit landed; `--features anthropic,minimax,ollama,openai` clippy is clean (or remaining errors documented).
- [ ] Release plan executed per the table above.

## Out of scope (defer to a separate spec)

- Adding new providers (e.g. Mistral, Cohere) — no demand surfaced.
- Refactoring the dispatch pattern in `client.rs::dispatch_chat` / `dispatch_stream` — works fine; refactoring without a use case is premature.
- The Python SDK at `sdks/python/` — separate maintainer track.
- `agent-tool` / `agent-loop` crates — separate spec.

---

## Reference: how the related capo work threads through

For context, the consuming project capo (`~/Projects/wade/capo/`):

- M3 Phase 0 merged 2026-05-16 (PR #10) — Settings + Auth + AGENTS.md walk-up.
- Multi-provider side-quest merged 2026-05-16 (PR #11) — added `Settings::model.provider = "anthropic"|"claude-code"|"codex-cli"` dispatch via this SDK. **This is the consumer whose smoke surfaced #2.**
- After this spec's section 1 is done (0.14.2 published), capo has a ~20 LOC follow-up to bump dep + add `Settings::anthropic.base_url` + thread through. That's tracked separately on the capo side; this spec doesn't need to address it.

capo's existing memory at `~/.claude/projects/-Users-daiwanwei-Projects-wade-capo/memory/capo_project.md` has a "multi-provider side-quest — merged" section with full details if the motosan-ai-side worker needs context.

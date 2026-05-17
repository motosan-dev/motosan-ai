# motosan-ai Rust SDK Follow-ups — 2026-05-16

**Status:** ✅ All sections (§1–§5) shipped across 0.14.2 / 0.14.3 / 0.15.0 on crates.io. B1 + B2 long-term backlog items remain (post-0.15). Spec retained as historical record of the multi-release work.

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

## 2. ✅ DONE — CLI providers (`ClaudeCode`, `CodexCli`) emit no text under capo's invocation

**Resolved in 0.14.3** (commits `0cadd98` + `d96cfc8`, merge `e496dfe`, tag `rust-v0.14.3`, crates.io published `2026-05-16T17:52:08Z`).

**Root cause** — TWO compounding bugs in claude_code:
1. Missing `--verbose` flag in `claude_code/mod.rs:396` (modern `claude` ≥ 2.1.x rejects `--print --output-format=stream-json` without it).
2. Stale NDJSON parser in `claude_code/stream_json.rs` — only matched the legacy `{"type":"text",...}` shape; modern `claude` emits text inside `{"type":"assistant","message":{"content":[...]}}`.

**Codex side acquitted** — direct invocation under motosan-ai's exact spawn args emits proper NDJSON. If capo continues to see empty output for `--provider codex-cli` after 0.14.3, that's a separate parser-shape issue (out of scope for this followup).

**Full investigation + repro commands**: see `docs/superpowers/notes/2026-05-16-cli-provider-smoke-debug.md`.

**Deferred to a future release**: `claude_code/mod.rs:445` `let _ = child.wait().await` silently swallows non-zero exit codes — both bugs would have surfaced faster if it yielded a `StreamEvent::Error`. Worth a small design conversation (new event variant for callers).

---

The original investigation protocol below is retained for historical record.

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

## 3. ✅ DONE — Ollama HTTP path silently ignores three builder fields

**Resolved in 0.15.0** (merge `004ef6b`, tag `rust-v0.15.0`, crates.io published `2026-05-17`). The fix turned out to require an architectural rethink rather than the wire-through approach the spec originally recommended:

- The OpenAI-compat `/v1/chat/completions` endpoint silently drops `keep_alive`, `options.num_ctx`, and `think` (verified against [ollama/openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go) — `ChatCompletionRequest` struct doesn't declare them, Go's encoding/json discards unknown fields). Wiring the fields through the OpenAI-compat body would have been theatrical.
- Instead, `Provider::Ollama` dispatch now auto-routes to `OllamaProvider` (native `/api/chat`) whenever any of the 3 fields is set. The OpenAI-compat path is retained as the default for callers who don't set any of these fields.
- `ClientBuilder::build()` returns `Err(MotosanError::Config)` if `ollama_*` fields are set on a non-`Provider::Ollama` client (option B).
- Setter doc-comments updated to describe the auto-switch + image-capability trade-off (option C).
- Cargo feature `ollama_native` collapsed into `ollama` so OllamaProvider is available whenever `ollama` is; `ollama_native` retained as alias.

mockito tests in `tests/ollama_http_autoswitch.rs` cover all four cells of the {routing-branch × input-shape} matrix.

---

The original spec text below is retained for historical record.

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

## 4. ✅ DONE — CLI providers under-documented

**Resolved in 0.14.3** (same commit `d96cfc8` as §2). Added "## Streaming vs Blocking" sections to both `providers::claude_code` and `providers::codex_cli` module-level docs. ~25 lines each, no code change.

---

The original spec text below is retained for historical record.



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

**5a. ✅ DONE in 0.14.3** — replaced the bare `#[allow(dead_code)]` on `ClaudeStreamEvent::Result::result` with a real comment explaining why the field is kept parsed but ignored (it duplicates text already yielded from the preceding `assistant` event; emitting it would double-up). See `claude_code/stream_json.rs:21-26`.

**5a (original spec text):** Unused `result` field in `sdks/rust/src/providers/claude_code/stream_json.rs:13`, currently annotated `#[allow(dead_code)]`. Either delete it or replace the `#[allow]` with a comment explaining why it's kept (e.g. "kept for forward-compat with future Claude --print --output-format stream-json schema additions").

**5b. ✅ DONE in 0.15.0** — all `unneeded return` warnings in `client.rs` dispatch arms removed. Some were cleared incidentally by the §3 routing fix; the rest by a follow-on sweep. `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings` now clean.

**5b (original spec text):** 10+ `unneeded return` clippy warnings across the codebase (pre-existing, surfaced when running `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings`). Pure style cleanup. Not blocking but worth a single grep+sed cleanup commit.

**5c. ✅ DONE in 0.15.0** — auto-cleared by the §3 routing fix; `ollama_*` fields are now read in `dispatch_chat` / `dispatch_stream_inner` to compute the routing decision.

**5c (original spec text):** Three `--all-features` clippy errors about `ollama_native, ollama_think, ollama_keep_alive, ollama_num_ctx` "never read" — this overlaps with #3 above. Resolving #3 (option A) also clears these.

---

## Suggested release sequencing

| Release | Includes | Trigger |
|---|---|---|
| **0.14.2** ✅ published 2026-05-16 | `anthropic_base_url` setter/getter | Live on crates.io; capo follow-up unblocked |
| **0.14.3** ✅ published 2026-05-16 | #2 (claude `--verbose` + NDJSON parser) + #4 docs + #5a cleanup | Live on crates.io; capo can bump dep and re-run smoke |
| **0.15.0** ✅ published 2026-05-17 | #3 (auto-switch + option B + option C) + #5b + #5c | Live on crates.io; followups.md §3/§5 closed |

---

## Done criteria for this spec

- [x] Section 1: 0.14.2 tagged + published to crates.io.
- [x] Section 2: investigation completed, root cause documented in `docs/superpowers/notes/2026-05-16-cli-provider-smoke-debug.md`, fix shipped in 0.14.3.
- [x] Section 3: Ollama HTTP gap addressed via auto-switch + option B + option C. Decision recorded in the 0.15.0 CHANGELOG.
- [x] Section 4: docs added to both CLI provider modules (shipped in 0.14.3).
- [x] Section 5: §5a done in 0.14.3; §5b + §5c done in 0.15.0.
- [x] Release plan executed per the table above (0.14.2 + 0.14.3 + 0.15.0 all published).

## Out of scope (defer to a separate spec)

- Adding new providers (e.g. Mistral, Cohere) — no demand surfaced.
- Refactoring the dispatch pattern in `client.rs::dispatch_chat` / `dispatch_stream` — works fine; refactoring without a use case is premature.
- The Python SDK at `sdks/python/` — separate maintainer track.
- `agent-tool` / `agent-loop` crates — separate spec.

---

## Long-term backlog (post-§2–§5, not for 0.14.x / 0.15.0)

Two ideas surfaced by reading [@earendil-works/pi-ai](https://github.com/earendil-works/pi/tree/main/packages/ai) (v0.74.0, TS/npm equivalent unified-LLM package) on 2026-05-16. Recording here so they don't get lost, but **explicitly out of §2–§5 scope** — neither is reachable through the current followups.

### B1. Built-in `Provider::Faux` for downstream testing

**What:** A canned-response provider that downstream consumers (capo, motosan-agent-loop, any future consumer) can use in tests without spinning up a mockito server. Returns scripted `ChatResponse` / `StreamEvent` sequences from a builder.

**Reference:** pi-ai's `packages/ai/src/providers/faux.ts` (~15 KB). It's a first-class provider in their registry, gated behind no feature flag.

**Motivating example:** capo's M2 smoke test had to spin up mockito to test motosan-ai dispatch wiring. A built-in faux provider would have cut that to ~5 LOC of canned responses.

**Why deferred:** Pure ergonomic improvement, no caller is currently blocked. Likely behind a `faux` feature flag to keep the default release surface small, but the implementation details (flag layout, builder shape, default behaviour) are left to the implementer. Earliest fit: 0.16.0 or later.

### B2. Re-evaluate stream error model — errors-as-events vs `Result<BoxStream, _>`

**What:** pi-ai encodes stream-terminal errors **into the stream itself** as an `AssistantMessage` with `stopReason="error"` + `errorMessage`. motosan-ai currently splits failure between (a) `Result<BoxStream, MotosanError>` at stream-acquisition time and (b) per-event error variants once the stream is live.

**Trade-off:** pi-ai's model gives consumers a single failure-handling site (the stream loop). motosan-ai's model surfaces setup failures synchronously, which lets the caller short-circuit before spinning up consumer state (UI, downstream tasks, telemetry) for a run that was never going to start. Neither is universally better.

**Why deferred:** Changing this is **breaking** for everyone currently consuming `Result<BoxStream, _>` (capo, motosan-agent-loop). Worth revisiting only when (a) we hit a real UX problem downstream, or (b) we're cutting a 1.0 anyway. Earliest fit: 1.0 release planning, not a 0.x patch.

### Not borrowed from pi-ai

- **Registry-based dispatch** (`Map<string, RegisteredApiProvider>` vs our `enum Provider`) — our enum is simpler and works for the closed set of providers we ship. Don't refactor.
- **Auto-generated model registry** (`models.generated.ts`, 448 KB) — useful for IDE autocomplete but heavy; motosan-ai keeps models as opaque strings on purpose. Don't pursue.
- **Image generation / Bedrock / Cloudflare AI Gateway / Vertex** — no demand from current consumers.

---

## Reference: how the related capo work threads through

For context, the consuming project capo (`~/Projects/wade/capo/`):

- M3 Phase 0 merged 2026-05-16 (PR #10) — Settings + Auth + AGENTS.md walk-up.
- Multi-provider side-quest merged 2026-05-16 (PR #11) — added `Settings::model.provider = "anthropic"|"claude-code"|"codex-cli"` dispatch via this SDK. **This is the consumer whose smoke surfaced #2.**
- After this spec's section 1 is done (0.14.2 published), capo has a ~20 LOC follow-up to bump dep + add `Settings::anthropic.base_url` + thread through. That's tracked separately on the capo side; this spec doesn't need to address it.

capo's existing memory at `~/.claude/projects/-Users-daiwanwei-Projects-wade-capo/memory/capo_project.md` has a "multi-provider side-quest — merged" section with full details if the motosan-ai-side worker needs context.

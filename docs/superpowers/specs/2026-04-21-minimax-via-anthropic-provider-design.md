# MiniMax via AnthropicProvider — Design

**Date:** 2026-04-21
**Scope:** Rust SDK only. Python SDK is a separate follow-up.
**Target release:** `motosan-ai` Rust SDK `v0.14.0` (breaking).

## Status (updated 2026-04-21, third pass — verification)

**Implementation 100% complete across all 5 slices. Verification passed.** Still uncommitted.

Verification run:
- `cargo test --features minimax` → **144 passed, 1 ignored** (35 suites)
- `cargo clippy --all-features --all-targets -- -D warnings` (the CI gate used in `.github/workflows/ci-rust.yml`) → **no issues**
- `cargo fmt --check` → clean
- `uv run ruff check motosan_ai/providers/minimax.py` → all checks passed
- `uv run pytest` → all passed (7 skipped, consistent with live-only tests)

Side finding: `sdks/python/motosan_ai/providers/minimax.py` has a 1-line ruff modernization (`isinstance(exc, (A, B))` → `isinstance(exc, A | B)`). Unrelated to the Rust refactor but can ride along.

Still not committed — 27 modified + 6 untracked. Next step is commit strategy (single feature commit vs. split by slice).

## Problem

Our current `MinimaxProvider` (`sdks/rust/src/providers/minimax.rs`, 682 lines) targets MiniMax's OpenAI-compatible `/v1/chat/completions` endpoint and defaults to the obsolete `MiniMax-Text-01` model. MiniMax's current official entry point is the **Anthropic-compatible** `/anthropic/v1/messages` endpoint, and the only models they accept on that endpoint are `MiniMax-M2.7` and `MiniMax-M2.7-highspeed` (pi-mono CHANGELOG confirms; older direct model IDs removed upstream).

Maintaining a bespoke OpenAI-compat provider means:
- Duplicating streaming/tool-use/retry logic that already exists in `AnthropicProvider`.
- Missing features (capabilities declaration, prompt caching wiring) that Anthropic provider has.
- Diverging from the pi-mono reference, which treats MiniMax as an Anthropic-messages variant identified only by `baseUrl`.

## Goal

Route MiniMax through the existing `AnthropicProvider`, mirroring the Ollama pattern (`ollama = ["openai"]` alias + `client.rs::build_ollama_provider()` factory). Delete the bespoke `MinimaxProvider`.

## Non-goals

- Python SDK port (tracked separately).
- Adding MiniMax models beyond `MiniMax-M2.7` and `MiniMax-M2.7-highspeed`.
- Vision/multimodal — MiniMax `/anthropic` endpoint is text-only per pi-mono's `input: ["text"]` declaration.
- Prompt caching tuning — inherits whatever `AnthropicProvider` already does.

## Design

### Architecture mirrors Ollama

Ollama's OpenAI-compat path has **no dedicated provider struct**. Instead:
- `Cargo.toml`: `ollama = ["openai"]` is an alias feature.
- `client.rs::build_ollama_provider()` constructs `OpenAIProvider` with Ollama base_url + auth style.
- `Provider::Ollama` enum variant routes to this factory.

We apply the same pattern to MiniMax:
- `Cargo.toml`: `minimax = ["anthropic"]` alias.
- `client.rs::build_minimax_provider()` constructs `AnthropicProvider` with MiniMax base_url + capabilities.
- `Provider::Minimax` enum variant routes to this factory.
- CN variant handled via `LlmClientBuilder::minimax_base_url(...)` override (defaults to `https://api.minimax.io/anthropic`, CN users override to `https://api.minimaxi.com/anthropic`).

### Prerequisite: instance-level capabilities on `AnthropicProvider`

Currently `AnthropicProvider::capabilities()` (`anthropic.rs:407`) hardcodes `ProviderCapabilities::full()`. MiniMax is text-only (`ProviderCapabilities::text_only()`), so the capabilities must become an instance field.

```rust
pub struct AnthropicProvider {
    // ...existing fields...
    capabilities: ProviderCapabilities,
}

impl AnthropicProvider {
    pub fn new(...) -> Self {
        Self { capabilities: ProviderCapabilities::full(), /* ... */ }
    }
    pub fn with_capabilities(mut self, caps: ProviderCapabilities) -> Self {
        self.capabilities = caps; self
    }
}

impl ProviderImpl for AnthropicProvider {
    fn capabilities(&self) -> ProviderCapabilities { self.capabilities }
}
```

### `build_minimax_provider` factory

```rust
#[cfg(feature = "minimax")]
fn build_minimax_provider(&self) -> crate::providers::anthropic::AnthropicProvider {
    use crate::providers::anthropic::AnthropicProvider;
    use crate::types::ProviderCapabilities;

    let model = self.model.clone().unwrap_or_else(|| "MiniMax-M2.7".to_string());
    let base_url = self.minimax_base_url.clone()
        .unwrap_or_else(|| "https://api.minimax.io/anthropic".to_string());

    AnthropicProvider::new(self.api_key.clone(), Some(model), Some(base_url))
        .with_capabilities(ProviderCapabilities::text_only())
        .with_retry_policy(self.retry_policy.clone())
}
```

### Public API changes (breaking, 0.14.0)

**Removed:**
- `motosan_ai::providers::minimax` module (entire file).
- `MinimaxProvider` struct and all its methods.
- `DEFAULT_MINIMAX_MODEL` constant (`src/models.rs`).
- `LlmClientBuilder::minimax_expose_reasoning(bool)` — Anthropic protocol's `thinking` blocks supersede the flag.
- `tests/minimax_provider.rs` — 760 lines of OpenAI-compat mock tests, all obsolete.

**Added:**
- `LlmClientBuilder::minimax_base_url(String)` — defaults to intl endpoint.
- `AnthropicProvider::with_capabilities(ProviderCapabilities)` builder.

**Unchanged (DX preserved):**
- `Provider::Minimax` enum variant still routes via `LlmClient::builder().provider(Provider::Minimax)`.
- `Cargo.toml` `features = ["minimax"]` still compiles (now a no-op alias enabling `anthropic`).

### Migration example

```rust
// Before (0.13.x)
let client = LlmClient::builder()
    .provider(Provider::Minimax)
    .api_key(key)
    .model("MiniMax-Text-01")
    .minimax_expose_reasoning(true)   // REMOVED
    .build();

// After (0.14.0)
let client = LlmClient::builder()
    .provider(Provider::Minimax)
    .api_key(key)
    .model("MiniMax-M2.7")            // was MiniMax-Text-01
    // reasoning now surfaces as Anthropic thinking blocks automatically
    .build();

// CN users
let client = LlmClient::builder()
    .provider(Provider::Minimax)
    .api_key(key)
    .minimax_base_url("https://api.minimaxi.com/anthropic")
    .build();
```

Direct `MinimaxProvider::new(...)` callers must migrate to `LlmClient::builder().provider(Provider::Minimax)` or construct `AnthropicProvider::new(key, Some("MiniMax-M2.7".into()), Some("https://api.minimax.io/anthropic".into()))` directly.

## Slices

Each slice is independently mergeable. Slice 3 is the single breaking cut.

### Slice 1 — Instance-level capabilities on `AnthropicProvider` (non-breaking, patch) ✅ DONE
- Add `capabilities: ProviderCapabilities` field.
- `new()` defaults to `ProviderCapabilities::full()`.
- Add `with_capabilities(caps)` builder.
- `capabilities()` returns the field.
- Existing Anthropic tests stay green.

**Files:** `src/providers/anthropic.rs`.
**Tests:** Add one unit test proving `with_capabilities(text_only())` overrides the default.

### Slice 2 — MiniMax factory + feature alias (soft-breaking, minor) ✅ DONE

Compiles without source changes for all downstream callers. One behavior change: `minimax_expose_reasoning(true)` becomes a silent no-op because `build_minimax_provider` now returns `AnthropicProvider`, which surfaces reasoning via `thinking` content blocks regardless of the flag. Intentional: callers who genuinely relied on the `<think>`-block stripping behavior must migrate, but no compile break. CHANGELOG note required.
- `Cargo.toml`: change `minimax = ["dep:reqwest", ...]` to `minimax = ["anthropic"]`.
- `client.rs`: add `minimax_base_url: Option<String>` field on builder + `LlmClientBuilder::minimax_base_url(String)` method.
- `client.rs`: rewrite `build_minimax_provider()` to return `AnthropicProvider` as shown above.
- `client.rs`: routing in `chat()` / `stream()` still matches `Provider::Minimax` but now delegates to the Anthropic-backed factory.
- `tests/`: new `anthropic_minimax_routing.rs` mock HTTP test — verifies request goes to `/anthropic/v1/messages`, includes correct model, correct capabilities reported.
- **Still keep `src/providers/minimax.rs` and `tests/minimax_provider.rs` compiling** — the old struct is orphaned (no longer referenced by `client.rs`) but not yet deleted. This slice is non-breaking because nobody who uses `Provider::Minimax` routing sees a public API change (except `minimax_expose_reasoning` which we defer deletion to Slice 3).

**Files:** `sdks/rust/Cargo.toml`, `src/client.rs`, `tests/anthropic_minimax_routing.rs` (new).

**Open decision in this slice:** Does Slice 2 keep `minimax_expose_reasoning` as a no-op until Slice 3? → Yes. Slice 2 must not break callers; deprecate it in Slice 3.

### Slice 3 — Delete `MinimaxProvider` (**breaking**, 0.14.0) ✅ DONE
- Delete `src/providers/minimax.rs`.
- Delete `tests/minimax_provider.rs` (760 lines, obsolete).
- `src/providers/mod.rs`: remove `pub mod minimax;` + any re-exports.
- `src/models.rs`: delete `DEFAULT_MINIMAX_MODEL` constant.
- `src/client.rs`: delete `minimax_expose_reasoning` field + builder method.
- `tests/error_mapping.rs`: delete MiniMax `base_resp`-specific tests (`/anthropic` endpoint returns Anthropic-shaped errors; the MiniMax quirk only applied to the OpenAI-compat path). If any remaining MiniMax error behavior is testable, it rides on Anthropic error mapping.
- `tests/client_builder.rs`, `tests/tool_use_integration.rs`: remove any references to `MinimaxProvider` / `minimax_expose_reasoning`.
- Verify `cargo check --all-features` and `cargo test --all-features` pass.

**Files:** `sdks/rust/src/providers/minimax.rs` (delete), `src/providers/mod.rs`, `src/models.rs`, `src/client.rs`, `src/lib.rs`, `tests/minimax_provider.rs` (delete), `tests/error_mapping.rs`, `tests/client_builder.rs`, `tests/tool_use_integration.rs`.

### Slice 4 — Live integration test (non-breaking) ✅ DONE

`tests/minimax_live.rs` now covers 5 scenarios (214 lines):
- `live_minimax_chat_basic_returns_text` — non-streaming chat returns text
- `live_minimax_tool_use_roundtrip` — tool-use round trip
- `live_minimax_thinking_blocks_exposed` — reasoning surfaces as Anthropic `thinking` blocks
- `live_minimax_stream_propagates_max_tokens_stop_reason` — stream `max_tokens` → `StopReason::MaxTokens`
- `live_minimax_collect_stream_records_max_tokens_on_chat_response` — same via `collect_stream`

All gated on `MINIMAX_API_KEY` env var; skip cleanly when unset.
- Rewrite `tests/minimax_live.rs`:
  - Use `LlmClient::builder().provider(Provider::Minimax).api_key(env::var("MINIMAX_API_KEY"))`.
  - Coverage: basic chat, streaming, tool use, thinking-block surfacing (reasoning arrives as Anthropic `thinking` content blocks).
  - Follow the structure of existing `anthropic_live.rs` and the `gemini_vision_live` precedent from recent commits.
- Gated behind env var presence; not run in default `check-all`.

**Files:** `sdks/rust/tests/minimax_live.rs` (rewrite).

### Slice 5 — Documentation + release ✅ DONE

- `sdks/rust/Cargo.toml` — bumped to `0.14.0` ✓
- `sdks/rust/CHANGELOG.md` — full `## [0.14.0]` section covering Breaking / Added / Changed / Tests ✓
- `sdks/rust/README.md` — MiniMax section rewritten (endpoint, `minimax_base_url`, text-only caps, CN example) ✓
- `skills/motosan-ai/SKILL.md` — default model table + stream `done` invariant note ✓
- `llms.txt` — provider table + default model ✓
- `CLAUDE.md` — provider rules + serialization paragraph reflect Anthropic-compat routing ✓
- Publish workflow: not modified; existing `motosan-ai-oauth` workflow (commit `5015a96`) is independent of MiniMax rename. No change needed.
- `sdks/rust/CHANGELOG.md`: 0.14.0 entry with breaking-change callouts and migration snippet.
- `sdks/rust/README.md`: update MiniMax example to new API.
- `skills/motosan-ai/SKILL.md`: rewrite MiniMax section to reflect Anthropic-compat routing.
- `llms.txt`: update provider matrix.
- `CLAUDE.md` (project root): review the MiniMax-related guidance (provider-logic paragraph, serialization-differences paragraph) and update any text that implies MiniMax uses OpenAI wire format.
- `sdks/rust/Cargo.toml`: bump to `0.14.0`.
- Add publish workflow entry if needed (reference recent `motosan-ai-oauth` publish workflow commit `5015a96`).

**Files:** documentation + `Cargo.toml` version bump.

## Risks and open questions

1. **Anthropic `thinking` block handling**: We assume `AnthropicProvider` already surfaces `thinking` content blocks in `ChatResponse` and streaming events in a way that's useful for MiniMax M2.7 reasoning. To verify in Slice 1 or Slice 2. If not, reasoning surfacing may need its own small follow-up.

2. **`minimax` feature alias collision**: Anyone today who does `features = ["minimax"]` without `["anthropic"]` currently compiles because the old `minimax.rs` pulled in its own deps. After Slice 2, `minimax = ["anthropic"]` auto-enables `anthropic` so they still compile. The only way this breaks is if someone depends on `anthropic` feature being *absent* while `minimax` is present — unlikely, but call out in CHANGELOG.

3. **Error response shape from `/anthropic` endpoint**: pi-mono reuses `anthropic.ts` verbatim, strongly implying the endpoint returns Anthropic-shaped errors. If MiniMax still sometimes returns `base_resp.status_code != 0` on this endpoint, `AnthropicProvider` error mapping might swallow the message. Slice 4 live tests should exercise an invalid-key path to confirm.

4. **`minimax_expose_reasoning` removal — any downstream users?** In-repo grep finds no external users; we control both SDKs + `motosan-chat`. Confirmed safe to delete in Slice 3.

## Success criteria

- `cargo test --all-features` passes after every slice.
- `cargo check --no-default-features --features minimax` compiles and exercises only the Anthropic-backed path.
- `tests/minimax_live.rs` passes against real MiniMax API with `MINIMAX_API_KEY` set.
- `LlmClient::builder().provider(Provider::Minimax).api_key(k).build().chat(req)` works without any MiniMax-specific config for the common case.
- Deleted line count (`minimax.rs` ~682 + `tests/minimax_provider.rs` ~760 = ~1,440 lines gone) exceeds added line count.

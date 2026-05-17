# Ollama HTTP Wiring + Clippy Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the silent no-op where `ClientBuilder::ollama_think` / `ollama_keep_alive` / `ollama_num_ctx` are dropped on the HTTP path (default Ollama mode), add a build-time guard rejecting nonsensical combinations, clarify setter docs, and clear the 10 `unneeded return` clippy warnings in `client.rs`. Ship as motosan-ai 0.15.0.

**Architecture:** Verified by reading [Ollama's openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go) on 2026-05-17 — the OpenAI-compat `/v1/chat/completions` endpoint silently drops `keep_alive`, `options.num_ctx`, and (top-level) `think` because its Go struct `ChatCompletionRequest` doesn't declare them and `encoding/json` discards unknown fields. The only place these fields are honored is Ollama's native `/api/chat` endpoint (used by motosan-ai's `OllamaProvider`).

Therefore the fix is a **transport switch**, not a body-injection: `build_ollama_provider` and the `Provider::Ollama` dispatch arms now auto-route to `OllamaProvider` (native `/api/chat`) whenever any of the 3 fields is set, falling back to the existing `OpenAIProvider` (`/v1/chat/completions`) path when none of them are. The `ollama_native(true)` runtime flag is retained but becomes redundant when any field is set (it still forces native when set explicitly with zero fields). The Cargo `ollama_native` feature is collapsed into `ollama` so `OllamaProvider` is always available when `ollama` is — no new deps, since `ollama` already pulls reqwest/tokio/tokio-stream via its `openai` dependency, and we add the small `bytes` dep that `ollama_native` previously gated.

**Tech Stack:** Rust 1.82, `OllamaProvider` (existing, `sdks/rust/src/providers/ollama.rs`, currently gated `cfg(feature = "ollama_native")` — Task 1 makes it available under `cfg(feature = "ollama")` too), `OpenAIProvider` (existing, retained as the default-path backend), `mockito` for HTTP integration tests. No type-erased dispatch needed: each branch of the routing `if` calls `.chat().await` / `.stream().await` on its own concrete provider type independently.

**Spec source:** `docs/superpowers/specs/2026-05-16-motosan-ai-followups.md` §3 + §5b + §5c.

**Why minor (0.15.0) not patch:** three technically-breaking surfaces — the build-time guard (Task 5), the Cargo feature collapse (Task 1, callers enabling only `ollama` now also pull `bytes`), and the auto-switch behavior loss of image capability (auto-switched callers can no longer send images even though they previously could via the OpenAI-compat path; see Task 6 setter docs and Task 8 CHANGELOG for the full warning). Semver requires minor for any of these alone.

**Pre-existing limitations NOT addressed in 0.15.0:**
- `OllamaProvider::build_request_body` at `sdks/rust/src/providers/ollama.rs:138-140` serializes `think` as a hard-coded boolean `true` whenever `self.think.is_some()`, ignoring the actual stored string value (so `ollama_think("yes")` and `ollama_think("no")` produce identical bodies). This pre-dates the plan. The auto-switch fix routes to OllamaProvider correctly; the boolean-coercion bug is a separate followup.

---

## File Structure

- **Modify:** `sdks/rust/Cargo.toml` — fold `ollama_native` deps into `ollama` feature; `ollama_native` becomes an alias.
- **Modify:** `sdks/rust/src/providers/mod.rs` (or `lib.rs` re-exports) — change `pub use providers::ollama` cfg gate from `ollama_native` to `ollama`.
- **Modify:** `sdks/rust/src/providers/ollama.rs:1` — change the top-level `#[cfg(...)]` (if any) from `ollama_native` to `ollama`.
- **Modify:** `sdks/rust/src/client.rs:225-247, 380-402` — `Provider::Ollama` arms in `dispatch_chat` + `dispatch_stream_inner` now auto-route based on whether any of the 3 fields is set.
- **Modify:** `sdks/rust/src/client.rs:549-582` — `build_ollama_provider` and `build_ollama_native_provider` retained as-is internally; the routing happens at dispatch.
- **Modify:** `sdks/rust/src/client.rs:768-781` — setter doc-comments updated to explain the auto-switch.
- **Modify:** `sdks/rust/src/client.rs::ClientBuilder::build` (around line 870) — add validation guard for `ollama_*` on non-Ollama provider.
- **Modify:** `sdks/rust/src/client.rs` — remove 10 `unneeded return` statements.
- **Create:** `sdks/rust/tests/ollama_http_autoswitch.rs` — mockito tests covering both routing branches (with-fields → `/api/chat`, without-fields → `/v1/chat/completions`).
- **Modify:** `sdks/rust/tests/client_builder.rs` — unit tests for the new validation guard.
- **Modify:** `sdks/rust/Cargo.toml` — version 0.14.3 → 0.15.0.
- **Modify:** `sdks/rust/CHANGELOG.md` — `## [0.15.0] - 2026-05-17` entry (substantive breaking note about the routing change).
- **Modify:** `AGENTS.md`, `llms.txt`, `README.md`, `sdks/rust/README.md`, `skills/motosan-ai/SKILL.md`, `skills/motosan-ai/references/rust-api.md` — version bumps.

---

## Task 1: Collapse `ollama_native` Cargo feature into `ollama`

**Files:**
- Modify: `sdks/rust/Cargo.toml:30-37`
- Modify: `sdks/rust/src/providers/ollama.rs:1-10` (if it has a top-level `#![cfg(...)]` — check first)
- Modify: `sdks/rust/src/lib.rs` and/or `sdks/rust/src/providers/mod.rs` — find `pub use ... ollama` cfg gates

- [ ] **Step 1: Verify current cfg gates on OllamaProvider**

Run: `grep -rnE "cfg.*ollama_native|pub use.*ollama" sdks/rust/src/ | head -20`

Record all hits — these are the places that need to change. Expected hits include `lib.rs` (re-export), `providers/mod.rs` (module declaration), `client.rs` (function gates), possibly the top of `ollama.rs` itself.

- [ ] **Step 2: Update Cargo.toml feature definition**

In `sdks/rust/Cargo.toml:30-37`, replace:

```toml
ollama = ["openai"]
ollama_native = [
  "ollama",
  "dep:reqwest",
  "dep:tokio",
  "dep:tokio-stream",
  "dep:bytes",
]
```

with:

```toml
ollama = [
  "openai",
  "dep:reqwest",
  "dep:tokio",
  "dep:tokio-stream",
  "dep:bytes",
]
# Retained as an alias for backwards compatibility. The `ollama_native(true)`
# runtime flag still controls explicit routing — see ClientBuilder::ollama_native.
# As of 0.15.0 the OllamaProvider (/api/chat) is also auto-selected whenever
# ollama_think / ollama_keep_alive / ollama_num_ctx is set, regardless of this
# feature, because Ollama's OpenAI-compat endpoint silently drops those fields.
ollama_native = ["ollama"]
```

Note: `reqwest`, `tokio`, `tokio-stream` are already pulled by `openai`. Only `bytes` is genuinely new for `ollama` consumers.

- [ ] **Step 3: Update cfg gates on production code only (NOT tests)**

Switch `#[cfg(feature = "ollama_native")]` → `#[cfg(feature = "ollama")]` ONLY on the production-code sites that make `OllamaProvider` reachable:

- `sdks/rust/src/lib.rs` — the `pub use providers::ollama::OllamaProvider;` re-export gate
- `sdks/rust/src/providers/mod.rs` — the `pub mod ollama;` declaration gate
- `sdks/rust/src/providers/ollama.rs` — if the file has a top-level `#![cfg(...)]`, change it (verify with `head -5 sdks/rust/src/providers/ollama.rs`)
- `sdks/rust/src/client.rs:571` — the `#[cfg(feature = "ollama_native")]` on `fn build_ollama_native_provider` (Tasks 2-3 will call this from the new routing arms which themselves live under `cfg(feature = "ollama")`)
- `sdks/rust/src/client.rs` lines ~226 and ~380 (the OUTER `#[cfg(feature = "ollama_native")]` annotation BEFORE the `{ if self.ollama_native { ... } }` blocks — find with `grep -n 'cfg(feature = "ollama_native")' sdks/rust/src/client.rs`). These will be restructured in Tasks 2-3 anyway, but in Task 1 just relax the gate from `ollama_native` to `ollama` so the code still compiles between commits.

**Do NOT touch** test files that intentionally gate on `ollama_native`:
- `tests/ollama_native_provider.rs` — exercises the explicit-native flag path; correctly keeps `cfg(feature = "ollama_native")` so it only runs when the user opts in.

If Step 1's grep surfaced any other sites not listed above, evaluate them individually — the rule is "if the site makes OllamaProvider compile / dispatch, switch it; if it gates an opt-in test or example, leave it."

- [ ] **Step 4: Verify the codebase compiles under both feature combos**

Run:

```bash
cargo build --features ollama
cargo build --features ollama_native
cargo build --all-features
```

Expected: all three succeed with no errors. Warnings about `ollama_native` being a no-op alias are acceptable and will be fixed in CHANGELOG documentation.

- [ ] **Step 4b: Verify the existing native-only test still triggers**

`tests/ollama_native_provider.rs` keeps its `cfg(feature = "ollama_native")` gate, so it should only compile-and-run when `ollama_native` is explicitly enabled. Confirm the gate didn't accidentally over-restrict:

```bash
cargo test --features ollama_native --test ollama_native_provider -- --list 2>&1 | head -10
```

Expected: at least one test name listed (not "0 tests"). If empty, the cfg substitution in Step 3 was too aggressive — re-check that `tests/ollama_native_provider.rs:1` still references `ollama_native` and that no production-code change made any of its imports unavailable under that gate.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/Cargo.toml sdks/rust/src/
git commit -m "refactor(rust): make OllamaProvider available under the 'ollama' Cargo feature

Folds the ollama_native feature's deps (bytes) into the ollama feature,
turning ollama_native into a pure alias retained for backwards
compatibility. Cfg gates on OllamaProvider switched from ollama_native
to ollama.

Motivation: 0.15.0's auto-switch fix for the silent-no-op on
ollama_keep_alive/num_ctx/think needs OllamaProvider available whenever
the ollama feature is enabled, since the OpenAI-compat /v1/chat/
completions path doesn't honor those fields server-side (verified
against Ollama's openai.go).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Auto-route dispatch_chat to OllamaProvider when fields are set

**Files:**
- Modify: `sdks/rust/src/client.rs:225-247` (the `Provider::Ollama` arm in `dispatch_chat`)

- [ ] **Step 1: Write the failing test**

Create `sdks/rust/tests/ollama_http_autoswitch.rs`:

```rust
#![cfg(feature = "ollama")]

use mockito::Matcher;
use motosan_ai::{Client, Message, Provider};

#[tokio::test]
async fn ollama_with_keep_alive_routes_to_api_chat_endpoint() {
    // With ollama_keep_alive set, the client must POST to /api/chat
    // (native) rather than /v1/chat/completions (OpenAI-compat), because
    // the OpenAI-compat endpoint silently drops keep_alive server-side.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(r#"\"keep_alive\"\s*:\s*\"10m\""#.to_string()))
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "llama3",
                "message": {"role": "assistant", "content": "ok"},
                "done": true,
                "done_reason": "stop",
                "prompt_eval_count": 1,
                "eval_count": 1
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_keep_alive("10m")
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat against mock should succeed");
    mock.assert_async().await;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --features ollama --test ollama_http_autoswitch ollama_with_keep_alive_routes_to_api_chat_endpoint -- --nocapture`

Expected: FAIL — current `Provider::Ollama` arm always routes to `OpenAIProvider`, which would POST to `/v1/chat/completions` not `/api/chat`. The mockito mock asserts on `/api/chat` and is never hit, so `mock.assert_async()` panics.

- [ ] **Step 3: Write minimal implementation**

In `sdks/rust/src/client.rs`, locate the `Provider::Ollama` arm in `dispatch_chat` (currently around lines 225-247 — verify with `grep -n "Provider::Ollama" sdks/rust/src/client.rs | head -2`). Replace the arm body with:

```rust
            Provider::Ollama => {
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    // Auto-route to OllamaProvider (native /api/chat) when
                    // ollama_native is explicitly enabled OR any of the
                    // Ollama-specific tuning fields is set, since the
                    // OpenAI-compat /v1/chat/completions endpoint silently
                    // drops keep_alive / options.num_ctx / think
                    // server-side. Otherwise stay on the OpenAI-compat
                    // path for backwards compatibility.
                    //
                    // Capability trade-off: OllamaProvider is text-only
                    // (no image capability) while the OpenAI-compat path
                    // declares with_image(). Auto-switching strips image
                    // capability — the wrapped validate_request error
                    // below tells the caller WHY their image input
                    // stopped working.
                    let needs_native = self.ollama_native
                        || self.ollama_keep_alive.is_some()
                        || self.ollama_num_ctx.is_some()
                        || self.ollama_think.is_some();
                    if needs_native {
                        let p = self.build_ollama_native_provider();
                        p.validate_request(&request).map_err(|e| match e {
                            MotosanError::UnsupportedFeature(msg) => MotosanError::UnsupportedFeature(format!(
                                "{msg} — Provider::Ollama was auto-routed to the native /api/chat endpoint \
                                 because one of ollama_keep_alive / ollama_num_ctx / ollama_think is set, \
                                 and the native endpoint is text-only. Either remove the tuning field(s) to \
                                 stay on the OpenAI-compat path (which supports images), or remove the image \
                                 input."
                            )),
                            other => other,
                        })?;
                        p.chat(request).await
                    } else {
                        let p = self.build_ollama_provider();
                        p.validate_request(&request)?;
                        p.chat(request).await
                    }
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("ollama"))
                }
            }
```

Note: this also incidentally removes 2 of the 10 `unneeded return` warnings from the arm body — leaving 8 for Task 7. Good.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --features ollama --test ollama_http_autoswitch ollama_with_keep_alive_routes_to_api_chat_endpoint`

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/client.rs sdks/rust/tests/ollama_http_autoswitch.rs
git commit -m "fix(rust): chat() auto-routes Ollama to native /api/chat when tuning fields set

Provider::Ollama dispatch now picks OllamaProvider (native /api/chat)
whenever any of ollama_keep_alive / ollama_num_ctx / ollama_think is
set, because Ollama's OpenAI-compat /v1/chat/completions endpoint
silently drops these fields server-side (verified against
ollama/openai.go). Without any of these fields set, the dispatch
stays on the existing OpenAIProvider HTTP path for backwards
compatibility.

mockito test in tests/ollama_http_autoswitch.rs locks in the routing
decision end-to-end.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Auto-route dispatch_stream to OllamaProvider when fields are set

**Files:**
- Modify: `sdks/rust/src/client.rs:380-402` (the `Provider::Ollama` arm in `dispatch_stream_inner`)

- [ ] **Step 1: Write the failing test**

Append to `sdks/rust/tests/ollama_http_autoswitch.rs`:

```rust
#[tokio::test]
async fn ollama_with_num_ctx_streams_from_api_chat_endpoint() {
    use tokio_stream::StreamExt;

    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/api/chat")
        .match_body(Matcher::Regex(
            r#"\"options\"\s*:\s*\{[^}]*\"num_ctx\"\s*:\s*4096"#.to_string(),
        ))
        .match_body(Matcher::Regex(r#"\"stream\"\s*:\s*true"#.to_string()))
        .with_status(200)
        // Minimal NDJSON: one chunk + a done marker.
        .with_body(
            "{\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"hi\"},\"done\":false}\n\
             {\"model\":\"llama3\",\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"done_reason\":\"stop\",\"prompt_eval_count\":1,\"eval_count\":1}\n",
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .ollama_num_ctx(4096)
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream against mock should open");
    let mut seen_text = false;
    while let Some(event) = stream.next().await {
        if !event.content.is_empty() {
            seen_text = true;
        }
    }
    assert!(seen_text, "stream should yield at least one text chunk");
    mock.assert_async().await;
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --features ollama --test ollama_http_autoswitch ollama_with_num_ctx_streams_from_api_chat_endpoint`

Expected: FAIL — dispatch_stream_inner still routes to OpenAIProvider for `Provider::Ollama`. The mock on `/api/chat` is never hit.

- [ ] **Step 3: Write minimal implementation**

In `sdks/rust/src/client.rs`, locate the `Provider::Ollama` arm in `dispatch_stream_inner` (currently around lines 380-402 — verify with `grep -n "Provider::Ollama" sdks/rust/src/client.rs`). Replace the arm body with the symmetric structure from Task 2:

```rust
            Provider::Ollama => {
                #[cfg(feature = "ollama")]
                {
                    use crate::providers::ProviderImpl;
                    // Same auto-switch + capability trade-off as
                    // dispatch_chat — keep the routing condition
                    // identical so chat and stream are never split.
                    let needs_native = self.ollama_native
                        || self.ollama_keep_alive.is_some()
                        || self.ollama_num_ctx.is_some()
                        || self.ollama_think.is_some();
                    if needs_native {
                        let p = self.build_ollama_native_provider();
                        p.validate_request(&request).map_err(|e| match e {
                            MotosanError::UnsupportedFeature(msg) => MotosanError::UnsupportedFeature(format!(
                                "{msg} — Provider::Ollama was auto-routed to the native /api/chat endpoint \
                                 because one of ollama_keep_alive / ollama_num_ctx / ollama_think is set, \
                                 and the native endpoint is text-only. Either remove the tuning field(s) to \
                                 stay on the OpenAI-compat path (which supports images), or remove the image \
                                 input."
                            )),
                            other => other,
                        })?;
                        p.stream(request).await
                    } else {
                        let p = self.build_ollama_provider();
                        p.validate_request(&request)?;
                        p.stream(request).await
                    }
                }
                #[cfg(not(feature = "ollama"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("ollama"))
                }
            }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --features ollama --test ollama_http_autoswitch`

Expected: both tests PASS.

Also run the full Ollama-relevant test suites to confirm no regression:

```
cargo test --features ollama --test ollama_http_autoswitch --test client_builder
cargo test --features ollama --test ollama_native_provider
```

Expected: all PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/client.rs sdks/rust/tests/ollama_http_autoswitch.rs
git commit -m "fix(rust): stream() auto-routes Ollama to native /api/chat when tuning fields set

Symmetric counterpart to the previous commit, for the streaming path.
mockito test asserts /api/chat receives the body with options.num_ctx
and stream:true.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Backwards-compat test + wrapped-error test

**Files:**
- Modify: `sdks/rust/tests/ollama_http_autoswitch.rs` (add two tests)

- [ ] **Step 1: Write the backwards-compat test**

Append to `sdks/rust/tests/ollama_http_autoswitch.rs`:

```rust
#[tokio::test]
async fn ollama_without_tuning_fields_stays_on_openai_compat_endpoint() {
    // Regression guard: callers who don't set any of the 3 tuning fields
    // and don't enable ollama_native(true) should continue to hit the
    // OpenAI-compat /v1/chat/completions endpoint as in 0.14.x. This
    // preserves backwards compatibility for the common case.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "llama3",
                "choices": [{
                    "message": {"role": "assistant", "content": "ok"},
                    "finish_reason": "stop"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url(server.url())
        .build()
        .expect("build client");

    let _ = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("chat should succeed on the openai-compat fallback");
    mock.assert_async().await;
}
```

- [ ] **Step 2: Write the image-capability wrapped-error test**

Append to the same file:

```rust
#[tokio::test]
async fn ollama_with_tuning_field_plus_image_returns_wrapped_error() {
    // When the auto-switch fires AND the request has image content, the
    // text-only OllamaProvider's validate_request rejects it. The
    // dispatch arm wraps that rejection with the auto-switch context so
    // the caller knows WHY images stopped working.
    use motosan_ai::MotosanError;

    // No mockito server needed — validate_request fires before any HTTP
    // call, so the request never leaves the client.
    let client = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_base_url("http://example.invalid")
        .ollama_keep_alive("5m") // triggers auto-switch
        .build()
        .expect("build client");

    let request = motosan_ai::ChatRequest::builder()
        .message(Message::user_with_image("describe this", "abc123", "image/png"))
        .build();

    let err = client
        .chat_with(request)
        .await
        .expect_err("validate_request should reject image on text-only OllamaProvider");

    match err {
        MotosanError::UnsupportedFeature(msg) => {
            assert!(
                msg.contains("auto-routed") && msg.contains("text-only"),
                "wrapped error should explain the auto-switch context, got: {msg}"
            );
            assert!(
                msg.contains("ollama_keep_alive") || msg.contains("ollama_num_ctx") || msg.contains("ollama_think"),
                "wrapped error should mention the field that triggered the switch, got: {msg}"
            );
        }
        other => panic!("expected UnsupportedFeature, got: {other:?}"),
    }
}
```

- [ ] **Step 3: Run both tests**

Run: `cargo test --features ollama --test ollama_http_autoswitch ollama_without_tuning_fields_stays_on_openai_compat_endpoint ollama_with_tuning_field_plus_image_returns_wrapped_error`

Expected: both PASS. The wrapped-error test verifies the `.map_err(|e| match e ...)` block added in Tasks 2-3 actually fires.

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/tests/ollama_http_autoswitch.rs
git commit -m "test(rust): backwards-compat + wrapped-error tests for Ollama auto-switch

Two tests covering the routing edges:
- no tuning fields → /v1/chat/completions path unchanged (backwards
  compat with 0.14.x callers)
- tuning field + image content → validate_request error is wrapped with
  the auto-switch context so the caller knows WHY images stopped
  working

Together with Tasks 2 + 3's positive-path mockito tests, the four tests
in tests/ollama_http_autoswitch.rs cover all four cells of the
{routing-branch} x {input-shape} matrix.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Build-time guard for `ollama_*` fields without `Provider::Ollama`

**Files:**
- Modify: `sdks/rust/src/client.rs::ClientBuilder::build` (around line 870 — find with `grep -n "pub fn build" sdks/rust/src/client.rs`)
- Modify: `sdks/rust/tests/client_builder.rs` (add unit test)

- [ ] **Step 1: Write the failing tests**

Append to `sdks/rust/tests/client_builder.rs`:

```rust
#[test]
fn build_rejects_ollama_fields_with_non_ollama_provider() {
    // ollama_keep_alive set but provider is OpenAI — should error at build()
    // rather than silently dropping the value at runtime.
    let result = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .ollama_keep_alive("10m")
        .build();
    let err = result.expect_err("should reject ollama_* on non-Ollama provider");
    let msg = format!("{err}");
    assert!(
        msg.contains("ollama_keep_alive") && msg.contains("Provider::Ollama"),
        "error message should name the field + correct provider, got: {msg}"
    );
}

#[test]
fn build_accepts_ollama_fields_with_ollama_provider() {
    // Sanity guard: the validation should NOT fire for the correct provider.
    let result = Client::builder()
        .provider(Provider::Ollama)
        .api_key("ollama")
        .ollama_keep_alive("10m")
        .ollama_num_ctx(8192)
        .ollama_think("yes")
        .build();
    assert!(result.is_ok(), "valid combo should build");
}
```

- [ ] **Step 2: Run tests to verify the first fails**

Run: `cargo test --features ollama --test client_builder build_rejects_ollama_fields_with_non_ollama_provider build_accepts_ollama_fields_with_ollama_provider`

Expected: `build_accepts_*` PASSES (existing behavior). `build_rejects_*` FAILS (build currently succeeds where it should error).

- [ ] **Step 3: Write minimal implementation**

In `sdks/rust/src/client.rs`, locate `pub fn build(self) -> Result<Client, MotosanError>` (around line 870 — confirm with `grep -n "pub fn build" sdks/rust/src/client.rs`). Insert this validation block at the top of `build()`, immediately after the existing provider/api_key checks:

```rust
        // Guard: ollama_* setters only make sense when the selected
        // provider routes through Ollama (either path — OpenAI-compat or
        // auto-switched native). Catching this at build time prevents the
        // silent no-op that motivated the 0.15.0 fix.
        if !matches!(self.provider, Some(crate::providers::Provider::Ollama)) {
            let mut misused: Vec<&str> = Vec::new();
            if self.ollama_keep_alive.is_some() {
                misused.push("ollama_keep_alive");
            }
            if self.ollama_num_ctx.is_some() {
                misused.push("ollama_num_ctx");
            }
            if self.ollama_think.is_some() {
                misused.push("ollama_think");
            }
            if !misused.is_empty() {
                return Err(MotosanError::Config(format!(
                    "{} can only be used with Provider::Ollama",
                    misused.join(", ")
                )));
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --features ollama --test client_builder build_rejects_ollama_fields_with_non_ollama_provider build_accepts_ollama_fields_with_ollama_provider`

Expected: both PASS.

Also run the full client_builder test suite to confirm no other tests regressed:

```
cargo test --features ollama,anthropic,openai --test client_builder
```

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/client.rs sdks/rust/tests/client_builder.rs
git commit -m "feat(rust)!: ClientBuilder::build() rejects ollama_* on non-Ollama provider

Previously ClientBuilder silently accepted ollama_keep_alive /
ollama_num_ctx / ollama_think on any provider, then dropped the values
at request time. Now build() returns MotosanError::Config listing the
misused fields, so the mistake fails fast and visibly.

BREAKING: any caller currently mis-using ollama_* with Provider::OpenAI
etc. will now get a build error. Likely zero affected callers (they
would have noticed the silent drop already), but cataloged as breaking
for semver discipline — contributes to the 0.15.0 minor bump.

Closes followups.md §3 option B.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Update setter doc-comments (Option C)

**Files:**
- Modify: `sdks/rust/src/client.rs:768-781` (the three setters: `ollama_think`, `ollama_keep_alive`, `ollama_num_ctx`)

- [ ] **Step 1: Write the implementation**

In `sdks/rust/src/client.rs`, replace each of the three setters (currently around lines 768-781) with the documented versions below:

```rust
    /// Set Ollama's `think` parameter controlling whether the model emits
    /// a thinking block. Forwarded only via the native `/api/chat`
    /// endpoint — Ollama's OpenAI-compatible `/v1/chat/completions`
    /// endpoint silently drops this field (verified against
    /// [ollama/openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go)).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider (native `/api/chat`) regardless of whether
    /// `ollama_native(true)` was called.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
    ///
    /// **Known limitation:** OllamaProvider currently emits this as a
    /// boolean `think: true` regardless of the input string value (see
    /// `providers/ollama.rs:138-140`). The string is accepted for forward
    /// compatibility but not differentiated server-side.
    pub fn ollama_think(mut self, think: impl Into<String>) -> Self {
        self.ollama_think = Some(think.into());
        self
    }

    /// Set Ollama's `keep_alive` duration (e.g. `"5m"`, `"-1"` for forever).
    /// Controls how long Ollama keeps the model loaded after a request.
    /// Forwarded only via the native `/api/chat` endpoint — the
    /// OpenAI-compatible endpoint silently drops this field (verified
    /// against ollama/openai.go).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider regardless of `ollama_native(true)`.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
    pub fn ollama_keep_alive(mut self, duration: impl Into<String>) -> Self {
        self.ollama_keep_alive = Some(duration.into());
        self
    }

    /// Set Ollama's `options.num_ctx` (context window size in tokens).
    /// Forwarded only via the native `/api/chat` endpoint — the
    /// OpenAI-compatible endpoint silently drops nested `options` fields
    /// (verified against ollama/openai.go).
    ///
    /// **Side effect (since 0.15.0):** setting this auto-routes the client
    /// to OllamaProvider regardless of `ollama_native(true)`.
    ///
    /// **Capability trade-off:** OllamaProvider is text-only. If you set
    /// this AND send image content in the request, `validate_request`
    /// will reject the request with a wrapped error explaining the
    /// auto-switch context.
    ///
    /// `ClientBuilder::build()` will return `Err(MotosanError::Config)` if you
    /// set this on a non-`Provider::Ollama` client.
    pub fn ollama_num_ctx(mut self, tokens: u32) -> Self {
        self.ollama_num_ctx = Some(tokens);
        self
    }
```

- [ ] **Step 2: Verify build still passes**

Run: `cargo build --features ollama,openai`

Expected: clean build, no warnings.

- [ ] **Step 3: Also update the `ollama_native` setter to acknowledge the auto-switch**

Find `pub fn ollama_native(mut self, ...)` in `client.rs` (likely around line 765 — `grep -n "fn ollama_native" sdks/rust/src/client.rs`). **DO NOT TOUCH THE FUNCTION BODY** — only replace the doc-comment block (the `///` lines) immediately above the function signature. The body stays exactly as it currently exists in the file.

New doc-comment block to place above the existing signature:

```rust
    /// Force the Ollama provider to use the native `/api/chat` endpoint
    /// instead of the OpenAI-compatible `/v1/chat/completions` path.
    ///
    /// As of 0.15.0, setting this explicitly is **only required** when you
    /// want the native endpoint without setting any tuning fields. Setting
    /// `ollama_think` / `ollama_keep_alive` / `ollama_num_ctx` auto-selects
    /// the native endpoint regardless of this flag, since the OpenAI-compat
    /// endpoint silently drops those fields.
    ///
    /// **Capability trade-off:** the native endpoint is text-only — image
    /// inputs will be rejected. The OpenAI-compatible default supports
    /// images.
```

Concrete procedure for the executor:
1. Read the current contents of `client.rs` around the `pub fn ollama_native` signature (use Read with a 20-line window).
2. Identify the existing `///` doc-comment lines above the signature.
3. Use Edit's `old_string` = "the existing doc-comment block exactly as it appears" and `new_string` = the block shown above. Do not include the function signature or body in either string — that guarantees the body cannot be accidentally altered.

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/src/client.rs
git commit -m "docs(rust): document the 0.15.0 Ollama auto-switch behavior on all 4 setters

Per followups.md §3 option C — the doc-comments now make clear that
the three Ollama tuning fields auto-route through the native /api/chat
endpoint (the OpenAI-compat one silently drops them), and that the
ollama_native(true) flag is now optional when any tuning field is set.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: §5b — remove remaining `unneeded return` statements in `client.rs`

**Files:**
- Modify: `sdks/rust/src/client.rs` — line numbers will have shifted after Tasks 2-3 collapsed the Ollama arms. Get the current list with `cargo clippy --features anthropic,minimax,ollama,openai --all-targets 2>&1 | grep -B2 needless_return`

- [ ] **Step 1: Capture the current warning list**

Run:

```bash
cargo clippy --features anthropic,minimax,ollama,openai --all-targets 2>&1 \
  | grep -E "warning|needless_return|src/client.rs:[0-9]+" \
  | head -40
```

Save the listed line numbers. The original count was 10; Tasks 2-3 should have removed 2 (the Ollama dispatch arms now use expression form, not `return`). Expect ~8 remaining, all in `client.rs` dispatch arms.

- [ ] **Step 2: Apply the cleanup**

For each warning, remove the leading `return ` and the trailing `;`. Example:

Before:
```rust
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    return Err(Self::feature_not_enabled("anthropic"));
                }
```

After:
```rust
                #[cfg(not(feature = "anthropic"))]
                {
                    let _ = request;
                    Err(Self::feature_not_enabled("anthropic"))
                }
```

Apply this transformation to all listed occurrences. Verify visually that each occurrence is the last expression in its enclosing block.

- [ ] **Step 3: Verify clippy is now clean**

Run:

```bash
cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings
```

Expected: command exits 0 with no output (i.e. 0 errors, 0 warnings).

The `never_read` warning on `ollama_*` fields (§5c in the spec) should also be gone — Tasks 2-3 now READ the fields in dispatch.

- [ ] **Step 4: Verify tests still pass**

Run: `cargo test --features anthropic,minimax,ollama,openai`

Expected: all tests PASS.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/client.rs
git commit -m "style(rust): drop remaining unneeded return statements in client.rs dispatch

Pure clippy cleanup of the trailing 'return ...;' as the final
expression of dispatch arms in dispatch_chat / dispatch_stream /
dispatch_stream_inner. Now expressions instead of statements.

The Ollama-arm occurrences were already cleaned up incidentally in the
previous routing-fix commits. This commit finishes the sweep on the
remaining ~8 arms.

Closes followups.md §5b. §5c (the 'never read' warning on ollama_*
fields) was auto-cleared by the §3 routing fix which now reads them.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 8: Bump 0.14.3 → 0.15.0 + CHANGELOG + release-checklist docs

**Files:**
- Modify: `sdks/rust/Cargo.toml`
- Modify: `sdks/rust/CHANGELOG.md`
- Modify: `AGENTS.md`, `llms.txt`, `README.md`, `sdks/rust/README.md`, `skills/motosan-ai/SKILL.md`, `skills/motosan-ai/references/rust-api.md`

- [ ] **Step 1: Bump Cargo.toml**

In `sdks/rust/Cargo.toml`, change `version = "0.14.3"` to `version = "0.15.0"`.

- [ ] **Step 2: Add CHANGELOG entry**

In `sdks/rust/CHANGELOG.md`, insert immediately after the `# Changelog` / intro lines and BEFORE `## [0.14.3]`:

```markdown
## [0.15.0] - 2026-05-17

### Fixed
- **Ollama HTTP path now honors `ollama_keep_alive` / `ollama_num_ctx` / `ollama_think`.** Previously these three `ClientBuilder` setters were wired only to the explicit native path (`ollama_native(true)`). HTTP-path callers (the default) silently dropped them, and even forwarding them to the OpenAI-compat `/v1/chat/completions` endpoint would have been theatrical — verified against [ollama/openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go), Ollama's OpenAI-compat handler's `ChatCompletionRequest` struct silently discards these fields server-side. Fix: `dispatch_chat` and `dispatch_stream` now auto-route to `OllamaProvider` (native `/api/chat`) whenever any of the three fields is set, regardless of the `ollama_native(true)` flag. Closes followups.md §3.
- Clippy `needless_return` cleanup: removed `return ...;` statements in `client.rs` dispatch arms. The `never_read` warning on `ollama_*` fields was auto-cleared by the routing fix. Closes followups.md §5b + §5c.

### Changed (BREAKING)
- **`ClientBuilder::build()` now returns `Err(MotosanError::Config)` if `ollama_keep_alive` / `ollama_num_ctx` / `ollama_think` are set on a non-`Provider::Ollama` client.** Previously these were silently accepted then dropped. The error message names the misused field(s). Closes followups.md §3 option B. Likely zero affected callers in practice (the silent drop was undetected), but cataloged as breaking for semver discipline.
- **Cargo feature `ollama_native` is now an alias for `ollama`.** Previously `ollama_native` added the `bytes` dep that the native `OllamaProvider` needs. To support the new auto-routing behavior, `ollama` now pulls `bytes` too; `ollama_native` is retained as a feature name for backwards compatibility but is a no-op. Existing `Cargo.toml` files with `features = ["ollama_native"]` continue to compile unchanged. Existing `features = ["ollama"]` callers will get a small dep tree increase (`bytes` ~80 KB plus its transitive closure) even when they don't trigger the native path. No workaround if you want `ollama` without `bytes` — accept and document.
- **`ClientBuilder::ollama_native(true)` is no longer the only way to reach the native `/api/chat` endpoint.** Setting any of the three tuning fields now also routes there. The flag remains a valid escape hatch for callers who want native dispatch without setting any tuning fields.
- **Image-capability loss when auto-routed.** `Provider::Ollama` callers who simultaneously set any of the three tuning fields AND send image content will now get a wrapped `MotosanError::UnsupportedFeature` from `validate_request`. The OpenAI-compatible path declares `with_image()` capability; `OllamaProvider` is text-only. Affected callers should either drop the tuning field (and lose the field's effect) or drop the image input (and use a different model). The error message explains the trade-off; see also the setter docs on `ollama_think` / `ollama_keep_alive` / `ollama_num_ctx`.

### Notes
- Setter doc-comments on `ollama_think` / `ollama_keep_alive` / `ollama_num_ctx` / `ollama_native` updated to describe the auto-switch behavior and the build-time guard.
- mockito integration tests in `tests/ollama_http_autoswitch.rs` lock in the routing behavior end-to-end for both branches (with-fields → `/api/chat`, without-fields → `/v1/chat/completions`).
```

- [ ] **Step 3: Bump version strings in release-checklist docs**

Apply these substitutions (every `0.14.3` → `0.15.0` and every `v0.14.3` → `v0.15.0`) in:

- `AGENTS.md` line 5: `Rust v0.14.3 (crates.io)` → `Rust v0.15.0 (crates.io)`
- `llms.txt` line 5: `Python 0.10.0 · Rust 0.14.3` → `Python 0.10.0 · Rust 0.15.0`
- `llms.txt` line 22: `motosan-ai = { version = "0.14.3"` → `motosan-ai = { version = "0.15.0"`
- `README.md` line 29: `| Rust | ... | v0.14.3 |` → `| Rust | ... | v0.15.0 |`
- `README.md` line 37: `motosan-ai = { version = "0.14.3"` → `motosan-ai = { version = "0.15.0"`
- `sdks/rust/README.md` lines 320, 429, 492: three `motosan-ai = { version = "0.14.3"` → `motosan-ai = { version = "0.15.0"`
- `skills/motosan-ai/SKILL.md` line 8: `Multi-provider LLM SDK — Python 0.10.0 / Rust 0.14.3` → `Multi-provider LLM SDK — Python 0.10.0 / Rust 0.15.0`
- `skills/motosan-ai/SKILL.md` line 23: `motosan-ai = { version = "0.14.3"` → `motosan-ai = { version = "0.15.0"`
- `skills/motosan-ai/references/rust-api.md` line 7: `motosan-ai = { version = "0.14.3"` → `motosan-ai = { version = "0.15.0"`

Confirm completeness with:

```
grep -rn "0\.14\.3\|v0\.14\.3" /Users/daiwanwei/Projects/wade/motosan-ai \
    --include="*.md" --include="*.txt" 2>/dev/null \
    | grep -v "CHANGELOG\|docs/superpowers\|target/"
```

Expected: empty output.

- [ ] **Step 4: Commit**

```bash
git add sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md AGENTS.md llms.txt README.md sdks/rust/README.md skills/motosan-ai/SKILL.md skills/motosan-ai/references/rust-api.md
git commit -m "chore(rust): bump 0.14.3 -> 0.15.0 + CHANGELOG + release-checklist docs

Per CLAUDE.md release process. 0.15.0 minor bump (vs patch) is
required by the breaking ClientBuilder::build() validation guard +
the Cargo feature ollama/ollama_native collapse.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

## Task 9: Run check-all + pre-push-gate

**Files:** none (verification only)

- [ ] **Step 1: Run check-all**

Run: `check-all`

Expected: `=== All checks passed ===` (Rust + Python lint + tests all green).

- [ ] **Step 2: Run pre-push-gate**

Run: `./scripts/pre-push-gate.sh`

Expected: `=== Pre-push gate PASSED ===` (4 stages including live tests if `ANTHROPIC_API_KEY` is resolvable via direnv).

If either fails, fix the underlying issue and re-run before proceeding to Task 10.

---

## Task 10: PR → merge → tag rust-v0.15.0 → publish + close followups

**Files:** none (release operations) — plus a final docs-only commit on `main` after publish

- [ ] **Step 1: Create branch + push**

If not already on a feature branch:

```bash
git checkout -b feat/ollama-http-autoswitch
git push -u origin feat/ollama-http-autoswitch
```

If already on a feature branch from Tasks 1-8, just push:

```bash
git push -u origin <current-branch>
```

- [ ] **Step 2: Open PR**

```bash
gh pr create --base main --title "fix(rust): Ollama HTTP auto-switches to /api/chat for tuning fields (v0.15.0)" --body "$(cat <<'EOF'
## Summary

Closes followups.md §3 + §5b + §5c. Ships as 0.15.0 minor bump because of the breaking validation guard + Cargo feature restructure.

- **§3 fix**: `Provider::Ollama` dispatch now auto-routes to `OllamaProvider` (native `/api/chat`) whenever any of `ollama_keep_alive` / `ollama_num_ctx` / `ollama_think` is set. The original spec recommended forwarding fields through the OpenAI-compat path; reading [ollama/openai.go](https://github.com/ollama/ollama/blob/main/openai/openai.go) confirmed Ollama silently drops those fields server-side, so a transport switch is the only honest fix.
- **§3 option B**: `ClientBuilder::build()` now returns `Err(MotosanError::Config)` listing the misused field names when `ollama_*` setters are used with a non-Ollama provider.
- **§3 option C**: Setter doc-comments updated to describe the auto-switch and reference ollama/openai.go.
- **Cargo feature change**: `ollama_native` collapses into `ollama` (alias for backwards compat). The `bytes` dep moves up; `ollama` callers see a tiny binary-size increase.
- **§5b**: `needless_return` clippy warnings in `client.rs` removed.
- **§5c**: `never_read` warning on `ollama_*` fields auto-cleared by the routing fix.

## Commits

(list will be auto-derived from branch; expect ~7-8 commits matching Tasks 1-8)

## Test plan

- [x] mockito test: Ollama + `ollama_keep_alive` → POSTs to `/api/chat` with `keep_alive` in body
- [x] mockito test: Ollama + `ollama_num_ctx` (stream) → POSTs to `/api/chat` with `options.num_ctx` and `stream: true`
- [x] mockito test (backwards-compat): Ollama with NO tuning fields → still POSTs to `/v1/chat/completions`
- [x] Unit test: `ClientBuilder::build()` rejects `ollama_*` on non-Ollama provider (positive + negative)
- [x] `cargo build` clean under `--features ollama`, `--features ollama_native`, `--all-features`
- [x] Existing `tests/ollama_native_provider.rs` still passes (no behavior change for explicit-native callers)
- [x] `check-all` green
- [x] `./scripts/pre-push-gate.sh` green (incl. live Anthropic tests)
- [x] `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings` clean

## Release readiness

After merge, tag `rust-v0.15.0` and push to trigger `publish-rust.yml` → crates.io.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

Note the URL printed by gh — open it in a browser to monitor CI checks.

- [ ] **Step 3: Wait for CI to settle, then merge**

```bash
until [ "$(gh pr view --json mergeStateStatus -q .mergeStateStatus 2>/dev/null)" = "CLEAN" ]; do sleep 30; done
gh pr merge --merge --delete-branch
```

- [ ] **Step 4: Pull main, tag, push**

```bash
git checkout main
git pull --ff-only origin main
git tag -a rust-v0.15.0 -m "rust-v0.15.0 — Ollama HTTP auto-switches to /api/chat

Fixes silent no-op where ollama_keep_alive / ollama_num_ctx /
ollama_think were dropped on the HTTP path. Now auto-routes to
OllamaProvider (native /api/chat) when any of these is set, since
Ollama's OpenAI-compat endpoint silently drops them server-side.

Adds build-time guard rejecting these fields on non-Ollama providers
(breaking). Collapses ollama_native feature into ollama.

Also clears remaining needless_return clippy warnings in client.rs.

Closes followups.md §3 + §5b + §5c."
git push origin rust-v0.15.0
```

- [ ] **Step 5: Watch publish workflow, verify crates.io**

```bash
sleep 5
RUN_ID=$(gh run list --workflow=publish-rust.yml --branch=rust-v0.15.0 --limit 1 --json databaseId -q '.[0].databaseId')
until [ "$(gh run view $RUN_ID --json status -q .status)" = "completed" ]; do sleep 45; done
gh run view $RUN_ID --json conclusion
```

Expected: `{"conclusion": "success"}`.

Then verify crates.io published the new version:

```bash
rtk proxy curl -sA "motosan-ai-check/1.0" "https://crates.io/api/v1/crates/motosan-ai/0.15.0" \
  | python3 -c "import sys,json; v=json.load(sys.stdin)['version']; print(v['num'], v['created_at'], 'yanked=' + str(v['yanked']))"
```

Expected: `0.15.0 <ISO timestamp> yanked=False`.

- [ ] **Step 6: Update followups.md to mark §3 + §5b + §5c done**

In `docs/superpowers/specs/2026-05-16-motosan-ai-followups.md`, apply the following edits. Substitute `<MERGE_SHA>` (from `git log -1 main`) and `<RELEASE_DATE>` (the crates.io `created_at` date in `YYYY-MM-DD` form) with the actual values.

(a) Replace the §3 heading and prepend a "Resolved" block. Before:

```markdown
## 3. HIGH — Ollama HTTP path silently ignores three builder fields

**Files:** `sdks/rust/src/client.rs` lines 20-24, 235-240, 578-580.
```

After:

```markdown
## 3. ✅ DONE — Ollama HTTP path silently ignores three builder fields

**Resolved in 0.15.0** (merge `<MERGE_SHA>`, tag `rust-v0.15.0`, crates.io published `<RELEASE_DATE>`). The fix turned out to require an architectural rethink rather than the wire-through approach the spec originally recommended:

- The OpenAI-compat `/v1/chat/completions` endpoint silently drops `keep_alive`, `options.num_ctx`, and `think` (verified against ollama/openai.go — `ChatCompletionRequest` struct doesn't declare them, Go's encoding/json discards unknown fields). Wiring the fields through the OpenAI-compat body would have been theatrical.
- Instead, `Provider::Ollama` dispatch now auto-routes to `OllamaProvider` (native `/api/chat`) whenever any of the 3 fields is set. The OpenAI-compat path is retained as the default for callers who don't set any of these fields.
- `ClientBuilder::build()` returns `Err(MotosanError::Config)` if `ollama_*` fields are set on a non-`Provider::Ollama` client (option B).
- Setter doc-comments updated to describe the auto-switch (option C).
- Cargo feature `ollama_native` collapsed into `ollama` so OllamaProvider is available whenever `ollama` is; `ollama_native` retained as alias.

mockito tests in `tests/ollama_http_autoswitch.rs` cover both routing branches (with-fields → `/api/chat`, without-fields → `/v1/chat/completions`).

---

The original spec text below is retained for historical record.

**Files:** `sdks/rust/src/client.rs` lines 20-24, 235-240, 578-580.
```

(b) Replace the §5b paragraph. Before:

```markdown
**5b. 10+ `unneeded return` clippy warnings** across the codebase (pre-existing, surfaced when running `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings`). Pure style cleanup. Not blocking but worth a single grep+sed cleanup commit.
```

After:

```markdown
**5b. ✅ DONE in 0.15.0** — all `unneeded return` warnings in `client.rs` dispatch arms removed. Some were cleared incidentally by the §3 routing fix; the rest by a follow-on sweep. `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings` now clean.

**5b (original spec text):** 10+ `unneeded return` clippy warnings across the codebase (pre-existing, surfaced when running `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings`). Pure style cleanup. Not blocking but worth a single grep+sed cleanup commit.
```

(c) Replace the §5c paragraph. Before:

```markdown
**5c. Three `--all-features` clippy errors** about `ollama_native, ollama_think, ollama_keep_alive, ollama_num_ctx` "never read" — this overlaps with #3 above. Resolving #3 (option A) also clears these.
```

After:

```markdown
**5c. ✅ DONE in 0.15.0** — auto-cleared by the §3 routing fix; `ollama_*` fields are now read in `dispatch_chat` / `dispatch_stream_inner` to compute the routing decision.

**5c (original spec text):** Three `--all-features` clippy errors about `ollama_native, ollama_think, ollama_keep_alive, ollama_num_ctx` "never read" — this overlaps with #3 above. Resolving #3 (option A) also clears these.
```

(d) In the "Suggested release sequencing" table, replace the `**0.15.0 (minor)**` row. Before:

```markdown
| **0.15.0 (minor)** | #3 (option A or B — both arguably breaking) + #5b clippy cleanup | Cut whenever scope warrants; both items are pure cleanup, not capo-blocking |
```

After:

```markdown
| **0.15.0** ✅ published `<RELEASE_DATE>` | #3 (auto-switch + option B + option C) + #5b + #5c | Live on crates.io; followups.md §3/§5 closed |
```

(e) Update the Done criteria checklist. Before:

```markdown
- [ ] Section 3: Ollama HTTP gap addressed via option A / B / C. Decision recorded in the commit message.
- [x] Section 4: docs added to both CLI provider modules (shipped in 0.14.3).
- [ ] Section 5: §5a done in 0.14.3; §5b (clippy `unneeded return` mass cleanup) still open for 0.15.0.
- [ ] Release plan executed per the table above (0.14.2 + 0.14.3 done; 0.15.0 pending).
```

After:

```markdown
- [x] Section 3: Ollama HTTP gap addressed via auto-switch + option B + option C. Decision recorded in the 0.15.0 CHANGELOG.
- [x] Section 4: docs added to both CLI provider modules (shipped in 0.14.3).
- [x] Section 5: §5a done in 0.14.3; §5b + §5c done in 0.15.0.
- [x] Release plan executed per the table above (0.14.2 + 0.14.3 + 0.15.0 all published).
```

(f) Update the Status banner at the top of the file. Before:

```markdown
**Status:** In progress — §1, §2, §4, §5a shipped (motosan-ai 0.14.2 + 0.14.3 on crates.io); §3 + §5b remain open for 0.15.0 minor. Ready to hand off to a fresh session running inside `~/Projects/wade/motosan-ai/`.
```

After:

```markdown
**Status:** ✅ All sections (§1–§5) shipped across 0.14.2 / 0.14.3 / 0.15.0 on crates.io. B1 + B2 long-term backlog items remain (post-0.15). Spec retained as historical record of the multi-release work.
```

Commit + push directly to main (docs-only, same pattern as prior `76a0f9f` / `ad19621` spec updates):

```bash
git add docs/superpowers/specs/2026-05-16-motosan-ai-followups.md
git commit -m "docs(superpowers): mark followups §3 + §5b + §5c done (0.15.0 published)"
git push origin main
```

---

## Done criteria

- [ ] All 10 tasks above complete with their final commits landed.
- [ ] motosan-ai 0.15.0 live on crates.io, not yanked.
- [ ] `cargo clippy --features anthropic,minimax,ollama,openai --all-targets -- -D warnings` produces 0 errors and 0 warnings.
- [ ] `cargo build` clean under `--features ollama`, `--features ollama_native`, `--all-features`.
- [ ] §3, §5b, §5c marked DONE in `docs/superpowers/specs/2026-05-16-motosan-ai-followups.md`.
- [ ] No regressions in the Python SDK (pre-push gate stages 1 + 3 verify this).
- [ ] `tests/ollama_native_provider.rs` unchanged and passing (no behavior change for callers who explicitly opted into native via `ollama_native(true)`).

## Out of scope for this plan

- §2.3 codex parser shape audit (separate spec — not blocking after 0.14.3 closed §2 on the claude side).
- `claude_code/mod.rs:445` `let _ = child.wait().await` silent-swallow followup (deferred per 0.14.3 notes; needs a new `StreamEvent::Error` variant design conversation).
- B1 (built-in Faux provider) and B2 (stream error model) backlog items — both explicitly post-§3 per spec.
- Python SDK changes — separate maintainer track.
- Eventually removing the `ollama_native` feature alias entirely (0.16.0 or later).
- Adding an `OpenAIProvider::with_extra_body_params` escape hatch for OTHER OpenAI-compat servers (would have been the original plan's centerpiece; now deferred since Ollama doesn't benefit and no other consumer has asked).

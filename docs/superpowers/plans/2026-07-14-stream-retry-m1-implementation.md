# M1 — Stream & Retry Correctness Patch Wave: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the P0 defects from the 2026-07-14 stream/retry audit — retry silently disabled on real-world 5xx, mid-stream errors converted into fabricated successes, and streamed tool-call corruption — across all three SDKs, plus four S-sized stream-hygiene fixes, with real-wire-shaped regression fixtures for every fix. No public API changes.

**Architecture:** Every fix is a local patch inside the existing client → provider → adapter → collector layering (the audit confirmed the layering is sound). The regression barrier is the new fixture suite: each task first lands a failing test shaped like the real wire (non-JSON 5xx bodies, mid-stream `error` frames, CLI child death, index-keyed parallel tool calls, distinct `fc_…`/`call_…` ids, CRLF SSE, cumulative usage), then the minimal fix. Milestone context: `docs/superpowers/plans/2026-07-14-stream-retry-milestones.md` (this plan is M1; M2–M4 get their own plans).

**Tech Stack:** Rust (tokio, reqwest, mockito/wiremock per existing tests) · Python 3.11+ (httpx, respx, pytest-asyncio `asyncio_mode=auto`) · TypeScript (vitest, undici-style fetch mocks).

## Global Constraints

- **Baseline:** authored 2026-07-14 against `origin/main` @ `3e3f413` (Rust 0.21.1 / Python 0.14.0 / TS 0.11.0). ALL line numbers are approximate. **Execute each task in a worktree off the CURRENT `origin/main` and ground every edit in the real files** — if the code has drifted from a quoted hunk, adapt to reality and note it; do not force-apply stale quotes.
- **House workflow:** every `.rs` / `Cargo.toml` change lands via PR + CI — never direct to main. Suggested PR grouping is listed below; the release task runs LAST, after all M1 PRs merge.
- **No public API changes.** No new required params, no renamed/removed public types. Behavior may change only where the current behavior returns fabricated or corrupted data (that is the point of M1).
- **House rules:** the tool-call field is `input` (never `args`/`params`); `ChatResponse.tool_calls` is always a list, never optional; provider logic stays in `providers/`; read `specs/types.md` before touching serialization; Anthropic `tool_call_id` appears only in `content_block_start`.
- **Commands** — Rust (from `sdks/rust`): `cargo test --all-features …`, `cargo fmt`, `cargo clippy --all-features -- -D warnings`. Python (from `sdks/python`): `uv run pytest tests/… -v`, `uv run ruff check motosan_ai/` (tests/ are NOT linted), `uv run ruff format`. TypeScript (from `sdks/typescript`): `npx vitest run tests/…`, `npm run build && npm test` (the full suite includes `pack-smoke.test.ts`, which requires `dist/` — ALWAYS build before `npm test`), `npm run typecheck` (the TS test dir is `tests/`; there is no ESLint/Prettier gate).
- **Commits:** conventional style `fix(scope): …`, ending with `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- **Ordering constraints** (same-file tasks): Task 6 before Task 7 (`claude_code/mod.rs`); Task 9 before Task 10 (`providers/claude_code.py`); Task 12 before Task 17 (`providers/chatgpt_codex.ts`); Task 18 before Task 19 (`http/sse.ts`). Everything else is order-independent within its PR.

## Suggested PR grouping

| PR | Tasks | Scope |
|---|---|---|
| PR-R1 | 1, 2, 3 | Rust retry parse-order fixes |
| PR-R2 | 5, 6, 7 | Rust stream error surfacing (HTTP + CLI backends) |
| PR-R3 | 13, 14, 21 | Rust tool-call integrity + usage merge |
| PR-P1 | 4, 8, 9, 10 | Python retry visibility + error surfacing |
| PR-P2 | 15, 16 | Python tool-call integrity |
| PR-T1 | 11, 12, 17 | TS error surfacing + codex id map |
| PR-T2 | 18, 19, 20 | TS SSE hygiene + usage merge |
| PR-REL | 22 | Release (after all of the above merge) |

---


## W1 — Un-break retry on real-world 5xx

### Task 1: Fix Anthropic chat() parsing response body before retryable-status check

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` (approximate lines 492–516, inside `chat()`'s retry loop)
- Test: `sdks/rust/tests/anthropic_chat.rs` (extend; ~222 lines, uses mockito)

**Interfaces:** None (self-contained). Internal restructure of `AnthropicProvider::chat()`'s retry loop; no signature or public API changes. Note: `Provider::Minimax` also routes through `AnthropicProvider`, so this fix covers MiniMax too.

**Bug:** `chat()` calls `response.json()` BEFORE checking `is_retryable_status(status)`. A 502/503/529 whose body is HTML or empty (the canonical proxy/load-balancer failure) therefore aborts on attempt 1 with a misleading `ProviderError("error decoding response body")` instead of retrying. The correct template is `gemini.rs` (~lines 347–361): decide retryability from status + Retry-After alone; parse the body only after success (propagate parse failure) or when constructing the terminal error (`unwrap_or(json!({}))` fallback so `extract_error_message` still works).

- [ ] **Step 1: Write the failing test** — In `sdks/rust/tests/anthropic_chat.rs`, first change the import at approximate line 6 from:

```rust
use motosan_ai::{ChatRequest, Message, MotosanError, StopReason, DEFAULT_ANTHROPIC_MODEL};
```

to (rustfmt-wrapped because the line exceeds 100 chars):

```rust
use motosan_ai::{
    ChatRequest, Message, MotosanError, RetryPolicy, StopReason, DEFAULT_ANTHROPIC_MODEL,
};
```

Then append this test at the END of the file (after `chat_with_explicit_max_tokens_overrides_default`, approximate line 221):

```rust
#[tokio::test]
async fn anthropic_chat_retries_503_with_non_json_body() {
    let mut server = mockito::Server::new_async().await;
    // Canonical proxy/LB failure: retryable status with an HTML (non-JSON) body.
    let unavailable = server
        .mock("POST", "/v1/messages")
        .with_status(503)
        .with_body("<html>Service Unavailable</html>")
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/messages")
        .with_status(200)
        .with_body(
            json!({
                "model": DEFAULT_ANTHROPIC_MODEL,
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "recovered"}]
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider =
        AnthropicProvider::new("test-key", None, Some(server.url())).with_retry_policy(policy);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider
        .chat(request)
        .await
        .expect("503 with non-JSON body should be retried, not aborted with a parse error");

    assert_eq!(response.content, "recovered");
    unavailable.assert_async().await;
    success.assert_async().await;
}
```

(`RetryPolicy` builder usage matches the existing pattern in `sdks/rust/tests/openai_retry.rs`. The `.expect(1)` on the 503 mock plus `unavailable.assert_async()`/`success.assert_async()` proves exactly 2 requests were made.)

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/rust`:

```bash
cargo test --all-features --test anthropic_chat anthropic_chat_retries_503_with_non_json_body
```

Expected failure signature (panic on the `.expect(...)`):

```
thread 'anthropic_chat_retries_503_with_non_json_body' panicked at tests/anthropic_chat.rs:...:
503 with non-JSON body should be retried, not aborted with a parse error: ProviderError("error decoding response body")
test result: FAILED. 0 passed; 1 failed
```

- [ ] **Step 3: Implement** — in `sdks/rust/src/providers/anthropic.rs`, inside `async fn chat()`'s `loop`.

Current code (approximate lines 492–510):

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "anthropic request failed");
```

Replace with:

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if status.is_success() {
                payload = response
                    .json()
                    .await
                    .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let error_payload: Value = response.json().await.unwrap_or(json!({}));
            let message = extract_error_message(&error_payload, "anthropic request failed");
```

Leave the lines that follow (`let message = Self::with_auth_hint(...)` through `return Err(map_http_error(...))`) unchanged. Retryability is now decided from status + Retry-After alone; the body is parsed only on success (parse failure still propagates as `ProviderError`) or when building the terminal error (falls back to `json!({})` so `extract_error_message` returns the default message for non-JSON bodies). Do NOT touch `stream()` — it already checks status before consuming the body.

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/rust`:

```bash
cargo test --all-features --test anthropic_chat anthropic_chat_retries_503_with_non_json_body
cargo test --all-features --test anthropic_chat
cargo test --all-features
```

Expected: first command `test result: ok. 1 passed`; second `test result: ok. 7 passed; 0 failed`; third: all suites pass (live-API tests are ignored/skipped without credentials).

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: `cargo fmt` makes no changes to the files as written above; clippy finishes with no warnings.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/anthropic.rs sdks/rust/tests/anthropic_chat.rs
git commit -m "fix(anthropic): check retryable status before parsing chat response body

A 502/503/529 with a non-JSON body (proxy/LB failure) aborted on attempt 1
with a misleading JSON-decode ProviderError instead of retrying. Decide
retryability from status + Retry-After first; parse the body only on
success or for the terminal error (with a json!({}) fallback). Also covers
Provider::Minimax, which routes through AnthropicProvider.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 2: Fix Rust OpenAI chat() retry order: decide retry before parsing error body

**Files:**
- Modify: `sdks/rust/src/providers/openai.rs` (approximate lines 501-526, inside `async fn chat` of `impl ProviderImpl for OpenAIProvider`)
- Test: `sdks/rust/tests/openai_retry.rs` (EXTEND — file already has 5 mockito-based retry tests)

**Interfaces:** Consumes existing crate-private helpers already imported at the top of `openai.rs` (line 3-6): `is_retryable_status(status_code: u16) -> bool`, `parse_retry_after(headers: &HeaderMap) -> Option<Duration>`, `sleep_before_retry(&RetryPolicy, attempt, Option<Duration>)`, `extract_error_message(payload: &Value, fallback: &str) -> String`, `map_http_error(status_code: u16, message: String) -> MotosanError` (all defined in `sdks/rust/src/providers/mod.rs`). Produces: no signature or public API changes — internal behavior fix in `OpenAIProvider::chat` only.

**Context (the bug):** In `chat()`, `response.json()` is awaited and its error is propagated with `?` BEFORE the `is_retryable_status` check. A 502/503 from a proxy with a non-JSON body (HTML or plain text) therefore fails JSON parsing and returns `MotosanError::ProviderError` on attempt 1, never retrying. The `stream()` method in the SAME file (approximate lines 617-634) already does it correctly: status + Retry-After decide retry first; the body is parsed only on the terminal attempt, with a default payload on parse failure (`gemini.rs` chat, approximate lines 346-362, follows the same pattern). Do NOT touch `stream()` — it is already correct.

- [ ] **Step 1: Write the failing test** — append this test at the END of `sdks/rust/tests/openai_retry.rs` (after `openai_stream_retries_initial_call`, approximate line 228). It matches the file's existing mockito style (`Server::new_async`, `.expect(n)`, `.create_async().await`):

```rust
#[tokio::test]
async fn openai_chat_retries_502_with_non_json_body() {
    let mut server = mockito::Server::new_async().await;
    let bad_gateway = server
        .mock("POST", "/v1/chat/completions")
        .with_status(502)
        .with_body("bad gateway")
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "recovered"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_retry_policy(policy);

    let response = provider
        .chat(
            ChatRequest::builder()
                .message(Message::user("hello"))
                .build(),
        )
        .await
        .expect("should retry past non-JSON 502 body and succeed");

    assert_eq!(response.content, "recovered");
    bad_gateway.assert_async().await;
    success.assert_async().await;
}
```

No new imports needed — the file already has `use serde_json::json;`, `OpenAIProvider`, `ProviderImpl`, `ChatRequest`, `Message`, `RetryPolicy` at the top. The two `.assert_async()` calls prove exactly 2 HTTP requests were made (one 502, one 200).

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/rust`:

```bash
cargo test --all-features --test openai_retry openai_chat_retries_502_with_non_json_body
```

Expected failure signature: the test panics at the `.expect(...)` because current code returns a parse error instead of retrying:

```
thread 'openai_chat_retries_502_with_non_json_body' panicked at ...:
should retry past non-JSON 502 body and succeed: ProviderError("error decoding response body...")
...
test result: FAILED. 0 passed; 1 failed
```

- [ ] **Step 3: Implement** — in `sdks/rust/src/providers/openai.rs`, inside `async fn chat`.

Current code (approximate lines 501-525):

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if self.responses_fallback && status.as_u16() == 404 {
                return self.chat_via_responses(&fallback_request).await;
            }

            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "openai request failed");
            return Err(map_http_error(status.as_u16(), message));
```

Replace with (same order as `stream()` in this file: success parse, then retry decision, then terminal error parse with default payload):

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if self.responses_fallback && status.as_u16() == 404 {
                return self.chat_via_responses(&fallback_request).await;
            }

            if status.is_success() {
                payload = response
                    .json()
                    .await
                    .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let error_payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
            let message = extract_error_message(&error_payload, "openai request failed");
            return Err(map_http_error(status.as_u16(), message));
```

Notes: `json!` and `Value` are already imported (line 18: `use serde_json::{json, Value};`). The `payload` variable is declared before the loop as `let payload: Value;` (approximate line 482) — assigning it inside the `if status.is_success()` branch right before `break` compiles fine (definite assignment via break). `extract_error_message(&json!({}), "openai request failed")` returns the fallback string, so a non-JSON terminal error body now yields `map_http_error(status, "openai request failed")` instead of a spurious `ProviderError`.

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/rust`:

```bash
cargo test --all-features --test openai_retry openai_chat_retries_502_with_non_json_body
cargo test --all-features --test openai_retry
cargo test --all-features
```

Expected: first command `test result: ok. 1 passed`; second `test result: ok. 6 passed` (5 existing + 1 new); third: all tests pass, 0 failed.

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diff complaints, clippy exits 0.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/openai.rs sdks/rust/tests/openai_retry.rs
git commit -m "fix(openai): decide retry before parsing chat error body

Non-JSON 5xx bodies (HTML/empty from proxies) no longer abort the
retry loop on attempt 1; body is parsed only on success or on the
terminal attempt, with a default payload on parse failure.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 3: Fix Rust Ollama chat() retry parse-order bug (non-JSON 5xx aborts retry)

**Files:**
- Modify: `sdks/rust/src/providers/ollama.rs` (chat() retry loop, approximate lines 266-285)
- Test: `sdks/rust/tests/ollama_native_provider.rs` (extend; existing mockito-based tests for this provider live here, gated by feature `ollama_native`)

**Interfaces:** Consumes crate-private helpers already imported at the top of `ollama.rs` (lines 3-7, no import changes needed): `is_retryable_status(u16) -> bool`, `parse_retry_after(&HeaderMap) -> Option<Duration>`, `extract_error_message(&Value, fallback: &str) -> String`, `map_http_error(u16, String) -> MotosanError`, `async sleep_before_retry(&RetryPolicy, attempt, Option<Duration>)`. Public API unchanged.

**Context:** `OllamaProvider::chat()` parses the response body as JSON (with `?`) BEFORE checking whether the HTTP status is retryable. Real-world 5xx responses from Ollama behind a proxy/load balancer are often plain text (e.g. `Service Unavailable`), so the JSON parse fails and chat() returns `MotosanError::ProviderError` on attempt 1 instead of retrying. `stream()` in the same file and `GeminiProvider::chat()` (`sdks/rust/src/providers/gemini.rs` ~346-362) already do it correctly: check status first, parse the error body tolerantly.

- [ ] **Step 1: Write the failing test** — in `sdks/rust/tests/ollama_native_provider.rs`, first extend the import on approximate line 6:

```rust
// Current (approximate line 6):
use motosan_ai::{ChatRequest, Message, StopReason, StreamEventType, Tool, DEFAULT_OLLAMA_MODEL};
// Replace with:
use motosan_ai::{
    ChatRequest, Message, RetryPolicy, StopReason, StreamEventType, Tool, DEFAULT_OLLAMA_MODEL,
};
```

Then append this test at the end of the file (same mockito style as the rest of the file and as `tests/openai_retry.rs`; the file already has `fn build_provider(base_url: String) -> OllamaProvider` at the top):

```rust
#[tokio::test]
async fn ollama_native_chat_retries_non_json_5xx_then_succeeds() {
    let mut server = mockito::Server::new_async().await;
    let error_mock = server
        .mock("POST", "/api/chat")
        .with_status(503)
        .with_header("content-type", "text/plain")
        .with_body("Service Unavailable")
        .expect(1)
        .create_async()
        .await;
    let success_mock = server
        .mock("POST", "/api/chat")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({
                "model": "llama3.2",
                "message": {"role": "assistant", "content": "recovered"},
                "done": true,
                "prompt_eval_count": 3,
                "eval_count": 2
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    let provider = build_provider(server.url()).with_retry_policy(policy);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider
        .chat(request)
        .await
        .expect("should retry and succeed");

    assert_eq!(response.content, "recovered");
    assert!(matches!(response.stop_reason, StopReason::Stop));
    // Exactly 2 requests total: one 503, one 200.
    error_mock.assert_async().await;
    success_mock.assert_async().await;
}
```

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/rust`:

```bash
cargo test --all-features --test ollama_native_provider ollama_native_chat_retries_non_json_5xx_then_succeeds
```

Expected failure signature: the test panics at the `.expect("should retry and succeed")` line with approximately:

```
thread 'ollama_native_chat_retries_non_json_5xx_then_succeeds' panicked at ...:
should retry and succeed: ProviderError("error decoding response body...")
```

(the buggy code turns the non-JSON 503 body into a JSON-decode `ProviderError` on attempt 1 and never retries). If instead it fails with a mock assertion error about the 503 mock not being hit, re-check that both mocks use path `/api/chat`.

- [ ] **Step 3: Implement** — in `sdks/rust/src/providers/ollama.rs`, inside `async fn chat` (the `loop` body, right after the `let response = match ... send().await` block).

Current code (approximate lines 266-285):

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let current_payload: Value = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

            if status.is_success() {
                payload = current_payload;
                break;
            }

            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = extract_error_message(&current_payload, "ollama request failed");
            return Err(map_http_error(status.as_u16(), message));
```

Replace with (status checked BEFORE any body parse; error body parsed tolerantly, same pattern as `gemini.rs` chat()):

```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if !status.is_success() {
                if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16())
                {
                    attempt += 1;
                    sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                    continue;
                }
                let error_payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
                let message = extract_error_message(&error_payload, "ollama request failed");
                return Err(map_http_error(status.as_u16(), message));
            }

            payload = response
                .json()
                .await
                .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
            break;
```

Notes: `json!` and `Value` are already imported (`use serde_json::{json, Value};`, approximate line 14). Do NOT touch `stream()` in the same file — it already checks status before reading the body. The success path (`payload = response.json() ... ?`) keeps its existing strict `ProviderError` behavior for a 200 with a non-JSON body.

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/rust`:

```bash
cargo test --all-features --test ollama_native_provider ollama_native_chat_retries_non_json_5xx_then_succeeds
cargo test --all-features --test ollama_native_provider
cargo test --all-features
```

Expected: the new test passes (`test ollama_native_chat_retries_non_json_5xx_then_succeeds ... ok`), all existing `ollama_native_provider` tests still pass, and the full suite is green (live-API tests are ignored/skipped without credentials).

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diff-breaking rustfmt changes beyond your edit, zero clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/ollama.rs sdks/rust/tests/ollama_native_provider.rs
git commit -m "fix(ollama): check retryable status before parsing chat error body"
```

### Task 4: Make Python OpenAI/MiniMax/ChatGPT-Codex 5xx errors visible to the retry classifier

**Why:** `sdks/python/motosan_ai/retry.py` decides whether a `ProviderError` is retryable by regexing its MESSAGE for `\b5\d{2}\b` (approx. lines 29, 46-51) and scrapes `Retry-After` out of the message text (approx. lines 32-37). AnthropicProvider builds messages as `"HTTP {status}: {body}"` with a `"Retry-After: {v}\n"` prefix when the header is present (`providers/anthropic.py` approx. lines 282-296), so its 5xx errors retry. OpenAI, MiniMax, and ChatGPT-Codex raise `ProviderError(<raw body text>)` with no status embedded, so a genuine 502/503 from them is NEVER retried, and codex drops the Retry-After header outright. Copy the anthropic message format exactly. Additionally, MiniMax `chat()` currently calls `response.json()` before checking the status code, so a non-JSON 5xx body (e.g. an HTML gateway page) crashes with `json.JSONDecodeError` instead of raising `ProviderError` — the fix reorders that. No public API changes; message text only.

**Files:**
- Modify: `sdks/python/motosan_ai/providers/openai.py` (approx. lines 150-156, 165-166, 213-215)
- Modify: `sdks/python/motosan_ai/providers/minimax.py` (approx. lines 143-150, 215-218)
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py` (approx. lines 341-343)
- Test: `sdks/python/tests/test_openai.py`, `sdks/python/tests/test_minimax.py`, `sdks/python/tests/test_chatgpt_codex_http.py`, `sdks/python/tests/test_retry.py` (extend all four)

**Interfaces:** Consumes (unchanged): `ProviderError` from `motosan_ai/error.py`; `with_retry(fn, max_retries=3, initial_backoff=0.1, max_backoff=2.0)`, `_is_retryable(error: Exception) -> bool`, `_parse_retry_after(error_message: str) -> float | None` from `motosan_ai/retry.py`. Produces: non-2xx error messages from these three providers now formatted `f"HTTP {status}: {body}"`, prefixed with `f"Retry-After: {value}\n"` when the response header is present — identical to `AnthropicProvider._response_error_message`. No signature or type changes.

- [ ] **Step 1: Write the failing tests.** Append to `sdks/python/tests/test_openai.py` (the file already imports `httpx`, `pytest`, `respx`, `ChatRequest`, `Message` and has a `provider` fixture; local imports match the style of `test_openai_401_raises_auth_error`):

```python
@respx.mock
@pytest.mark.asyncio
async def test_openai_502_then_200_is_retried(provider):
    from motosan_ai.retry import with_retry

    route = respx.post("https://mock.openai.com/v1/chat/completions").mock(
        side_effect=[
            httpx.Response(502, text="<html>bad gateway</html>"),
            httpx.Response(
                200,
                json={
                    "model": "gpt-4o",
                    "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1},
                },
            ),
        ]
    )

    resp = await with_retry(
        lambda: provider.chat(ChatRequest(messages=[Message.user("hi")])),
        max_retries=2,
        initial_backoff=0.001,
    )
    assert resp.content == "ok"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_openai_5xx_message_has_status_and_retry_after(provider):
    from motosan_ai.error import ProviderError

    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(503, text="overloaded", headers={"retry-after": "7"})
    )
    with pytest.raises(ProviderError) as exc_info:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    msg = str(exc_info.value)
    assert "HTTP 503: overloaded" in msg
    assert "Retry-After: 7" in msg
```

Append to `sdks/python/tests/test_minimax.py` (file imports `Response` from httpx, `ChatRequest`, `Message`, `MinimaxProvider`):

```python
@pytest.mark.asyncio
@respx.mock
async def test_minimax_502_then_200_is_retried():
    from motosan_ai.retry import with_retry

    route = respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        side_effect=[
            Response(502, text="<html>bad gateway</html>"),
            Response(
                200,
                json={
                    "model": "MiniMax-Text-01",
                    "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                    "usage": {"prompt_tokens": 1, "completion_tokens": 1},
                },
            ),
        ]
    )
    provider = MinimaxProvider("test-key")
    resp = await with_retry(
        lambda: provider.chat(ChatRequest(messages=[Message.user("hi")])),
        max_retries=2,
        initial_backoff=0.001,
    )
    assert resp.content == "ok"
    assert route.call_count == 2


@pytest.mark.asyncio
@respx.mock
async def test_minimax_5xx_message_has_status_and_retry_after():
    from motosan_ai.error import ProviderError

    respx.post("https://api.minimax.chat/v1/text/chatcompletion_v2").mock(
        return_value=Response(503, text="overloaded", headers={"retry-after": "7"})
    )
    provider = MinimaxProvider("test-key")
    with pytest.raises(ProviderError) as exc_info:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    msg = str(exc_info.value)
    assert "HTTP 503: overloaded" in msg
    assert "Retry-After: 7" in msg
```

Append to `sdks/python/tests/test_chatgpt_codex_http.py` (file already imports `httpx`, `ProviderError`, `ChatGptCodexProvider`, `ChatRequest`, `Message` and defines `_URL` and `_text_stream()`):

```python
@respx.mock
@pytest.mark.asyncio
async def test_chat_502_then_200_is_retried():
    from motosan_ai.retry import with_retry

    route = respx.post(_URL).mock(
        side_effect=[
            httpx.Response(502, text="<html>bad gateway</html>"),
            httpx.Response(
                200, text=_text_stream(), headers={"content-type": "text/event-stream"}
            ),
        ]
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    resp = await with_retry(
        lambda: p.chat(ChatRequest(messages=[Message.user("hi")])),
        max_retries=2,
        initial_backoff=0.001,
    )
    assert resp.content == "Hello world."
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_stream_5xx_message_has_status_and_retry_after():
    respx.post(_URL).mock(
        return_value=httpx.Response(503, text="overloaded", headers={"retry-after": "7"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(ProviderError) as exc_info:
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
    msg = str(exc_info.value)
    assert "HTTP 503: overloaded" in msg
    assert "Retry-After: 7" in msg
```

Append to `sdks/python/tests/test_retry.py` (file already imports `ProviderError`, `_is_retryable`, `_parse_retry_after` at top):

```python
class TestProviderErrorHttpMessageFormat:
    """Pins the message format providers must emit for retry classification."""

    def test_http_prefixed_502_is_retryable(self):
        assert _is_retryable(ProviderError("HTTP 502: <html>bad gateway</html>")) is True

    def test_retry_after_prefixed_message_is_retryable(self):
        assert _is_retryable(ProviderError("Retry-After: 3\nHTTP 503: overloaded")) is True

    def test_retry_after_prefix_is_parsed(self):
        assert _parse_retry_after("Retry-After: 3\nHTTP 503: overloaded") == 3.0
```

- [ ] **Step 2: Run the tests, verify the six provider tests FAIL.** From `sdks/python`:

```bash
uv run pytest tests/test_openai.py tests/test_minimax.py tests/test_chatgpt_codex_http.py tests/test_retry.py -v
```

Expected: exactly 6 failures, everything else passes (the 3 new `test_retry.py` tests pass already — they pin the unchanged classifier contract). Failure signatures: `test_openai_502_then_200_is_retried` and `test_chat_502_then_200_is_retried` fail with `motosan_ai.error.ProviderError: <html>bad gateway</html>` (error not classified retryable, raised on first attempt); `test_openai_5xx_message_has_status_and_retry_after` and `test_stream_5xx_message_has_status_and_retry_after` fail with `AssertionError` (message is bare `overloaded`); both minimax tests fail with `json.decoder.JSONDecodeError: Expecting value: line 1 column 1` (minimax parses the non-JSON error body as JSON before checking status).

- [ ] **Step 3: Implement.** Four hunks in `openai.py`, two in `minimax.py`, one in `chatgpt_codex.py`.

**3a — `sdks/python/motosan_ai/providers/openai.py`.** Current code (approximate lines 150-156):

```python
    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)
```

Replace with (adds a helper below the existing method, same format as `AnthropicProvider._response_error_message`):

```python
    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)

    @staticmethod
    def _response_error_message(status: int, headers: httpx.Headers, text: str) -> str:
        message = f"HTTP {status}: {text}"
        retry_after = headers.get("retry-after")
        if retry_after:
            message = f"Retry-After: {retry_after}\n{message}"
        return message
```

**3b — same file, `chat()`.** Current code (approximate lines 165-166):

```python
        if not resp.is_success:
            raise self._map_http_error(resp.status_code, resp.text)
```

Replace with:

```python
        if not resp.is_success:
            message = self._response_error_message(resp.status_code, resp.headers, resp.text)
            raise self._map_http_error(resp.status_code, message)
```

**3c — same file, `stream()`.** Current code (approximate lines 213-215, inside the `try:` after the send):

```python
            if not resp.is_success:
                error_body = await resp.aread()
                raise self._map_http_error(resp.status_code, error_body.decode())
```

Replace with:

```python
            if not resp.is_success:
                error_body = await resp.aread()
                message = self._response_error_message(
                    resp.status_code, resp.headers, error_body.decode()
                )
                raise self._map_http_error(resp.status_code, message)
```

**3d — `sdks/python/motosan_ai/providers/minimax.py`, `chat()`.** Current code (approximate lines 143-150 — note the unconditional `response.json()` BEFORE the status check is what crashes on HTML error bodies):

```python
        payload = response.json() if response.content else {}
        if response.status_code >= 400:
            message = (
                (payload.get("error") or {}).get("message")
                or response.text
                or "minimax request failed"
            )
            self._raise_for_status(response.status_code, message)
```

Replace with (`json` is already imported at the top of the file; the catch tuple matches `anthropic.py`):

```python
        if response.status_code >= 400:
            try:
                error_payload = response.json() if response.content else {}
                detail = str((error_payload.get("error") or {}).get("message") or "")
            except (json.JSONDecodeError, TypeError, AttributeError):
                detail = ""
            if not detail:
                detail = response.text or "minimax request failed"
            message = f"HTTP {response.status_code}: {detail}"
            retry_after = response.headers.get("retry-after")
            if retry_after:
                message = f"Retry-After: {retry_after}\n{message}"
            self._raise_for_status(response.status_code, message)

        payload = response.json() if response.content else {}
```

**3e — same file, `stream()`.** Current code (approximate lines 215-218, inside the `async with self._client.stream(...)` block):

```python
                if response.status_code >= 400:
                    text = await response.aread()
                    message = text.decode("utf-8", errors="ignore") or "minimax stream failed"
                    self._raise_for_status(response.status_code, message)
```

Replace with:

```python
                if response.status_code >= 400:
                    text = await response.aread()
                    detail = text.decode("utf-8", errors="ignore") or "minimax stream failed"
                    message = f"HTTP {response.status_code}: {detail}"
                    retry_after = response.headers.get("retry-after")
                    if retry_after:
                        message = f"Retry-After: {retry_after}\n{message}"
                    self._raise_for_status(response.status_code, message)
```

**3f — `sdks/python/motosan_ai/providers/chatgpt_codex.py`, `stream()`** (this is the only HTTP error path — `chat()` delegates via `collect_stream`). Current code (approximate lines 341-343, inside the `try:` after the send):

```python
            if not resp.is_success:
                error_body = await resp.aread()
                raise self._map_http_error(resp.status_code, error_body.decode())
```

Replace with:

```python
            if not resp.is_success:
                error_body = await resp.aread()
                message = f"HTTP {resp.status_code}: {error_body.decode()}"
                retry_after = resp.headers.get("retry-after")
                if retry_after:
                    message = f"Retry-After: {retry_after}\n{message}"
                raise self._map_http_error(resp.status_code, message)
```

- [ ] **Step 4: Run the tests + the package suite.** From `sdks/python`:

```bash
uv run pytest tests/test_openai.py tests/test_minimax.py tests/test_chatgpt_codex_http.py tests/test_retry.py -v
uv run pytest
```

Expected: all tests in the four files PASS (including the 6 previously failing); full suite passes (live integration tests under `tests/integration/` auto-skip without API-key env vars).

- [ ] **Step 5: Format & lint.** From `sdks/python`:

```bash
uv run ruff format
uv run ruff check motosan_ai/
```

Expected: format makes no further changes (or reformats only the files you touched); check reports no errors.

- [ ] **Step 6: Commit.**

```bash
git add sdks/python/motosan_ai/providers/openai.py sdks/python/motosan_ai/providers/minimax.py sdks/python/motosan_ai/providers/chatgpt_codex.py sdks/python/tests/test_openai.py sdks/python/tests/test_minimax.py sdks/python/tests/test_chatgpt_codex_http.py sdks/python/tests/test_retry.py
git commit -m "fix(python): embed HTTP status and Retry-After in OpenAI/MiniMax/ChatGPT-Codex errors so 5xx retries work"
```


## W2 — Surface in-band errors (streams stop lying)

### Task 5: Surface Anthropic mid-stream SSE error frames as stream errors (Rust)

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` (the `poll_next` event-type match in `AnthropicStreamAdapter`, approximate lines 916-1116; the edit point is the `"message_stop"` arm + `_ => continue` catch-all at approximate lines 1108-1115)
- Test: `sdks/rust/tests/anthropic_stream.rs` (EXTEND — append a new test at the end of the file, after `orphan_thinking_delta_without_start_does_not_crash`, approximate line 779)

**Interfaces:** Consumes `MotosanError::Stream(String)` (defined in `sdks/rust/src/error.rs`, re-exported as `motosan_ai::MotosanError`; already in scope in `anthropic.rs` — used unqualified at approximate line 1119). No public API changes: the fix is entirely inside `AnthropicStreamAdapter::poll_next`, changing a silently-dropped frame into an `Err` stream item.

**Background:** Anthropic can deliver an `event: error` frame on an HTTP 200 SSE stream (payload `{"type":"error","error":{"type":"overloaded_error","message":"..."}}`). Today it falls into the `_ => continue` catch-all, the stream then ends, and `collect_stream` fabricates a clean truncated response. The proven pattern to copy lives in `sdks/rust/src/providers/chatgpt_codex.rs` (approximate lines 482-500), where `error`/`response.failed` frames become `Err(MotosanError::Stream(...))`.

- [ ] **Step 1: Write the failing test.** Append the following to the END of `sdks/rust/tests/anthropic_stream.rs` (it follows the file's existing mockito + `concat!` SSE fixture style — read the first test `anthropic_stream_emits_content_and_done_event` at the top of the file to confirm; no new imports are needed, the file already has `use motosan_ai::{ChatRequest, Message, StopReason, StreamEventType, Tool};` and `use tokio_stream::StreamExt;`):

```rust
#[tokio::test]
async fn anthropic_stream_surfaces_mid_stream_error_frame() {
    // Anthropic can send an `event: error` frame on an HTTP 200 SSE stream
    // (e.g. overloaded_error). It must surface as Err, not be silently
    // dropped (which would fabricate a clean truncated response).
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_1\",\"usage\":{\"input_tokens\":10,\"output_tokens\":1}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"partial\"}}\n\n",
        "event: error\n",
        "data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut ok_events = Vec::new();
    let mut stream_err = None;

    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => ok_events.push(ev),
            Err(e) => {
                stream_err = Some(e);
                break;
            }
        }
    }

    // The text delta emitted before the error frame must still be delivered.
    let text_events: Vec<_> = ok_events
        .iter()
        .filter(|e| e.event_type == StreamEventType::Text)
        .collect();
    assert_eq!(text_events.len(), 1);
    assert_eq!(text_events[0].content, "partial");

    // No fabricated done event: the stream ended in an error, not message_stop.
    assert!(ok_events.iter().all(|e| !e.done));

    // The error frame surfaces as MotosanError::Stream carrying type + message.
    let err = stream_err.expect("stream must yield Err for the error frame");
    match err {
        motosan_ai::MotosanError::Stream(msg) => {
            assert!(msg.contains("overloaded_error"), "got: {msg}");
            assert!(msg.contains("Overloaded"), "got: {msg}");
        }
        other => panic!("expected MotosanError::Stream, got {other:?}"),
    }

    mock.assert_async().await;
}
```

- [ ] **Step 2: Run the test, verify it FAILS.** From `sdks/rust`:

```bash
cargo test --all-features --test anthropic_stream anthropic_stream_surfaces_mid_stream_error_frame
```

Expected failure: the error frame is currently swallowed by the `_ => continue` catch-all, so the stream ends without ever yielding `Err`, and the test panics with:

```
thread 'anthropic_stream_surfaces_mid_stream_error_frame' panicked ... 'stream must yield Err for the error frame'
```

(If it instead fails to compile, fix the test code before proceeding — the implementation must not exist yet.)

- [ ] **Step 3: Implement.** In `sdks/rust/src/providers/anthropic.rs`, inside `impl Stream for AnthropicStreamAdapter` / `poll_next`, add an `"error"` arm between the `"message_stop"` arm and the catch-all.

Current code (approximate lines 1108-1116):

```rust
                        "message_stop" => {
                            let done = match self.current_stop_reason.take() {
                                Some(reason) => StreamEvent::done_with_stop_reason(reason),
                                None => StreamEvent::done(),
                            };
                            return Poll::Ready(Some(Ok(done)));
                        }
                        _ => continue,
                    }
```

Replace with:

```rust
                        "message_stop" => {
                            let done = match self.current_stop_reason.take() {
                                Some(reason) => StreamEvent::done_with_stop_reason(reason),
                                None => StreamEvent::done(),
                            };
                            return Poll::Ready(Some(Ok(done)));
                        }
                        "error" => {
                            // Anthropic can deliver an error frame mid-stream on an
                            // HTTP 200 SSE connection (e.g. overloaded_error).
                            // Surface it as a stream error instead of silently
                            // dropping it — otherwise the stream just ends and
                            // collect_stream fabricates a clean truncated response.
                            let err = payload.get("error");
                            let err_type = err
                                .and_then(|e| e.get("type"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown_error");
                            let message = err
                                .and_then(|e| e.get("message"))
                                .and_then(Value::as_str)
                                .unwrap_or("unknown stream error");
                            return Poll::Ready(Some(Err(MotosanError::Stream(format!(
                                "anthropic stream error: {err_type}: {message}"
                            )))));
                        }
                        _ => continue,
                    }
```

Notes: `Value` is `serde_json::Value`, already imported in this file; `MotosanError` is already in scope (the `Poll::Ready(Some(Err(e)))` arm a few lines below uses it). Inline format args (`{err_type}`) match the existing style in this file (see `with_auth_hint`, approximate lines 116-122).

- [ ] **Step 4: Run the test + the touched package test suite.** From `sdks/rust`:

```bash
cargo test --all-features --test anthropic_stream anthropic_stream_surfaces_mid_stream_error_frame
cargo test --all-features
```

Expected: the new test passes (`test anthropic_stream_surfaces_mid_stream_error_frame ... ok`) and the full suite is green. In particular `anthropic_stream_ignores_unknown_and_malformed_events` must still pass — it exercises unknown event types, not `error` frames, so it is unaffected.

- [ ] **Step 5: Format & lint.** From `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diff complaints, zero clippy warnings.

- [ ] **Step 6: Commit.**

```bash
git add sdks/rust/src/providers/anthropic.rs sdks/rust/tests/anthropic_stream.rs
git commit -m "fix(anthropic): surface mid-stream SSE error frames as stream errors"
```

### Task 6: Surface claude_code error-subtype terminal events instead of silently dropping them

**Files:**
- Modify: `sdks/rust/src/providers/claude_code/stream_json.rs` (Result variant ~lines 20-35; Result match arm ~lines 124-152; tests mod starts ~line 157)
- Modify: `sdks/rust/src/providers/claude_code/mod.rs` (drive_lines Error arm ~lines 593-597 — LEFT UNCHANGED, see 3c; two existing tests ~lines 709-781 — LEFT UNCHANGED, see 3d)
- Modify: `sdks/rust/src/providers/claude_code/spawn.rs` (`parse_agent_json` ~lines 449-459; tests mod ends ~line 1007 — last test `common_args_full_loadout_order_is_stable` ends line 1006, module closing `}` on line 1007)
- Test: extend the existing `#[cfg(test)] mod tests` blocks inside those same three files (no separate test file exists for this provider)

**Interfaces:** Reuses the existing `MotosanError::ProviderError(String)` variant from `sdks/rust/src/error.rs` — the same variant the current claude_code error paths already yield (`mod.rs` drive_lines Error arm, `spawn.rs` parse_agent_json parse-failure). No new variant is introduced. Internal signatures unchanged: `pub fn parse_ndjson_line(&str) -> Option<NdjsonAction>`, `fn parse_agent_json(&str) -> Result<(String, Usage, Option<String>), MotosanError>`. No public API changes.

**Context:** The claude CLI's terminal `result` NDJSON line omits the `result` field when subtype is `error_max_turns` / `error_during_execution`. Because `ClaudeStreamEvent::Result` declares `result: String` as required, serde fails, `parse_ndjson_line` returns `None`, and the stream ends with no done event and no error. The blocking agent-mode path (`parse_agent_json`) ignores `is_error`/`subtype` and fabricates an empty-content success.

**Variant decision (the boundary this task must respect):** Every claude_code error terminal surfaces as `MotosanError::ProviderError` — the variant the existing `is_error: true` + non-empty `result` path already yields. This is an M1 minimal-behavior change: the already-working paths keep their exact variant, and the two previously-vanishing cases (a) the serde-defaulted missing `result` line that used to be dropped, and (b) an `error_*` subtype with no `is_error` flag that used to fall through to a normal `done` — newly *reach* that same `ProviderError`. Do NOT switch any of these to `MotosanError::Stream`; that would change the variant of a path that already errored.

- [ ] **Step 1: Write the failing tests**

In `sdks/rust/src/providers/claude_code/stream_json.rs`, append inside `mod tests` directly after the test `result_event_with_is_error_surfaces_error_action` (~line 209):

```rust
    #[test]
    fn result_error_subtype_without_result_field_maps_to_error() {
        // claude CLI omits the `result` field on error_max_turns /
        // error_during_execution terminals. The line must still parse
        // (not be silently dropped) and surface as an error.
        let line = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":42,"num_turns":10,"session_id":"sess_1"}"#;
        match parse_ndjson_line(line).expect("must parse despite missing result field") {
            NdjsonAction::Error(msg) => {
                assert!(msg.contains("error_max_turns"), "msg={msg}");
                assert!(msg.contains("claude_code terminal error"), "msg={msg}");
            }
            _ => panic!("expected Error action"),
        }
    }

    #[test]
    fn result_error_subtype_without_is_error_flag_maps_to_error() {
        // Defensive: an error_* subtype is terminal-error even when the
        // is_error flag is absent.
        let line = r#"{"type":"result","subtype":"error_during_execution"}"#;
        match parse_ndjson_line(line).expect("must parse") {
            NdjsonAction::Error(msg) => {
                assert!(msg.contains("error_during_execution"), "msg={msg}");
            }
            _ => panic!("expected Error action"),
        }
    }
```

In `sdks/rust/src/providers/claude_code/mod.rs`, append inside `mod tests` directly after the test `stream_surfaces_provider_error_as_err_item` (~line 732):

```rust
    #[tokio::test]
    async fn stream_error_subtype_terminal_yields_provider_error() {
        use std::io::Cursor;
        use tokio::io::BufReader;
        use tokio_stream::StreamExt;

        // Real claude CLI max-turns terminal: subtype error_max_turns, NO
        // `result` field. Must surface as an Err item, not a silent EOF with
        // no done event. It reuses the SAME `ProviderError` variant the
        // existing error terminals already yield — this line only newly
        // *reaches* that error; it does not change the variant.
        let raw = b"{\"type\":\"result\",\"subtype\":\"error_max_turns\",\"is_error\":true,\"duration_ms\":5,\"num_turns\":10}\n";
        let mut s = super::drive_lines(
            None::<tokio::process::Child>,
            BufReader::new(Cursor::new(&raw[..])),
            None,
        );
        match s.next().await {
            Some(Err(crate::error::MotosanError::ProviderError(msg))) => {
                assert!(msg.contains("error_max_turns"), "msg={msg}");
            }
            other => panic!("expected ProviderError mentioning error_max_turns, got {other:?}"),
        }
    }
```

In `sdks/rust/src/providers/claude_code/spawn.rs`, append inside `mod tests` directly after the last test `common_args_full_loadout_order_is_stable` (which ends at line 1006), immediately before the module's closing `}` on line 1007:

```rust
    #[test]
    fn agent_json_error_subtype_without_result_is_err() {
        // Agent-mode (--output-format json) terminal error: `result` omitted.
        // Must be an Err, not a fabricated empty-content success. Uses the same
        // ProviderError variant parse_agent_json already returns for a JSON
        // parse failure.
        let raw = r#"{"type":"result","subtype":"error_max_turns","is_error":true,"duration_ms":42,"num_turns":10,"session_id":"sess_1","usage":{"input_tokens":10,"output_tokens":2}}"#;
        match parse_agent_json(raw) {
            Err(MotosanError::ProviderError(msg)) => {
                assert!(msg.contains("error_max_turns"), "msg={msg}");
            }
            Err(other) => panic!("expected MotosanError::ProviderError, got {other:?}"),
            Ok(_) => panic!("error terminal must not parse as success"),
        }
    }

    #[test]
    fn agent_json_is_error_with_result_surfaces_message() {
        let raw = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"result":"boom"}"#;
        match parse_agent_json(raw) {
            Err(MotosanError::ProviderError(msg)) => {
                assert!(
                    msg.contains("error_during_execution") && msg.contains("boom"),
                    "msg={msg}"
                );
            }
            Err(other) => panic!("expected MotosanError::ProviderError, got {other:?}"),
            Ok(_) => panic!("error terminal must not parse as success"),
        }
    }
```

- [ ] **Step 2: Run the tests, verify they FAIL** — from `sdks/rust`:

```bash
cargo test --all-features claude_code
```

Expected: exactly 5 new failures —
- `result_error_subtype_without_result_field_maps_to_error` and `result_error_subtype_without_is_error_flag_maps_to_error` panic with `must parse` (serde rejects the line because `result` is required, so `parse_ndjson_line` returns `None`);
- `stream_error_subtype_terminal_yields_provider_error` panics with `expected ProviderError mentioning error_max_turns, got None` (the unparseable line is skipped and the stream hits EOF);
- both `agent_json_*` tests panic with `error terminal must not parse as success` (baseline `parse_agent_json` fabricates an empty-content `Ok`).

- [ ] **Step 3: Implement**

**3a — `sdks/rust/src/providers/claude_code/stream_json.rs`.** Current code (approximate lines 20-26):

```rust
    #[serde(rename = "result")]
    Result {
        // The `result` field on a terminal `result` event duplicates what
        // we already yielded as Text from the preceding `assistant` event.
        // Kept parsed so the variant matches the wire shape, and used as the
        // provider error message when `is_error` is true.
        result: String,
```

Replace with:

```rust
    #[serde(rename = "result")]
    Result {
        // The `result` field on a terminal `result` event duplicates what
        // we already yielded as Text from the preceding `assistant` event.
        // Error-subtype terminals (error_max_turns / error_during_execution)
        // omit it entirely, so it must default rather than fail the whole
        // line's deserialization. Used in the error message when the
        // terminal is an error.
        #[serde(default)]
        result: String,
```

**3b — same file.** Current code (approximate lines 124-137):

```rust
        ClaudeStreamEvent::Result {
            result,
            usage,
            session_id,
            is_error,
            subtype,
        } => {
            if is_error {
                let msg = if result.is_empty() {
                    subtype.unwrap_or_else(|| "claude CLI provider error".to_string())
                } else {
                    result
                };
                return Some(NdjsonAction::Error(msg));
            }
```

Replace with:

```rust
        ClaudeStreamEvent::Result {
            result,
            usage,
            session_id,
            is_error,
            subtype,
        } => {
            // Error terminals: `is_error: true`, or an `error_*` subtype
            // (error_max_turns / error_during_execution). The CLI omits
            // the `result` field on those, so never rely on it being set.
            let subtype_is_error = subtype.as_deref().is_some_and(|s| s.starts_with("error"));
            if is_error || subtype_is_error {
                let subtype = subtype.as_deref().unwrap_or("unknown");
                let detail = if result.is_empty() {
                    "no result message"
                } else {
                    result.as_str()
                };
                return Some(NdjsonAction::Error(format!(
                    "claude_code terminal error ({subtype}): {detail}"
                )));
            }
```

This still routes through `NdjsonAction::Error`, which `drive_lines` (3c) yields as `MotosanError::ProviderError` — the same variant the pre-existing `is_error: true` path already produced. The message is enriched uniformly (all error terminals get `claude_code terminal error (<subtype>): <detail>`), but the delivered error *variant* is unchanged.

**3c — `sdks/rust/src/providers/claude_code/mod.rs`, `drive_lines` Error arm — LEAVE UNCHANGED.** Per the variant decision above, this arm keeps yielding `MotosanError::ProviderError`. Do NOT edit it. It reads (approximate lines 593-597):

```rust
                    stream_json::NdjsonAction::Error(msg) => {
                        reap_child(&mut child, true).await;
                        yield Err(MotosanError::ProviderError(msg));
                        break;
                    }
```

Both the already-working `is_error: true` + non-empty `result` path and the newly-surfaced cases flow through this single arm, so both surface as `MotosanError::ProviderError`. Switching this to `MotosanError::Stream` would change the variant of a path that already errored — that is the M1 boundary violation this task exists to avoid.

**3d — same file, the two existing tests — LEAVE UNCHANGED.** Because 3c is untouched, `stream_surfaces_provider_error_as_err_item` (~lines 725-731) and `terminal_error_reaps_child_before_yield` (~lines 771-774) keep asserting `MotosanError::ProviderError(_)` and stay green. Do NOT rewrite their assertions.

**3e — `sdks/rust/src/providers/claude_code/spawn.rs`.** Current code (approximate lines 450-459):

```rust
fn parse_agent_json(raw: &str) -> Result<(String, Usage, Option<String>), MotosanError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        MotosanError::ProviderError(format!("failed to parse claude JSON output: {e}"))
    })?;

    let text = v
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
```

Replace with:

```rust
fn parse_agent_json(raw: &str) -> Result<(String, Usage, Option<String>), MotosanError> {
    let v: serde_json::Value = serde_json::from_str(raw).map_err(|e| {
        MotosanError::ProviderError(format!("failed to parse claude JSON output: {e}"))
    })?;

    // Error terminals: `is_error: true`, or an `error_*` subtype
    // (error_max_turns / error_during_execution). The CLI omits the
    // `result` field on those — never fabricate an empty success. Reuse the
    // ProviderError variant this function already returns for a parse failure.
    let is_error = v.get("is_error").and_then(|b| b.as_bool()).unwrap_or(false);
    let subtype = v.get("subtype").and_then(|s| s.as_str());
    if is_error || subtype.is_some_and(|s| s.starts_with("error")) {
        let subtype = subtype.unwrap_or("unknown");
        let detail = v
            .get("result")
            .and_then(|r| r.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or("no result message");
        return Err(MotosanError::ProviderError(format!(
            "claude_code terminal error ({subtype}): {detail}"
        )));
    }

    let text = v
        .get("result")
        .and_then(|r| r.as_str())
        .unwrap_or("")
        .to_string();
```

- [ ] **Step 4: Run the tests + the touched package suite** — from `sdks/rust`:

```bash
cargo test --all-features claude_code
cargo test --all-features
```

Expected: all claude_code tests PASS, specifically —
- the 5 new tests from Step 1 now pass;
- the 2 existing mod.rs tests (`stream_surfaces_provider_error_as_err_item`, `terminal_error_reaps_child_before_yield`) stay green *unchanged*, because 3c preserves the `MotosanError::ProviderError` variant;
- the pre-existing `result_event_with_is_error_surfaces_error_action` still passes because the enriched message `claude_code terminal error (success): bad model` still contains `bad model`;

then the full crate suite passes with 0 failed (`#[ignore]`d live tests stay ignored).

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diffs beyond formatting, clippy clean.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/claude_code/stream_json.rs sdks/rust/src/providers/claude_code/mod.rs sdks/rust/src/providers/claude_code/spawn.rs
git commit -m "fix(claude-code): surface error-subtype terminal events instead of dropping them"
```

### Task 7: Surface CLI child crash / premature EOF as stream errors in all three Rust CLI backends

> **Ordering:** Execute AFTER the claude_code terminal-event task — both edit `claude_code/mod.rs` (different regions).

**Execute after the claude_code terminal-event task to avoid merge friction** (both tasks edit `sdks/rust/src/providers/claude_code/mod.rs`, in different regions).

**Files:**
- Modify: `sdks/rust/src/providers/claude_code/mod.rs` (read-loop arms ~560-564; helper inserted after `reap_child` ~625; test inserted ~732)
- Modify: `sdks/rust/src/providers/codex_cli/mod.rs` (read-loop arms ~542-546; helper inserted after `reap_child` ~606; test inserted ~723)
- Modify: `sdks/rust/src/providers/gemini_cli/mod.rs` (read-loop arms ~341-345; helper inserted after `reap_child` ~404; test inserted ~517)
- Test: the in-file `#[cfg(test)] mod tests` of each of the three files above. No separate test file covers the CLI stream drivers; existing tests there call `drive_lines` directly and (for child-process behavior) spawn `sh -c` children with `tokio::process::Command` — see `terminal_error_reaps_child_before_yield` in `claude_code/mod.rs` (~734). All line refs are approximate.

**Interfaces:** Consumes the existing `MotosanError::Stream(String)` variant (`sdks/rust/src/error.rs` ~line 18) and each backend's `pub(crate) fn drive_lines<R>(child: Option<tokio::process::Child>, reader: R, read_timeout: Option<Duration>) -> BoxStream`. Produces, per file, a private `const CLI_LABEL: &str` and a private `async fn abnormal_exit_error(child: &mut Option<tokio::process::Child>, read_err: Option<std::io::Error>) -> MotosanError`. No public API changes.

**Bug being fixed:** in all three backends the `drive_lines` read loop has `Ok(None) => break` / `Err(_) => break`, so a crashed/killed CLI child (stdout closes with no terminal event) looks like a clean end-of-stream and downstream collectors fabricate success. Every terminal action (result/done event, error event, read timeout) already `break`s out of the loop before EOF can be read, so reaching `Ok(None)`/`Err(_)` always means no terminal event was seen — no extra state flag is needed. Do NOT touch the read-timeout arm or `reap_child`: cancellation (`kill_on_drop(true)` + explicit reap) is already correct.

- [ ] **Step 1: Write the failing tests** — one per backend, inside each file's existing `#[cfg(test)] mod tests`. Each test below is complete; paste it verbatim at the stated location. The three tests are identical except for two backend-specific spots: the second comment line and the fake-CLI event line (each backend's wire format differs). No other edits.

**(1a) `sdks/rust/src/providers/claude_code/mod.rs`** — insert after the closing `}` of `stream_surfaces_provider_error_as_err_item` (~732) and before the `#[cfg(unix)]` attribute of `terminal_error_reaps_child_before_yield` (~734):

```rust
    #[cfg(unix)]
    #[tokio::test]
    async fn premature_child_exit_surfaces_status_and_stderr() {
        use tokio::io::BufReader;
        use tokio::process::Command;
        use tokio_stream::StreamExt;

        // Fake CLI: one valid text event, "boom" on stderr, then exit 1 —
        // no terminal result event ever arrives.
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(r#"printf '%s\n' '{"type":"text","text":"hello"}'; echo boom >&2; exit 1"#)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn fake CLI");
        let stdout = child.stdout.take().expect("fake CLI stdout");

        let mut s = super::drive_lines(
            Some(child),
            BufReader::new(stdout),
            Some(std::time::Duration::from_secs(10)),
        );

        // The valid event still comes through first.
        match s.next().await {
            Some(Ok(event)) => {
                assert_eq!(event.content, "hello");
                assert!(!event.done);
            }
            other => panic!("expected the text event first, got {other:?}"),
        }

        // Then the crash surfaces as a Stream error with status + stderr.
        match s.next().await {
            Some(Err(crate::error::MotosanError::Stream(msg))) => {
                assert!(msg.contains("exited unexpectedly"), "got: {msg}");
                assert!(msg.contains("status 1"), "got: {msg}");
                assert!(msg.contains("boom"), "got: {msg}");
            }
            other => panic!("expected MotosanError::Stream, got {other:?}"),
        }

        assert!(s.next().await.is_none(), "stream must end after the error");
    }
```

**(1b) `sdks/rust/src/providers/codex_cli/mod.rs`** — insert after the closing `}` of `stream_surfaces_provider_error_as_err_item` (~723) and before `fn user_request` (~725). Codex emits `item.completed` / `agent_message`; the fake-CLI line and second comment differ from the claude_code test, and the longer event line makes rustfmt wrap the `.arg(...)` call:

```rust
    #[cfg(unix)]
    #[tokio::test]
    async fn premature_child_exit_surfaces_status_and_stderr() {
        use tokio::io::BufReader;
        use tokio::process::Command;
        use tokio_stream::StreamExt;

        // Fake CLI: one valid text event, "boom" on stderr, then exit 1 —
        // no terminal turn.completed event ever arrives.
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(
                r#"printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"hello"}}'; echo boom >&2; exit 1"#,
            )
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn fake CLI");
        let stdout = child.stdout.take().expect("fake CLI stdout");

        let mut s = super::drive_lines(
            Some(child),
            BufReader::new(stdout),
            Some(std::time::Duration::from_secs(10)),
        );

        // The valid event still comes through first.
        match s.next().await {
            Some(Ok(event)) => {
                assert_eq!(event.content, "hello");
                assert!(!event.done);
            }
            other => panic!("expected the text event first, got {other:?}"),
        }

        // Then the crash surfaces as a Stream error with status + stderr.
        match s.next().await {
            Some(Err(crate::error::MotosanError::Stream(msg))) => {
                assert!(msg.contains("exited unexpectedly"), "got: {msg}");
                assert!(msg.contains("status 1"), "got: {msg}");
                assert!(msg.contains("boom"), "got: {msg}");
            }
            other => panic!("expected MotosanError::Stream, got {other:?}"),
        }

        assert!(s.next().await.is_none(), "stream must end after the error");
    }
```

**(1c) `sdks/rust/src/providers/gemini_cli/mod.rs`** — insert after the closing `}` of `stream_surfaces_provider_error_as_err_item` (~517) and before the `#[test]` attribute of `env_builder_threads_and_debug_redacts` (~519). Gemini emits streamed `message` / `delta` events; the fake-CLI line differs and (being longer) makes rustfmt wrap the `.arg(...)` call, but the second comment stays `// no terminal result event ever arrives.`:

```rust
    #[cfg(unix)]
    #[tokio::test]
    async fn premature_child_exit_surfaces_status_and_stderr() {
        use tokio::io::BufReader;
        use tokio::process::Command;
        use tokio_stream::StreamExt;

        // Fake CLI: one valid text event, "boom" on stderr, then exit 1 —
        // no terminal result event ever arrives.
        let mut child = Command::new("sh")
            .arg("-c")
            .arg(
                r#"printf '%s\n' '{"type":"message","role":"assistant","content":"hello","delta":true}'; echo boom >&2; exit 1"#,
            )
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .expect("spawn fake CLI");
        let stdout = child.stdout.take().expect("fake CLI stdout");

        let mut s = super::drive_lines(
            Some(child),
            BufReader::new(stdout),
            Some(std::time::Duration::from_secs(10)),
        );

        // The valid event still comes through first.
        match s.next().await {
            Some(Ok(event)) => {
                assert_eq!(event.content, "hello");
                assert!(!event.done);
            }
            other => panic!("expected the text event first, got {other:?}"),
        }

        // Then the crash surfaces as a Stream error with status + stderr.
        match s.next().await {
            Some(Err(crate::error::MotosanError::Stream(msg))) => {
                assert!(msg.contains("exited unexpectedly"), "got: {msg}");
                assert!(msg.contains("status 1"), "got: {msg}");
                assert!(msg.contains("boom"), "got: {msg}");
            }
            other => panic!("expected MotosanError::Stream, got {other:?}"),
        }

        assert!(s.next().await.is_none(), "stream must end after the error");
    }
```

- [ ] **Step 2: Run the tests, verify they FAIL** — from `sdks/rust`:

```bash
cargo test --all-features premature_child_exit_surfaces_status_and_stderr
```

Expected: everything compiles (the tests reference nothing new), then all three tests fail at the second `s.next().await` because the buggy code ends the stream cleanly after the text event:

```
thread 'providers::claude_code::tests::premature_child_exit_surfaces_status_and_stderr' panicked at ...:
expected MotosanError::Stream, got None
...
test result: FAILED. 0 passed; 3 failed; ...
```

- [ ] **Step 3: Implement** — two edits per file; the inserted code is byte-identical in all three files except one `const` line.

**(3a) Replace the silent-break arms in `drive_lines`.**

Current code (byte-identical in all three files; approximate lines: claude_code 560-564, codex_cli 542-546, gemini_cli 341-345):

```rust
            let line = match next {
                Ok(Some(line)) => line.trim().to_string(),
                Ok(None) => break,
                Err(_) => break,
            };
```

Replace with (in each of the three files):

```rust
            let line = match next {
                Ok(Some(line)) => line.trim().to_string(),
                // EOF or a stdout read error before any terminal event: the
                // child died mid-stream. Every terminal action (result/done,
                // error, read timeout) breaks out of this loop, so reaching
                // either arm always means no terminal event was seen —
                // surface the child's exit status + stderr instead of letting
                // collectors mistake the truncated stream for success.
                Ok(None) => {
                    yield Err(abnormal_exit_error(&mut child, None).await);
                    break;
                }
                Err(e) => {
                    yield Err(abnormal_exit_error(&mut child, Some(e)).await);
                    break;
                }
            };
```

Leave the surrounding code untouched: the `Some(dur) => ... StreamReadTimeout` timeout arm above and the `reap_child(&mut child, false).await;` at the loop tail stay exactly as they are (the tail reap is a no-op on the new error paths because `abnormal_exit_error` takes the child).

**(3b) Insert the label const + helper immediately after the closing `}` of `async fn reap_child(...)`:**
- `claude_code/mod.rs`: ~line 625, before `impl Default for ClaudeCodeProvider`
- `codex_cli/mod.rs`: ~line 606, before `impl Default for CodexCliProvider`
- `gemini_cli/mod.rs`: ~line 404, before the doc comment `/// Merge the system prompt onto the user prompt for stdin delivery.`

The block below is for `claude_code/mod.rs`. In the other two files it is byte-identical except the const line, which becomes:
- `codex_cli/mod.rs`: `const CLI_LABEL: &str = "codex CLI";`
- `gemini_cli/mod.rs`: `const CLI_LABEL: &str = "gemini CLI";`

```rust
/// Human-readable backend label used in abnormal-exit diagnostics.
const CLI_LABEL: &str = "claude CLI";

/// Build the error for an abnormal end-of-stream (stdout read error, or EOF
/// before any terminal event): reap the child (bounded), then fold its exit
/// status and buffered stderr into a diagnostic [`MotosanError::Stream`].
///
/// A crashed or killed CLI child closes stdout without ever emitting a
/// terminal event; without this the stream would end cleanly and
/// downstream collectors would fabricate a successful response.
async fn abnormal_exit_error(
    child: &mut Option<tokio::process::Child>,
    read_err: Option<std::io::Error>,
) -> MotosanError {
    let mut status = String::from("unknown");
    let mut stderr_excerpt = String::new();

    if let Some(mut c) = child.take() {
        // Take stderr BEFORE waiting so we can drain what the child buffered.
        let stderr_pipe = c.stderr.take();
        match tokio::time::timeout(Duration::from_secs(5), c.wait()).await {
            Ok(Ok(exit)) => {
                status = exit
                    .code()
                    .map(|code| code.to_string())
                    .unwrap_or_else(|| exit.to_string());
            }
            _ => {
                // Wait failed or timed out — SIGKILL so the pipe closes and
                // the stderr read below cannot block forever.
                let _ = c.start_kill();
                let _ = c.wait().await;
            }
        }
        if let Some(mut pipe) = stderr_pipe {
            use tokio::io::AsyncReadExt;
            let mut buf = Vec::new();
            let _ = tokio::time::timeout(Duration::from_secs(5), pipe.read_to_end(&mut buf)).await;
            stderr_excerpt = String::from_utf8_lossy(&buf)
                .trim()
                .chars()
                .take(2000)
                .collect();
        }
    }

    let detail = if !stderr_excerpt.is_empty() {
        stderr_excerpt
    } else if let Some(e) = read_err {
        format!("stdout read error: {e}")
    } else {
        "stream ended before a terminal event".to_string()
    };

    MotosanError::Stream(format!(
        "{CLI_LABEL} exited unexpectedly (status {status}): {detail}"
    ))
}
```

Note: `stream()` in each backend pipes stderr but only takes stdin/stdout from the `Child`, so `c.stderr.take()` gets the live pipe here; when `drive_lines` is called with `None` child (unit tests) the helper falls back to `status unknown` + `stream ended before a terminal event`.

- [ ] **Step 4: Run the new tests + the full suite** — from `sdks/rust`:

```bash
cargo test --all-features premature_child_exit_surfaces_status_and_stderr
```

Expected: `3 passed; 0 failed`, e.g. `test providers::claude_code::tests::premature_child_exit_surfaces_status_and_stderr ... ok` (and the codex_cli / gemini_cli twins). Then:

```bash
cargo test --all-features
```

Expected: `0 failed` across all suites (~537 passed, 18 ignored at plan-writing time; no pre-existing test asserts the old silent-EOF behavior).

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: `cargo fmt` makes no changes (the blocks above are rustfmt-canonical) and clippy finishes with no warnings.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/claude_code/mod.rs sdks/rust/src/providers/codex_cli/mod.rs sdks/rust/src/providers/gemini_cli/mod.rs
git commit -m "fix(cli): surface child crash and premature EOF as stream errors in CLI backends"
```

### Task 8: Surface Anthropic mid-stream SSE error frames as StreamError (Python)

**Files:**
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (SSE event dispatch inside `stream()`, approximate lines 419-421)
- Test: `sdks/python/tests/test_anthropic_stream_usage.py` (extend; it already imports `StreamError` and has the `_sse` helper and a precedent mid-stream-raise test at approximate lines 105-126)

**Interfaces:** Consumes `motosan_ai.error.StreamError` (defined in `sdks/python/motosan_ai/error.py`, a plain `MotosanError` subclass; already imported at the top of `anthropic.py`). No public API changes — `AnthropicProvider.stream(request) -> AsyncIterator[StreamEvent]` keeps its signature; it just now raises `StreamError` when the server sends an error frame instead of silently ending the stream.

**Context:** Anthropic can send `{"type": "error", "error": {"type": "overloaded_error", "message": "..."}}` as an SSE event on an HTTP 200 stream. Today the dispatch in `stream()` has no branch for `event_type == "error"`, so the frame is silently ignored and the truncated stream collects as success (including via the OAuth `chat()` path, which wraps `stream()` with `collect_stream`). Per the repo's F1 fallible-stream policy, mid-stream failures raise `StreamError` (see the existing `malformed SSE chunk` raise a few lines above the dispatch).

- [ ] **Step 0: Sync deps** — from `sdks/python`, run `uv sync --extra full --extra dev` (fresh worktrees lack `respx`/`pytest`, which are dev extras; this mirrors CI).

- [ ] **Step 1: Write the failing tests** — append to the end of `sdks/python/tests/test_anthropic_stream_usage.py` (no new imports needed; `respx`, `pytest`, `httpx`, `StreamError`, `ChatRequest`, `Message`, the `provider` fixture, and `_sse` all exist in this file):

```python
@respx.mock
@pytest.mark.asyncio
async def test_stream_raises_on_error_event(provider):
    # message_start + one good text delta, then a mid-stream error frame on HTTP 200
    sse = _sse(
        {"type": "message_start", "message": {"usage": {"input_tokens": 3, "output_tokens": 0}}},
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "par"}},
        {"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}},
    )
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(StreamError, match="anthropic stream error: overloaded_error: Overloaded"):
        async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(ev)
    assert any(e.content == "par" for e in seen)  # text before the error was still yielded


@respx.mock
@pytest.mark.asyncio
async def test_stream_error_event_with_missing_fields(provider):
    sse = _sse({"type": "error"})
    respx.post("https://mock.anthropic.com/v1/messages").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    with pytest.raises(StreamError, match="anthropic stream error: unknown_error"):
        async for _ in provider.stream(ChatRequest(messages=[Message.user("hi")])):
            pass
```

Note: two blank lines between the existing last test and the first new function (ruff E303/format rule).

- [ ] **Step 2: Run the tests, verify they FAIL** — from `sdks/python`:

```bash
uv run pytest tests/test_anthropic_stream_usage.py -v
```

Expected: the 5 pre-existing tests pass; both new tests fail with exactly:

```
Failed: DID NOT RAISE <class 'motosan_ai.error.StreamError'>
...
FAILED tests/test_anthropic_stream_usage.py::test_stream_raises_on_error_event
FAILED tests/test_anthropic_stream_usage.py::test_stream_error_event_with_missing_fields
2 failed, 5 passed
```

- [ ] **Step 3: Implement** — in `sdks/python/motosan_ai/providers/anthropic.py`, inside `stream()`'s `async for line in resp.aiter_lines():` loop, insert the error branch immediately after `event_type` is read and before the `message_start` branch.

Current code (approximate lines 419-421):

```python
                event_type = payload.get("type")

                if event_type == "message_start":
```

Replace with:

```python
                event_type = payload.get("type")

                if event_type == "error":
                    error = payload.get("error") or {}
                    error_type = error.get("type") or "unknown_error"
                    error_message = error.get("message") or ""
                    raise StreamError(f"anthropic stream error: {error_type}: {error_message}")

                if event_type == "message_start":
```

Keep the `raise StreamError(...)` on a single line — it fits the project's 100-char limit and is the ruff-format canonical form. No other changes: the surrounding `try` already has `except StreamError: raise` (pass-through) and a `finally: await resp.aclose()`, and the OAuth `chat()` path (which calls `collect_stream(self.stream(request))`) inherits the fix automatically.

- [ ] **Step 4: Run the tests + the Python unit suite** — from `sdks/python`:

```bash
uv run pytest tests/test_anthropic_stream_usage.py -v
uv run pytest tests/ -q --ignore=tests/integration
```

Expected: first command prints `7 passed`; second command ends with all tests passing, 0 failures.

- [ ] **Step 5: Format & lint** — from `sdks/python`:

```bash
uv run ruff format motosan_ai/ tests/
uv run ruff check motosan_ai/
```

Expected: `ruff format` reports 0 files reformatted (or reformats only whitespace you introduced); `ruff check` prints `All checks passed!`.

- [ ] **Step 6: Commit**

```bash
git add sdks/python/motosan_ai/providers/anthropic.py sdks/python/tests/test_anthropic_stream_usage.py
git commit -m "fix(python): raise StreamError on Anthropic mid-stream SSE error frames"
```

### Task 9: Raise StreamError on Claude Code CLI error terminal results (Python)

> **Coordinate with the CLI premature-EOF task if executing concurrently; prefer sequential.** That task edits the EOF handling inside `stream()` (~lines 617-618 of the same file); this task edits the parse helpers (~lines 98-116 and 188-207). Different regions, same file.

**Context:** The `claude` CLI emits a terminal NDJSON line `{"type":"result","subtype":"error_max_turns","is_error":true}` (subtypes `error_max_turns`, `error_during_execution`, ...) when a turn fails or is truncated. The Python provider currently converts that into a clean `done=True` stream event (and a normal `ChatResponse` in agent-mode `chat()`), so agent loops consume the truncated turn as genuine. The Rust provider already surfaces this as an error (`sdks/rust/src/providers/claude_code/stream_json.rs`, `if is_error { ... }`). Fix Python to raise `StreamError` in both the `stream()` path and the agent-mode `chat()` path. Non-agent-mode `chat()` receives plain text (no JSON), so there is nothing to detect there.

**Files:**
- Modify: `sdks/python/motosan_ai/providers/claude_code.py` (import at approximate line 10; `_parse_agent_json` approximate lines 98-116; `_parse_ndjson_line` result branch approximate lines 188-207)
- Test: `sdks/python/tests/test_claude_code_runtime.py` (EXTEND — has the `_make_proc` fake-CLI fixture; 5 existing tests)
- Test: `sdks/python/tests/test_claude_code.py` (EXTEND — `TestParseAgentJson` approximate lines 85-118, `TestParseNdjsonLine` approximate lines 126-232)

**Interfaces:**
- Consumes: `motosan_ai.error.StreamError` (`sdks/python/motosan_ai/error.py` line 29: `class StreamError(MotosanError)`) — the established fallible-stream exception already raised by every HTTP provider.
- Produces: private module helper `_raise_on_error_result(event: dict) -> None` in `claude_code.py`. No public API changes: `ClaudeCodeClient.chat()`/`.stream()` signatures unchanged; behavior changes only where a failed turn was previously fabricated as a clean completion.

- [ ] **Step 1: Write the failing tests**

In `sdks/python/tests/test_claude_code_runtime.py`, add the import (isort order: `motosan_ai.error` before `motosan_ai.providers.claude_code`). Current imports (approximate lines 1-7):

```python
import os
from unittest.mock import AsyncMock, patch

import pytest

from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.types import ChatRequest, Message, Role, StopReason
```

Replace with:

```python
import os
from unittest.mock import AsyncMock, patch

import pytest

from motosan_ai.error import StreamError
from motosan_ai.providers.claude_code import ClaudeCodeClient
from motosan_ai.types import ChatRequest, Message, Role, StopReason
```

Append at the end of the same file (after `test_chat_no_timeout_skips_wait_for`, approximate line 99):

```python
@pytest.mark.asyncio
async def test_stream_error_result_raises_stream_error(monkeypatch):
    stdout = (
        b'{"type":"assistant","message":{"content":[{"type":"text","text":"partial"}]}}\n'
        b'{"type":"result","subtype":"error_max_turns","is_error":true}\n'
    )
    proc = _make_proc(stdout=stdout)
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    events = []
    with pytest.raises(StreamError, match="error_max_turns"):
        async for ev in ClaudeCodeClient().stream(
            ChatRequest(messages=[Message(role=Role.user, content="hi")])
        ):
            events.append(ev)
    # partial text already yielded is kept; no fabricated clean done event
    assert [e.content for e in events] == ["partial"]
    assert not any(e.done for e in events)


@pytest.mark.asyncio
async def test_chat_agent_mode_error_result_raises_stream_error(monkeypatch):
    stdout = (
        b'{"type":"result","subtype":"error_during_execution",'
        b'"is_error":true,"result":"boom"}\n'
    )
    proc = _make_proc(stdout=stdout)
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    client = ClaudeCodeClient().agent_mode(True)
    with pytest.raises(StreamError, match="error_during_execution"):
        await client.chat(ChatRequest(messages=[Message(role=Role.user, content="hi")]))
```

In `sdks/python/tests/test_claude_code.py`, add one test at the end of class `TestParseAgentJson` (after `test_session_id_none_when_absent`, approximate line 118; note the local `from motosan_ai.error import ...` style matches the existing `test_invalid_json` in the same class):

```python
    def test_error_result_raises_stream_error(self):
        from motosan_ai.error import StreamError

        raw = json.dumps({"type": "result", "subtype": "error_max_turns", "is_error": True})
        with pytest.raises(StreamError, match="error_max_turns"):
            _parse_agent_json(raw)
```

And add two tests in class `TestParseNdjsonLine` (after `test_result_event`, approximate line 194):

```python
    def test_result_is_error_raises_stream_error(self):
        from motosan_ai.error import StreamError

        line = '{"type":"result","subtype":"error_max_turns","is_error":true}'
        with pytest.raises(StreamError, match="error_max_turns"):
            _parse_ndjson_line(line)

    def test_result_error_subtype_without_is_error_flag_raises(self):
        from motosan_ai.error import StreamError

        line = '{"type":"result","subtype":"error_during_execution","result":"boom"}'
        with pytest.raises(StreamError, match="error_during_execution: boom"):
            _parse_ndjson_line(line)
```

- [ ] **Step 2: Run the tests, verify they FAIL**

```bash
cd sdks/python
uv run pytest tests/test_claude_code_runtime.py tests/test_claude_code.py -v
```

Expected: exactly 5 failures, all with the signature `Failed: DID NOT RAISE <class 'motosan_ai.error.StreamError'>` — `test_stream_error_result_raises_stream_error`, `test_chat_agent_mode_error_result_raises_stream_error`, `test_error_result_raises_stream_error`, `test_result_is_error_raises_stream_error`, `test_result_error_subtype_without_is_error_flag_raises`. All 56 pre-existing tests pass.

- [ ] **Step 3: Implement** — three edits in `sdks/python/motosan_ai/providers/claude_code.py`.

Edit A — import. Current code (approximate line 10):

```python
from motosan_ai.error import ProviderError
```

Replace with:

```python
from motosan_ai.error import ProviderError, StreamError
```

Edit B — add the helper and hook it into the agent-mode `chat()` path. Current code (approximate lines 98-105):

```python
def _parse_agent_json(raw: str) -> tuple[str, Usage, str | None]:
    """Parse JSON output from agent mode: ``result``, ``usage``, ``session_id``."""
    try:
        v = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ProviderError(f"failed to parse claude JSON output: {e}") from e

    text = v.get("result", "")
```

Replace with:

```python
def _raise_on_error_result(event: dict) -> None:
    """Raise ``StreamError`` when a terminal ``result`` payload reports an error.

    Claude Code marks failed/truncated turns with ``is_error: true`` and an
    ``error_*`` subtype (e.g. ``error_max_turns``, ``error_during_execution``).
    Emitting them as clean completions would let agent loops consume a
    truncated turn as a genuine one, so surface them as ``StreamError``.
    """
    subtype = event.get("subtype")
    has_error_subtype = isinstance(subtype, str) and subtype.startswith("error")
    if not (event.get("is_error") or has_error_subtype):
        return
    result_text = event.get("result")
    parts = [p for p in (subtype, result_text) if isinstance(p, str) and p]
    detail = ": ".join(parts) if parts else "unknown error"
    raise StreamError(f"claude CLI reported an error result: {detail}")


def _parse_agent_json(raw: str) -> tuple[str, Usage, str | None]:
    """Parse JSON output from agent mode: ``result``, ``usage``, ``session_id``.

    Raises :class:`~motosan_ai.error.StreamError` when the payload reports an
    error result (``is_error: true`` or an ``error_*`` subtype).
    """
    try:
        v = json.loads(raw)
    except json.JSONDecodeError as e:
        raise ProviderError(f"failed to parse claude JSON output: {e}") from e

    if isinstance(v, dict):
        _raise_on_error_result(v)

    text = v.get("result", "")
```

Edit C — hook it into the `stream()` path (`_parse_ndjson_line` is only called from `stream()`; the raise propagates out of the async generator and the existing `finally` block still kills the child process). Current code (approximate lines 188-190, now shifted ~+22 lines by Edit B):

```python
    if event_type == "result":
        events_out: list[StreamEvent] = []
        sid = event.get("session_id")
```

Replace with:

```python
    if event_type == "result":
        _raise_on_error_result(event)
        events_out: list[StreamEvent] = []
        sid = event.get("session_id")
```

- [ ] **Step 4: Run the tests + the touched suite, verify PASS**

```bash
cd sdks/python
uv run pytest tests/test_claude_code_runtime.py tests/test_claude_code.py tests/test_claude_code_flags.py tests/test_claude_code_stream_usage.py -v
uv run pytest
```

Expected: first command — `test_claude_code_runtime.py` 7 passed, `test_claude_code.py` 54 passed, flags and stream_usage files unchanged and passing, 0 failures. Second command — full package suite passes, exit code 0. (Existing fixtures use `is_error: false` or omit it, and `subtype: "success"` does not start with `"error"`, so no existing test trips the new raise.)

- [ ] **Step 5: Format & lint**

```bash
cd sdks/python
uv run ruff format
uv run ruff check motosan_ai/
```

Expected: format reports files unchanged (or reformats only the files you touched), check reports `All checks passed!`.

- [ ] **Step 6: Commit**

```bash
git add sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_claude_code_runtime.py sdks/python/tests/test_claude_code.py
git commit -m "fix(claude-code): raise StreamError on is_error terminal results in Python provider"
```

### Task 10: Raise StreamError on CLI child death / premature EOF in Python CLI providers

> **Ordering:** Execute AFTER "Raise StreamError on Claude Code CLI error terminal results (Python)" — both edit `providers/claude_code.py`, INCLUDING the same import lines. If that task already landed, the `StreamError` import may already exist: merge the import edit instead of duplicating it, and adapt surrounding line refs.

**Files:**
- Modify: `sdks/python/motosan_ai/providers/codex_cli.py` (error import ~line 11; `stream()` loop ~lines 414-449)
- Modify: `sdks/python/motosan_ai/providers/gemini_cli.py` (error import ~line 11; `stream()` loop ~lines 381-414)
- Modify: `sdks/python/motosan_ai/providers/claude_code.py` (error import ~line 10; `stream()` loop ~lines 604-635)
- Test: `sdks/python/tests/test_codex_cli_stream.py`, `sdks/python/tests/test_gemini_cli_stream.py`, `sdks/python/tests/test_claude_code_runtime.py` (extend all three — they already cover these stream loops)

**Interfaces:** Consumes `StreamError` from `sdks/python/motosan_ai/error.py` (~line 29, `class StreamError(MotosanError)` — already exists). No public API changes: `async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]` is unchanged on all three clients. New behavior only on the corrupted-data path: EOF on child stdout *before* the terminal (`done=True`) event now raises `StreamError("<codex|gemini|claude> CLI exited unexpectedly (returncode {rc}): {stderr excerpt}")` instead of ending the stream silently. The per-read `asyncio.wait_for` timeouts are healthy — do NOT change them.

**Bug:** all three CLI providers' `stream()` loops treat stdout EOF as a clean end (`if not raw: break`) without checking that a terminal event arrived and without inspecting returncode/stderr, so a child that crashes mid-turn yields a silently truncated stream. (EOF *after* the terminal event is currently unreachable — the loop `return`s on `done` — so the `saw_done` flag below is defensive; the raise is the behavioral fix.)

- [ ] **Step 1: Write the failing tests.** The repo sets `asyncio_mode = "auto"` but neighboring tests keep explicit `@pytest.mark.asyncio` — match that. In `sdks/python/tests/test_codex_cli_stream.py`: (1) change the import at ~line 9 from `from motosan_ai.error import ProviderError` to `from motosan_ai.error import ProviderError, StreamError`; (2) insert this class between `_FakeStdout` (ends ~line 185) and `_FakeProc` (~line 188), and add the line `self.stderr = _FakeStderr(self._stderr)` inside `_FakeProc.__init__` directly after `self.stdout = _FakeStdout(stdout)` (~line 194):

```python
class _FakeStderr:
    def __init__(self, data: bytes) -> None:
        self._data = data

    async def read(self) -> bytes:
        return self._data
```

(3) add this test directly above `test_stream_read_stall_raises` (~line 361):

```python
@pytest.mark.asyncio
async def test_stream_child_death_raises_stream_error(monkeypatch):
    # One valid event, then EOF with exit code 1: the child crashed mid-turn.
    jsonl = '{"type": "item.completed", "item": {"type": "agent_message", "text": "par"}}\n'
    _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=1, stderr="boom\n"))

    client = CodexCliClient(binary_path="codex")
    events = []
    with pytest.raises(StreamError, match="returncode 1") as excinfo:
        async for event in client.stream(ChatRequest(messages=[Message.user("hi")])):
            events.append(event)

    assert "boom" in str(excinfo.value)
    assert [e.content for e in events] == ["par"]
    assert not any(e.done for e in events)
```

In `sdks/python/tests/test_gemini_cli_stream.py`: repeat edits (1) and (2) verbatim — the `_FakeStdin`/`_FakeStdout`/`_FakeProc` helpers in the two files are line-for-line identical (`_FakeStdout` ends ~line 185, `_FakeProc` ~line 188, `self.stdout = _FakeStdout(stdout)` ~line 194) — then add this test directly above `test_stream_stall_raises` (~line 360):

```python
@pytest.mark.asyncio
async def test_stream_child_death_raises_stream_error(monkeypatch):
    # One valid event, then EOF with exit code 1: the child crashed mid-turn.
    jsonl = '{"type": "message", "role": "assistant", "content": "par", "delta": true}\n'
    _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=1, stderr="boom\n"))

    client = GeminiCliClient(binary_path="gemini")
    events = []
    with pytest.raises(StreamError, match="returncode 1") as excinfo:
        async for event in client.stream(ChatRequest(messages=[Message.user("hi")])):
            events.append(event)

    assert "boom" in str(excinfo.value)
    assert [e.content for e in events] == ["par"]
    assert not any(e.done for e in events)
```

In `sdks/python/tests/test_claude_code_runtime.py`: add `from motosan_ai.error import StreamError` on its own line after `import pytest` (~line 4, before the `motosan_ai.providers.claude_code` import), then add this test directly above `test_chat_no_timeout_skips_wait_for` (~line 87):

```python
@pytest.mark.asyncio
async def test_stream_child_death_raises_stream_error(monkeypatch):
    # One valid assistant event, then EOF with exit code 1 and stderr "boom".
    stdout = b'{"type":"assistant","message":{"content":[{"type":"text","text":"par"}]}}\n'
    proc = _make_proc(stdout=stdout)
    proc.returncode = 1
    proc.wait = AsyncMock(return_value=1)
    proc.stderr = AsyncMock()
    proc.stderr.read = AsyncMock(return_value=b"boom\n")
    monkeypatch.setattr(
        "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
        AsyncMock(return_value=proc),
    )
    events = []
    with pytest.raises(StreamError, match="returncode 1") as excinfo:
        async for ev in ClaudeCodeClient().stream(
            ChatRequest(messages=[Message(role=Role.user, content="hi")])
        ):
            events.append(ev)

    assert "boom" in str(excinfo.value)
    assert [e.content for e in events] == ["par"]
    assert not any(e.done for e in events)
```

- [ ] **Step 2: Run the tests, verify they FAIL**

```bash
cd sdks/python && uv run pytest \
  tests/test_codex_cli_stream.py::test_stream_child_death_raises_stream_error \
  tests/test_gemini_cli_stream.py::test_stream_child_death_raises_stream_error \
  tests/test_claude_code_runtime.py::test_stream_child_death_raises_stream_error -v
```

Expected: `3 failed`, each with `Failed: DID NOT RAISE <class 'motosan_ai.error.StreamError'>` (the streams currently end silently at EOF after yielding only the text event).

- [ ] **Step 3: Implement** — four small edits per provider file. (3a) In all three files change the error import (codex_cli.py ~line 11, gemini_cli.py ~line 11, claude_code.py ~line 10) from `from motosan_ai.error import ProviderError` to `from motosan_ai.error import ProviderError, StreamError`. (3b) In each `stream()` method add the flag init — Current code (codex_cli.py ~414, gemini_cli.py ~381, claude_code.py ~605):

```python
        saw_tool_call = False
        try:
```

Replace with:
```python
        saw_tool_call = False
        saw_done = False
        try:
```

(3c) `codex_cli.py` — Current code (approximate lines 435-436, inside the `while True:` read loop):

```python
                if not raw:
                    break
```

Replace with:
```python
                if not raw:
                    if saw_done:
                        break
                    # EOF before the terminal event: the child crashed or
                    # closed stdout early — raise instead of truncating.
                    returncode = proc.returncode
                    if returncode is None:
                        with contextlib.suppress(TimeoutError):
                            returncode = await asyncio.wait_for(proc.wait(), timeout=5.0)
                    stderr_excerpt = ""
                    if proc.stderr is not None:
                        with contextlib.suppress(TimeoutError, OSError):
                            stderr_bytes = await asyncio.wait_for(proc.stderr.read(), timeout=5.0)
                            stderr_excerpt = stderr_bytes.decode(errors="replace").strip()[-2048:]
                    raise StreamError(
                        f"codex CLI exited unexpectedly (returncode {returncode}): {stderr_excerpt}"
                    )
```

Then in the same file, track the terminal event — Current code (approximate lines 445-446):

```python
                    if event.done and saw_tool_call:
                        event.stop_reason = StopReason.tool_use
```

Replace with:
```python
                    if event.done:
                        saw_done = True
                    if event.done and saw_tool_call:
                        event.stop_reason = StopReason.tool_use
```

(3d) `gemini_cli.py` — Current code (approximate lines 402-403) is the identical two-line `if not raw:` / `break` pair quoted in (3c). Replace with:
```python
                if not raw:
                    if saw_done:
                        break
                    # EOF before the terminal event: the child crashed or
                    # closed stdout early — raise instead of truncating.
                    returncode = proc.returncode
                    if returncode is None:
                        with contextlib.suppress(TimeoutError):
                            returncode = await asyncio.wait_for(proc.wait(), timeout=5.0)
                    stderr_excerpt = ""
                    if proc.stderr is not None:
                        with contextlib.suppress(TimeoutError, OSError):
                            stderr_bytes = await asyncio.wait_for(proc.stderr.read(), timeout=5.0)
                            stderr_excerpt = stderr_bytes.decode(errors="replace").strip()[-2048:]
                    raise StreamError(
                        f"gemini CLI exited unexpectedly "
                        f"(returncode {returncode}): {stderr_excerpt}"
                    )
```

(3e) `claude_code.py` — Current code (approximate lines 617-618):

```python
                if not raw_line:
                    break
```

Replace with:
```python
                if not raw_line:
                    if saw_done:
                        break
                    # EOF before the terminal event: the child crashed or
                    # closed stdout early — raise instead of truncating.
                    returncode = proc.returncode
                    if returncode is None:
                        with contextlib.suppress(TimeoutError):
                            returncode = await asyncio.wait_for(proc.wait(), timeout=5.0)
                    stderr_excerpt = ""
                    if proc.stderr is not None:
                        with contextlib.suppress(TimeoutError, OSError):
                            stderr_bytes = await asyncio.wait_for(proc.stderr.read(), timeout=5.0)
                            stderr_excerpt = stderr_bytes.decode(errors="replace").strip()[-2048:]
                    raise StreamError(
                        f"claude CLI exited unexpectedly "
                        f"(returncode {returncode}): {stderr_excerpt}"
                    )
```

(3f) In `gemini_cli.py` (~lines 408-409) AND `claude_code.py` (~lines 629-630), the event loop has an identical done-branch — Current code (both files):

```python
                    if event.done:
                        event.stop_reason = (
```

Replace with (both files):
```python
                    if event.done:
                        saw_done = True
                        event.stop_reason = (
```

Notes: `contextlib` and `asyncio` are already imported in all three files. The codex raise is a single f-string line while gemini/claude split across two — that is exactly what `ruff format` (line length 100) produces, since "gemini"/"claude" are one character longer than "codex". Do not add a returncode check to the happy path: exit code is irrelevant once the terminal event arrived.

- [ ] **Step 4: Run the tests + the touched package test suite**

```bash
cd sdks/python && uv run pytest tests/test_codex_cli_stream.py tests/test_gemini_cli_stream.py tests/test_claude_code_runtime.py -v
cd sdks/python && uv run pytest tests/ -q
```

Expected: first command `66 passed` (63 pre-existing + 3 new; count may grow if main has moved). Second command: full suite passes — `tests/integration/` live tests are env-guarded and skip automatically.

- [ ] **Step 5: Format & lint**

```bash
cd sdks/python && uv run ruff format motosan_ai/providers/codex_cli.py motosan_ai/providers/gemini_cli.py motosan_ai/providers/claude_code.py tests/test_codex_cli_stream.py tests/test_gemini_cli_stream.py tests/test_claude_code_runtime.py
cd sdks/python && uv run ruff check motosan_ai/
```

Expected: `6 files left unchanged` (the code above is already formatter-clean) and `All checks passed!`. Do not run `ruff check tests/` — unrelated test files have pre-existing RUF059/E402 findings that are out of scope.

- [ ] **Step 6: Commit**

```bash
git add sdks/python/motosan_ai/providers/codex_cli.py sdks/python/motosan_ai/providers/gemini_cli.py sdks/python/motosan_ai/providers/claude_code.py sdks/python/tests/test_codex_cli_stream.py sdks/python/tests/test_gemini_cli_stream.py sdks/python/tests/test_claude_code_runtime.py
git commit -m "fix(python): raise StreamError when a CLI child dies before the terminal event"
```

### Task 11: Surface Anthropic mid-stream error frames (TS)
**Files:**
- Modify: `sdks/typescript/src/providers/anthropic.ts` (import block, approximate lines 1-5; SSE event switch tail, approximate lines 373-383)
- Test: `sdks/typescript/tests/providers-anthropic.test.ts` (EXTEND the existing `describe('AnthropicProvider stream')` block, approximate lines 225-455)

**Interfaces:** Consumes `StreamError` from `sdks/typescript/src/error.ts` (approx. line 11: `export class StreamError extends MotosanError {}`, where `MotosanError extends Error`). No public API change: `AnthropicProvider.stream(request: ChatRequest): BoxStream` is unchanged; the returned async generator now throws `StreamError` when the server sends an SSE `event: error` frame mid-stream on an HTTP 200 (today that frame falls into the switch's `default:` case, is silently ignored, and the stream ends with a fabricated clean done).

**Scope guard:** Do NOT touch the defensive end-of-stream fallback at approximate lines 386-391 (`// Defensive: terminate even if message_stop never arrived.` … `yield doneEvent()`). Removing the fabricated clean-done-on-EOF is milestone M3, not this task.

- [ ] **Step 1: Write the failing test.** Two edits to `sdks/typescript/tests/providers-anthropic.test.ts`.

  (a) At the top of the file, after the existing line 3 `import type { ChatRequest, StreamEvent } from '../src/types.js'`, add:

  ```ts
  import { StreamError } from '../src/error.js'
  ```

  (b) Inside `describe('AnthropicProvider stream', ...)` (it already defines a `streamFromTranscript(sse, onRequest?)` helper at approx. lines 230-251 that stubs global fetch with a `ReadableStream` SSE body), add this test AFTER the test `'does not emit thinking_done for an empty thinking block (start/stop, no deltas)'` (which ends at approx. line 454), before the closing `})` of the describe block:

  ```ts
  it('rejects with a StreamError on a mid-stream error frame (deltas already emitted survive)', async () => {
    const sse =
      'event: message_start\n' +
      'data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3-5-sonnet-20241022","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":3,"output_tokens":0}}}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n' +
      'event: error\n' +
      'data: {"type":"error","error":{"type":"overloaded_error","message":"Overloaded"}}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(event)
      }
    } catch (e) {
      error = e
    }

    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toContain('overloaded_error')
    expect((error as Error).message).toContain('Overloaded')
    // The text delta emitted before the error frame was still yielded,
    // and NO fabricated done event followed the error.
    expect(
      events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content),
    ).toEqual(['partial'])
    expect(events.some((e) => e.done)).toBe(false)
  })
  ```

- [ ] **Step 2: Run the test, verify it FAILS.** From the repo root: `cd sdks/typescript` (if `node_modules/` is missing, run `npm install` first), then:

  ```bash
  npx vitest run tests/providers-anthropic.test.ts
  ```

  Expected failure signature — the current adapter ignores the `error` frame and fabricates a clean done at EOF, so no error is thrown:

  ```
  FAIL  tests/providers-anthropic.test.ts > AnthropicProvider stream > rejects with a StreamError on a mid-stream error frame (deltas already emitted survive)
  AssertionError: expected undefined to be an instance of StreamError
  ```

- [ ] **Step 3: Implement.** Two edits to `sdks/typescript/src/providers/anthropic.ts`.

  Edit 1 — Current code (approximate lines 1-5):

  ```ts
  import {
    isRetryableNetworkError,
    isRetryableStatus,
    ProviderError,
  } from '../error.js'
  ```

  Replace with (note: `ProviderError` is currently imported but unused; keep it to minimize the diff — tsconfig has no `noUnusedLocals`):

  ```ts
  import {
    isRetryableNetworkError,
    isRetryableStatus,
    ProviderError,
    StreamError,
  } from '../error.js'
  ```

  Edit 2 — Current code (approximate lines 373-383, the tail of the `switch (evt.event)` inside `streamImpl`; indentation is 8 spaces before `case`):

  ```ts
        case 'message_stop': {
          yield state.stopReason !== undefined
            ? doneWithStopReason(state.stopReason)
            : doneEvent()
          return
        }

        default:
          // ping / unknown events are ignored.
          break
  ```

  Replace with:

  ```ts
        case 'message_stop': {
          yield state.stopReason !== undefined
            ? doneWithStopReason(state.stopReason)
            : doneEvent()
          return
        }

        case 'error': {
          // Anthropic emits `event: error` frames mid-stream on an HTTP 200
          // (e.g. overloaded_error). Surface them as a StreamError instead
          // of swallowing them and fabricating a clean done at EOF.
          const errType = String(data?.error?.type ?? 'error')
          const errMessage = String(data?.error?.message ?? 'unknown')
          throw new StreamError(`anthropic stream error (${errType}): ${errMessage}`)
        }

        default:
          // ping / unknown events are ignored.
          break
  ```

  Leave the defensive EOF block at approximate lines 386-391 exactly as is.

- [ ] **Step 4: Run the test + the touched package test suite.** From `sdks/typescript`:

  ```bash
  npx vitest run tests/providers-anthropic.test.ts
  npm run build && npm test
  ```

  The `npm run build` before `npm test` is REQUIRED in a fresh worktree: `tests/pack-smoke.test.ts` asserts that `dist/index.js` and `dist/index.d.ts` exist, and `dist/` is gitignored — without a prior build, that one unrelated test fails ('exports-map targets exist after build').

  Expected: the new test passes; `tests/providers-anthropic.test.ts` reports all tests passed (the `'AnthropicProvider live'` test is auto-skipped without `ANTHROPIC_API_KEY`); `npm test` (full vitest suite) passes with no failures — in particular the `tests/edge-cases.test.ts` test `'Anthropic: stream that ends without message_stop terminates silently with a partial response'` still passes because the EOF fallback was not touched.

- [ ] **Step 5: Format & lint.** The TS package has no formatter/linter config; the gate is the typechecker. From `sdks/typescript`:

  ```bash
  npx tsc -p tsconfig.json --noEmit
  ```

  Expected: no output, exit code 0.

- [ ] **Step 6: Commit.**

  ```bash
  git add sdks/typescript/src/providers/anthropic.ts sdks/typescript/tests/providers-anthropic.test.ts
  git commit -m "fix(ts): surface Anthropic mid-stream SSE error frames as StreamError"
  ```

### Task 12: Stop swallowing chatgpt_codex error/response.failed frames (TS)
**Files:**
- Modify: `sdks/typescript/src/providers/chatgpt_codex.ts` (module docstring, approximate lines 1-9; import, approximate line 11; helper docstring, approximate lines 38-46; adapter comment, approximate lines 253-254; error case, approximate lines 317-321; catch block, approximate lines 326-330)
- Modify: `sdks/typescript/src/index.ts` (stale comment, approximate line 12)
- Test: `sdks/typescript/tests/providers-chatgpt-codex.test.ts` (FLIP the two silent-termination pinning tests at approximate lines 323-351; extend the import at approximate line 7)

**Interfaces:** Consumes `chatGptCodexErrorMessage(chunk: any): string` (already exported from the SAME file at approximate lines 47-54 — shipped but unused by the stream path; REUSE it, do not reimplement) and `StreamError` from `sdks/typescript/src/error.ts` (approx. line 11: `export class StreamError extends MotosanError {}`). No public API change: `ChatGptCodexProvider.stream()` signature is unchanged; the async generator now throws `StreamError` on fatal `error` / `response.failed` frames — parity with Rust (`chatgpt_codex.rs` surfaces `MotosanError::Stream(msg)`) and Python (`chatgpt_codex.py` raises `StreamError`). Today TS bare-returns, producing a truncated success.

- [ ] **Step 1: Flip the pinning tests.** Two edits to `sdks/typescript/tests/providers-chatgpt-codex.test.ts`.

  (a) Current code (approximate line 7):

  ```ts
  import { AuthError, NetworkError, ProviderError, RateLimitError } from '../src/error.js'
  ```

  Replace with:

  ```ts
  import { AuthError, NetworkError, ProviderError, RateLimitError, StreamError } from '../src/error.js'
  ```

  (b) Current code (approximate lines 323-351, inside `describe('ChatGptCodexProvider SSE adapter', ...)` — these two tests PIN the silent-swallow behavior):

  ```ts
  it('terminates silently (no throw) on a top-level error frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"error","message":"rate limited"}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    await expect(
      (async () => {
        for await (const e of prov.stream(REQ)) events.push(e)
      })(),
    ).resolves.toBeUndefined()
    // the partial text was yielded; NO terminal done; NO throw
    expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])
  })

  it('terminates silently on a response.failed frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.failed","response":{"error":{"message":"boom"}}}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    await expect(
      (async () => {
        for await (const e of prov.stream(REQ)) events.push(e)
      })(),
    ).resolves.toBeUndefined()
    expect(events).toHaveLength(0)
  })
  ```

  Replace with:

  ```ts
  it('rejects with a StreamError on a top-level error frame (partial text still emitted)', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"error","message":"rate limited"}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const e of prov.stream(REQ)) events.push(e)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('rate limited')
    // the partial text was yielded before the throw; NO terminal done
    expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])
  })

  it('rejects with the helper-formatted message on a response.failed frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.failed","response":{"error":{"message":"boom"}}}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    let error: unknown
    try {
      for await (const e of prov.stream(REQ)) events.push(e)
    } catch (e) {
      error = e
    }
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('boom')
    expect(events).toHaveLength(0)
  })
  ```

- [ ] **Step 2: Run the tests, verify the flipped ones FAIL.** From the repo root: `cd sdks/typescript` (if `node_modules/` is missing, run `npm install` first), then:

  ```bash
  npx vitest run tests/providers-chatgpt-codex.test.ts
  ```

  Expected failure signature — the current adapter bare-returns on the error frames, so nothing is thrown:

  ```
  FAIL  tests/providers-chatgpt-codex.test.ts > ChatGptCodexProvider SSE adapter > rejects with a StreamError on a top-level error frame (partial text still emitted)
  AssertionError: expected undefined to be an instance of StreamError
  FAIL  tests/providers-chatgpt-codex.test.ts > ChatGptCodexProvider SSE adapter > rejects with the helper-formatted message on a response.failed frame
  AssertionError: expected undefined to be an instance of StreamError
  ```

- [ ] **Step 3: Implement.** Five edits: four in `sdks/typescript/src/providers/chatgpt_codex.ts`, one comment fix in `sdks/typescript/src/index.ts`. IMPORTANT: the throw sits inside a `try` whose bare `catch` currently swallows everything — Edit 4 (the re-throw) is what makes Edit 3 work; do not skip it.

  Edit 1 — Current code (approximate lines 6-8, inside the module docstring):

  ```ts
   * `chatgpt_codex.rs`) in idiomatic TS, with one deliberate divergence: a
   * mid-stream `error` / `response.failed` frame terminates the stream SILENTLY
   * (TS convention — ollama.ts:362-366), instead of the Python mid-stream raise.
  ```

  Replace with:

  ```ts
   * `chatgpt_codex.rs`) in idiomatic TS: a mid-stream `error` / `response.failed`
   * frame throws a `StreamError` (parity with Rust `MotosanError::Stream` and
   * the Python mid-stream `StreamError` raise).
  ```

  Edit 2 — Current code (approximate line 11):

  ```ts
  import { isRetryableNetworkError, isRetryableStatus } from '../error.js'
  ```

  Replace with:

  ```ts
  import { StreamError, isRetryableNetworkError, isRetryableStatus } from '../error.js'
  ```

  Also update the helper docstring — Current code (approximate lines 41-43, in the `chatGptCodexErrorMessage` doc comment):

  ```ts
   * `error.message` → fallback. Pure; used by tests only. The stream path silently
   * terminates without surfacing this (plan §C), so this is NOT re-exported from
   * `src/index.ts`.
  ```

  Replace with:

  ```ts
   * `error.message` → fallback. Used by the stream adapter to build the
   * `StreamError` for fatal frames. NOT re-exported from `src/index.ts`.
  ```

  Edit 3 — Current code (approximate lines 253-255 and 317-321, inside `streamImpl`):

  ```ts
      // Mid-stream body errors terminate the stream SILENTLY (TS convention;
      // ollama.ts:362-366, providers-ollama.test.ts:377). NO mid-stream throw.
      try {
  ```

  Replace with:

  ```ts
      // Fatal `error` / `response.failed` frames throw a StreamError (Rust/
      // Python parity). Other post-start body errors still end silently (M3).
      try {
  ```

  and

  ```ts
            case 'error':
            case 'response.failed':
              // Silent terminate (TS convention). The Python `raise StreamError`
              // path is intentionally NOT ported. See plan §C.
              return
  ```

  Replace with:

  ```ts
            case 'error':
            case 'response.failed':
              // Fatal stream error frame: surface it (Rust MotosanError::Stream
              // / Python StreamError parity).
              throw new StreamError(chatGptCodexErrorMessage(data))
  ```

  Edit 4 — Current code (approximate lines 326-330, the catch wrapping the SSE loop):

  ```ts
      } catch {
        // Ignore post-start stream-body errors; end without a terminal done
        // (mirrors ollama.ts:362-366).
        return
      }
  ```

  Replace with:

  ```ts
      } catch (error) {
        if (error instanceof StreamError) {
          throw error
        }
        // Ignore other post-start stream-body errors; end without a terminal
        // done (mirrors ollama.ts:362-366). Surfacing these is milestone M3.
        return
      }
  ```

  Edit 5 — Current code in `sdks/typescript/src/index.ts` (approximate line 12):

  ```ts
  // `chatGptCodexErrorMessage` is @internal (test-only) and NOT re-exported.
  ```

  Replace with:

  ```ts
  // `chatGptCodexErrorMessage` is @internal and NOT re-exported.
  ```

- [ ] **Step 4: Run the test + the touched package test suite.** From `sdks/typescript`. NOTE: `tests/pack-smoke.test.ts` asserts `dist/index.js` exists, and `dist/` is gitignored (absent in a fresh worktree) — the `test` script does NOT build, so you must run `npm run build` before the full suite or pack-smoke fails spuriously:

  ```bash
  npx vitest run tests/providers-chatgpt-codex.test.ts
  npm run build && npm test
  ```

  Expected: both flipped tests pass, every other test in the file passes (the four `chatGptCodexErrorMessage` unit tests are unchanged and still pass), and the full `npm test` suite (including `pack-smoke.test.ts`, now that `dist/` exists from the build) is green.

- [ ] **Step 5: Format & lint.** The TS package has no formatter/linter config; the gate is the typechecker. From `sdks/typescript`:

  ```bash
  npx tsc -p tsconfig.json --noEmit
  ```

  Expected: no output, exit code 0.

- [ ] **Step 6: Commit.**

  ```bash
  git add sdks/typescript/src/providers/chatgpt_codex.ts sdks/typescript/src/index.ts sdks/typescript/tests/providers-chatgpt-codex.test.ts
  git commit -m "fix(ts): throw StreamError on chatgpt_codex error/response.failed frames"
  ```


## W3 — Streamed tool-call integrity

### Task 13: Make Rust OpenAI stream adapter index-aware for parallel tool calls

**Files:**
- Modify: `sdks/rust/src/providers/openai.rs` (adapter construction ~638-644; struct fields ~662-663; `[DONE]` branch ~677-683; tool-calls loop ~723-746; finish_reason block ~749-761; in-file unit test ~846-852 — all line refs approximate)
- Test: `sdks/rust/tests/openai_provider.rs` (extend; mockito SSE-fixture style used throughout this file)
- Test (live, self-skipping): `sdks/rust/tests/openai_live.rs` (extend; skips when `OPENAI_API_KEY` unset — this repo's live convention, no `#[ignore]`)

**Interfaces:** Consumes `StreamEvent::tool_call_start(id: impl Into<String>, name: impl Into<String>)`, `StreamEvent::tool_call_args_with_id(id: impl Into<String>, delta: impl Into<String>)`, `StreamEvent::tool_call_end_with_id(id: impl Into<String>)` (all in `sdks/rust/src/types.rs`, set `tool_call_id: Some(...)`), and `motosan_ai::collect_stream(BoxStream) -> Result<ChatResponse, MotosanError>` (tests only). Produces: NO public API change — only private fields of `OpenAIStreamAdapter` change.

**Why:** OpenAI streams parallel tool calls as fragments carrying `index` (id+name only on the first fragment of each call; later arg fragments carry index but NO id). The current loop never reads `index` and does `tc_id.unwrap_or("")`, emitting empty-id arg events; with two interleaved calls the single-accumulator collector in `sdks/rust/src/stream.rs` drops/corrupts calls. **Constraint: emitted events must stay SEQUENTIAL per call (start A, args A…, end A, start B, …). Do NOT modify `sdks/rust/src/stream.rs`.**

- [ ] **Step 1: Write the failing test** — append to `sdks/rust/tests/openai_provider.rs` (imports at top of file already provide everything needed):

```rust
#[tokio::test]
async fn openai_stream_parallel_tool_calls_interleaved_stay_sequential_per_call() {
    // OpenAI streams parallel tool calls as fragments keyed by `index`:
    // id+name arrive only on the FIRST fragment of each call, later
    // argument fragments carry index but NO id, and fragments of different
    // calls may interleave. The adapter must re-serialize them so events
    // stay sequential per call with real ids — collect_stream accumulates
    // into a single current-tool buffer and is corrupted otherwise.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_A\",\"function\":{\"name\":\"get_weather\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"call_B\",\"function\":{\"name\":\"get_time\",\"arguments\":\"\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"city\\\":\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"function\":{\"arguments\":\"{\\\"tz\\\":\\\"Asia/Tokyo\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"Taipei\\\"}\"}}]},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n"
    );

    let mock = server
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()));
    let request = ChatRequest::builder()
        .message(Message::user("weather in Taipei and time in Tokyo?"))
        .build();

    let mut stream = provider.stream(request).await.expect("stream response");
    let mut events = Vec::new();
    while let Some(event_item) = stream.next().await {
        events.push(event_item.expect("stream item should not fail"));
    }

    let tool_seq: Vec<(StreamEventType, String)> = events
        .iter()
        .filter(|e| {
            matches!(
                e.event_type,
                StreamEventType::ToolCallStart
                    | StreamEventType::ToolCallArgs
                    | StreamEventType::ToolCallEnd
            )
        })
        .map(|e| {
            (
                e.event_type.clone(),
                e.tool_call_id.clone().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(
        tool_seq,
        vec![
            (StreamEventType::ToolCallStart, "call_A".to_string()),
            (StreamEventType::ToolCallArgs, "call_A".to_string()),
            (StreamEventType::ToolCallArgs, "call_A".to_string()),
            (StreamEventType::ToolCallEnd, "call_A".to_string()),
            (StreamEventType::ToolCallStart, "call_B".to_string()),
            (StreamEventType::ToolCallArgs, "call_B".to_string()),
            (StreamEventType::ToolCallEnd, "call_B".to_string()),
        ],
        "per-call events must be sequential with real (buffered) ids"
    );

    // The single-accumulator collector must assemble both calls intact.
    let boxed: motosan_ai::BoxStream =
        Box::pin(tokio_stream::iter(events.clone().into_iter().map(Ok)));
    let response = motosan_ai::collect_stream(boxed).await.unwrap();
    assert_eq!(response.tool_calls.len(), 2);
    assert_eq!(response.tool_calls[0].id, "call_A");
    assert_eq!(response.tool_calls[0].name, "get_weather");
    assert_eq!(response.tool_calls[0].input, json!({"city": "Taipei"}));
    assert_eq!(response.tool_calls[1].id, "call_B");
    assert_eq!(response.tool_calls[1].name, "get_time");
    assert_eq!(response.tool_calls[1].input, json!({"tz": "Asia/Tokyo"}));
    assert!(matches!(response.stop_reason, StopReason::ToolUse));

    mock.assert_async().await;
}
```

Also extend `sdks/rust/tests/openai_live.rs`. Change its import line (~line 10) from `use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason};` to `use motosan_ai::{ChatRequest, Client, Message, Provider, StopReason, Tool};` and append (it self-skips without `OPENAI_API_KEY`, matching the two existing tests in that file):

```rust
#[tokio::test]
async fn live_openai_parallel_tool_calls_collect_intact() {
    let Some(client) = client() else {
        eprintln!("OPENAI_API_KEY not set, skipping");
        return;
    };

    let tool = |name: &str, desc: &str, field: &str| Tool {
        schema: motosan_agent_primitives::ToolSchema {
            name: name.to_string(),
            description: desc.to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {field: {"type": "string"}},
                "required": [field]
            }),
        },
        cache: false,
    };

    // gpt-4o-mini emits parallel tool calls when asked for two lookups.
    let request = ChatRequest::builder()
        .message(Message::user(
            "Call get_weather for Taipei AND get_time for Asia/Tokyo. \
             Use both tools in one turn.",
        ))
        .tools(vec![
            tool("get_weather", "Get current weather for a city", "city"),
            tool("get_time", "Get current time for an IANA timezone", "tz"),
        ])
        .build();

    let stream = client.stream_with(request).await.expect("stream failed");
    let response = motosan_ai::collect_stream(stream).await.unwrap();

    assert!(
        !response.tool_calls.is_empty(),
        "expected tool calls, got none (content: {})",
        response.content
    );
    for tc in &response.tool_calls {
        assert!(!tc.id.is_empty(), "tool call id must not be empty");
        assert!(!tc.name.is_empty(), "tool call name must not be empty");
        assert!(tc.input.is_object(), "tool input must be assembled JSON");
    }
    assert_eq!(response.stop_reason, StopReason::ToolUse);

    cooldown().await;
}
```

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/rust`:
```bash
cargo test --all-features --test openai_provider openai_stream_parallel_tool_calls_interleaved_stay_sequential_per_call
```
Expected failure: panic `assertion \`left == right\` failed: per-call events must be sequential with real (buffered) ids` — the left side shows the corrupted sequence `[(ToolCallStart, "call_A"), (ToolCallStart, "call_B"), (ToolCallArgs, ""), (ToolCallArgs, ""), (ToolCallArgs, ""), (ToolCallEnd, "call_A"), (ToolCallEnd, "call_B")]` (empty-id args events, ends only at finish_reason).

- [ ] **Step 3: Implement** — six edits in `sdks/rust/src/providers/openai.rs`.

**(a)** Current code (approximate lines 638-644):
```rust
        let adapter = OpenAIStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            seen_tool_ids: Vec::new(),
            pending_stop_reason: None,
            done_emitted: false,
        };
```
Replace with:
```rust
        let adapter = OpenAIStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            tool_bufs: std::collections::BTreeMap::new(),
            open_tool_index: None,
            pending_stop_reason: None,
            done_emitted: false,
        };
```

**(b)** Current code (approximate lines 662-663, inside `struct OpenAIStreamAdapter`):
```rust
    pending: std::collections::VecDeque<StreamEvent>,
    seen_tool_ids: Vec<String>,
```
Replace with:
```rust
    pending: std::collections::VecDeque<StreamEvent>,
    /// Per-index buffers for tool calls. OpenAI streams parallel calls as
    /// fragments keyed by `index` (id+name only on the first fragment of
    /// each call; later argument fragments carry index but no id).
    /// `BTreeMap` keeps flush order deterministic (ascending index).
    tool_bufs: std::collections::BTreeMap<u64, ToolBuf>,
    /// Index of the call currently streamed eagerly (the first to appear).
    /// Later parallel calls are buffered whole and flushed as complete
    /// start/args/end sequences so events stay sequential per call for
    /// single-accumulator consumers like `collect_stream`.
    open_tool_index: Option<u64>,
```

**(c)** Insert a new struct between the closing `}` of `struct OpenAIStreamAdapter` (approximate line 673) and `impl OpenAIStreamAdapter {`:
```rust
/// Buffered fragments of one indexed tool call from an OpenAI stream.
#[derive(Default)]
struct ToolBuf {
    id: String,
    name: String,
    args: String,
}
```

**(d)** Current code (approximate lines 677-680, top of `parse_event`):
```rust
        if data.trim() == "[DONE]" {
            // Emit a single terminal done event, attaching any stop_reason
            // captured from the previous chunk's finish_reason field.
            let done = match self.pending_stop_reason.take() {
```
Replace with:
```rust
        if data.trim() == "[DONE]" {
            // Flush any tool calls the provider never closed with a
            // finish_reason chunk, then emit a single terminal done event,
            // attaching any stop_reason captured from the previous chunk.
            self.flush_tool_calls();
            let done = match self.pending_stop_reason.take() {
```

**(e)** Current code (approximate lines 723-746, the tool-calls loop):
```rust
            // Tool calls in delta
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    let tc_id = tc.get("id").and_then(Value::as_str);
                    let function = tc.get("function");
                    let tc_name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                    let tc_args = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str);

                    if let (Some(id), Some(name)) = (tc_id, tc_name) {
                        self.seen_tool_ids.push(id.to_string());
                        self.pending
                            .push_back(StreamEvent::tool_call_start(id, name));
                    }
                    if let Some(args) = tc_args {
                        if !args.is_empty() {
                            let id = tc_id.unwrap_or("");
                            self.pending
                                .push_back(StreamEvent::tool_call_args_with_id(id, args));
                        }
                    }
                }
            }
```
Replace with:
```rust
            // Tool calls in delta. OpenAI streams parallel calls as
            // fragments keyed by `index`: id+name arrive only on the first
            // fragment of each call; later argument fragments carry index
            // but NO id. The first call streams eagerly; other parallel
            // calls are buffered per index and flushed whole (see
            // `flush_tool_calls`) so events stay sequential per call.
            if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                for tc in tool_calls {
                    // Missing `index` (non-conformant proxies) maps to 0,
                    // preserving the old single-call behavior.
                    let idx = tc.get("index").and_then(Value::as_u64).unwrap_or(0);
                    let tc_id = tc.get("id").and_then(Value::as_str);
                    let function = tc.get("function");
                    let tc_name = function.and_then(|f| f.get("name")).and_then(Value::as_str);
                    let tc_args = function
                        .and_then(|f| f.get("arguments"))
                        .and_then(Value::as_str);

                    if let (Some(id), Some(name)) = (tc_id, tc_name) {
                        let buf = self.tool_bufs.entry(idx).or_default();
                        buf.id = id.to_string();
                        buf.name = name.to_string();
                        if self.open_tool_index.is_none() {
                            // First call to appear: stream it eagerly.
                            self.open_tool_index = Some(idx);
                            self.pending
                                .push_back(StreamEvent::tool_call_start(id, name));
                        }
                    }
                    if let Some(args) = tc_args {
                        if !args.is_empty() {
                            match self.tool_bufs.get_mut(&idx) {
                                Some(buf) if self.open_tool_index == Some(idx) => {
                                    let id = buf.id.clone();
                                    self.pending.push_back(
                                        StreamEvent::tool_call_args_with_id(id, args),
                                    );
                                }
                                Some(buf) => buf.args.push_str(args),
                                // Fragment for an index that never announced
                                // id+name: drop it — emitting a fabricated
                                // empty id corrupts downstream collectors.
                                None => {}
                            }
                        }
                    }
                }
            }
```

**(f)** Current code (approximate lines 749-761, finish_reason block):
```rust
        // Finish reason — stash for the upcoming `[DONE]` sentinel so we
        // emit exactly one terminal done event with stop_reason attached.
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        if let Some(reason) = finish_reason {
            if reason == "tool_calls" {
                let ids: Vec<String> = self.seen_tool_ids.drain(..).collect();
                for id in &ids {
                    self.pending
                        .push_back(StreamEvent::tool_call_end_with_id(id));
                }
            }
            self.pending_stop_reason = Some(map_finish_reason(reason));
        }
```
Replace with:
```rust
        // Finish reason — stash for the upcoming `[DONE]` sentinel so we
        // emit exactly one terminal done event with stop_reason attached.
        let finish_reason = choice.get("finish_reason").and_then(Value::as_str);
        if let Some(reason) = finish_reason {
            if reason == "tool_calls" {
                self.flush_tool_calls();
            }
            self.pending_stop_reason = Some(map_finish_reason(reason));
        }
```
Then add this method inside `impl OpenAIStreamAdapter` immediately after the closing `}` of `parse_event` (before the `impl` block's closing `}`):
```rust
    /// Close the eagerly-streamed call (if any), then emit every other
    /// buffered parallel call as a complete start/args/end sequence in
    /// ascending index order. No-op when no tool calls are pending.
    fn flush_tool_calls(&mut self) {
        if let Some(open_idx) = self.open_tool_index.take() {
            if let Some(buf) = self.tool_bufs.remove(&open_idx) {
                self.pending
                    .push_back(StreamEvent::tool_call_end_with_id(buf.id));
            }
        }
        for buf in std::mem::take(&mut self.tool_bufs).into_values() {
            self.pending
                .push_back(StreamEvent::tool_call_start(buf.id.clone(), buf.name));
            if !buf.args.is_empty() {
                self.pending
                    .push_back(StreamEvent::tool_call_args_with_id(buf.id.clone(), buf.args));
            }
            self.pending
                .push_back(StreamEvent::tool_call_end_with_id(buf.id));
        }
    }
```

**(g)** Current code (approximate lines 846-852, in `mod tests` at the bottom of openai.rs — this is the last remaining `seen_tool_ids` reference; `grep -n seen_tool_ids sdks/rust/src/providers/openai.rs` must return nothing after this edit):
```rust
        let mut adapter = OpenAIStreamAdapter {
            inner: Box::pin(inner),
            pending: std::collections::VecDeque::new(),
            seen_tool_ids: Vec::new(),
            pending_stop_reason: None,
            done_emitted: false,
        };
```
Replace with:
```rust
        let mut adapter = OpenAIStreamAdapter {
            inner: Box::pin(inner),
            pending: std::collections::VecDeque::new(),
            tool_bufs: std::collections::BTreeMap::new(),
            open_tool_index: None,
            pending_stop_reason: None,
            done_emitted: false,
        };
```

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/rust`:
```bash
cargo test --all-features --test openai_provider openai_stream_parallel_tool_calls_interleaved_stay_sequential_per_call
cargo test --all-features --test openai_provider
cargo test --all-features
```
Expected: the new test passes; `--test openai_provider` reports `test result: ok. 18 passed` (17 existing + 1 new — in particular `openai_stream_emits_tool_call_events` and `openai_stream_propagates_finish_reason_tool_calls` must still pass unchanged); the full suite passes (live tests self-skip without API keys; `#[ignore]`d CLI tests stay ignored). Optional manual live check: `OPENAI_API_KEY=... cargo test --features openai --test openai_live live_openai_parallel_tool_calls_collect_intact -- --nocapture`.

- [ ] **Step 5: Format & lint** — from `sdks/rust`:
```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```
Expected: no diff complaints, zero clippy warnings.

- [ ] **Step 6: Commit**
```bash
git add sdks/rust/src/providers/openai.rs sdks/rust/tests/openai_provider.rs sdks/rust/tests/openai_live.rs
git commit -m "fix(openai): buffer parallel tool calls by index in stream adapter"
```

### Task 14: Fix chatgpt-codex streamed tool-call arg fragments keyed by wire item_id instead of call_id

**Files:**
- Modify: `sdks/rust/src/providers/chatgpt_codex.rs` (imports ~line 16; adapter struct ~330-341; `stream()` adapter construction ~295-301; `handle_event` ~372-408; inline tests ~736-753, ~833-871, ~897-903)
- Test: `sdks/rust/src/providers/chatgpt_codex.rs` (extend existing `mod tests::adapter_tests`) and NEW `sdks/rust/tests/chatgpt_codex_live.rs` (`#[ignore]` live smoke)

**Interfaces:** None (self-contained). Uses existing constructors from `sdks/rust/src/types.rs`: `StreamEvent::tool_call_start(id, name)`, `StreamEvent::tool_call_args_with_id(id, delta)`, `StreamEvent::tool_call_end_with_id(id)`.

**Context.** On the real ChatGPT-backend Responses wire, a `function_call` output item carries TWO ids: the item `id` (`fc_…`) and the `call_id` (`call_…`). `response.output_item.added`/`.done` expose both, and the adapter correctly emits ToolCallStart/ToolCallEnd with `call_id`. But `response.function_call_arguments.delta` frames are keyed by `item_id` — the item `id` (`fc_…`) — which the adapter currently passes through verbatim, so every real streamed tool call emits arg fragments whose `tool_call_id` (`fc_…`) matches no ToolCallStart (`call_…`): orphaned fragments for any consumer that correlates by id. The existing unit test masks this by using `item_id == call_id == "call_42"`.

- [ ] **Step 1: Write the failing test** — in `sdks/rust/src/providers/chatgpt_codex.rs`, inside `mod tests` → `mod adapter_tests` (~line 736), REPLACE the whole existing `adapter_handles_function_call_lifecycle` test (~lines 833-871) with the two tests below (the second is new; keep all other tests untouched):

```rust
        #[test]
        fn adapter_handles_function_call_lifecycle() {
            // Real-wire shape: the `item` carries BOTH ids — `id` ("fc_…", the
            // item id) and `call_id` ("call_…"). Argument delta frames are
            // keyed by `item_id` == the item's `id`, NOT the call_id, so the
            // adapter must translate fc_001 -> call_001 for arg events.
            let mut adapter = fresh_adapter();
            let events = drive(
                &mut adapter,
                &[
                    r#"{"type":"response.output_item.added","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"get_weather"}}"#,
                    r#"{"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"{\"city\":"}"#,
                    r#"{"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"\"Paris\"}"}"#,
                    r#"{"type":"response.output_item.done","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"get_weather"}}"#,
                    r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":3,"output_tokens":7}}}"#,
                ],
            );

            let start = events
                .iter()
                .find(|e| e.event_type == StreamEventType::ToolCallStart)
                .expect("tool_call_start");
            assert_eq!(start.tool_call_id.as_deref(), Some("call_001"));
            assert_eq!(start.tool_call_name.as_deref(), Some("get_weather"));

            // Every arg fragment must carry the call_id, not the wire item_id
            // — consumers correlate fragments to the ToolCallStart by id.
            let arg_events: Vec<_> = events
                .iter()
                .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
                .collect();
            assert_eq!(arg_events.len(), 2);
            for e in &arg_events {
                assert_eq!(e.tool_call_id.as_deref(), Some("call_001"));
            }
            let args: String = arg_events
                .iter()
                .filter_map(|e| e.tool_call_args_delta.clone())
                .collect();
            assert_eq!(args, r#"{"city":"Paris"}"#);

            let end = events
                .iter()
                .find(|e| e.event_type == StreamEventType::ToolCallEnd)
                .expect("tool_call_end");
            assert_eq!(end.tool_call_id.as_deref(), Some("call_001"));

            // Any tool call seen => terminal stop reason is ToolUse, not EndTurn.
            let done = events.iter().find(|e| e.done).expect("a done event");
            assert_eq!(done.stop_reason, Some(StopReason::ToolUse));
        }

        #[test]
        fn adapter_passes_through_unknown_arg_item_id() {
            // A delta whose item_id was never registered via output_item.added
            // falls back to the raw wire id (defensive: never drop fragments).
            let mut adapter = fresh_adapter();
            let events = drive(
                &mut adapter,
                &[r#"{"type":"response.function_call_arguments.delta","item_id":"fc_unseen","delta":"{}"}"#],
            );
            let arg = events
                .iter()
                .find(|e| e.event_type == StreamEventType::ToolCallArgs)
                .expect("tool_call_args");
            assert_eq!(arg.tool_call_id.as_deref(), Some("fc_unseen"));
            assert_eq!(arg.tool_call_args_delta.as_deref(), Some("{}"));
        }
```

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/rust`:

```sh
cargo test --all-features adapter_handles_function_call_lifecycle
```

Expected failure: `adapter_handles_function_call_lifecycle` panics at the arg-fragment id assertion with:

```
assertion `left == right` failed
  left: Some("fc_001")
 right: Some("call_001")
```

(`adapter_passes_through_unknown_arg_item_id` already passes — it pins the current pass-through as the post-fix fallback.)

- [ ] **Step 3: Implement** — four edits in `sdks/rust/src/providers/chatgpt_codex.rs`.

**(a) Import HashMap.** Current code (approximate line 16):
```rust
use std::collections::{HashSet, VecDeque};
```
Replace with:
```rust
use std::collections::{HashMap, HashSet, VecDeque};
```

**(b) Add the map field.** Current code (approximate lines 330-333, in `struct ChatGptCodexStreamAdapter`):
```rust
    pending: VecDeque<StreamEvent>,
    /// `call_id`s seen via `response.output_item.added` (function_call) so the
    /// matching `response.output_item.done` can close the same id.
    seen_tool_ids: HashSet<String>,
```
Replace with:
```rust
    pending: VecDeque<StreamEvent>,
    /// `call_id`s seen via `response.output_item.added` (function_call) so the
    /// matching `response.output_item.done` can close the same id.
    seen_tool_ids: HashSet<String>,
    /// Maps a `function_call` item's wire `id` ("fc_…") to its `call_id`
    /// ("call_…"). `response.function_call_arguments.delta` frames are keyed
    /// by `item_id` — the item `id`, NOT the `call_id` — so arg fragments are
    /// translated through this map to match `tool_call_start`/`tool_call_end`.
    item_to_call_id: HashMap<String, String>,
```

**(c) Register the mapping in `output_item.added`.** Current code (approximate lines 385-390, inside the `"response.output_item.added"` arm):
```rust
                        if !call_id.is_empty() {
                            self.saw_tool_call = true;
                            self.seen_tool_ids.insert(call_id.clone());
                            self.pending
                                .push_back(StreamEvent::tool_call_start(&call_id, &name));
                        }
```
Replace with:
```rust
                        if !call_id.is_empty() {
                            self.saw_tool_call = true;
                            self.seen_tool_ids.insert(call_id.clone());
                            if let Some(item_id) = item
                                .get("id")
                                .and_then(Value::as_str)
                                .filter(|s| !s.is_empty())
                            {
                                self.item_to_call_id
                                    .insert(item_id.to_string(), call_id.clone());
                            }
                            self.pending
                                .push_back(StreamEvent::tool_call_start(&call_id, &name));
                        }
```

**(d) Translate in `function_call_arguments.delta`.** Current code (approximate lines 395-408):
```rust
            // Streamed tool-call argument fragments. The wire key is `item_id`.
            "response.function_call_arguments.delta" => {
                let id = data
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !id.is_empty() {
                        self.pending
                            .push_back(StreamEvent::tool_call_args_with_id(&id, delta));
                    }
                }
            }
```
Replace with:
```rust
            // Streamed tool-call argument fragments. The wire key is `item_id`
            // — the function_call item's `id` ("fc_…"), NOT its `call_id`
            // ("call_…"). Translate through the map built in
            // `response.output_item.added` so fragments carry the same id as
            // ToolCallStart/ToolCallEnd; fall back to the raw wire id if the
            // item was never registered.
            "response.function_call_arguments.delta" => {
                let wire_id = data
                    .get("item_id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(delta) = data.get("delta").and_then(Value::as_str) {
                    if !wire_id.is_empty() {
                        let id = self
                            .item_to_call_id
                            .get(&wire_id)
                            .cloned()
                            .unwrap_or(wire_id);
                        self.pending
                            .push_back(StreamEvent::tool_call_args_with_id(&id, delta));
                    }
                }
            }
```

**Then initialize the new field at all THREE struct-literal sites** (the compiler will error on any you miss). At each, add `item_to_call_id: HashMap::new(),` directly after the `seen_tool_ids: HashSet::new(),` line, keeping that site's indentation:
1. `stream()` (approximate lines 295-301): `let adapter = ChatGptCodexStreamAdapter { inner: Box::pin(sse), pending: VecDeque::new(), seen_tool_ids: HashSet::new(), saw_tool_call: false, error: None };`
2. `fresh_adapter()` in `mod adapter_tests` (approximate lines 745-753).
3. The literal inside `adapter_surfaces_top_level_error` (approximate lines 897-903).

Also update the `adapter_tests` import. Current code (approximate line 740):
```rust
        use std::collections::{HashSet, VecDeque};
```
Replace with:
```rust
        use std::collections::{HashMap, HashSet, VecDeque};
```

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/rust`:

```sh
cargo test --all-features providers::chatgpt_codex
cargo test --all-features --test chatgpt_codex
cargo test --all-features
```

Expected: all PASS, including `adapter_handles_function_call_lifecycle` and `adapter_passes_through_unknown_arg_item_id`; the `--test chatgpt_codex` fixture-replay tests (text-only fixture, unaffected) stay green; zero failures in the full suite.

- [ ] **Step 5: Add the `#[ignore]` live smoke** — create NEW file `sdks/rust/tests/chatgpt_codex_live.rs` (auto-discovered; the `#![cfg]` gate means NO `Cargo.toml` `[[test]]` entry is needed — same pattern as `tests/chatgpt_codex.rs`):

```rust
//! Live ChatGPT-backend (codex) streamed tool-call smoke — hits the real API.
//!
//! `#[ignore]` because it makes one authenticated network call. Requires a
//! valid `~/.codex/auth.json` (mint one with `codex login`); skips with a
//! message if it is missing.
//!
//! Run manually:
//!     cargo test --features chatgpt-codex --test chatgpt_codex_live -- --ignored --nocapture

#![cfg(feature = "chatgpt-codex")]

use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{ChatRequest, Message, StreamEventType, Tool, ToolSchema};
use serde_json::{json, Value};
use tokio_stream::StreamExt;

/// Read `tokens.access_token` / `tokens.account_id` from `~/.codex/auth.json`
/// (same source as `examples/chatgpt_codex_smoke.rs`). Never print the token.
fn load_codex_auth() -> Option<(String, String)> {
    let home = std::env::var("HOME").ok()?;
    let raw = std::fs::read_to_string(format!("{home}/.codex/auth.json")).ok()?;
    let auth: Value = serde_json::from_str(&raw).ok()?;
    let tokens = auth.get("tokens")?;
    let access_token = tokens.get("access_token")?.as_str()?.to_string();
    let account_id = tokens.get("account_id")?.as_str()?.to_string();
    (!access_token.is_empty() && !account_id.is_empty()).then_some((access_token, account_id))
}

#[tokio::test]
#[ignore = "live: makes a real chatgpt.com call; needs ~/.codex/auth.json"]
async fn live_codex_streamed_tool_call_ids_are_consistent() {
    let Some((access_token, account_id)) = load_codex_auth() else {
        eprintln!("~/.codex/auth.json missing or incomplete, skipping (run `codex login`)");
        return;
    };

    let provider = ChatGptCodexProvider::new(access_token, account_id, "gpt-5.5", None);
    let tools = vec![Tool {
        schema: ToolSchema {
            name: "add".to_string(),
            description: "Add two integers".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "a": {"type": "integer"},
                    "b": {"type": "integer"}
                },
                "required": ["a", "b"]
            }),
        },
        cache: false,
    }];
    // The codex body hardcodes tool_choice "auto", so force the call by prompt.
    let req = ChatRequest::builder()
        .message(Message::user(
            "You MUST call the `add` tool with a=2 and b=3. Do not answer in text.",
        ))
        .tools(tools)
        .build();

    let mut stream = provider.stream(req).await.expect("stream request failed");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream item should not fail"));
    }

    let start = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallStart)
        .expect("model should have opened a tool call");
    let call_id = start.tool_call_id.clone().expect("start carries an id");
    assert!(
        call_id.starts_with("call_"),
        "tool_call_start id should be the call_id (call_…), got {call_id:?}"
    );

    // THE regression this file guards: on the real wire, arg fragments arrive
    // keyed by the item id (fc_…); the adapter must re-key them to call_id.
    let mut args = String::new();
    for e in events
        .iter()
        .filter(|e| e.event_type == StreamEventType::ToolCallArgs)
    {
        assert_eq!(
            e.tool_call_id.as_deref(),
            Some(call_id.as_str()),
            "arg fragment id must match the tool_call_start call_id"
        );
        args.push_str(e.tool_call_args_delta.as_deref().unwrap_or(""));
    }
    let parsed: Value = serde_json::from_str(&args).expect("assembled args are valid JSON");
    assert!(parsed.is_object(), "args should be a JSON object: {parsed}");

    let end = events
        .iter()
        .find(|e| e.event_type == StreamEventType::ToolCallEnd)
        .expect("tool_call_end");
    assert_eq!(end.tool_call_id.as_deref(), Some(call_id.as_str()));
}
```

Verify it compiles and is skipped by default — from `sdks/rust`:

```sh
cargo test --all-features --test chatgpt_codex_live
```

Expected: `test result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 0 filtered out`. Do NOT run with `-- --ignored` in CI; that is a manual smoke.

- [ ] **Step 6: Format & lint** — from `sdks/rust`:

```sh
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diff complaints, zero clippy warnings.

- [ ] **Step 7: Commit**

```sh
git add sdks/rust/src/providers/chatgpt_codex.rs sdks/rust/tests/chatgpt_codex_live.rs
git commit -m "fix(chatgpt-codex): key streamed tool-call arg fragments by call_id, not wire item_id

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 15: Fix Python OpenAI streamed parallel tool calls and stream stop_reason

**Files:**
- Modify: `sdks/python/motosan_ai/providers/openai.py` (~270 lines: finish-reason map near line 26, `chat()` mapping ~184-189, `stream()` SSE loop ~217-262; all line refs approximate)
- Modify: `sdks/python/motosan_ai/_stream_collect.py` (~lines 25 and 77)
- Test: `sdks/python/tests/test_openai.py` (extend)
- Test: `sdks/python/tests/test_client_stream_collect.py` (extend)

**Interfaces:** Consumes `StreamEvent(content, done, tool_call_id=None, tool_call_name=None, tool_call_args_delta=None, event_type="text", ..., stop_reason=None)` and `StopReason` (`end_turn|max_tokens|tool_use|stop|stop_sequence|other`) from `motosan_ai/types.py`; `collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse` from `motosan_ai/_stream_collect.py`. No public API change. Port sources (read them first): TS `sdks/typescript/src/providers/openai.ts` ~400-447 (`toolBuffer` keyed by `tool_calls[].index`, close-on-index-switch) and Rust `sdks/rust/src/stream.rs` ~120-124 (stop-reason fallback).

Three defects: (1) `stream()` ignores `delta.tool_calls[].index`, so with OpenAI parallel tool calls it emits ONE anonymous `tool_call_end` and collectors keep only the LAST call; (2) the terminal `StreamEvent(done=True)` never carries `stop_reason`; (3) `collect_stream` has no tool-use fallback, so streamed tool turns collect as `end_turn` and agent loops silently terminate.

**Scope note:** this fix ports the TS reference semantics (`providers/openai.ts` — close-on-index-switch, one call open at a time), which is correct for OpenAI's actual emission pattern: parallel calls stream their fragments grouped per index. True cross-index interleaving of argument deltas is NOT re-serialized here (nor in the TS baseline); only the Rust adapter (Task 13) buffers per index and flushes calls whole. Do not claim interleaving support for Python/TS in docs or changelogs.

- [ ] **Step 1: Write the failing tests.** In `sdks/python/tests/test_openai.py`, add one import line directly ABOVE the existing `from motosan_ai.error import StreamError` (line 7): `from motosan_ai._stream_collect import collect_stream`. Then append at end of the same file:

```python
# ---------------------------------------------------------------------------
# stream: parallel tool calls
# ---------------------------------------------------------------------------


def _tool_chunk(index: int, tc_id=None, name=None, args=None) -> dict:
    fn = {}
    if name is not None:
        fn["name"] = name
    if args is not None:
        fn["arguments"] = args
    tc = {"index": index, "function": fn}
    if tc_id is not None:
        tc["id"] = tc_id
    return {"choices": [{"delta": {"tool_calls": [tc]}, "finish_reason": None}]}


@respx.mock
@pytest.mark.asyncio
async def test_openai_stream_parallel_tool_calls(provider):
    # OpenAI streams parallel tool calls sequentially by index: all fragments
    # for index 0, then all fragments for index 1, then finish_reason.
    sse = _sse_lines(
        _tool_chunk(0, tc_id="call_1", name="get_weather", args=""),
        _tool_chunk(0, args='{"city":'),
        _tool_chunk(0, args='"Taipei"}'),
        _tool_chunk(1, tc_id="call_2", name="get_time", args=""),
        _tool_chunk(1, args='{"tz":"UTC"}'),
        {"choices": [{"delta": {}, "finish_reason": "tool_calls"}]},
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )

    events = [e async for e in provider.stream(ChatRequest(messages=[Message.user("hi")]))]

    starts = [e for e in events if e.event_type == "tool_call_start"]
    assert [(e.tool_call_id, e.tool_call_name) for e in starts] == [
        ("call_1", "get_weather"),
        ("call_2", "get_time"),
    ]

    ends = [e for e in events if e.event_type == "tool_call_end"]
    assert [e.tool_call_id for e in ends] == ["call_1", "call_2"]

    # call_1 must be closed before call_2 opens (close-on-index-switch).
    assert events.index(ends[0]) < events.index(starts[1])

    args_events = [e for e in events if e.event_type == "tool_call_args"]
    assert [(e.tool_call_id, e.tool_call_args_delta) for e in args_events] == [
        ("call_1", '{"city":'),
        ("call_1", '"Taipei"}'),
        ("call_2", '{"tz":"UTC"}'),
    ]

    assert events[-1].done is True
    assert events[-1].stop_reason == StopReason.tool_use

    # Collected end-to-end (fresh stream; the respx mock replays the response):
    # both calls survive with assembled inputs and stop_reason tool_use.
    resp = await collect_stream(provider.stream(ChatRequest(messages=[Message.user("hi")])))
    assert [(tc.id, tc.name) for tc in resp.tool_calls] == [
        ("call_1", "get_weather"),
        ("call_2", "get_time"),
    ]
    assert resp.tool_calls[0].input == {"city": "Taipei"}
    assert resp.tool_calls[1].input == {"tz": "UTC"}
    assert resp.stop_reason == StopReason.tool_use
```

Append at end of `sdks/python/tests/test_client_stream_collect.py` (imports it needs are already present):

```python
@pytest.mark.asyncio
async def test_collect_infers_tool_use_when_done_lacks_stop_reason():
    events = [
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_start",
            tool_call_id="t1",
            tool_call_name="get_weather",
        ),
        StreamEvent(
            content="",
            done=False,
            event_type="tool_call_args",
            tool_call_id="t1",
            tool_call_args_delta='{"city":"Taipei"}',
        ),
        StreamEvent(content="", done=False, event_type="tool_call_end", tool_call_id="t1"),
        StreamEvent(content="", done=True),
    ]
    resp = await collect_stream(_events_to_iter(events))
    assert resp.stop_reason == StopReason.tool_use
```

- [ ] **Step 2: Run the tests, verify they FAIL.** From `sdks/python`:

```bash
uv run pytest tests/test_openai.py::test_openai_stream_parallel_tool_calls "tests/test_client_stream_collect.py::test_collect_infers_tool_use_when_done_lacks_stop_reason" -v
```

Expected: 2 failed — the first with `AssertionError: assert [None] == ['call_1', 'call_2']` (single anonymous end event), the second with `AssertionError: assert <StopReason.end_turn: 'end_turn'> == <StopReason.tool_use: 'tool_use'>`.

- [ ] **Step 3: Implement — six exact edits.** Edits 3a-3e are in `sdks/python/motosan_ai/providers/openai.py`; 3f is in `sdks/python/motosan_ai/_stream_collect.py`. Every "Current code" block below occurs exactly once in its file.

**3a** — Current code (approximate line 26):

```python
_DEFAULT_BASE_URL = "https://api.openai.com"
```

Replace with:

```python
_DEFAULT_BASE_URL = "https://api.openai.com"

_FINISH_REASON_TO_STOP = {
    "stop": StopReason.stop,
    "length": StopReason.max_tokens,
    "tool_calls": StopReason.tool_use,
}
```

**3b** — in `chat()`. Current code (approximate lines 184-189):

```python
        finish_reason = choice.get("finish_reason")
        stop_reason = {
            "stop": StopReason.stop,
            "length": StopReason.max_tokens,
            "tool_calls": StopReason.tool_use,
        }.get(finish_reason, StopReason.other)
```

Replace with:

```python
        finish_reason = choice.get("finish_reason")
        stop_reason = _FINISH_REASON_TO_STOP.get(finish_reason, StopReason.other)
```

**3c** — in `stream()`, add adapter state. Current code (approximate line 217):

```python
            async for line in resp.aiter_lines():
```

Replace with:

```python
            # Per-index tool-call tracking (mirrors TS providers/openai.ts):
            # index -> (id, name); only one tool call is open at a time.
            tool_buffer: dict[int, tuple[str, str]] = {}
            open_tool_index: int | None = None

            async for line in resp.aiter_lines():
```

**3d** — in `stream()`, flush an open tool call at `[DONE]` (defensive: provider ended without finish_reason). Current code (approximate lines 221-225):

```python
                if not data or data == "[DONE]":
                    if data == "[DONE]":
                        yield StreamEvent(content="", done=True)
                        return
                    continue
```

Replace with:

```python
                if not data or data == "[DONE]":
                    if data == "[DONE]":
                        if open_tool_index is not None:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[open_tool_index][0],
                                event_type="tool_call_end",
                            )
                            open_tool_index = None
                        yield StreamEvent(content="", done=True)
                        return
                    continue
```

**3e** — in `stream()`, the index-aware buffering + terminal stop_reason. Current code (approximate lines 237-262, the `for tc in ...` loop AND the `if choice.get("finish_reason"):` block that follows it):

```python
                    for tc in delta.get("tool_calls") or []:
                        fn = tc.get("function") or {}
                        tc_id = tc.get("id")
                        tc_name = fn.get("name")
                        tc_args = fn.get("arguments")
                        if tc_id and tc_name:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tc_id,
                                tool_call_name=tc_name,
                                event_type="tool_call_start",
                            )
                        if tc_args:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_args_delta=tc_args,
                                event_type="tool_call_args",
                            )

                    if choice.get("finish_reason"):
                        if choice.get("finish_reason") == "tool_calls":
                            yield StreamEvent(content="", done=False, event_type="tool_call_end")
                        yield StreamEvent(content="", done=True)
                        return
```

Replace with:

```python
                    for tc in delta.get("tool_calls") or []:
                        tc_index = tc.get("index")
                        if tc_index is None:
                            continue
                        fn = tc.get("function") or {}
                        tc_id = tc.get("id")
                        tc_name = fn.get("name")
                        tc_args = fn.get("arguments")
                        if tc_id and tc_name:
                            # First fragment for this index: close any open
                            # tool call from a different index, then open this one.
                            if open_tool_index is not None and open_tool_index != tc_index:
                                yield StreamEvent(
                                    content="",
                                    done=False,
                                    tool_call_id=tool_buffer[open_tool_index][0],
                                    event_type="tool_call_end",
                                )
                            tool_buffer[tc_index] = (tc_id, tc_name)
                            open_tool_index = tc_index
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tc_id,
                                tool_call_name=tc_name,
                                event_type="tool_call_start",
                            )
                        if tc_args and tc_index in tool_buffer:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[tc_index][0],
                                tool_call_args_delta=tc_args,
                                event_type="tool_call_args",
                            )

                    finish_reason = choice.get("finish_reason")
                    if finish_reason:
                        if finish_reason == "tool_calls" and open_tool_index is not None:
                            yield StreamEvent(
                                content="",
                                done=False,
                                tool_call_id=tool_buffer[open_tool_index][0],
                                event_type="tool_call_end",
                            )
                            open_tool_index = None
                        yield StreamEvent(
                            content="",
                            done=True,
                            stop_reason=_FINISH_REASON_TO_STOP.get(finish_reason, StopReason.other),
                        )
                        return
```

**3f** — `sdks/python/motosan_ai/_stream_collect.py`, two one-line-anchor changes. Current code (approximate line 25):

```python
    stop_reason = StopReason.end_turn
```

Replace with:

```python
    stop_reason: StopReason | None = None
```

Current code (approximate line 77, right after the `async for` loop):

```python
    return ChatResponse(
```

Replace with (mirrors Rust `stream.rs` fallback):

```python
    if stop_reason is None:
        stop_reason = StopReason.tool_use if tool_calls else StopReason.end_turn

    return ChatResponse(
```

- [ ] **Step 4: Run the tests + touched suites.** From `sdks/python`:

```bash
uv run pytest tests/test_openai.py tests/test_client_stream_collect.py -v
uv run pytest tests/
```

Expected: first command 22 passed (incl. the 2 new tests); full suite ~673 passed, ~32 skipped (integration tests self-skip without env vars). Pre-existing `test_openai_stream_tool_use` and `test_collect_default_stop_reason_when_done_lacks_one` must still pass unchanged.

- [ ] **Step 5: Format & lint.** From `sdks/python`:

```bash
uv run ruff format motosan_ai/providers/openai.py motosan_ai/_stream_collect.py tests/test_openai.py tests/test_client_stream_collect.py
uv run ruff check motosan_ai/
```

Expected: "4 files left unchanged" (the code above is already formatter-clean) and "All checks passed!".

- [ ] **Step 6: Commit.**

```bash
git add sdks/python/motosan_ai/providers/openai.py sdks/python/motosan_ai/_stream_collect.py sdks/python/tests/test_openai.py sdks/python/tests/test_client_stream_collect.py
git commit -m "fix(python): index-aware OpenAI streamed tool calls + tool_use stop_reason fallback"
```

### Task 16: Fix Python chatgpt_codex arg-delta item_id/call_id mismatch

**Files:**
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py` (state dataclass approx lines 33-37; event branches approx lines 74-104)
- Test: `sdks/python/tests/test_chatgpt_codex_stream.py` (EXTEND; also replace the masking fixture at approx lines 125-167)
- Test: `sdks/python/tests/test_chatgpt_codex_http.py` (replace the masking fixture at approx lines 118-144)

**Interfaces:** Consumes module-private `_parse_sse_event(data: str, state: _ChatGptCodexAdapterState) -> list[StreamEvent]` and `_ChatGptCodexAdapterState` (same file), plus `StreamEvent(content, done, event_type, tool_call_id, tool_call_name, tool_call_args_delta)` from `motosan_ai.types`. Produces: NO public API change — only a new private field `item_to_call_id: dict[str, str]` on the underscore-private state dataclass.

**Context:** On the real ChatGPT-backend Responses wire, a `function_call` output item carries both an item id (`"fc_…"`, field `item.id`) and a `call_id` (`"call_…"`). The adapter emits `tool_call_start`/`tool_call_end` keyed by `call_id`, but `response.function_call_arguments.delta` frames are keyed by the wire `item_id` — so the emitted `tool_call_args` events carry `"fc_…"` and orphan for any consumer correlating by id. Existing fixtures use `item_id == call_id`, masking the bug. Note: `motosan_ai/_stream_collect.py` assembles args positionally (it ignores ids on `tool_call_args` events), so only the stream-level test below can fail pre-fix; the chat-level fixture update is realism, not the failing signal.

- [ ] **Step 1: Write the failing test** — In `sdks/python/tests/test_chatgpt_codex_stream.py`, replace the entire existing `test_adapter_handles_function_call_lifecycle` (approx lines 125-167; its fixture currently uses `"call_42"` for both ids) with the version below, and append the new `test_args_delta_for_unknown_item_id_passes_through` after it:

```python
def test_adapter_handles_function_call_lifecycle():
    # Real wire: the item carries both an item id ("fc_…") and a call_id
    # ("call_…"); argument fragments are keyed by the ITEM id. All emitted
    # events must use the call_id.
    events = _drive(
        [
            {
                "type": "response.output_item.added",
                "item": {
                    "type": "function_call",
                    "id": "fc_42",
                    "call_id": "call_42",
                    "name": "get_weather",
                },
            },
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_42",
                "delta": '{"city":',
            },
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_42",
                "delta": '"Paris"}',
            },
            {
                "type": "response.output_item.done",
                "item": {
                    "type": "function_call",
                    "id": "fc_42",
                    "call_id": "call_42",
                    "name": "get_weather",
                },
            },
            {
                "type": "response.completed",
                "response": {
                    "status": "completed",
                    "usage": {"input_tokens": 3, "output_tokens": 7},
                },
            },
        ]
    )

    start = next(e for e in events if e.event_type == "tool_call_start")
    assert start.tool_call_id == "call_42"
    assert start.tool_call_name == "get_weather"

    arg_events = [e for e in events if e.event_type == "tool_call_args"]
    assert [e.tool_call_id for e in arg_events] == ["call_42", "call_42"]
    assert "".join(e.tool_call_args_delta or "" for e in arg_events) == '{"city":"Paris"}'

    end = next(e for e in events if e.event_type == "tool_call_end")
    assert end.tool_call_id == "call_42"

    done = next(e for e in events if e.done)
    assert done.stop_reason == StopReason.tool_use


def test_args_delta_for_unknown_item_id_passes_through():
    state = _ChatGptCodexAdapterState()
    events = _parse_sse_event(
        json.dumps(
            {
                "type": "response.function_call_arguments.delta",
                "item_id": "fc_orphan",
                "delta": "{}",
            }
        ),
        state,
    )
    assert len(events) == 1
    assert events[0].event_type == "tool_call_args"
    assert events[0].tool_call_id == "fc_orphan"
    assert events[0].tool_call_args_delta == "{}"
```

Also in `sdks/python/tests/test_chatgpt_codex_http.py`, replace the entire existing `test_chat_tool_call_lifecycle_yields_tool_call` (approx lines 118-144; its fixture currently uses `"c1"` for both ids) with:

```python
@respx.mock
@pytest.mark.asyncio
async def test_chat_tool_call_lifecycle_yields_tool_call():
    # Distinct item id ("fc_…") vs call_id ("call_…"), matching the real wire.
    sse = _sse_text(
        {
            "type": "response.output_item.added",
            "item": {
                "type": "function_call",
                "id": "fc_001",
                "call_id": "call_001",
                "name": "get_weather",
            },
        },
        {
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_001",
            "delta": '{"city":',
        },
        {
            "type": "response.function_call_arguments.delta",
            "item_id": "fc_001",
            "delta": '"Paris"}',
        },
        {
            "type": "response.output_item.done",
            "item": {
                "type": "function_call",
                "id": "fc_001",
                "call_id": "call_001",
                "name": "get_weather",
            },
        },
        {"type": "response.completed", "response": {"status": "completed"}},
    )
    respx.post(_URL).mock(
        return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
    )
    resp = await ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None).chat(
        ChatRequest(messages=[Message.user("weather?")])
    )
    assert resp.stop_reason == StopReason.tool_use
    assert len(resp.tool_calls) == 1
    assert resp.tool_calls[0].id == "call_001"
    assert resp.tool_calls[0].name == "get_weather"
    assert resp.tool_calls[0].input == {"city": "Paris"}
```

- [ ] **Step 2: Run the tests, verify the stream-level one FAILS** — from `sdks/python`:

```bash
uv run pytest tests/test_chatgpt_codex_stream.py tests/test_chatgpt_codex_http.py -v
```

Expected: exactly one failure —
`FAILED tests/test_chatgpt_codex_stream.py::test_adapter_handles_function_call_lifecycle - AssertionError: assert ['fc_42', 'fc_42'] == ['call_42', 'call_42']`.
The other two changed/new tests PASS even pre-fix (unknown-id pass-through already holds, and `collect_stream` assembles args positionally) — that is expected; do not "fix" them.

- [ ] **Step 3: Implement** — in `sdks/python/motosan_ai/providers/chatgpt_codex.py`.

Current code (approximate lines 33-37):

```python
@dataclass
class _ChatGptCodexAdapterState:
    seen_tool_ids: set[str] = field(default_factory=set)
    saw_tool_call: bool = False
    error: str | None = None
```

Replace with:

```python
@dataclass
class _ChatGptCodexAdapterState:
    seen_tool_ids: set[str] = field(default_factory=set)
    item_to_call_id: dict[str, str] = field(default_factory=dict)
    saw_tool_call: bool = False
    error: str | None = None
```

Current code (approximate lines 74-104):

```python
    elif event_type == "response.output_item.added":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            name = item.get("name") or ""
            if call_id:
                state.saw_tool_call = True
                state.seen_tool_ids.add(call_id)
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_start",
                        tool_call_id=call_id,
                        tool_call_name=name,
                    )
                )

    elif event_type == "response.function_call_arguments.delta":
        item_id = chunk.get("item_id") or ""
        delta = chunk.get("delta")
        if item_id and isinstance(delta, str):
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    tool_call_id=item_id,
                    tool_call_args_delta=delta,
                )
            )
```

Replace with:

```python
    elif event_type == "response.output_item.added":
        item = chunk.get("item")
        if isinstance(item, dict) and item.get("type") == "function_call":
            call_id = item.get("call_id") or ""
            name = item.get("name") or ""
            if call_id:
                state.saw_tool_call = True
                state.seen_tool_ids.add(call_id)
                item_id = item.get("id")
                if isinstance(item_id, str) and item_id:
                    state.item_to_call_id[item_id] = call_id
                out.append(
                    StreamEvent(
                        content="",
                        done=False,
                        event_type="tool_call_start",
                        tool_call_id=call_id,
                        tool_call_name=name,
                    )
                )

    elif event_type == "response.function_call_arguments.delta":
        item_id = chunk.get("item_id") or ""
        delta = chunk.get("delta")
        if item_id and isinstance(delta, str):
            out.append(
                StreamEvent(
                    content="",
                    done=False,
                    event_type="tool_call_args",
                    # Wire fragments are keyed by the item's "fc_…" id; translate
                    # to the "call_…" call_id announced in output_item.added so
                    # consumers can correlate. Unknown ids pass through unchanged.
                    tool_call_id=state.item_to_call_id.get(item_id, item_id),
                    tool_call_args_delta=delta,
                )
            )
```

- [ ] **Step 4: Run the tests + the Python suite** — from `sdks/python`:

```bash
uv run pytest tests/test_chatgpt_codex_stream.py tests/test_chatgpt_codex_http.py -v
uv run pytest
```

Expected: first command shows 24 passed (15 in the stream file, 9 in the http file), zero failures; second command shows the full suite passing with no new failures.

- [ ] **Step 5: Format & lint** — from `sdks/python`:

```bash
uv run ruff format motosan_ai/ tests/
uv run ruff check motosan_ai/
```

Expected: format reports files unchanged (or reformats only the files you touched); check reports `All checks passed!`.

- [ ] **Step 6: Commit**

```bash
git add sdks/python/motosan_ai/providers/chatgpt_codex.py sdks/python/tests/test_chatgpt_codex_stream.py sdks/python/tests/test_chatgpt_codex_http.py
git commit -m "fix(chatgpt-codex): map wire item_id to call_id for streamed tool args in Python"
```

### Task 17: Fix TS chatgpt_codex item_id/call_id mismatch in argument deltas

> **Ordering:** Execute AFTER "Stop swallowing chatgpt_codex error/response.failed frames (TS)" — both edit `providers/chatgpt_codex.ts` and the same test file; this section's line refs assume that task already landed.

**Files:**
- Modify: `sdks/typescript/src/providers/chatgpt_codex.ts` (approximate lines 249-294)
- Test: `sdks/typescript/tests/providers-chatgpt-codex.test.ts` (extend — update two existing fixtures at approximate lines 292-315 and 444-455, add one new test)

**Interfaces:** Consumes (unchanged) from `sdks/typescript/src/stream.ts`: `toolCallStart(id: string, name: string): StreamEvent`, `toolCallArgsWithId(id: string, delta: string): StreamEvent`, `toolCallEndWithId(id: string): StreamEvent`. No public API changes.

**Context:** On the real OpenAI Responses wire, `response.output_item.added` carries BOTH `item.id` (`"fc_…"`) and `item.call_id` (`"call_…"`), but `response.function_call_arguments.delta` frames are keyed by `item_id` (`"fc_…"`) only. The adapter yields start/end events keyed by `call_id` but argument deltas keyed by the raw wire `item_id`, so per-id streaming consumers see orphaned argument fragments. Existing fixtures use identical ids, masking the bug. Fix: record `item.id → call_id` in a `Map` when the function_call item is added, translate in the delta branch, fall back to pass-through for unknown ids. Note: `collectStream` ignores the id on `tool_call_args` events, so the failing assertions are the stream-event-level `toolCallId` checks, not the `chat()` result.

- [ ] **Step 1: Write the failing test** — In `sdks/typescript/tests/providers-chatgpt-codex.test.ts`, inside `describe('ChatGptCodexProvider SSE adapter', …)`, replace the existing test `'runs the function_call lifecycle and ends with tool_use'` (approximate lines 292-315, currently using matching ids `call_42`) with the following two tests (updated lifecycle test + new fallback test):

```ts
  it('runs the function_call lifecycle and ends with tool_use', async () => {
    // Distinct ids as on the real wire: item.id "fc_001" keys the argument
    // deltas; call_id "call_001" keys start/end. Every tool event must carry
    // the call_id.
    const sse =
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"get_weather"}}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"{\\"city\\":"}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"\\"Paris\\"}"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_001","call_id":"call_001"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    const tool = events.filter((e) => e.eventType.startsWith('tool_call'))
    expect(tool[0]).toMatchObject({
      eventType: 'tool_call_start',
      toolCallId: 'call_001',
      toolCallName: 'get_weather',
    })
    expect(tool[1]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_001' })
    expect(tool[2]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_001' })
    expect(tool[3]).toMatchObject({ eventType: 'tool_call_end', toolCallId: 'call_001' })
    const argText = tool
      .filter((e) => e.eventType === 'tool_call_args')
      .map((e) => e.toolCallArgsDelta)
      .join('')
    expect(argText).toBe('{"city":"Paris"}')
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'tool_use' })
  })

  it('passes an unmapped item_id through on argument deltas (fallback)', async () => {
    const sse =
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_orphan","delta":"{}"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    const args = events.filter((e) => e.eventType === 'tool_call_args')
    expect(args).toHaveLength(1)
    expect(args[0]).toMatchObject({ toolCallId: 'fc_orphan', toolCallArgsDelta: '{}' })
  })
```

Then, inside `describe('ChatGptCodexProvider HTTP', …)`, replace the test `'chat() yields a tool call from the lifecycle'` (approximate lines 444-455, currently using matching ids `call_9`) with:

```ts
  it('chat() yields a tool call from the lifecycle', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_001","call_id":"call_001","name":"lookup"}}\n\n' +
        'data: {"type":"response.function_call_arguments.delta","item_id":"fc_001","delta":"{\\"q\\":\\"x\\"}"}\n\n' +
        'data: {"type":"response.output_item.done","item":{"type":"function_call","id":"fc_001","call_id":"call_001"}}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.stopReason).toBe('tool_use')
    expect(resp.toolCalls).toHaveLength(1)
    expect(resp.toolCalls[0]).toMatchObject({ id: 'call_001', name: 'lookup', input: { q: 'x' } })
  })
```

- [ ] **Step 2: Run the test, verify it FAILS** — from `sdks/typescript` (run `npm ci` first if `node_modules/` is missing):

```bash
cd sdks/typescript
npm ci
npx vitest run tests/providers-chatgpt-codex.test.ts
```

Expected: exactly 1 failure — `ChatGptCodexProvider SSE adapter > runs the function_call lifecycle and ends with tool_use` fails with an `AssertionError` on `expect(tool[1]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_001' })`: the actual event has `toolCallId: 'fc_001'`. The new fallback test and the updated `chat()` test PASS pre-fix (pass-through is current behavior, and `collectStream` takes the tool call id from the `tool_call_end` event). If anything else fails, stop and re-check the fixture strings.

- [ ] **Step 3: Implement** — in `sdks/typescript/src/providers/chatgpt_codex.ts`, two edits inside `streamImpl`.

Edit A — Current code (approximate lines 249-251):

```ts
    // Only `sawToolCall` drives the terminal stop_reason (parity with Rust/Python,
    // which also track a seen-ids set that is write-only — dropped here per plan R1).
    let sawToolCall = false
```

Replace with:

```ts
    // Only `sawToolCall` drives the terminal stop_reason (parity with Rust/Python,
    // which also track a seen-ids set that is write-only — dropped here per plan R1).
    let sawToolCall = false

    // Real wire: output_item.added carries BOTH item.id ("fc_…") and call_id
    // ("call_…"), but function_call_arguments.delta frames are keyed by item_id
    // only. Map item.id → call_id so every tool event carries the call_id.
    const itemIdToCallId = new Map<string, string>()
```

Edit B — Current code (approximate lines 272-287):

```ts
          case 'response.output_item.added': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              sawToolCall = true
              yield toolCallStart(String(item.call_id), String(item.name ?? ''))
            }
            break
          }
          case 'response.function_call_arguments.delta': {
            const itemId = data.item_id
            const delta = data.delta
            if (itemId && typeof delta === 'string') {
              yield toolCallArgsWithId(String(itemId), delta)
            }
            break
          }
```

Replace with:

```ts
          case 'response.output_item.added': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              sawToolCall = true
              if (item.id) itemIdToCallId.set(String(item.id), String(item.call_id))
              yield toolCallStart(String(item.call_id), String(item.name ?? ''))
            }
            break
          }
          case 'response.function_call_arguments.delta': {
            const itemId = data.item_id
            const delta = data.delta
            if (itemId && typeof delta === 'string') {
              const callId = itemIdToCallId.get(String(itemId)) ?? String(itemId)
              yield toolCallArgsWithId(callId, delta)
            }
            break
          }
```

Do not touch the `response.output_item.done` branch — it already keys by `item.call_id`.

- [ ] **Step 4: Run the test + the touched package test suite** — from `sdks/typescript`:

```bash
cd sdks/typescript
npx vitest run tests/providers-chatgpt-codex.test.ts
npm run build && npm test
```

(`npm test` alone fails in `tests/pack-smoke.test.ts` unless `dist/` exists, so build first.)

Expected: the chatgpt-codex file passes with 0 failures (including the previously failing lifecycle test), and the full suite passes (live/integration suites such as `integration.anthropic.test.ts` and `gemini-live.test.ts` auto-skip without API keys — skipped suites are expected, failures are not).

- [ ] **Step 5: Format & lint** — the TypeScript SDK has no prettier/eslint config; the gate is the type checker. From `sdks/typescript`:

```bash
cd sdks/typescript
npm run typecheck
```

Expected: exits 0 with no output. Match the file's existing style by hand: 2-space indent, single quotes, no semicolons.

- [ ] **Step 6: Commit**

```bash
git add sdks/typescript/src/providers/chatgpt_codex.ts sdks/typescript/tests/providers-chatgpt-codex.test.ts
git commit -m "fix(chatgpt-codex): key TS argument deltas by call_id, not wire item_id"
```


## W4 — Stream hygiene quick wins

### Task 18: Cancel the underlying reader on early stream exit in TS SSE and NDJSON parsers

**Files:**
- Modify: `sdks/typescript/src/http/sse.ts` (finally block, approx lines 89-91)
- Modify: `sdks/typescript/src/http/ndjson.ts` (finally block, approx lines 67-69)
- Test: `sdks/typescript/tests/http.sse.test.ts` (EXTEND — append inside `describe('parseSse', ...)`, after the last `it(...)` which ends around line 189)
- Test: `sdks/typescript/tests/http.ndjson.test.ts` (EXTEND — append inside `describe('parseNdjson', ...)`, after the last `it(...)` which ends around line 250)

**Interfaces:** None (self-contained). `parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseEvent>` and `parseNdjson(body: ReadableStream<Uint8Array>): AsyncGenerator<any>` signatures unchanged.

**Why:** Both parsers' `finally` blocks only call `reader.releaseLock()`. When a consumer exits the `for await` loop early (break / return / throw), the async generator's finally runs but the HTTP response body is never cancelled — the socket stays pinned for every abandoned stream. `reader.cancel()` propagates cancellation to the underlying source; on an already-fully-consumed (closed) stream it resolves harmlessly, so normal completion is unaffected.

- [ ] **Step 1: Write the failing tests.** In `sdks/typescript/tests/http.sse.test.ts`, add this test as the last `it` inside the `describe('parseSse', ...)` block (style matches neighbors: vitest, no semicolons, 2-space indent):

```ts
  it('cancels the underlying stream when the consumer exits early', async () => {
    let cancelled = false
    const input = 'event: first\ndata: {"n":1}\n\nevent: second\ndata: {"n":2}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        // Never close: simulates a long-lived HTTP body held open by the server.
      },
      cancel() {
        cancelled = true
      }
    })

    for await (const event of parseSse(stream)) {
      expect(event.data).toEqual({ n: 1 })
      break // early exit after the first event
    }

    expect(cancelled).toBe(true)
  })
```

In `sdks/typescript/tests/http.ndjson.test.ts`, add this test as the last `it` inside the `describe('parseNdjson', ...)` block:

```ts
  it('cancels the underlying stream when the consumer exits early', async () => {
    let cancelled = false
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode('{"n":1}\n{"n":2}\n'))
        // Never close: simulates a long-lived HTTP body held open by the server.
      },
      cancel() {
        cancelled = true
      }
    })

    for await (const obj of parseNdjson(stream)) {
      expect(obj).toEqual({ n: 1 })
      break // early exit after the first object
    }

    expect(cancelled).toBe(true)
  })
```

- [ ] **Step 2: Run the tests, verify they FAIL.**

```bash
cd sdks/typescript
npm ci   # first time in a fresh worktree
npx vitest run tests/http.sse.test.ts tests/http.ndjson.test.ts
```

Expected: `Tests  2 failed | 20 passed (22)`. Both new tests fail with `AssertionError: expected false to be true` at the `expect(cancelled).toBe(true)` line.

- [ ] **Step 3: Implement.** The same three-line finally block appears once in each file.

Current code in `src/http/sse.ts` (approximate lines 89-91) and `src/http/ndjson.ts` (approximate lines 67-69) — identical in both:

```ts
  } finally {
    reader.releaseLock()
  }
```

Replace with (in BOTH files):

```ts
  } finally {
    try {
      await reader.cancel()
    } catch {
      // Ignore cancel errors — the stream may already be closed or errored.
    }
    reader.releaseLock()
  }
```

- [ ] **Step 4: Run the tests + full suite, verify PASS.**

```bash
cd sdks/typescript
npx vitest run tests/http.sse.test.ts tests/http.ndjson.test.ts
```

Expected: `Tests  22 passed (22)`. Then the full suite (`npm test` alone fails in tests/pack-smoke.test.ts unless `dist/` exists, so build first):

```bash
npm run build && npm test
```

Expected: 0 failed (some integration files are skipped without API keys — that is normal).

- [ ] **Step 5: Format & lint.** This SDK has no ESLint/Prettier; the static gate is the compiler. Match the existing style (2-space indent, no semicolons) and run:

```bash
cd sdks/typescript
npm run typecheck
```

Expected: no output after the tsc command line, exit 0.

- [ ] **Step 6: Commit.**

```bash
git add sdks/typescript/src/http/sse.ts sdks/typescript/src/http/ndjson.ts sdks/typescript/tests/http.sse.test.ts sdks/typescript/tests/http.ndjson.test.ts
git commit -m "fix(typescript): cancel underlying reader on early stream exit in SSE/NDJSON parsers"
```

### Task 19: WHATWG-correct SSE line terminators and field parsing in the TS SSE parser

> **Ordering:** Execute AFTER the reader.cancel task — both edit `http/sse.ts`.

**Files:**
- Modify: `sdks/typescript/src/http/sse.ts` (locals approx lines 32-35, decode sites approx lines 56-62, `parseEventText` approx lines 98-121, header doc comment approx line 6)
- Test: `sdks/typescript/tests/http.sse.test.ts` (EXTEND — append inside `describe('parseSse', ...)`, after the last `it(...)` which ends around line 189)

**Interfaces:** None (self-contained). `parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseEvent>` signature unchanged.

**Why:** Event boundary detection is `buffer.indexOf('\n\n')` ONLY — a spec-valid CRLF stream (`\r\n\r\n` separators) yields ZERO events mid-stream. Also `parseEventText` trims each line and strips arbitrary whitespace after `data:`; the WHATWG SSE spec says remove at most ONE leading space after the colon and preserve the rest verbatim. Fix: normalize `\r\n` and bare `\r` to `\n` in the buffer (holding back a chunk-trailing `\r` so a `\r\n` pair split across chunks isn't misread), and parse fields as name + at-most-one-space + verbatim value.

- [ ] **Step 1: Write the failing tests.** Add these five tests as the last `it`s inside `describe('parseSse', ...)` in `sdks/typescript/tests/http.sse.test.ts`. The first three FAIL on current code; the last two are regression guards that already pass (JSON.parse tolerates outer whitespace, so verbatim preservation is only observable as a guard):

```ts
  it('parses CRLF line endings and CRLF CRLF event separators (WHATWG spec)', async () => {
    // Identical payload to the LF multi-event test, delivered with \r\n.
    const input =
      'event: start\r\ndata: {"id":1}\r\n\r\nevent: delta\r\ndata: {"text":"hi"}\r\n\r\nevent: done\r\ndata: [DONE]\r\n\r\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[0].event).toBe('start')
    expect(events[0].data).toEqual({ id: 1 })
    expect(events[1].event).toBe('delta')
    expect(events[1].data).toEqual({ text: 'hi' })
    expect(events[2].event).toBe('done')
    expect(events[2].data).toBe('[DONE]')
  })

  it('parses bare CR line endings and CR CR event separators', async () => {
    const input = 'event: message\rdata: {"n":1}\r\r'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ n: 1 })
  })

  it('parses a CRLF pair split across chunk boundaries', async () => {
    const full = 'event: a\r\ndata: {"n":1}\r\n\r\nevent: b\r\ndata: {"n":2}\r\n\r\n'
    const splitAt = full.indexOf('\r\n\r\n') + 1 // chunk1 ends with a lone \r
    const chunk1 = full.substring(0, splitAt)
    const chunk2 = full.substring(splitAt) // starts with \n\r\n

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(2)
    expect(events[0].event).toBe('a')
    expect(events[0].data).toEqual({ n: 1 })
    expect(events[1].event).toBe('b')
    expect(events[1].data).toEqual({ n: 2 })
  })

  it('preserves data bytes verbatim after removing at most one leading space', async () => {
    const input = 'data: {"text":"  two leading spaces kept"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].data).toEqual({ text: '  two leading spaces kept' })
  })

  it('parses a data field with no space after the colon', async () => {
    const input = 'data:{"n":7}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].data).toEqual({ n: 7 })
  })
```

- [ ] **Step 2: Run the test file, verify the three CRLF/CR tests FAIL.**

```bash
cd sdks/typescript
npm ci   # first time in a fresh worktree
npx vitest run tests/http.sse.test.ts
```

Expected: `Tests  3 failed | 12 passed (15)` with `AssertionError: expected [] to have a length of 3 but got +0` (CRLF test), `expected [] to have a length of 1 but got +0` (bare CR test), and `expected [] to have a length of 2 but got +0` (split test). The two guard tests pass.

- [ ] **Step 3: Implement.** Four edits in `sdks/typescript/src/http/sse.ts`.

Edit 1 — Current code (approximate lines 32-35):

```ts
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let pendingEventName: string | undefined
```

Replace with:

```ts
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let pendingCr = false
  let pendingEventName: string | undefined

  // Normalize \r\n and bare \r line terminators to \n (WHATWG SSE spec).
  // A chunk ending in \r is held back (pendingCr) so a \r\n pair split
  // across chunk boundaries is not mistaken for two terminators.
  function normalizeNewlines(text: string): string {
    if (pendingCr) {
      text = '\r' + text
      pendingCr = false
    }
    if (text.endsWith('\r')) {
      pendingCr = true
      text = text.substring(0, text.length - 1)
    }
    return text.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  }
```

Edit 2 — Current code (approximate lines 56-62, inside the read loop):

```ts
      if (value) {
        buffer += decoder.decode(value, { stream: true })
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += decoder.decode()
```

Replace with:

```ts
      if (value) {
        buffer += normalizeNewlines(decoder.decode(value, { stream: true }))
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += normalizeNewlines(decoder.decode())
        if (pendingCr) {
          // A trailing lone \r is a line terminator too
          buffer += '\n'
          pendingCr = false
        }
```

Edit 3 — Current code (the field loop in `parseEventText`, approximate lines 103-114):

```ts
  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed) {
      continue
    }

    if (trimmed.startsWith('event:')) {
      eventName = trimmed.substring('event:'.length).trim()
    } else if (trimmed.startsWith('data:')) {
      dataLines.push(trimmed.substring('data:'.length).trim())
    }
  }
```

Replace with:

```ts
  for (const line of lines) {
    if (!line || line.startsWith(':')) {
      continue // empty line or comment
    }

    const colonIndex = line.indexOf(':')
    const field = colonIndex === -1 ? line : line.substring(0, colonIndex)
    let value = colonIndex === -1 ? '' : line.substring(colonIndex + 1)
    // WHATWG SSE spec: remove at most ONE leading space after the colon;
    // the rest of the value is preserved verbatim.
    if (value.startsWith(' ')) {
      value = value.substring(1)
    }

    if (field === 'event') {
      eventName = value
    } else if (field === 'data') {
      dataLines.push(value)
    }
  }
```

Edit 4 — keep the header doc comment truthful. Current code (approximate lines 5-6):

```ts
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete events, splits on \n\n boundaries, and parses event:/data: lines.
```

Replace with:

```ts
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete events, normalizes \r\n / bare \r to \n, splits on blank-line
 * boundaries, and parses event:/data: lines (at most one leading space is
 * removed after the colon, per the WHATWG SSE spec).
```

- [ ] **Step 4: Run the test file + full suite, verify PASS.**

```bash
cd sdks/typescript
npx vitest run tests/http.sse.test.ts
```

Expected: `Tests  15 passed (15)` — all 10 pre-existing tests (9 baseline + 1 added by Task 18) still pass. Then (`npm test` alone fails in tests/pack-smoke.test.ts unless `dist/` exists, so build first):

```bash
npm run build && npm test
```

Expected: 0 failed (integration files skip without API keys — normal).

- [ ] **Step 5: Format & lint.** No ESLint/Prettier in this SDK; the gate is the compiler. Match existing style (2-space indent, no semicolons) and run:

```bash
cd sdks/typescript
npm run typecheck
```

Expected: no output after the tsc command line, exit 0.

- [ ] **Step 6: Commit.**

```bash
git add sdks/typescript/src/http/sse.ts sdks/typescript/tests/http.sse.test.ts
git commit -m "fix(typescript): WHATWG-correct SSE line terminators and field value parsing"
```

### Task 20: Replace-semantics usage merge in TS collectStream

**Files:**
- Modify: `sdks/typescript/src/stream.ts` (the `case 'usage':` branch in `collectStream`, approximate lines 128-141)
- Test: `sdks/typescript/tests/stream.test.ts` (EXTEND — and REWRITE the existing `'sums cache tokens with lazy initialization'` test at approximate lines 182-211, whose expectations encode the bug)

**Interfaces:** Consumes `StreamEvent` / `Usage` from `sdks/typescript/src/types.ts` (`Usage = { inputTokens: number; outputTokens: number; cacheCreationInputTokens?: number; cacheReadInputTokens?: number }`). Produces `collectStream(stream: BoxStream): Promise<ChatResponse>` — signature unchanged, only the usage-merge behavior changes.

**Why:** `collectStream` SUMS usage events (`inputTokens += ...`). The Anthropic provider (`src/providers/anthropic.ts` approx lines 298-300 and 365-368) emits usage twice per stream: once from `message_start` (input tokens + a few output tokens + cache fields) and once from `message_delta`, whose output_tokens value is CUMULATIVE — so summing double-counts (100 input/5 output then 100/50 yields 200/55 instead of 100/50). The Python SDK already uses replace-with-fallback semantics; mirror it exactly. Python reference, `sdks/python/motosan_ai/_stream_collect.py` lines 58-72:

```python
        elif event.event_type == "usage" and event.usage is not None:
            usage = Usage(
                input_tokens=event.usage.input_tokens or usage.input_tokens,
                output_tokens=event.usage.output_tokens or usage.output_tokens,
                cache_creation_input_tokens=(
                    event.usage.cache_creation_input_tokens
                    if event.usage.cache_creation_input_tokens is not None
                    else usage.cache_creation_input_tokens
                ),
                cache_read_input_tokens=(
                    event.usage.cache_read_input_tokens
                    if event.usage.cache_read_input_tokens is not None
                    else usage.cache_read_input_tokens
                ),
            )
```

- [ ] **Step 1: Write the failing tests.** In `sdks/typescript/tests/stream.test.ts`, inside `describe('collectStream', ...)`, FIRST rewrite the existing test at approximate lines 182-211. Current code:

```ts
    it('sums cache tokens with lazy initialization', async () => {
```

change the title line to:

```ts
    it('keeps last-provided cache tokens (replace-with-fallback, not summed)', async () => {
```

and change its four expectations (currently `toBe(17)` / `toBe(9)` / `toBe(120)` / `toBe(50)`) to:

```ts
      expect(response.usage.inputTokens).toBe(2)
      expect(response.usage.outputTokens).toBe(1)
      expect(response.usage.cacheCreationInputTokens).toBe(20)
      expect(response.usage.cacheReadInputTokens).toBe(50)
```

(the event fixtures — three `usageEvent`s of {10,5,cc:100}, {5,3,cr:50}, {2,1,cc:20} — stay unchanged). THEN add these two tests directly after it, still inside `describe('collectStream', ...)`:

```ts
    it('replaces usage instead of summing (Anthropic cumulative message_delta)', async () => {
      // Anthropic emits usage on message_start (input tokens + a few output
      // tokens) and again on message_delta with CUMULATIVE output tokens.
      // Summing would report 200 input / 55 output.
      const events: StreamEvent[] = [
        usageEvent({ inputTokens: 100, outputTokens: 5 }),
        usageEvent({ inputTokens: 100, outputTokens: 50 }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.usage.inputTokens).toBe(100)
      expect(response.usage.outputTokens).toBe(50)
    })

    it('zero fields fall back to previous values; absent cache fields are kept', async () => {
      // message_delta usage carries no input_tokens (adapter maps it to 0)
      // and no cache fields — neither may clobber message_start values.
      const events: StreamEvent[] = [
        usageEvent({
          inputTokens: 100,
          outputTokens: 5,
          cacheCreationInputTokens: 30,
          cacheReadInputTokens: 70,
        }),
        usageEvent({ inputTokens: 0, outputTokens: 50 }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.usage.inputTokens).toBe(100)
      expect(response.usage.outputTokens).toBe(50)
      expect(response.usage.cacheCreationInputTokens).toBe(30)
      expect(response.usage.cacheReadInputTokens).toBe(70)
    })
```

Note the single-usage tests (`'accumulates text and usage for simple response (no thinking)'`, approx line 21, and the tool-call tests) stay untouched — replace semantics is identical to sum semantics for a single usage event.

- [ ] **Step 2: Run the test file, verify the three usage tests FAIL.**

```bash
cd sdks/typescript
npm ci   # first time in a fresh worktree
npx vitest run tests/stream.test.ts
```

Expected: `Tests  3 failed | 12 passed (15)` with `AssertionError: expected 17 to be 2` (rewritten cache test), `expected 200 to be 100` (cumulative test), `expected 55 to be 50` (zero-fallback test).

- [ ] **Step 3: Implement.** In `sdks/typescript/src/stream.ts` — Current code (approximate lines 128-141, inside the `switch` in `collectStream`):

```ts
      case 'usage':
        if (event.usage) {
          inputTokens += event.usage.inputTokens
          outputTokens += event.usage.outputTokens
          if (event.usage.cacheCreationInputTokens !== undefined) {
            cacheCreationInputTokens =
              (cacheCreationInputTokens ?? 0) + event.usage.cacheCreationInputTokens
          }
          if (event.usage.cacheReadInputTokens !== undefined) {
            cacheReadInputTokens =
              (cacheReadInputTokens ?? 0) + event.usage.cacheReadInputTokens
          }
        }
        break
```

Replace with:

```ts
      case 'usage':
        if (event.usage) {
          // Replace-with-fallback, mirroring Python _stream_collect.py:
          // Anthropic message_delta usage is CUMULATIVE, so summing would
          // double-count output tokens. A zero field keeps the previous
          // value (message_delta carries no input_tokens, mapped to 0);
          // an absent cache field keeps the previous cache value.
          inputTokens = event.usage.inputTokens || inputTokens
          outputTokens = event.usage.outputTokens || outputTokens
          if (event.usage.cacheCreationInputTokens !== undefined) {
            cacheCreationInputTokens = event.usage.cacheCreationInputTokens
          }
          if (event.usage.cacheReadInputTokens !== undefined) {
            cacheReadInputTokens = event.usage.cacheReadInputTokens
          }
        }
        break
```

(`||` deliberately mirrors Python's `or` — a 0 falls back to the previous value; `!== undefined` mirrors `is not None` for the optional cache fields.)

- [ ] **Step 4: Run the test file + full suite, verify PASS.**

```bash
cd sdks/typescript
npx vitest run tests/stream.test.ts
```

Expected: `Tests  14 passed (14)`. Then (`npm test` alone fails in tests/pack-smoke.test.ts unless `dist/` exists, so build first):

```bash
npm run build && npm test
```

Expected: 0 failed (integration files skip without API keys — normal).

- [ ] **Step 5: Format & lint.** No ESLint/Prettier in this SDK; the gate is the compiler. Match existing style (2-space indent, no semicolons) and run:

```bash
cd sdks/typescript
npm run typecheck
```

Expected: no output after the tsc command line, exit 0.

- [ ] **Step 6: Commit.**

```bash
git add sdks/typescript/src/stream.ts sdks/typescript/tests/stream.test.ts
git commit -m "fix(typescript): replace-semantics usage merge in collectStream"
```

### Task 21: Fix cumulative-usage double counting in Rust collect_stream

**Files:**
- Modify: `sdks/rust/src/stream.rs` (the `StreamEventType::Usage` match arm, approximate lines 74-85)
- Test: `sdks/rust/tests/collect_stream.rs` (EXTEND — append after the last test, approximate line 428)

**Interfaces:** Consumes `motosan_ai::types::Usage { input_tokens: u32, output_tokens: u32, cache_creation_input_tokens: Option<u32>, cache_read_input_tokens: Option<u32> }` and `StreamEvent::usage(usage: Usage) -> StreamEvent`. Produces no signature changes — `pub async fn collect_stream(stream: BoxStream) -> Result<ChatResponse, MotosanError>` is unchanged; only the usage-merge behavior inside it changes.

**Context:** Anthropic streams emit usage twice: once on `message_start` (real `input_tokens`, tiny `output_tokens`) and once on `message_delta` (CUMULATIVE `output_tokens` for the whole message, `input_tokens: 0`). The current Rust code sums the two, double-counting output tokens (billing-visible). The Python SDK (`sdks/python/motosan_ai/_stream_collect.py` lines 58-72) already has the correct semantics: last-writer-wins per field, where a later `0` does NOT clobber an earlier non-zero `input_tokens`/`output_tokens` (Python `or`), and cache fields replace whenever the event reports them, even as `Some(0)` (Python `is not None`). Mirror those rules exactly. OpenAI-style streams (one final usage event) must keep working.

- [ ] **Step 1: Write the failing tests** — Append the following at the END of `sdks/rust/tests/collect_stream.rs` (after `anthropic_stream_emits_max_tokens_stop_reason`, approximate line 428). The file already has `boxed_stream(...)` and the needed imports at the top; add nothing else.

```rust

// ---------------------------------------------------------------------------
// Usage merge semantics: last-writer-wins per field, never summed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn collect_stream_usage_is_last_writer_wins_not_summed() {
    // Anthropic emits usage on message_start AND cumulative usage on
    // message_delta. Summing the two double-counts (200 input / 55 output).
    let events = vec![
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 100,
            output_tokens: 5,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::text("hi"),
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
        }),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await.unwrap();

    assert_eq!(response.usage.input_tokens, 100, "must not sum to 200");
    assert_eq!(response.usage.output_tokens, 50, "must not sum to 55");
}

#[tokio::test]
async fn collect_stream_usage_single_event_unchanged() {
    // OpenAI-style: a single final usage event carries the whole picture.
    let events = vec![
        StreamEvent::text("done"),
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 20,
            output_tokens: 9,
            cache_creation_input_tokens: Some(3),
            cache_read_input_tokens: Some(4),
        }),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await.unwrap();

    assert_eq!(response.usage.input_tokens, 20);
    assert_eq!(response.usage.output_tokens, 9);
    assert_eq!(response.usage.cache_creation_input_tokens, Some(3));
    assert_eq!(response.usage.cache_read_input_tokens, Some(4));
}

#[tokio::test]
async fn collect_stream_usage_zero_does_not_clobber_and_cache_replaces() {
    // Mirrors the Python SDK rules (_stream_collect.py): a later 0 keeps the
    // earlier non-zero input/output count; cache fields replace whenever the
    // event reports them (last writer wins, not summed).
    let events = vec![
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 100,
            output_tokens: 1,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: Some(11),
        }),
        StreamEvent::usage(motosan_ai::Usage {
            input_tokens: 0,
            output_tokens: 50,
            cache_creation_input_tokens: Some(7),
            cache_read_input_tokens: Some(11),
        }),
        StreamEvent::done(),
    ];
    let response = collect_stream(boxed_stream(events)).await.unwrap();

    assert_eq!(response.usage.input_tokens, 100, "later 0 must not clobber");
    assert_eq!(response.usage.output_tokens, 50);
    assert_eq!(
        response.usage.cache_creation_input_tokens,
        Some(7),
        "must not sum to 14"
    );
    assert_eq!(
        response.usage.cache_read_input_tokens,
        Some(11),
        "must not sum to 22"
    );
}
```

- [ ] **Step 2: Run the tests, verify they FAIL** — from `sdks/rust`:

```bash
cargo test --all-features --test collect_stream usage
```

Expected: `test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 14 filtered out`. The two failures are `collect_stream_usage_is_last_writer_wins_not_summed` (panics with ``assertion `left == right` failed: must not sum to 200 / left: 200 / right: 100``) and `collect_stream_usage_zero_does_not_clobber_and_cache_replaces` (panics with `left: 51 / right: 50`). The two passes are expected: `collect_stream_with_usage_events` (pre-existing) and `collect_stream_usage_single_event_unchanged` (a regression guard that must pass both before and after the fix — do not be alarmed that it is green at this step).

- [ ] **Step 3: Implement** — in `sdks/rust/src/stream.rs`, inside `collect_stream`'s `match event.event_type` block.

Current code (approximate lines 74-85):

```rust
            StreamEventType::Usage => {
                if let Some(ref usage) = event.usage {
                    input_tokens += usage.input_tokens;
                    output_tokens += usage.output_tokens;
                    if let Some(v) = usage.cache_creation_input_tokens {
                        *cache_creation_input_tokens.get_or_insert(0) += v;
                    }
                    if let Some(v) = usage.cache_read_input_tokens {
                        *cache_read_input_tokens.get_or_insert(0) += v;
                    }
                }
            }
```

Replace with:

```rust
            StreamEventType::Usage => {
                if let Some(ref usage) = event.usage {
                    // Last-writer-wins per field, mirroring the Python SDK
                    // (_stream_collect.py). Anthropic emits usage on
                    // message_start AND cumulative usage on message_delta,
                    // so summing double-counts tokens. A later 0 does not
                    // clobber an earlier non-zero count (Anthropic's
                    // message_delta reports input_tokens as 0); cache
                    // fields replace whenever the event reports them.
                    if usage.input_tokens != 0 {
                        input_tokens = usage.input_tokens;
                    }
                    if usage.output_tokens != 0 {
                        output_tokens = usage.output_tokens;
                    }
                    if let Some(v) = usage.cache_creation_input_tokens {
                        cache_creation_input_tokens = Some(v);
                    }
                    if let Some(v) = usage.cache_read_input_tokens {
                        cache_read_input_tokens = Some(v);
                    }
                }
            }
```

Do not change anything else in the file. The `Some(0)` cache value intentionally replaces a previous value while a `0` input/output token count intentionally does not — that asymmetry is exact Python parity (`or` vs `is not None`).

- [ ] **Step 4: Run the test file + the package suite** — from `sdks/rust`:

```bash
cargo test --all-features --test collect_stream
cargo test --all-features
```

Expected: the first command reports `18 passed; 0 failed` for the collect_stream suite. The second (full suite) reports 0 failures overall, with ~18 ignored (live/integration tests — ignored is normal). Pre-existing tests such as `collect_stream_with_usage_events`, `oauth_chat_uses_collect_stream_internally`, `stream_then_collect_returns_chat_response`, and `chat_vs_stream_collect_consistency` all keep passing because Anthropic's `message_delta` mock bodies carry `input_tokens: 0`, which the new non-zero-wins rule correctly ignores.

- [ ] **Step 5: Format & lint** — from `sdks/rust`:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no output from fmt (or no diff), clippy finishes with zero warnings.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/stream.rs sdks/rust/tests/collect_stream.rs
git commit -m "fix(stream): stop double-counting cumulative usage in collect_stream

Anthropic emits usage on message_start and cumulative usage on
message_delta; summing the two double-counted output tokens. Merge
usage last-writer-wins per field instead, mirroring the Python SDK's
_stream_collect.py rules (a later 0 keeps an earlier non-zero token
count; cache fields replace whenever reported).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```


## Release

### Task 22: Release M1 — bump Rust 0.22.0 / Python 0.15.0 / TypeScript 0.12.0, update changelogs and doc versions

**Run this task LAST, only after every other M1 PR has merged to main.** Work on a fresh branch off the then-current `origin/main`.

**Files:**
- Modify: `sdks/rust/Cargo.toml` (version, approx line 3), `sdks/python/pyproject.toml` (version, approx line 3), `uv.lock` (REPO ROOT — uv workspace lockfile, regenerated), `sdks/typescript/package.json` (version, approx line 3), `sdks/typescript/package-lock.json` (regenerated), `CHANGELOG.md` (root, insert at approx line 5), `sdks/rust/CHANGELOG.md` (insert after `## [Unreleased]`, approx line 5), `sdks/python/CHANGELOG.md` (insert at approx line 5), `sdks/typescript/CHANGELOG.md` (insert at approx line 7), `AGENTS.md` (approx lines 5 and 9), `llms.txt` (approx lines 5, 24, 923), `skills/motosan-ai/SKILL.md` (approx lines 8, 25), `README.md` (approx lines 29–31). NOTE: the root `Cargo.lock` is gitignored (`.gitignore` line 2: `/Cargo.lock`) and is never committed — do not stage it.
- Test: none (version/docs-only change; the gate is the full Rust + Python + TypeScript suites in Step 6)

**Interfaces:** None (self-contained). No code changes — versions, changelogs, and doc version strings only.

Wherever `<DATE>` appears below, substitute the output of `date +%F` (e.g. `2026-07-14`).

- [ ] **Step 1: Preflight — verify current versions and list the M1 commits**
  ```bash
  grep -n 'version = "0.21.1"' sdks/rust/Cargo.toml        # expect: 3:version = "0.21.1"
  grep -n 'version = "0.14.0"' sdks/python/pyproject.toml   # expect: 3:version = "0.14.0"
  grep -n '"version": "0.11.0"' sdks/typescript/package.json # expect: 3:  "version": "0.11.0",
  git log --oneline 3e3f413..HEAD                            # M1 PRs merged since the plan baseline
  ```
  If any of the three greps returns nothing, that SDK was already re-released after this plan was written — STOP and report back instead of guessing versions. Keep the `git log` output visible; Step 5 checks changelog bullets against it.

- [ ] **Step 2: Bump the three version files**
  `sdks/rust/Cargo.toml` — Current code (approximate line 3):
  ```toml
  version = "0.21.1"
  ```
  Replace with:
  ```toml
  version = "0.22.0"
  ```
  `sdks/python/pyproject.toml` — Current code (approximate line 3):
  ```toml
  version = "0.14.0"
  ```
  Replace with:
  ```toml
  version = "0.15.0"
  ```
  `sdks/typescript/package.json` — Current code (approximate line 3):
  ```json
    "version": "0.11.0",
  ```
  Replace with:
  ```json
    "version": "0.12.0",
  ```

- [ ] **Step 3: Regenerate lockfiles** (from the repo root)
  ```bash
  uv lock --project sdks/python
  grep -n -A1 'name = "motosan-ai"' uv.lock | head -3   # expect: version = "0.15.0" (uv.lock lives at the REPO ROOT — sdks/python is a uv workspace member)
  cd sdks/typescript && npm install --package-lock-only && cd ../..
  grep -n '"version": "0.12.0"' sdks/typescript/package-lock.json   # expect matches on approx lines 3 and 9
  ```
  The root `Cargo.lock` is local-only — it is gitignored (`.gitignore` line 2: `/Cargo.lock`) and never committed. It updates automatically when cargo runs in Step 7, where a grep merely confirms the local build picked up the bump.

- [ ] **Step 4: Write the changelog entries**
  **Root `CHANGELOG.md`** — insert directly above `## [rust-0.18.0] — 2026-05-29` (approximate line 5):
  ```markdown
  ## [rust-0.22.0 / python-0.15.0 / ts-0.12.0] — <DATE>

  M1 reliability release — cross-SDK bug-fix pass. No new features, no public API changes.

  ### Fixed

  - **Retry on non-JSON 5xx** (Rust · Python): a 5xx response whose body is not valid JSON no longer breaks the retry loop — classification falls back to the HTTP status code, so transient server errors are retried again. (TypeScript already classified by status at baseline.)
  - **Mid-stream error frames surfaced** (Rust · Python · TypeScript): provider `error` events arriving mid-stream now surface as stream errors instead of being dropped and letting the stream end as if the turn had completed.
  - **CLI child-process death surfaced** (Rust · Python): a `claude` / `codex` / `gemini` child process dying mid-run now produces an explicit error instead of a silently truncated, seemingly successful response.
  - **Parallel tool-call index handling** (Rust · Python): OpenAI-style streamed tool calls are keyed by `tool_calls[].index`, so parallel calls are no longer dropped or merged. Rust additionally buffers argument deltas per index and flushes calls whole; TypeScript already keyed by index at baseline.
  - **chatgpt-codex `item_id` → `call_id`** (Rust · Python · TypeScript): function-call events are correlated by `item_id` and emitted with the correct `call_id`, fixing tool-call round-trips when the two ids differ.
  - **Streamed tool-call stop reason** (Python): streamed turns that emit tool calls now finish with the tool-use stop reason instead of a generic end-of-turn.
  - **Usage replace-merge** (Rust · Python · TypeScript): later usage frames replace previously seen fields instead of accumulating into double-counted totals.
  - **Stream cancel + CRLF SSE** (TypeScript): aborting a stream now cancels the underlying `ReadableStream` reader (releasing the HTTP connection), and the SSE parser accepts `\r\n` line terminators.

  Per-SDK detail: [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md), [`sdks/python/CHANGELOG.md`](sdks/python/CHANGELOG.md), [`sdks/typescript/CHANGELOG.md`](sdks/typescript/CHANGELOG.md).
  ```
  **`sdks/rust/CHANGELOG.md`** — insert between `## [Unreleased]` (approx line 5) and `## 0.21.1 — 2026-06-13` (approx line 7), keeping the `## [Unreleased]` heading in place:
  ```markdown
  ## [0.22.0] - <DATE>

  ### Fixed
  - Retry: 5xx responses with non-JSON bodies are classified by HTTP status and retried instead of aborting the retry loop.
  - Streaming: mid-stream `error` frames surface as stream errors instead of being dropped.
  - CLI providers (`claude-code` / `codex-cli` / `gemini-cli`): child-process death mid-run surfaces as an error instead of a truncated success.
  - OpenAI streaming: parallel tool calls are buffered per `tool_calls[].index` and flushed whole, so interleaved argument deltas no longer corrupt one another.
  - chatgpt-codex: function-call events are correlated by `item_id` and emitted with the correct `call_id`.
  - Streaming usage: later usage frames replace earlier fields instead of double-counting.
  ```
  **`sdks/python/CHANGELOG.md`** — insert directly above `## [0.14.0] - 2026-06-23` (approximate line 5):
  ```markdown
  ## [0.15.0] - <DATE>

  ### Fixed
  - Retry: 5xx responses with non-JSON bodies are classified by HTTP status and retried instead of aborting the retry loop.
  - Streaming: mid-stream `error` frames raise `StreamError` instead of being dropped.
  - CLI providers (`claude_code` / `codex_cli` / `gemini_cli`): child-process death mid-run surfaces as an error instead of a truncated success.
  - OpenAI streaming: parallel tool calls are keyed by `tool_calls[].index` (ports the TypeScript adapter), so parallel calls are no longer dropped or merged.
  - chatgpt-codex: function-call events are correlated by `item_id` and emitted with the correct `call_id`.
  - Streaming: turns that emit tool calls now finish with the tool-use stop reason instead of a generic end-of-turn.
  - Usage: later usage frames replace earlier fields instead of double-counting.
  ```
  **`sdks/typescript/CHANGELOG.md`** — insert directly above `## [0.11.0] - 2026-06-23` (approximate line 7):
  ```markdown
  ## [0.12.0] - <DATE>

  ### Fixed
  - Streaming: mid-stream `error` frames surface as stream errors instead of being dropped.
  - chatgpt-codex: `error` / `response.failed` frames now reject the stream instead of ending it silently.
  - Streaming: aborting a stream cancels the underlying `ReadableStream` reader, releasing the HTTP connection.
  - SSE parser accepts `\r\n` (CRLF) line terminators and strips at most one leading space after `data:` per the SSE spec.
  - chatgpt-codex: function-call events are correlated by `item_id` and emitted with the correct `call_id`.
  - Usage: later usage frames replace earlier fields instead of double-counting.
  ```

  (Do NOT list retry-status classification or OpenAI `index` keying for TypeScript — the TS baseline already handled both; those fixes are Rust/Python-only.)
  If M1 PRs already added bullets under `## [Unreleased]` in any per-SDK changelog, MOVE those bullets into the new release section (merging with the text above, no duplicates) rather than leaving them under Unreleased.

- [ ] **Step 5: Cross-check the bullets against what actually merged** — for each bullet in Step 4, find its commit in the Step 1 `git log --oneline 3e3f413..HEAD` output (commit subjects carry fix scopes like `fix(retry):`, `fix(openai):`, `fix(chatgpt-codex):`, `fix(cli):`, `fix(sse):`). Delete any bullet — and its SDK tag in the root entry — whose fix did NOT land in that SDK, and add a bullet for any merged M1 fix the lists above miss. Do not describe unmerged work.

- [ ] **Step 6: Update doc version lines**
  `AGENTS.md` — Current code (approximate line 5):
  ```markdown
  Rust v0.21.1 · Python v0.14.0 (PyPI) · TypeScript v0.11.0 (npm)
  ```
  Replace with:
  ```markdown
  Rust v0.22.0 · Python v0.15.0 (PyPI) · TypeScript v0.12.0 (npm)
  ```
  `AGENTS.md` — insert as a new paragraph after the `Python 0.14.0 and TypeScript 0.11.0 add the **chatgpt-codex** provider…` paragraph (approximate line 9), separated by blank lines:
  ```markdown
  Rust 0.22.0 / Python 0.15.0 / TypeScript 0.12.0 are the M1 reliability releases: retry survives non-JSON 5xx bodies, mid-stream error frames and CLI child-process death surface as errors, parallel tool-call `index` and chatgpt-codex `item_id`→`call_id` are handled correctly, streamed usage merges by replacement, Python streamed tool turns report the tool-use stop reason, and the TypeScript SSE reader cancels on abort and accepts CRLF.
  ```
  (Trim that sentence to match any bullets removed in Step 5.)
  `llms.txt` — Current code (approximate line 5):
  ```markdown
  - Python 0.14.0 · TypeScript 0.11.0 · Rust 0.21.1
  ```
  Replace with:
  ```markdown
  - Python 0.15.0 · TypeScript 0.12.0 · Rust 0.22.0
  ```
  `llms.txt` Install section — Current code (approximate line 24):
  ```toml
  motosan-ai = { version = "0.20.0", features = ["anthropic"] }
  ```
  Replace with:
  ```toml
  motosan-ai = { version = "0.22.0", features = ["anthropic"] }
  ```
  `README.md` Install example — Current code (approximate line 38; it has drifted several releases):
  ```toml
  motosan-ai = { version = "0.18.0", features = ["anthropic"] }
  ```
  Replace with:
  ```toml
  motosan-ai = { version = "0.22.0", features = ["anthropic"] }
  ```
  `llms.txt` Tag Convention table — Current code (approximate line 923):
  ```markdown
  | TypeScript   | `ts-vX.Y.Z`           | `ts-v0.11.0`          |
  ```
  Replace with:
  ```markdown
  | TypeScript   | `ts-vX.Y.Z`           | `ts-v0.12.0`          |
  ```
  `skills/motosan-ai/SKILL.md` — Current code (approximate line 8; note it already lags at "Rust 0.20.0"):
  ```markdown
  Multi-provider LLM SDK — Python 0.14.0 / Rust 0.20.0 / TypeScript 0.11.0
  ```
  Replace with:
  ```markdown
  Multi-provider LLM SDK — Python 0.15.0 / Rust 0.22.0 / TypeScript 0.12.0
  ```
  `skills/motosan-ai/SKILL.md` Install section — Current code (approximate line 25):
  ```toml
  motosan-ai = { version = "0.20.0", features = ["anthropic"] }
  ```
  Replace with:
  ```toml
  motosan-ai = { version = "0.22.0", features = ["anthropic"] }
  ```
  `README.md` Languages table — Current code (approximate lines 29–31; baseline values are several releases stale):
  ```markdown
  | Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v0.18.0 |
  | Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v0.12.1 |
  | TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) | v0.10.0 |
  ```
  Replace with:
  ```markdown
  | Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v0.22.0 |
  | Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v0.15.0 |
  | TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) | v0.12.0 |
  ```
  Leave historical mentions alone (e.g. `llms.txt` lines ~7/208/598/610/686/833, `AGENTS.md` line ~9, README "since v0.11.0" comments) — they describe past releases.

- [ ] **Step 7: Run the full gate** — inside `nix develop` run `check-all` (expect final line `=== All checks passed ===`); without nix run the equivalents:
  ```bash
  cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration && cd ../..   # ruff check scope is motosan_ai/ only, mirroring devshell/scripts.nix + CI (tests/ has pre-existing findings)
  cd sdks/rust && cargo fmt && cargo clippy --all-features -- -D warnings && cargo test --all-features && cd ../..
  cd sdks/typescript && npm run typecheck && npm run build && npm test && cd ../..   # check-all does NOT cover TS; pack-smoke.test.ts requires dist/, so build before the full suite
  ```
  Expect: pytest all passed, clippy clean, cargo test all passed, vitest `Test Files … passed`. Then confirm the local build picked up the bump: `grep -n -A1 'name = "motosan-ai"' Cargo.lock | head -3` → `version = "0.22.0"` (if unchanged, run `cargo check` from `sdks/rust` and re-grep). This is a local sanity check only — `Cargo.lock` is gitignored (`.gitignore` line 2: `/Cargo.lock`) and is NOT committed.

- [ ] **Step 8: Commit**
  ```bash
  git add sdks/rust/Cargo.toml sdks/python/pyproject.toml uv.lock \
    sdks/typescript/package.json sdks/typescript/package-lock.json \
    CHANGELOG.md sdks/rust/CHANGELOG.md sdks/python/CHANGELOG.md sdks/typescript/CHANGELOG.md \
    AGENTS.md llms.txt skills/motosan-ai/SKILL.md README.md
  git commit -m "chore(release): rust-v0.22.0 / python-v0.15.0 / ts-v0.12.0 — M1 reliability fixes"
  ```
  Do NOT `git add Cargo.lock` — it is gitignored and `git add` would fail with "The following paths are ignored by one of your .gitignore files".

- [ ] **Step 9: Hand off publishing — do NOT tag or publish in this task.** Per `llms.txt` § Release, after this change reaches `main` the MAINTAINER creates and pushes the tags, and each tag push triggers CI publishing: `rust-v0.22.0` → `publish-rust.yml` → crates.io; `python-v0.15.0` → `publish-python.yml` → PyPI; `ts-v0.12.0` → `publish-typescript.yml` → npm (annotated tags for Rust/Python, plain tag for TS, per the llms.txt examples). State this handoff in your completion report.

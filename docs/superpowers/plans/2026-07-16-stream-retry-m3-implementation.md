# M3 — Stream Termination Contract & Timeout/Lifecycle: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make stream truncation distinguishable from completion (typed `IncompleteStream` on EOF-without-terminal-event — retiring the v0.10.1 "exactly one done" invariant), give all three SDKs one timeout model (connect 10s / read-idle 120s / total opt-in), and fix client lifecycle (Rust build-once providers + shared reqwest client, TS AbortSignal cancellation + `CancelledError`, Python `aclose()` + context manager) — audit ranks 5+6, the M3 milestone.

**Architecture:** Spec-first (specs/types.md termination contract + specs/retry.md cancellation amendment), then per-SDK enforcement AT THE ADAPTERS (collectors keep propagating; the stop_reason heuristic survives only for a real `done` lacking a reason). BREAKING and three-SDK-coordinated: Rust 0.24.0 / Python 0.17.0 / TS 0.14.0 in one release; Python/TS soften migration by subclassing `IncompleteStreamError` under `StreamError`. The M2 conformance suites pin the specs — every retired behavior flips its pinned tests in the same PR group, never letting CI drift.

**Tech Stack:** Rust (reqwest builder connect_timeout, tokio idle deadlines, mockito) · Python 3.11+ (httpx.Timeout, asyncio, respx) · TypeScript (AbortSignal.timeout/any, vitest).

## Global Constraints

- **Baseline:** authored 2026-07-16 against `origin/main` @ `acf5d7f` (post-M2: 0.23.0/0.16.0/0.13.0 shipped; `send_with_retry`, structured error metadata, `specs/retry.md` + three conformance suites all exist). ALL line refs approximate; execute in a worktree off CURRENT `origin/main` and ground every edit in real files.
- **Locked design (E1–E9):** error names `IncompleteStream` (Rust variant) / `IncompleteStreamError` (Py/TS, subclass of StreamError) / `CancelledError` (TS) / `StreamReadTimeoutError` (Py, mirrors existing Rust/TS); enforcement at ADAPTERS; timeout defaults **connect 10s / read-idle 120s / total None** (total = chat-only, never streams); Rust providers built ONCE in `build()` sharing one `reqwest::Client`; TS caller-signal abort → `CancelledError`, never retried (fetch-internal AbortError stays retryable); the v0.10.1 "exactly one done even on truncated EOF" invariant is deliberately retired. Deviation = wrong even if it compiles.
- **Pinned-behavior flips are explicit:** any task retiring pinned behavior lists every flipped test file:name and updates spec + conformance in the same PR group. Everything not explicitly flipped (M1 retry tests, M2 conformance) MUST pass unchanged; editing tests to make them pass remains unacceptable.
- **Breaking budget:** Rust `MotosanError` gains a variant + stream EOF semantics (0.24.0, changelog migration section); Python stream truncation now raises (0.17.0, softened by subclassing); TS same + cancellation (0.14.0). Python `LlmClient` Protocol stays additive-only.
- **House workflow:** every `.rs`/`Cargo.toml` change via PR + CI; Rust clippy gate is `cargo clippy --all-features --all-targets -- -D warnings` (`--all-targets` mandatory); fresh Python worktrees run `uv sync --all-extras` before first push; TS full suite = `npm run build && npm test`.
- **Commands** — Rust (from `sdks/rust`): `cargo test --all-features …`, `cargo fmt`, clippy as above. Python (from `sdks/python`): `uv run pytest tests/… -v`, `uv run ruff check motosan_ai/`, `uv run ruff format`. TypeScript (from `sdks/typescript`): `npx vitest run tests/…`, `npm run typecheck`.
- **Commits:** conventional style + `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- **Ordering:** Task 1 (spec) first; Rust 2→3→4; Python 5→6; TS 7→8; Task 9 (conformance) after all SDK tasks; Task 10 (release) last.
- **Blessed E4 narrowing:** read-idle deadlines guard **streaming reads only**; non-streaming `chat()` body reads are bounded by `connect_timeout` + the opt-in `total_timeout` (reqwest/fetch cannot cheaply idle-bound a buffered body read). Recorded here so no task "fixes" it back.
- **Length-cap waiver:** Tasks 7, 8, and 9 exceed the ~280-line guidance deliberately — they carry complete test files per the no-placeholder rule. Executors must not trim them.

## Suggested PR grouping

| PR | Tasks | Scope |
|---|---|---|
| PR-S | 1 | specs amendment (docs-only) |
| PR-R | 2, 3, 4 | Rust: IncompleteStream + EOF, build-once + shared client, timeout model (sequential) |
| PR-P | 5, 6 | Python: IncompleteStreamError + EOF, timeouts + lifecycle (sequential) |
| PR-T | 7, 8 | TS: IncompleteStreamError + EOF, timeouts + cancellation (sequential) |
| PR-C | 9 | kill-connection + hung-stream conformance ×3 (after PR-R/P/T) |
| PR-REL | 10 | Release 0.24.0 / 0.17.0 / 0.14.0 (last) |

---


## S — Spec first

### Task 1: Amend specs/types.md and specs/retry.md: stream termination contract, cancellation, and read-idle retry rules

> **This task lands FIRST in M3.** It is **docs-only — no test cycle** (no failing test, no test run steps). The M2 conformance suites pin these specs; the suites are NOT touched here. Every conformance-suite update happens inside the SDK task that changes the pinned behavior (E3 invariant flips, E6 CancelledError classification), in the same PR group as that behavior change — never in this task.
>
> **MiniMax terminal-event note (verified against real code):** MiniMax is NOT uniformly OpenAI-wire post-M2. Rust routes `Provider::Minimax` through `AnthropicProvider` (`build_minimax_provider`, `sdks/rust/src/client.rs` ~611–631) and TypeScript `MinimaxProvider` is a thin `AnthropicProvider` delegate (`sdks/typescript/src/providers/minimax.ts` ~23–51) — in both, the terminal event is `message_stop`. Only Python has its own OpenAI-compatible-wire MiniMax adapter with a `data: [DONE]` terminal (`sdks/python/motosan_ai/providers/minimax.py` ~244–252). The terminal-event table below splits the MiniMax row per SDK accordingly; do not "simplify" it back to one wire format.

**Files:**
- `specs/types.md` — StreamEvent table `done` row (~line 115); insertion point between `## StreamEventType` (~line 123) and `## MotosanError (Rust)` (~line 127); MotosanError variant list (~line 129)
- `specs/retry.md` — Classification table (~lines 21–28); transport-error table block (~lines 44–50); `## Streaming` section (~lines 108–118)

**Interfaces:** Produces the normative contract the M3 SDK tasks implement (spellings verbatim from the locked design): Rust `MotosanError::IncompleteStream(String)` with `#[error("incomplete stream: {0}")]` (breaking, ships 0.24.0); Python `class IncompleteStreamError(StreamError)`; TS `export class IncompleteStreamError extends StreamError`; TS `export class CancelledError extends MotosanError`; message convention `incomplete stream: <provider> ended without a terminal event`. Consumes: nothing (self-contained docs change).

**Steps:**

- [ ] 1. Read the current anchors to confirm line drift: `specs/types.md` ~lines 110–131 and `specs/retry.md` ~lines 12–50 and ~108–118. All "Current text" blocks below are from origin/main @ acf5d7f; if they have drifted, match on the quoted text, not the line numbers.

- [ ] 2. **Edit `specs/types.md` — three edits.**

  **(a)** In the `## StreamEvent` table, current text (approximate line 115):

  ```
  | `done` | `bool` | Exactly one terminal event per stream |
  ```

  Replace with:

  ```
  | `done` | `bool` | Exactly one terminal event per *successfully completed* stream — see [Stream termination contract](#stream-termination-contract) |
  ```

  **(b)** Insert a new section between the `## StreamEventType` block (~lines 123–125) and `## MotosanError (Rust)` (~line 127). Insert this text in full, verbatim:

  ```
  ## Stream termination contract

  Every provider defines a **terminal event** that marks the successful
  end of a stream:

  | Provider family | Terminal event |
  |-----------------|----------------|
  | OpenAI | `data: [DONE]` SSE sentinel |
  | MiniMax | Python: `data: [DONE]` SSE sentinel (own OpenAI-compatible-wire adapter). Rust / TypeScript: `message_stop` — both delegate to the Anthropic adapter (Rust `build_minimax_provider` constructs an `AnthropicProvider`; TS `MinimaxProvider` wraps one), so the Anthropic rule applies |
  | Anthropic | `message_stop` SSE event (the Python adapter additionally treats a stray `data: [DONE]` as terminal) |
  | Gemini, GeminiCodeAssist | final SSE chunk carrying `finishReason` (a trailing `[DONE]` is tolerated but not required) |
  | ChatGPT Codex | `response.completed` SSE event |
  | Ollama | final NDJSON object with `"done": true` |

  **Terminal-event rule.** Enforcement lives in the **stream adapters**,
  not the collectors. When the upstream byte/event stream ends (EOF)
  **without** the provider's terminal event, the adapter yields/throws
  the `IncompleteStream` error below. Adapters MUST NOT fabricate a
  synthetic `done` event and MUST NOT end the stream silently:
  truncation is always distinguishable from completion.

  Collectors are unchanged: they keep propagating adapter errors (the
  M1 fallible-stream contract) and keep the `stop_reason` heuristic
  **only** for a real terminal event that lacks a reason — never as a
  substitute for a missing terminal event.

  ### IncompleteStream error

  | SDK | Spelling |
  |-----|----------|
  | Rust | `MotosanError::IncompleteStream(String)` — `#[error("incomplete stream: {0}")]`; new enum variant ⇒ breaking, ships 0.24.0 |
  | Python | `class IncompleteStreamError(StreamError)` |
  | TypeScript | `export class IncompleteStreamError extends StreamError` |

  Message convention (all SDKs):
  `incomplete stream: <provider> ended without a terminal event` — e.g.
  `incomplete stream: openai ended without a terminal event`.

  Python and TypeScript subclass `StreamError` deliberately, as a
  migration softener: existing `except StreamError` /
  `instanceof StreamError` handlers still catch truncation. Handlers
  that must distinguish truncation match the subclass. Rust has no such
  softener — the new enum variant is the breaking change.

  ### Retired invariant (v0.10.1)

  The former guarantee that streams "emit exactly one terminal `done`
  event **even when the upstream provider closes the connection
  without** `[DONE]` **and without any** `finish_reason` **chunk**"
  (introduced in the v0.10.1 era; implemented via the `done_emitted` /
  `doneEmitted` EOF fabrication in the Rust and TypeScript OpenAI
  adapters — which also served MiniMax before its v0.14 move to the
  Anthropic wire — and equivalent defensive-EOF fabrication or
  silent-end paths elsewhere, e.g. the TypeScript Anthropic adapter's
  fallback `done` at EOF) is **deliberately retired**. Fabricating
  `done` on a truncated EOF made truncation indistinguishable from
  completion. The narrower invariant that survives: a stream that
  terminates *without error* emits exactly one terminal `done` event.

  ### Cancellation

  - **Rust** — drop-cancellation: dropping the stream (or the `chat()`
    future) drops the underlying `reqwest` response/future, which
    cancels the in-flight HTTP request and releases the connection.
    There is no explicit cancel API; this is documented behavior, not a
    code change.
  - **TypeScript** — per-request `AbortSignal`: aborting a
    caller-supplied signal cancels the underlying `fetch` and surfaces
    `CancelledError extends MotosanError`, which is never retried — see
    [`retry.md`](./retry.md#classification).
  - **Python** — standard `asyncio` task cancellation: cancelling the
    task awaiting `chat()` / iterating `stream()` raises
    `asyncio.CancelledError` through the SDK, and `httpx` closes the
    underlying connection. The SDK neither swallows nor converts
    `CancelledError`.
  ```

  **(c)** In `## MotosanError (Rust)`, current text (approximate line 129):

  ```
  `Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `UnsupportedFeature(String)`
  ```

  Replace with:

  ```
  `Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `IncompleteStream(String)` | `UnsupportedFeature(String)`
  ```

- [ ] 3. **Edit `specs/retry.md` — three edits.**

  **(a)** In the `## Classification` condition table, current text (approximate lines 26–27):

  ```
  | Transport / connection error (table below) | ✅ |
  | Any other 4xx (400, 401, 403, 404, 422, …) | ❌ never |
  ```

  Replace with:

  ```
  | Transport / connection error (table below) | ✅ |
  | Caller-initiated cancellation (TypeScript `CancelledError`, table below) | ❌ never |
  | Any other 4xx (400, 401, 403, 404, 422, …) | ❌ never |
  ```

  **(b)** Replace the transport-error block. Current text (approximate lines 44–50):

  ```
  Transport / connection errors (always retryable):

  | SDK | Surfaced as | Predicate |
  |-----|-------------|-----------|
  | Rust | `MotosanError::Network` | `is_retryable_network_error`: `reqwest::Error::is_timeout() \|\| is_connect() \|\| is_request() \|\| is_body()` |
  | Python | `NetworkError` | providers wrap `httpx.HTTPError` raised while sending (`httpx.TransportError`-derived in practice: `ConnectError`, `ConnectTimeout`, `ReadTimeout`, `ReadError`, …); every `NetworkError` is retryable |
  | TypeScript | raw fetch/Node error, classified directly | `isRetryableNetworkError`: `error.name === 'AbortError'`, `error instanceof TypeError`, or Node `error.code` ∈ {`ECONNREFUSED`, `ENOTFOUND`, `ETIMEDOUT`} |
  ```

  Replace with:

  ```
  Transport / connection / cancellation errors:

  | SDK | Surfaced as | Predicate | Retry |
  |-----|-------------|-----------|-------|
  | Rust | `MotosanError::Network` | `is_retryable_network_error`: `reqwest::Error::is_timeout() \|\| is_connect() \|\| is_request() \|\| is_body()` | ✅ |
  | Python | `NetworkError` | providers wrap `httpx.HTTPError` raised while sending (`httpx.TransportError`-derived in practice: `ConnectError`, `ConnectTimeout`, `ReadTimeout`, `ReadError`, …); every `NetworkError` is retryable | ✅ |
  | TypeScript | raw fetch/Node error, classified directly | `isRetryableNetworkError`: `error.name === 'AbortError'` **when no caller-supplied signal is aborted**, `error instanceof TypeError`, or Node `error.code` ∈ {`ECONNREFUSED`, `ENOTFOUND`, `ETIMEDOUT`} | ✅ |
  | TypeScript | `CancelledError` (`extends MotosanError`) | the caller-supplied per-request `AbortSignal` is aborted at failure time | ❌ never |

  TypeScript AbortError split — an abort is classified by **who
  aborted**. If the caller-supplied per-request `AbortSignal` is
  aborted, the SDK throws `CancelledError`: the caller asked to stop,
  and retrying would override that intent. A fetch-internal
  `AbortError` with no caller signal aborted (e.g. an SDK-composed
  `AbortSignal.timeout`) remains a retryable transport error, exactly
  as before.
  ```

  **(c)** In `## Streaming`, insert one paragraph between the existing first paragraph (ending "…follow the normal classification table.", approximate line 114) and the "Reference conformance tests:" paragraph (approximate line 116). Insert verbatim:

  ```
  Read-idle timeout errors that fire mid-stream (Rust
  `MotosanError::StreamReadTimeout`, TypeScript
  `StreamReadTimeoutError`, and Python `StreamReadTimeoutError` —
  surfaced from `httpx.ReadTimeout`) are **not retried**: they occur
  after the first emitted event, so the rule above applies, even though
  timeout-class transport errors are retryable during the connection
  phase.
  ```

- [ ] 4. Verify consistency: `grep -n "Exactly one terminal event per stream |" specs/types.md` returns nothing (the old row note is gone); `grep -n "IncompleteStream" specs/types.md` hits the MotosanError list and the new section; `grep -c "message_stop" specs/types.md` is ≥ 2 (the MiniMax Rust/TS row AND the Anthropic row of the terminal-event table); `grep -n "CancelledError" specs/retry.md` hits the classification table, the transport table, and the AbortError-split paragraph; both files still render as valid Markdown tables (pipe counts match per row).

- [ ] 5. No format/lint gate applies: `fmt` covers Rust + Python + TOML + Nix only, and `specs/*.md` is outside every linter target. Nothing to run.

- [ ] 6. Commit (docs/spec changes may land without the .rs PR+CI gate per repo workflow, but keep M3 sequencing — this commit precedes every SDK task):

  ```
  docs(specs): add stream termination contract; amend retry rules for cancellation and read-idle timeouts

  - types.md: terminal-event rule (adapter-enforced) with per-SDK
    MiniMax terminal events (Python [DONE] wire vs Rust/TS Anthropic
    delegate), IncompleteStream error spellings, retire the v0.10.1
    fabricated-done invariant, document cancellation semantics (Rust
    drop / TS AbortSignal / Python task cancellation)
  - retry.md: TS CancelledError never retried (caller-signal aborts);
    fetch-internal AbortError stays retryable; mid-stream read-idle
    timeouts are not retried

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```


## R — Rust: IncompleteStream, build-once, timeouts

### Task 2: Add MotosanError::IncompleteStream and make every Rust HTTP stream adapter error on EOF without the provider terminal event

Breaking change — this is the 0.24.0 headline. The "exactly one terminal done event even when upstream closes without `[DONE]`" invariant (documented since v0.10.1) is deliberately retired: adapters now yield `Err(IncompleteStream)` when the upstream byte/event stream ends without the provider terminal event (OpenAI `[DONE]`, Anthropic `message_stop`, Gemini/CodeAssist `finishReason` frame, codex `response.completed`, Ollama `"done":true`). Collectors are unchanged: `collect_stream` keeps propagating `Err` items (M1) and keeps the stop_reason heuristic only for a real done event that lacks a reason. House rule: all `.rs` changes land via PR + CI, never direct to main.

**E9 spec coupling (merge ordering):** This task's PR group must land AFTER the E9 spec task that amends `specs/types.md` §stream termination — the `done`-row language "Exactly one terminal event per stream" (~line 115) is retired/reworded there and replaced with the termination contract. Do not merge this PR group until that spec amendment has landed: otherwise spec and code contradict each other (this task flips the pinned openai tests the current spec language backs). The spec's per-provider terminal-event table introduced by the E9 task is the normative reference for which event counts as terminal for each provider; the parenthetical list above (`[DONE]` / `message_stop` / `finishReason` / `response.completed` / `"done":true`) is a convenience restatement of that table, and the table wins on any discrepancy.

**Files:**
- `sdks/rust/src/error.rs` — enum ~40-43, display test ~127-135, accessors test ~157-163
- `sdks/rust/src/providers/openai.rs` — `done_emitted` doc ~658-661, EOF arm ~846-861
- `sdks/rust/src/providers/anthropic.rs` — init ~828-834, struct ~852-868, `message_stop` arm ~1087-1093, `error` arm ~1094-1103, Err/EOF arms ~1107-1111, test init ~1128-1134
- `sdks/rust/src/providers/gemini.rs` — init ~379-382, struct ~398, finish arm ~481-491, Err/EOF arms ~498-501, test init ~540-543
- `sdks/rust/src/providers/gemini_code_assist.rs` — init ~145-149, struct ~165-167, finish arm ~279-289, Err/EOF arms ~296-299, test inits ~409, ~422, ~441, ~457
- `sdks/rust/src/providers/chatgpt_codex.rs` — init ~279-286, struct ~322-325, `response.completed` arm ~474-476, error-take sites ~516-518 and ~536-538, Err/EOF arms ~541-544, test inits ~743-750, ~921-928
- `sdks/rust/src/providers/ollama.rs` — init ~390-393, struct ~401-404, done arm ~429-431, Err/EOF arms ~474-479, test init ~557-560
- `sdks/rust/tests/openai_provider.rs` — flip two tests ~751-835
- `sdks/rust/tests/anthropic_stream.rs`, `sdks/rust/tests/gemini_provider.rs`, `sdks/rust/tests/gemini_code_assist.rs`, `sdks/rust/tests/chatgpt_codex.rs`, `sdks/rust/tests/ollama_native_provider.rs`, `sdks/rust/tests/collect_stream.rs` — new tests (append at end of each file)
- `sdks/rust/CHANGELOG.md` — `[Unreleased]` section ~5

**Interfaces:**
- Produces: `#[error("incomplete stream: {0}")] IncompleteStream(String)` on `MotosanError`. Message payload convention: `"<provider> ended without a terminal event"` with provider ∈ {`openai`, `anthropic`, `gemini`, `gemini-code-assist`, `chatgpt-codex`, `ollama`} (full Display: `"incomplete stream: openai ended without a terminal event"`).
- Consumes: `BoxStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, MotosanError>> + Send>>` (unchanged); `StreamEvent::done()` / `done_with_stop_reason(StopReason)` (unchanged).

**M2/M1 regression contract — pinned tests explicitly retired by E3 (the ONLY flips):**
- `sdks/rust/tests/openai_provider.rs::openai_stream_eof_flush_when_done_sentinel_missing` (~752)
- `sdks/rust/tests/openai_provider.rs::openai_stream_emits_done_on_eof_without_finish_reason_or_done_sentinel` (~795)

Every other stream test fixture already ends with its terminal frame and must pass unchanged.

Steps:

- [ ] 1. Write the failing tests. (a) In `sdks/rust/src/error.rs`, add to the `cases` vec in `display_strings_unchanged` (~line 131, after the `StreamReadTimeout` case):

  ```rust
            (
                MotosanError::IncompleteStream(
                    "openai ended without a terminal event".into(),
                ),
                "incomplete stream: openai ended without a terminal event",
            ),
  ```

  and add `MotosanError::IncompleteStream("i".into()),` to the `errors` array in `accessors_return_none_on_non_http_variants` (~line 161). (b) In `sdks/rust/tests/openai_provider.rs`, replace the two retired tests (approximate lines 751-835, both fixture bodies kept verbatim from the originals) with:

  ```rust
  #[tokio::test]
  async fn openai_stream_eof_without_done_sentinel_yields_incomplete_stream() {
      // Flip of pre-0.24 `openai_stream_eof_flush_when_done_sentinel_missing`:
      // finish_reason arrived but the `[DONE]` terminal sentinel never did —
      // that is truncation now, not an "EOF flush".
      let mut server = mockito::Server::new_async().await;
      let sse_body = concat!(
          "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"},\"finish_reason\":null}]}\n\n",
          "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"length\"}]}\n\n" // no [DONE]
      );
      server
          .mock("POST", "/v1/chat/completions")
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(sse_body)
          .create_async()
          .await;
      let provider = OpenAIProvider::new("test-key", None)
          .with_chat_url(format!("{}/v1/chat/completions", server.url()));
      let request = ChatRequest::builder()
          .message(Message::user("hello"))
          .build();
      let mut stream = provider.stream(request).await.expect("stream response");
      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without [DONE] must yield an error") {
          MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "openai ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }

  #[tokio::test]
  async fn openai_stream_eof_without_terminal_yields_incomplete_stream() {
      // Flip of pre-0.24 `openai_stream_emits_done_on_eof_without_finish_reason_or_done_sentinel`.
      let mut server = mockito::Server::new_async().await;
      let sse_body = "data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n";
      server
          .mock("POST", "/v1/chat/completions")
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(sse_body)
          .create_async()
          .await;
      let provider = OpenAIProvider::new("test-key", None)
          .with_chat_url(format!("{}/v1/chat/completions", server.url()));
      let request = ChatRequest::builder()
          .message(Message::user("hello"))
          .build();
      let mut stream = provider.stream(request).await.expect("stream response");
      let mut text = String::new();
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => text.push_str(&ev.content),
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert_eq!(text, "hello", "deltas before truncation still arrive");
      match last_err.expect("EOF without terminal must yield an error") {
          MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "openai ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (c) Append to `sdks/rust/tests/anthropic_stream.rs` (same drain-until-Err shape as `anthropic_stream_surfaces_mid_stream_error_frame` ~782; `MotosanError` is NOT imported there, use the full path):

  ```rust
  #[tokio::test]
  async fn anthropic_stream_eof_without_message_stop_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      let sse_body = concat!(
          "event: content_block_delta\n",
          "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"par\"}}\n\n",
      );
      server
          .mock("POST", "/v1/messages")
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(sse_body)
          .create_async()
          .await;
      let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
      let request = ChatRequest::builder().message(Message::user("hi")).build();
      let mut stream = provider.stream(request).await.expect("stream");
      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without message_stop must yield an error") {
          motosan_ai::MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "anthropic ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (d) Append to `sdks/rust/tests/gemini_provider.rs` (reuses that file's `sse_text` helper ~410; `MotosanError` IS imported there):

  ```rust
  #[tokio::test]
  async fn gemini_stream_eof_without_finish_reason_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      // Truncated: one text chunk, no finishReason frame ever arrives.
      let body = sse_text("Hello", None);
      server
          .mock("POST", Matcher::Regex("streamGenerateContent".into()))
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(body)
          .create_async()
          .await;

      let provider = GeminiProvider::new("key", None, Some(server.url()));
      let mut stream = provider
          .stream(
              ChatRequest::builder()
                  .messages(vec![Message::user("hi")])
                  .build(),
          )
          .await
          .unwrap();

      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without finishReason must yield an error") {
          MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "gemini ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (e) Append to `sdks/rust/tests/gemini_code_assist.rs` (reuses that file's `sse_text` helper ~34, which wraps the candidate in the CodeAssist `{"response": {...}}` envelope; `MotosanError` IS imported there; same `Matcher::Regex("streamGenerateContent".into())` route as the file's `stream_multi_chunk_text` test ~285-300):

  ```rust
  #[tokio::test]
  async fn code_assist_stream_eof_without_finish_reason_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      // Truncated: one text chunk, no finishReason frame ever arrives.
      let body = sse_text("Hello", None);
      server
          .mock("POST", Matcher::Regex("streamGenerateContent".into()))
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(body)
          .create_async()
          .await;

      let provider =
          GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()));
      let mut stream = provider
          .stream(
              ChatRequest::builder()
                  .messages(vec![Message::user("hi")])
                  .build(),
          )
          .await
          .unwrap();

      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without finishReason must yield an error") {
          MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "gemini-code-assist ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (f) Append to `sdks/rust/tests/chatgpt_codex.rs` — the established home for codex mockito/HTTP stream tests (the in-file `adapter_tests` module in `src/providers/chatgpt_codex.rs` is for pure adapter unit tests; this test drives real HTTP, so it belongs in the integration file). `MotosanError` IS imported there:

  ```rust
  #[tokio::test]
  async fn codex_stream_eof_without_response_completed_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      // Truncated: response.created + one text delta, but the terminal
      // `response.completed` frame never arrives.
      let truncated = concat!(
          "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"status\":\"in_progress\"}}\n\n",
          "data: {\"type\":\"response.output_text.delta\",\"content_index\":0,\"delta\":\"Hi\",\"item_id\":\"msg_1\",\"output_index\":1}\n\n"
      );
      server
          .mock("POST", Matcher::Any)
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(truncated)
          .create_async()
          .await;

      let provider =
          ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
      let mut stream = provider
          .stream(
              ChatRequest::builder()
                  .messages(vec![Message::user("hi")])
                  .build(),
          )
          .await
          .unwrap();

      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without response.completed must yield an error") {
          MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "chatgpt-codex ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (g) Append to `sdks/rust/tests/ollama_native_provider.rs` (`MotosanError` is NOT imported there, use the full path; `build_provider` is that file's helper ~12):

  ```rust
  #[tokio::test]
  async fn ollama_stream_eof_without_done_frame_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      // NDJSON truncated mid-stream: no `"done":true` terminal object.
      let ndjson_body = "{\"message\":{\"role\":\"assistant\",\"content\":\"The\"},\"done\":false}\n";
      server
          .mock("POST", "/api/chat")
          .with_status(200)
          .with_header("content-type", "application/x-ndjson")
          .with_body(ndjson_body)
          .create_async()
          .await;

      let provider = build_provider(server.url());
      let request = ChatRequest::builder()
          .message(Message::user("hello"))
          .build();

      let mut stream = provider.stream(request).await.expect("stream response");
      let mut saw_done = false;
      let mut last_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(ev) => saw_done |= ev.done,
              Err(e) => {
                  last_err = Some(e);
                  break;
              }
          }
      }
      assert!(!saw_done, "must not fabricate a done event on truncation");
      match last_err.expect("EOF without a done:true frame must yield an error") {
          motosan_ai::MotosanError::IncompleteStream(msg) => {
              assert_eq!(msg, "ollama ended without a terminal event")
          }
          other => panic!("expected IncompleteStream, got {other:?}"),
      }
  }
  ```

  (h) Append to `sdks/rust/tests/collect_stream.rs`:

  ```rust
  #[tokio::test]
  async fn collect_stream_propagates_incomplete_stream_error() {
      let stream: motosan_ai::BoxStream = Box::pin(tokio_stream::iter(vec![
          Ok(StreamEvent::text("par")),
          Err(motosan_ai::MotosanError::IncompleteStream(
              "openai ended without a terminal event".to_string(),
          )),
      ]));
      let err = collect_stream(stream)
          .await
          .expect_err("truncation must not collect into a ChatResponse");
      assert!(matches!(
          err,
          motosan_ai::MotosanError::IncompleteStream(msg)
              if msg == "openai ended without a terminal event"
      ));
  }
  ```

- [ ] 2. Run `cargo test --all-features incomplete_stream` from `sdks/rust` and confirm it FAILS to compile with `error[E0599]: no variant or associated item named `IncompleteStream` found for enum `MotosanError`` (one per test file).

- [ ] 3. Implement. (a) `sdks/rust/src/error.rs` — Current code (approximate lines 40-41):

  ```rust
      #[error("stream read timeout: no data received within {0} seconds")]
      StreamReadTimeout(u64),
  ```

  Replace with:

  ```rust
      #[error("stream read timeout: no data received within {0} seconds")]
      StreamReadTimeout(u64),
      /// Upstream byte/event stream ended without the provider terminal event
      /// (`[DONE]` / `message_stop` / `finishReason` / `response.completed` /
      /// `"done":true`). Payload: `"<provider> ended without a terminal event"`.
      #[error("incomplete stream: {0}")]
      IncompleteStream(String),
  ```

  (b) `sdks/rust/src/providers/openai.rs` — update the `done_emitted` doc comment (~658-661) to: `/// Whether a terminal `[DONE]` was parsed (or the stream already ended in an error). The EOF arm uses this to decide clean end vs IncompleteStream.` Then, Current code (approximate lines 846-861):

  ```rust
                  Poll::Ready(None) => {
                      // End of upstream stream. Guarantee the consumer always
                      // sees exactly one terminal `done` event, even when the
                      // provider closes the connection without sending
                      // `[DONE]` and without any `finish_reason` chunk (some
                      // non-conformant proxies do this).
                      if !self.done_emitted {
                          self.done_emitted = true;
                          let done = match self.pending_stop_reason.take() {
                              Some(reason) => StreamEvent::done_with_stop_reason(reason),
                              None => StreamEvent::done(),
                          };
                          return Poll::Ready(Some(Ok(done)));
                      }
                      return Poll::Ready(None);
                  }
  ```

  Replace with:

  ```rust
                  Poll::Ready(None) => {
                      // M3: upstream closed without the `[DONE]` terminal
                      // sentinel. The pre-0.24 "EOF flush" that fabricated a
                      // clean `done` here is retired — truncation is a typed
                      // error. Any stashed finish_reason is discarded: without
                      // `[DONE]` the stream did not terminate.
                      if !self.done_emitted {
                          self.done_emitted = true;
                          return Poll::Ready(Some(Err(MotosanError::IncompleteStream(
                              "openai ended without a terminal event".to_string(),
                          ))));
                      }
                      return Poll::Ready(None);
                  }
  ```

  (c) `sdks/rust/src/providers/anthropic.rs` — add struct field after `current_thinking_buf` (~868): `/// True once `message_stop` (or a terminal error) has been yielded.` + `saw_terminal: bool,`; add `saw_terminal: false,` to BOTH initializers (~828-834 and test init ~1128-1134); add `self.saw_terminal = true;` as the first statement of the `"message_stop"` arm (~1088), of the `"error"` arm (~1095), and of the `Poll::Ready(Some(Err(e)))` arm (~1107). Then, Current code (approximate line 1110):

  ```rust
                  Poll::Ready(None) => return Poll::Ready(None),
  ```

  Replace with:

  ```rust
                  Poll::Ready(None) => {
                      if !self.saw_terminal {
                          self.saw_terminal = true;
                          return Poll::Ready(Some(Err(MotosanError::IncompleteStream(
                              "anthropic ended without a terminal event".to_string(),
                          ))));
                      }
                      return Poll::Ready(None);
                  }
  ```

  (d) `sdks/rust/src/providers/gemini.rs` — same pattern: field `saw_terminal: bool,` on `GeminiStreamAdapter` (~398); `saw_terminal: false,` in initializers ~379-382 and test init ~540-543; `self.saw_terminal = true;` as first statement inside `if let Some(reason) = finish_reason {` (~481) and in the `Poll::Ready(Some(Err(e)))` arm (~498); replace `Poll::Ready(None) => return Poll::Ready(None),` (~501) with the (c) block using message `"gemini ended without a terminal event"`.
  (e) `sdks/rust/src/providers/gemini_code_assist.rs` — same pattern on `CodeAssistStreamAdapter`: field ~167; initializers ~145-149 and test inits ~409, ~422, ~441, ~457; flag-set inside `if let Some(reason) = finish_reason {` (~279) and the Err arm (~296); replace the EOF arm (~299) with message `"gemini-code-assist ended without a terminal event"`.
  (f) `sdks/rust/src/providers/chatgpt_codex.rs` — field `saw_terminal: bool,` after `error` (~325); `saw_terminal: false,` in initializers ~279-286 and test inits ~743-750, ~921-928; `self.saw_terminal = true;` immediately after the `push_back(StreamEvent::done_with_stop_reason(stop_reason));` in the `"response.completed"` arm (~475), inside BOTH `if let Some(msg) = self.error.take()` blocks before returning Err (~516-518 and ~536-538), and in the `Poll::Ready(Some(Err(e)))` arm (~541); replace the EOF arm (~544) with message `"chatgpt-codex ended without a terminal event"`.
  (g) `sdks/rust/src/providers/ollama.rs` — field on `OllamaStreamAdapter` (~403); `saw_terminal: false,` in initializers ~390-393 and test init ~557-560; in the done arm (~429-431) set `self.saw_terminal = true;` before `return Poll::Ready(Some(Ok(StreamEvent::done())));`; set it in the Err passthrough arm (~474-477); replace the EOF arm (~479) with message `"ollama ended without a terminal event"`.
  (h) `sdks/rust/CHANGELOG.md` — under `## [Unreleased]` (~line 5) add:

  ```markdown
  ### Breaking
  - **Truncated streams now error instead of ending cleanly.** Every HTTP stream adapter (openai, anthropic, gemini, gemini-code-assist, chatgpt-codex, ollama) yields `Err(MotosanError::IncompleteStream("<provider> ended without a terminal event"))` when the upstream connection closes without the provider terminal event (`[DONE]` / `message_stop` / `finishReason` / `response.completed` / `"done":true`). The v0.10.1 invariant "exactly one terminal done event even when upstream closes without `[DONE]`" is retired. New `MotosanError::IncompleteStream(String)` variant — enum addition breaks exhaustive matches.
  ```

- [ ] 4. Run `cargo test --all-features incomplete_stream` from `sdks/rust` — all new/flipped tests pass. Then run the full package suite: `cargo test --all-features` — everything else must pass unchanged (the only retired behaviors are the two openai tests flipped in step 1).

- [ ] 5. Run `cargo fmt` then `cargo clippy --all-features --all-targets -- -D warnings` from `sdks/rust`; fix any lint (note: `--all-targets` lints the test files too).

- [ ] 6. Commit on a feature branch and open a PR (never direct to main; CI must pass). Per the E9 spec coupling above, do not merge this PR until the E9 `specs/types.md` termination-contract amendment has landed:

  ```
  feat(rust)!: surface truncated streams as MotosanError::IncompleteStream

  BREAKING: EOF without the provider terminal event now yields
  Err(IncompleteStream) from every HTTP stream adapter instead of a
  fabricated clean done event; retires the v0.10.1 "exactly one done
  even without [DONE]" invariant. Flipped pinned tests:
  tests/openai_provider.rs::openai_stream_eof_flush_when_done_sentinel_missing,
  tests/openai_provider.rs::openai_stream_emits_done_on_eof_without_finish_reason_or_done_sentinel.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

### Task 3: Build Rust providers once in ClientBuilder::build() with a shared connect-timeout reqwest::Client (E5)

**Files:** (all line refs approximate, against origin/main @ acf5d7f)
- `sdks/rust/src/client.rs` — Client struct ~9–68; dispatch_chat ~199–375; dispatch_stream ~377–390 (unchanged); dispatch_stream_inner ~392–557; `feature_not_enabled` ~559–574 (delete); `build_*_provider` helpers ~576–762; `ClientBuilder::build()` ~1059–1144
- `sdks/rust/src/providers/anthropic.rs` ~24 (derive), ~49–52 (insert after `with_retry_policy`)
- `sdks/rust/src/providers/openai.rs` ~36 (derive), ~81–84
- `sdks/rust/src/providers/ollama.rs` ~18 (derive), ~56–59
- `sdks/rust/src/providers/gemini.rs` ~58–61 (already derives Debug/Clone)
- `sdks/rust/src/providers/gemini_code_assist.rs` ~71–74 (already derives)
- `sdks/rust/src/providers/chatgpt_codex.rs` ~58–61 (already derives)
- `sdks/rust/tests/client_builder.rs` — append two tests

**Interfaces:**
- Consumes: `pub trait ProviderImpl: Send + Sync` (`src/providers/mod.rs` ~79) — `fn validate_request(&self, req: &ChatRequest) -> Result<(), MotosanError>`, `async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError>`, `async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError>`. Object-safe; dyn-dispatch is the sanctioned seam (CLAUDE.md: interchangeable via `Box<dyn ProviderImpl>`).
- Consumes (E4 interop): if the E4 Rust timeouts task has already landed and `client.rs` contains `pub(crate) struct TimeoutConfig`, source the connect timeout below from its `connect: Duration` field instead of adding `DEFAULT_CONNECT_TIMEOUT`. Otherwise use the const (E4 default: connect = 10s).
- Produces: `pub fn with_http_client(mut self, http: Client) -> Self` on all six HTTP providers (`Client` = `reqwest::Client`, already imported in each file); private `enum BuiltProvider` + `fn as_impl(&self) -> Result<&dyn crate::providers::ProviderImpl, MotosanError>`; shared client via `reqwest::Client::builder().connect_timeout(...)` (E5 spelling). Public API unchanged: `Client::builder()...build()`, all ClientBuilder setters, `Client` keeps `#[derive(Debug, Clone)]`.
- M2 regression contract: NO pinned behavior retired here — full suite must pass unchanged.

**Steps:**

- [ ] 1. Append two tests to `sdks/rust/tests/client_builder.rs` (mockito, matching the file's existing `builder_anthropic_base_url_is_forwarded_to_http_request` style and `tests/gemini_code_assist.rs`'s 5xx-then-200 two-mock pattern):

```rust
#[cfg(feature = "anthropic")]
#[tokio::test]
async fn built_client_serves_sequential_requests_from_one_provider() {
    // 0.24.0 build-once smoke: ClientBuilder::build() constructs the provider a
    // single time and dispatch borrows it. Two sequential chat() calls against
    // one mockito server prove the pre-built provider (and its shared
    // reqwest::Client) serves repeated requests.
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_body(
            serde_json::json!({
                "model": "claude-sonnet-4-6",
                "stop_reason": "end_turn",
                "usage": {"input_tokens": 1, "output_tokens": 1},
                "content": [{"type": "text", "text": "ok"}]
            })
            .to_string(),
        )
        .expect(2)
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::Anthropic)
        .api_key("test-key")
        .anthropic_base_url(server.url())
        .build()
        .expect("build client");

    let first = client.chat(vec![Message::user("one")]).await.expect("first chat");
    let second = client.chat(vec![Message::user("two")]).await.expect("second chat");
    assert_eq!(first.content, "ok");
    assert_eq!(second.content, "ok");
    mock.assert_async().await;
}

#[cfg(feature = "gemini-code-assist")]
#[tokio::test]
async fn prebuilt_gemini_code_assist_applies_client_builder_retry_policy() {
    // Regression guard (0.24.0): ClientBuilder::retry_policy was silently
    // discarded when a pre-built GeminiCodeAssistProvider was attached. The
    // pre-built provider carries a zero-retry policy; the builder sets one
    // fast retry. Fixed: builder policy wins -> the 503 is retried and chat
    // succeeds. Old behavior: zero-retry wins -> chat() errors on the 503.
    use motosan_ai::providers::gemini_code_assist::GeminiCodeAssistProvider;

    let mut server = mockito::Server::new_async().await;
    let failed = server
        .mock("POST", mockito::Matcher::Regex("streamGenerateContent".into()))
        .with_status(503)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
        .expect(1)
        .create_async()
        .await;
    let sse = serde_json::json!({"response": {"candidates": [{"content": {"parts":
        [{"text": "recovered"}], "role": "model"}, "finishReason": "STOP"}],
        "usageMetadata": {"promptTokenCount": 5, "candidatesTokenCount": 3}}});
    let recovered = server
        .mock("POST", mockito::Matcher::Regex("streamGenerateContent".into()))
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(format!("data: {sse}\n\n"))
        .expect(1)
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::GeminiCodeAssist)
        .gemini_code_assist(
            GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()))
                .with_retry_policy(RetryPolicy::new().max_retries(0).base_delay_ms(0).max_delay_ms(0)),
        )
        .retry_policy(RetryPolicy::new().max_retries(1).base_delay_ms(1).max_delay_ms(5).jitter(false))
        .build()
        .expect("build client");

    let response = client
        .chat(vec![Message::user("hi")])
        .await
        .expect("builder retry_policy must be applied to the pre-built provider");
    assert_eq!(response.content, "recovered");
    failed.assert_async().await;
    recovered.assert_async().await;
}
```

- [ ] 2. Run-fail (from `sdks/rust`): `cargo test --all-features --test client_builder prebuilt_gemini_code_assist` — expect FAILED, panic `builder retry_policy must be applied to the pre-built provider: ProviderError { message: "overloaded", status_code: Some(503), .. }` (503 not retried under the zero-retry pre-built policy). The sequential-requests smoke PASSES pre-change — it pins behavior parity through the refactor.

- [ ] 3. Implement. **The 7-feature HTTP cfg union** below, referred to as `#[cfg(HTTP)]`, must be written out in full at every site marked `#[cfg(HTTP)]`:
```rust
#[cfg(any(feature = "anthropic", feature = "openai", feature = "minimax",
    feature = "ollama", feature = "gemini", feature = "gemini-code-assist",
    feature = "chatgpt-codex"))]
```
  **3a — providers.** Add `#[derive(Debug, Clone)]` directly above `pub struct AnthropicProvider` (anthropic.rs ~24), `pub struct OpenAIProvider` (openai.rs ~36), `pub struct OllamaProvider` (ollama.rs ~18) — all fields already impl Debug+Clone (RetryPolicy has a manual Debug impl; ProviderCapabilities/OpenAIAuthStyle derive both). Then insert this method immediately after `with_retry_policy` in all SIX files (anthropic.rs ~52, openai.rs ~84, ollama.rs ~59, gemini.rs ~61, gemini_code_assist.rs ~74, chatgpt_codex.rs ~61); each file already has `use reqwest::Client;`:
```rust
    /// Replace the internal `reqwest::Client` with a caller-supplied one.
    /// `ClientBuilder::build()` uses this to hand every HTTP provider one
    /// shared, connect-timeout-configured client so all providers share a
    /// single connection pool instead of each constructing their own.
    pub fn with_http_client(mut self, http: Client) -> Self {
        self.http = http;
        self
    }
```
  **3b — client.rs new types.** Below the `use` block (~line 8) add, under `#[cfg(HTTP)]`: `const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);` (skip if sourcing from E4's TimeoutConfig — see Interfaces). Above `pub struct Client` add:
```rust
/// The provider selected and fully constructed at `ClientBuilder::build()`
/// time (E5 build-once). Generalizes the pre-0.24 per-provider pre-built
/// Option fields: dispatch borrows this instead of rebuilding per request.
#[derive(Debug, Clone)]
enum BuiltProvider {
    #[cfg(feature = "anthropic")]
    Anthropic(crate::providers::anthropic::AnthropicProvider),
    #[cfg(feature = "openai")]
    OpenAI(crate::providers::openai::OpenAIProvider),
    #[cfg(feature = "minimax")]
    Minimax(crate::providers::anthropic::AnthropicProvider),
    #[cfg(feature = "ollama")]
    OllamaCompat(crate::providers::openai::OpenAIProvider),
    #[cfg(feature = "ollama")]
    OllamaNative(crate::providers::ollama::OllamaProvider),
    #[cfg(feature = "claude-code")]
    ClaudeCode(crate::providers::claude_code::ClaudeCodeProvider),
    #[cfg(feature = "codex-cli")]
    CodexCli(crate::providers::codex_cli::CodexCliProvider),
    #[cfg(feature = "gemini-cli")]
    GeminiCli(crate::providers::gemini_cli::GeminiCliProvider),
    #[cfg(feature = "gemini")]
    Gemini(crate::providers::gemini::GeminiProvider),
    #[cfg(feature = "gemini-code-assist")]
    GeminiCodeAssist(crate::providers::gemini_code_assist::GeminiCodeAssistProvider),
    #[cfg(feature = "chatgpt-codex")]
    ChatGptCodex(crate::providers::chatgpt_codex::ChatGptCodexProvider),
    /// Selected provider's cargo feature is not enabled; dispatch returns
    /// `MotosanError::Config` (pre-0.24 `feature_not_enabled` behavior).
    Disabled(&'static str),
}

impl BuiltProvider {
    fn as_impl(&self) -> Result<&dyn crate::providers::ProviderImpl, MotosanError> {
        match self {
            #[cfg(feature = "anthropic")]
            BuiltProvider::Anthropic(p) => Ok(p),
            // ...one identical `(p) => Ok(p)` arm for EACH of the ten
            // remaining provider variants above, each under the exact same
            // #[cfg(...)] attribute as its variant declaration...
            BuiltProvider::Disabled(feature) => Err(MotosanError::Config(format!(
                "{feature} feature is not enabled"
            ))),
        }
    }
}
```
  In `struct Client` append two fields after the chatgpt fields: `built: BuiltProvider,` (no cfg) and, under `#[cfg(HTTP)]`, `http: reqwest::Client,` (doc: shared transport; reqwest::Client is reference-counted so per-provider clones share one pool).
  **3c — dispatch.** Current code (approximate lines 199–375 `dispatch_chat`, 392–557 `dispatch_stream_inner`, 559–574 `feature_not_enabled`): three members — two ~170-line per-request `match self.provider` construction blocks plus the helper. Replace ALL THREE with:
```rust
    async fn dispatch_chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
        let provider = self.built.as_impl()?;
        self.validate_for_dispatch(provider, &request)?;
        provider.chat(request).await
    }
```
  keep `dispatch_stream` (~377–390) as-is, then an identical `dispatch_stream_inner` whose last line is `provider.stream(request).await`, then:
```rust
    fn validate_for_dispatch(
        &self,
        provider: &dyn crate::providers::ProviderImpl,
        request: &ChatRequest,
    ) -> Result<(), MotosanError> {
        // Same auto-switch capability trade-off message as the pre-0.24
        // dispatch arms — chat and stream must never split on this.
        #[cfg(feature = "ollama")]
        if matches!(self.built, BuiltProvider::OllamaNative(_)) {
            return provider.validate_request(request).map_err(|e| match e {
                MotosanError::UnsupportedFeature(msg) => MotosanError::UnsupportedFeature(format!(
                    "{msg} — Provider::Ollama was auto-routed to the native /api/chat endpoint \
                     because one of ollama_keep_alive / ollama_num_ctx / ollama_think is set, \
                     and the native endpoint is text-only. Either remove the tuning field(s) to \
                     stay on the OpenAI-compat path (which supports images), or remove the image \
                     input."
                )),
                other => other,
            });
        }
        provider.validate_request(request)
    }
```
  (Copy the wrapped message VERBATIM from the deleted Ollama arms — byte-identical.) Add to `impl Client` a `fn construct_built_provider(&self) -> BuiltProvider` matching on `self.provider` with ten arms in the codebase's existing two-block cfg pattern; first arm in full:
```rust
            Provider::Anthropic => {
                #[cfg(feature = "anthropic")]
                {
                    BuiltProvider::Anthropic(self.build_anthropic_provider())
                }
                #[cfg(not(feature = "anthropic"))]
                {
                    BuiltProvider::Disabled("anthropic")
                }
            }
```
  `Provider::Ollama`'s enabled block hoists the routing verbatim from the deleted dispatch arm: `let needs_native = self.ollama_native || self.ollama_keep_alive.is_some() || self.ollama_num_ctx.is_some() || self.ollama_think.is_some();` then `if needs_native { BuiltProvider::OllamaNative(self.build_ollama_native_provider()) } else { BuiltProvider::OllamaCompat(self.build_ollama_provider()) }`. Remaining eight arms use the identical pattern with these exact (match arm | feature | enabled variant(helper) | Disabled str) tuples: OpenAI | openai | `OpenAI(self.build_openai_provider())` | "openai"; Minimax | minimax | `Minimax(self.build_minimax_provider())` | "minimax"; ClaudeCode | claude-code | `ClaudeCode(self.build_claude_code_provider())` | "claude-code"; CodexCli | codex-cli | `CodexCli(self.build_codex_cli_provider())` | "codex-cli"; GeminiCli | gemini-cli | `GeminiCli(self.build_gemini_cli_provider())` | "gemini-cli"; Gemini | gemini | `Gemini(self.build_gemini_provider())` | "gemini"; GeminiCodeAssist | gemini-code-assist | `GeminiCodeAssist(self.build_gemini_code_assist_provider())` | "gemini-code-assist"; OpenAiChatGpt | chatgpt-codex | `ChatGptCodex(self.build_chatgpt_codex_provider())` | "chatgpt-codex".
  **3d — helpers get the shared client.** In each HTTP `build_*_provider` helper append `.with_http_client(self.http.clone())` to the constructor chain — 8 sites: `build_anthropic_provider` (~584, after `.with_retry_policy(...)`), `build_openai_provider` (~599, after `.with_retry_policy(...)` inside the `let mut provider` chain), `build_minimax_provider` (~630), `build_ollama_provider` (~652), `build_ollama_native_provider` (~665), `build_gemini_provider` (~693), `build_gemini_code_assist_provider` None-branch only (~710), `build_chatgpt_codex_provider` (~761). Do NOT touch the pre-built `Some(provider)` arms or CLI helpers — a caller-supplied provider keeps its own client. Simplify `build_gemini_code_assist_provider`'s `Some` arm to stay exactly `Some(provider) => provider` (the retry fix lands in build(), next).
  **3e — build().** Current code (approximate lines 1106–1144): `Ok(Client { ... })` literal. Replace the tail with:
```rust
        // Shared HTTP transport, built once with the connect timeout applied
        // (E4/E5). reqwest::Client is internally reference-counted: the
        // per-provider clones share this one connection pool.
        #[cfg(HTTP)]  // write the full 7-feature union here
        let http = reqwest::Client::builder()
            .connect_timeout(DEFAULT_CONNECT_TIMEOUT)
            .build()
            .map_err(|e| MotosanError::Config(format!("failed to build HTTP client: {e}")))?;

        // 0.24.0 fix: an explicitly-set ClientBuilder::retry_policy now
        // reaches a pre-built GeminiCodeAssistProvider (previously silently
        // discarded). Without an explicit builder policy the pre-built
        // provider's own policy stands — do not clobber it with the default.
        #[cfg(feature = "gemini-code-assist")]
        let gemini_code_assist = match (self.gemini_code_assist, self.retry_policy.as_ref()) {
            (Some(provider), Some(policy)) => Some(provider.with_retry_policy(policy.clone())),
            (provider, _) => provider,
        };

        let mut client = Client {
            // ...ALL existing field initializers verbatim, EXCEPT the
            // gemini-code-assist line becomes `gemini_code_assist,` (the
            // local above) instead of `gemini_code_assist: self.gemini_code_assist,`...
            built: BuiltProvider::Disabled("uninitialized"),
            #[cfg(HTTP)]  // full union again
            http,
        };
        let built = client.construct_built_provider();
        client.built = built;
        Ok(client)
```
  If clippy flags `large_enum_variant` on `BuiltProvider`, Box the flagged variant's payload (e.g. `ClaudeCode(Box<...ClaudeCodeProvider>)`) and adjust its construct arm (`Box::new(...)`) and `as_impl` arm (`Ok(&**p)`) — nothing else.

- [ ] 4. Run-pass (from `sdks/rust`): `cargo test --all-features --test client_builder` — both new tests pass. Then the full package suite: `cargo test --all-features` — everything green, including `tests/ollama_http_autoswitch.rs`, `tests/anthropic_minimax_routing.rs`, `tests/gemini_code_assist.rs`, the in-crate `dispatch_validation_tests`, and the M2 `retry_conformance` module (this task retires no pinned behavior). Also verify the no-features build compiles: `cargo test --no-default-features`.

- [ ] 5. Format and lint: `cargo fmt` then `cargo clippy --all-features --all-targets -- -D warnings` (—all-targets is mandatory; CI lints tests).

- [ ] 6. Commit on the milestone branch (every .rs change lands via PR + CI per house rules):
```
refactor(client): construct providers once in build() with shared reqwest client

ClientBuilder::build() now builds the selected provider a single time into
a BuiltProvider enum; dispatch borrows it instead of rebuilding provider +
connection pool per request. One shared reqwest::Client (connect timeout
applied) is handed to every HTTP provider via new with_http_client(). Also
fixes ClientBuilder::retry_policy being silently discarded by the pre-built
gemini_code_assist path.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```

### Task 4: Add the unified Rust timeout model (connect / read-idle / total) to ClientBuilder and the HTTP providers

> **Execute AFTER the build-once task.** This task threads the configurable connect timeout into the shared `reqwest::Client` that the build-once task created in `ClientBuilder::build()`, and chains `.with_total_timeout(...)` onto the provider instances that build() now constructs once. All line refs below are approximate and taken from origin/main @ acf5d7f — the build-once task moves the provider-construction code, so locate build() seams by searching, not by line number.

**Files:**
- `sdks/rust/src/client.rs` (~7 imports; ~27 Client field; ~91-93 getter; ~377-390 `dispatch_stream`; ~765-801 ClientBuilder fields; ~962-965 `stream_read_timeout_secs`; ~1059-1144 `build()`; ~1147-1223 `ReadTimeoutStream` cfg gates)
- `sdks/rust/src/providers/mod.rs` (~55-76 `Provider` enum — add impl after it; new helpers after `send_with_retry` ~465)
- `sdks/rust/src/providers/openai.rs` (~4 imports, ~36-69 struct+new, ~81-84 setters, ~247-250 + ~496-499 chat closures; stream ~602 UNCHANGED)
- `sdks/rust/src/providers/anthropic.rs` (~6 imports, ~24-29 struct, `new`/`with_retry_policy` nearby, ~459-462 OAuth collect branch, ~470-479 chat closure; stream ~795 UNCHANGED)
- `sdks/rust/src/providers/gemini.rs` (~6 imports, ~36-40 struct, ~324-331 chat closure; stream ~359 UNCHANGED)
- `sdks/rust/src/providers/ollama.rs` (~4 imports, ~19-40 struct+new, ~250-253 chat closure; stream ~346 UNCHANGED)
- `sdks/rust/src/providers/gemini_code_assist.rs` (~7 imports, ~46-51 struct, ~111-119 chat collect; stream closure ~125 UNCHANGED)
- `sdks/rust/src/providers/chatgpt_codex.rs` (~3 imports, ~27-32 struct, ~245-253 chat collect; stream closure ~259 UNCHANGED)
- `sdks/rust/tests/client_timeouts.rs` (new)
- `sdks/rust/CHANGELOG.md` (Unreleased section)

**Interfaces:**
- Consumes: `pub(crate) async fn send_with_retry(policy: &RetryPolicy, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, MotosanError>` (providers/mod.rs ~432); the shared `reqwest::Client` built in `ClientBuilder::build()` by the build-once task; the cfg-gated `const DEFAULT_CONNECT_TIMEOUT` the build-once task added to client.rs (DELETED and replaced by this task — see step 3(a)); `struct ReadTimeoutStream` (client.rs ~1157) and `MotosanError::StreamReadTimeout(u64)` (error.rs ~41); the CLI per-line deadline precedent `tokio::time::timeout(dur, lines.next_line())` in `drive_lines` (providers/claude_code/mod.rs ~568-575) — same pattern, same error.
- Produces: `pub(crate) struct TimeoutConfig { connect: Duration /* 10s default */, read_idle: Duration /* 120s default */, total: Option<Duration> /* None */ }` (client.rs); `ClientBuilder::connect_timeout(Duration) / .read_idle_timeout(Duration) / .total_timeout(Duration)`; `Client::connect_timeout() -> Duration`, `Client::read_idle_timeout() -> Duration`, `Client::total_timeout() -> Option<Duration>`; `Provider::uses_http_transport(&self) -> bool` (pub(crate)); `pub(crate) fn apply_total_timeout(rb: reqwest::RequestBuilder, total: Option<Duration>) -> reqwest::RequestBuilder`; `pub(crate) async fn collect_stream_with_total_timeout(stream: BoxStream, total: Option<Duration>) -> Result<ChatResponse, MotosanError>`; `pub fn with_total_timeout(mut self, total: Option<Duration>) -> Self` on AnthropicProvider, OpenAIProvider, GeminiProvider, OllamaProvider, GeminiCodeAssistProvider, ChatGptCodexProvider.

**Unification resolution (E4, stated per instruction):** the existing knob is `stream_read_timeout` (client.rs field ~27, getter ~91, builder setter `stream_read_timeout_secs` ~962) — opt-in, default None, applied in `dispatch_stream` (~386) by wrapping the event stream in `ReadTimeoutStream`. `read_idle_timeout` supersedes it: same `ReadTimeoutStream` enforcement point, now always-on for HTTP providers with a 120s default. CLI backends (claude-code/codex-cli/gemini-cli) are excluded from the wrap — they already enforce an idle deadline per line inside `drive_lines` (yielding the same `MotosanError::StreamReadTimeout`), configurable via provider-level `.timeout()`/`.no_timeout()`. The builder setter `stream_read_timeout_secs` stays as a `#[deprecated]` alias writing the same field; the `Client::stream_read_timeout()` getter is removed (breaking — this ships in 0.24.0). `total` uses reqwest's `RequestBuilder::timeout()`, which spans connect + headers + the whole body read — that is exactly why it is applied ONLY inside blocking-chat build closures and never to stream request phases: it would kill any stream living longer than `total`. It is per-attempt: each retry gets a fresh budget.

**Scope note (blessed narrowing of E4):** E4's wording covered "streaming reads AND response reads"; this task deliberately narrows read-idle to streaming reads only. Non-streaming `chat()` body reads get NO idle guard — they are bounded by `connect_timeout` plus the opt-in `total_timeout`. This is deliberate: reqwest cannot cheaply idle-bound a buffered body read (there is no per-chunk idle knob on a `.json()`/collected-body read). The narrowing is blessed; the plan's Global Constraints records the same.

**Steps:**

- [ ] 1. Write the failing tests. Create `sdks/rust/tests/client_timeouts.rs` (mockito + `Server::new_async`, matching `tests/openai_retry.rs` style):

```rust
#![cfg(feature = "openai")]

use motosan_ai::{Client, Message, MotosanError, Provider, RetryCause, RetryPolicy};
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio_stream::StreamExt;

#[test]
fn builder_timeout_defaults_are_10s_120s_none() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .build()
        .expect("build client");
    assert_eq!(client.connect_timeout(), Duration::from_secs(10));
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(120));
    assert_eq!(client.total_timeout(), None);
}

#[test]
fn builder_timeout_setters_override_defaults() {
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .connect_timeout(Duration::from_secs(3))
        .read_idle_timeout(Duration::from_secs(30))
        .total_timeout(Duration::from_secs(90))
        .build()
        .expect("build client");
    assert_eq!(client.connect_timeout(), Duration::from_secs(3));
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(30));
    assert_eq!(client.total_timeout(), Some(Duration::from_secs(90)));
}

#[test]
fn stream_read_timeout_secs_is_an_alias_for_read_idle() {
    #[allow(deprecated)]
    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .stream_read_timeout_secs(30)
        .build()
        .expect("build client");
    assert_eq!(client.read_idle_timeout(), Duration::from_secs(30));
}

// One chunk, then the connection goes idle past read_idle. The stream must
// surface MotosanError::StreamReadTimeout — not hang and not end silently.
//
// NOTE on structure: Client::stream() wraps everything in ThinkStripperStream
// (client.rs ~1226), which (a) holds back up to "<think>".len()-1 = 6 trailing
// chars of every text event (think_stripper.rs ~49-56), and (b) flushes that
// buffered tail as one final Ok(text) AFTER the inner stream ends — i.e.
// after the Err (client.rs ~1269-1275). So exact item ordering is not pinned
// here: the test drains the whole stream and asserts the invariants instead.
// The 11-char delta guarantees the stripper emits "hello" (buffering " world")
// before the stall, so pre-stall delivery is still observable.
#[tokio::test]
async fn hung_stream_yields_stream_read_timeout() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_chunked_body(|w| {
            w.write_all(
                b"data: {\"choices\":[{\"delta\":{\"content\":\"hello world\"}}]}\n\n",
            )?;
            w.flush()?;
            // Stall far longer than the configured 200ms read_idle.
            std::thread::sleep(std::time::Duration::from_millis(1500));
            Ok(())
        })
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("{}/v1/chat/completions", server.url()))
        .read_idle_timeout(Duration::from_millis(200))
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream opens");
    let mut text = String::new();
    let mut saw_timeout = false;
    let mut saw_done = false;
    while let Some(item) = stream.next().await {
        match item {
            Ok(ev) => {
                saw_done |= ev.done;
                text.push_str(&ev.content);
            }
            Err(MotosanError::StreamReadTimeout(_)) => saw_timeout = true,
            Err(other) => panic!("expected StreamReadTimeout, got {other:?}"),
        }
    }
    assert!(saw_timeout, "idle stall must yield MotosanError::StreamReadTimeout");
    assert!(!saw_done, "no fabricated terminal done after an idle timeout");
    assert!(
        text.starts_with("hello"),
        "pre-stall content must be delivered, got {text:?}"
    );
}

// total_timeout must NEVER be applied to stream request phases: reqwest's
// RequestBuilder::timeout() spans the entire body read and would kill any
// stream that lives longer than `total`.
#[tokio::test]
async fn total_timeout_does_not_apply_to_streams() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_chunked_body(|w| {
            w.write_all(b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\n")?;
            w.flush()?;
            // Keep the stream alive well past total_timeout (100ms).
            std::thread::sleep(std::time::Duration::from_millis(400));
            w.write_all(b"data: [DONE]\n\n")?;
            w.flush()
        })
        .create_async()
        .await;

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("{}/v1/chat/completions", server.url()))
        .total_timeout(Duration::from_millis(100))
        .build()
        .expect("build client");

    let mut stream = client
        .stream(vec![Message::user("hi")])
        .await
        .expect("stream opens");
    let mut events = Vec::new();
    while let Some(item) = stream.next().await {
        events.push(item.expect("stream must outlive total_timeout"));
    }
    // The 5-char "hello" is buffered by ThinkStripperStream until the done
    // event triggers a flush, so it surfaces as the first event here.
    assert_eq!(events.first().map(|e| e.content.as_str()), Some("hello"));
    assert!(events.last().is_some_and(|e| e.done), "terminal done must arrive");
}

// A server that accepts TCP but never writes a response: total fires during
// send() -> reqwest error with is_timeout() -> classified RetryCause::Network
// and RETRIED (specs/retry.md transport table), surfacing as
// MotosanError::Network once retries are exhausted. This pins the mapping:
// total-timeout expiry == retryable Network(timeout), NOT a distinct error.
// (Raw std listener because mockito cannot delay response headers.)
#[tokio::test]
async fn chat_total_timeout_maps_to_network_and_is_retried() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = Arc::clone(&hits);
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for conn in listener.incoming() {
            let Ok(socket) = conn else { break };
            hits_srv.fetch_add(1, Ordering::SeqCst);
            held.push(socket); // hold open, never respond
        }
    });

    let events: Arc<Mutex<Vec<motosan_ai::RetryEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |event| sink.lock().unwrap().push(event)));

    let client = Client::builder()
        .provider(Provider::OpenAI)
        .api_key("k")
        .openai_chat_url(format!("http://{addr}/v1/chat/completions"))
        .total_timeout(Duration::from_millis(200))
        .retry_policy(policy)
        .build()
        .expect("build client");

    let result = client.chat(vec![Message::user("hi")]).await;
    assert!(
        matches!(result, Err(MotosanError::Network(_))),
        "total-timeout expiry maps to Network, got {result:?}"
    );
    let recorded = std::mem::take(&mut *events.lock().unwrap());
    assert_eq!(recorded.len(), 1, "is_timeout() is retryable -> exactly one retry");
    assert!(matches!(recorded[0].cause, RetryCause::Network(_)));
    assert!(hits.load(Ordering::SeqCst) >= 2, "each attempt opens a fresh connection");
}
```

- [ ] 2. Run and confirm failure (from `sdks/rust`): `cargo test --all-features --test client_timeouts` — expect a compile error, signature: `error[E0599]: no method named \`connect_timeout\` found for struct \`ClientBuilder\`` (plus the same for `read_idle_timeout` / `total_timeout` / `Client::connect_timeout`).

- [ ] 3. Implement.
  **(a) client.rs — TimeoutConfig (reconcile the build-once const FIRST).** The build-once task added a cfg-gated connect-timeout const to client.rs (`#[cfg(any(/* 7-feature HTTP union */))] const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);`). DELETE that const together with its `#[cfg(...)]` attribute — its only usage in `build()` is rewired to `timeouts.connect` in step (e)(i) below. Leaving it in place is `error[E0428]: the name \`DEFAULT_CONNECT_TIMEOUT\` is defined multiple times` against the const added here; keeping it under another name would be dead code under `-D warnings`. In its place, after the imports (~line 7), add:
```rust
pub(crate) const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
pub(crate) const DEFAULT_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

/// Unified timeout model (one model, all SDKs — M3):
/// - `connect`: TCP/TLS connect deadline, set on the shared reqwest client.
/// - `read_idle`: max gap between HTTP stream chunks before the stream
///   yields `MotosanError::StreamReadTimeout` (supersedes the old
///   `stream_read_timeout_secs` knob — same enforcement point).
/// - `total`: opt-in wall-clock budget per blocking `chat()` attempt.
///   NEVER applied to stream request phases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimeoutConfig {
    pub(crate) connect: Duration,
    pub(crate) read_idle: Duration,
    pub(crate) total: Option<Duration>,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect: DEFAULT_CONNECT_TIMEOUT,
            read_idle: DEFAULT_READ_IDLE_TIMEOUT,
            total: None,
        }
    }
}
```
  These consts are deliberately ungated (`pub(crate)`): they are used by the ungated `TimeoutConfig::default()` and by `build()`, so no cfg gate is needed and no dead-code warning can fire.
  **(b) client.rs — Client field + getters.** Current code (approximate line 27): `    stream_read_timeout: Option<Duration>,` — Replace with: `    timeouts: TimeoutConfig,`. Current code (approximate lines 91-93):
```rust
    pub fn stream_read_timeout(&self) -> Option<Duration> {
        self.stream_read_timeout
    }
```
  Replace with (BREAKING removal of the old getter — CHANGELOG in (h)):
```rust
    pub fn connect_timeout(&self) -> Duration {
        self.timeouts.connect
    }

    pub fn read_idle_timeout(&self) -> Duration {
        self.timeouts.read_idle
    }

    pub fn total_timeout(&self) -> Option<Duration> {
        self.timeouts.total
    }
```
  **(c) client.rs — dispatch_stream.** Current code (approximate lines 377-390): the body of `dispatch_stream` with `if let Some(timeout) = self.stream_read_timeout { return Ok(Box::pin(ReadTimeoutStream::new(raw, timeout))); }` gated on a 5-feature cfg. Replace with:
```rust
    async fn dispatch_stream(&self, request: ChatRequest) -> Result<BoxStream, MotosanError> {
        let raw = self.dispatch_stream_inner(request).await?;
        #[cfg(any(
            feature = "anthropic",
            feature = "openai",
            feature = "minimax",
            feature = "ollama_native",
            feature = "gemini",
            feature = "gemini-code-assist",
            feature = "chatgpt-codex",
        ))]
        // read_idle always guards HTTP streams (default 120s). CLI backends
        // (claude-code / codex-cli / gemini-cli) keep their own per-line
        // deadline inside drive_lines and are not double-wrapped.
        if self.provider.uses_http_transport() {
            return Ok(Box::pin(ReadTimeoutStream::new(raw, self.timeouts.read_idle)));
        }
        Ok(raw)
    }
```
  Extend the three cfg gates on `ReadTimeoutStream` (struct ~1150, inherent impl ~1164, Stream impl ~1182) by adding `feature = "gemini-code-assist",` and `feature = "chatgpt-codex",` to each `#[cfg(any(...))]` list, so those HTTP providers get idle protection when built alone.
  **(d) client.rs — ClientBuilder fields + setters.** Current code (approximate line 782): `    stream_read_timeout_secs: Option<u64>,` — Replace with:
```rust
    connect_timeout: Option<Duration>,
    read_idle_timeout: Option<Duration>,
    total_timeout: Option<Duration>,
```
  Current code (approximate lines 962-965):
```rust
    pub fn stream_read_timeout_secs(mut self, secs: u64) -> Self {
        self.stream_read_timeout_secs = Some(secs);
        self
    }
```
  Replace with:
```rust
    /// TCP/TLS connect deadline for the shared HTTP client. Default: 10s.
    pub fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = Some(timeout);
        self
    }

    /// Max idle gap between HTTP stream chunks before the stream yields
    /// `MotosanError::StreamReadTimeout`. Default: 120s.
    pub fn read_idle_timeout(mut self, timeout: Duration) -> Self {
        self.read_idle_timeout = Some(timeout);
        self
    }

    /// Opt-in wall-clock budget per blocking `chat()` attempt (connect +
    /// headers + full body read; each retry gets a fresh budget). Default:
    /// off. Never applied to streams — reqwest's request timeout spans the
    /// whole body read and would kill long-lived streams.
    pub fn total_timeout(mut self, timeout: Duration) -> Self {
        self.total_timeout = Some(timeout);
        self
    }

    /// Superseded alias for [`read_idle_timeout`](Self::read_idle_timeout);
    /// writes the same knob — there is no second timeout.
    #[deprecated(since = "0.24.0", note = "use read_idle_timeout(Duration)")]
    pub fn stream_read_timeout_secs(mut self, secs: u64) -> Self {
        self.read_idle_timeout = Some(Duration::from_secs(secs));
        self
    }
```
  **(e) client.rs — build().** At the top of `build()` (after the api_key/ollama guards, before provider construction) add:
```rust
        let timeouts = TimeoutConfig {
            connect: self.connect_timeout.unwrap_or(DEFAULT_CONNECT_TIMEOUT),
            read_idle: self.read_idle_timeout.unwrap_or(DEFAULT_READ_IDLE_TIMEOUT),
            total: self.total_timeout,
        };
```
  Then (build-once seams — search, don't trust line numbers): (i) in the shared-client construction added by the build-once task, replace its reference to the (now deleted — see (a)) cfg-gated const so it reads `reqwest::Client::builder().connect_timeout(timeouts.connect)`; (ii) locate the single construction site per provider inside `ClientBuilder::build()` (search for `reqwest::Client::builder()` / the shared-client handoff added by the build-once task); do NOT hunt per-request `build_*_provider` fns (they no longer construct clients — the build-once task centralized provider construction inside `build()`). At each of those single per-provider construction sites (anthropic / openai / minimax / ollama / ollama-native / gemini / gemini-code-assist / chatgpt-codex), chain `.with_total_timeout(timeouts.total)` after `.with_retry_policy(...)` — the `timeouts` local from the snippet above is in scope right there; (iii) in the `Client { ... }` literal, replace the old `stream_read_timeout: self.stream_read_timeout_secs.map(Duration::from_secs),` (baseline ~1124) with `timeouts,`.
  **(f) providers/mod.rs — three helpers.** After the `Provider` enum (~76) add:
```rust
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
impl Provider {
    /// True when the provider speaks HTTP through the shared reqwest client;
    /// false for CLI backends, which own their per-line read deadline in
    /// `drive_lines`.
    pub(crate) fn uses_http_transport(&self) -> bool {
        !matches!(
            self,
            Provider::ClaudeCode | Provider::CodexCli | Provider::GeminiCli
        )
    }
}
```
  After `send_with_retry` (~465) add:
```rust
/// Apply the opt-in total (per-attempt) timeout to a blocking-chat request.
/// Chat paths ONLY: reqwest's `RequestBuilder::timeout()` spans the entire
/// body read, so attaching it to a stream request would kill any stream
/// outliving `total`.
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
pub(crate) fn apply_total_timeout(
    rb: reqwest::RequestBuilder,
    total: Option<Duration>,
) -> reqwest::RequestBuilder {
    match total {
        Some(t) => rb.timeout(t),
        None => rb,
    }
}

/// Total-timeout wrapper for providers whose `chat()` is stream+collect
/// (anthropic OAuth branch, gemini_code_assist, chatgpt_codex): bounds the
/// whole collect. Expiry maps to `Network` to match the reqwest
/// total-timeout surface; retries already ran inside `stream()`, so this
/// is not retried.
#[cfg(any(
    feature = "anthropic",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
pub(crate) async fn collect_stream_with_total_timeout(
    stream: BoxStream,
    total: Option<Duration>,
) -> Result<ChatResponse, MotosanError> {
    match total {
        Some(t) => match tokio::time::timeout(t, crate::stream::collect_stream(stream)).await {
            Ok(result) => result,
            Err(_) => Err(MotosanError::Network(format!(
                "total timeout: chat did not complete within {}s",
                t.as_secs()
            ))),
        },
        None => crate::stream::collect_stream(stream).await,
    }
}
```
  **(g) providers — field, setter, chat-site edits.** For EACH of the six HTTP provider structs (openai.rs ~36-49, anthropic.rs ~24-29, gemini.rs ~36-40, ollama.rs ~19-25, gemini_code_assist.rs ~46-51, chatgpt_codex.rs ~27-32): add field `total_timeout: Option<Duration>,` after `retry_policy`, add `total_timeout: None,` in `new()`, ensure `Duration` is in scope (per-file import edits below — NOT a blanket new `use` line), and add this setter directly after `with_retry_policy` (style precedent: `OllamaProvider::with_think(Option<String>)`):
```rust
    /// Opt-in wall-clock budget per blocking `chat()` attempt (connect +
    /// headers + full body read). `None` disables (default). Never applied
    /// to `stream()` — a total deadline would kill long-lived streams.
    pub fn with_total_timeout(mut self, total: Option<Duration>) -> Self {
        self.total_timeout = total;
        self
    }
```
  Imports, per file (baseline shapes verified at acf5d7f):
  - openai.rs, anthropic.rs, gemini.rs, ollama.rs — the ONLY four files that call `apply_total_timeout`: add `apply_total_timeout` to each file's existing `use crate::providers::{...}` list, and add the new line `use std::time::Duration;` (none of the four imports `std::time` today).
  - gemini_code_assist.rs — its own import edit: it already has `use std::time::{SystemTime, UNIX_EPOCH};` (~line 21); extend that line to `use std::time::{Duration, SystemTime, UNIX_EPOCH};` — do NOT add a second `use std::time::...;` line. Do NOT add `apply_total_timeout` to its `use crate::providers::{...}` list: this file never calls it (its `chat()` uses `crate::providers::collect_stream_with_total_timeout` by full path), and the unused import fails `cargo clippy --all-features --all-targets -- -D warnings` at step 5.
  - chatgpt_codex.rs — no `std::time` import exists: add the new line `use std::time::Duration;`. Same rule as gemini_code_assist.rs: do NOT import `apply_total_timeout` (no call site; full-path `collect_stream_with_total_timeout` only).

  Then edit ONLY the blocking-chat request closures. Full example — openai.rs `chat()`, current code (approximate lines 496-499):
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(self.http.post(&self.chat_url).json(&body))
        })
        .await?;
```
  Replace with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            apply_total_timeout(
                self.apply_auth(self.http.post(&self.chat_url).json(&body)),
                self.total_timeout,
            )
        })
        .await?;
```
  Apply the identical `apply_total_timeout(<existing closure expression>, self.total_timeout)` wrap at exactly these other blocking-chat sites, and NOWHERE else: openai.rs `chat_via_responses` (~247-250, wraps `self.apply_auth(self.http.post(&self.responses_url).json(&body))`); anthropic.rs `chat` (~470-479, wraps the final `self.apply_auth(request)` expression inside the closure); gemini.rs `chat` (~324-331, wraps the whole `self.apply_auth(...).json(&body)` expression); ollama.rs `chat` (~250-253, wraps `self.http.post(self.endpoint()).json(&body)`). The `stream()` closures (openai ~602, anthropic ~795, gemini ~359, ollama ~346, gemini_code_assist ~125, chatgpt_codex ~259) stay byte-for-byte UNCHANGED.
  For the three stream-backed chat() paths, replace the collect call with the helper. anthropic.rs, current code (approximate lines 459-462):
```rust
        if is_oauth {
            let stream = self.stream(req).await?;
            let mut response = crate::stream::collect_stream(stream).await?;
```
  Replace with:
```rust
        if is_oauth {
            let stream = self.stream(req).await?;
            let mut response =
                crate::providers::collect_stream_with_total_timeout(stream, self.total_timeout)
                    .await?;
```
  gemini_code_assist.rs `chat` (~113-114) and chatgpt_codex.rs `chat` (~247-248): replace `let mut response = collect_stream(stream).await?;` with `let mut response = crate::providers::collect_stream_with_total_timeout(stream, self.total_timeout).await?;`.
  **(h) CHANGELOG.** Add to `sdks/rust/CHANGELOG.md` under the Unreleased/0.24.0 heading:
```markdown
- **Breaking:** `Client::stream_read_timeout()` getter removed; `ClientBuilder::stream_read_timeout_secs` deprecated — use `read_idle_timeout(Duration)` (same knob). HTTP streams now enforce a 120s default read-idle deadline (previously none unless `stream_read_timeout_secs` was set); idle expiry yields `MotosanError::StreamReadTimeout` and is never retried mid-stream.
- Unified timeout model: `ClientBuilder::connect_timeout(Duration)` (default 10s, on the shared reqwest client), `.read_idle_timeout(Duration)` (default 120s), `.total_timeout(Duration)` (default off; bounds each blocking `chat()` attempt, never streams; expiry surfaces as retryable `MotosanError::Network`).
```

- [ ] 4. Run to pass (from `sdks/rust`): `cargo test --all-features --test client_timeouts` — all 6 tests green. Then the full package suite: `cargo test --all-features` — everything green. M2 regression contract: this task retires NO pinned behavior; `retry_conformance` (src/providers/mod.rs), `tests/openai_retry.rs`, and all M1/M2 suites must pass unchanged.

- [ ] 5. Format and lint (from `sdks/rust`): `cargo fmt` then `cargo clippy --all-features --all-targets -- -D warnings` — zero warnings (tests are linted; keep the `#[allow(deprecated)]` on the alias test).

- [ ] 6. Commit on the M3 working branch (all .rs/Cargo.toml changes go through PR + CI, never direct to main):
```
feat(client): unified timeout model — connect/read-idle/total (E4)

connect=10s on the shared reqwest client, read_idle=120s guarding HTTP
streams via ReadTimeoutStream (supersedes stream_read_timeout_secs),
total opt-in per blocking chat() attempt only. BREAKING: removes
Client::stream_read_timeout(); ships in 0.24.0.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```


## P — Python: IncompleteStreamError, timeouts + lifecycle

### Task 5: Add Python IncompleteStreamError and raise it when HTTP provider streams end without a terminal event

**Files:**
- `sdks/python/motosan_ai/error.py` (~lines 40-41)
- `sdks/python/motosan_ai/__init__.py` (~lines 4-13, ~line 82)
- `sdks/python/motosan_ai/providers/anthropic.py` (~line 10, ~lines 512-515)
- `sdks/python/motosan_ai/providers/openai.py` (~line 9, ~lines 244-264, ~lines 313-329)
- `sdks/python/motosan_ai/providers/minimax.py` (~lines 9-16, ~lines 288-292)
- `sdks/python/motosan_ai/providers/gemini.py` (~line 10, ~lines 345-352)
- `sdks/python/motosan_ai/providers/gemini_code_assist.py` (~line 13, ~lines 233-237)
- `sdks/python/motosan_ai/providers/chatgpt_codex.py` (~lines 11-17, ~lines 379-381)
- `sdks/python/motosan_ai/providers/ollama.py` (~line 10, ~lines 221-227)
- `sdks/python/tests/test_incomplete_stream.py` (new file)
- `sdks/python/CHANGELOG.md` (top)

All line refs approximate, against origin/main @ acf5d7f.

**Interfaces:**
- Consumes: `class StreamError(MotosanError)` (error.py ~40); each HTTP provider's `async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]`; `collect_stream(events)` M1 fallible-stream propagation (`_stream_collect.py` ~12-19, unchanged).
- Produces: `class IncompleteStreamError(StreamError)` exported from `motosan_ai`; raise sites use exactly `IncompleteStreamError("incomplete stream: <provider> ended without a terminal event")` with `<provider>` one of `anthropic`, `openai`, `minimax`, `gemini`, `gemini_code_assist`, `chatgpt_codex`, `ollama`.

**Scope guards (read before coding):**
- Terminal events per provider (normative: the specs/types.md terminal-event table from the E9 task, and the Rust Task 2 resolution): anthropic `message_stop` (its explicit `data: [DONE]` defensive branch at ~421-424 also terminates — keep, it is a frame not EOF); openai strictly `data: [DONE]` — a `finish_reason` chunk is NOT terminal: the adapter stashes the mapped stop_reason and keeps reading, only `[DONE]` emits the done event (carrying the stash), and EOF after finish_reason-without-`[DONE]` raises `IncompleteStreamError` with the stash discarded (mirrors the Rust openai adapter in Task 2); minimax — checked for the same defect: its chunks carry the same OpenAI-compatible `finish_reason` shape, but the baseline adapter (~288-291) is already strictly `[DONE]`-terminated (`finish_reason` only maps `tool_calls` to a non-terminal `tool_call_end`; the `[DONE]` done event deliberately carries no stop_reason — the collector heuristic fills it, E2), so hunk 3f's post-loop raise is its only change; gemini `finishReason`; gemini_code_assist `finishReason` inside the `response` wrapper; chatgpt_codex `response.completed` (`[DONE]` alone is NOT terminal); ollama `{"done": true}` NDJSON line.
- CLI providers (`claude_code.py`, `codex_cli.py`, `gemini_cli.py`) are deliberately NOT touched: since M1 they already raise `StreamError` with returncode/stderr on child-process death — child death is not HTTP truncation; the asymmetry is intentional.
- `_stream_collect.py` is NOT changed (E2): errors propagate (M1), and the stop_reason heuristic at ~77-78 stays — with adapters enforcing EOF it can only trigger on a real done event lacking a reason.
- M2 regression contract — flipped tests: **NONE in Python.** Every existing stream fixture already carries its terminal frame (verified: test_anthropic.py, test_anthropic_stream_usage.py, test_anthropic_thinking.py, test_openai.py, test_minimax.py, test_gemini_stream.py, test_code_assist_stream.py, test_chatgpt_codex_stream.py, test_chatgpt_codex_http.py, test_ollama.py, test_ollama_native.py, test_client_stream_collect.py, test_client_stream_with.py, test_client_retry_policy.py, tests/parity/test_stream_contract.py). OpenAI strict-`[DONE]` note: every openai-wire stream fixture appends the `data: [DONE]` sentinel after its `finish_reason` chunk via a shared helper (`test_openai.py::_sse_lines` ~19, `test_ollama.py` openai-compat helper ~18, `tests/parity` fixture builders ~37/~50), so under hunk 3e the done event just moves from the finish_reason chunk to the `[DONE]` frame with the same stashed stop_reason — identical observable event sequence, nothing flips. The full suite must pass unchanged. This task lands in the same PR group as the specs/types.md amendment task (E9): specs/types.md ~line 115 ("Exactly one terminal event per stream") is retired there.

**Steps:**

- [ ] 1. Write the failing test — create `sdks/python/tests/test_incomplete_stream.py` (respx + pytest.mark.asyncio, matching neighboring style; parametrization mirrors `tests/parity/conftest.py`):

```python
"""M3 stream-termination contract: EOF without the provider terminal event.

Every HTTP provider stream() must raise IncompleteStreamError (a StreamError
subclass - deliberate migration softener so existing `except StreamError`
handlers keep catching truncation) when the upstream body ends without that
provider's terminal event. CLI providers are intentionally not covered: since
M1 they raise StreamError with returncode/stderr on child-process death.
"""

from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai._stream_collect import collect_stream
from motosan_ai.error import IncompleteStreamError, StreamError
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.chatgpt_codex import ChatGptCodexProvider
from motosan_ai.providers.gemini import GeminiProvider
from motosan_ai.providers.gemini_code_assist import GeminiCodeAssistProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.providers.ollama import OllamaProvider
from motosan_ai.providers.openai import OpenAIProvider
from motosan_ai.types import ChatRequest, Message, StopReason, StreamEvent


def _sse(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


# Each case: provider factory, mocked endpoint, body ending after one text
# delta with NO terminal frame, and the <provider> token in the message.
_TRUNCATED = [
    pytest.param(
        lambda: AnthropicProvider("test-key", base_url="https://mock.anthropic.com"),
        "https://mock.anthropic.com/v1/messages",
        _sse(
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "par"},
            }
        ),
        "anthropic",
        id="anthropic",
    ),
    pytest.param(
        lambda: OpenAIProvider("test-key", base_url="https://mock.openai.com"),
        "https://mock.openai.com/v1/chat/completions",
        _sse({"choices": [{"delta": {"content": "par"}, "finish_reason": None}]}),
        "openai",
        id="openai",
    ),
    pytest.param(
        lambda: MinimaxProvider("test-key"),
        "https://api.minimax.chat/v1/text/chatcompletion_v2",
        _sse({"choices": [{"delta": {"content": "par"}}]}),
        "minimax",
        id="minimax",
    ),
    pytest.param(
        lambda: GeminiProvider(api_key="test-key"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        _sse({"candidates": [{"content": {"parts": [{"text": "par"}]}}]}),
        "gemini",
        id="gemini",
    ),
    pytest.param(
        lambda: GeminiCodeAssistProvider("ya29.fake", "myproj"),
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse",
        _sse({"response": {"candidates": [{"content": {"parts": [{"text": "par"}]}}]}}),
        "gemini_code_assist",
        id="gemini_code_assist",
    ),
    pytest.param(
        lambda: ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None),
        "https://chatgpt.com/backend-api/codex/responses",
        _sse({"type": "response.output_text.delta", "delta": "par"}),
        "chatgpt_codex",
        id="chatgpt_codex",
    ),
    pytest.param(
        lambda: OllamaProvider(),
        "http://localhost:11434/api/chat",
        '{"message":{"content":"par"},"done":false}\n',
        "ollama",
        id="ollama",
    ),
]


@respx.mock
@pytest.mark.asyncio
@pytest.mark.parametrize(("make_provider", "url", "body", "name"), _TRUNCATED)
async def test_stream_eof_without_terminal_event_raises(make_provider, url, body, name):
    respx.post(url).mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    seen = []
    with pytest.raises(
        IncompleteStreamError,
        match=f"incomplete stream: {name} ended without a terminal event",
    ):
        async for event in make_provider().stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(event)
    # Deltas received before EOF were still yielded, not swallowed.
    assert [e.content for e in seen if e.event_type == "text"] == ["par"]


@respx.mock
@pytest.mark.asyncio
async def test_codex_done_sentinel_alone_is_not_terminal():
    # [DONE] without response.completed is still truncation for chatgpt_codex.
    body = _sse({"type": "response.output_text.delta", "delta": "par"}) + "data: [DONE]\n"
    respx.post("https://chatgpt.com/backend-api/codex/responses").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)
    with pytest.raises(IncompleteStreamError, match="chatgpt_codex"):
        async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
            pass


@respx.mock
@pytest.mark.asyncio
async def test_openai_finish_reason_without_done_is_truncation():
    # finish_reason is NOT terminal for OpenAI (specs/types.md terminal-event
    # table: strictly `data: [DONE]`). EOF after a finish_reason chunk without
    # the sentinel is truncation; the stashed stop_reason is discarded.
    body = _sse(
        {"choices": [{"delta": {"content": "par"}, "finish_reason": None}]},
        {"choices": [{"delta": {}, "finish_reason": "stop"}]},
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = OpenAIProvider("test-key", base_url="https://mock.openai.com")
    seen = []
    with pytest.raises(
        IncompleteStreamError,
        match="incomplete stream: openai ended without a terminal event",
    ):
        async for event in p.stream(ChatRequest(messages=[Message.user("hi")])):
            seen.append(event)
    assert [e.content for e in seen if e.event_type == "text"] == ["par"]
    assert not any(e.done for e in seen)  # no done event fabricated from finish_reason


@respx.mock
@pytest.mark.asyncio
async def test_openai_finish_reason_then_done_completes_with_stashed_stop_reason():
    # Happy path: the finish_reason-derived stop_reason is stashed and emitted
    # with the [DONE] done event — [DONE] is OpenAI's only terminal event.
    body = (
        _sse(
            {"choices": [{"delta": {"content": "hi"}, "finish_reason": None}]},
            {"choices": [{"delta": {}, "finish_reason": "stop"}]},
        )
        + "data: [DONE]\n"
    )
    respx.post("https://mock.openai.com/v1/chat/completions").mock(
        return_value=httpx.Response(200, text=body, headers={"content-type": "text/event-stream"})
    )
    p = OpenAIProvider("test-key", base_url="https://mock.openai.com")
    events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
    dones = [e for e in events if e.done]
    assert len(dones) == 1 and events[-1] is dones[0]
    assert dones[0].stop_reason == StopReason.stop


def test_incomplete_stream_error_is_stream_error_subclass():
    # E1 migration softener: pre-existing `except StreamError` call sites keep
    # catching truncation; MotosanError metadata kwargs are inherited.
    err = IncompleteStreamError("incomplete stream: openai ended without a terminal event")
    assert isinstance(err, StreamError)
    assert err.status_code is None and err.retry_after is None and err.request_id is None


def test_incomplete_stream_error_exported_from_top_level():
    import motosan_ai

    assert motosan_ai.IncompleteStreamError is IncompleteStreamError


@pytest.mark.asyncio
async def test_collect_stream_propagates_incomplete_stream_error():
    # E2: collector keeps M1 fallible-stream propagation - no fallback, no
    # partial ChatResponse. (Sibling: test_client_stream_collect.py::
    # test_collect_stream_propagates_mid_stream_raise.)
    async def _truncated():
        yield StreamEvent(content="partial", done=False)
        raise IncompleteStreamError("incomplete stream: anthropic ended without a terminal event")

    with pytest.raises(IncompleteStreamError):
        await collect_stream(_truncated())
```

- [ ] 2. Run and confirm failure — from `sdks/python`: `uv run pytest tests/test_incomplete_stream.py -v`. Expected: collection error `ImportError: cannot import name 'IncompleteStreamError' from 'motosan_ai.error'`.

- [ ] 3. Implement.

  **3a. `error.py`** — Current code (approximate lines 40-41):
```python
class StreamError(MotosanError):
    pass
```
  Replace with:
```python
class StreamError(MotosanError):
    pass


class IncompleteStreamError(StreamError):
    """The upstream ended without the provider's terminal event.

    Deliberately subclasses StreamError as a migration softener: existing
    ``except StreamError`` handlers keep catching truncation; catch this type
    to handle truncation specifically. Message convention:
    ``"incomplete stream: <provider> ended without a terminal event"``.
    """
```

  **3b. `__init__.py`** — in the `from motosan_ai.error import (...)` block (~lines 4-13) insert `IncompleteStreamError,` after `ConfigError,`; in `__all__` (~line 82) insert `"IncompleteStreamError",` after `"ImageSourceUrl",` (alphabetical, before `"InvalidRequestError"`).

  **3c. Provider imports** — add `IncompleteStreamError` to each provider's `from motosan_ai.error import ...`. For `anthropic.py` (~10), `openai.py` (~9), `gemini.py` (~10), `gemini_code_assist.py` (~13) the single-line import exceeds 100 chars, so replace with the parenthesized form:
```python
from motosan_ai.error import (
    AuthError,
    IncompleteStreamError,
    NetworkError,
    ProviderError,
    RateLimitError,
    StreamError,
)
```
  For `minimax.py` (~9-16) and `chatgpt_codex.py` (~11-17) (already multiline): insert `    IncompleteStreamError,` after `    AuthError,`. For `ollama.py` (~10): `from motosan_ai.error import NetworkError, ProviderError, StreamError` becomes `from motosan_ai.error import IncompleteStreamError, NetworkError, ProviderError, StreamError`.

  **3d. `anthropic.py`** — Current code (approximate lines 512-515, end of the SSE loop):
```python
                elif event_type == "message_stop":
                    yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
                    return
        except StreamError:
```
  Replace with:
```python
                elif event_type == "message_stop":
                    yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
                    return

            raise IncompleteStreamError(
                "incomplete stream: anthropic ended without a terminal event"
            )
        except StreamError:
```

  **3e. `openai.py`** — strict-`[DONE]` termination, two hunks. The locked contract (Task 1 terminal-event table; Rust Task 2 resolution) makes `data: [DONE]` OpenAI's ONLY terminal event: a `finish_reason` chunk stashes its mapped stop_reason and the loop keeps reading; `[DONE]` emits the done event carrying the stash; EOF without `[DONE]` — even after a `finish_reason` chunk — raises `IncompleteStreamError` (the stash is discarded: without `[DONE]` the stream did not terminate).

  First hunk — Current code (approximate lines 244-264, loop state + `[DONE]` branch):
```python
            # Per-index tool-call tracking (mirrors TS providers/openai.ts):
            # index -> (id, name); only one tool call is open at a time.
            tool_buffer: dict[int, tuple[str, str]] = {}
            open_tool_index: int | None = None

            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                data = line[6:].strip()
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
  Replace with:
```python
            # Per-index tool-call tracking (mirrors TS providers/openai.ts):
            # index -> (id, name); only one tool call is open at a time.
            tool_buffer: dict[int, tuple[str, str]] = {}
            open_tool_index: int | None = None
            # M3: stop_reason stashed from finish_reason chunks, emitted with
            # the [DONE] done event -- [DONE] is OpenAI's only terminal event.
            current_stop_reason: StopReason | None = None

            async for line in resp.aiter_lines():
                if not line.startswith("data: "):
                    continue
                data = line[6:].strip()
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
                        yield StreamEvent(content="", done=True, stop_reason=current_stop_reason)
                        return
                    continue
```
  Second hunk — Current code (approximate lines 313-329, finish_reason branch then except):
```python
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
        except StreamError:
```
  Replace with:
```python
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
                        # NOT terminal: stash and keep reading until [DONE].
                        current_stop_reason = _FINISH_REASON_TO_STOP.get(
                            finish_reason, StopReason.other
                        )

            raise IncompleteStreamError(
                "incomplete stream: openai ended without a terminal event"
            )
        except StreamError:
```
  (The tool_call_end close on `finish_reason == "tool_calls"` is kept where the chunk arrives; the `[DONE]` branch's own close then no-ops because `open_tool_index` is already `None`. `StopReason` is already imported — the `_FINISH_REASON_TO_STOP` fallback uses it.)

  **3f. `minimax.py`** — Current code (approximate lines 288-292, end of loop inside `async with`):
```python
                    finish_reason = choice.get("finish_reason")
                    if finish_reason == "tool_calls":
                        yielded = True
                        yield StreamEvent(content="", done=False, event_type="tool_call_end")
        except (AuthError, RateLimitError, InvalidRequestError, ProviderError, StreamError):
```
  Replace with:
```python
                    finish_reason = choice.get("finish_reason")
                    if finish_reason == "tool_calls":
                        yielded = True
                        yield StreamEvent(content="", done=False, event_type="tool_call_end")

                raise IncompleteStreamError(
                    "incomplete stream: minimax ended without a terminal event"
                )
        except (AuthError, RateLimitError, InvalidRequestError, ProviderError, StreamError):
```

  **3g. `gemini.py`** — Current code (approximate lines 345-352):
```python
                if finish_reason:
                    yield StreamEvent(
                        content="",
                        done=True,
                        stop_reason=_stop_reason_for(finish_reason, has_tool_calls),
                    )
                    return
        except StreamError:
```
  Replace with:
```python
                if finish_reason:
                    yield StreamEvent(
                        content="",
                        done=True,
                        stop_reason=_stop_reason_for(finish_reason, has_tool_calls),
                    )
                    return

            raise IncompleteStreamError(
                "incomplete stream: gemini ended without a terminal event"
            )
        except StreamError:
```

  **3h. `gemini_code_assist.py`** — Current code (approximate lines 233-237, inner try):
```python
                    for event in _parse_sse_event(data, state):
                        yield event
                        if event.done:
                            return
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
```
  Replace with:
```python
                    for event in _parse_sse_event(data, state):
                        yield event
                        if event.done:
                            return

                raise IncompleteStreamError(
                    "incomplete stream: gemini_code_assist ended without a terminal event"
                )
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
```

  **3i. `chatgpt_codex.py`** — Current code (approximate lines 379-381, inner try):
```python
                    if state.error is not None:
                        raise StreamError(state.error)
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
```
  Replace with:
```python
                    if state.error is not None:
                        raise StreamError(state.error)

                raise IncompleteStreamError(
                    "incomplete stream: chatgpt_codex ended without a terminal event"
                )
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
```

  **3j. `ollama.py`** — Current code (approximate lines 221-227, last tool_call_end yield then except):
```python
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=tc_id,
                            event_type="tool_call_end",
                        )
        except (ProviderError, StreamError):
```
  Replace with:
```python
                        yield StreamEvent(
                            content="",
                            done=False,
                            tool_call_id=tc_id,
                            event_type="tool_call_end",
                        )

                raise IncompleteStreamError(
                    "incomplete stream: ollama ended without a terminal event"
                )
        except (ProviderError, StreamError):
```
  Note the raise sits at the same indent as each file's read loop (`async for`), inside any `async with`/inner `try`, so it is re-raised by the file's `except ... StreamError` pass-through and still runs the `finally: await resp.aclose()` cleanup where present.

  **3k. `CHANGELOG.md`** — insert directly under the `# Changelog` intro, above `## [0.16.0]`:
```markdown
## [Unreleased]

### Breaking
- **Streaming:** HTTP provider streams that end without the provider's terminal event (`message_stop` / `[DONE]` / `finishReason` / `response.completed` / `{"done": true}`) now raise `IncompleteStreamError` instead of ending silently as if complete — truncation is no longer indistinguishable from completion. `IncompleteStreamError` subclasses `StreamError` deliberately (migration softener): existing `except StreamError` handlers keep catching it; catch `IncompleteStreamError` to handle truncation specifically. Not retried (mid-stream errors are never retried, per `specs/retry.md`). CLI providers unchanged — child-process death already raises `StreamError` (M1); child death is not HTTP truncation.
- **OpenAI streaming:** a `finish_reason` chunk is no longer terminal — per the terminal-event contract (`specs/types.md`), only `data: [DONE]` completes an OpenAI stream. The finish_reason-derived `stop_reason` is stashed and emitted with the `[DONE]` done event; EOF after `finish_reason` without `[DONE]` raises `IncompleteStreamError`. Mirrors the Rust 0.24.0 adapter.
```

- [ ] 4. Run to pass, then the package suite — from `sdks/python`: `uv run pytest tests/test_incomplete_stream.py -v` (13 passed: 7 parametrized + 6 singles), then `uv run pytest` (full suite; live/integration tests self-skip without keys). All M1 retry tests and M2 conformance/parity suites must pass unchanged — zero flips expected (see Scope guards).

- [ ] 5. Format and lint — from `sdks/python`: `uv run ruff format` then `uv run ruff check motosan_ai/` (tests/ is not linted). Re-run `uv run pytest tests/test_incomplete_stream.py -v` if the formatter touched provider files.

- [ ] 6. Commit on the milestone feature branch (PR + CI per house rules):
```
feat(python)!: raise IncompleteStreamError on stream EOF without terminal event

HTTP provider streams (anthropic, openai, minimax, gemini,
gemini_code_assist, chatgpt_codex, ollama) now raise
IncompleteStreamError("incomplete stream: <provider> ended without a
terminal event") when the upstream body ends without the provider's
terminal frame, instead of ending silently as if complete.
IncompleteStreamError subclasses StreamError (migration softener).

OpenAI is now strictly [DONE]-terminated per the specs/types.md
terminal-event table: a finish_reason chunk stashes its mapped
stop_reason (emitted with the [DONE] done event) instead of
terminating the stream; EOF after finish_reason without [DONE] is
truncation. Existing openai-wire fixtures all append [DONE], so the
observable event sequence is unchanged.

collect_stream unchanged: it propagates the error (M1). CLI providers
unchanged: child death already raises StreamError; not HTTP truncation.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```

### Task 6: Add unified timeout model, StreamReadTimeoutError, and client lifecycle to the Python SDK (E4+E8)

> **Ordering:** Execute AFTER Task 5 — it already rewrote the provider error-import lines (adding `IncompleteStreamError`). MERGE the `StreamReadTimeoutError` import into those same lines; do not duplicate or clobber them.

All paths repo-relative; all line numbers approximate (baseline: origin/main @ acf5d7f, Python 0.16.0). Work from `sdks/python/`.

**Files:**
- `sdks/python/tests/test_client_timeouts.py` (new)
- `sdks/python/motosan_ai/error.py` (~40-41)
- `sdks/python/motosan_ai/__init__.py` (~4-13, ~99-101)
- `sdks/python/motosan_ai/client.py` (~1-9, ~26, ~69-84, ~86-152, ~264-294, ~321-329, ~352-369)
- `sdks/python/motosan_ai/providers/anthropic.py` (~10, ~92-102, ~515-522)
- `sdks/python/motosan_ai/providers/openai.py` (~9, ~48-52, ~329-336)
- `sdks/python/motosan_ai/providers/gemini.py` (~10, ~172-181, ~279-289, ~352-359)
- `sdks/python/motosan_ai/providers/minimax.py` (~9-16, ~43-47, ~292-297)
- `sdks/python/motosan_ai/providers/ollama.py` (~10, ~37-50, ~227-232)
- `sdks/python/motosan_ai/providers/gemini_code_assist.py` (~13, ~163-174, ~237-242)
- `sdks/python/motosan_ai/providers/chatgpt_codex.py` (~10-17, ~192-204, ~381-386)
- `sdks/python/motosan_ai/providers/codex_cli.py` (~292-299)
- `sdks/python/motosan_ai/providers/gemini_cli.py` (~267-274)
- `sdks/python/motosan_ai/providers/claude_code.py` (~409-417)
- `sdks/python/CHANGELOG.md` (top)

**Interfaces:**
- Produces: `Client(provider, ..., *, connect_timeout: float = 10.0, read_idle_timeout: float = 120.0, total_timeout: float | None = None, cli_timeout: float | None = _UNSET_CLI_TIMEOUT)` — E4/E8 spellings verbatim.
- Produces (every HTTP provider): `httpx.AsyncClient(timeout=httpx.Timeout(connect=connect_timeout, read=read_idle_timeout, write=read_idle_timeout, pool=connect_timeout))`.
- Produces: `class StreamReadTimeoutError(MotosanError)` in `motosan_ai/error.py`, exported from `motosan_ai` (mirrors Rust `StreamReadTimeout` / TS `StreamReadTimeoutError`).
- Produces: `async def aclose(self) -> None` on `Client` and every provider; `Client.__aenter__/__aexit__`.
- Consumes: `CodexCliClient.timeout(secs: float)` / `.no_timeout()` (codex_cli.py ~292-299); `GeminiCliClient.timeout(seconds: float)` / `.no_timeout()` (gemini_cli.py ~267-274); `RetryPolicy` (retry.py ~68); `Client.stream_with`'s retry catch tuple `except (RateLimitError, NetworkError, ProviderError)` (client.py ~422).

Retry-interaction facts (verified against real code — the tests below pin them): `StreamReadTimeoutError` subclasses `MotosanError` directly, so it is caught by NEITHER `stream_with`'s tuple nor `retry._is_retryable` → never retried, pre- or mid-stream. Non-stream `chat()` keeps mapping `httpx.ReadTimeout` to `NetworkError` (retryable) — only streaming paths change. No existing test pins the old mid-stream ReadTimeout→`StreamError` mapping (grep of `tests/` for `ReadTimeout` / `stream transport error` is clean), so no M1/M2 conformance flips are needed.

**Steps:**

- [ ] 1. Write the failing test file `sdks/python/tests/test_client_timeouts.py` (complete; matches neighboring respx + monkeypatch style, `asyncio_mode=auto` markers kept for consistency):

```python
"""E4+E8 (M3): unified timeout model, StreamReadTimeoutError, client lifecycle."""

import asyncio

import httpx
import pytest
import respx

from motosan_ai import Client, Message, Provider
from motosan_ai.error import NetworkError, StreamReadTimeoutError
from motosan_ai.providers import (
    ChatGptCodexProvider,
    GeminiCodeAssistProvider,
    GeminiProvider,
    OpenAIProvider,
)
from motosan_ai.retry import RetryPolicy
from motosan_ai.types import ChatRequest, ChatResponse, StopReason, StreamEvent, Usage

_OPENAI_URL = "https://api.openai.com/v1/chat/completions"


class _TimeoutAfterChunks(httpx.AsyncByteStream):
    """Yield the given chunks, then raise httpx.ReadTimeout (idle expiry)."""

    def __init__(self, chunks: list[bytes]) -> None:
        self._chunks = chunks

    async def __aiter__(self):
        for chunk in self._chunks:
            yield chunk
        raise httpx.ReadTimeout("read timed out")


def test_provider_timeout_kwargs_map_to_httpx_timeout():
    provider = OpenAIProvider(api_key="k", connect_timeout=1.5, read_idle_timeout=3.0)
    assert provider._http.timeout == httpx.Timeout(connect=1.5, read=3.0, write=3.0, pool=1.5)


def test_minimax_30s_outlier_is_unified(monkeypatch):
    monkeypatch.setenv("MINIMAX_API_KEY", "m")
    client = Client(Provider.minimax)
    assert client._provider._client.timeout == httpx.Timeout(
        connect=10.0, read=120.0, write=120.0, pool=10.0
    )


@respx.mock
@pytest.mark.asyncio
async def test_mid_stream_read_timeout_is_distinct_and_never_retried(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    chunk = b'data: {"choices":[{"delta":{"content":"hi"}}]}\n\n'
    route = respx.post(_OPENAI_URL).mock(
        return_value=httpx.Response(
            200,
            stream=_TimeoutAfterChunks([chunk]),
            headers={"content-type": "text/event-stream"},
        )
    )
    client = Client(Provider.openai, retry_policy=RetryPolicy(max_retries=3, base_delay=0.001))
    seen = []
    with pytest.raises(StreamReadTimeoutError, match="stream read timed out after 120"):
        async for event in client.stream([Message.user("hi")]):
            seen.append(event)
    assert any(e.content == "hi" for e in seen)
    assert route.call_count == 1  # a retry would replay already-yielded deltas


# Providers whose non-2xx error-body `await resp.aread()` sat outside the
# ReadTimeout-mapping scope at baseline (gemini: outside the try/finally
# entirely; gemini_code_assist/chatgpt_codex: outside the inner catch chain).
_ERROR_BODY_TIMEOUT_CASES = [
    pytest.param(
        lambda: GeminiProvider(api_key="k"),
        "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:streamGenerateContent?alt=sse",
        id="gemini",
    ),
    pytest.param(
        lambda: GeminiCodeAssistProvider("ya29.fake", "myproj"),
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse",
        id="gemini_code_assist",
    ),
    pytest.param(
        lambda: ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None),
        "https://chatgpt.com/backend-api/codex/responses",
        id="chatgpt_codex",
    ),
]


@respx.mock
@pytest.mark.asyncio
@pytest.mark.parametrize(("make_provider", "url"), _ERROR_BODY_TIMEOUT_CASES)
async def test_non_2xx_error_body_read_timeout_maps_to_stream_read_timeout(make_provider, url):
    # Idle expiry while reading a non-2xx error body must map to
    # StreamReadTimeoutError like any other post-header ReadTimeout (and the
    # response must be cleaned up), not escape as a raw httpx.ReadTimeout.
    respx.post(url).mock(return_value=httpx.Response(500, stream=_TimeoutAfterChunks([b"boom"])))
    with pytest.raises(StreamReadTimeoutError, match="stream read timed out after 120"):
        async for _ in make_provider().stream(ChatRequest(messages=[Message.user("hi")])):
            pass


class _SlowProvider:
    async def chat(self, request):
        await asyncio.sleep(0.5)
        return ChatResponse(content="late", usage=Usage(1, 1), stop_reason=StopReason.stop)

    async def stream(self, request):
        await asyncio.sleep(0.1)
        yield StreamEvent(content="slow", done=False)
        yield StreamEvent(content="", done=True)

    async def aclose(self):
        pass


@pytest.mark.asyncio
async def test_total_timeout_bounds_chat(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai, total_timeout=0.05)
    client._provider = _SlowProvider()
    with pytest.raises(NetworkError, match="total timeout of 0.05s exceeded"):
        await client.chat([Message.user("hi")])


@pytest.mark.asyncio
async def test_total_timeout_never_applies_to_streams(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai, total_timeout=0.05)
    client._provider = _SlowProvider()
    events = [e async for e in client.stream([Message.user("hi")])]
    assert [e.content for e in events] == ["slow", ""]


@pytest.mark.asyncio
async def test_aclose_closes_provider_pool(monkeypatch):
    monkeypatch.setenv("OPENAI_API_KEY", "k")
    client = Client(Provider.openai)
    assert client._provider._http.is_closed is False
    await client.aclose()
    assert client._provider._http.is_closed is True


@pytest.mark.asyncio
async def test_async_context_manager_round_trip(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    async with Client(Provider.anthropic) as client:
        assert client._provider._http.is_closed is False
    assert client._provider._http.is_closed is True


@pytest.mark.asyncio
async def test_cli_timeout_threading():
    assert Client(Provider.codex_cli, cli_timeout=60.0)._provider._config.timeout_secs == 60.0
    assert Client(Provider.gemini_cli, cli_timeout=None)._provider._config.timeout_secs is None
    assert Client(Provider.codex_cli)._provider._config.timeout_secs == 600.0  # default kept
    await Client(Provider.gemini_cli).aclose()  # CLI aclose is a reachable no-op
```

- [ ] 2. Run and watch it fail: `cd sdks/python && uv run pytest tests/test_client_timeouts.py -v` — expect a collection-time failure: `ImportError: cannot import name 'StreamReadTimeoutError' from 'motosan_ai.error'`.

- [ ] 3. Implement:

  **3a — error.py.** Current code (approximate lines 40-41):
```python
class StreamError(MotosanError):
    pass
```
  Replace with:
```python
class StreamError(MotosanError):
    pass


class StreamReadTimeoutError(MotosanError):
    """A streaming response body went idle past ``read_idle_timeout``.

    Raised after response headers arrive when no bytes show up within the
    idle window. Deliberately not a NetworkError: it is never retried —
    mid-stream, a retry would replay already-yielded deltas (specs/retry.md
    streaming rule). Mirrors Rust ``StreamReadTimeout`` / TS
    ``StreamReadTimeoutError``.
    """
```

  **3b — motosan_ai/__init__.py.** In the `from motosan_ai.error import (...)` block (~4-13), add `StreamReadTimeoutError,` on its own line after `StreamError,`. In `__all__` (~99-101), add `"StreamReadTimeoutError",` after `"StreamEventType",` (alphabetical: StreamError < StreamEvent < StreamEventType < StreamReadTimeoutError).

  **3c — client.py imports, sentinel, signature.** In the import block (~1-9) insert `from types import TracebackType` between `from enum import StrEnum` and `from typing import Any`. After `logger = logging.getLogger(__name__)` (~26) add:
```python
# cli_timeout sentinel: distinguishes "not passed" (keep the CLI provider's
# 600s default) from cli_timeout=None (which maps to .no_timeout()).
_UNSET_CLI_TIMEOUT: Any = object()
```
  In `Client.__init__`'s keyword-only section (~69-77), insert after `retry_policy: RetryPolicy | None = None,` and before `) -> None:`:
```python
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
        total_timeout: float | None = None,
        cli_timeout: float | None = _UNSET_CLI_TIMEOUT,
```
  (This lands AFTER the existing `*` and after `retry_policy`, the current last parameter — nothing shifts position for existing callers.) After `self._max_retries = self._retry_policy.max_retries` (~84) add `self._total_timeout = total_timeout`.

  **3d — client.py provider construction.** Current code (approximate lines 110-115):
```python
        elif provider_value == Provider.codex_cli:
            self.api_key = ""
            self._provider = CodexCliClient(binary_path=binary_path)
        elif provider_value == Provider.gemini_cli:
            self.api_key = ""
            self._provider = GeminiCliClient(binary_path=binary_path)
```
  Replace with:
```python
        elif provider_value == Provider.codex_cli:
            self.api_key = ""
            self._provider = self._apply_cli_timeout(
                CodexCliClient(binary_path=binary_path), cli_timeout
            )
        elif provider_value == Provider.gemini_cli:
            self.api_key = ""
            self._provider = self._apply_cli_timeout(
                GeminiCliClient(binary_path=binary_path), cli_timeout
            )
```
  Then append `connect_timeout=connect_timeout, read_idle_timeout=read_idle_timeout,` as the final keyword arguments INSIDE each HTTP provider constructor call: `GeminiCodeAssistProvider(...)` ~92-97, `ChatGptCodexProvider(...)` ~104-109 (inside the constructor parens, before the chained `.reasoning_effort(...)`), `NativeOllamaProvider(...)` ~121-127, ollama-compat `OpenAIProvider(...)` ~129-133, `AnthropicProvider(...)` ~140, `OpenAIProvider(...)` ~142-144, `GeminiProvider(...)` ~146-148, `MinimaxProvider(...)` ~150-152. Example — current code (approximate lines 139-140):
```python
            if provider_value == Provider.anthropic:
                self._provider = AnthropicProvider(api_key=self.api_key, model=model)
```
  Replace with:
```python
            if provider_value == Provider.anthropic:
                self._provider = AnthropicProvider(
                    api_key=self.api_key,
                    model=model,
                    connect_timeout=connect_timeout,
                    read_idle_timeout=read_idle_timeout,
                )
```
  In the `codex_cli` and `gemini_cli` classmethods (~264-294), add `cli_timeout` as the LAST parameter — NOT before `max_retries` or `retry_policy`. These classmethods have no `*` marker, so every existing parameter is positional-or-keyword; inserting earlier would silently shift the positional index of `max_retries`/`retry_policy` for external callers (the in-repo suite only calls them with kwargs, so CI would not catch it). Current code (approximate lines 264-278):
```python
    @classmethod
    def codex_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
        retry_policy: RetryPolicy | None = None,
    ) -> Client:
        return cls(
            provider=Provider.codex_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
            retry_policy=retry_policy,
        )
```
  Replace with:
```python
    @classmethod
    def codex_cli(
        cls,
        binary_path: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
        retry_policy: RetryPolicy | None = None,
        cli_timeout: float | None = _UNSET_CLI_TIMEOUT,
    ) -> Client:
        return cls(
            provider=Provider.codex_cli,
            binary_path=binary_path,
            model=model,
            max_retries=max_retries,
            retry_policy=retry_policy,
            cli_timeout=cli_timeout,
        )
```
  Apply the identical edit to `gemini_cli` (~280-294), swapping `Provider.gemini_cli`. `cli_timeout` is keyword-only-by-position here (nobody passes five positionals to these); do NOT add a `*` before it — retrofitting a keyword-only marker onto existing positional-or-keyword params is a separate API decision outside this task. Signature-compat audit for everything else this task touches: `Client.__init__` additions sit in the existing keyword-only section after the current last param (3c), and every provider `__init__` addition sits behind a new trailing `*,` (3f) — no existing parameter shifts position anywhere in this task.

  **3e — client.py lifecycle + total_timeout.** Insert directly after `_load_api_key` (~329):
```python
    @staticmethod
    def _apply_cli_timeout(
        cli: CodexCliClient | GeminiCliClient,
        cli_timeout: float | None,
    ) -> CodexCliClient | GeminiCliClient:
        if cli_timeout is _UNSET_CLI_TIMEOUT:
            return cli
        if cli_timeout is None:
            return cli.no_timeout()
        return cli.timeout(cli_timeout)

    async def aclose(self) -> None:
        """Close the provider's underlying connection pool (idempotent)."""
        await self._provider.aclose()

    async def __aenter__(self) -> Client:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        tb: TracebackType | None,
    ) -> None:
        await self.aclose()
```
  Current code (approximate lines 352-369) — `chat_with` from its docstring through `return await self._provider.chat(request)`. Replace with the block below. The docstring's final sentence (the `total_timeout` line) is the only new docstring text; everything above it is the existing docstring, kept verbatim:
```python
        """Send a fully-built ChatRequest.

        Use this when you need fields that ``chat()`` kwargs do not expose,
        such as tool_choice, thinking, mcp_servers, system_blocks, or
        stop_sequences. If ``request.model`` is None, ``self.model`` is used.
        ``total_timeout`` bounds blocking calls like this one and ``chat()``
        (retries included), never stream consumption; ``None`` disables it.
        """
        if request.model is None and self.model is not None:
            request = replace(request, model=self.model)

        if self._total_timeout is None:
            return await self._dispatch_chat(request)
        # E4: total_timeout is opt-in, bounds the WHOLE call (retries
        # included), applies to chat only — never silently to streams. The
        # expiry is raised outside the retry loop, so it is not retried.
        try:
            async with asyncio.timeout(self._total_timeout):
                return await self._dispatch_chat(request)
        except TimeoutError as exc:
            raise NetworkError(f"total timeout of {self._total_timeout}s exceeded") from exc

    async def _dispatch_chat(self, request: ChatRequest) -> ChatResponse:
        if self._retry_policy.max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(
                lambda: self._provider.chat(request),
                policy=self._retry_policy,
            )
        return await self._provider.chat(request)
```

  **3f — HTTP provider constructors (7 files).** Current code (anthropic.py, approximate lines 92-102):
```python
    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
    ) -> None:
        self.api_key = api_key
        self.model = model or "claude-sonnet-4-6"
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._is_oauth = api_key.startswith("sk-ant-oat01-")
        self._http = httpx.AsyncClient(timeout=120.0)
```
  Replace with:
```python
    def __init__(
        self,
        api_key: str,
        model: str | None = None,
        base_url: str | None = None,
        *,
        connect_timeout: float = 10.0,
        read_idle_timeout: float = 120.0,
    ) -> None:
        self.api_key = api_key
        self.model = model or "claude-sonnet-4-6"
        self.base_url = (base_url or _DEFAULT_BASE_URL).rstrip("/")
        self._is_oauth = api_key.startswith("sk-ant-oat01-")
        self._read_idle_timeout = read_idle_timeout
        self._http = httpx.AsyncClient(
            timeout=httpx.Timeout(
                connect=connect_timeout,
                read=read_idle_timeout,
                write=read_idle_timeout,
                pool=connect_timeout,
            )
        )

    async def aclose(self) -> None:
        """Close the underlying HTTP connection pool."""
        await self._http.aclose()
```
  Apply the identical edit (same keyword-only params after a `*,`, same `self._read_idle_timeout` line, same `httpx.Timeout(...)` body, same `aclose`) to each remaining constructor, keeping every existing assignment line: openai.py ~48-52 (`timeout=120.0` at ~52), gemini.py ~172-181 (~181), ollama.py ~37-50 (~50; its positional params `model, base_url, think, keep_alive, num_ctx` keep their defaults — add `*,` only if not present before the new params), gemini_code_assist.py ~163-174 (~174), chatgpt_codex.py ~192-204 (~204), minimax.py ~43-47 (~47 — the 30s outlier; attribute is `self._client`, so its `aclose` awaits `self._client.aclose()`).

  **3g — stream ReadTimeout mapping (7 files).** In each provider file, add `StreamReadTimeoutError` to the existing `from motosan_ai.error import (...)` line/block (anthropic.py ~10, openai.py ~9, gemini.py ~10, gemini_code_assist.py ~13, chatgpt_codex.py ~10-17, minimax.py ~9-16, ollama.py ~10). Then insert this branch immediately BEFORE the mid-stream `except httpx.HTTPError as exc:` in each `stream()` catch chain (order matters — `ReadTimeout` subclasses `HTTPError`):
```python
        except httpx.ReadTimeout as exc:
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
```
  Plain insertion sites (do NOT touch the separate `send()`-phase `except httpx.HTTPError: raise NetworkError` blocks at anthropic ~403, openai ~233, gemini ~279, gemini_code_assist ~219, chatgpt_codex ~357 — connection/header-phase timeouts stay NetworkError/retryable there): anthropic.py ~519 (between the `except (AuthError, RateLimitError, ProviderError, NetworkError): raise` pair and `except httpx.HTTPError`), openai.py ~333, gemini.py ~356 (AFTER applying restructure hunk (i) below), minimax.py ~294 (before its `except httpx.HTTPError` that splits on `yielded`), ollama.py ~229 (same). gemini_code_assist.py and chatgpt_codex.py are handled by full hunks (ii)/(iii) below instead — their insertion is combined with the error-body-read coverage fix. Nuance to note in the PR: minimax/ollama use a single try covering the header phase too, so a header-phase ReadTimeout (previously NetworkError, retryable) now raises StreamReadTimeoutError for those two — deliberate: an idle expiry is a timeout signal, and the upstream already accepted the request.

  **Non-2xx error-body reads — every `await resp.aread()` site must sit inside the mapping/cleanup scope.** A ReadTimeout can also fire while reading a non-2xx *error body* (post-header, so the send()-phase NetworkError mapping does not apply). anthropic (~410-415) and openai (~237-242) already run that `aread()` inside the same single `try` the branch above lands in (its `finally` closes resp), and minimax (~236) / ollama (~173) run it inside their `async with` + single `try` — those four are covered by the plain insertions alone. Three files need structural fixes (the new parametrized test in step 1 pins all three):

  (i) `gemini.py` — the baseline runs the non-2xx `aread()` BEFORE the stream `try`/`finally`: a ReadTimeout during the error-body read would bypass both the new mapping and response cleanup, and `resp` is never closed on the non-2xx path at all (pre-existing leak). Move the block inside the `try`. Current code (approximate lines 279-289; this region sits above the SSE loop and is untouched by Task 5):
```python
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc
        if not resp.is_success:
            error_body = await resp.aread()
            message = self._response_error_message(
                resp.status_code, resp.headers, error_body.decode()
            )
            raise self._map_http_error(resp.status_code, message, resp.headers)

        try:
            async for line in resp.aiter_lines():
```
  Replace with:
```python
        except httpx.HTTPError as exc:
            raise NetworkError(str(exc)) from exc

        try:
            if not resp.is_success:
                error_body = await resp.aread()
                message = self._response_error_message(
                    resp.status_code, resp.headers, error_body.decode()
                )
                raise self._map_http_error(resp.status_code, message, resp.headers)

            async for line in resp.aiter_lines():
```
  (This matches the anthropic/openai layout. The mapped non-2xx errors pass through the existing `except (AuthError, RateLimitError, ProviderError, NetworkError): raise` arm, and `finally: await resp.aclose()` now also covers the non-2xx path.)

  (ii) `gemini_code_assist.py` — its non-2xx `aread()` (~224) sits in the OUTER try (so `finally` already closes resp) but OUTSIDE the inner catch chain where the mapping branch goes, so a ReadTimeout there would escape raw. Add the inner branch AND an outer catch. Current code (approximate lines 237-242; Task 5's post-loop raise lands above this region and is unaffected):
```python
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        finally:
            await resp.aclose()
```
  Replace with:
```python
            except (StreamError, AuthError, RateLimitError, ProviderError, NetworkError):
                raise
            except httpx.ReadTimeout as exc:
                raise StreamReadTimeoutError(
                    f"stream read timed out after {self._read_idle_timeout}s"
                ) from exc
            except httpx.HTTPError as exc:
                raise StreamError(f"stream transport error: {exc}") from exc
        except httpx.ReadTimeout as exc:
            # Only reachable from the non-2xx error-body aread() above: loop
            # ReadTimeouts are already mapped by the inner chain, and the
            # StreamReadTimeoutError it raises is a MotosanError that passes
            # through this except untouched.
            raise StreamReadTimeoutError(
                f"stream read timed out after {self._read_idle_timeout}s"
            ) from exc
        finally:
            await resp.aclose()
```
  (iii) `chatgpt_codex.py` — identical shape (non-2xx `aread()` ~362 in the outer try; inner catch chain ~381-384). Its current code block (approximate lines 381-386) is textually identical to (ii)'s Current code; apply the same replacement — inner `except httpx.ReadTimeout` branch before the inner `except httpx.HTTPError`, plus the outer `except httpx.ReadTimeout` (same comment) before `finally:`.

  **3h — CLI provider aclose no-ops.** Insert after `no_timeout` in codex_cli.py (~299), gemini_cli.py (~274), and claude_code.py (~417):
```python
    async def aclose(self) -> None:
        """No-op: CLI providers spawn a subprocess per call and hold no pool."""
        return None
```
  (CLI subprocess read-loop timeouts keep raising `ProviderError` — unchanged, controlled by `cli_timeout`.)

  **3i — CHANGELOG.md.** The sibling py-errors-eof task lands FIRST in this PR group and adds a `## [Unreleased]` heading. Check the top of the file: if `## [Unreleased]` already exists (under the title, above `## [0.16.0]`), do NOT insert a second heading — append the bullets below under the existing section, merging them into its `### Added` / `### Changed` subsections when those headings are already present (add whichever subsection heading is missing). Otherwise, insert the whole block verbatim under the title, above `## [0.16.0]`:
```markdown
## [Unreleased]

### Added
- Unified timeout model (E4): `Client(..., connect_timeout: float = 10.0, read_idle_timeout: float = 120.0, total_timeout: float | None = None)` threaded into every HTTP provider as `httpx.Timeout(connect=connect_timeout, read=read_idle_timeout, write=read_idle_timeout, pool=connect_timeout)`. `total_timeout` (opt-in) bounds `chat()`/`chat_with()` wall clock including retries; it never applies to streams.
- `StreamReadTimeoutError` (`MotosanError` subclass; mirrors Rust `StreamReadTimeout` / TS `StreamReadTimeoutError`): a streaming body idle past `read_idle_timeout` raises it. Never retried — mid-stream retry would replay already-yielded deltas.
- Client lifecycle (E8): `await client.aclose()` and `async with Client(...) as client:`; every provider gains `aclose()` (no-op for CLI providers).
- `cli_timeout` facade kwarg (keyword-only on `Client`; trailing parameter on `Client.codex_cli()`/`Client.gemini_cli()` — pass it by keyword): threads to `CodexCliClient`/`GeminiCliClient` `.timeout()`; `cli_timeout=None` maps to `.no_timeout()`.

### Changed
- **MiniMax timeout unified**: the hardcoded `httpx.AsyncClient(timeout=30)` outlier now uses the shared model (connect 10s, read/write 120s) — requests that previously failed at 30s idle now wait 120s.
- Default connect timeout tightened from 120s (blanket `timeout=120.0`) to 10s across all HTTP providers.
- A streaming-phase `httpx.ReadTimeout` now raises `StreamReadTimeoutError` instead of `StreamError("stream transport error: ...")` (anthropic/openai/gemini/gemini_code_assist/chatgpt_codex) or `NetworkError`/`StreamError` (minimax/ollama).
- Non-2xx error-body reads in `stream()` now sit inside the same ReadTimeout-mapping/cleanup scope as the SSE loop. gemini previously ran `await resp.aread()` before its `try`/`finally` — a `ReadTimeout` there escaped as a raw httpx exception and the response was never closed on error statuses (leak now fixed); gemini_code_assist/chatgpt_codex ran it outside the inner catch chain (raw `ReadTimeout` escaped, response was closed). All three now raise `StreamReadTimeoutError` and always close the response.
```

- [ ] 4. Run to green: `cd sdks/python && uv run pytest tests/test_client_timeouts.py -v` (all pass), then the full package suite: `uv run pytest tests/` — every pre-existing test must pass unchanged (this task retires no pinned behavior; `tests/test_retry_conformance.py` does not cover httpx exception mapping).

- [ ] 5. Format and lint: `cd sdks/python && uv run ruff format && uv run ruff check motosan_ai/` (tests/ is not linted).

- [ ] 6. Commit on the milestone branch:
```bash
git add sdks/python && git commit -m "feat(python): unified timeout model, StreamReadTimeoutError, client lifecycle (E4+E8)

MiniMax 30s outlier unified to connect=10s/read=120s. Streaming-phase
httpx.ReadTimeout now raises StreamReadTimeoutError (never retried).
Non-2xx error-body reads sit inside the mapping/cleanup scope too:
gemini's ran before its try/finally (raw ReadTimeout escaped, response
leaked); gemini_code_assist/chatgpt_codex's sat outside the inner
catch chain. Client gains aclose()/async-with and a cli_timeout
facade kwarg.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```


## T — TypeScript: IncompleteStreamError, timeouts + cancellation

### Task 7: Add TS IncompleteStreamError and enforce adapter EOF termination in all six provider stream adapters

> **Changelog note:** the retired-invariant headline lands in the CHANGELOGs via the release task (Task 10); this task's flip list is its source of truth — keep it accurate.

**Files:**
- `sdks/typescript/src/error.ts` (~line 12)
- `sdks/typescript/src/index.ts` (line 4 — NO edit needed: `export * from './error.js'` already re-exports the new class; verified by test)
- `sdks/typescript/src/providers/anthropic.ts` (~1, ~140-153, ~362-368)
- `sdks/typescript/src/providers/openai.ts` (~1, ~428-442)
- `sdks/typescript/src/providers/gemini.ts` (~1, ~169-177, ~217-224)
- `sdks/typescript/src/providers/ollama.ts` (~1, ~290-336)
- `sdks/typescript/src/providers/minimax.ts` (~26-32)
- `sdks/typescript/src/providers/chatgpt_codex.ts` (~11, ~17-28, ~242-329)
- `sdks/typescript/tests/incomplete-stream.test.ts` (NEW)
- `sdks/typescript/tests/edge-cases.test.ts` (~10, ~124-189)
- `sdks/typescript/tests/providers-gemini.test.ts` (~4, ~352-381)
- `sdks/typescript/tests/providers-ollama.test.ts` (~1-5, ~362-407)
- `sdks/typescript/tests/providers-anthropic.test.ts` (~2, ~617-643)

**Interfaces:**
- Produces (E1): `export class IncompleteStreamError extends StreamError` in `src/error.ts`, `constructor(message: string)`, `name = 'IncompleteStreamError'`. Message convention: `incomplete stream: <provider> ended without a terminal event`, `<provider>` ∈ the `src/provider.ts:102` union spellings `anthropic | openai | minimax | ollama | gemini | chatgpt_codex`.
- Produces: `AnthropicProvider` constructor gains additive optional 4th param `providerName = 'anthropic'`; `MinimaxProvider` passes `'minimax'`.
- Consumes: `StreamError` (`src/error.ts:12`), `parseSse` (`src/http/sse.ts`), `parseNdjson` (`src/http/ndjson.ts`), `collectStream`/`BoxStream`/`doneEvent`/`doneWithStopReason` (`src/stream.ts`). `collectStream` is NOT changed (E2: its stop_reason heuristic stays, for a real done event lacking a reason — pinned green by `tests/stream.test.ts` ~161-167).

**E3 retired-pin flips (M2 regression contract — file-by-file; everything else passes unchanged):**
1. `tests/edge-cases.test.ts` — flip `'Anthropic: stream that ends without message_stop terminates silently with a partial response'` and `'OpenAI: stream that ends without [DONE]/finish_reason terminates silently with a partial response'` (~124-189).
2. `tests/providers-gemini.test.ts` — flip `'skips a defensive [DONE] line and does not fabricate a done on EOF (gemini.rs:447-449,531)'` (~352).
3. `tests/providers-ollama.test.ts` — flip `'ends WITHOUT synthesizing a done event on EOF without done:true'` (~362) and `'ends without throwing when the NDJSON body errors after yielding data'` (~377; body errors now propagate — the swallow's own comment deferred surfacing to M3).
4. `tests/providers-anthropic.test.ts` — `'streamImpl also adds the MCP beta header'` (~617) drains an EMPTY SSE body; the drain now throws — expect `IncompleteStreamError`, keep the header assertion.

Steps:
- [ ] 1. **Write failing tests.** (a) NEW file `sdks/typescript/tests/incomplete-stream.test.ts`:
```ts
import { describe, it, expect, vi, afterEach } from 'vitest'
import * as sdk from '../src/index.js'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { GeminiProvider } from '../src/providers/gemini.js'
import { OllamaProvider } from '../src/providers/ollama.js'
import { MinimaxProvider } from '../src/providers/minimax.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { collectStream } from '../src/stream.js'
import { IncompleteStreamError, MotosanError, StreamError } from '../src/error.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

const REQ: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }

// One-shot ReadableStream body + fetch stub (style: tests/edge-cases.test.ts:19-40).
function stubBodyFetch(transcript: string, contentType = 'text/event-stream'): void {
  const body = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(transcript))
      controller.close()
    },
  })
  vi.stubGlobal(
    'fetch',
    vi.fn(async () => new Response(body, { status: 200, headers: { 'content-type': contentType } })),
  )
}

async function drain(stream: AsyncIterable<StreamEvent>) {
  const events: StreamEvent[] = []
  let error: unknown
  try {
    for await (const e of stream) events.push(e)
  } catch (e) {
    error = e
  }
  return { events, error }
}

describe('IncompleteStreamError (E1)', () => {
  it('subclasses StreamError (migration softener) + MotosanError, and is exported from the package root', () => {
    const err = new IncompleteStreamError('incomplete stream: anthropic ended without a terminal event')
    expect(err).toBeInstanceOf(StreamError)
    expect(err).toBeInstanceOf(MotosanError)
    expect(err.name).toBe('IncompleteStreamError')
    expect(typeof sdk.IncompleteStreamError).toBe('function')
  })
})

describe('adapter EOF-without-terminal-event enforcement (E2/E3)', () => {
  afterEach(() => vi.unstubAllGlobals())

  const ANTHROPIC_PARTIAL =
    'event: content_block_delta\n' +
    'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'

  // Each transcript yields one text event, then EOF with NO terminal event.
  const cases = [
    {
      name: 'gemini',
      transcript: 'data: {"candidates":[{"content":{"parts":[{"text":"partial"}],"role":"model"}}]}\n\n',
      contentType: 'text/event-stream',
      make: () => new GeminiProvider('k'),
    },
    {
      name: 'ollama',
      transcript: '{"message":{"content":"partial"},"done":false}\n',
      contentType: 'application/x-ndjson',
      make: () => new OllamaProvider('llama3.2', 'http://localhost:11434'),
    },
    {
      name: 'minimax',
      transcript: ANTHROPIC_PARTIAL,
      contentType: 'text/event-stream',
      make: () => new MinimaxProvider('key'),
    },
    {
      name: 'chatgpt_codex',
      transcript: 'data: {"type":"response.output_text.delta","delta":"partial"}\n\n',
      contentType: 'text/event-stream',
      make: () => new ChatGptCodexProvider('tok', 'acct'),
    },
  ]

  for (const c of cases) {
    it(`${c.name}: EOF after a text delta throws IncompleteStreamError (partial text already yielded)`, async () => {
      stubBodyFetch(c.transcript, c.contentType)
      const { events, error } = await drain(c.make().stream(REQ))
      expect(error).toBeInstanceOf(IncompleteStreamError)
      expect((error as Error).message).toBe(`incomplete stream: ${c.name} ended without a terminal event`)
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual(['partial'])
      expect(events.some((e) => e.done)).toBe(false)
    })
  }

  it('anthropic: a message_delta stop_reason without message_stop is still incomplete (only message_stop is terminal)', async () => {
    stubBodyFetch(
      ANTHROPIC_PARTIAL +
        'event: message_delta\n' +
        'data: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}\n\n',
    )
    const { error } = await drain(new AnthropicProvider('key', 'claude-3-5-sonnet-20241022').stream(REQ))
    expect(error).toBeInstanceOf(IncompleteStreamError)
  })

  it('collectStream propagates IncompleteStreamError (no stop_reason fallback for truncation)', async () => {
    stubBodyFetch(ANTHROPIC_PARTIAL)
    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    await expect(collectStream(provider.stream(REQ))).rejects.toBeInstanceOf(IncompleteStreamError)
  })
})
```
(b) **Flip the pinned tests.** In `tests/edge-cases.test.ts`: change line ~10 to `import { IncompleteStreamError, MotosanError, ProviderError } from '../src/error.js'`, then replace the whole `describe('mid-stream reset / partial success', ...)` block (~124-189, including its 5-line lead comment) with:
```ts
  describe('mid-stream truncation (M3/E3: EOF without a terminal event throws)', () => {
    // M3 retired the v0.10.1 "fabricate a clean done at EOF" invariant
    // (anthropic.ts / openai.ts defensive tails removed).
    async function drainErr(stream: AsyncIterable<StreamEvent>) {
      const events: StreamEvent[] = []
      let error: unknown
      try {
        for await (const evt of stream) events.push(evt)
      } catch (e) {
        error = e
      }
      return { events, error }
    }

    it('Anthropic: stream that ends without message_stop throws IncompleteStreamError', async () => {
      const transcript =
        'event: content_block_start\n' +
        'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'
      stubSseFetch(transcript)
      const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
      const { events, error } = await drainErr(provider.stream({ messages: [{ role: 'user', content: 'hi' }] }))
      expect(error).toBeInstanceOf(IncompleteStreamError)
      expect((error as Error).message).toBe('incomplete stream: anthropic ended without a terminal event')
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual(['partial'])
      expect(events.some((e) => e.done)).toBe(false)

      stubSseFetch(transcript)
      await expect(
        collectStream(provider.stream({ messages: [{ role: 'user', content: 'hi' }] })),
      ).rejects.toBeInstanceOf(IncompleteStreamError)
    })

    it('OpenAI: stream that ends without [DONE] throws IncompleteStreamError', async () => {
      const transcript = 'data: {"choices":[{"index":0,"delta":{"content":"partial"}}]}\n\n'
      stubSseFetch(transcript)
      const provider = new OpenAIProvider('sk-test', 'gpt-4o')
      const { events, error } = await drainErr(provider.stream({ messages: [{ role: 'user', content: 'hi' }] }))
      expect(error).toBeInstanceOf(IncompleteStreamError)
      expect((error as Error).message).toBe('incomplete stream: openai ended without a terminal event')
      expect(events.some((e) => e.done)).toBe(false)

      stubSseFetch(transcript)
      await expect(
        collectStream(provider.stream({ messages: [{ role: 'user', content: 'hi' }] })),
      ).rejects.toBeInstanceOf(IncompleteStreamError)
    })
  })
```
In `tests/providers-gemini.test.ts`: change line ~4 to `import { IncompleteStreamError, UnsupportedFeatureError } from '../src/error.js'`; in the test at ~352, rename it to `'skips a defensive [DONE] line and throws IncompleteStreamError on EOF without finishReason (M3/E2)'`, wrap the existing drain loop in try/catch capturing `error`, keep the two existing text-event/no-done assertions, add `expect(error).toBeInstanceOf(IncompleteStreamError)`, and replace the trailing `collectStream` block (`expect(resp.content)...expect(resp.stopReason).toBe('end_turn')`) with `await expect(collectStream(provider.stream({ messages: [{ role: 'user', content: 'hi' }] }))).rejects.toBeInstanceOf(IncompleteStreamError)`.
In `tests/providers-ollama.test.ts`: add `import { IncompleteStreamError } from '../src/error.js'` after line 2; rewrite the test at ~362 as `'throws IncompleteStreamError on EOF without done:true (M3/E2)'` — same `stubNdjson` fixture, drain in try/catch, then `expect(error).toBeInstanceOf(IncompleteStreamError)`, `expect((error as Error).message).toBe('incomplete stream: ollama ended without a terminal event')`, `expect(events.map((e) => e.content)).toEqual(['a', 'b'])`; rewrite the test at ~377 as `'propagates an NDJSON body error after yielding partial data (M3: swallow removed)'` — keep the erroring-ReadableStream fetch stub verbatim, change the drain assertion from `.resolves.toBeUndefined()` to `.rejects.toThrow('socket closed')`, keep `expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])`.
In `tests/providers-anthropic.test.ts`: change line 2 to `import { IncompleteStreamError, StreamError } from '../src/error.js'`; in `'streamImpl also adds the MCP beta header'` (~617) wrap the drain in try/catch capturing `error`, drop the unused `events` array, and before the header assertion add `expect(error).toBeInstanceOf(IncompleteStreamError)`.
- [ ] 2. **Run and watch them fail.** From `sdks/typescript`: `npx vitest run tests/incomplete-stream.test.ts tests/edge-cases.test.ts tests/providers-gemini.test.ts tests/providers-ollama.test.ts tests/providers-anthropic.test.ts`. Expected signature: every file that imports the new class fails at import time with `SyntaxError: The requested module '../src/error.js' does not provide an export named 'IncompleteStreamError'` (same shape as the sibling Task 8 CancelledError red step under this repo's vitest 3 setup), and enforcement/flipped tests in files that do not import it fail with `expected undefined to be an instance of IncompleteStreamError` or `promise resolved ... instead of rejecting` (adapters today fabricate a done — anthropic.ts ~363-368, openai.ts ~428-442, chatgpt_codex.ts ~327-328 — or end silently — gemini.ts ~223, ollama.ts ~329-335).
- [ ] 3. **Implement.** (a) `src/error.ts` — current code (approximate line 12): `export class StreamError extends MotosanError {}`. Replace with:
```ts
export class StreamError extends MotosanError {}

/**
 * Error thrown when the upstream byte/event stream ends (EOF) without the
 * provider's terminal event (Anthropic message_stop, OpenAI [DONE], Gemini
 * finishReason, Ollama done:true, Codex response.completed). Message
 * convention: `incomplete stream: <provider> ended without a terminal event`.
 * Subclasses StreamError deliberately (migration softener): existing
 * `instanceof StreamError` handlers keep catching truncation. Replaces the
 * retired v0.10.1 "exactly one terminal done even when upstream closes
 * without [DONE]" invariant (M3/E3; specs/types.md stream termination).
 */
export class IncompleteStreamError extends StreamError {
  constructor(message: string) {
    super(message)
    this.name = 'IncompleteStreamError'
  }
}
```
(b) `src/providers/anthropic.ts` — line 1 becomes `import { IncompleteStreamError, ProviderError, StreamError } from '../error.js'`. Current code (approximate lines 140-153): class head with fields `model`/`baseUrl`/`retryPolicy` and `constructor(private readonly apiKey: string, model?: string, baseUrl = 'https://api.anthropic.com')`. Replace with the same plus a `private readonly providerName: string` field, a 4th constructor param `providerName = 'anthropic',` and `this.providerName = providerName` in the body. Current code (approximate lines 362-368):
```ts
    // Defensive: terminate even if message_stop never arrived.
    if (state.stopReason !== undefined) {
      yield doneWithStopReason(state.stopReason)
    } else {
      yield doneEvent()
    }
```
Replace with:
```ts
    // EOF without message_stop: truncation, not completion (M3/E3 — the
    // fabricated clean done is retired). A message_delta stop_reason alone
    // is NOT terminal; only message_stop is.
    throw new IncompleteStreamError(
      `incomplete stream: ${this.providerName} ended without a terminal event`,
    )
```
(`doneEvent`/`doneWithStopReason` stay imported — the message_stop branch ~341-346 still uses both.)
(c) `src/providers/openai.ts` — add `import { IncompleteStreamError } from '../error.js'` as a new first line. Current code (approximate lines 428-442): the `// Defensive: EOF without [DONE] — emit terminal once.` block (`if (!doneEmitted) { ...open-tool flush... yield pendingStopReason !== undefined ? doneWithStopReason(...) : doneEvent() }`). Replace with:
```ts
    // EOF without the [DONE] sentinel: truncation, not completion (M3/E3 —
    // the doneEmitted EOF fabrication is retired; no open-tool flush either).
    if (!doneEmitted) {
      throw new IncompleteStreamError('incomplete stream: openai ended without a terminal event')
    }
```
(d) `src/providers/gemini.ts` — add `import { IncompleteStreamError } from '../error.js'` as a new first line. In `streamImpl`, insert `let sawTerminal = false` on its own line directly above `for await (const evt of parseSse(responseBody)) {` (~173). Current code (approximate lines 217-224):
```ts
      // done LAST, only when finishReason present — the ONLY terminator
      // (gemini.rs:513-523).
      if (finishReason !== undefined) {
        yield doneWithStopReason(mapFinishReason(finishReason, hasToolCalls))
      }
    }
    // EOF: generator ends naturally. NO fabricated done (gemini.rs:531).
  }
```
Replace with:
```ts
      // done LAST, only when finishReason present — the ONLY terminator
      // (gemini.rs:513-523).
      if (finishReason !== undefined) {
        sawTerminal = true
        yield doneWithStopReason(mapFinishReason(finishReason, hasToolCalls))
      }
    }
    // EOF without any finishReason: truncation, not completion (M3/E2).
    if (!sawTerminal) {
      throw new IncompleteStreamError('incomplete stream: gemini ended without a terminal event')
    }
  }
```
(e) `src/providers/ollama.ts` — add `import { IncompleteStreamError } from '../error.js'` as a new first line. In `streamImpl` (~290-336): delete the `try {` wrapper — replace `    // Adapter over parseNdjson: decide termination on done:true.\n    try {\n      for await (const obj of parseNdjson(responseBody)) {` with `    // Adapter over parseNdjson: terminal event is done:true. Body errors\n    // propagate (M3 removed the M1-era swallow).\n    for await (const obj of parseNdjson(responseBody)) {`, outdent the loop body by one level (2 spaces; byte-identical otherwise), and replace the tail — current code (approximate lines 328-336):
```ts
      }
    } catch {
      // Ignore post-start stream body errors, matching Rust's partial-success
      // stream semantics: end without synthesizing a terminal done event.
      return
    }
    // EOF without done:true — let the generator end (NO synthesized done),
    // matching Rust Poll::Ready(None). collectStream fabricates a stop_reason.
  }
```
with:
```ts
    }
    // EOF without done:true: truncation, not completion (M3/E2). done:true
    // returns from the generator above, so reaching here means no terminal.
    throw new IncompleteStreamError('incomplete stream: ollama ended without a terminal event')
  }
```
(f) `src/providers/minimax.ts` — current code (approximate lines 27-32): `this.inner = new AnthropicProvider(apiKey, model ?? DEFAULT_MINIMAX_MODEL, baseUrl ?? DEFAULT_MINIMAX_BASE_URL,)`. Add a 4th argument `'minimax',` (so the E1 message names the outer provider).
(g) `src/providers/chatgpt_codex.ts` — line ~11 becomes `import { IncompleteStreamError, StreamError } from '../error.js'`; remove `doneEvent,` from the `../stream.js` import block (~17-28; it becomes unused). In `streamImpl`: replace `    // Fatal `error` / `response.failed` frames throw a StreamError (Rust/\n    // Python parity). Other post-start body errors still end silently (M3).\n    try {\n      for await (const evt of parseSse(responseBody)) {` (~242-245) with `    // Fatal `error` / `response.failed` frames throw a StreamError; other\n    // body errors propagate (M3 removed the swallow).\n    for await (const evt of parseSse(responseBody)) {`, outdent the loop body one level, and replace the tail — current code (approximate lines 316-329): the closing `}` of the loop, the whole `} catch (error) { if (error instanceof StreamError) { throw error } ... return }` block, and `// Defensive terminal ...\n    yield doneEvent()`. Replace with:
```ts
    }

    // EOF without response.completed: truncation, not completion (M3/E2/E3 —
    // the defensive doneEvent() is retired). chat() = collectStream(stream()),
    // so a truncated chat() now rejects with IncompleteStreamError too.
    throw new IncompleteStreamError(
      'incomplete stream: chatgpt_codex ended without a terminal event',
    )
  }
```
(h) `src/index.ts` — no change: line 4 `export * from './error.js'` already exports the class (asserted by the new test via `sdk.IncompleteStreamError`).
- [ ] 4. **Run to green + package suite.** From `sdks/typescript`: `npx vitest run tests/incomplete-stream.test.ts tests/edge-cases.test.ts tests/providers-gemini.test.ts tests/providers-ollama.test.ts tests/providers-anthropic.test.ts tests/providers-openai.test.ts tests/providers-chatgpt-codex.test.ts tests/providers-minimax.test.ts` — all pass. Then the full gate: `npm run build && npm test` (M2 regression contract: retry.test.ts, retry-integration.test.ts, retry-conformance.test.ts, stream.test.ts, client-builder.test.ts all pass UNCHANGED — their stream transcripts all carry terminal events; only the four files in the flip list above changed).
- [ ] 5. **Typecheck (the TS SDK has no eslint/prettier script; this is the lint gate).** From `sdks/typescript`: `npm run typecheck` — zero errors.
- [ ] 6. **Commit** (same PR group as the specs/types.md stream-termination amendment per E9 — never let the spec and this behavior drift):
```
git add sdks/typescript
git commit -m "feat(typescript)!: throw IncompleteStreamError on stream EOF without a terminal event

BREAKING CHANGE: retires the v0.10.1 'exactly one terminal done event even
when upstream closes without [DONE]' invariant (M3/E3). Anthropic/OpenAI EOF
done-fabrication removed; Gemini/Ollama silent EOF removed; ChatGPT-Codex
defensive done removed; Ollama/Codex post-start body-error swallow removed.
IncompleteStreamError extends StreamError as a deliberate migration softener.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 8: Add the TS timeout model, per-request cancellation, and a throwing read-idle stream timeout (E4+E6+E7)

**Files:** (all under `sdks/typescript/`, line refs approximate, origin/main @ acf5d7f)
- `src/error.ts` (~12 add class; ~91-94 isRetryableNetworkError)
- `src/retry.ts` (~1 import; ~147-162 classifyForRetry; new helper at end)
- `src/http/fetch.ts` (~3-5 FetchOptions; ~32-41 postJson fetch call; ~56-65 postStream fetch call; new helper)
- `src/provider.ts` (~8 import; ~102-133 options types + ProviderImpl + dispatch; ~136-192 readTimeoutStream)
- `src/client.ts` (~1 import; ~24-28 ProviderLike; ~55-65 asDispatchProvider; ~76/113-116 builder field+setter; ~317-318 build; ~328-424 Client)
- `src/providers/anthropic.ts` (~193-202 chat; ~236-256 stream/streamImpl)
- `src/providers/openai.ts` (~144, ~210-214, ~241-261, ~301-319), `src/providers/gemini.ts` (~93-102, ~151-166), `src/providers/ollama.ts` (~221-227, ~276-288), `src/providers/chatgpt_codex.ts` (~209-231), `src/providers/minimax.ts` (~39-45)
- `src/index.ts` (~32), `package.json` (~15-17 engines)
- `tests/client-builder.test.ts` (~3 import; ~232-244/~277-296/~298-316 flipped pinned tests), `tests/retry-conformance.test.ts` (~8-9 imports + append), new `tests/timeouts-cancellation.test.ts`

**Interfaces:**
- Produces `export class CancelledError extends MotosanError` (error.ts).
- Produces `ClientBuilder.timeouts({connectMs?, readIdleMs?, totalMs?}): this` — defaults connectMs = 10_000, readIdleMs = 120_000, totalMs = undefined (E4 spelling).
- Produces `Client.chat(request: ChatRequest, opts?: RequestOptions): Promise<ChatResponse>` and `Client.stream(request: ChatRequest, opts?: RequestOptions): AsyncIterable<StreamEvent>` with `export interface RequestOptions { signal?: AbortSignal }` and internal `export interface ProviderRequestOptions extends RequestOptions { callerSignal?: AbortSignal; preHeadersTimeoutMs?: number }` (provider.ts) — `signal` is the caller signal plus the opt-in totalMs `AbortSignal.timeout` composed via `AbortSignal.any` (chat only); `preHeadersTimeoutMs` carries the connect budget down to http/fetch.ts.
- Produces `FetchOptions.preHeadersTimeoutMs?: number` in `src/http/fetch.ts` (postJson/postStream at ~37-39/~61-63 already forward `options.signal`; this task extends both with a disarm-at-headers connect timer).
- Produces `export async function attemptWithCancellation<T>(callerSignal: AbortSignal | undefined, op: () => Promise<T>): Promise<T>` (retry.ts); `readTimeoutStream` now THROWS `StreamReadTimeoutError` (exists at error.ts:18-26, never thrown today).
- Coordination: the M3 spec task adds the CancelledError row to specs/retry.md's transport table; THIS task lands the matching TS conformance rows (Rust/Python conformance siblings are updated by their own M3 tasks). Land in the same PR group so suites never drift from the spec (E9).

**Conformance note (E4 TS spelling amended — flagged per plan rules, do not "fix" back):** E4's literal TS spelling ("connect+total via AbortSignal.timeout composition on fetch") cannot express a connect-phase timeout: a composed `AbortSignal.timeout(connectMs)` stays armed through `response.json()` / the stream body (src/http/fetch.ts keeps one signal live for the fetch AND the body read), so the 10s default would abort every non-streaming chat whose generation takes >10s — a de-facto per-attempt total timeout, contradicting E4's "total = None default" and cross-SDK parity (reqwest `connect_timeout` / httpx `Timeout(connect=...)` are socket-scoped). Closest conforming version implemented here: `totalMs` keeps the literal spelling (opt-in `AbortSignal.timeout`, armed across the whole chat call — the ONLY signal allowed to stay armed whole-call besides the caller's); the connect budget moves into `src/http/fetch.ts` as an AbortController timer DISARMED in a `finally` the moment `await fetch(...)` resolves (headers received) — it never bounds body reads. Because fetch cannot observe socket-connect separately from waiting-for-response-start, and E4 assigns response-start waiting to read_idle (httpx semantics: response start counts against `read`), the pre-headers deadline is `connectMs + readIdleMs` (default 130s): dead-host detection without double- or under-bounding either budget. True socket-level connect timeouts would need an undici `Agent({ connect: { timeout } })` dispatcher (new runtime dependency + npm-undici/built-in-fetch interop risk) — out of scope; noted for a follow-up if wanted.

**Steps:**

- [ ] 1. Write the failing test file `tests/timeouts-cancellation.test.ts` (style matches tests/retry-integration.test.ts: vitest + `vi.stubGlobal('fetch', ...)`):

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client, ClientBuilder } from '../src/client.js'
import { CancelledError, StreamReadTimeoutError } from '../src/error.js'
import { readTimeoutStream } from '../src/provider.js'
import { RetryPolicy } from '../src/retry.js'

function immediateRetryPolicy(): RetryPolicy {
  return new RetryPolicy({ maxRetries: 2, baseDelayMs: 0, maxDelayMs: 0, jitter: false, respectRetryAfter: false })
}

function anthropicPayload(text: string): string {
  return JSON.stringify({
    content: [{ type: 'text', text }],
    model: 'claude-sonnet-4-6',
    usage: { input_tokens: 1, output_tokens: 2 },
    stop_reason: 'end_turn',
  })
}

function abortError(): Error {
  const error = new Error('This operation was aborted')
  error.name = 'AbortError'
  return error
}

describe('E7: readTimeoutStream throws on idle expiry', () => {
  it('throws StreamReadTimeoutError instead of ending silently', async () => {
    const stalled = (async function* () {
      await new Promise(() => {})
      yield { content: 'never', done: false, eventType: 'text' as const }
    })()

    await expect(async () => {
      for await (const _ of readTimeoutStream(stalled, 0.05)) void _
    }).rejects.toThrow(StreamReadTimeoutError)
  })

  it('Client.stream applies readIdleMs by default and throws on a stalled provider', async () => {
    const fakeProvider = {
      capabilities: () => ({ supportsImage: false, supportsDocument: false, supportsMcp: false }),
      async chat(): Promise<never> { throw new Error('unused') },
      async *stream() {
        yield { content: 'a', done: false, eventType: 'text' as const }
        await new Promise(() => {})
      },
    }

    const client = new Client(fakeProvider, { readIdleMs: 50 })
    await expect(async () => {
      for await (const _ of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) void _
    }).rejects.toThrow(StreamReadTimeoutError)
  })
})

describe('E6/E4: per-request cancellation and timeout composition', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('caller abort mid-request throws CancelledError after exactly 1 fetch (never retried)', async () => {
    const controller = new AbortController()
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        calls += 1
        expect(init?.signal).toBeDefined() // caller signal composed in http/fetch.ts reached fetch
        controller.abort()
        throw abortError() // undici rejects once the composed signal aborts
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .build()

    await expect(
      client.chat({ messages: [{ role: 'user', content: 'hi' }] }, { signal: controller.signal }),
    ).rejects.toThrow(CancelledError)
    expect(calls).toBe(1)
  })

  it('AbortSignal.timeout expiry (no caller signal) stays retryable', async () => {
    let calls = 0
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        if (calls === 1) {
          const error = new Error('The operation was aborted due to timeout')
          error.name = 'TimeoutError' // AbortSignal.timeout reason: undici rejects with signal.reason
          throw error
        }
        return new Response(anthropicPayload('ok'), { status: 200 })
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .timeouts({ totalMs: 5_000 })
      .build()

    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('ok')
    expect(calls).toBe(2)
  })

  it('default timeouts never abort a slow-but-successful chat (50ms to headers)', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        await new Promise((resolve) => setTimeout(resolve, 50))
        return new Response(anthropicPayload('ok'), { status: 200 })
      }),
    )
    const client = new ClientBuilder().provider('anthropic').apiKey('test-key').build()
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('ok')
  })

  it('connect budget disarms at headers: a body slower than connectMs+readIdleMs still succeeds', async () => {
    let capturedSignal: AbortSignal | undefined
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, init?: RequestInit) => {
        capturedSignal = init?.signal ?? undefined
        // Headers immediately; JSON body 80ms later — past the 30ms pre-headers
        // budget below. An always-armed AbortSignal.timeout(connectMs) would have
        // aborted capturedSignal by then; the disarm-at-headers timer must not.
        const body = new ReadableStream<Uint8Array>({
          async start(c) {
            await new Promise((resolve) => setTimeout(resolve, 80))
            c.enqueue(new TextEncoder().encode(anthropicPayload('slow ok')))
            c.close()
          },
        })
        return new Response(body, { status: 200 })
      }),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .timeouts({ connectMs: 10, readIdleMs: 20 })
      .build()

    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
    expect(response.content).toBe('slow ok')
    expect(capturedSignal?.aborted).toBe(false)
  })

  it('caller abort mid-stream surfaces CancelledError, not a raw AbortError', async () => {
    const controller = new AbortController()
    const sse =
      'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'
    const body = new ReadableStream<Uint8Array>({
      start(c) {
        c.enqueue(new TextEncoder().encode(sse))
      },
      pull() {
        controller.abort()
        throw abortError() // body read fails once the caller aborts
      },
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response(body, { status: 200, headers: { 'content-type': 'text/event-stream' } })),
    )

    const client = new ClientBuilder()
      .provider('anthropic')
      .apiKey('test-key')
      .retryPolicy(immediateRetryPolicy())
      .build()

    await expect(async () => {
      for await (const _ of client.stream(
        { messages: [{ role: 'user', content: 'hi' }] },
        { signal: controller.signal },
      ))
        void _
    }).rejects.toThrow(CancelledError)
  })
})
```

- [ ] 2. Run it and confirm the failure signature. From `sdks/typescript`: `npx vitest run tests/timeouts-cancellation.test.ts` — expect the whole file to fail at import time with `SyntaxError: The requested module '../src/error.js' does not provide an export named 'CancelledError'`.

- [ ] 3. Implement:

  **3a `src/error.ts`** — after `export class StreamError extends MotosanError {}` (~line 12) add:
```ts
/**
 * Thrown when the CALLER's AbortSignal aborts a request. NEVER retried
 * (specs/retry.md transport table). Fetch-internal AbortError/TimeoutError
 * with no caller signal aborted (e.g. AbortSignal.timeout) stay retryable.
 */
export class CancelledError extends MotosanError {
  constructor(message = 'request cancelled by caller') {
    super(message)
    this.name = 'CancelledError'
  }
}
```
  Current code (approximate lines 91-94):
```ts
  // AbortError (fetch cancelled/timed out at fetch level)
  if (error.name === 'AbortError') {
    return true
  }
```
  Replace with:
```ts
  // AbortError / TimeoutError: fetch cancelled or timed out at fetch level.
  // undici rejects with signal.reason — a TimeoutError DOMException for
  // AbortSignal.timeout. Caller-signal aborts are translated to
  // CancelledError BEFORE classification and never reach this predicate.
  if (error.name === 'AbortError' || error.name === 'TimeoutError') {
    return true
  }
```

  **3b `src/retry.ts`** — change line 1 to `import { CancelledError, isRetryableNetworkError, isRetryableStatus } from './error.js'`. In `classifyForRetry` (current body starts ~line 148 with `if (typeof errOrStatus === 'number')`), insert as the FIRST statement:
```ts
  if (errOrStatus instanceof CancelledError) {
    return { retryable: false } // specs/retry.md: caller cancellation is never retried
  }
```
  Append at end of file:
```ts
/**
 * Run one request attempt; if it fails while the CALLER's signal is aborted,
 * throw CancelledError (classified non-retryable) instead of the raw abort
 * (E6: the provider request catch tests callerSignal.aborted).
 */
export async function attemptWithCancellation<T>(
  callerSignal: AbortSignal | undefined,
  op: () => Promise<T>,
): Promise<T> {
  try {
    return await op()
  } catch (error) {
    if (callerSignal?.aborted) {
      throw new CancelledError()
    }
    throw error
  }
}
```

  **3c `src/http/fetch.ts`** — Current code (approximate lines 3-5):
```ts
export interface FetchOptions {
  signal?: AbortSignal
}
```
  Replace with:
```ts
export interface FetchOptions {
  /** Caller signal (streams) or caller+totalMs composition (chat). Stays armed for the whole call. */
  signal?: AbortSignal
  /**
   * E4 connect budget (the Client passes connectMs + readIdleMs — see the
   * task's Conformance note). Arms a timer-driven AbortController that is
   * DISARMED the moment `await fetch(...)` resolves (headers received), so it
   * never bounds body reads and a slow generation cannot trip it. If it fires
   * (dead host / black-holed connect), fetch rejects with an AbortError,
   * which stays retryable.
   */
  preHeadersTimeoutMs?: number
}

/** Compose options into RequestInit.signal, run fetch, disarm the pre-headers timer at headers. */
async function fetchWithTimeouts(
  url: string,
  fetchOptions: RequestInit,
  options?: FetchOptions,
): Promise<Response> {
  const signals: AbortSignal[] = []
  if (options?.signal) signals.push(options.signal)
  let timer: ReturnType<typeof setTimeout> | undefined
  if (options?.preHeadersTimeoutMs !== undefined) {
    const connectController = new AbortController()
    timer = setTimeout(() => connectController.abort(), options.preHeadersTimeoutMs)
    signals.push(connectController.signal)
  }
  if (signals.length === 1) fetchOptions.signal = signals[0]
  if (signals.length > 1) fetchOptions.signal = AbortSignal.any(signals)
  try {
    return await fetch(url, fetchOptions)
  } finally {
    if (timer !== undefined) clearTimeout(timer)
  }
}
```
  (Single-signal fast path passes the caller's signal object through untouched — keeps the pinned identity assertion in tests/http-fetch.test.ts ~141-146 green.) Then in `postJson` — current code (approximate lines 32-41):
```ts
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }
  if (options?.signal) {
    fetchOptions.signal = options.signal
  }

  const response = await fetch(url, fetchOptions)
```
  Replace with:
```ts
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }

  const response = await fetchWithTimeouts(url, fetchOptions, options)
```
  Apply the IDENTICAL replacement in `postStream` (approximate lines 56-65).

  **3d `src/provider.ts`** — line 8 becomes `import { StreamReadTimeoutError, UnsupportedFeatureError } from './error.js'`. Before `export interface ProviderImpl` (~line 109) add:
```ts
/** Per-request options accepted by Client.chat / Client.stream. */
export interface RequestOptions {
  /** Caller cancellation signal. Abort => CancelledError, never retried (E6). */
  signal?: AbortSignal
}

/**
 * What providers receive: `signal` is the fetch signal (caller signal, plus
 * the opt-in totalMs AbortSignal.timeout on chat paths only); `callerSignal`
 * is the raw caller signal, kept separate so the CancelledError-vs-
 * retryable-abort split can test callerSignal.aborted; `preHeadersTimeoutMs`
 * is the E4 connect budget, disarmed by http/fetch.ts once headers arrive.
 */
export interface ProviderRequestOptions extends RequestOptions {
  callerSignal?: AbortSignal
  preHeadersTimeoutMs?: number
}
```
  In `ProviderImpl` (~110-113) change the two methods to `chat(req: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse>` and `stream(req: ChatRequest, opts?: ProviderRequestOptions): BoxStream` (existing classes still structurally satisfy — TS permits fewer params). `dispatchChat`/`dispatchStream` (~120-133) each gain a third param `opts?: ProviderRequestOptions` forwarded as `provider.chat(req, opts)` / `provider.stream(req, opts)`.
  In `readTimeoutStream` — current code (approximate lines 174-177):
```ts
        if (raced === '__timeout__') {
          cancelInnerWithoutWaiting()
          return
        }
```
  Replace with:
```ts
        if (raced === '__timeout__') {
          cancelInnerWithoutWaiting()
          throw new StreamReadTimeoutError(timeoutSecs)
        }
```
  Update its docstring (~136-140): "Stream wrapper that applies a read-idle timeout. THROWS StreamReadTimeoutError when no event arrives within the deadline (E7 — the pre-M3 silent-end behavior is retired); the deadline resets on each yielded event and the inner iterator is cancelled before throwing."

  **3e `src/client.ts`** — line 1 becomes `import { CancelledError, ConfigError } from './error.js'`; extend the provider.js import block (~2-9) with `type RequestOptions, type ProviderRequestOptions`. Add near the top:
```ts
/** E4 one-timeout-model settings (milliseconds). */
export interface TimeoutSettings {
  /** Connect budget; fused with readIdleMs into http/fetch.ts's disarm-at-headers deadline. Default 10_000. */
  connectMs?: number
  /** Max gap between stream events (readTimeoutStream). Default 120_000. */
  readIdleMs?: number
  /** Whole-call budget, chat() only — NEVER applied to stream body consumption. Default undefined (off). */
  totalMs?: number
}

const DEFAULT_CONNECT_MS = 10_000
const DEFAULT_READ_IDLE_MS = 120_000
```
  `ProviderLike` (~24-28): both methods gain `opts?: ProviderRequestOptions`; `asDispatchProvider` (~55-65) forwards it. Builder: replace field `protected _streamReadTimeoutSecs?: number` (~76) with `protected _timeouts: TimeoutSettings = {}`. Current setter (approximate lines 113-116):
```ts
  streamReadTimeoutSecs(n: number): this {
    this._streamReadTimeoutSecs = n
    return this
  }
```
  Replace with:
```ts
  /** E4 one-timeout-model. Unset fields keep defaults (connect 10s, readIdle 120s, total off). */
  timeouts(t: TimeoutSettings): this {
    this._timeouts = { ...this._timeouts, ...t }
    return this
  }

  /** @deprecated Alias for timeouts({ readIdleMs: n * 1000 }); superseded by the E4 timeout model. */
  streamReadTimeoutSecs(n: number): this {
    this._timeouts = { ...this._timeouts, readIdleMs: n * 1000 }
    return this
  }
```
  `build()` (~317-318): `return new Client(provider, this._timeouts)`. Client class: replace field `private streamReadTimeoutSecs?: number` with `private readonly connectMs: number`, `private readonly readIdleMs: number`, `private readonly totalMs?: number`; ctor second param `streamReadTimeoutSecs?: number` becomes `timeouts?: TimeoutSettings` and the assignment (~347) becomes:
```ts
    this.connectMs = timeouts?.connectMs ?? DEFAULT_CONNECT_MS
    this.readIdleMs = timeouts?.readIdleMs ?? DEFAULT_READ_IDLE_MS
    this.totalMs = timeouts?.totalMs
```
  Current code (approximate lines 391-409, chat + stream):
```ts
  /** Send a chat request; validates capabilities BEFORE any HTTP call. */
  async chat(request: ChatRequest): Promise<ChatResponse> {
    return dispatchChat(this.provider, request)
  }

  /**
   * Stream a chat request: dispatch (validate → provider.stream) → optional
   * readTimeoutStream → stripThink. Matches Rust ordering.
   */
  stream(request: ChatRequest): AsyncIterable<StreamEvent> {
    let stream: BoxStream = dispatchStream(this.provider, request)

    if (this.streamReadTimeoutSecs !== undefined) {
      stream = readTimeoutStream(stream, this.streamReadTimeoutSecs)
    }

    stream = stripThink(stream)
    return stream
  }
```
  Replace with:
```ts
  /**
   * Send a chat request; validates capabilities BEFORE any HTTP call.
   * Signals (E4/E6): the caller signal plus the opt-in totalMs budget
   * (AbortSignal.timeout, armed across the WHOLE call including retries) are
   * the only whole-call signals. The connect budget is NOT composed here — it
   * rides preHeadersTimeoutMs into http/fetch.ts, where its timer is
   * disarmed the moment headers arrive (see Conformance note).
   */
  async chat(request: ChatRequest, opts?: RequestOptions): Promise<ChatResponse> {
    let signal = opts?.signal
    if (this.totalMs !== undefined) {
      const total = AbortSignal.timeout(this.totalMs)
      signal = signal ? AbortSignal.any([signal, total]) : total
    }
    try {
      return await dispatchChat(this.provider, request, {
        signal,
        callerSignal: opts?.signal,
        preHeadersTimeoutMs: this.connectMs + this.readIdleMs,
      })
    } catch (error) {
      // Covers errors surfacing outside the provider's retry op (e.g.
      // ChatGptCodexProvider.chat consuming its own stream body).
      if (opts?.signal?.aborted && !(error instanceof CancelledError)) {
        throw new CancelledError()
      }
      throw error
    }
  }

  /**
   * Stream a chat request: dispatch (validate -> provider.stream) ->
   * readTimeoutStream(readIdleMs, ALWAYS on; default 120s) -> stripThink ->
   * caller-abort translation. Streams get ONLY the caller signal: totalMs
   * never applies to stream body consumption. preHeadersTimeoutMs bounds the
   * initial fetch at the transport; the consumer-visible deadline for the
   * lazy initial fetch is readTimeoutStream's first next(), which spans it.
   */
  stream(request: ChatRequest, opts?: RequestOptions): AsyncIterable<StreamEvent> {
    let stream: BoxStream = dispatchStream(this.provider, request, {
      signal: opts?.signal,
      callerSignal: opts?.signal,
      preHeadersTimeoutMs: this.connectMs + this.readIdleMs,
    })
    stream = readTimeoutStream(stream, this.readIdleMs / 1000)
    stream = stripThink(stream)
    return this.translateCallerAbort(stream, opts?.signal)
  }

  /** Mid-stream caller abort surfaces as CancelledError, never a raw AbortError (E6). */
  private async *translateCallerAbort(
    inner: BoxStream,
    callerSignal: AbortSignal | undefined,
  ): AsyncIterable<StreamEvent> {
    try {
      for await (const evt of inner) yield evt
    } catch (error) {
      if (callerSignal?.aborted && !(error instanceof CancelledError)) {
        throw new CancelledError()
      }
      throw error
    }
  }
```
  `streamCollect` / `streamCollectWith` (~412-423): add `opts?: RequestOptions` and pass to `this.stream(request, opts)`.

  **3f providers** — thread options into every request seam. In each file add imports `attemptWithCancellation` (from '../retry.js') and `type ProviderRequestOptions` (from '../provider.js'). Exemplar, `src/providers/anthropic.ts` — current code (approximate lines 193-202):
```ts
  async chat(request: ChatRequest): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const serialized = serializeAnthropicRequest(request, model)
    const body = isSetupToken(this.apiKey) ? withOAuthSystemIdentity(serialized) : serialized
    const headers = this.requestHeaders(request, body)
    const payload = await withRetry(
      this.retryPolicy,
      async () => postJson<any>(`${this.baseUrl}/v1/messages`, headers, body),
      classifyForRetry,
    )
```
  Replace with:
```ts
  async chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const serialized = serializeAnthropicRequest(request, model)
    const body = isSetupToken(this.apiKey) ? withOAuthSystemIdentity(serialized) : serialized
    const headers = this.requestHeaders(request, body)
    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<any>(`${this.baseUrl}/v1/messages`, headers, body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )
```
  Same transformation at anthropic `stream`/`streamImpl` (~236-256): `stream(request: ChatRequest, opts?: ProviderRequestOptions): BoxStream { return this.streamImpl(request, opts) }`; `streamImpl` gains `opts?: ProviderRequestOptions` and its `postStream(...)` call becomes `attemptWithCancellation(opts?.callerSignal, () => postStream(`${this.baseUrl}/v1/messages`, headers, body, { signal: opts?.signal, preHeadersTimeoutMs: opts?.preHeadersTimeoutMs }))`. Apply the IDENTICAL two-part transformation (add `opts?: ProviderRequestOptions` to the signature; wrap the postJson/postStream call in `attemptWithCancellation(opts?.callerSignal, () => post...(url, headers, body, { signal: opts?.signal, preHeadersTimeoutMs: opts?.preHeadersTimeoutMs }))`) at each remaining seam:
  - `openai.ts`: `chatViaResponses` (~144 sig, ~210-214 postJson), `chat` (~241 sig, ~247-251 postJson; ~258 `return this.chatViaResponses(request)` -> `return this.chatViaResponses(request, opts)`), `stream`/`streamImpl` (~301-303 forward opts, ~309 sig, ~315-319 postStream)
  - `gemini.ts`: `chat` (~93 sig, ~98-102 postJson `postJson<any>(url, this.headers(), body, {...})`), `stream`/`streamImpl` (~151-155 sigs, ~162-166 postStream)
  - `ollama.ts`: `chat` (~221 sig, ~223-227 `postJson<any>(this.endpoint(), {}, body, {...})`), `stream`/`streamImpl` (~276-288)
  - `chatgpt_codex.ts`: `chat` (~209-214) gains opts and calls `collectStream(this.stream(request, opts))`; `stream`/`streamImpl` (~216-231) forward opts, wrap `postStream(this.baseUrl, headers, body, {...})`
  - `minimax.ts` (~39-45): `chat(request: ChatRequest, opts?: ProviderRequestOptions)` -> `this.inner.chat(request, opts)`; same for `stream` (import `type ProviderRequestOptions` only — no fetch here)

  **3g exports + engines** — `src/index.ts` line ~32 becomes `export type { ProviderCapabilities, Provider, RequestOptions, ProviderRequestOptions } from './provider.js'` (CancelledError and TimeoutSettings already flow through the `export *` of error.js/client.js). `package.json` ~15-17: `"node": ">=18"` -> `"node": ">=20.3"` (AbortSignal.any landed in Node 20.3 — a bare `>=20` would admit 20.0-20.2 where it is undefined; used in Client.chat and http/fetch.ts).

  **3h flip pinned tests (M2-regression-contract exceptions, E7 retires silent-timeout-end)** — all three in `tests/client-builder.test.ts`, `describe('readTimeoutStream')`; add `StreamReadTimeoutError` to the error.js import at line 3. (1) ~232-244 `'silently terminates a stalled stream without throwing'` — rename to `'throws StreamReadTimeoutError on a stalled stream (silent-end retired in M3)'`, keep the stalled generator, replace the for-await/`expect(results.length).toBe(0)` body with `await expect(async () => { for await (const _ of readTimeoutStream(stalled, 0.05)) void _ }).rejects.toThrow(StreamReadTimeoutError)`. (2) ~277-296 `'suppresses rejected return promises when a timeout cancels the inner iterator'` — same rejects-based loop replacement, then KEEP `await new Promise((resolve) => setTimeout(resolve, 0))` and `expect(returnSpy).toHaveBeenCalledOnce()`. (3) ~298-316 `'calls return on the inner iterator when a read timeout terminates the stream'` — same rejects-based loop replacement, keep `expect(returnSpy).toHaveBeenCalledOnce()` (this preserves the M2 abandoned-stream-cancel guarantee: the inner iterator is still cancelled exactly once, now before the throw). The `tests/http.sse.test.ts`/`tests/http.ndjson.test.ts` consumer-exit cancel tests and the `tests/http-fetch.test.ts` postJson/postStream pins are untouched and must stay green.

  **3i `tests/retry-conformance.test.ts`** — change imports (~8-9) to `import { CancelledError, isRetryableStatus, parseRetryAfter } from '../src/error.js'` and `import { classifyForRetry, RetryPolicy } from '../src/retry.js'`; append:
```ts
describe('specs/retry.md § transport classification (M3 amendment)', () => {
  // Coordination: the M3 spec task adds the CancelledError row to
  // specs/retry.md's transport table; this TS suite is the ONLY conformance
  // suite that changes — Rust (drop-based cancellation) and Python (asyncio
  // CancelledError propagates untouched) had no classification change, so
  // their suites need NO new row (Task 9 verifies exactly that).
  it('CancelledError (caller-supplied signal aborted) is NEVER retryable', () => {
    expect(classifyForRetry(new CancelledError()).retryable).toBe(false)
  })

  it('fetch-internal AbortError / TimeoutError (no caller signal) stay retryable', () => {
    const abort = new Error('aborted')
    abort.name = 'AbortError'
    const timeout = new Error('timed out')
    timeout.name = 'TimeoutError'
    expect(classifyForRetry(abort).retryable).toBe(true)
    expect(classifyForRetry(timeout).retryable).toBe(true)
  })
})
```

- [ ] 4. From `sdks/typescript`: `npx vitest run tests/timeouts-cancellation.test.ts tests/client-builder.test.ts tests/retry-conformance.test.ts tests/retry-integration.test.ts tests/http-fetch.test.ts tests/http.sse.test.ts tests/http.ndjson.test.ts` — all green. Then the full package suite: `npm run build && npm test`. All M1 retry tests and M2 conformance/cancel tests must pass unchanged except the three flips listed in 3h.

- [ ] 5. `npm run typecheck` (the TS package has no separate lint/format script; tsc --noEmit is the gate).

- [ ] 6. Commit on a feature branch and open a PR (house rule: CI on every change):
```
feat(typescript)!: one timeout model, per-request cancellation, throwing read-idle timeout

E4: ClientBuilder.timeouts({connectMs=10s, readIdleMs=120s, totalMs=off}).
totalMs (opt-in) is composed via AbortSignal.timeout/any onto chat fetches
and stays armed across the whole call; the connect budget is a
disarm-at-headers AbortController timer in http/fetch.ts (pre-headers
deadline = connectMs + readIdleMs) because fetch cannot scope
AbortSignal.timeout to the connect phase — it never bounds body reads, so
slow non-streaming generations are unaffected. E6: Client.chat/stream accept
{ signal }; caller abort => CancelledError, never retried; fetch-internal
AbortError/TimeoutError stay retryable. E7: readTimeoutStream now THROWS
StreamReadTimeoutError (silent-end retired).

BREAKING CHANGE: Client.stream always applies a 120s read-idle timeout and
throws StreamReadTimeoutError on expiry; Client constructor's second
parameter is now TimeoutSettings; Node >= 20 required.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```


## C — Milestone-Done conformance

### Task 9: Add M3 milestone-Done stream-termination and read-idle conformance gates across all three SDKs

> **Execute LAST before release — consumes every E1/E4/E7 surface.** These are the milestone-Done gates from `docs/superpowers/plans/2026-07-14-stream-retry-milestones.md` § M3: "kill-the-connection-mid-stream test per SDK yields the typed error, not a clean response; hung-stream test hits the read timeout everywhere". On the completed M3 tree they PASS; any failure is a regression in the owning E-task (fix there, never by weakening these tests).
>
> **Why these homes (not `providers::retry_conformance`):** Rust's M2 conformance suite is an in-crate unit module (`sdks/rust/src/providers/mod.rs` ~lines 756-894) that exists there only because `is_retryable_status`/`parse_retry_after` are `pub(crate)` and feature-gated — it asserts spec *tables*, and cannot open a socket against the built adapter stack. The M3 gates are *behavioral* (real HTTP server, truncated/stalled bodies; full `Client` dispatch where the enforcement point demands it — Rust's `ReadTimeoutStream` lives in `Client::dispatch_stream`, while the Python gates exercise the providers directly, which is where Python's enforcement lives), so they belong in the integration homes where every wire-shaped stream test already lives: `tests/anthropic_stream.rs` is the established mockito home for the Anthropic stream contract (M1's error-surfacing tests live there, and it already holds a client-adjacent test `client_stream_with_dispatches_to_provider` ~line 237); the Rust hung-stream test additionally MUST go through `Client` because `ReadTimeoutStream` is applied only in `Client::dispatch_stream` (client.rs ~386), never inside providers. Python's home is `tests/test_anthropic_stream_usage.py` (M1's mid-stream error-surfacing tests: malformed chunk ~107, error frame ~131; `_sse` helper + respx idiom). TS's home is `tests/edge-cases.test.ts` (holds the flipped mid-stream-reset tests and the `stubSseFetch`/`sseStream` fetch-stub helpers).
>
> Anthropic is used in all three SDKs deliberately: it is the one provider with an identical SSE wire shape everywhere, making the three tests literal cross-SDK mirrors.

**Files:**
- `sdks/rust/tests/anthropic_stream.rs` — imports ~lines 1-8; append two tests after the last test (~line 832)
- `sdks/python/tests/test_anthropic_stream_usage.py` — import ~line 7; append helper + two tests after ~line 157
- `sdks/typescript/tests/edge-cases.test.ts` — import ~line 10; append one new top-level `describe` after ~line 233
- Verify-only, NO edits: `sdks/rust/src/providers/mod.rs` (mod `retry_conformance`, ~756-894), `sdks/python/tests/test_retry_conformance.py` (~57-62), `sdks/typescript/tests/retry-conformance.test.ts`, `specs/retry.md` (transport table ~44-50)

**Interfaces:** Consumes (E-spellings verbatim; all land in earlier M3 tasks): Rust `#[error("incomplete stream: {0}")] IncompleteStream(String)` on `MotosanError` (E1) and `ClientBuilder::read_idle_timeout(Duration)` (E4; supersedes `stream_read_timeout_secs`, client.rs ~91/~962), plus existing `MotosanError::StreamReadTimeout(u64)` (error.rs ~41). Python `class IncompleteStreamError(StreamError)` (E1) and `StreamReadTimeoutError` from `motosan_ai/error.py` (Python timeout task). TS `export class IncompleteStreamError extends StreamError` (E1), `ClientBuilder.timeouts({connectMs?, readIdleMs?, totalMs?})` (E4), `StreamReadTimeoutError` (error.ts ~18) actually thrown by `readTimeoutStream` (E7, provider.ts ~141). Message convention everywhere: `"incomplete stream: <provider> ended without a terminal event"`. Produces: test code only — no new interfaces.

**Steps:**

- [ ] **1. Write the six conformance tests (complete code).**

  **Rust — `sdks/rust/tests/anthropic_stream.rs`.** Extend imports (current ~lines 1-8):
  ```rust
  use motosan_ai::{ChatRequest, Client, Message, MotosanError, Provider, StopReason, StreamEventType, Tool};
  use std::io::Write;
  use std::time::Duration;
  ```
  Append at end of file (~line 832):
  ```rust
  // ---------------------------------------------------------------------------
  // M3 milestone-Done conformance gates (specs/types.md § stream termination).
  // Execute LAST before release — consumes the E1 (IncompleteStream) and E4
  // (read_idle_timeout) surfaces end-to-end. Cross-SDK mirrors:
  //   sdks/python/tests/test_anthropic_stream_usage.py
  //   sdks/typescript/tests/edge-cases.test.ts (M3 stream termination describe)
  // ---------------------------------------------------------------------------

  #[tokio::test]
  async fn anthropic_stream_eof_without_terminal_event_yields_incomplete_stream() {
      let mut server = mockito::Server::new_async().await;
      // Connection killed mid-stream: text arrives, then the body ends with
      // NO message_stop terminal event.
      let sse_body = concat!(
          "event: message_start\n",
          "data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":0}}}\n\n",
          "event: content_block_delta\n",
          "data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"partial\"}}\n\n"
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
      let request = ChatRequest::builder().message(Message::user("hi")).build();

      let mut stream = provider.stream(request).await.expect("stream response");
      let mut contents = Vec::new();
      let mut terminal_err = None;
      while let Some(item) = stream.next().await {
          match item {
              Ok(event) => {
                  assert!(!event.done, "no done event may be fabricated on EOF");
                  contents.push(event.content.clone());
              }
              Err(err) => {
                  terminal_err = Some(err);
                  break;
              }
          }
      }
      assert!(
          contents.iter().any(|t| t == "partial"),
          "text before the drop is still yielded, got {contents:?}"
      );
      let err = terminal_err.expect("EOF without message_stop must yield a typed error");
      assert!(matches!(err, MotosanError::IncompleteStream(_)), "got {err:?}");
      assert_eq!(
          err.to_string(),
          "incomplete stream: anthropic ended without a terminal event"
      );
      mock.assert_async().await;
  }

  #[tokio::test]
  async fn client_stream_read_idle_timeout_yields_stream_read_timeout() {
      let mut server = mockito::Server::new_async().await;
      // One flushed chunk, then the connection goes silent far longer than the
      // read-idle deadline. Client-level on purpose: ReadTimeoutStream is
      // applied in Client::dispatch_stream (client.rs ~386), not in providers.
      let _mock = server
          .mock("POST", "/v1/messages")
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_chunked_body(|w| {
              // Text must be LONGER than ThinkStripper's hold-back ("<think>".len()-1 = 6
              // chars): Client.stream() wraps adapters in ThinkStripper, which buffers the
              // trailing 6 chars and does NOT flush them when the timeout Err arrives — a
              // short "tick" would never reach the consumer before the timeout.
              w.write_all(b"event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"tick-tick-tick\"}}\n\n")?;
              w.flush()?;
              std::thread::sleep(std::time::Duration::from_secs(2));
              // Client hung up at ~250ms; ignore the write error on the dead socket.
              let _ = w.write_all(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
              Ok(())
          })
          .create_async()
          .await;

      let client = Client::builder()
          .provider(Provider::Anthropic)
          .api_key("test-key")
          .anthropic_base_url(server.url())
          .read_idle_timeout(Duration::from_millis(250))
          .build()
          .expect("client build");

      let mut stream = client
          .stream(vec![Message::user("hi")])
          .await
          .expect("stream open");

      let first = stream
          .next()
          .await
          .expect("first event")
          .expect("first chunk arrives before the stall");
      // ThinkStripper holds back the trailing 6 chars, so assert the flushed prefix.
      assert!(
          first.content.starts_with("tick"),
          "expected the pre-stall text to flush through ThinkStripper, got: {:?}",
          first.content
      );

      // The typed error IS the contract: Network/Stream here means the E4
      // wiring regressed (reqwest-level timeout beat ReadTimeoutStream).
      match stream.next().await.expect("timeout item") {
          Err(MotosanError::StreamReadTimeout(_)) => {}
          other => panic!("expected StreamReadTimeout on read-idle expiry, got {other:?}"),
      }
      // Stream terminates after the typed error — no fabricated done.
      assert!(stream.next().await.is_none());
  }
  ```
  (No `mock.assert_async()` on the stalled mock — its handler thread is still sleeping when the test finishes. `with_chunked_body` is the mockito 1.x API — `mockito = "1"` per `sdks/rust/Cargo.toml` ~110; if the resolved minor spells it `with_body_from_fn`, use that alias.)

  **Python — `sdks/python/tests/test_anthropic_stream_usage.py`.** Change import (current ~line 7 `from motosan_ai.error import StreamError`) to:
  ```python
  from motosan_ai.error import IncompleteStreamError, StreamError, StreamReadTimeoutError
  ```
  Append at end of file (~line 157):
  ```python
  # ---------------------------------------------------------------------------
  # M3 milestone-Done conformance gates (specs/types.md § stream termination).
  # Execute LAST before release. Cross-SDK mirrors:
  #   sdks/rust/tests/anthropic_stream.rs
  #   sdks/typescript/tests/edge-cases.test.ts (M3 stream termination describe)
  # ---------------------------------------------------------------------------


  class _StallThenReadTimeout(httpx.AsyncByteStream):
      """One SSE chunk, then the socket goes silent past the read deadline.

      respx replaces the real transport, so httpx's read timer cannot fire on
      a real socket here; raising httpx.ReadTimeout from the byte stream is
      exactly what httpx surfaces mid-iteration when the peer stalls.
      """

      def __init__(self, first_chunk: bytes) -> None:
          self._first_chunk = first_chunk

      async def __aiter__(self):
          yield self._first_chunk
          raise httpx.ReadTimeout("read timed out")


  @respx.mock
  @pytest.mark.asyncio
  async def test_stream_eof_without_message_stop_raises_incomplete_stream(provider):
      # Kill-the-connection mid-stream: text arrives, then EOF with NO message_stop.
      sse = _sse(
          {"type": "message_start", "message": {"usage": {"input_tokens": 3, "output_tokens": 0}}},
          {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "par"}},
      )
      respx.post("https://mock.anthropic.com/v1/messages").mock(
          return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
      )
      seen = []
      with pytest.raises(
          IncompleteStreamError,
          match="incomplete stream: anthropic ended without a terminal event",
      ):
          async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
              seen.append(ev)
      assert any(e.content == "par" for e in seen)  # partial text still yielded first
      assert not any(e.done for e in seen)  # no fabricated done event
      # E1 migration softener: existing `except StreamError` call sites still catch it.
      assert issubclass(IncompleteStreamError, StreamError)


  @respx.mock
  @pytest.mark.asyncio
  async def test_stream_read_idle_timeout_raises_stream_read_timeout_error(provider):
      first = b'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"par"}}\n\n'
      respx.post("https://mock.anthropic.com/v1/messages").mock(
          return_value=httpx.Response(
              200,
              stream=_StallThenReadTimeout(first),
              headers={"content-type": "text/event-stream"},
          )
      )
      seen = []
      with pytest.raises(StreamReadTimeoutError):
          async for ev in provider.stream(ChatRequest(messages=[Message.user("hi")])):
              seen.append(ev)
      assert any(e.content == "par" for e in seen)  # text before the stall still yielded
  ```

  **TypeScript — `sdks/typescript/tests/edge-cases.test.ts`.** Change import (current ~line 10 `import { MotosanError, ProviderError } from '../src/error.js'`) to:
  ```ts
  import { IncompleteStreamError, MotosanError, ProviderError, StreamError, StreamReadTimeoutError } from '../src/error.js'
  ```
  Append a new top-level describe after the closing `})` of `describe('edge cases')` (~line 233):
  ```ts
  // ---------------------------------------------------------------------------
  // M3 milestone-Done conformance gates (specs/types.md § stream termination).
  // Execute LAST before release — consumes E1 (IncompleteStreamError), E4
  // (.timeouts) and E7 (readTimeoutStream throws) end-to-end through Client.
  // Cross-SDK mirrors: sdks/rust/tests/anthropic_stream.rs and
  // sdks/python/tests/test_anthropic_stream_usage.py.
  // ---------------------------------------------------------------------------
  describe('M3 stream termination conformance (milestone Done gates)', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('kill-the-connection mid-stream: Client.stream rejects with IncompleteStreamError, not a clean response', async () => {
      const transcript =
        'event: message_start\n' +
        'data: {"type":"message_start","message":{"usage":{"input_tokens":5,"output_tokens":0}}}\n\n' +
        'event: content_block_start\n' +
        'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"partial"}}\n\n'
      // No message_stop — the connection died mid-flight.
      stubSseFetch(transcript)

      const client = Client.builder().provider('anthropic').apiKey('sk-ant-api03-x').build()
      const events: StreamEvent[] = []
      const err = await (async () => {
        for await (const evt of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
          events.push(evt)
        }
      })().then(
        () => null,
        (e: unknown) => e,
      )

      expect(err).toBeInstanceOf(IncompleteStreamError)
      expect(err).toBeInstanceOf(StreamError) // E1 migration softener: catch StreamError still works
      expect((err as Error).message).toBe('incomplete stream: anthropic ended without a terminal event')
      expect(
        events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content),
      ).toEqual(['partial'])
      expect(events.some((e) => e.done)).toBe(false) // no fabricated done
    })

    it('hung stream: read-idle expiry rejects with StreamReadTimeoutError', async () => {
      const firstChunk =
        'event: content_block_start\n' +
        'data: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n' +
        'event: content_block_delta\n' +
        'data: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"tick"}}\n\n'
      vi.stubGlobal(
        'fetch',
        vi.fn(
          async () =>
            new Response(
              new ReadableStream<Uint8Array>({
                start(controller) {
                  controller.enqueue(new TextEncoder().encode(firstChunk))
                  // Never enqueue again, never close — the connection hangs.
                },
              }),
              { status: 200, headers: { 'content-type': 'text/event-stream' } },
            ),
        ),
      )

      const client = Client.builder()
        .provider('anthropic')
        .apiKey('sk-ant-api03-x')
        .timeouts({ readIdleMs: 50 })
        .build()

      const events: StreamEvent[] = []
      const err = await (async () => {
        for await (const evt of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
          events.push(evt)
        }
      })().then(
        () => null,
        (e: unknown) => e,
      )

      expect(err).toBeInstanceOf(StreamReadTimeoutError)
      expect(events.filter((e) => e.eventType === 'text').map((e) => e.content)).toEqual(['tick'])
    })
  })
  ```

- [ ] **2. Run the targeted suites.** This task executes LAST, so on the completed M3 tree all six tests must PASS:
  - Rust (from `sdks/rust`): `cargo test --all-features --test anthropic_stream` → all tests in the binary `ok`, including the two new names.
  - Python (from `sdks/python`): `uv run pytest tests/test_anthropic_stream_usage.py -v` → all pass, including `test_stream_eof_without_message_stop_raises_incomplete_stream` and `test_stream_read_idle_timeout_raises_stream_read_timeout_error`.
  - TS (from `sdks/typescript`): `npx vitest run tests/edge-cases.test.ts` → all pass, including the two new `it` blocks.

  Regression diagnosis (each test's pre-M3 failure signature — proves the gate is not vacuous; a reappearance means the named E-task regressed): Rust drop → panic `EOF without message_stop must yield a typed error` (adapter ended silently; E1/E2). Rust hung → compile error `no method named read_idle_timeout` or a `Network`/`Stream` error item instead of `StreamReadTimeout` (E4). Python drop → `Failed: DID NOT RAISE` (anthropic.py stream loop ~417-515 ended silently; E1/E2). Python hung → `ImportError: cannot import name 'StreamReadTimeoutError'` or `StreamError("stream transport error: …")` raised via the catch-all at anthropic.py ~525 (E4/E7-py). TS drop → `err` is `null` because a `done` event was fabricated (the pre-M3 TS Anthropic adapter's EOF fallback, ~363-368 at the M3 baseline — grep for the EOF `doneEvent()` path if drifted; E2/E3). TS hung → `TypeError: ….timeouts is not a function` (E4) or `err` is `null` because `readTimeoutStream` ended silently (provider.ts ~141; E7).

- [ ] **3. No production code changes — plus the mandated coordination checks (verify-only, NO edits).** This task's entire diff is the three test files from Step 1. Then verify the M2 conformance suites against the E9-amended `specs/retry.md`:
  1. **Rust suite needs NO change.** Current code (approximate lines 756-894 of `sdks/rust/src/providers/mod.rs`): `mod retry_conformance` asserts only the status-classification table (`RETRYABLE`/`NON_RETRYABLE` arrays), `parse_retry_after` clamping, full-jitter bounds, and `RetryPolicy` defaults. It asserts **no transport-error rows at all**, and M3 does not touch Rust transport classification (`is_retryable_network_error` unchanged) — so the spec's new TS-only `CancelledError` transport row cannot be pinned here. No replacement — record "verified: no change needed" in the PR description.
  2. **Python suite needs NO change.** Current code (approximate lines 57-58 of `sdks/python/tests/test_retry_conformance.py`): `def test_network_error_is_retryable(self): assert _is_retryable(NetworkError("connection reset")) is True` — the only transport assertion in the file, and the Python `NetworkError → always retryable` row in `specs/retry.md` (~line 49) is untouched by M3. No replacement — record the check in the PR description.
  3. **TS suite is NOT edited here.** The `CancelledError` transport-table row (caller-aborted signal → `CancelledError`, never retried) is owned by the ts-timeouts-cancel task. Verify it landed: `grep -n "CancelledError" sdks/typescript/tests/retry-conformance.test.ts` must return at least one match. If it returns nothing, STOP — that task has not landed; do not add the row here.
  4. **M2 regression contract:** this task flips nothing. The E2/E3 adapter tasks own the file-by-file flips (TS `edge-cases.test.ts` "mid-stream reset / partial success" block ~124-189; Rust `openai_provider.rs` `openai_stream_eof_flush_when_done_sentinel_missing` ~752). Confirm `git status` / `git diff --stat` shows exactly the three Step-1 test files and nothing else.

- [ ] **4. Run each full package suite.**
  - Rust (from `sdks/rust`): `cargo test --all-features` → all green.
  - Python (from `sdks/python`): `uv run pytest tests/ -v` → all green.
  - TS (from `sdks/typescript`): `npm run build && npm test` and `npm run typecheck` → all green (full suite's `pack-smoke.test.ts` requires `dist/`).

- [ ] **5. Format and lint.**
  - Rust (from `sdks/rust`): `cargo fmt` then `cargo clippy --all-features --all-targets -- -D warnings` (CI lints tests; `--all-targets` mandatory).
  - Python (from `sdks/python`): `uv run ruff format` then `uv run ruff check motosan_ai/` (tests/ not linted).
  - TS: `npm run typecheck` already run in Step 4.

- [ ] **6. Commit on a branch and open a PR (a `.rs` file changed → PR + CI is mandatory).**
  ```bash
  git checkout -b test/m3-milestone-done-conformance
  git add sdks/rust/tests/anthropic_stream.rs \
          sdks/python/tests/test_anthropic_stream_usage.py \
          sdks/typescript/tests/edge-cases.test.ts
  git commit -m "test(stream): add M3 milestone-Done kill-connection and hung-stream conformance gates" \
             -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```
  PR description must state the Step-3 verification results verbatim: rust/python M2 conformance suites verified unchanged (their transport tables were not amended by M3); TS CancelledError row verified present (owned by the ts-timeouts-cancel task).


## Release

### Task 10: Cut the M3 release: Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0 (no tag, no publish)

Last task of M3 — runs only after every other M3 task is merged. Version bumps, lockfiles, changelogs with migration guidance, and rewriting every doc paragraph that still documents the retired "exactly one `done`" invariant (E3). Follows `llms.txt` § Release (read it first) with the M2 precedent of ONE combined release commit. **NO `git tag`, NO publish** — tagging happens after PR merge, outside this task.

**Files:** (all line refs approximate — measured pre-M3 at acf5d7f; earlier M3 tasks shift them, so grep before editing)
- `sdks/rust/Cargo.toml` (~3), `sdks/python/pyproject.toml` (~3), `sdks/typescript/package.json` (~3)
- `uv.lock` (root; motosan-ai block ~97), `sdks/typescript/package-lock.json` (~3, ~9). `Cargo.lock` is gitignored (`/Cargo.lock` in `.gitignore`) — NEVER staged.
- `CHANGELOG.md` (new section at ~5), `sdks/rust/CHANGELOG.md` (~5, below `## [Unreleased]`), `sdks/python/CHANGELOG.md` (~5), `sdks/typescript/CHANGELOG.md` (~7)
- `AGENTS.md` (~5 header, insert M3 paragraph after ~13), `llms.txt` (~5 header, insert M3 bullet after ~7, ~24 Install, ~454 invariant prose, ~923 tag-table example), `README.md` (~29-31 Languages, ~38 Install, ~239 invariant comment), `skills/motosan-ai/SKILL.md` (~8 header, ~25 Install, ~131 invariant bullet), `skills/motosan-ai/references/rust-api.md` (~7), `sdks/rust/README.md` (~50 invariant paragraph; Cargo snippets ~323, ~433, ~497), `sdks/typescript/README.md` (~207 invariant paragraph, ~211-216 "Mid-stream partial success" blockquote, ~485-486 tag example)

**Interfaces:** Consumes (documented, not defined — copy spellings verbatim): Rust `MotosanError::IncompleteStream(String)` with `#[error("incomplete stream: {0}")]`; Python `class IncompleteStreamError(StreamError)`; TS `export class IncompleteStreamError extends StreamError` and `export class CancelledError extends MotosanError`; message convention `"incomplete stream: <provider> ended without a terminal event"`; Rust `ClientBuilder::connect_timeout(Duration) / .read_idle_timeout(Duration) / .total_timeout(Duration)`; Python `Client(..., connect_timeout: float = 10.0, read_idle_timeout: float = 120.0, total_timeout: float | None = None)`; TS `ClientBuilder.timeouts({connectMs?, readIdleMs?, totalMs?})`. Produces: None (docs/manifests/locks only — no code).

**Steps:**

- [ ] 1. **Failing test** — release tasks have no unit-test seam; the test is a version-verification script. Write `/tmp/m3-verify-release.sh` (do NOT commit it):

```bash
#!/usr/bin/env bash
# M3 release verification — run from the repo root. Exit 0 only when fully bumped.
set -u
fail=0
ck() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "ok   $d"; else echo "FAIL $d"; fail=1; fi; }
no() { local d="$1"; shift; if "$@" >/dev/null 2>&1; then echo "FAIL $d"; fail=1; else echo "ok   $d"; fi; }
# Manifests + locks
ck "Cargo.toml 0.24.0"          grep -q '^version = "0.24.0"' sdks/rust/Cargo.toml
ck "pyproject.toml 0.17.0"      grep -q '^version = "0.17.0"' sdks/python/pyproject.toml
ck "package.json 0.14.0"        grep -q '"version": "0.14.0"' sdks/typescript/package.json
ck "uv.lock motosan-ai 0.17.0"  sh -c 'grep -A1 "name = \"motosan-ai\"" uv.lock | grep -q "version = \"0.17.0\""'
ck "package-lock 0.14.0"        grep -q '"version": "0.14.0"' sdks/typescript/package-lock.json
# Changelogs
ck "root CHANGELOG section"     grep -q '^## \[rust-0.24.0 / python-0.17.0 / ts-0.14.0\]' CHANGELOG.md
ck "rust CHANGELOG 0.24.0"      grep -q '^## \[0.24.0\]' sdks/rust/CHANGELOG.md
ck "python CHANGELOG 0.17.0"    grep -q '^## \[0.17.0\]' sdks/python/CHANGELOG.md
ck "ts CHANGELOG 0.14.0"        grep -q '^## \[0.14.0\]' sdks/typescript/CHANGELOG.md
# Version lines
ck "AGENTS.md header"           grep -q 'Rust v0.24.0 · Python v0.17.0 (PyPI) · TypeScript v0.14.0 (npm)' AGENTS.md
ck "llms.txt header"            grep -q 'Python 0.17.0 · TypeScript 0.14.0 · Rust 0.24.0' llms.txt
ck "SKILL.md header"            grep -q 'Python 0.17.0 / Rust 0.24.0 / TypeScript 0.14.0' skills/motosan-ai/SKILL.md
ck "README Rust v0.24.0"        grep -q 'v0.24.0' README.md
ck "README Python v0.17.0"      grep -q 'v0.17.0' README.md
ck "README TS v0.14.0"          grep -q 'v0.14.0' README.md
ck "rust-api.md 0.24.0"         grep -q '"0.24.0"' skills/motosan-ai/references/rust-api.md
# Stale current-version strings (CHANGELOG history + docs/superpowers plans legitimately keep old versions)
no "no stale 0.23.0 install snippets" sh -c 'grep -rn "motosan-ai = { version = \"0.23.0\"" --include="*.md" --include="*.txt" . | grep -v CHANGELOG | grep -v docs/superpowers | grep -q .'
no "no stale ts-v0.13.0 tag examples" grep -n 'ts-v0.13.0' llms.txt sdks/typescript/README.md
no "Cargo.toml not 0.23.0"      grep -q '^version = "0.23.0"' sdks/rust/Cargo.toml
no "pyproject not 0.16.0"       grep -q '^version = "0.16.0"' sdks/python/pyproject.toml
no "package.json not 0.13.0"    grep -q '"version": "0.13.0"' sdks/typescript/package.json
# Retired-invariant prose must be gone from user-facing docs (E3).
# (AGENTS.md and the CHANGELOGs are deliberately NOT in this grep list — their M3 entries
#  NAME the retired invariant when describing the change. The five files below must not
#  contain the phrase at all; the step-3g replacement texts are worded to avoid it.
#  Keep this pattern broad: README.md ~239 spells it "emit exactly one" without bold.)
no "no 'exactly one' prose"     grep -rn 'exactly one' README.md llms.txt sdks/rust/README.md sdks/typescript/README.md skills/motosan-ai/SKILL.md
no "no silent-truncation prose" grep -n 'does NOT throw mid-stream' sdks/typescript/README.md
exit $fail
```

- [ ] 2. **Run it — expect failure.** From the repo root: `bash /tmp/m3-verify-release.sh; echo "exit=$?"`. Expected signature: `FAIL` on every `ck` line (at minimum `FAIL Cargo.toml 0.24.0`, `FAIL root CHANGELOG section`, `FAIL AGENTS.md header`) plus `FAIL no 'exactly one' prose`, ending `exit=1`.

- [ ] 3. **Implement**, in this order:

  **3a. Fold `[Unreleased]`.** Earlier M3 tasks may have parked bullets under `## [Unreleased]` in the per-SDK changelogs. Move any such content into the new version sections below (dedupe against them); leave `## [Unreleased]` present but empty in `sdks/rust/CHANGELOG.md` (existing style).

  **3b. Manifests** (one-line bumps): `sdks/rust/Cargo.toml` ~3 `version = "0.23.0"` → `version = "0.24.0"`; `sdks/python/pyproject.toml` ~3 `version = "0.16.0"` → `version = "0.17.0"`; `sdks/typescript/package.json` ~3 `"version": "0.13.0",` → `"version": "0.14.0",`.

  **3c. Lockfiles.** From the repo root: `uv lock --project sdks/python` (root `uv.lock` is the workspace lock — root `pyproject.toml` has `[tool.uv.workspace] members = ["sdks/python"]`; confirm the `name = "motosan-ai"` block now says `version = "0.17.0"`). Then `cd sdks/typescript && npm install --package-lock-only && cd ../..` (confirm `package-lock.json` lines ~3/~9 say `0.14.0`). If a root `Cargo.lock` was regenerated by any build, leave it alone — it is gitignored and must never be staged.

  **3d. Root `CHANGELOG.md`** — insert directly above the `## [rust-0.23.0 / python-0.16.0 / ts-0.13.0]` section (use today's date in place of `YYYY-MM-DD`, here and in 3e):

````markdown
## [rust-0.24.0 / python-0.17.0 / ts-0.14.0] — YYYY-MM-DD

M3 stream-termination + timeout/lifecycle release. **Breaking for all three SDKs**: a stream that ends without the provider's terminal event is now a typed error, not a fabricated clean `done`.

### Breaking

- **Stream termination contract** (Rust · Python · TypeScript): when the upstream byte/event stream ends WITHOUT the provider terminal event (OpenAI `[DONE]`, Anthropic `message_stop`, Gemini / chatgpt-codex terminal frames), the stream adapter yields a typed error — message `"incomplete stream: <provider> ended without a terminal event"` — instead of fabricating a terminal `done`. This retires the v0.10.1 "exactly one `done` event even when upstream closes without `[DONE]`" invariant. Collectors keep propagating errors and keep the stop-reason heuristic only for a real terminal event that lacks a reason. See `specs/types.md` § stream termination.
- **`MotosanError::IncompleteStream(String)`** (Rust): new enum variant — breaking for exhaustive matches. Migration example in `sdks/rust/CHANGELOG.md`.
- Python `IncompleteStreamError` subclasses `StreamError`; TypeScript `IncompleteStreamError extends StreamError` — existing `except StreamError:` / `instanceof StreamError` handlers keep catching truncation (deliberate migration softener).

### Added

- **One timeout model** (Rust · Python · TypeScript): connect = 10 s default, read-idle (per-chunk gap on streaming reads — see the blessed E4 narrowing) = 120 s default, total = off by default (opt-in; blocking `chat()` only, never silently applied to streams). Rust `ClientBuilder::connect_timeout(Duration) / .read_idle_timeout(Duration) / .total_timeout(Duration)`; Python `Client(..., connect_timeout=10.0, read_idle_timeout=120.0, total_timeout=None)` threaded into every provider `httpx.AsyncClient`; TypeScript `ClientBuilder.timeouts({connectMs?, readIdleMs?, totalMs?})`.
- **Per-request cancellation** (TypeScript): a caller-supplied `AbortSignal` threads Client → provider → `postJson`/`postStream`; a caller-aborted request throws the new `CancelledError extends MotosanError` and is **never retried** (fetch-internal `AbortError` with no caller signal aborted stays retryable). `specs/retry.md` transport table amended; the TS conformance suite gains the CancelledError row (Rust/Python suites unchanged — their transport tables did not change).
- **Python client lifecycle** (Python): `Client.aclose()` and `async with` close every provider `httpx.AsyncClient`; CLI provider `.timeout()` / `.no_timeout()` reachable through the `Client` facade.
- **Specs** (all SDKs): `specs/types.md` stream-termination contract section (terminal-event rule, `IncompleteStream`, retired invariant); `specs/retry.md` `CancelledError` row + note that read-idle timeout errors are not retried mid-stream; Rust drop-cancellation documented.

### Changed

- **Rust build-once providers** (Rust): `ClientBuilder::build()` constructs the provider once with one shared `reqwest::Client` (configured with `connect_timeout`); `dispatch_chat` / `dispatch_stream` no longer rebuild the provider and connection pool per request. `read_idle_timeout` supersedes `stream_read_timeout_secs`.
- **Python MiniMax timeout outlier** (Python): the hardcoded 30 s httpx timeout joins the shared timeout model.

### Fixed

- **`readTimeoutStream` throws** (TypeScript): idle expiry now throws `StreamReadTimeoutError` instead of silently ending the stream; read-idle default 120 s wired through providers.
- **`gemini_code_assist` retry policy** (Rust): the pre-built provider path applies `ClientBuilder::retry_policy` instead of silently discarding it.

Per-SDK detail: [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md), [`sdks/python/CHANGELOG.md`](sdks/python/CHANGELOG.md), [`sdks/typescript/CHANGELOG.md`](sdks/typescript/CHANGELOG.md).
````

  **3e. Per-SDK changelogs.** `sdks/rust/CHANGELOG.md` — insert below `## [Unreleased]`, above `## [0.23.0]`:

````markdown
## [0.24.0] - YYYY-MM-DD

### Breaking
- `MotosanError` gains `#[error("incomplete stream: {0}")] IncompleteStream(String)` — exhaustive `match`es need a new arm:

  ```rust
  // 0.23 — exhaustive match compiles without the new arm
  match err {
      MotosanError::Stream(msg) => eprintln!("stream failed: {msg}"),
      other => return Err(other),
  }
  // 0.24 — add an IncompleteStream arm (or a catch-all)
  match err {
      MotosanError::Stream(msg) => eprintln!("stream failed: {msg}"),
      MotosanError::IncompleteStream(msg) => eprintln!("truncated: {msg}"),
      other => return Err(other),
  }
  ```

- Stream EOF semantics: a provider stream that ends **without** the provider's terminal event (OpenAI `[DONE]`, Anthropic `message_stop`, Gemini / chatgpt-codex terminal frames) now yields `Err(MotosanError::IncompleteStream(_))` — `"incomplete stream: <provider> ended without a terminal event"` — instead of fabricating a final `done` event. Retires the v0.10.1 "exactly one `done` event" invariant. Handling truncation:

  ```rust
  while let Some(item) = stream.next().await {
      match item {
          Ok(event) => {
              if event.done { break; }              // real provider terminal event
              print!("{}", event.content);
          }
          Err(MotosanError::IncompleteStream(msg)) => {
              // Upstream closed without its terminal event; events so far are partial.
              eprintln!("truncated: {msg}");
              break;
          }
          Err(other) => return Err(other),
      }
  }
  ```

### Added
- `ClientBuilder::connect_timeout(Duration)` (default 10 s), `.read_idle_timeout(Duration)` (default 120 s; supersedes `stream_read_timeout_secs` — same per-chunk semantics, now always-on for HTTP streams; streaming reads only per the blessed E4 narrowing), `.total_timeout(Duration)` (opt-in; blocking `chat()` only).

### Changed
- `ClientBuilder::build()` constructs the provider once with a single shared `reqwest::Client` (configured with `connect_timeout`); `dispatch_chat`/`dispatch_stream` no longer rebuild the provider per request.

### Fixed
- Pre-built `gemini_code_assist` provider honors `ClientBuilder::retry_policy` (previously silently discarded).
````

  `sdks/python/CHANGELOG.md` — insert above `## [0.16.0]`:

````markdown
## [0.17.0] - YYYY-MM-DD

### Breaking
- Stream EOF semantics: an HTTP provider stream that ends without the provider's terminal event (OpenAI `[DONE]`, Anthropic `message_stop`, Gemini / chatgpt-codex terminal frames) now raises `IncompleteStreamError` — `"incomplete stream: <provider> ended without a terminal event"` — instead of ending as if the turn had completed. `IncompleteStreamError` subclasses `StreamError`, so existing `except StreamError:` handlers keep working unchanged; catch `IncompleteStreamError` first to treat truncation specially.

### Added
- `Client(..., connect_timeout=10.0, read_idle_timeout=120.0, total_timeout=None)` threaded into every provider `httpx.AsyncClient` via `httpx.Timeout(...)`; `total_timeout` applies to blocking `chat()` only, never silently to streams.
- `Client.aclose()` and `async with Client...:` close every provider `AsyncClient`.
- CLI providers' `.timeout()` / `.no_timeout()` reachable through the `Client` facade (copy the exact kwarg name from the merged E8 code).

### Changed
- MiniMax's hardcoded 30 s httpx timeout is unified into the shared timeout model (10 s connect / 120 s read-idle defaults).
````

  `sdks/typescript/CHANGELOG.md` — insert above `## [0.13.0]`:

````markdown
## [0.14.0] - YYYY-MM-DD

### Breaking
- Stream EOF semantics: a provider stream that ends without the provider's terminal event now throws `IncompleteStreamError` — `"incomplete stream: <provider> ended without a terminal event"` — instead of terminating silently with a partial, success-looking response. `IncompleteStreamError extends StreamError`, so existing `instanceof StreamError` handlers keep working unchanged.
- Retry classification: a request aborted by a **caller-supplied** `AbortSignal` throws the new `CancelledError extends MotosanError` and is **never retried**; fetch-internal `AbortError` with no caller signal aborted (e.g. `AbortSignal.timeout`) stays retryable. (`specs/retry.md` transport table amended.)

### Added
- Per-request cancellation: caller `AbortSignal` threads Client → provider `chat`/`stream` → `postJson`/`postStream` `options.signal` (copy the exact request/options seam name from the merged E6 code).
- `ClientBuilder.timeouts({connectMs?, readIdleMs?, totalMs?})` — connect 10 s and total (opt-in, chat only) via `AbortSignal.timeout` composition on fetch; read-idle 120 s via `readTimeoutStream`.

### Fixed
- `readTimeoutStream` actually throws `StreamReadTimeoutError` on idle expiry (previously it silently ended the stream).
````

  **3f. Version lines** (edit in place; do NOT touch historical mentions — CHANGELOG history, `docs/superpowers/`, AGENTS.md/llms.txt/SKILL.md paragraphs describing PAST releases like "Python 0.13.0 adds CLI-runtime setters" or "Gemini (v0.13.0, feature `gemini`)" stay as-is):

  | File (approx line) | Old → New |
  |---|---|
  | `AGENTS.md` ~5 | `Rust v0.23.0 · Python v0.16.0 (PyPI) · TypeScript v0.13.0 (npm)` → `Rust v0.24.0 · Python v0.17.0 (PyPI) · TypeScript v0.14.0 (npm)` |
  | `llms.txt` ~5 | `- Python 0.16.0 · TypeScript 0.13.0 · Rust 0.23.0` → `- Python 0.17.0 · TypeScript 0.14.0 · Rust 0.24.0` |
  | `llms.txt` ~24, `README.md` ~38, `skills/motosan-ai/SKILL.md` ~25, `skills/motosan-ai/references/rust-api.md` ~7, `sdks/rust/README.md` ~323/~433/~497 | every `motosan-ai = { version = "0.23.0", ...` → `version = "0.24.0"` (grep `motosan-ai = { version = "0.23.0"` to find all) |
  | `llms.txt` ~923 tag-table example | `ts-v0.13.0` → `ts-v0.14.0` |
  | `README.md` ~29-31 Languages table | `v0.23.0` → `v0.24.0`, `v0.16.0` → `v0.17.0`, `v0.13.0` → `v0.14.0` |
  | `skills/motosan-ai/SKILL.md` ~8 | `Python 0.16.0 / Rust 0.23.0 / TypeScript 0.13.0` → `Python 0.17.0 / Rust 0.24.0 / TypeScript 0.14.0` |
  | `sdks/typescript/README.md` ~485-486 | `git tag ts-v0.13.0` / `git push origin ts-v0.13.0` → `ts-v0.14.0` both lines |

  Then insert a new paragraph in `AGENTS.md` directly after the M2 paragraph ("Rust 0.23.0 / Python 0.16.0 / TypeScript 0.13.0 are the M2 retry releases...", ~13, which stays). AGENTS.md is deliberately excluded from the step-1 "exactly one" grep, so this paragraph MAY name the retired invariant:

  > Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0 are the M3 stream-contract + timeout releases: a stream that ends without the provider's terminal event raises a typed error (Rust `MotosanError::IncompleteStream` — **breaking** enum addition; Python `IncompleteStreamError(StreamError)`; TS `IncompleteStreamError extends StreamError`), retiring the v0.10.1 "exactly one `done`" invariant; one timeout model (connect 10 s / read-idle 120 s / total opt-in) lands on all three builders; Rust builds the provider once with a shared `reqwest::Client`; TypeScript gains per-request `AbortSignal` + `CancelledError` (never retried) and a `readTimeoutStream` that actually throws; Python gains `Client.aclose()` / async context manager.

  And insert a matching bullet in `llms.txt` after the chatgpt-codex bullet (~7) — llms.txt IS in the step-1 grep list, so this bullet must NOT contain the literal phrase "exactly one" (the wording below is safe):

  > - Rust 0.24.0 / Python 0.17.0 / TS 0.14.0 (M3): **breaking** stream termination — EOF without the provider terminal event raises Rust `MotosanError::IncompleteStream` / Python `IncompleteStreamError(StreamError)` / TS `IncompleteStreamError extends StreamError`; one timeout model (connect 10 s / read-idle 120 s / total opt-in) on all builders; Rust build-once shared `reqwest::Client`; TS per-request `AbortSignal` + `CancelledError` (never retried); Python `Client.aclose()` / `async with`.

  **3g. Retired-invariant prose rewrites (E3)** — run `grep -rn 'exactly one' README.md llms.txt sdks/rust/README.md sdks/typescript/README.md skills/motosan-ai/SKILL.md` and `grep -rn 'terminal \`done\`' README.md llms.txt sdks/rust/README.md sdks/typescript/README.md skills/` and rewrite every hit that states the old guarantee. CONSISTENCY RULE: the step-1 script asserts zero `exactly one` hits remain in these five files, so none of the replacement texts below contains that literal phrase — when describing what was retired, say "the v0.10.1 fabricated-`done` invariant" instead; do not reintroduce the phrase while editing. Five known sites:
  - `sdks/rust/README.md` ~50 — Current (approximate): the paragraph beginning `Each provider stream emits **exactly one** terminal \`done\` event — guaranteed since v0.10.1...`. Replace with: `A stream is complete only when the provider sends its terminal event (OpenAI \`[DONE]\`, Anthropic \`message_stop\`, Gemini / chatgpt-codex terminal frames). Since v0.24.0, EOF without that event yields \`Err(MotosanError::IncompleteStream(_))\` (\`"incomplete stream: <provider> ended without a terminal event"\`) instead of a fabricated clean \`done\` — truncation is distinguishable from completion. \`event.stop_reason\` carries the provider's reported reason when present (\`Anthropic\` \`message_delta.stop_reason\`, \`OpenAI\` / \`MiniMax\` \`finish_reason\`).`
  - `llms.txt` ~454 — Current: the paragraph beginning `Each provider stream emits **exactly one** \`done\` event — guaranteed since v0.10.1...` (through `...heuristic when none was reported.`). Replace with: `A stream is complete only when the provider sends its terminal event (OpenAI \`[DONE]\`, Anthropic \`message_stop\`, Gemini / chatgpt-codex terminal frames). Since rust-0.24.0 / python-0.17.0 / ts-0.14.0, EOF without that event is a typed error — Rust \`MotosanError::IncompleteStream\`, Python \`IncompleteStreamError\` (subclass of \`StreamError\`), TS \`IncompleteStreamError extends StreamError\` — message \`"incomplete stream: <provider> ended without a terminal event"\`. If the provider reports a stop reason it lands on the terminal event; \`collect_stream\` honors it and falls back to the tool-calls heuristic only on a real terminal event that lacks a reason.`
  - `README.md` ~239 — Current: `// Streams emit exactly one \`done\` event, even on non-conformant proxies.` Replace with: `// EOF without a terminal event is Err(MotosanError::IncompleteStream), not a done event.`
  - `sdks/typescript/README.md` ~207 — Current: paragraph beginning `Each provider stream emits **exactly one** terminal \`done\` event, even when the upstream provider closes...`. Replace with: `A stream is complete only when the provider sends its terminal event (\`[DONE]\`, \`message_stop\`, ...). Since v0.14.0, if the upstream closes without it the stream throws \`IncompleteStreamError\` (subclass of \`StreamError\`) — \`"incomplete stream: <provider> ended without a terminal event"\`. \`event.stopReason\` carries the provider's reported reason when present.` Also replace the `> **Mid-stream partial success (important):** ... does NOT throw mid-stream ...` blockquote (~211-216) with: `> **Mid-stream failures:** provider \`error\` frames and transport faults reject the stream (since 0.12.0), and truncation (EOF without a terminal event) rejects with \`IncompleteStreamError\` (since 0.14.0). Retries apply only to the *initial* fetch, never mid-stream (see Retry). Aborting via your own \`AbortSignal\` throws \`CancelledError\` and is never retried.`
  - `skills/motosan-ai/SKILL.md` ~131 — Current: the bullet beginning `- **Stream \`done\` invariant** (Rust, since v0.10.1): every provider stream emits **exactly one** terminal event with \`done == true\`...`. Replace with: `- **Stream termination contract** (Rust 0.24.0 / Python 0.17.0 / TS 0.14.0; replaces the v0.10.1 fabricated-\`done\` invariant): a stream is complete only when the provider sends its terminal event (OpenAI \`[DONE]\`, Anthropic \`message_stop\`, Gemini/codex terminal frames). EOF without it errors with Rust \`MotosanError::IncompleteStream\` / Python \`IncompleteStreamError\` (subclass of \`StreamError\`) / TS \`IncompleteStreamError extends StreamError\` — message \`"incomplete stream: <provider> ended without a terminal event"\`. Successful streams still end with one terminal \`done == true\` event carrying \`stop_reason\` when reported; \`collect_stream\` keeps the tool-calls heuristic only for a real terminal event that lacks a reason.`
  - `skills/motosan-ai/SKILL.md` ~131 replacement note: the parenthetical deliberately says "fabricated-\`done\` invariant" — NOT the old invariant's name verbatim — because SKILL.md is in the step-1 grep list (see CONSISTENCY RULE above).

  **3h. Cross-check against reality.** Run `git log --oneline acf5d7f..HEAD`. Every Breaking/Added/Changed/Fixed bullet written above must map to at least one merged M3 commit: delete bullets whose feature did not merge; add bullets for merged M3 changes not yet covered (tag each with only the SDKs it truly touches); replace the two copy-from-code placeholders (Python E8 facade kwarg name — `grep -n 'timeout' sdks/python/motosan_ai/client.py`; TS E6 signal seam — `grep -n 'signal' sdks/typescript/src/client.ts`) with the exact merged spellings; confirm the Rust `read_idle_timeout` vs `stream_read_timeout_secs` wording matches the E4 task's actual resolution (`grep -n 'read_idle_timeout\|stream_read_timeout' sdks/rust/src/client.rs`) and adjust the "supersedes" bullet if the E4 PR kept the old name as an alias. Finally run the full stale scan `grep -rn '0\.23\.0\|0\.16\.0\|0\.13\.0' --include='*.md' --include='*.txt' --include='*.toml' --include='*.json' . | grep -v node_modules | grep -v CHANGELOG | grep -v docs/superpowers | grep -v package-lock.json` — every remaining hit must be a historical mention (past-release paragraphs in AGENTS.md/llms.txt/SKILL.md, Gemini `v0.13.0` feature notes); anything else gets bumped.

- [ ] 4. **Run-pass + package suites.** `bash /tmp/m3-verify-release.sh; echo "exit=$?"` from the repo root — expected: every line `ok`, `exit=0`. Then the release gate: from the repo root (nix develop shell) `check-all` (Rust: fmt + clippy + test; Python: ruff + pytest — expected `=== All checks passed ===`); if not in the nix shell, run from `sdks/rust`: `cargo fmt --check && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features`, and from `sdks/python`: `uv run ruff check motosan_ai/ && uv run pytest tests/ -q --ignore=tests/integration`. Then TypeScript from `sdks/typescript`: `npm run typecheck && npm run build && npm test` — all green (check-all does NOT cover TS; this is mandatory).

- [ ] 5. **Format/lint.** This task touches no `.rs`/`.py`/`.ts` source, so formatters should be no-ops: from `sdks/rust` run `cargo fmt --check` (expect silence); if the nix shell is active run `fmt` and confirm `git status` shows no unexpected reformatting. Confirm `git status` does NOT list `Cargo.lock` (gitignored — if it appears, stop and check `.gitignore` before proceeding; never `git add` it).

- [ ] 6. **Commit + PR** (house rule: any `Cargo.toml` change goes through PR + CI — never direct to main). On a branch `release/m3-rust-0.24.0-python-0.17.0-ts-0.14.0`, stage EXACTLY (no `git add -A`):

```bash
git add sdks/rust/Cargo.toml sdks/python/pyproject.toml sdks/typescript/package.json \
  uv.lock sdks/typescript/package-lock.json \
  CHANGELOG.md sdks/rust/CHANGELOG.md sdks/python/CHANGELOG.md sdks/typescript/CHANGELOG.md \
  AGENTS.md llms.txt README.md skills/motosan-ai/SKILL.md skills/motosan-ai/references/rust-api.md \
  sdks/rust/README.md sdks/typescript/README.md
git commit -m "chore(release): M3 stream contract + timeouts — rust-v0.24.0 / python-v0.17.0 / ts-v0.14.0" \
  -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

  Push the branch and open a PR against `main` (`gh pr create`) titled `chore(release): M3 — rust-v0.24.0 / python-v0.17.0 / ts-v0.14.0`; the body lists the two Rust breaking changes and links the three per-SDK changelogs, ending with the standard generated-with footer. Do NOT run `git tag`, do NOT push any tag, do NOT trigger `publish-*.yml` — tagging per `llms.txt` § Release happens only after this PR merges, outside this task.

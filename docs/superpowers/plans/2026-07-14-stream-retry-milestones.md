# Stream & Retry Hardening — Milestone Plan

**Source:** Deep multi-agent audit run 2026-07-14 against `origin/main` @ `3e3f413` (74 agents; 32 raw
findings → 29 survived adversarial verification; the 3 P0 evidence chains were additionally re-verified
by hand). Verdict: the client → provider → adapter → collector layering is sound — **no rewrite**. The
damage splits into (a) local, patchable defects that silently corrupt results today, and (b) three real
architectural gaps (error metadata, stream-termination contract, timeout/lifecycle model).

**Root cause to keep in mind everywhere:** every stream/retry fixture in all three SDKs is synthetic and
happy-path (one codex fixture even uses `item_id == call_id`, exactly masking a P0 bug). Every milestone
below therefore ships **real-wire-shaped fixtures** alongside the fix — that is the regression barrier,
not the individual patches.

**Versions at baseline:** Rust 0.21.1 · Python 0.14.0 · TypeScript 0.11.0.

---

## The 8 verified workstreams (from the audit)

| Rank | Sev | Effort | Workstream | Milestone |
|---|---|---|---|---|
| 1 | P0 | M | Silent success-on-failure: in-band SSE error frames + CLI terminal errors swallowed | **M1** |
| 2 | P0 | M | Streamed tool-call corruption: OpenAI `index` ignored, codex `item_id`/`call_id` mismatch, Python `stop_reason` | **M1** |
| 3 | P0 | S | Retry dead on real-world 5xx: Rust parses body before status; Python errors carry no status | **M1** |
| 4 | P1 | L | Structured error metadata + status-based retry classification + spec'd Retry-After rule | **M2** |
| 5 | P1 | M | Terminal-event / incomplete-stream contract; unified read-timeout behavior | **M3** |
| 6 | P1 | L | One timeout model + client lifecycle (build-once providers, cancellation, `close()`) | **M3** (S-sized `reader.cancel()` pulled into M1) |
| 7 | P2 | L | Consolidate ~20-site copy-pasted retry transport; Python `RetryPolicy`; real jitter; `on_retry` | **M2** (rides rank 4 — same files) |
| 8 | P2 | M | Spec & parity cleanup (event vocabulary, usage double-count, TS SSE CRLF, CLI `stop_reason`, OAuth seam) | **M4** (S-sized usage + CRLF fixes pulled into M1) |

---

## M1 — Correctness patch wave (P0) — "stop lying to callers"

**Theme:** every fix converts a silent wrong answer into either a correct answer or a typed error.
**Breaking:** none by design — no public API changes; behavior changes only where current behavior is
returning fabricated/corrupted data. Ships as Rust **0.22.0**, Python **0.15.0**, TS **0.12.0**
(minor bumps: error-surfacing is observable behavior change).

Workstreams:

- **W1 — Un-break retry on real 5xx** (rank 3). Rust `anthropic`/`openai`/`ollama` `chat()`: decide
  retryability from status + `Retry-After` alone; parse the body only on the terminal attempt (copy the
  proven `gemini.rs` shape). Python `openai`/`minimax`/`chatgpt_codex`: embed `HTTP {status}` +
  `Retry-After` into the raised message the way `anthropic.py` already does, so `retry.py`'s existing
  classifier can see them (full structured-metadata fix is M2's job — this is the minimal unblocking).
- **W2 — Surface in-band errors** (rank 1). Anthropic mid-stream `error` frames (e.g.
  `overloaded_error` on HTTP 200) → typed stream error in all three SDKs; TS `chatgpt_codex`
  `error`/`response.failed` frames stop being swallowed (copy Rust's arm; reuse the dead
  `chatGptCodexErrorMessage` helper); Rust `claude_code` terminal events with error subtypes stop
  vanishing on serde (`result` → `#[serde(default)]`, branch on `is_error`/`subtype`); Python
  `claude_code` errors on `is_error: true`; Rust `drive_lines` + Python CLI streams surface read
  errors / child death (reap child, read stderr, yield typed error) instead of clean EOF.
- **W3 — Tool-call integrity** (rank 2). Port TS's `toolBuffer` (the one correct implementation,
  `openai.ts:353-447`) to Rust + Python OpenAI adapters (keyed by `tool_calls[].index`, close-on-index-
  switch); codex adapters in all three SDKs get an `item.id → call_id` map so arg deltas join their
  call; Python OpenAI terminal event carries `finish_reason`-derived `stop_reason` + `_stream_collect`
  gets the tool-use fallback (parity with Rust/TS).
- **W4 — S-sized stream hygiene pulled forward** (from ranks 6/8). TS: `reader.cancel()` in
  `sse.ts`/`ndjson.ts` finally-blocks (stops pinning a socket per abandoned stream); TS SSE parser:
  WHATWG-correct terminators (CRLF/CR/LF) + single-leading-space rule; Rust/TS collectors: Anthropic
  usage merge switches from summing (double-counts cumulative `message_delta` usage) to Python's
  replace-with-fallback semantics.

**Exit criteria (Done = all of):**
1. `check-all` green on every PR — note it covers **Rust + Python only** (`devshell/scripts.nix`);
   TS PRs additionally need the TS gate: `npm run typecheck && npm run build && npm test` in
   `sdks/typescript` (the full suite's `pack-smoke.test.ts` requires `dist/`). Every
   `.rs`/`Cargo.toml` change lands via PR + CI (house rule).
2. New real-wire fixtures exist and fail on pre-M1 code: non-JSON 5xx body; Anthropic mid-stream
   `error` frame; `claude_code` `error_max_turns` terminal; parallel tool calls keyed by `index`
   (the Rust fixture interleaves arg deltas across indexes; Python/TS follow the TS reference
   semantics — sequential fragments per call, as OpenAI actually emits); codex stream with
   **distinct** `fc_…` `item_id` / `call_…` `call_id`; CRLF SSE stream; cumulative-usage stream.
3. `#[ignore]`/env-gated live smokes: Anthropic streamed tool-call live smokes already exist at
   baseline in Rust (`anthropic_live.rs`) and Python (`test_anthropic_live.py`) and must stay
   green; M1 adds a chatgpt-codex streamed tool-call live smoke where a live pattern exists
   (Rust, Task 14). TS live coverage is text-only at baseline — the TS streamed-tool-call live
   smoke is deferred to M3's TS stream-contract work.
4. Release checklist executed per `llms.txt` § Release (Cargo.toml/pyproject.toml, CHANGELOG.md,
   AGENTS.md, llms.txt, skills/motosan-ai/SKILL.md).

**Detailed task breakdown:** `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`.

---

## M2 — Errors carry structure; one retry engine (ranks 4 + 7)

**Theme:** kill message-string coupling. One spec'd retry semantics, one transport helper per SDK.
**Breaking:** Rust `MotosanError` variants gain fields (batch into ONE minor release, e.g. 0.23.0);
Python exceptions gain attributes (additive); TS is already the reference design.

- Add `status_code` / `retry_after` / `request_id` to Rust error variants and Python exceptions;
  populate in every provider's error mapping. Rewrite Python `_is_retryable` to
  `status == 429 or status >= 500`; delete the `\b5\d{2}\b` regex and message scraping.
- Write the **normative retry section in `specs/`** (today specs contain zero retry semantics):
  classification on status (incl. 408/409 — official SDKs retry both), `Retry-After` honored
  (integer AND HTTP-date) capped at ~60s independent of `max_delay_ms`, exponential backoff + **real**
  jitter otherwise (current jitter is a deterministic LCG of the attempt number — zero herd
  protection; replace with full jitter from a real RNG, injectable for tests).
- Extract one `send_with_retry(policy, build_request)` helper per SDK; collapse the ~20 hand-copied
  Rust loops and 5 TS stream-loop reimplementations onto it. Port `RetryPolicy` to Python (today only
  `max_retries: int`). Add `on_retry(attempt, delay, cause)` observer at the single choke point.
  Decide + spec CLI-backend retry semantics (currently undefined).
- Update the pinning tests that currently freeze deterministic jitter as contract.

**Done:** spec section merged; all HTTP providers routed through the shared helper; Python has
`RetryPolicy`; cross-provider retry conformance tests driven from the spec table; a non-JSON-5xx and a
`Retry-After: 86400` (capped) test per SDK.

---

## M3 — Stream termination contract + timeout/lifecycle (ranks 5 + 6)

**Theme:** truncation must be distinguishable from completion; nothing hangs forever.
**Breaking:** yes — stream semantics change (EOF-without-terminal-event becomes a typed error).
Coordinate across all three SDKs in one release. **Depends on M1** (error frames must surface first,
else this converts them into `IncompleteStream` noise).

- Amend `specs/types.md`: EOF without the provider's terminal event ⇒ typed `IncompleteStream` error
  (one rule, all SDKs). Remove the fabricated clean-`done` paths (TS anthropic, Rust openai) and the
  collector fallbacks that mask truncation. TS `readTimeoutStream` actually throws its (currently
  never-thrown) `StreamReadTimeoutError`; Python gets a configurable read-timeout with a distinct
  error type.
- One timeout model (connect / per-read idle / total) exposed on all three builders + the Python
  `Client` facade (today: Rust/TS have NO request timeout — bare `reqwest::Client::new()`, unused
  `FetchOptions.signal`; Python hardcodes 120s/30s and the CLI `.timeout()` setters are unreachable
  through the facade).
- Rust: construct providers **once** in `build()` with a shared, timeout-configured `reqwest::Client`
  (also fixes: provider + connection pool rebuilt per request; pre-built `gemini_code_assist` silently
  discarding `ClientBuilder::retry_policy`). TS: thread `AbortSignal` Client→`postStream`; stop
  classifying `AbortError` as retryable. Python: `Client.aclose()` / async context manager; fix the
  30s MiniMax outlier. Document Rust drop-cancellation in specs.

**Done:** spec rule merged; kill-the-connection-mid-stream test per SDK yields the typed error, not a
clean response; hung-stream test hits the read timeout everywhere; no per-request client construction
in Rust; TS abandoned-stream test shows the connection actually cancelled.

---

## M4 — Spec & parity cleanup (rank 8 remainder)

- Rewrite `specs/types.md` stream-event vocabulary to match reality (`thinking_delta`/`thinking_done`,
  terminal-done semantics per M3) and treat it as the conformance source; add missing members to
  Python's `StreamEventType` (thinking events are currently ad-hoc untyped strings — exhaustive
  dispatch drops thinking content).
- Decide the CLI chat-vs-stream `stop_reason`/`tool_calls` contract once (today: identical call
  reports `tool_use` + triplets via `stream()` but `end_turn` + `tool_calls=[]` via `chat()` in both
  Rust and Python) and apply to both SDKs.
- Design the per-attempt OAuth token-source seam for `chatgpt_codex` (tokens are frozen constructor
  strings with ~1h lifetime; refresh machinery shipped but unwired) — slots into M3's build-once
  provider work. Add `claude_code` to Python's `Provider` enum or document it out of scope.

**Done:** spec == implementation (conformance tests reference spec sections); CLI chat/stream parity
test; codex live smoke survives >1h via token source (or documented out of scope).

---

## Sequencing & dependencies

```
M1 (P0 patches, non-breaking)          ← ship first, immediately
 ├─→ M2 (error metadata → retry engine)   [rank 7 rides rank 4: same files/loops]
 └─→ M3 (stream contract + timeout/lifecycle)   [needs M1's error surfacing]
          └─→ M4 (spec cleanup; OAuth seam slots into M3 builder work)
```

Do **not** start M2/M3 refactors before M1 lands: M1's fixture suite is what makes the later
refactors safe to review, and three providers were added in the last cycle each re-inheriting every
gap untested — the fixture barrier stops that class of regression first.

## Explicitly healthy — do not touch

Rust gemini/GCA/codex `chat()` retry loops (template for W1); TS OpenAI `toolBuffer` (port source for
W3); TS error taxonomy (reference for M2); Python usage merge + `stream_with` first-event retry
discipline; all three Anthropic `current_tool_id` implementations; Rust drop-cancellation + CLI
`kill_on_drop`; eventsource-stream / httpx SSE transports (only TS's hand-rolled parser needs work).

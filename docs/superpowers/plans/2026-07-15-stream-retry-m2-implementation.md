# M2 — Structured Errors & One Retry Engine: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give all three SDKs structured error metadata (status/retry-after/request-id), one spec'd retry semantics (`specs/retry.md`), and one shared retry transport per SDK — killing the message-string coupling and the ~20-site copy-pasted loops that the 2026-07 audit identified as the root drift mechanism (ranks 4+7).

**Architecture:** Spec-first: `specs/retry.md` becomes the normative contract, mirrored by table-driven conformance tests in each SDK. Rust converts the four HTTP-mapped `MotosanError` variants to struct variants behind the existing `map_http_error` choke point and collapses every provider request loop onto one `send_with_retry` helper (success/terminal responses returned for caller-side body handling — preserving M1's status-first behavior exactly). Python gains attributes on exceptions + a real `RetryPolicy`; classification flips from regex-on-message to attributes. TS (the reference design) adds `requestId`, real jitter, and routes its five hand-rolled provider loops through the existing `withRetry`.

**Tech Stack:** Rust (thiserror, chrono for HTTP-date, fastrand or in-tree RNG for jitter, mockito tests) · Python 3.11+ (dataclasses, email.utils, respx) · TypeScript (vitest, injectable RNG).

## Global Constraints

- **Baseline:** authored 2026-07-15 against `origin/main` @ `d7c06ff` (post-M1: Rust 0.22.0 / Python 0.15.0 / TS 0.12.0). ALL line refs approximate. **Execute each task in a worktree off the CURRENT `origin/main`** and ground every edit in the real files.
- **Locked design (D1–D9):** every signature/constant in the tasks below is normative — `RETRY_AFTER` cap **60s**; full jitter = uniform in `[0, min(base·2^(attempt−1), max_delay)]`; retry-after used **verbatim, no jitter**; retryable statuses **408/409/429/≥500**; `on_retry` lives ON `RetryPolicy`; CLI backends get **no transport retry** (spec'd); per-request overrides/deadlines are explicitly out of scope. A task deviating from these is wrong even if it compiles.
- **Breaking-change budget:** Rust `MotosanError` enum shape only (four HTTP variants → struct variants; Display strings byte-identical) — ships as **Rust 0.23.0** with a changelog migration note. Python is additive only (`LlmClient` Protocol untouched — motosan-chat depends on it). TS additive (**0.13.0**); Python **0.16.0**.
- **M1 behavioral contract:** the M1 retry tests (non-JSON 5xx → exactly 2 requests → success; stream retry only before first event) MUST pass unchanged through every task — they are the regression barrier for this refactor.
- **House workflow:** every `.rs`/`Cargo.toml` change lands via PR + CI; suggested PR grouping below; conformance PR after all SDK PRs; release LAST.
- **Commands** — Rust (from `sdks/rust`): `cargo test --all-features …`, `cargo fmt`, `cargo clippy --all-features -- -D warnings`. Python (from `sdks/python`): `uv run pytest tests/… -v`, `uv run ruff check motosan_ai/` (tests/ NOT linted), `uv run ruff format`. TypeScript (from `sdks/typescript`): `npx vitest run tests/…`, `npm run build && npm test` (pack-smoke needs `dist/`), `npm run typecheck`. Fresh Python worktrees: `uv sync --all-extras` first (pre-push hook runs the full suite).
- **Commits:** conventional style, ending `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`.
- **Ordering:** Rust tasks strictly 2→3→4→5→6 (each consumes the previous task's interfaces); Python 7→8→9; TS 10→11→12; Task 1 (spec) first overall; Task 13 (conformance) after all SDK tasks; Task 14 (release) last.
- **Cross-SDK consistency (authoritative):**
  - The normative constant is **`RETRY_AFTER_CAP_SECS = 60`** (spec + Python + `grep` gate). Each SDK spells the same value idiomatically: Rust `pub(crate) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60)`, Python `RETRY_AFTER_CAP_SECS: float = 60.0`, TS `RETRY_AFTER_CAP_MS = 60_000`. Same value — not a mismatch.
  - **Rust retry internals stay `pub(crate)` — NOT public API.** `is_retryable_status` / `parse_retry_after` live in `providers/mod.rs`; only `RetryPolicy`/`RetryEvent`/`RetryCause` are public. The Rust conformance assertions (Task 13) are therefore an **in-crate `#[cfg(test)] mod`** in `providers/mod.rs`, not an integration test — do not widen visibility.
  - **Retry-After parse semantics (all three SDKs):** integer OR decimal seconds parse as float (`"1.5"` → 1.5); a **negative** numeric value is invalid → `None`; only a **past HTTP-date** clamps to 0; valid values clamp to `[0, 60]`. Rust transport-retry predicate is the real 4-clause `is_timeout() || is_connect() || is_request() || is_body()` (wider than D5's abbreviation) — blessed as-is; `specs/retry.md` documents the real predicate.

## Suggested PR grouping

| PR | Tasks | Scope |
|---|---|---|
| PR-S | 1 | `specs/retry.md` (docs-only) |
| PR-R1 | 2, 3, 4 | Rust error metadata + Retry-After/status hardening + jitter/on_retry |
| PR-R2 | 5, 6 | Rust `send_with_retry` + all six provider migrations (after PR-R1) |
| PR-P | 7, 8, 9 | Python attrs → RetryPolicy → client threading (sequential) |
| PR-T | 10, 11, 12 | TS requestId/Retry-After → jitter/onRetry → withRetry routing (sequential) |
| PR-C | 13 | Conformance test suites ×3 (after PR-R2/P/T merge) |
| PR-REL | 14 | Release 0.23.0 / 0.16.0 / 0.13.0 (last) |

---


## S — Normative spec first

### Task 1: Write specs/retry.md — the normative cross-SDK retry contract

**Files:**
- Create: `specs/retry.md` (new — full content below, copy verbatim)
- Modify: `specs/types.md` (~lines 127–129, `## MotosanError (Rust)` section — add one pointer line; types.md has no related-specs section, so this is the anchor)
- Test: none — **docs-only task, deliberately no test cycle** (the deliverable is prose; conformance tests in other M2 tasks cite this file's section anchors)

**Interfaces:**
- Consumes (verified at baseline): Rust `is_retryable_status` / `is_retryable_network_error` / `parse_retry_after` (`sdks/rust/src/providers/mod.rs` ~292/~305/~318), `map_http_error` (~237); Python `_is_retryable` (`sdks/python/motosan_ai/retry.py` ~40); TS `isRetryableStatus` / `isRetryableNetworkError` (`sdks/typescript/src/error.ts` ~65/~80); guard tests `sdks/python/tests/test_client_stream_with.py` (~88, ~111).
- Produces: section anchors cited by every M2 conformance-test task: `#classification`, `#retry-after`, `#backoff-and-jitter`, `#streaming`, `#cli-backends`, `#observability`, `#one-retry-engine-per-sdk`, `#error-metadata`, `#out-of-scope-future-work`; constant name `RETRY_AFTER_CAP_SECS = 60`.

- [ ] **Step 1 — Create `specs/retry.md`** with exactly this content (no test cycle for this task; write the file):

````markdown
# Retry Semantics

Normative retry contract shared across all language SDKs (Rust, Python,
TypeScript). Conformance tests cite the sections below. Applies to the
`chat()` and `stream()` HTTP transports of every HTTP provider; CLI
backends are excluded (see [CLI backends](#cli-backends)).

| Constant | Value |
|----------|-------|
| `RETRY_AFTER_CAP_SECS` | `60` |

## Classification

Canonical status predicate — identical in all SDKs:

```
retryable_status(s) = s == 408 || s == 409 || s == 429 || s >= 500
```

| Condition | Retry |
|-----------|-------|
| HTTP 408 (request timeout) | ✅ |
| HTTP 409 (conflict) | ✅ |
| HTTP 429 (rate limit) | ✅ |
| HTTP ≥ 500 (server error) | ✅ |
| Transport / connection error (table below) | ✅ |
| Any other 4xx (400, 401, 403, 404, 422, …) | ❌ never |
| Success-body parse error; any error after the first stream event | ❌ never |

Classification reads **structured error metadata only** (`status_code`
on the typed error, or the transport-error class) — never message
strings.

| SDK | Status predicate | Home |
|-----|------------------|------|
| Rust | `is_retryable_status` | `sdks/rust/src/providers/mod.rs` |
| Python | `_is_retryable` | `sdks/python/motosan_ai/retry.py` |
| TypeScript | `isRetryableStatus` | `sdks/typescript/src/error.ts` |

Python's `_is_retryable` is attribute-based: `RateLimitError` → retry;
`NetworkError` → retry; `ProviderError` → retry iff
`error.status_code in {408, 409}` or `(error.status_code or 0) >= 500`.

Transport / connection errors (always retryable):

| SDK | Surfaced as | Predicate |
|-----|-------------|-----------|
| Rust | `MotosanError::Network` | `is_retryable_network_error`: `reqwest::Error::is_timeout() \|\| is_connect() \|\| is_request() \|\| is_body()` |
| Python | `NetworkError` | providers wrap `httpx.HTTPError` raised while sending (`httpx.TransportError`-derived in practice: `ConnectError`, `ConnectTimeout`, `ReadTimeout`, `ReadError`, …); every `NetworkError` is retryable |
| TypeScript | raw fetch/Node error, classified directly | `isRetryableNetworkError`: `error.name === 'AbortError'`, `error instanceof TypeError`, or Node `error.code` ∈ {`ECONNREFUSED`, `ENOTFOUND`, `ETIMEDOUT`} |

## Retry-After

When a retryable response carries a `Retry-After` header and
`respect_retry_after = true`:

- Both RFC 7231 forms are honored: integer seconds (`Retry-After: 30`)
  and HTTP-date (`Retry-After: Wed, 15 Jul 2026 08:00:00 GMT`).
- The parsed value is clamped to `[0, RETRY_AFTER_CAP_SECS]` seconds —
  **independent of `max_delay`**. Past HTTP-dates clamp to 0 (retry
  immediately). A `Retry-After: 86400` sleeps 60 s, not a day.
- The clamped value is used **verbatim — no jitter**.
- Unparseable values are ignored (fall through to backoff).
- With `respect_retry_after = false` the header is ignored entirely.

HTTP-date parsing: Rust `chrono::DateTime::parse_from_rfc2822`;
Python `email.utils.parsedate_to_datetime`; TypeScript `Date.parse`.

## Backoff and jitter

For the *n*-th retry (n = 1…`max_retries`, 1-based):

```
exp_delay(n) = min(base_delay * 2^(n-1), max_delay)
sleep(n)     = uniform_random(0, exp_delay(n))   # jitter = true (FULL jitter)
             = exp_delay(n)                      # jitter = false
```

`uniform_random(0, d) = rng() * d` with `rng()` uniform on `[0, 1)`.
Deterministic jitter (the pre-M2 LCG `attempt * 1103515245 + 12345`)
is non-conformant. The RNG is injectable for tests:

| SDK | Injection point | Default |
|-----|-----------------|---------|
| Rust | RNG hook on `RetryPolicy` (tiny RNG crate, e.g. `fastrand`) | thread-local RNG |
| Python | `rng: Callable[[], float]` parameter | `random.random` |
| TypeScript | `RetryPolicyOptions.random?: () => number` | `Math.random` |

Policy knobs — identical semantics, per-language spelling:

| Knob | Rust | Python | TypeScript | Default |
|------|------|--------|------------|---------|
| Max retries | `max_retries` | `max_retries` | `maxRetries` | `3` (4 attempts total) |
| Base delay | `base_delay_ms` | `base_delay` (seconds) | `baseDelayMs` | 100 ms |
| Max delay | `max_delay_ms` | `max_delay` (seconds) | `maxDelayMs` | 2000 ms |
| Jitter | `jitter` | `jitter` | `jitter` | `true` |
| Honor Retry-After | `respect_retry_after` | `respect_retry_after` | `respectRetryAfter` | `true` |

Delay selection for each scheduled retry:

```
if respect_retry_after and a Retry-After value is present:
    delay = min(retry_after, RETRY_AFTER_CAP_SECS)   # verbatim, no jitter
else:
    delay = sleep(n)                                 # full-jitter backoff
```

## Streaming

Retry **only before the first emitted event**. Once any `StreamEvent`
has been yielded to the caller, every error — even one retryable by
class — propagates verbatim: replaying a partially consumed stream
would double-emit content. Connection-phase failures (before the first
event) follow the normal classification table.

Reference conformance tests:
`sdks/python/tests/test_client_stream_with.py::test_stream_with_does_not_retry_provider_error_after_yield`
and `::test_stream_with_does_not_retry_stream_error`.

## CLI backends

`claude_code`, `codex_cli`, and `gemini_cli` perform **no
transport-level retry**. Spawning a child process is not cheaply
idempotent (session state, side effects, cost). Callers own retry for
CLI backends; `RetryPolicy` settings have no effect on them.

## Observability

`RetryPolicy` carries an optional `on_retry` observer. It fires **once
per scheduled retry, immediately before the sleep**, from inside the
shared retry engine — never on the first attempt, never after the
terminal failure.

| SDK | Shape |
|-----|-------|
| Rust | `on_retry: Option<Arc<dyn Fn(RetryEvent) + Send + Sync>>`; `RetryEvent { attempt: u32, delay: Duration, cause: RetryCause }`; `RetryCause::Status(u16) \| Network(String)` |
| Python | `on_retry: Callable[[RetryEvent], None] \| None`; `RetryEvent(attempt: int, delay: float, cause: str)` |
| TypeScript | `onRetry?: (evt: RetryEvent) => void`; `RetryEvent { attempt: number; delayMs: number; cause: string }` |

`attempt` is the 1-based retry number (the *n* of the backoff formula);
`delay` is the exact duration about to be slept. Python/TS `cause`
strings identify the trigger (HTTP status or transport error); their
exact format is SDK-local and non-normative.

## One retry engine per SDK

All HTTP providers route through a single engine per SDK; hand-rolled
per-provider loops are non-conformant.

| SDK | Engine | Contract |
|-----|--------|----------|
| Rust | `send_with_retry(policy, build)` in `sdks/rust/src/providers/mod.rs` | returns the terminal `reqwest::Response` with the body **untouched** (success, non-retryable status, or attempts exhausted); the caller does its own tolerant body parse + `map_http_error` + provider-specific hints |
| Python | `with_retry(fn, policy=…)` in `sdks/python/motosan_ai/retry.py` | `client.py` threads one `RetryPolicy` through both chat and stream paths; stream backoff uses the shared policy math |
| TypeScript | `withRetry(policy, op, classify)` in `sdks/typescript/src/retry.ts` | classification via `isRetryableStatus` / `isRetryableNetworkError` |

## Error metadata

HTTP-mapped errors carry `status_code`, `retry_after`, and `request_id`,
populated at each SDK's single `map_http_error` choke point.
`request_id` is read from response headers: `request-id` first, then
`x-request-id` (first match wins). Human-readable `HTTP {status}: …`
message prefixes remain for display, but classification MUST NOT parse
messages. Error taxonomy: see `types.md`.

## Out of scope (future work)

Per-request retry-policy overrides and total-deadline (wall-clock)
budgets are explicitly out of scope for this contract: the policy is
client-level and the only budget is attempt count.
````

- [ ] **Step 2 — Add pointer in `specs/types.md`.** Current code (approximate lines 127–129):

```markdown
## MotosanError (Rust)

`Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `UnsupportedFeature(String)`
```

Replace with:

```markdown
## MotosanError (Rust)

`Auth` | `RateLimit` | `InvalidRequest` | `Config` | `ProviderError` | `Network` | `Stream` | `StreamReadTimeout(u64)` | `UnsupportedFeature(String)`

Retry classification, backoff, and `Retry-After` handling are specified in [`retry.md`](./retry.md).
```

- [ ] **Step 3 — Proofread against the D5 checklist** (this replaces the test cycle — docs have no run-fail step). Verify each item is present in `specs/retry.md`:
  - (a) classification table retries 408/409/429/≥500 + transport errors; other 4xx never; all three SDK transport mappings listed with real predicate names (`is_retryable_network_error`, `NetworkError`/`httpx.HTTPError`, `isRetryableNetworkError`);
  - (b) Retry-After: integer seconds AND HTTP-date, `RETRY_AFTER_CAP_SECS = 60` cap independent of `max_delay`, verbatim/no-jitter, `86400 → 60 s` example;
  - (c) backoff: base 100 ms doubling, cap 2000 ms, full jitter `uniform_random(0, exp_delay)`, `max_retries=3` (4 attempts);
  - (d) streaming: first-emitted-event rule + the two cited Python guard tests;
  - (e) CLI backends: no transport-level retry, callers own retry;
  - (f) `on_retry(attempt, delay, cause)` fires before each sleep;
  - (g) out-of-scope note for per-request overrides + deadline budgets.

  Sanity checks (run from repo root):
  ```bash
  grep -c '^## ' specs/retry.md        # expect: 9
  grep -c 'RETRY_AFTER_CAP_SECS' specs/retry.md   # expect: 3
  grep -n 'retry.md' specs/types.md    # expect: 1 hit, in the MotosanError section
  ```

- [ ] **Step 4 — Commit** (docs-only change; no `.rs`/`Cargo.toml` touched, so the PR+CI house rule for Rust code does not apply — land it on the M2 working branch with the milestone's normal flow):
  ```bash
  git add specs/retry.md specs/types.md
  git commit -m "docs(specs): add normative cross-SDK retry contract (specs/retry.md)" -m "Classification (408/409/429/>=500 + transport errors), Retry-After (integer + HTTP-date, 60s cap, verbatim), full-jitter backoff, streaming first-event rule, CLI no-retry, on_retry, single engine per SDK." -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```


## R — Rust: error metadata, retry hardening, one transport

### Task 2: Rust structured error metadata (D1)

**Files:**
- Modify: `sdks/rust/src/error.rs` (entire file — currently 23 lines)
- Modify: `sdks/rust/src/providers/mod.rs` (`map_http_error` ~237-244; add `extract_request_id` after `parse_retry_after` ~318-322; new test module)
- Modify (request_id capture + new call args + imports + tuple→struct constructions): `sdks/rust/src/providers/anthropic.rs`, `openai.rs`, `ollama.rs`, `gemini.rs`, `gemini_code_assist.rs`, `chatgpt_codex.rs`
- Modify (tuple→struct sweep only): `sdks/rust/src/providers/claude_code/mod.rs`, `claude_code/spawn.rs`, `codex_cli/mod.rs`, `codex_cli/spawn.rs`, `gemini_cli/mod.rs`, `gemini_cli/spawn.rs`
- Test: `sdks/rust/tests/error_mapping.rs` (extend + pattern sweep); pattern sweep only in `tests/anthropic_chat.rs`, `tests/chatgpt_codex.rs`, `tests/gemini_code_assist.rs`, `tests/gemini_provider.rs`, `tests/openai_provider.rs`, `tests/openai_retry.rs`

**Interfaces:** Produces (later M2 tasks — D6 `send_with_retry`, D9 parity — depend on these exact signatures):
- `MotosanError::{Auth, RateLimit, InvalidRequest, ProviderError}` become struct variants `{ message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> }`; Display strings byte-identical; other variants keep tuple form.
- `impl MotosanError`: `pub fn status_code(&self) -> Option<u16>`, `pub fn retry_after(&self) -> Option<Duration>`, `pub fn request_id(&self) -> Option<&str>` (all `None` for non-HTTP variants).
- `pub(crate) fn map_http_error(status_code: u16, message: String, retry_after: Option<Duration>, request_id: Option<String>) -> MotosanError` — the single construction choke point for HTTP errors.
- `pub(crate) fn extract_request_id(headers: &HeaderMap) -> Option<String>` — reads `request-id` then `x-request-id`, first match.
Consumes: existing `pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration>` (`providers/mod.rs` ~318). Breaking change; ships in Rust 0.23.0 (version bump handled by the release task). All work from `sdks/rust/`; lands via PR + CI per repo rules.

- [ ] **Step 1 — write failing tests.** (a) Append to `sdks/rust/src/error.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn display_strings_unchanged() {
        let cases: Vec<(MotosanError, &str)> = vec![
            (MotosanError::Auth { message: "bad key".into(), status_code: None, retry_after: None, request_id: None }, "auth error: bad key"),
            (MotosanError::RateLimit { message: "too many".into(), status_code: None, retry_after: None, request_id: None }, "rate limit error: too many"),
            (MotosanError::InvalidRequest { message: "bad field".into(), status_code: None, retry_after: None, request_id: None }, "invalid request: bad field"),
            (MotosanError::ProviderError { message: "boom".into(), status_code: None, retry_after: None, request_id: None }, "provider error: boom"),
            (MotosanError::Config("c".into()), "config error: c"),
            (MotosanError::Network("n".into()), "network error: n"),
            (MotosanError::Stream("s".into()), "stream error: s"),
            (MotosanError::StreamReadTimeout(5), "stream read timeout: no data received within 5 seconds"),
            (MotosanError::UnsupportedFeature("u".into()), "unsupported feature: u"),
        ];
        for (err, expected) in cases {
            assert_eq!(err.to_string(), expected);
        }
    }

    #[test]
    fn accessors_expose_metadata_on_http_variants() {
        let err = MotosanError::RateLimit {
            message: "too many".into(),
            status_code: Some(429),
            retry_after: Some(Duration::from_secs(7)),
            request_id: Some("req_123".into()),
        };
        assert_eq!(err.status_code(), Some(429));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(err.request_id(), Some("req_123"));
    }

    #[test]
    fn accessors_return_none_on_non_http_variants() {
        let errors = [
            MotosanError::Config("c".into()),
            MotosanError::Network("n".into()),
            MotosanError::Stream("s".into()),
            MotosanError::StreamReadTimeout(5),
            MotosanError::UnsupportedFeature("u".into()),
        ];
        for err in errors {
            assert_eq!(err.status_code(), None);
            assert_eq!(err.retry_after(), None);
            assert_eq!(err.request_id(), None);
        }
    }
}
```
(b) Append to `sdks/rust/src/providers/mod.rs` (bottom of file, after `validate_tests`):
```rust
#[cfg(test)]
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
mod http_error_metadata_tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderName};

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).expect("header name"),
                value.parse().expect("header value"),
            );
        }
        map
    }

    #[test]
    fn extract_request_id_prefers_request_id_then_x_request_id() {
        let both = headers(&[("x-request-id", "xrid"), ("request-id", "rid")]);
        assert_eq!(extract_request_id(&both).as_deref(), Some("rid"));
        let fallback = headers(&[("x-request-id", "xrid")]);
        assert_eq!(extract_request_id(&fallback).as_deref(), Some("xrid"));
        assert_eq!(extract_request_id(&HeaderMap::new()), None);
    }

    #[test]
    fn map_http_error_populates_metadata_from_headers() {
        let map = headers(&[("retry-after", "7"), ("request-id", "req_123")]);
        let err = map_http_error(
            429,
            "too many".to_string(),
            parse_retry_after(&map),
            extract_request_id(&map),
        );
        assert!(matches!(err, MotosanError::RateLimit { .. }));
        assert_eq!(err.status_code(), Some(429));
        assert_eq!(err.retry_after(), Some(Duration::from_secs(7)));
        assert_eq!(err.request_id(), Some("req_123"));
        assert_eq!(err.to_string(), "rate limit error: too many");
    }

    #[test]
    fn map_http_error_maps_status_to_variant() {
        assert!(matches!(map_http_error(401, "m".into(), None, None), MotosanError::Auth { .. }));
        assert!(matches!(map_http_error(400, "m".into(), None, None), MotosanError::InvalidRequest { .. }));
        assert!(matches!(map_http_error(429, "m".into(), None, None), MotosanError::RateLimit { .. }));
        assert!(matches!(map_http_error(500, "m".into(), None, None), MotosanError::ProviderError { .. }));
        assert_eq!(map_http_error(500, "m".into(), None, None).status_code(), Some(500));
    }
}
```
(c) In `sdks/rust/tests/error_mapping.rs`, replace the 429 block of `anthropic_maps_401_429_500` (approximate lines 39-51): add `.with_header("retry-after", "7")` and `.with_header("request-id", "req_abc")` after `.with_status(429)`, change the assertion `assert!(matches!(err, MotosanError::RateLimit(_)));` to:
```rust
    assert!(matches!(err, MotosanError::RateLimit { .. }));
    assert_eq!(err.status_code(), Some(429));
    assert_eq!(err.retry_after(), Some(std::time::Duration::from_secs(7)));
    assert_eq!(err.request_id(), Some("req_abc"));
    assert_eq!(err.to_string(), "rate limit error: too many");
```
(Provider uses `max_retries(0)`, so the 429 is terminal and metadata must surface.)
- [ ] **Step 2 — run, expect compile failure.** From `sdks/rust/`: `cargo test --all-features error`. Expected failure signature: `error[E0574]: expected struct, variant or union type, found variant 'MotosanError::Auth'` (struct-literal syntax against current tuple variants) plus `error[E0425]: cannot find function 'extract_request_id'`. Tests must NOT pass. (The error.rs test module carries its own `use std::time::Duration;`, so the failure is limited to those two signatures — the baseline tuple `error.rs` imports only `thiserror::Error`, so without that line `use super::*;` would not bring `Duration` into scope and the red step would ALSO throw a spurious `error[E0433]: failed to resolve: use of undeclared type 'Duration'`, muddying the intended signal.)
- [ ] **Step 3a — rewrite `src/error.rs`.** Current code (approximate lines 1-23): tuple enum `Auth(String) … UnsupportedFeature(String)` with `#[error("auth error: {0}")]`-style attributes. Replace lines 1-23 (keep the Step 1 test module below) with:
```rust
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Clone, Error)]
pub enum MotosanError {
    #[error("auth error: {message}")]
    Auth { message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> },
    #[error("rate limit error: {message}")]
    RateLimit { message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> },
    #[error("invalid request: {message}")]
    InvalidRequest { message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> },
    #[error("config error: {0}")]
    Config(String),
    #[error("provider error: {message}")]
    ProviderError { message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> },
    #[error("network error: {0}")]
    Network(String),
    #[error("stream error: {0}")]
    Stream(String),
    #[error("stream read timeout: no data received within {0} seconds")]
    StreamReadTimeout(u64),
    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),
}

impl MotosanError {
    /// HTTP status that produced this error, when it came from an HTTP response.
    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::Auth { status_code, .. }
            | Self::RateLimit { status_code, .. }
            | Self::InvalidRequest { status_code, .. }
            | Self::ProviderError { status_code, .. } => *status_code,
            _ => None,
        }
    }

    /// Parsed `Retry-After` from the failing HTTP response, if present.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::Auth { retry_after, .. }
            | Self::RateLimit { retry_after, .. }
            | Self::InvalidRequest { retry_after, .. }
            | Self::ProviderError { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Provider request id (`request-id` / `x-request-id` header), if present.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Auth { request_id, .. }
            | Self::RateLimit { request_id, .. }
            | Self::InvalidRequest { request_id, .. }
            | Self::ProviderError { request_id, .. } => request_id.as_deref(),
            _ => None,
        }
    }
}
```
- [ ] **Step 3b — extend `src/providers/mod.rs`.** Current code (approximate lines 237-244):
```rust
pub(crate) fn map_http_error(status_code: u16, message: String) -> MotosanError {
    match status_code {
        401 => MotosanError::Auth(message),
        429 => MotosanError::RateLimit(message),
        400 => MotosanError::InvalidRequest(message),
        _ => MotosanError::ProviderError(message),
    }
}
```
Replace with (keep the existing `#[cfg(any(...))]` attribute above it unchanged):
```rust
pub(crate) fn map_http_error(
    status_code: u16,
    message: String,
    retry_after: Option<Duration>,
    request_id: Option<String>,
) -> MotosanError {
    match status_code {
        401 => MotosanError::Auth { message, status_code: Some(status_code), retry_after, request_id },
        429 => MotosanError::RateLimit { message, status_code: Some(status_code), retry_after, request_id },
        400 => MotosanError::InvalidRequest { message, status_code: Some(status_code), retry_after, request_id },
        _ => MotosanError::ProviderError { message, status_code: Some(status_code), retry_after, request_id },
    }
}
```
Then directly after `parse_retry_after` (approximate lines 318-322), add — copying the exact same 7-feature `#[cfg(any(...))]` attribute that sits on `parse_retry_after`:
```rust
pub(crate) fn extract_request_id(headers: &HeaderMap) -> Option<String> {
    ["request-id", "x-request-id"].iter().find_map(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    })
}
```
- [ ] **Step 3c — update the 12 `map_http_error` call sites (HTTP providers).** First add `extract_request_id` to each file's `use crate::providers::{...}` list (alphabetically after `extract_error_message`) in: `anthropic.rs` (~line 6), `openai.rs` (~4), `ollama.rs` (~4), `gemini.rs` (~6), `gemini_code_assist.rs` (~7), `chatgpt_codex.rs` (~3). Fully-worked example — `anthropic.rs` chat, current code (approximate lines 492-493 and 516):
```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            // …
            return Err(map_http_error(status.as_u16(), message));
```
Replace with:
```rust
            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            // …
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
```
Apply the same two-part edit at every site below (response variable is `resp` in gemini/gemini_code_assist/chatgpt_codex; there `status` is already `u16`, so the call is `map_http_error(status, msg, retry_after, request_id)`):

| File | Add `request_id` capture after (approx) | Update call (approx) |
|---|---|---|
| `anthropic.rs` stream | :829 | :845 |
| `openai.rs` `chat_via_responses` | :253 `let status = response.status();` — add BOTH `let retry_after = parse_retry_after(response.headers());` AND the `request_id` capture (this path had neither; captures MUST precede the body-consuming `response.json()` at ~:254) | :261 |
| `openai.rs` chat | :503 | :525 |
| `openai.rs` stream | :622 | :634 |
| `ollama.rs` chat | :267 | :277 |
| `ollama.rs` stream | :373 | :386 pass `request_id.clone()` (first of two sequential returns — plain `request_id` is a conditional move, E0382); :394 pass `request_id` |
| `gemini.rs` chat | :349 | :357 |
| `gemini.rs` stream | :397 | :405 |
| `gemini_code_assist.rs` stream | :150 | :158 |
| `chatgpt_codex.rs` stream | :284 | :292 |
- [ ] **Step 3d — mechanical sweep of all remaining tuple-form sites.** Three shapes; apply the matching rewrite at every listed line (all approximate). **Shape A — construction with only a message** (metadata all `None`; per D1 the choke point is `map_http_error`, so NO helper fn). Example, `codex_cli/mod.rs:494` — current: `.map_err(|e| MotosanError::ProviderError(format!("failed to spawn codex CLI: {e}")))?;` — replace with:
```rust
.map_err(|e| MotosanError::ProviderError {
    message: format!("failed to spawn codex CLI: {e}"),
    status_code: None,
    retry_after: None,
    request_id: None,
})?;
```
Sites (keep each site's message expression verbatim): `anthropic.rs:499`; `openai.rs:257,513`; `ollama.rs:283`; `gemini.rs:360`; `claude_code/mod.rs:510,515,518,524,602`; `claude_code/spawn.rs:397,401,410,415,419,424,452,468`; `codex_cli/mod.rs:494,498,501,506,583`; `codex_cli/spawn.rs:282,286,294,299,303,308,358`; `gemini_cli/mod.rs:290,294,300,305,381`; `gemini_cli/spawn.rs:222,226,234,239,243,248,295`. **Shape B — wildcard pattern**: `MotosanError::Auth(_)` → `MotosanError::Auth { .. }` (same for the other three, incl. inside `Some(Err(...))`/`matches!`). Sites: `claude_code/mod.rs:954,1019`; `codex_cli/mod.rs:883`; `gemini_cli/mod.rs:677`; `tests/anthropic_chat.rs:161`; `tests/chatgpt_codex.rs:178`; `tests/error_mapping.rs:36,64,86,100,114,140,154,168`; `tests/gemini_code_assist.rs:210,455`; `tests/gemini_provider.rs:256,282,308,403,630`; `tests/openai_provider.rs:302`; `tests/openai_retry.rs:130`. **Shape C — binding pattern**: `MotosanError::ProviderError(msg)` → `MotosanError::ProviderError { message: msg, .. }` (works in `match` arms and `let … else`). Sites: `claude_code/mod.rs:973`; `claude_code/spawn.rs:1030,1041`; `codex_cli/spawn.rs:805`; `gemini_cli/spawn.rs:647`. Leave the 5 doc-comment links (`codex_cli/stream_json.rs:141`, `codex_cli/mod.rs:424,467`, `codex_cli/spawn.rs:271`, `gemini_cli/spawn.rs:211`) unchanged — `[`MotosanError::ProviderError`]` resolves for struct variants too. Verify completeness: `grep -rn "MotosanError::\(Auth\|RateLimit\|InvalidRequest\|ProviderError\)(" src/ tests/` from `sdks/rust/` must print nothing.
- [ ] **Step 4 — run tests.** From `sdks/rust/`: `cargo test --all-features error` — expect the new `error::tests::*` (3 tests), `providers::http_error_metadata_tests::*` (3 tests), and `error_mapping` tests all green. Then full package suite: `cargo test --all-features` — expect 0 failures (live `#[ignore]` tests stay ignored).
- [ ] **Step 5 — format & lint.** From `sdks/rust/`: `cargo fmt` (reflows the multi-line struct constructions from Step 3d), then `cargo clippy --all-features -- -D warnings` — expect clean.
- [ ] **Step 6 — commit on a feature branch and open a PR** (every `.rs` change lands via PR + CI):
```bash
git checkout -b feat/rust-error-metadata
git add sdks/rust
git commit -m "feat(rust)!: add structured metadata to HTTP-mapped error variants

Auth/RateLimit/InvalidRequest/ProviderError become struct variants with
status_code/retry_after/request_id; Display strings unchanged. map_http_error
now the single choke point populating metadata; request-id/x-request-id read
via extract_request_id. (D1, M2)

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 3: Harden Rust Retry-After parsing and retryable status set (D8)

**Files:**
- Modify: `sdks/rust/Cargo.toml` (feature lists at approx. lines 17–61, `[dependencies]` at approx. lines 73–95)
- Modify: `sdks/rust/src/providers/mod.rs` (`is_retryable_status` approx. lines 292–294, `parse_retry_after` approx. lines 318–322, `sleep_before_retry` approx. lines 333–345)
- Test: `sdks/rust/src/providers/mod.rs` — NEW `#[cfg(test)] mod retry_after_tests` inserted immediately after `sleep_before_retry` (approx. line 346). Note: the file's existing test mods (`cli_terminal_tests` approx. 255–281, `validate_tests` approx. 377–456) cover other areas; do not touch them.

**Interfaces:**
- Produces: `pub(crate) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);` in `providers/mod.rs`; `pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration>` (signature UNCHANGED — now also accepts RFC-7231 HTTP-dates and always returns a value clamped to `[0, RETRY_AFTER_CAP]`); `pub(crate) fn is_retryable_status(status_code: u16) -> bool` now `status_code == 408 || status_code == 409 || status_code == 429 || status_code >= 500` (D8).
- Consumes: `crate::retry::RetryPolicy` (unchanged by this task), `reqwest::header::HeaderMap`. All six HTTP providers already call these helpers; because signatures are unchanged, NO provider file changes are needed. The later `send_with_retry` (D6) task consumes these helpers as-is.

D8 drift note: D8 claims chrono is already a dependency — it is NOT in `sdks/rust/Cargo.toml` at baseline. This task adds it as a direct optional dep with minimal features (`default-features = false, features = ["clock"]` — `clock` enables `Utc::now()` and pulls `std`, which `parse_from_rfc2822`/`to_rfc2822` need).

- [ ] **Step 1 — Add the chrono dep and write the failing tests.**
  In `sdks/rust/Cargo.toml`, current code (approximate lines 81–82):
  ```toml
  motosan-agent-primitives = "0.4.0"
  bytes = { version = "1", optional = true }
  ```
  Replace with:
  ```toml
  motosan-agent-primitives = "0.4.0"
  bytes = { version = "1", optional = true }
  chrono = { version = "0.4", optional = true, default-features = false, features = [
    "clock",
  ] }
  ```
  Then add the line `  "dep:chrono",` to exactly these five feature arrays in `[features]` (approx. lines 17–61), each directly after its `"dep:reqwest",` line: `anthropic`, `openai`, `gemini`, `gemini-code-assist`, `chatgpt-codex`. (Do NOT touch `minimax`, `ollama`, `ollama_native`, `claude-code`, `codex-cli`, `gemini-cli`, `full` — they inherit via feature chains, e.g. `minimax = ["anthropic"]`, `ollama_native = ["ollama"]` → `ollama = ["openai", ...]`.) Example — `anthropic` becomes:
  ```toml
  anthropic = [
    "dep:reqwest",
    "dep:chrono",
    "dep:eventsource-stream",
    "dep:tokio-stream",
    "dep:tokio",
  ]
  ```
  In `sdks/rust/src/providers/mod.rs`, insert this NEW test mod immediately after the closing brace of `sleep_before_retry` (approx. line 345) and before `#[cfg(feature = "anthropic")] pub mod anthropic;`:
  ```rust
  #[cfg(test)]
  #[cfg(any(
      feature = "anthropic",
      feature = "openai",
      feature = "minimax",
      feature = "ollama_native",
      feature = "gemini",
      feature = "gemini-code-assist",
      feature = "chatgpt-codex",
  ))]
  mod retry_after_tests {
      use super::{is_retryable_status, parse_retry_after, RETRY_AFTER_CAP};
      use reqwest::header::{HeaderMap, HeaderValue};
      use std::time::Duration;

      fn headers_with_retry_after(value: &str) -> HeaderMap {
          let mut headers = HeaderMap::new();
          headers.insert(
              "retry-after",
              HeaderValue::from_str(value).expect("ascii header value"),
          );
          headers
      }

      #[test]
      fn integer_seconds_parse() {
          let headers = headers_with_retry_after("5");
          assert_eq!(parse_retry_after(&headers), Some(Duration::from_secs(5)));
      }

      #[test]
      fn integer_seconds_above_cap_clamp_to_cap() {
          let headers = headers_with_retry_after("120");
          assert_eq!(parse_retry_after(&headers), Some(RETRY_AFTER_CAP));
      }

      #[test]
      fn http_date_near_future_parses_to_remaining_seconds() {
          // Fixed offset from now via chrono arithmetic — no wall-clock strings.
          let when = chrono::Utc::now() + chrono::Duration::seconds(30);
          let headers = headers_with_retry_after(&when.to_rfc2822());
          let delay = parse_retry_after(&headers).expect("http-date should parse");
          // signed_duration_since truncates and the test itself takes time,
          // so allow a small window below 30s.
          assert!(
              (28..=30).contains(&delay.as_secs()),
              "expected ~30s, got {delay:?}"
          );
      }

      #[test]
      fn http_date_in_past_clamps_to_zero() {
          let when = chrono::Utc::now() - chrono::Duration::seconds(100);
          let headers = headers_with_retry_after(&when.to_rfc2822());
          assert_eq!(parse_retry_after(&headers), Some(Duration::ZERO));
      }

      #[test]
      fn http_date_far_future_clamps_to_cap() {
          let when = chrono::Utc::now() + chrono::Duration::seconds(300);
          let headers = headers_with_retry_after(&when.to_rfc2822());
          assert_eq!(parse_retry_after(&headers), Some(RETRY_AFTER_CAP));
      }

      #[test]
      fn garbage_value_returns_none() {
          let headers = headers_with_retry_after("soon");
          assert_eq!(parse_retry_after(&headers), None);
      }

      #[test]
      fn missing_header_returns_none() {
          assert_eq!(parse_retry_after(&HeaderMap::new()), None);
      }

      #[test]
      fn retryable_status_set_is_408_409_429_and_5xx() {
          assert!(is_retryable_status(408));
          assert!(is_retryable_status(409));
          assert!(is_retryable_status(429));
          assert!(is_retryable_status(500));
          assert!(is_retryable_status(503));
          assert!(!is_retryable_status(400));
          assert!(!is_retryable_status(404));
          assert!(!is_retryable_status(200));
      }
  }
  ```
- [ ] **Step 2 — Run and watch it fail.** From `sdks/rust`:
  ```bash
  cargo test --all-features retry_after_tests
  ```
  Expected failure signature (compile error — the constant does not exist yet):
  ```
  error[E0432]: unresolved import `super::RETRY_AFTER_CAP`
  ```
- [ ] **Step 3 — Implement.** In `sdks/rust/src/providers/mod.rs`:
  (a) Current code (approximate lines 292–294):
  ```rust
  pub(crate) fn is_retryable_status(status_code: u16) -> bool {
      status_code == 429 || status_code >= 500
  }
  ```
  Replace with:
  ```rust
  pub(crate) fn is_retryable_status(status_code: u16) -> bool {
      status_code == 408 || status_code == 409 || status_code == 429 || status_code >= 500
  }
  ```
  (b) Current code (approximate lines 309–322, keep the `#[cfg(any(...))]` attribute above the function exactly as-is and duplicate it verbatim onto the new const):
  ```rust
  pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
      let raw = headers.get("retry-after")?.to_str().ok()?.trim();
      let seconds = raw.parse::<u64>().ok()?;
      Some(Duration::from_secs(seconds))
  }
  ```
  Replace with (the const gets its own copy of the same 8-feature `#[cfg(any(...))]` block that already sits on `parse_retry_after`):
  ```rust
  pub(crate) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60);

  pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration> {
      let raw = headers.get("retry-after")?.to_str().ok()?.trim();
      let uncapped = if let Ok(seconds) = raw.parse::<u64>() {
          Duration::from_secs(seconds)
      } else {
          // RFC 7231 HTTP-date form (e.g. "Fri, 31 Dec 1999 23:59:59 GMT").
          let when = chrono::DateTime::parse_from_rfc2822(raw).ok()?;
          let remaining = when.signed_duration_since(chrono::Utc::now());
          Duration::from_secs(remaining.num_seconds().max(0) as u64)
      };
      Some(uncapped.min(RETRY_AFTER_CAP))
  }
  ```
  (c) Verify `sleep_before_retry` (approximate lines 333–345) already uses a present `retry_after` VERBATIM — current body is `retry_after.unwrap_or_else(|| policy.delay_for_attempt(attempt))` under `policy.respect_retry_after` — confirmed at baseline; do NOT change its logic. Only add this comment line directly above `let delay = if policy.respect_retry_after {`:
  ```rust
      // retry_after arrives pre-capped to RETRY_AFTER_CAP by parse_retry_after.
  ```
- [ ] **Step 4 — Run to green, then the package suite.** From `sdks/rust`:
  ```bash
  cargo test --all-features retry_after_tests
  ```
  Expected: `test result: ok. 8 passed; 0 failed`. Then the full suite (also proves the pre-existing `tests/openai_retry.rs` pins — 400 stays non-retryable, `retry-after: 1` honored below the cap — still hold):
  ```bash
  cargo test --all-features
  ```
  Expected: all test binaries report `0 failed`.
- [ ] **Step 5 — Format and lint.** From `sdks/rust`:
  ```bash
  cargo fmt
  cargo clippy --all-features -- -D warnings
  ```
  Expected: no diffs left unstaged after fmt; clippy exits 0 with no warnings.
- [ ] **Step 6 — Commit on a branch (repo rule: every `.rs`/`Cargo.toml` change lands via PR + CI, never direct to main).**
  ```bash
  git checkout -b feat/rust-retry-after-hardening
  git add sdks/rust/Cargo.toml sdks/rust/src/providers/mod.rs
  git commit -m "feat(rust): accept HTTP-date Retry-After, cap at 60s, retry 408/409

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

### Task 4: Rust: full-jitter backoff + on_retry hook on RetryPolicy

**Scope (per canon — SCOPE-RESTRICTED):** this task touches ONLY `sdks/rust/src/retry.rs`, `sdks/rust/src/lib.rs`, and `sdks/rust/Cargo.toml`, plus a single one-field fix to the `#[cfg(test)]`-mod struct literal in `gemini.rs`. It adds full-jitter backoff (D4) and the `RetryEvent`/`RetryCause`/`on_retry` surface (D7). It does **NOT** change `sleep_before_retry` (stays 3-arg) and does **NOT** fire `on_retry` — the hook is wired into `send_with_retry` in the later send-helper task (Task 5, D6), which is the sole site that fires it. There is **no provider call-site sweep** and **no `providers/mod.rs` edit** in this task. All line numbers below are approximate (verified against baseline origin/main @ d7c06ff).

**Files:**
- Modify: `sdks/rust/src/retry.rs` (entire file — currently 69 lines; deterministic LCG jitter at approx. lines 48-53)
- Modify: `sdks/rust/Cargo.toml` (add `fastrand = "2"` under `[dependencies]`, directly below `thiserror = "2"` at approx. line 79)
- Modify: `sdks/rust/src/lib.rs` (export the two new types, approx. line 33)
- Modify: `sdks/rust/src/providers/gemini.rs` (ONE new field on the `#[cfg(test)]`-mod `RetryPolicy` struct literal at approx. lines 1129-1135 — the ONLY provider-side change, required so `--all-features` still compiles once `RetryPolicy` gains `on_retry`)
- Test: unit tests appended to `sdks/rust/src/retry.rs`

**Interfaces:**
- Produces (D7, verbatim): `pub struct RetryEvent { pub attempt: u32, pub delay: Duration, pub cause: RetryCause }`; `pub enum RetryCause { Status(u16), Network(String) }`; field `pub on_retry: Option<std::sync::Arc<dyn Fn(RetryEvent) + Send + Sync>>` on `RetryPolicy` (Clone kept via Arc; manual `Debug` skips the closure); builder setter `pub fn on_retry(mut self, on_retry: impl Fn(RetryEvent) + Send + Sync + 'static) -> Self`.
- Produces (D4): `pub fn delay_for_attempt(&self, attempt: u32) -> Duration` now full jitter — uniform in `[0, min(base_delay_ms * 2^(attempt-1), max_delay_ms)]`; injectable RNG via `pub(crate) fn delay_for_attempt_with_rng(&self, attempt: u32, rng: &mut fastrand::Rng) -> Duration`.
- Consumes: existing `RetryPolicy` fields (`max_retries`, `base_delay_ms`, `max_delay_ms`, `jitter`, `respect_retry_after`) — unchanged.
- NOT produced here: `sleep_before_retry` keeps its current 3-arg signature `pub(crate) async fn sleep_before_retry(policy: &RetryPolicy, attempt: u32, retry_after: Option<Duration>)` in `providers/mod.rs` — it gains **no** `cause` argument and fires **no** hook in this task. The `on_retry` field and builder land here as inert surface; Task 5 (`send_with_retry`, D6) is the sole place `on_retry` fires.

- [ ] **Step 1 — add the RNG dependency and write the failing tests.**
  1a. In `sdks/rust/Cargo.toml`, directly below the line `thiserror = "2"` (approx. line 79), add:
  ```toml
  fastrand = "2"
  ```
  (Verified: no `fastrand`/`rand`/`getrandom` exists anywhere in this manifest or the tree — this is a new regular dependency; `retry.rs` is compiled unconditionally so it must NOT be optional.)
  1b. Append to the END of `sdks/rust/src/retry.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      fn exp_delay_ms(policy: &RetryPolicy, attempt: u32) -> u64 {
          let exp_factor = 1_u64 << attempt.saturating_sub(1).min(31);
          policy
              .base_delay_ms
              .saturating_mul(exp_factor)
              .min(policy.max_delay_ms)
      }

      #[test]
      fn full_jitter_is_bounded_by_exponential_delay() {
          let policy = RetryPolicy::new();
          let mut rng = fastrand::Rng::with_seed(42);
          for attempt in 1..=8 {
              let bound = exp_delay_ms(&policy, attempt);
              for _ in 0..200 {
                  let delay = policy.delay_for_attempt_with_rng(attempt, &mut rng);
                  assert!(
                      delay <= Duration::from_millis(bound),
                      "attempt {attempt}: {delay:?} exceeds {bound}ms"
                  );
              }
          }
      }

      #[test]
      fn full_jitter_varies_and_is_seed_deterministic() {
          let policy = RetryPolicy::new();
          let mut a = fastrand::Rng::with_seed(7);
          let mut b = fastrand::Rng::with_seed(7);
          let draws_a: Vec<Duration> = (0..32)
              .map(|_| policy.delay_for_attempt_with_rng(4, &mut a))
              .collect();
          let draws_b: Vec<Duration> = (0..32)
              .map(|_| policy.delay_for_attempt_with_rng(4, &mut b))
              .collect();
          assert_eq!(draws_a, draws_b, "same seed must produce same delays");
          assert!(
              draws_a.iter().any(|d| d != &draws_a[0]),
              "full jitter must vary across draws: {draws_a:?}"
          );
      }

      #[test]
      fn jitter_disabled_is_exact_exponential() {
          let policy = RetryPolicy::new().jitter(false);
          assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
          assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
          assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(400));
          assert_eq!(policy.delay_for_attempt(4), Duration::from_millis(800));
          assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(1600));
          assert_eq!(policy.delay_for_attempt(6), Duration::from_millis(2000));
          assert_eq!(policy.delay_for_attempt(99), Duration::from_millis(2000));
      }

      #[test]
      fn debug_skips_on_retry_closure() {
          let with_hook = RetryPolicy::new().on_retry(|_evt| {});
          assert!(format!("{with_hook:?}").contains("on_retry: true"));
          assert!(format!("{:?}", RetryPolicy::new()).contains("on_retry: false"));
      }
  }
  ```
  (No test is added to `providers/mod.rs` — `sleep_before_retry` is unchanged, so there is nothing new to exercise there. The `RetryCause`/`RetryEvent` firing is tested by the send-helper task, Task 5.)
- [ ] **Step 2 — run and watch it fail to compile.** From `sdks/rust`: `cargo test --all-features retry`
  Expected failure signatures (the build must fail, tests must not run — `fastrand` already resolves via Step 1a, so the errors are the missing methods on `RetryPolicy`):
  ```
  error[E0599]: no method named `delay_for_attempt_with_rng` found for struct `RetryPolicy`
  error[E0599]: no method named `on_retry` found for struct `RetryPolicy`
  ```
- [ ] **Step 3a — rewrite `sdks/rust/src/retry.rs` (keep the Step 1 test mod at the bottom).** Current code (approximate lines 1-57, LCG core shown):
  ```rust
  #[derive(Debug, Clone)]
  pub struct RetryPolicy {
      pub max_retries: u32,
      pub base_delay_ms: u64,
      pub max_delay_ms: u64,
      pub jitter: bool,
      pub respect_retry_after: bool,
  }
  // ... builder setters ...
      pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
          let exponent = attempt.saturating_sub(1).min(31);
          let exp_factor = 1_u64 << exponent;
          let mut delay_ms = self.base_delay_ms.saturating_mul(exp_factor);
          delay_ms = delay_ms.min(self.max_delay_ms);

          if self.jitter {
              let jitter_seed = attempt as u64 * 1_103_515_245 + 12_345;
              let jitter_percent = jitter_seed % 100;
              let jittered = delay_ms + (delay_ms.saturating_mul(jitter_percent) / 100);
              delay_ms = jittered.min(self.max_delay_ms);
          }

          Duration::from_millis(delay_ms)
      }
  ```
  Replace everything ABOVE the `#[cfg(test)]` mod (i.e. the whole file body from the `use std::time::Duration;` at line 1 through the `Default` impl ending at line 69) with:
  ```rust
  use std::fmt;
  use std::sync::Arc;
  use std::time::Duration;

  /// Why a retry is happening. Passed to `RetryPolicy::on_retry`.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub enum RetryCause {
      /// Retryable HTTP status code received from the provider.
      Status(u16),
      /// Retryable transport/network error (message from the HTTP client).
      Network(String),
  }

  /// Fired via `RetryPolicy::on_retry` before each retry sleep.
  #[derive(Debug, Clone, PartialEq, Eq)]
  pub struct RetryEvent {
      pub attempt: u32,
      pub delay: Duration,
      pub cause: RetryCause,
  }

  #[derive(Clone)]
  pub struct RetryPolicy {
      pub max_retries: u32,
      pub base_delay_ms: u64,
      pub max_delay_ms: u64,
      pub jitter: bool,
      pub respect_retry_after: bool,
      /// Observability hook: called once before each retry sleep.
      pub on_retry: Option<Arc<dyn Fn(RetryEvent) + Send + Sync>>,
  }

  impl RetryPolicy {
      pub fn new() -> Self {
          Self::default()
      }

      pub fn max_retries(mut self, max_retries: u32) -> Self {
          self.max_retries = max_retries;
          self
      }

      pub fn base_delay_ms(mut self, base_delay_ms: u64) -> Self {
          self.base_delay_ms = base_delay_ms;
          self
      }

      pub fn max_delay_ms(mut self, max_delay_ms: u64) -> Self {
          self.max_delay_ms = max_delay_ms;
          self
      }

      pub fn jitter(mut self, jitter: bool) -> Self {
          self.jitter = jitter;
          self
      }

      pub fn respect_retry_after(mut self, respect_retry_after: bool) -> Self {
          self.respect_retry_after = respect_retry_after;
          self
      }

      pub fn on_retry(mut self, on_retry: impl Fn(RetryEvent) + Send + Sync + 'static) -> Self {
          self.on_retry = Some(Arc::new(on_retry));
          self
      }

      /// Full-jitter backoff: uniform in [0, min(base * 2^(attempt-1), max_delay)].
      pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
          self.delay_for_attempt_with_rng(attempt, &mut fastrand::Rng::new())
      }

      /// Injectable-RNG variant so tests can seed the jitter deterministically.
      pub(crate) fn delay_for_attempt_with_rng(
          &self,
          attempt: u32,
          rng: &mut fastrand::Rng,
      ) -> Duration {
          let exponent = attempt.saturating_sub(1).min(31);
          let exp_factor = 1_u64 << exponent;
          let exp_delay_ms = self
              .base_delay_ms
              .saturating_mul(exp_factor)
              .min(self.max_delay_ms);

          let delay_ms = if self.jitter {
              rng.u64(0..=exp_delay_ms)
          } else {
              exp_delay_ms
          };

          Duration::from_millis(delay_ms)
      }
  }

  impl fmt::Debug for RetryPolicy {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          f.debug_struct("RetryPolicy")
              .field("max_retries", &self.max_retries)
              .field("base_delay_ms", &self.base_delay_ms)
              .field("max_delay_ms", &self.max_delay_ms)
              .field("jitter", &self.jitter)
              .field("respect_retry_after", &self.respect_retry_after)
              .field("on_retry", &self.on_retry.is_some())
              .finish()
      }
  }

  impl Default for RetryPolicy {
      fn default() -> Self {
          Self {
              max_retries: 3,
              base_delay_ms: 100,
              max_delay_ms: 2_000,
              jitter: true,
              respect_retry_after: true,
              on_retry: None,
          }
      }
  }
  ```
  Also in `sdks/rust/src/lib.rs` (approx. line 33) change `pub use retry::RetryPolicy;` to `pub use retry::{RetryCause, RetryEvent, RetryPolicy};`.
- [ ] **Step 3b — add the missing `on_retry` field to `gemini.rs`'s test-mod struct literal (the sole provider-side edit).** In `sdks/rust/src/providers/gemini.rs`, the `#[cfg(test)]`-mod struct literal at approx. lines 1129-1135 builds a `RetryPolicy` by fields and now misses `on_retry`. Current code:
  ```rust
  let policy = RetryPolicy {
      max_retries: 2,
      base_delay_ms: 1,
      max_delay_ms: 10,
      jitter: false,
      respect_retry_after: false,
  };
  ```
  Replace with the same literal plus `on_retry: None,` as the last field:
  ```rust
  let policy = RetryPolicy {
      max_retries: 2,
      base_delay_ms: 1,
      max_delay_ms: 10,
      jitter: false,
      respect_retry_after: false,
      on_retry: None,
  };
  ```
  (This is the ONLY `RetryPolicy { .. }` struct literal in the tree outside `retry.rs` — verified. No other provider constructs `RetryPolicy` by struct literal, so no other file needs touching.)
- [ ] **Step 4 — run-pass + package suite.** From `sdks/rust`:
  `cargo test --all-features retry` — expect the 4 new tests green among the matches:
  ```
  test tests::full_jitter_is_bounded_by_exponential_delay ... ok
  test tests::full_jitter_varies_and_is_seed_deterministic ... ok
  test tests::jitter_disabled_is_exact_exponential ... ok
  test tests::debug_skips_on_retry_closure ... ok
  ```
  Then the full suite: `cargo test --all-features` — expect `0 failed` across all targets (the existing retry integration tests in `tests/openai_retry.rs` etc. all use `.jitter(false)`/0ms delays and are unaffected by full jitter; the `gemini.rs` retry test now compiles with the added `on_retry: None`).
- [ ] **Step 5 — format & lint.** From `sdks/rust`: `cargo fmt`, then `cargo clippy --all-features -- -D warnings` — expect no warnings.
- [ ] **Step 6 — commit on a branch (never direct to main; .rs/Cargo.toml changes go through PR + CI).**
  ```
  git checkout -b feat/rust-full-jitter-on-retry
  git add sdks/rust
  git commit -m "feat(retry)!: full-jitter backoff and on_retry hook on RetryPolicy

  - replace deterministic LCG jitter with full jitter (uniform 0..=exp_delay) via fastrand
  - add RetryEvent/RetryCause and RetryPolicy.on_retry (Arc<dyn Fn(RetryEvent) + Send + Sync>); manual Debug skips the closure, Clone kept via Arc
  - add on_retry: None to gemini.rs's test-mod RetryPolicy struct literal so --all-features compiles
  - the on_retry hook is inert surface here; it is fired from send_with_retry in a later task
  - BREAKING: RetryPolicy gains the on_retry field (external struct literals must add it); ships in 0.23.0

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

### Task 5: Extract Rust send_with_retry engine; migrate Anthropic + OpenAI onto it

**Ordering (read first):** This task runs **AFTER Task 2 (rust-error-enum / D1)** and **AFTER Task 4 (rust-jitter-onretry / D4+D7)** — Task 3 (rust-retryafter-status / D8) is already sequenced between them. Consequences that shape every quote below:
- **D1 already changed `map_http_error` to the 4-arg form** `map_http_error(status_code, message, retry_after, request_id)` and added `extract_request_id(headers) -> Option<String>` beside `parse_retry_after`. D1 also edited the SAME anthropic/openai terminal regions this task touches: it added a `let request_id = extract_request_id(response.headers());` capture next to each `let retry_after = parse_retry_after(...)`, and switched every terminal `return Err(map_http_error(...))` to the 4-arg call. So the "Current code" quotes below are the **post-D1** loops (4-arg calls + `request_id` capture + `extract_request_id` in the `use`), NOT the raw M1 baseline. The migrated terminal blocks MUST keep the 4-arg call and re-capture `retry_after`/`request_id` at each terminal site — dropping them regresses D1's error-attribute assertions (terminal 429 → `err.retry_after() == Some(7s)`, `err.request_id() == Some("req_abc")`).
- **D8 already made `parse_retry_after` return a value pre-capped at 60s**; the engine and terminal sites use it verbatim.
- **Task 4 is scope-restricted to `src/retry.rs` / `src/lib.rs` / `Cargo.toml`** — it did NOT touch `sleep_before_retry` (still 3-arg) and did NOT rewrite any provider call site. So the provider loops still call the 3-arg `sleep_before_retry`, they do NOT yet fire `on_retry`, and the provider `use crate::retry::RetryPolicy;` imports carry no `RetryCause`/`RetryEvent`. This task is therefore the FIRST place `on_retry` fires (via the engine), and Step 2's "observer never fires" failure is real.

**Files:**
- Modify: `sdks/rust/src/providers/mod.rs` (retry import ~line 11; insert `observe_and_sleep` + `send_with_retry` immediately after `sleep_before_retry`, whose closing brace is ~line 345, just before `#[cfg(feature = "anthropic")] pub mod anthropic;`)
- Modify: `sdks/rust/src/providers/anthropic.rs` (`use crate::providers::{...}` block ~lines 5-8; chat request loop ~lines 470-517; stream request loop ~lines 803-846)
- Modify: `sdks/rust/src/providers/openai.rs` (`use crate::providers::{...}` block ~lines 3-6; chat request loop ~lines 482-526; stream request loop ~lines 598-635; `chat_via_responses` single-shot send ~lines 247-262 — the Responses-API fallback path)
- Test: `sdks/rust/tests/openai_retry.rs` (imports ~lines 1-8; append one new test)

All line refs are approximate (baseline origin/main @ d7c06ff, then mutated by D1/D8/Task 4) — re-locate by searching for the quoted code, not by line number. Contract suites that MUST pass unchanged: `sdks/rust/tests/openai_retry.rs` (6 existing tests incl. `openai_chat_retries_502_with_non_json_body`, `openai_chat_does_not_retry_on_400`, `openai_chat_honors_retry_after_header`, `openai_stream_retries_initial_call`) and `sdks/rust/tests/anthropic_chat.rs` (7 tests incl. `anthropic_chat_retries_503_with_non_json_body` and `anthropic_setup_token_401_includes_actionable_hint`). D1's error-attribute suite (terminal 429 populates `retry_after` + `request_id`) MUST also stay green.

**Interfaces:**
Consumes (must already exist — this task is sequenced AFTER D1, D8, and the retry.rs task that shipped D4 full jitter + D7 on_retry):
- `crate::retry::RetryPolicy` with `pub on_retry: Option<std::sync::Arc<dyn Fn(RetryEvent) + Send + Sync>>`, `pub respect_retry_after: bool`, and `pub fn delay_for_attempt(&self, attempt: u32) -> Duration` (full jitter, uniform `[0, exp_delay]`)
- `crate::retry::RetryEvent { pub attempt: u32, pub delay: Duration, pub cause: RetryCause }`
- `crate::retry::RetryCause { Status(u16), Network(String) }`
- From `providers/mod.rs` (unchanged by this task): `is_retryable_status(status_code: u16) -> bool`, `is_retryable_network_error(error: &reqwest::Error) -> bool`, `parse_retry_after(headers: &HeaderMap) -> Option<Duration>` (returns a value pre-capped at 60s after D8; the engine uses it verbatim), and the **4-arg** `map_http_error(status_code: u16, message: String, retry_after: Option<Duration>, request_id: Option<String>) -> MotosanError` plus `extract_request_id(headers: &HeaderMap) -> Option<String>` (both from D1)

Produces (D6 — copy exactly; the next task, rust-migrate-rest, migrates gemini/gemini_code_assist/ollama/chatgpt_codex onto this same engine):
- `pub(crate) async fn send_with_retry(policy: &RetryPolicy, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, MotosanError>` in `sdks/rust/src/providers/mod.rs`, carrying the SAME 7-feature `#[cfg(any(...))]` attribute as the other HTTP-only helpers, and being the **SOLE place `on_retry` fires**.

Behavior contract (preserves the post-M1 status-first loops exactly): SUCCESS status OR terminal non-success (non-retryable status, or attempts exhausted) -> `Ok(response)` with body untouched — the caller does its own tolerant body parse, captures `retry_after`/`request_id` from the headers, then calls the 4-arg `map_http_error` (+ anthropic 401 auth-hint); retryable non-success -> the engine fires `on_retry` then sleeps (Retry-After verbatim when `respect_retry_after`) and retries; network error -> retry if `is_retryable_network_error` else `Err(MotosanError::Network)`, firing `on_retry` before each network-retry sleep. `on_retry` fires exactly once per retry, before each sleep, with the same `delay` value that is actually slept.

All commands run from `sdks/rust/`. House rule: work on a feature branch — every `.rs` change lands via PR + CI, never direct to main.

- [ ] **Step 1 — Write the failing on_retry observer test.** In `sdks/rust/tests/openai_retry.rs`, add two imports after the existing `use motosan_ai::{ChatRequest, Message, MotosanError, RetryPolicy};` (~line 5):

```rust
use motosan_ai::retry::{RetryCause, RetryEvent};
use std::sync::{Arc, Mutex};
```

Append at the end of the file:

```rust
#[tokio::test]
async fn openai_chat_fires_on_retry_observer_on_503_then_200() {
    let mut server = mockito::Server::new_async().await;
    let unavailable = server
        .mock("POST", "/v1/chat/completions")
        .with_status(503)
        .with_body(json!({"error": {"message": "unavailable"}}).to_string())
        .expect(1)
        .create_async()
        .await;
    let success = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(
            json!({
                "model": "gpt-5.3-codex",
                "choices": [{"message": {"content": "ok"}, "finish_reason": "stop"}],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1}
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let events: Arc<Mutex<Vec<RetryEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&events);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |event: RetryEvent| {
        sink.lock().unwrap().push(event);
    }));
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
        .expect("should retry once and succeed");

    assert_eq!(response.content, "ok");
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1, "on_retry must fire exactly once");
    assert_eq!(events[0].attempt, 1);
    assert!(matches!(events[0].cause, RetryCause::Status(503)));
    unavailable.assert_async().await;
    success.assert_async().await;
}
```

- [ ] **Step 2 — Run it, confirm the exact failure.**

```bash
cargo test --all-features --test openai_retry openai_chat_fires_on_retry_observer_on_503_then_200
```

Expected: the retry itself already happens (both mocks satisfied, chat succeeds) but nothing fires the observer, so the test panics with `assertion `left == right` failed: on_retry must fire exactly once` / `left: 0` / `right: 1`. This is the real failure because Task 4 left the hand-rolled provider loops calling the 3-arg `sleep_before_retry`, which does NOT fire `on_retry`. GUARD: if instead it fails to COMPILE with `no field `on_retry` on type `RetryPolicy`` or `unresolved import `motosan_ai::retry::RetryEvent``, the retry-policy task (D4/D7) has not landed — stop; this task must run after it.

- [ ] **Step 3a — Add the engine to `sdks/rust/src/providers/mod.rs`.** Current code (approximate line 11, inside the cfg-gated import block at the top):

```rust
use crate::retry::RetryPolicy;
```

Replace with (this is the ONLY file that names `RetryCause`/`RetryEvent`; the provider files keep importing only `RetryPolicy`):

```rust
use crate::retry::{RetryCause, RetryEvent, RetryPolicy};
```

Then, immediately after the closing brace of `sleep_before_retry` (approximate line 345, just before `#[cfg(feature = "anthropic")] pub mod anthropic;`), insert (keep `sleep_before_retry` itself — gemini/gemini_code_assist/ollama/chatgpt_codex still use it until the rust-migrate-rest task). BOTH new items carry the same 7-feature `#[cfg(any(...))]` attribute as every other reqwest-touching helper in this file (`map_http_error`, `parse_retry_after`, `sleep_before_retry`) — without it, a default-feature build (`default = []`) fails to resolve `reqwest`/`is_retryable_*`/`parse_retry_after`:

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
async fn observe_and_sleep(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
    cause: RetryCause,
) {
    // Compute the delay ONCE and reuse it for both the event and the sleep.
    // (Do NOT delegate the sleep to `sleep_before_retry` here: post-D4 the
    // delay is jittered, so a second `delay_for_attempt` call would sleep a
    // different duration than the one reported to `on_retry`.)
    let delay = if policy.respect_retry_after {
        retry_after.unwrap_or_else(|| policy.delay_for_attempt(attempt))
    } else {
        policy.delay_for_attempt(attempt)
    };
    if let Some(on_retry) = policy.on_retry.as_deref() {
        on_retry(RetryEvent {
            attempt,
            delay,
            cause,
        });
    }
    tokio::time::sleep(delay).await;
}

/// One retry engine for every HTTP provider (normative contract: specs/retry.md).
///
/// - Network error: retried while [`is_retryable_network_error`] and attempts
///   remain; otherwise `Err(MotosanError::Network)`.
/// - Retryable non-success status ([`is_retryable_status`]): sleeps (a
///   `Retry-After` value from [`parse_retry_after`], already 60s-capped by D8,
///   wins verbatim when `respect_retry_after` is set) and retries.
/// - SUCCESS status OR terminal non-success (non-retryable status, or attempts
///   exhausted): returns `Ok(response)` with the body UNTOUCHED — the caller
///   does its own tolerant body parse, captures `retry_after`/`request_id` from
///   the headers, and calls the 4-arg [`map_http_error`] (+ provider-specific
///   hints, e.g. the anthropic 401 auth hint).
///
/// `policy.on_retry` fires once per retry, before each sleep, with the exact
/// `delay` slept; `attempt` is the 1-based retry number. This is the SOLE
/// site that fires `on_retry`.
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
pub(crate) async fn send_with_retry(
    policy: &RetryPolicy,
    build: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, MotosanError> {
    let mut attempt: u32 = 0;
    loop {
        let response = match build().send().await {
            Ok(response) => response,
            Err(error) => {
                if attempt < policy.max_retries && is_retryable_network_error(&error) {
                    attempt += 1;
                    let cause = RetryCause::Network(error.to_string());
                    observe_and_sleep(policy, attempt, None, cause).await;
                    continue;
                }
                return Err(MotosanError::Network(error.to_string()));
            }
        };

        let status = response.status();
        if !status.is_success()
            && attempt < policy.max_retries
            && is_retryable_status(status.as_u16())
        {
            let retry_after = parse_retry_after(response.headers());
            attempt += 1;
            let cause = RetryCause::Status(status.as_u16());
            observe_and_sleep(policy, attempt, retry_after, cause).await;
            continue;
        }

        return Ok(response);
    }
}
```

- [ ] **Step 3b — Migrate `sdks/rust/src/providers/anthropic.rs`.** Current code — the `use crate::providers::{...}` block (approximate lines 5-8, **post-D1**, so `extract_request_id` is already imported):

```rust
use crate::providers::{
    extract_error_message, extract_request_id, is_retryable_network_error, is_retryable_status,
    map_http_error, parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
```

Replace with (drop `is_retryable_network_error`, `is_retryable_status`, `sleep_before_retry` — the engine owns them now; KEEP `parse_retry_after` + `extract_request_id`, still used at each terminal site; add `send_with_retry`):

```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
```

Leave `use crate::retry::RetryPolicy;` (approx line 9) UNCHANGED — Task 4 did not push `RetryCause`/`RetryEvent` into the providers, so nothing here needs them.

In `chat()` — current code (approximate lines 470-517, **post-D1**, from `let mut attempt = 0;` through the loop's closing `}`):

```rust
        let mut attempt = 0;
        let payload: Value;
        loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            let response = match self.apply_auth(request).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());

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
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```

Replace with (capture `retry_after` + `request_id` from the headers BEFORE the body-consuming `response.json()`):

```rust
        let response = send_with_retry(&self.retry_policy, || {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            self.apply_auth(request)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let error_payload: Value = response.json().await.unwrap_or(json!({}));
            let message = extract_error_message(&error_payload, "anthropic request failed");
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
```

In `stream()` — current code (approximate lines 803-846, **post-D1**, from `let mut attempt = 0;` through the loop's closing `};`; the `let adaptive_thinking = ...` line directly above stays):

```rust
        let mut attempt = 0;
        let response = loop {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            let response = match self.apply_auth(request).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            if status.is_success() {
                break response;
            }

            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        };
```

Replace with:

```rust
        let response = send_with_retry(&self.retry_policy, || {
            let request = self
                .http
                .post(self.endpoint())
                .header("anthropic-version", "2023-06-01")
                .json(&body);
            let request = Self::apply_beta_header(request, has_mcp, is_oauth, adaptive_thinking);
            self.apply_auth(request)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let message = response
                .text()
                .await
                .unwrap_or_else(|_| "anthropic stream request failed".to_string());
            let message = Self::with_auth_hint(
                status.as_u16(),
                message,
                Self::is_setup_token(&self.api_key),
            );
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```

The following line `let raw_stream = response.bytes_stream().eventsource();` and everything after it stays untouched.

- [ ] **Step 3c — Migrate `sdks/rust/src/providers/openai.rs`.** Current code — the `use crate::providers::{...}` block (approximate lines 3-6, **post-D1**):

```rust
use crate::providers::{
    extract_error_message, extract_request_id, is_retryable_network_error, is_retryable_status,
    map_http_error, parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
```

Replace with:

```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
```

Leave `use crate::retry::RetryPolicy;` (approx line 7) UNCHANGED.

In `chat()` — current code (approximate lines 482-526, **post-D1**; the `fallback_request` / `body` lines above stay):

```rust
        let mut attempt = 0;
        let payload: Value;
        loop {
            let response = match self
                .apply_auth(self.http.post(&self.chat_url).json(&body))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());

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
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```

Replace with (404 is never retryable, so the engine returns the 404 as a terminal `Ok(resp)` and checking the responses-fallback after it returns is observationally identical — exactly one POST to `chat_url`, then fallback):

```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(self.http.post(&self.chat_url).json(&body))
        })
        .await?;

        let status = response.status();

        if self.responses_fallback && status.as_u16() == 404 {
            return self.chat_via_responses(&fallback_request).await;
        }

        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let error_payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
            let message = extract_error_message(&error_payload, "openai request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
```

In `stream()` — current code (approximate lines 598-635, **post-D1**):

```rust
        let mut attempt = 0;
        let response = loop {
            let response = match self
                .apply_auth(self.http.post(&self.chat_url).json(&body))
                .send()
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            if status.is_success() {
                break response;
            }

            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let current_payload: Value = response
                .json()
                .await
                .unwrap_or_else(|_| json!({"error": {"message": "openai stream request failed"}}));
            let message = extract_error_message(&current_payload, "openai stream request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        };
```

Replace with:

```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(self.http.post(&self.chat_url).json(&body))
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let current_payload: Value = response
                .json()
                .await
                .unwrap_or_else(|_| json!({"error": {"message": "openai stream request failed"}}));
            let message = extract_error_message(&current_payload, "openai stream request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```

The following `let raw_stream = response.bytes_stream().eventsource();` stays untouched. `chat_via_responses` (the Responses-API fallback path) is NOT left alone — it is migrated in Step 3d below, because at baseline it is a raw single-shot `.send()` that never touches the retry engine and parses its JSON body BEFORE checking status, violating D6 (one retry engine, status-first).

- [ ] **Step 3d — Migrate `chat_via_responses` (the OpenAI Responses-API fallback) in `sdks/rust/src/providers/openai.rs`.** This is the path `chat()` delegates to on a 404 when `responses_fallback` is set. At baseline it was left as a raw single-shot `.send()` that parses the JSON body BEFORE checking status — it never went through the retry engine (D6 violation) and is not status-first. Current code (**post-D1**, approximate lines 247-262 — D1 added the `retry_after`/`request_id` captures after `let status = response.status();` and switched the terminal call to the 4-arg `map_http_error`, but kept the raw `.send()` and the pre-status body parse):

```rust
        let response = self
            .apply_auth(self.http.post(&self.responses_url).json(&body))
            .send()
            .await
            .map_err(|error| MotosanError::Network(error.to_string()))?;

        let status = response.status();
        let retry_after = parse_retry_after(response.headers());
        let request_id = extract_request_id(response.headers());
        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;

        if !status.is_success() {
            let message = extract_error_message(&payload, "openai responses request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```

Replace with (SAME pattern as the chat/stream migrations: build closure, `send_with_retry`, then a status-FIRST terminal branch with a tolerant body parse + `retry_after`/`request_id` captured before the body is consumed + the 4-arg `map_http_error`; on success, parse `payload` strictly — that binding stays in scope for the `extract_responses_text`/usage code below, which is unchanged):

```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(self.http.post(&self.responses_url).json(&body))
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let error_payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
            let message = extract_error_message(&error_payload, "openai responses request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
```

Everything from `let content = Self::extract_responses_text(&payload);` down is unchanged. No new import is needed — Step 3c already added `send_with_retry` to this file's `use crate::providers::{...}` block, and `parse_retry_after`/`extract_request_id`/`map_http_error`/`extract_error_message`/`json!` are all already in scope. The existing `tests/openai_provider.rs::openai_chat_can_fallback_to_responses_api_on_404` test (a single 200 on `/v1/responses`) stays green: `send_with_retry` returns that 200 on the first attempt, so the fallback is still exactly one POST to the responses endpoint.

- [ ] **Step 4 — Run the new test, the contract suites, the full suite, then prove the default build still compiles.**

```bash
cargo test --all-features --test openai_retry
cargo test --all-features --test anthropic_chat
cargo test --all-features
cargo check --no-default-features
```

Expected: `openai_retry` → `test result: ok. 7 passed; 0 failed` (6 pre-existing incl. `openai_chat_retries_502_with_non_json_body`, `openai_chat_does_not_retry_on_400`, `openai_chat_honors_retry_after_header`, `openai_stream_retries_initial_call` — all unchanged — plus the new observer test); `anthropic_chat` → `test result: ok. 7 passed; 0 failed` (incl. `anthropic_chat_retries_503_with_non_json_body` and `anthropic_setup_token_401_includes_actionable_hint` — the auth-hint proves terminal responses still get caller-side hints). D1's error-attribute suite stays green because the migrated terminal blocks still capture `retry_after` + `request_id` and pass them to the 4-arg `map_http_error`. The full suite also exercises `tests/openai_provider.rs::openai_chat_can_fallback_to_responses_api_on_404`, which stays green after the Step 3d migration (single 200 on `/v1/responses` → no retry, one fallback POST). Full suite all green (live tests stay ignored); `cargo check --no-default-features` compiles clean (guards against the missing-`#[cfg]`-on-`send_with_retry` regression, which `--all-features` would never catch). If any pre-existing retry test fails, the engine has diverged from the loop semantics quoted above — fix the engine, never the test.

- [ ] **Step 5 — Format and lint.**

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```

Expected: no diffs beyond whitespace, zero clippy warnings. In particular no "unused import" — the dropped `is_retryable_network_error`/`is_retryable_status`/`sleep_before_retry` are gone from the provider `use` blocks, and `parse_retry_after`/`extract_request_id`/`map_http_error` remain because each terminal site still calls them.

- [ ] **Step 6 — Commit on the feature branch and push for PR + CI** (never direct to main for `.rs` changes):

```bash
git checkout -b refactor/rust-send-with-retry 2>/dev/null || true
git add sdks/rust/src/providers/mod.rs sdks/rust/src/providers/anthropic.rs sdks/rust/src/providers/openai.rs sdks/rust/tests/openai_retry.rs
git commit -m "refactor(rust): route anthropic/openai request loops through send_with_retry

One retry engine (D6): success or terminal non-success returns the raw
response for caller-side tolerant parse + 4-arg map_http_error (with
retry_after + request_id) + auth hints; network exhaustion -> Network
error; on_retry observer fires before each sleep. Providers keep only
serialization + response handling. The OpenAI Responses-API fallback
(chat_via_responses) now routes through the same engine and is status-first
(previously a raw single-shot send that parsed the body before the status).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 6: Migrate ollama, gemini, gemini_code_assist, chatgpt_codex onto send_with_retry

> **Execute AFTER Task 5 (send_with_retry), which runs after Task 2 (D1) and Task 4 (jitter).** By the time this task runs the tree already has: `send_with_retry` in `providers/mod.rs` (Task 5); the 4-arg `map_http_error` + `extract_request_id` in `providers/mod.rs` (Task 2 / D1); the `on_retry` field + `RetryEvent` + `RetryCause` in `src/retry.rs` (Task 4 / D7). `sleep_before_retry` stays 3-arg, but `send_with_retry` does NOT call it — the engine sleeps inline via `observe_and_sleep` (Task 5). At this task's start, the ONLY remaining callers of `sleep_before_retry` are the four hand-rolled loops this task deletes (Task 5 already moved anthropic/openai off it). Once the last provider migrates, `sleep_before_retry` has zero callers, so this task DELETES it (new Step 7) — otherwise `cargo clippy --all-features -- -D warnings` fails on `dead_code`. Because Task 4 is scope-restricted to `retry.rs`/`lib.rs`/`Cargo.toml`, the four provider files at pre-task state still hold their post-M1 hand-rolled loops (3-arg `sleep_before_retry`, `use crate::retry::RetryPolicy;`) except that D1 has already rewritten their terminal `map_http_error` calls to the 4-arg form and inserted `extract_request_id` captures — irrelevant here because this task deletes those loops wholesale and re-emits the terminal blocks fresh (below).

Collapses the four remaining hand-rolled Rust HTTP request/retry loops onto the shared engine. Behavior is preserved: each provider keeps its own terminal-error body handling (tolerant NDJSON/bytes parse for ollama stream, `resp.json().unwrap_or(json!({}))` + `extract_error_message` for the rest); only the loop/sleep/classification plumbing moves into `send_with_retry`, which also fires `RetryPolicy.on_retry`. `gemini_code_assist::chat` and `chatgpt_codex::chat` delegate to `stream`, so migrating each `stream` loop covers both paths. All seven terminal blocks call the **4-arg** `map_http_error(status, msg, parse_retry_after(headers), extract_request_id(headers))`, capturing `retry_after`/`request_id` from the response headers *before* the body is consumed.

**Files:**
- Modify: `sdks/rust/src/providers/ollama.rs` (imports ~lines 3-6; chat loop ~249-285; stream loop ~352-395; file is 653 lines)
- Modify: `sdks/rust/src/providers/gemini.rs` (imports ~5-8; chat loop ~324-365; stream loop ~372-415)
- Modify: `sdks/rust/src/providers/gemini_code_assist.rs` (imports ~6-9; stream loop ~125-169)
- Modify: `sdks/rust/src/providers/chatgpt_codex.rs` (imports ~2-5; stream loop ~259-306)
- Modify: `sdks/rust/src/providers/mod.rs` (DELETE the now-dead `sleep_before_retry` ~lines 333-345 after the last provider migrates — Step 7)
- Test: `sdks/rust/tests/ollama_native_provider.rs` (append after ~line 522), `sdks/rust/tests/gemini_provider.rs` (append after ~line 667), `sdks/rust/tests/gemini_code_assist.rs` (append after ~line 456), `sdks/rust/tests/chatgpt_codex.rs` (append after ~line 179)

All line refs are **approximate** — requote against the live pre-task tree before editing.

**Interfaces:**
- Consumes (from Task 5, `sdks/rust/src/providers/mod.rs`): `pub(crate) async fn send_with_retry(policy: &RetryPolicy, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, MotosanError>` — returns `Ok(response)` for success OR terminal (non-retryable / attempts-exhausted) statuses with body untouched; `Err(MotosanError::Network(...))` for terminal network errors; fires `on_retry` before each sleep.
- Consumes (Task 2 / D1, `sdks/rust/src/providers/mod.rs`): `pub(crate) fn map_http_error(status_code: u16, message: String, retry_after: Option<Duration>, request_id: Option<String>) -> MotosanError` (**4-arg** post-D1); `pub(crate) fn extract_request_id(headers: &HeaderMap) -> Option<String>`; `pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration>` (signature unchanged; parsing extended by Task 3 / D8); `pub(crate) fn extract_error_message(payload: &Value, fallback: &str) -> String`.
- Consumes (Task 4 / D7, `sdks/rust/src/retry.rs`): `pub on_retry: Option<std::sync::Arc<dyn Fn(RetryEvent) + Send + Sync>>` on `RetryPolicy`; `pub struct RetryEvent { pub attempt: u32, pub delay: Duration, pub cause: RetryCause }`; `pub enum RetryCause { Status(u16), Network(String) }`. **These three D7 names are referenced only inside `send_with_retry` (in `providers/mod.rs`) and inside the new test files — NOT in the four provider modules.** Each provider's retry import therefore stays `use crate::retry::RetryPolicy;` unchanged (no `RetryCause`/`RetryEvent`).
- Produces: no new public API; four providers routed through the shared engine.
- Removes: `pub(crate) async fn sleep_before_retry(policy, attempt, retry_after)` from `providers/mod.rs` — dead once the last hand-rolled loop is gone (its former callers were exactly these four provider loops plus the two Task 5 migrated off; `send_with_retry` sleeps inline via `observe_and_sleep`, so nothing calls it after this task). `is_retryable_status`/`is_retryable_network_error`/`parse_retry_after` stay — `send_with_retry` and the terminal blocks still call them.

All commands run from `sdks/rust/`. Ollama is feature `ollama_native` — always use `--all-features`.

- [ ] **Step 1 — Add four failing on_retry tests (one per provider test file).** They pin that retries route through the shared engine: the hand-rolled loops sleep via 3-arg `sleep_before_retry`, which does NOT fire `on_retry` (per D7, only `send_with_retry` fires it), so each test fails until its provider is migrated. Append to `sdks/rust/tests/ollama_native_provider.rs` (end of file, after `ollama_native_chat_retries_non_json_5xx_then_succeeds`):

```rust
#[tokio::test]
async fn ollama_native_chat_fires_on_retry_via_shared_engine() {
    use motosan_ai::retry::RetryCause;
    use std::sync::{Arc, Mutex};

    let mut server = mockito::Server::new_async().await;
    let error_mock = server
        .mock("POST", "/api/chat")
        .with_status(503)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
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
                "done": true
            })
            .to_string(),
        )
        .expect(1)
        .create_async()
        .await;

    let seen: Arc<Mutex<Vec<(u32, u16)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |evt| {
        let status = match evt.cause {
            RetryCause::Status(code) => code,
            RetryCause::Network(_) => 0,
        };
        sink.lock().unwrap().push((evt.attempt, status));
    }));

    let provider = build_provider(server.url()).with_retry_policy(policy);
    let request = ChatRequest::builder()
        .message(Message::user("hello"))
        .build();

    let response = provider.chat(request).await.expect("retry then succeed");
    assert_eq!(response.content, "recovered");
    assert_eq!(*seen.lock().unwrap(), vec![(1, 503)]);
    error_mock.assert_async().await;
    success_mock.assert_async().await;
}
```

Append to `sdks/rust/tests/gemini_provider.rs` (end of file):

```rust
#[tokio::test]
async fn chat_fires_on_retry_via_shared_engine() {
    use motosan_ai::retry::RetryCause;
    use std::sync::{Arc, Mutex};

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(503)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", Matcher::Regex("generateContent".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(ok_body("recovered"))
        .expect(1)
        .create_async()
        .await;

    let seen: Arc<Mutex<Vec<(u32, u16)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let mut policy = fast_retry();
    policy.on_retry = Some(Arc::new(move |evt| {
        let status = match evt.cause {
            RetryCause::Status(code) => code,
            RetryCause::Network(_) => 0,
        };
        sink.lock().unwrap().push((evt.attempt, status));
    }));

    let provider = GeminiProvider::new("key", None, Some(server.url())).with_retry_policy(policy);
    let resp = provider
        .chat(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(resp.content, "recovered");
    assert_eq!(*seen.lock().unwrap(), vec![(1, 503)]);
}
```

Append to `sdks/rust/tests/gemini_code_assist.rs` (end of file) — same body as the gemini test above with these substitutions: test name `chat_fires_on_retry_via_shared_engine`, both matchers `Matcher::Regex("streamGenerateContent".into())`, success mock uses `.with_header("content-type", "text/event-stream")` and `.with_body(sse_text("recovered", Some("STOP")))`, and the provider line is:

```rust
    let provider =
        GeminiCodeAssistProvider::new("ya29.fake", "my-project", None, Some(server.url()))
            .with_retry_policy(policy);
```

Append to `sdks/rust/tests/chatgpt_codex.rs` (end of file):

```rust
#[tokio::test]
async fn stream_fires_on_retry_via_shared_engine() {
    use motosan_ai::retry::RetryCause;
    use std::sync::{Arc, Mutex};

    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", Matcher::Any)
        .with_status(503)
        .with_body(r#"{"error":{"message":"overloaded"}}"#)
        .expect(1)
        .create_async()
        .await;
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(FIXTURE)
        .expect(1)
        .create_async()
        .await;

    let seen: Arc<Mutex<Vec<(u32, u16)>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    let mut policy = RetryPolicy::new()
        .max_retries(1)
        .base_delay_ms(0)
        .max_delay_ms(0)
        .jitter(false);
    policy.on_retry = Some(Arc::new(move |evt| {
        let status = match evt.cause {
            RetryCause::Status(code) => code,
            RetryCause::Network(_) => 0,
        };
        sink.lock().unwrap().push((evt.attempt, status));
    }));

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()))
            .with_retry_policy(policy);
    let mut stream = provider
        .stream(
            ChatRequest::builder()
                .messages(vec![Message::user("hi")])
                .build(),
        )
        .await
        .unwrap();
    let mut text = String::new();
    while let Some(item) = stream.next().await {
        let ev = item.expect("stream item should not fail");
        if ev.event_type == StreamEventType::Text {
            text.push_str(&ev.content);
        }
    }
    assert_eq!(text, EXPECTED_TEXT);
    assert_eq!(*seen.lock().unwrap(), vec![(1, 503)]);
}
```

- [ ] **Step 2 — Run the new tests; all four must fail.**
```bash
cargo test --all-features via_shared_engine
```
Expected: 4 test binaries each run 1 matching test and FAIL on the `seen` assertion. The hand-rolled loops retry successfully — content is "recovered"/fixture text — but sleep via 3-arg `sleep_before_retry`, which never invokes `on_retry` (D7 restricts firing to `send_with_retry`), so `seen` stays empty:
```
assertion `left == right` failed
  left: []
 right: [(1, 503)]
```
If they fail to COMPILE with `no field on_retry on type RetryPolicy` or unresolved `motosan_ai::retry::RetryEvent`/`RetryCause`, Task 4 (D7: `on_retry`/`RetryEvent`/`RetryCause`) has not landed; if `send_with_retry` is unresolved, Task 5 (D6) has not landed — stop and execute the missing task first.

- [ ] **Step 3 — Migrate `sdks/rust/src/providers/ollama.rs`.** Current imports (approximate lines 3-6):
```rust
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
```
Replace with (drop `is_retryable_network_error`/`is_retryable_status`/`sleep_before_retry`; add `extract_request_id`/`send_with_retry`; keep `parse_retry_after` — the terminal blocks still call it):
```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
```
Leave the separate `use crate::retry::RetryPolicy;` line (approximate line 7) unchanged. Current `chat` loop (approximate lines 249-285, from `let mut attempt = 0;` through the closing `}` of the `loop`):
```rust
        let mut attempt = 0;
        let payload: Value;

        loop {
            let response = match self.http.post(self.endpoint()).json(&body).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            let retry_after = parse_retry_after(response.headers());

            if !status.is_success() {
                if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
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
        }
```
(D1 has already rewritten the terminal `map_http_error` call above to 4 args and added an `extract_request_id` capture; ignore that — the whole block is deleted.) Replace with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.http.post(self.endpoint()).json(&body)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let error_payload: Value = response.json().await.unwrap_or_else(|_| json!({}));
            let message = extract_error_message(&error_payload, "ollama request failed");
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }

        let payload: Value = response
            .json()
            .await
            .map_err(|error| MotosanError::ProviderError(error.to_string()))?;
```
Everything from `let message = payload.get("message");` down is unchanged. Current `stream` loop (approximate lines 352-395, from `let mut attempt = 0;` through the `};` closing `let response = loop {`):
```rust
        let mut attempt = 0;

        let response = loop {
            let response = match self.http.post(self.endpoint()).json(&body).send().await {
                Ok(response) => response,
                Err(error) => {
                    if attempt < self.retry_policy.max_retries && is_retryable_network_error(&error)
                    {
                        attempt += 1;
                        sleep_before_retry(&self.retry_policy, attempt, None).await;
                        continue;
                    }
                    return Err(MotosanError::Network(error.to_string()));
                }
            };

            let status = response.status();
            if status.is_success() {
                break response;
            }

            let retry_after = parse_retry_after(response.headers());
            if attempt < self.retry_policy.max_retries && is_retryable_status(status.as_u16()) {
                attempt += 1;
                sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                continue;
            }

            let body_bytes = response.bytes().await.ok();
            if let Some(payload) = body_bytes
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            {
                let message = extract_error_message(&payload, "ollama stream request failed");
                return Err(map_http_error(status.as_u16(), message));
            }

            let message = body_bytes
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "ollama stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message));
        };
```
(Same as chat: D1 has already made those two terminal `map_http_error` calls 4-arg — the whole block is deleted regardless.) Replace with (the M1 tolerant terminal body parse stays caller-side; capture `retry_after`/`request_id` once, before `response.bytes()` consumes the body, and pass them to both terminal calls):
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.http.post(self.endpoint()).json(&body)
        })
        .await?;

        let status = response.status();
        if !status.is_success() {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let body_bytes = response.bytes().await.ok();
            if let Some(payload) = body_bytes
                .as_deref()
                .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            {
                let message = extract_error_message(&payload, "ollama stream request failed");
                return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
            }

            let message = body_bytes
                .as_deref()
                .map(|bytes| String::from_utf8_lossy(bytes).trim().to_string())
                .filter(|message| !message.is_empty())
                .unwrap_or_else(|| "ollama stream request failed".to_string());
            return Err(map_http_error(status.as_u16(), message, retry_after, request_id));
        }
```
`request_id` is `Option<String>` but is consumed in two mutually-exclusive diverging arms (each `return`s), so the borrow checker accepts the reuse; `retry_after` is `Option<Duration>` (Copy). The `// NDJSON parsing: ...` block below is unchanged.

- [ ] **Step 4 — Migrate `sdks/rust/src/providers/gemini.rs`.** Current imports (approximate lines 5-8):
```rust
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ChatResponseBuilder, ProviderImpl,
};
```
Replace with:
```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ChatResponseBuilder, ProviderImpl,
};
```
Leave the separate `use crate::retry::RetryPolicy;` line (approximate line 9) unchanged. Current `chat` loop (approximate lines 324-365):
```rust
        let mut attempt = 0u32;
        loop {
            let result = self
                .apply_auth(
                    self.http
                        .post(&url)
                        .header("content-type", "application/json"),
                )
                .json(&body)
                .send()
                .await;

            match result {
                Err(e)
                    if is_retryable_network_error(&e)
                        && attempt < self.retry_policy.max_retries =>
                {
                    attempt += 1;
                    sleep_before_retry(&self.retry_policy, attempt, None).await;
                    continue;
                }
                Err(e) => return Err(MotosanError::Network(e.to_string())),
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    if status != 200 {
                        let retry_after = parse_retry_after(resp.headers());
                        let payload: Value = resp.json().await.unwrap_or(json!({}));
                        let msg = extract_error_message(&payload, "Gemini API error");
                        if is_retryable_status(status) && attempt < self.retry_policy.max_retries {
                            attempt += 1;
                            sleep_before_retry(&self.retry_policy, attempt, retry_after).await;
                            continue;
                        }
                        return Err(map_http_error(status, msg));
                    }
                    let payload: Value = resp.json().await.map_err(|e| {
                        MotosanError::ProviderError(format!("failed to parse Gemini response: {e}"))
                    })?;
                    return Ok(Self::parse_response(&payload, &model));
                }
            }
        }
```
(D1 has already rewritten the terminal `map_http_error` to 4-arg and captured `request_id` alongside `retry_after` — deleted regardless.) Replace with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini API error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }
        let payload: Value = response.json().await.map_err(|e| {
            MotosanError::ProviderError(format!("failed to parse Gemini response: {e}"))
        })?;
        Ok(Self::parse_response(&payload, &model))
```
Current `stream` loop (approximate lines 372-415) is the same shape with `"Gemini stream error"` and a success arm that builds the SSE adapter. Replace it with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini stream error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }
        let sse = response.bytes_stream().eventsource();
        let adapter = GeminiStreamAdapter {
            inner: Box::pin(sse),
            pending: VecDeque::new(),
        };
        Ok(Box::pin(adapter))
```
Note the pre-M2 loop parsed the error body BEFORE deciding to retry and discarded it on retry; `send_with_retry` never touches the body, so terminal behavior (what callers observe) is identical. Capture `retry_after`/`request_id` before `response.json()` consumes the body.

- [ ] **Step 5 — Migrate `sdks/rust/src/providers/gemini_code_assist.rs`.** Current imports (approximate lines 6-9):
```rust
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ProviderImpl,
};
```
Replace with:
```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ProviderImpl,
};
```
Leave the separate `use crate::retry::RetryPolicy;` line (approximate line 10) unchanged. Current `stream` loop (approximate lines 125-169) — identical shape to gemini's stream loop but with fallback `"Gemini Code Assist error"` and success arm:
```rust
                    let sse = resp.bytes_stream().eventsource();
                    let adapter = CodeAssistStreamAdapter {
                        inner: Box::pin(sse),
                        pending: VecDeque::new(),
                        seen_tool_ids: std::collections::HashSet::new(),
                    };
                    return Ok(Box::pin(adapter));
```
Replace the whole `let mut attempt = 0u32; loop { ... }` block with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "Gemini Code Assist error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }
        let sse = response.bytes_stream().eventsource();
        let adapter = CodeAssistStreamAdapter {
            inner: Box::pin(sse),
            pending: VecDeque::new(),
            seen_tool_ids: std::collections::HashSet::new(),
        };
        Ok(Box::pin(adapter))
```
`chat()` (~line 111) delegates to `stream()` + `collect_stream` — no change there.

- [ ] **Step 6 — Migrate `sdks/rust/src/providers/chatgpt_codex.rs`.** Current imports (approximate lines 2-5):
```rust
use crate::providers::{
    extract_error_message, is_retryable_network_error, is_retryable_status, map_http_error,
    parse_retry_after, sleep_before_retry, ProviderImpl,
};
```
Replace with:
```rust
use crate::providers::{
    extract_error_message, extract_request_id, map_http_error, parse_retry_after, send_with_retry,
    ProviderImpl,
};
```
Leave the separate `use crate::retry::RetryPolicy;` line (approximate line 6) unchanged. Current `stream` loop (approximate lines 259-306) — same shape again, fallback `"ChatGPT-backend error"`, success arm builds `ChatGptCodexStreamAdapter`. Replace the whole `let mut attempt = 0u32; loop { ... }` block with:
```rust
        let response = send_with_retry(&self.retry_policy, || {
            self.apply_auth(
                self.http
                    .post(&url)
                    .header("content-type", "application/json"),
            )
            .json(&body)
        })
        .await?;

        let status = response.status().as_u16();
        if status != 200 {
            let retry_after = parse_retry_after(response.headers());
            let request_id = extract_request_id(response.headers());
            let payload: Value = response.json().await.unwrap_or(json!({}));
            let msg = extract_error_message(&payload, "ChatGPT-backend error");
            return Err(map_http_error(status, msg, retry_after, request_id));
        }
        let sse = response.bytes_stream().eventsource();
        let adapter = ChatGptCodexStreamAdapter {
            inner: Box::pin(sse),
            pending: VecDeque::new(),
            item_to_call_id: HashMap::new(),
            seen_tool_ids: HashSet::new(),
            saw_tool_call: false,
            error: None,
        };
        Ok(Box::pin(adapter))
```
`chat()` (~line 245) delegates to `stream()` + `collect_stream` — no change there.

- [ ] **Step 7 — Delete the now-dead `sleep_before_retry` from `sdks/rust/src/providers/mod.rs`.** All six HTTP providers now sleep through the shared engine (anthropic/openai via Task 5; ollama/gemini/gemini_code_assist/chatgpt_codex via Steps 3-6), and `send_with_retry` sleeps INLINE via `observe_and_sleep` — it never calls `sleep_before_retry`. So `sleep_before_retry` has zero callers; leaving it triggers `dead_code`, which fails `cargo clippy --all-features -- -D warnings`. First confirm no callers remain:
```bash
grep -rn "sleep_before_retry(" sdks/rust/src/
```
Expected: exactly ONE hit — the definition line `pub(crate) async fn sleep_before_retry(` in `sdks/rust/src/providers/mod.rs`. Every provider `use crate::providers::{...}` block and call site was dropped by Steps 3-6 (and Task 5 for anthropic/openai). (The only other textual mention is a prose comment inside `observe_and_sleep` — "Do NOT delegate the sleep to `sleep_before_retry` here" — which has no `(`, so it is not matched and is a harmless design note.) Then delete the whole function, including its `#[cfg(any(...))]` attribute (approximate lines 333-345):
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
pub(crate) async fn sleep_before_retry(
    policy: &RetryPolicy,
    attempt: u32,
    retry_after: Option<Duration>,
) {
    let delay = if policy.respect_retry_after {
        retry_after.unwrap_or_else(|| policy.delay_for_attempt(attempt))
    } else {
        policy.delay_for_attempt(attempt)
    };

    tokio::time::sleep(delay).await;
}
```
Re-run `grep -rn "sleep_before_retry(" sdks/rust/src/` — it must now print NOTHING. Do NOT delete `is_retryable_status`, `is_retryable_network_error`, or `parse_retry_after`: they are still called from inside `send_with_retry`/`observe_and_sleep` and the terminal blocks, so they stay live. This deletion is exactly what keeps Step 9's `cargo clippy --all-features -- -D warnings` green.

- [ ] **Step 8 — Run new tests, then the four provider suites unchanged, then the full suite.**
```bash
cargo test --all-features via_shared_engine
cargo test --all-features --test ollama_native_provider --test gemini_provider --test gemini_code_assist --test chatgpt_codex
cargo test --all-features
```
Expected: the 4 `via_shared_engine` tests pass; every pre-existing test in the four files passes UNCHANGED (in particular `ollama_native_chat_retries_non_json_5xx_then_succeeds`, `chat_retries_429_then_succeeds`, `chat_retries_500_then_succeeds`, `chat_exhausts_retries_on_persistent_500`, `stream_retries_500_then_succeeds`, `chat_500_retries_then_succeeds`, `stream_401_returns_auth_error`); full suite green, 0 failed. Do not edit any pre-existing test — if one fails, the migration changed behavior; fix the provider, not the test.

- [ ] **Step 9 — Format and lint** (clippy at `-D warnings` also catches any leftover unused import, and the `dead_code` that would fire on `sleep_before_retry` if Step 7 were skipped):
```bash
cargo fmt
cargo clippy --all-features -- -D warnings
```
Expected: no diffs beyond the edited files, zero warnings, and `cargo clippy --all-features -- -D warnings` PASSES. Each provider's import now drops `is_retryable_network_error`/`is_retryable_status`/`sleep_before_retry` (no longer referenced in-module) and keeps `parse_retry_after`/`extract_request_id` (still called by the terminal blocks) plus `send_with_retry` — so no `unused_imports`. `is_retryable_status`, `is_retryable_network_error`, and `parse_retry_after` remain live crate-wide (their callers are `send_with_retry`/`observe_and_sleep` and the terminal blocks), so no `dead_code`. `sleep_before_retry` was DELETED in Step 7 — it had zero remaining callers (`send_with_retry` sleeps inline), and had it been left it would have failed this clippy gate on `dead_code`.

- [ ] **Step 10 — Commit and open a PR** (repo rule: every `.rs` change lands via PR + CI, never direct to main):
```bash
git checkout -b refactor/providers-send-with-retry-remaining
git add sdks/rust/src/providers/ollama.rs sdks/rust/src/providers/gemini.rs sdks/rust/src/providers/gemini_code_assist.rs sdks/rust/src/providers/chatgpt_codex.rs sdks/rust/src/providers/mod.rs sdks/rust/tests/ollama_native_provider.rs sdks/rust/tests/gemini_provider.rs sdks/rust/tests/gemini_code_assist.rs sdks/rust/tests/chatgpt_codex.rs
git commit -m "refactor(providers): route ollama/gemini/gca/chatgpt-codex through send_with_retry

Collapse the four remaining hand-rolled HTTP retry loops onto the shared
engine; terminal error-body handling stays caller-side and unchanged,
calling the 4-arg map_http_error with per-site retry_after/request_id.
Deletes the now-dead sleep_before_retry (every provider sleeps via the
engine's inline observe_and_sleep, so it had zero callers). Adds one
on_retry-observability test per provider.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
git push -u origin refactor/providers-send-with-retry-remaining
gh pr create --title "refactor(providers): route remaining HTTP providers through send_with_retry" --fill
```
Wait for CI green before merge.


## P — Python: error attributes, RetryPolicy, client threading

### Task 7: Add structured HTTP metadata to Python errors and populate at provider raise sites

**Files:**
- Modify: `sdks/python/motosan_ai/error.py` (replace whole file; currently 31 lines)
- Modify: `sdks/python/motosan_ai/retry.py` (add helper after constants, approx lines 24-29)
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (approx lines 11, 298-304, 342, 402)
- Modify: `sdks/python/motosan_ai/providers/openai.py` (approx lines 10, 156-162, 181, 229)
- Modify: `sdks/python/motosan_ai/providers/gemini.py` (approx lines 11, 204-210, 222, 273)
- Modify: `sdks/python/motosan_ai/providers/gemini_code_assist.py` (approx lines 15, 188-194, 212)
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py` (approx lines 18, 326-332, 354)
- Modify: `sdks/python/motosan_ai/providers/minimax.py` (approx lines 17, 99-107, 155, 229)
- Modify: `sdks/python/motosan_ai/providers/ollama.py` (approx lines 11, 111-112, 159-163)
- Test: `sdks/python/tests/test_errors.py` (replace whole file; currently 16 lines)
- Test: `sdks/python/tests/test_retry.py` (add imports at top; append one class after approx line 216)

**Interfaces:**
- Produces (D2 verbatim): `MotosanError.__init__(self, message: str = "", *, status_code: int | None = None, retry_after: float | None = None, request_id: str | None = None)` — stores the three attributes, calls `super().__init__(message)`; all subclasses inherit unchanged.
- Produces (D8, consumed by the later Python RetryPolicy/D9 task): `RETRY_AFTER_CAP_SECS = 60.0` and `parse_retry_after_header(value: str | None) -> float | None` in `motosan_ai/retry.py` (delta-seconds — integer OR decimal, e.g. `"1.5"` → 1.5 — AND RFC 7231 HTTP-date, clamped to [0, 60]; a negative delta-seconds value is invalid → None, while a past HTTP-date means retry-immediately → 0.0). Task 8 (py-retrypolicy) preserves this exact function verbatim.
- Produces (provider-internal): `_map_http_error(status: int, message: str, headers: httpx.Headers | None = None) -> Exception` in anthropic/openai/gemini/gemini_code_assist/chatgpt_codex; `MinimaxProvider._raise_for_status(status_code: int, message: str, headers: httpx.Headers | None = None) -> None`.
- The M1 "HTTP {status}: ..." / "Retry-After: N\n" message strings stay byte-identical. `LlmClient` Protocol untouched (additive only). Does NOT touch `client.py` or the existing `_parse_retry_after`/`_STATUS_5XX_RE` message-scrapers (the D9 task removes those).

- [ ] **Step 1 — Write failing tests.** Replace `sdks/python/tests/test_errors.py` with exactly:
```python
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime

import httpx
import pytest
import respx

from motosan_ai.error import (
    AuthError,
    InvalidRequestError,
    MotosanError,
    ProviderError,
    RateLimitError,
)
from motosan_ai.providers.anthropic import AnthropicProvider
from motosan_ai.providers.minimax import MinimaxProvider
from motosan_ai.types import ChatRequest, Message

_URL = "https://mock.anthropic.com/v1/messages"


@pytest.fixture
def provider():
    return AnthropicProvider("test-key", base_url="https://mock.anthropic.com")


def test_error_mapping():
    with pytest.raises(AuthError):
        MinimaxProvider._raise_for_status(401, "unauthorized")
    with pytest.raises(RateLimitError):
        MinimaxProvider._raise_for_status(429, "rate")
    with pytest.raises(InvalidRequestError):
        MinimaxProvider._raise_for_status(400, "bad")
    with pytest.raises(ProviderError):
        MinimaxProvider._raise_for_status(500, "oops")


class TestErrorAttributes:
    def test_kwargs_stored_and_str_unchanged(self):
        err = RateLimitError(
            "Retry-After: 120\nHTTP 429: slow down",
            status_code=429,
            retry_after=60.0,
            request_id="req_123",
        )
        assert err.status_code == 429
        assert err.retry_after == 60.0
        assert err.request_id == "req_123"
        assert str(err) == "Retry-After: 120\nHTTP 429: slow down"

    def test_defaults_are_none(self):
        err = ProviderError("boom")
        assert err.status_code is None
        assert err.retry_after is None
        assert err.request_id is None
        assert str(err) == "boom"

    def test_all_subclasses_accept_kwargs(self):
        for cls in (AuthError, RateLimitError, InvalidRequestError, ProviderError):
            err = cls("m", status_code=503)
            assert isinstance(err, MotosanError)
            assert err.status_code == 503


@respx.mock
@pytest.mark.asyncio
async def test_429_populates_attributes_and_caps_retry_after(provider):
    respx.post(_URL).mock(
        return_value=httpx.Response(
            429,
            headers={"retry-after": "120", "request-id": "req_abc"},
            json={"error": {"message": "slow down"}},
        )
    )
    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    err = exc.value
    assert err.status_code == 429
    assert err.retry_after == 60.0  # 120s capped at RETRY_AFTER_CAP_SECS
    assert err.request_id == "req_abc"
    assert str(err) == "Retry-After: 120\nHTTP 429: slow down"  # M1 message stays


@respx.mock
@pytest.mark.asyncio
async def test_retry_after_http_date_form(provider):
    http_date = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=30), usegmt=True)
    respx.post(_URL).mock(
        return_value=httpx.Response(
            429, headers={"retry-after": http_date}, json={"error": {"message": "slow down"}}
        )
    )
    with pytest.raises(RateLimitError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert exc.value.retry_after is not None
    assert 20.0 <= exc.value.retry_after <= 30.0


@respx.mock
@pytest.mark.asyncio
async def test_500_uses_x_request_id_fallback(provider):
    respx.post(_URL).mock(
        return_value=httpx.Response(
            500, headers={"x-request-id": "req_x"}, json={"error": {"message": "boom"}}
        )
    )
    with pytest.raises(ProviderError) as exc:
        await provider.chat(ChatRequest(messages=[Message.user("hi")]))
    assert exc.value.status_code == 500
    assert exc.value.request_id == "req_x"
    assert exc.value.retry_after is None
```
In `sdks/python/tests/test_retry.py`: add `from datetime import datetime, timedelta, timezone` and `from email.utils import format_datetime` above `import pytest` (approx line 1); change the import on approx line 9 to `from motosan_ai.retry import (RETRY_AFTER_CAP_SECS, _is_retryable, _parse_retry_after, parse_retry_after_header, with_retry)`; append at end of file (after approx line 216):
```python
class TestParseRetryAfterHeader:
    def test_integer_seconds(self):
        assert parse_retry_after_header("5") == 5.0

    def test_decimal_seconds(self):
        assert parse_retry_after_header("1.5") == 1.5

    def test_capped_at_60_seconds(self):
        assert parse_retry_after_header("120") == RETRY_AFTER_CAP_SECS

    def test_negative_seconds_returns_none(self):
        assert parse_retry_after_header("-5") is None

    def test_http_date_future(self):
        http_date = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=30), usegmt=True)
        value = parse_retry_after_header(http_date)
        assert value is not None
        assert 20.0 <= value <= 30.0

    def test_http_date_past_clamps_to_zero(self):
        http_date = format_datetime(datetime.now(timezone.utc) - timedelta(seconds=30), usegmt=True)
        assert parse_retry_after_header(http_date) == 0.0

    def test_none_empty_and_garbage(self):
        assert parse_retry_after_header(None) is None
        assert parse_retry_after_header("") is None
        assert parse_retry_after_header("soon") is None
```
- [ ] **Step 2 — Run and watch them fail.** From `sdks/python`: `uv run pytest tests/test_errors.py tests/test_retry.py -v`. Expected: `tests/test_retry.py` errors at collection with `ImportError: cannot import name 'RETRY_AFTER_CAP_SECS' from 'motosan_ai.retry'`; in `tests/test_errors.py`, `TestErrorAttributes` fails with `TypeError: RateLimitError() takes no keyword arguments` and the three respx tests fail with `AttributeError: 'RateLimitError' object has no attribute 'status_code'` (or the TypeError, pre-provider-change).
- [ ] **Step 3 — Rewrite `motosan_ai/error.py` base class (D2).** Current code (whole file, lines 1-31): eight bare classes, `class MotosanError(Exception): pass` plus seven `pass` subclasses. Replace the ENTIRE file with:
```python
from __future__ import annotations


class MotosanError(Exception):
    """Base error. HTTP metadata is populated by providers at raise time.

    ``retry_after`` is seconds, already clamped to [0, 60] when parsed from
    a Retry-After header. ``str(err)`` is exactly the message passed in --
    the M1 "HTTP {status}: ..." prefixes are unchanged.
    """

    def __init__(
        self,
        message: str = "",
        *,
        status_code: int | None = None,
        retry_after: float | None = None,
        request_id: str | None = None,
    ) -> None:
        super().__init__(message)
        self.status_code = status_code
        self.retry_after = retry_after
        self.request_id = request_id


class AuthError(MotosanError):
    pass


class RateLimitError(MotosanError):
    pass


class InvalidRequestError(MotosanError):
    pass


class ConfigError(MotosanError):
    pass


class ProviderError(MotosanError):
    pass


class NetworkError(MotosanError):
    pass


class StreamError(MotosanError):
    pass
```
- [ ] **Step 4 — Add the D8 header parser to `motosan_ai/retry.py`.** Add two imports so the from-import block (approx lines 15-18) reads: `from collections.abc import Awaitable, Callable` / `from datetime import datetime, timezone` / `from email.utils import parsedate_to_datetime` / `from typing import TypeVar` (keep `from motosan_ai.error import ...` last). Then, current code (approximate lines 24-29):
```python
DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 0.1  # seconds (aligned with Rust: 100ms)
DEFAULT_MAX_BACKOFF = 2.0  # seconds (aligned with Rust: 2000ms)

# 5xx status codes in error messages
_STATUS_5XX_RE = re.compile(r"\b5\d{2}\b")
```
Replace with:
```python
DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 0.1  # seconds (aligned with Rust: 100ms)
DEFAULT_MAX_BACKOFF = 2.0  # seconds (aligned with Rust: 2000ms)
RETRY_AFTER_CAP_SECS = 60.0

# 5xx status codes in error messages
_STATUS_5XX_RE = re.compile(r"\b5\d{2}\b")


def parse_retry_after_header(value: str | None) -> float | None:
    """Parse a ``Retry-After`` header into seconds, clamped to [0, RETRY_AFTER_CAP_SECS].

    Accepts both RFC 7231 forms: delay-seconds ("120", "1.5" — integer OR
    decimal, preserved) and HTTP-date ("Wed, 15 Jul 2026 08:00:00 GMT").
    A negative delay-seconds value is invalid and returns None; a past
    HTTP-date means "retry immediately" and clamps to 0.0. Returns None when
    absent, blank, or unparseable.
    """
    if value is None:
        return None
    text = value.strip()
    if not text:
        return None
    try:
        seconds = float(text)
    except ValueError:
        try:
            when = parsedate_to_datetime(text)
        except (TypeError, ValueError):
            return None
        if when is None:
            return None
        if when.tzinfo is None:
            when = when.replace(tzinfo=timezone.utc)
        # A past HTTP-date means "retry immediately" -> clamp up to 0.0.
        seconds = (when - datetime.now(timezone.utc)).total_seconds()
        return min(max(seconds, 0.0), RETRY_AFTER_CAP_SECS)
    # A negative delay-seconds value is invalid per RFC 7231 -> None.
    if seconds < 0:
        return None
    return min(seconds, RETRY_AFTER_CAP_SECS)
```
Do NOT delete `_STATUS_5XX_RE` or `_parse_retry_after` — the D9 task removes them.
- [ ] **Step 5 — Thread headers through the five identical mappers.** The five files `providers/anthropic.py` (approx 298-304), `providers/openai.py` (approx 156-162), `providers/gemini.py` (approx 204-210), `providers/gemini_code_assist.py` (approx 188-194), `providers/chatgpt_codex.py` (approx 326-332) each contain this byte-identical block — Current code:
```python
    @staticmethod
    def _map_http_error(status: int, message: str) -> Exception:
        if status == 401:
            return AuthError(message)
        if status == 429:
            return RateLimitError(message)
        return ProviderError(message)
```
In EACH of the five files, replace it with:
```python
    @staticmethod
    def _map_http_error(
        status: int, message: str, headers: httpx.Headers | None = None
    ) -> Exception:
        retry_after = (
            parse_retry_after_header(headers.get("retry-after")) if headers is not None else None
        )
        request_id = (
            (headers.get("request-id") or headers.get("x-request-id"))
            if headers is not None
            else None
        )
        if status == 401:
            return AuthError(
                message, status_code=status, retry_after=retry_after, request_id=request_id
            )
        if status == 429:
            return RateLimitError(
                message, status_code=status, retry_after=retry_after, request_id=request_id
            )
        return ProviderError(
            message, status_code=status, retry_after=retry_after, request_id=request_id
        )
```
Add the import `from motosan_ai.retry import parse_retry_after_header` on its own line immediately after: `from motosan_ai.provider_base import BaseProvider, ProviderCapabilities` in anthropic.py (approx line 11), gemini.py (approx line 11), chatgpt_codex.py (approx line 18); after `from motosan_ai.provider_base import ProviderCapabilities` in openai.py (approx line 10); after `from motosan_ai.providers.gemini import build_gemini_body` in gemini_code_assist.py (approx line 15). Then update every call site to pass headers (`resp` is in scope at all of them):
- anthropic.py approx 342 (chat) and approx 402 (stream): `raise self._map_http_error(resp.status_code, message)` -> `raise self._map_http_error(resp.status_code, message, resp.headers)`
- openai.py approx 181 and 229: same one-line change
- gemini.py approx 222 and 273: same one-line change
- chatgpt_codex.py approx 354: same one-line change
- gemini_code_assist.py approx 212: `raise self._map_http_error(resp.status_code, error_body.decode())` -> `raise self._map_http_error(resp.status_code, error_body.decode(), resp.headers)` (this message has no "HTTP {status}:" prefix in the baseline — keep it byte-identical)
- [ ] **Step 6 — Minimax and Ollama.** minimax.py — Current code (approximate lines 99-107):
```python
    @staticmethod
    def _raise_for_status(status_code: int, message: str) -> None:
        if status_code == 401:
            raise AuthError(message)
        if status_code == 429:
            raise RateLimitError(message)
        if status_code == 400:
            raise InvalidRequestError(message)
        raise ProviderError(message)
```
Replace with:
```python
    @staticmethod
    def _raise_for_status(
        status_code: int, message: str, headers: httpx.Headers | None = None
    ) -> None:
        retry_after = (
            parse_retry_after_header(headers.get("retry-after")) if headers is not None else None
        )
        request_id = (
            (headers.get("request-id") or headers.get("x-request-id"))
            if headers is not None
            else None
        )
        if status_code == 401:
            raise AuthError(
                message, status_code=status_code, retry_after=retry_after, request_id=request_id
            )
        if status_code == 429:
            raise RateLimitError(
                message, status_code=status_code, retry_after=retry_after, request_id=request_id
            )
        if status_code == 400:
            raise InvalidRequestError(
                message, status_code=status_code, retry_after=retry_after, request_id=request_id
            )
        raise ProviderError(
            message, status_code=status_code, retry_after=retry_after, request_id=request_id
        )
```
Add `from motosan_ai.retry import parse_retry_after_header` after `from motosan_ai.provider_base import ProviderCapabilities` (approx line 17). Update both call sites — approx line 155 (chat) and approx line 229 (stream): `self._raise_for_status(response.status_code, message)` -> `self._raise_for_status(response.status_code, message, response.headers)`. ollama.py — add the same `from motosan_ai.retry import parse_retry_after_header` import after `from motosan_ai.provider_base import ProviderCapabilities` (approx line 11). Current code (approximate lines 111-112, chat):
```python
        if resp.status_code >= 400:
            raise ProviderError(f"Ollama error {resp.status_code}: {resp.text}")
```
Replace with:
```python
        if resp.status_code >= 400:
            raise ProviderError(
                f"Ollama error {resp.status_code}: {resp.text}",
                status_code=resp.status_code,
                retry_after=parse_retry_after_header(resp.headers.get("retry-after")),
                request_id=resp.headers.get("request-id") or resp.headers.get("x-request-id"),
            )
```
Current code (approximate lines 159-163, stream):
```python
                if resp.status_code >= 400:
                    text = await resp.aread()
                    raise ProviderError(
                        f"Ollama error {resp.status_code}: {text.decode('utf-8', errors='ignore')}"
                    )
```
Replace with:
```python
                if resp.status_code >= 400:
                    text = await resp.aread()
                    raise ProviderError(
                        f"Ollama error {resp.status_code}: {text.decode('utf-8', errors='ignore')}",
                        status_code=resp.status_code,
                        retry_after=parse_retry_after_header(resp.headers.get("retry-after")),
                        request_id=resp.headers.get("request-id")
                        or resp.headers.get("x-request-id"),
                    )
```
Leave every `NetworkError`/`StreamError` raise site alone — no HTTP response is in scope there; the attributes correctly default to None.
- [ ] **Step 7 — Run to green, then the whole package.** From `sdks/python`: `uv run pytest tests/test_errors.py tests/test_retry.py -v` — expected: all 7 tests in test_errors.py and all 33 in test_retry.py pass. Then the full suite: `uv run pytest` — expected: 0 failures (message-format tests like `test_retry_after_header_is_preserved_in_error_message` in tests/test_gemini_errors.py and `TestProviderErrorHttpMessageFormat` in tests/test_retry.py still pass because every message string is byte-identical).
- [ ] **Step 8 — Format and lint.** From `sdks/python`: `uv run ruff format` (expected: files reformatted or left unchanged, no errors) and `uv run ruff check motosan_ai/` (expected: `All checks passed!`; tests/ is not linted).
- [ ] **Step 9 — Commit.**
```bash
git add sdks/python
git commit -m "feat(python): structured HTTP metadata on errors (status_code, retry_after, request_id)" -m "MotosanError gains D2 keyword attributes; every provider HTTP raise site
populates them from the response. Retry-After parses integer-seconds and
RFC 7231 HTTP-date forms, clamped to 60s (RETRY_AFTER_CAP_SECS). Error
message strings are byte-identical to M1.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 8: Python RetryPolicy: full-jitter backoff and attribute-based retry classification

> **Ordering (M2 canon).** This is **Task 8** of the M2 Python sequence. It runs **AFTER Task 7 (py-error-attrs)** and **BEFORE Task 9 (py-client-thread)**.
> - Task 7 has **already** added `parse_retry_after_header(value: str | None) -> float | None` and `RETRY_AFTER_CAP_SECS = 60.0` (plus their `from datetime import datetime, timezone` / `from email.utils import parsedate_to_datetime` imports) to `retry.py`, and pointed all seven provider modules (`anthropic.py`, `openai.py`, `gemini.py`, `gemini_code_assist.py`, `chatgpt_codex.py`, `minimax.py`, `ollama.py`) at `from motosan_ai.retry import parse_retry_after_header` **at module level**. Task 7 also added a `TestParseRetryAfterHeader` class (7 tests) to `tests/test_retry.py`. This task **preserves all of that verbatim** — wiping `parse_retry_after_header` breaks every provider import (and therefore `import motosan_ai` itself).
> - Task 9 imports **exactly** `from motosan_ai.retry import RetryPolicy, RetryEvent, compute_delay, retry_cause, with_retry`. Produce those names exactly (note: public `retry_cause`, not `_retry_cause`).
> - Every line number below is **approximate** — ground each edit against the real post-Task-7 file, not the pristine baseline.

**Execute AFTER the py-error-attrs task (D2).** Tests below construct errors with `status_code=`/`retry_after=` kwargs and `client.py` reads `e.retry_after`; none of that exists before py-error-attrs lands.

**Files:**
- Modify: `sdks/python/motosan_ai/retry.py` (rewrite of the **post-py-error-attrs** file — deletes the old message-based helpers but **preserves** `parse_retry_after_header` + `RETRY_AFTER_CAP_SECS` and their `datetime`/`email.utils` imports)
- Modify: `sdks/python/motosan_ai/client.py` (stream retry block, approx lines 390–413)
- Modify: `sdks/python/tests/test_gemini_errors.py` (approx lines 7, 64)
- Modify: `sdks/python/tests/test_anthropic_validation.py` (approx lines 8, 65)
- Modify: `sdks/python/tests/test_client_stream_with.py` (approx line 116)
- Test: `sdks/python/tests/test_retry.py` (rewrite of the **post-py-error-attrs** file — **keeps** Task 7's `TestParseRetryAfterHeader` class)

**Interfaces:**
- Consumes (from py-error-attrs, D2): `MotosanError.__init__(self, message: str = "", *, status_code: int | None = None, retry_after: float | None = None, request_id: str | None = None)`; providers pass kwargs at raise sites; `retry_after` is already capped at 60 s at raise time. Also consumes the Task-7 symbols already present in `retry.py`: `parse_retry_after_header` and `RETRY_AFTER_CAP_SECS` (preserved, not re-created).
- Produces (consumed by the client-policy-threading task / Task 9 and the specs/retry.md task), all in `motosan_ai/retry.py`:
  - `@dataclass RetryPolicy(max_retries: int = 3, base_delay: float = 0.1, max_delay: float = 2.0, jitter: bool = True, respect_retry_after: bool = True, on_retry: Callable[[RetryEvent], None] | None = None)`
  - `@dataclass RetryEvent(attempt: int, delay: float, cause: str)` — `attempt` is 1-based (1 = first retry); `cause` is `"status:<code>"` or `"network:<message>"`
  - `compute_delay(policy: RetryPolicy, attempt: int, retry_after: float | None = None, rng: Callable[[], float] = random.random) -> float`
  - `retry_cause(error: Exception) -> str` — **public** (Task 9 imports it); returns `"network:<message>"` / `"status:<code>"` / falls back to the exception class name
  - `async def with_retry(fn: Callable[[], Awaitable[T]], max_retries: int = 3, initial_backoff: float = 0.1, max_backoff: float = 2.0, *, policy: RetryPolicy | None = None, rng: Callable[[], float] = random.random) -> T` — **ADDITIVE/backward-compatible signature.** The baseline positional params (`max_retries`, `initial_backoff`, `max_backoff`) keep their order and positions, so legacy positional callers like `with_retry(fn, 3, 0.1, 2.0)` still work; `policy` and `rng` are keyword-only. When `policy is None`, an equivalent `RetryPolicy` is built from the legacy kwargs; when `policy` is passed (`with_retry(fn, policy=my_policy)`), the legacy kwargs are ignored.
  - `_is_retryable(error: Exception) -> bool` (attribute-based, D9)
- Preserved verbatim from py-error-attrs (do NOT re-derive, do NOT wipe): `parse_retry_after_header(value: str | None) -> float | None` and `RETRY_AFTER_CAP_SECS = 60.0` (+ the `from datetime import datetime, timezone` / `from email.utils import parsedate_to_datetime` imports).
- DELETED (old message-based helpers still present after Task 7): `_STATUS_5XX_RE`, `_parse_retry_after`, and the message-based `_is_retryable` — all importers are updated in this task.

- [ ] **Step 1 — Rewrite the test file (keeping Task 7's header-parser tests).** Task 7 already replaced the pristine baseline `tests/test_retry.py`; it now pins attribute-based expectations **plus** a `TestParseRetryAfterHeader` class (7 D8 tests). Replace the file's contents with the following, which **retains that `TestParseRetryAfterHeader` class verbatim from Task 7** (shown here **approximate** — copy the class body forward exactly as py-error-attrs landed it, including its `test_negative_seconds_returns_none` case) and adds the new RetryPolicy/compute_delay/with_retry coverage:

```python
import random
from datetime import datetime, timedelta, timezone
from email.utils import format_datetime

import pytest

from motosan_ai.error import AuthError, NetworkError, ProviderError, RateLimitError
from motosan_ai.retry import (
    RETRY_AFTER_CAP_SECS,
    RetryEvent,
    RetryPolicy,
    _is_retryable,
    compute_delay,
    parse_retry_after_header,
    with_retry,
)


# --- preserved from py-error-attrs (Task 7); copy forward verbatim (approx) ---
class TestParseRetryAfterHeader:
    def test_integer_seconds(self):
        assert parse_retry_after_header("5") == pytest.approx(5.0)

    def test_none_and_blank_return_none(self):
        assert parse_retry_after_header(None) is None
        assert parse_retry_after_header("   ") is None

    def test_value_over_cap_is_clamped(self):
        assert parse_retry_after_header("120") == pytest.approx(RETRY_AFTER_CAP_SECS)

    def test_negative_seconds_returns_none(self):
        assert parse_retry_after_header("-5") is None

    def test_http_date_in_future(self):
        when = format_datetime(datetime.now(timezone.utc) + timedelta(seconds=30))
        assert parse_retry_after_header(when) == pytest.approx(30.0, abs=2.0)

    def test_http_date_in_past_clamps_to_zero(self):
        when = format_datetime(datetime.now(timezone.utc) - timedelta(seconds=30))
        assert parse_retry_after_header(when) == pytest.approx(0.0)

    def test_unparseable_returns_none(self):
        assert parse_retry_after_header("not-a-date") is None


class TestIsRetryable:
    def test_retryable_classes_and_statuses(self):
        assert _is_retryable(RateLimitError("slow down", status_code=429)) is True
        assert _is_retryable(NetworkError("connection reset")) is True
        assert _is_retryable(ProviderError("HTTP 500: boom", status_code=500)) is True
        assert _is_retryable(ProviderError("HTTP 408: timeout", status_code=408)) is True
        assert _is_retryable(ProviderError("HTTP 409: conflict", status_code=409)) is True

    def test_not_retryable(self):
        assert _is_retryable(ProviderError("HTTP 400: bad request", status_code=400)) is False
        assert _is_retryable(AuthError("unauthorized", status_code=401)) is False
        assert _is_retryable(ValueError("bad")) is False

    def test_provider_error_without_status_not_retryable(self):
        # Attribute-based classification: a 5xx-looking MESSAGE alone must not retry.
        assert _is_retryable(ProviderError("Error code: 500 - server error")) is False


class TestComputeDelay:
    def test_full_jitter_scales_rng_against_exponential_ceiling(self):
        policy = RetryPolicy()  # base 0.1, cap 2.0, jitter on
        assert compute_delay(policy, 1, None, rng=lambda: 0.5) == pytest.approx(0.05)
        assert compute_delay(policy, 2, None, rng=lambda: 0.5) == pytest.approx(0.1)
        assert compute_delay(policy, 6, None, rng=lambda: 1.0) == pytest.approx(2.0)  # capped
        assert compute_delay(policy, 3, None, rng=lambda: 0.0) == pytest.approx(0.0)

    def test_seeded_rng_stays_within_bounds(self):
        policy = RetryPolicy()
        rng = random.Random(42).random
        for attempt in (1, 2, 3, 6):
            ceiling = min(0.1 * 2 ** (attempt - 1), 2.0)
            assert 0.0 <= compute_delay(policy, attempt, None, rng=rng) <= ceiling

    def test_no_jitter_is_pure_exponential(self):
        policy = RetryPolicy(jitter=False)
        assert compute_delay(policy, 1, None) == pytest.approx(0.1)
        assert compute_delay(policy, 3, None) == pytest.approx(0.4)

    def test_retry_after_verbatim_capped_at_60_not_max_delay(self):
        policy = RetryPolicy()  # max_delay=2.0 must NOT clamp retry-after
        assert compute_delay(policy, 1, 7.0, rng=lambda: 0.5) == pytest.approx(7.0)
        assert compute_delay(policy, 1, 120.0) == pytest.approx(60.0)

    def test_retry_after_ignored_when_respect_disabled(self):
        policy = RetryPolicy(respect_retry_after=False, jitter=False)
        assert compute_delay(policy, 1, 7.0) == pytest.approx(0.1)


class TestWithRetry:
    @pytest.mark.asyncio
    async def test_succeeds_first_try(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            return "ok"

        assert await with_retry(fn, max_retries=3) == "ok"
        assert calls == 1

    @pytest.mark.asyncio
    async def test_retries_on_rate_limit_with_policy(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 3:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        policy = RetryPolicy(max_retries=3, base_delay=0.001)
        assert await with_retry(fn, policy=policy) == "ok"
        assert calls == 3

    @pytest.mark.asyncio
    async def test_legacy_positional_args_still_work(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls < 2:
                raise NetworkError("connection reset")
            return "ok"

        # Backward compat: old positional call `with_retry(fn, max_retries,
        # initial_backoff, max_backoff)` must keep working (policy is a new
        # keyword-only param appended AFTER the legacy positional params).
        assert await with_retry(fn, 3, 0.001, 2.0) == "ok"
        assert calls == 2

    @pytest.mark.asyncio
    async def test_does_not_retry_provider_error_without_status(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ProviderError("Error code: 500 - server error")  # no status_code

        with pytest.raises(ProviderError):
            await with_retry(fn, max_retries=3, initial_backoff=0.001)
        assert calls == 1

    @pytest.mark.asyncio
    async def test_exhausts_retries(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise ProviderError("HTTP 503: overloaded", status_code=503)

        with pytest.raises(ProviderError):
            await with_retry(fn, policy=RetryPolicy(max_retries=2, base_delay=0.001))
        assert calls == 3

    @pytest.mark.asyncio
    async def test_uses_retry_after_attribute_verbatim(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise RateLimitError("slow down", status_code=429, retry_after=0.001)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=2, base_delay=0.05, on_retry=events.append)
        assert await with_retry(fn, policy=policy) == "ok"
        # retry-after attribute (0.001 s) used verbatim, NOT the 0.05 s backoff
        assert events[0].delay == pytest.approx(0.001)

    @pytest.mark.asyncio
    async def test_on_retry_fires_before_each_sleep(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise NetworkError("timeout")
            if calls == 2:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=3, base_delay=0.001, on_retry=events.append)
        assert await with_retry(fn, policy=policy) == "ok"
        assert [e.attempt for e in events] == [1, 2]
        assert events[0].cause == "network:timeout"
        assert events[1].cause == "status:429"
        assert all(e.delay >= 0.0 for e in events)

    @pytest.mark.asyncio
    async def test_injected_rng_drives_jitter(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            if calls == 1:
                raise RateLimitError("slow down", status_code=429)
            return "ok"

        events: list[RetryEvent] = []
        policy = RetryPolicy(max_retries=1, base_delay=0.002, on_retry=events.append)
        assert await with_retry(fn, policy=policy, rng=lambda: 0.5) == "ok"
        assert events[0].delay == pytest.approx(0.001)  # 0.5 * (0.002 * 2**0)

    @pytest.mark.asyncio
    async def test_max_retries_zero_does_not_retry(self):
        calls = 0

        async def fn():
            nonlocal calls
            calls += 1
            raise RateLimitError("slow down", status_code=429)

        with pytest.raises(RateLimitError):
            await with_retry(fn, max_retries=0)
        assert calls == 1
```

- [ ] **Step 2 — Run and watch it fail.** From `sdks/python`: `uv run pytest tests/test_retry.py -v` → collection error: `ImportError: cannot import name 'RetryEvent' from 'motosan_ai.retry'` (the first of the new names in the import block's declaration order). The **existing** symbols all resolve fine — `RETRY_AFTER_CAP_SECS` and `parse_retry_after_header` from Task 7, and `_is_retryable` and `with_retry` from the baseline `retry.py` module — so the failure is solely the **NEW** names this task introduces: `RetryPolicy`, `RetryEvent`, `compute_delay`, and `retry_cause`. (`_is_retryable` and `with_retry` already exist; they are being *rewritten* here, not newly created.)

- [ ] **Step 3 — Implement (preserving Task 7's header parser).** Rewrite `sdks/python/motosan_ai/retry.py`. After py-error-attrs the file holds the **old** message-based helpers (`_STATUS_5XX_RE`, `_parse_retry_after`, the message-based `_is_retryable`, and the old 3-kwarg `with_retry`) **plus** py-error-attrs' additions (`parse_retry_after_header`, `RETRY_AFTER_CAP_SECS`, and the `from datetime import datetime, timezone` / `from email.utils import parsedate_to_datetime` imports). This rewrite **deletes the old message-based helpers** but **carries `parse_retry_after_header` + `RETRY_AFTER_CAP_SECS` (and their two imports) forward verbatim** — every provider module imports `parse_retry_after_header` at module level, so wiping it breaks all imports. Replace the file with:

```python
"""Retry engine with full-jitter exponential backoff. Normative contract: specs/retry.md.

- Classification is attribute-based (error.status_code), never message-based:
  RateLimitError, NetworkError, and ProviderError with status 408/409/5xx retry.
- Backoff: full jitter — uniform(0, min(base_delay * 2**(attempt-1), max_delay)).
- error.retry_after (capped at raise time) is used verbatim: no jitter,
  independent of max_delay, hard-capped at RETRY_AFTER_CAP_SECS.
- parse_retry_after_header / RETRY_AFTER_CAP_SECS are preserved from
  py-error-attrs (all seven provider modules import parse_retry_after_header).
"""

from __future__ import annotations

import asyncio
import logging
import random
from collections.abc import Awaitable, Callable
from dataclasses import dataclass
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from typing import TypeVar

from motosan_ai.error import MotosanError, NetworkError, ProviderError, RateLimitError

logger = logging.getLogger(__name__)

T = TypeVar("T")

DEFAULT_MAX_RETRIES = 3
DEFAULT_INITIAL_BACKOFF = 0.1  # seconds (aligned with Rust: 100ms)
DEFAULT_MAX_BACKOFF = 2.0  # seconds (aligned with Rust: 2000ms)
RETRY_AFTER_CAP_SECS = 60.0  # specs/retry.md: Retry-After hard cap


# --- preserved verbatim from py-error-attrs (Task 7); DO NOT wipe (approx body) ---
# Seven provider modules import parse_retry_after_header at module level. Copy the
# function forward EXACTLY as py-error-attrs landed it — delta-seconds parsed with
# float() so decimals are preserved ("1.5" -> 1.5), a negative delta-seconds value
# is invalid -> None, HTTP-date via email.utils.parsedate_to_datetime with a past
# date -> 0.0, all clamped to [0, RETRY_AFTER_CAP_SECS]. Do NOT re-introduce an
# int-only float(int(value)) variant — it breaks decimal Retry-After headers.
def parse_retry_after_header(value: str | None) -> float | None:
    """Parse a ``Retry-After`` header into seconds, clamped to [0, RETRY_AFTER_CAP_SECS].

    Accepts both RFC 7231 forms: delay-seconds ("120", "1.5" — integer OR
    decimal, preserved) and HTTP-date ("Wed, 15 Jul 2026 08:00:00 GMT").
    A negative delay-seconds value is invalid and returns None; a past
    HTTP-date means "retry immediately" and clamps to 0.0. Returns None when
    absent, blank, or unparseable.
    """
    if value is None:
        return None
    text = value.strip()
    if not text:
        return None
    try:
        seconds = float(text)
    except ValueError:
        try:
            when = parsedate_to_datetime(text)
        except (TypeError, ValueError):
            return None
        if when is None:
            return None
        if when.tzinfo is None:
            when = when.replace(tzinfo=timezone.utc)
        # A past HTTP-date means "retry immediately" -> clamp up to 0.0.
        seconds = (when - datetime.now(timezone.utc)).total_seconds()
        return min(max(seconds, 0.0), RETRY_AFTER_CAP_SECS)
    # A negative delay-seconds value is invalid per RFC 7231 -> None.
    if seconds < 0:
        return None
    return min(seconds, RETRY_AFTER_CAP_SECS)


# --- end preserved block ---


@dataclass
class RetryEvent:
    """Passed to RetryPolicy.on_retry before each retry sleep."""

    attempt: int  # 1-based retry number (1 = first retry)
    delay: float  # seconds the engine will sleep before this retry
    cause: str  # "status:<code>" or "network:<message>"


@dataclass
class RetryPolicy:
    """Cross-SDK retry policy (specs/retry.md)."""

    max_retries: int = DEFAULT_MAX_RETRIES
    base_delay: float = DEFAULT_INITIAL_BACKOFF
    max_delay: float = DEFAULT_MAX_BACKOFF
    jitter: bool = True
    respect_retry_after: bool = True
    on_retry: Callable[[RetryEvent], None] | None = None


def _is_retryable(error: Exception) -> bool:
    """Attribute-based classification: 408/409/429/5xx and network errors."""
    if isinstance(error, RateLimitError):
        return True
    if isinstance(error, NetworkError):
        return True
    if isinstance(error, ProviderError):
        return error.status_code in {408, 409} or (error.status_code or 0) >= 500
    return False


def retry_cause(error: Exception) -> str:
    """Render a RetryEvent.cause tag: 'network:<message>' or 'status:<code>'."""
    if isinstance(error, NetworkError):
        return f"network:{error}"
    status = getattr(error, "status_code", None)
    if status is not None:
        return f"status:{status}"
    return type(error).__name__


def compute_delay(
    policy: RetryPolicy,
    attempt: int,
    retry_after: float | None = None,
    rng: Callable[[], float] = random.random,
) -> float:
    """Seconds to sleep before retry number ``attempt`` (1-based).

    A Retry-After value is used verbatim (no jitter, independent of
    max_delay) when respect_retry_after is enabled; otherwise full jitter:
    uniform(0, min(base_delay * 2**(attempt-1), max_delay)).
    """
    if policy.respect_retry_after and retry_after is not None:
        return min(retry_after, RETRY_AFTER_CAP_SECS)
    exp_delay = min(policy.base_delay * (2 ** (attempt - 1)), policy.max_delay)
    return rng() * exp_delay if policy.jitter else exp_delay


async def with_retry(
    fn: Callable[[], Awaitable[T]],
    max_retries: int = DEFAULT_MAX_RETRIES,
    initial_backoff: float = DEFAULT_INITIAL_BACKOFF,
    max_backoff: float = DEFAULT_MAX_BACKOFF,
    *,
    policy: RetryPolicy | None = None,
    rng: Callable[[], float] = random.random,
) -> T:
    """Execute fn, retrying transient errors per ``policy``.

    Backward-compatible: the legacy positional params
    (max_retries/initial_backoff/max_backoff) keep their baseline order and
    positions, so old callers like ``with_retry(fn, 3, 0.1, 2.0)`` and
    ``with_retry(fn, max_retries=3)`` keep working. ``policy`` and ``rng``
    are keyword-only. The legacy params are honored only when ``policy`` is
    None; they build an equivalent RetryPolicy. Passing ``policy=...``
    (e.g. ``with_retry(fn, policy=my_policy)``) ignores the legacy params.
    """
    if policy is None:
        policy = RetryPolicy(
            max_retries=max_retries,
            base_delay=initial_backoff,
            max_delay=max_backoff,
        )
    last_error: Exception | None = None
    for attempt in range(policy.max_retries + 1):
        try:
            return await fn()
        except Exception as e:
            if not _is_retryable(e):
                raise
            last_error = e
            if attempt >= policy.max_retries:
                break
            retry_number = attempt + 1
            retry_after = e.retry_after if isinstance(e, MotosanError) else None
            wait = compute_delay(policy, retry_number, retry_after, rng)
            if policy.on_retry is not None:
                policy.on_retry(
                    RetryEvent(attempt=retry_number, delay=wait, cause=retry_cause(e))
                )
            logger.warning(
                "Retryable error (attempt %d/%d), retrying in %.2fs: %s",
                retry_number,
                policy.max_retries,
                wait,
                type(e).__name__,
            )
            await asyncio.sleep(wait)
    raise last_error  # type: ignore[misc]
```

- [ ] **Step 3b — Fix `client.py` (it imports the deleted `_parse_retry_after`).** Two hunks in `sdks/python/motosan_ai/client.py`. Hunk 1 — current code (approximate lines 390–396):

```python
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import (
                    DEFAULT_INITIAL_BACKOFF,
                    DEFAULT_MAX_BACKOFF,
                    _is_retryable,
                    _parse_retry_after,
                )
```

Replace with:

```python
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import RetryPolicy, _is_retryable, compute_delay
```

Hunk 2 — current code (approximate lines 407–413):

```python
                retry_after = _parse_retry_after(str(e))
                wait = min(
                    retry_after
                    if retry_after is not None
                    else DEFAULT_INITIAL_BACKOFF * (2**attempt),
                    DEFAULT_MAX_BACKOFF,
                )
```

Replace with:

```python
                wait = compute_delay(
                    RetryPolicy(max_retries=self._max_retries),
                    attempt + 1,
                    e.retry_after,
                )
```

(The F1 comment, `yielded` guard, `logger.warning`, and `asyncio.sleep(wait)` around these hunks stay untouched. Full policy threading through `Client` is Task 9; this is the minimal D4-conformant swap. Note `e.retry_after` is safe here because the `except` already narrows to `MotosanError` subclasses.)

- [ ] **Step 3c — Fix the two provider tests that import `_parse_retry_after`.** (If py-error-attrs already rewrote these assertions to attribute form, skip the file it fixed.) In `sdks/python/tests/test_gemini_errors.py`: delete the import line `from motosan_ai.retry import _parse_retry_after` (approx line 7) and change the assertion (approx line 64) from `assert _parse_retry_after(str(exc.value)) == 1.5` to `assert exc.value.retry_after == 1.5`. In `sdks/python/tests/test_anthropic_validation.py`: delete `from motosan_ai.retry import _parse_retry_after` (approx line 8) and change the assertion (approx line 65) from `assert _parse_retry_after(str(exc.value)) == 2.0` to `assert exc.value.retry_after == 2.0`.

- [ ] **Step 3d — Keep the F1 mid-stream guard test meaningful.** In `sdks/python/tests/test_client_stream_with.py` (approx line 116), change `raise ProviderError("503 server error")  # retryable BY CLASS, but mid-stream` to `raise ProviderError("HTTP 503: server error", status_code=503)  # retryable, but mid-stream` — without `status_code` the error is no longer retryable by class and the test would pass without exercising the guard.

- [ ] **Step 4 — Run to green.** From `sdks/python`:
  - `uv run pytest tests/test_retry.py -v` → `24 passed` (17 new RetryPolicy/compute_delay/with_retry cases + 7 preserved `TestParseRetryAfterHeader` cases from py-error-attrs).
  - Touched neighbors: `uv run pytest tests/test_gemini_errors.py tests/test_anthropic_validation.py tests/test_client_stream_with.py tests/test_client_integration.py -v` → all pass.
  - Verify the M1 non-JSON-5xx respx end-to-end retry tests pass UNCHANGED (they build errors via providers, which attach `status_code=502` since py-error-attrs): `uv run pytest tests/test_openai.py tests/test_minimax.py tests/test_chatgpt_codex_http.py -k "502" -v` → 3 passed.
  - Full package suite: `uv run pytest` → all pass (integration tests auto-skip without `ANTHROPIC_API_KEY`). This is the real check that `parse_retry_after_header` survived — every provider imports it at module level, so a wiped function would surface here as a collection-time `ImportError`.

- [ ] **Step 5 — Format and lint.** From `sdks/python`: `uv run ruff format` then `uv run ruff check motosan_ai/` → `All checks passed!` (tests/ is not linted).

- [ ] **Step 6 — Commit.**

```bash
git add sdks/python
git commit -m "feat(python): RetryPolicy with full jitter and attribute-based retry classification

Adds RetryPolicy/RetryEvent dataclasses, injectable-rng full-jitter backoff,
verbatim capped Retry-After from error attributes, retry_cause, and on_retry
observability. Deletes message-scraping _parse_retry_after/_STATUS_5XX_RE and
updates all importers (client.py stream path now uses shared compute_delay).
Preserves parse_retry_after_header/RETRY_AFTER_CAP_SECS from py-error-attrs.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

### Task 9: Thread RetryPolicy through Python Client chat and stream paths

**Execute AFTER Task 8 (py-retrypolicy)** — which itself executes after Task 7 (py-error-attrs, D2). This task consumes py-retrypolicy's public retry surface (`RetryPolicy`, `RetryEvent`, `compute_delay`, `retry_cause`, `with_retry`), so it MUST run only after Task 8 has landed. Work from `sdks/python/`.

**Files:**
- Modify: `sdks/python/motosan_ai/client.py` — imports (approx lines 11-22), `__init__` (approx 68-79), classmethod constructors (approx 149-288), `chat_with` (approx 331-338), `stream_with` (approx 369-422). All line refs are APPROXIMATE and taken from the pre-py-retrypolicy baseline; **re-locate every edit site by symbol name** — Task 7, Task 8, and other M2 tasks shift line numbers. In particular, Task 8/py-retrypolicy has ALREADY rewritten the `stream_with` except block (its lazy retry import and its delay math), so Step 3(f) below quotes that post-Task-8 state, not the baseline.
- Modify (guarded cleanup, only if still present): `sdks/python/motosan_ai/retry.py`, `sdks/python/tests/test_retry.py`
- Create: `sdks/python/tests/test_client_retry_policy.py`
- Test (must stay green, no edits): `sdks/python/tests/test_client_stream_with.py` (esp. `test_stream_with_does_not_retry_stream_error`, `test_stream_with_does_not_retry_provider_error_after_yield` — the retry-only-before-first-event guard), `sdks/python/tests/test_client_integration.py` (legacy `max_retries` kwarg behavior)

**Interfaces:**
- Consumes from `motosan_ai/retry.py` (produced by Task 8 / py-retrypolicy). The exact importable surface is `from motosan_ai.retry import RetryPolicy, RetryEvent, compute_delay, retry_cause, with_retry` (plus the private attribute-based `_is_retryable`). These are the ONLY names this task may import from `retry.py` — do NOT invent a differently-named cause helper; the cause helper is the PUBLIC `retry_cause`, never `_retry_cause`. Signatures:
  - `@dataclass RetryPolicy(max_retries: int = 3, base_delay: float = 0.1, max_delay: float = 2.0, jitter: bool = True, respect_retry_after: bool = True, on_retry: Callable[[RetryEvent], None] | None = None)`
  - `@dataclass RetryEvent(attempt: int, delay: float, cause: str)` — `attempt` is 1-based (1 = first retry); `cause` is `"status:<code>"` or `"network:<message>"`.
  - `compute_delay(policy: RetryPolicy, attempt: int, retry_after: float | None = None, rng: Callable[[], float] = random.random) -> float` — `attempt` 1-based; `retry_after` used verbatim, capped at 60s, when `respect_retry_after`; else full-jitter exp backoff per D4.
  - `retry_cause(error: Exception) -> str` — returns `"network:<message>"` for `NetworkError`, else `"status:<code>"` when a `status_code` attribute is set, else the exception class name.
  - `with_retry(fn, policy: RetryPolicy | None = None, *, max_retries=..., initial_backoff=..., max_backoff=..., rng: Callable[[], float] = random.random)` — fires `policy.on_retry(RetryEvent(...))` before each sleep; legacy kwargs honored only when `policy is None`.
  - `_is_retryable(error: Exception) -> bool` — attribute-based classification per D9.
- Consumes from `motosan_ai/error.py` (produced by py-errors/D2): `MotosanError.status_code`, `.retry_after`, `.request_id` attributes; kwargs form `ProviderError(msg, status_code=503)`.
- Produces: `Client.__init__(..., retry_policy: RetryPolicy | None = None)` storing `self._retry_policy: RetryPolicy` (None -> `RetryPolicy(max_retries=max_retries)`); all classmethod constructors (`anthropic`, `openai`, `gemini`, `gemini_code_assist`, `chatgpt_codex`, `minimax`, `codex_cli`, `gemini_cli`, `ollama`) accept `retry_policy: RetryPolicy | None = None`. Additive keyword only — LlmClient Protocol unbroken.

- [ ] **Step 1 — Write the failing test.** Create `sdks/python/tests/test_client_retry_policy.py`:
```python
from __future__ import annotations

import json

import httpx
import pytest
import respx

from motosan_ai import Client, Provider
from motosan_ai.error import ProviderError
from motosan_ai.retry import RetryEvent, RetryPolicy
from motosan_ai.types import ChatRequest, Message, StreamEvent

_OK_JSON = {
    "model": "claude-sonnet-4-6",
    "stop_reason": "end_turn",
    "usage": {"input_tokens": 1, "output_tokens": 1},
    "content": [{"type": "text", "text": "ok"}],
}

_FAST = {"max_retries": 1, "base_delay": 0.001, "max_delay": 0.002}


def _sse_lines(*events: dict) -> str:
    return "\n".join(f"data: {json.dumps(e)}" for e in events) + "\n"


def test_legacy_max_retries_builds_default_policy(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    client = Client(provider=Provider.anthropic, max_retries=2)
    assert client._retry_policy.max_retries == 2


def test_explicit_retry_policy_wins_over_max_retries(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    client = Client(
        provider=Provider.anthropic, max_retries=5, retry_policy=RetryPolicy(max_retries=0)
    )
    assert client._retry_policy.max_retries == 0


def test_classmethod_constructor_accepts_retry_policy(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    client = Client.anthropic(retry_policy=RetryPolicy(max_retries=7))
    assert client._retry_policy.max_retries == 7


@respx.mock
@pytest.mark.asyncio
async def test_chat_retry_policy_retries_once_then_succeeds(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(500, json={"error": {"message": "overloaded"}}),
            httpx.Response(200, json=_OK_JSON),
        ]
    )
    client = Client(provider=Provider.anthropic, retry_policy=RetryPolicy(**_FAST))
    resp = await client.chat([Message.user("hi")])
    assert resp.content == "ok"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_on_retry_fires_through_client_chat_path(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(429, json={"error": {"message": "slow down"}}),
            httpx.Response(200, json=_OK_JSON),
        ]
    )
    events: list[RetryEvent] = []
    policy = RetryPolicy(on_retry=events.append, **_FAST)
    client = Client(provider=Provider.anthropic, retry_policy=policy)
    resp = await client.chat([Message.user("hi")])
    assert resp.content == "ok"
    assert len(events) == 1
    assert events[0].attempt == 1
    assert events[0].cause == "status:429"
    assert 0.0 <= events[0].delay <= 0.002


@respx.mock
@pytest.mark.asyncio
async def test_stream_retry_policy_retries_before_first_event(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")
    sse = _sse_lines(
        {"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "hi"}},
        {"type": "message_stop"},
    )
    route = respx.post("https://api.anthropic.com/v1/messages").mock(
        side_effect=[
            httpx.Response(500, json={"error": {"message": "overloaded"}}),
            httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"}),
        ]
    )
    observed: list[RetryEvent] = []
    policy = RetryPolicy(on_retry=observed.append, **_FAST)
    client = Client(provider=Provider.anthropic, retry_policy=policy)
    out = [ev async for ev in client.stream([Message.user("hi")])]
    assert any(ev.content == "hi" for ev in out)
    assert route.call_count == 2
    assert len(observed) == 1
    assert observed[0].attempt == 1
    assert observed[0].cause == "status:500"


class _CountingProvider:
    """Provider stub that records how many times stream() was invoked."""

    def __init__(self, make_gen):
        self._make_gen = make_gen
        self.stream_calls = 0

    async def stream(self, request):
        self.stream_calls += 1
        async for event in self._make_gen():
            yield event


@pytest.mark.asyncio
async def test_stream_policy_refuses_retry_after_first_event(monkeypatch):
    monkeypatch.setenv("ANTHROPIC_API_KEY", "k")

    async def gen():
        yield StreamEvent(content="partial", done=False)
        raise ProviderError("HTTP 503: overloaded", status_code=503)  # retryable BY CLASS

    client = Client(
        provider=Provider.anthropic,
        retry_policy=RetryPolicy(max_retries=3, base_delay=0.001),
    )
    provider = _CountingProvider(gen)
    client._provider = provider  # documented injection seam
    req = ChatRequest(messages=[Message.user("hi")])
    with pytest.raises(ProviderError):
        async for _ in client.stream_with(req):
            pass
    assert provider.stream_calls == 1  # mid-stream retryable error is NOT replayed
```

- [ ] **Step 2 — Run and confirm failure.** From `sdks/python`: `uv run pytest tests/test_client_retry_policy.py -v`. Expect failures/errors like `TypeError: Client.__init__() got an unexpected keyword argument 'retry_policy'` and `TypeError: Client.anthropic() got an unexpected keyword argument 'retry_policy'`.

- [ ] **Step 3 — Implement in `sdks/python/motosan_ai/client.py`.**
  (a) Imports. Current code (approximate line 11): `from motosan_ai.error import ConfigError, NetworkError, ProviderError, RateLimitError` — Replace with: `from motosan_ai.error import ConfigError, MotosanError, NetworkError, ProviderError, RateLimitError`. Then directly below the `from motosan_ai.providers import (...)` block (approximate line 21) and above `from motosan_ai.think_stripper import ThinkStripper`, add: `from motosan_ai.retry import RetryPolicy` (no cycle: retry.py imports only from error.py).
  (b) Constructor. Current code (approximate lines 74-79):
```python
        max_retries: int = 3,
    ) -> None:
        provider_value = Provider(provider)
        self.provider = provider_value
        self.model = model
        self._max_retries = max_retries
```
  Replace with:
```python
        max_retries: int = 3,
        retry_policy: RetryPolicy | None = None,
    ) -> None:
        provider_value = Provider(provider)
        self.provider = provider_value
        self.model = model
        self._retry_policy = (
            retry_policy if retry_policy is not None else RetryPolicy(max_retries=max_retries)
        )
        # Legacy mirror; retry decisions read self._retry_policy only.
        self._max_retries = self._retry_policy.max_retries
```
  (c) Classmethod constructors. Current code (approximate lines 149-158):
```python
    @classmethod
    def anthropic(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
    ) -> Client:
        return cls(
            provider=Provider.anthropic, api_key=api_key, model=model, max_retries=max_retries
        )
```
  Replace with:
```python
    @classmethod
    def anthropic(
        cls,
        api_key: str | None = None,
        model: str | None = None,
        max_retries: int = 3,
        retry_policy: RetryPolicy | None = None,
    ) -> Client:
        return cls(
            provider=Provider.anthropic,
            api_key=api_key,
            model=model,
            max_retries=max_retries,
            retry_policy=retry_policy,
        )
```
  Apply the identical two-line mechanical edit — append `retry_policy: RetryPolicy | None = None,` as the final parameter, and `retry_policy=retry_policy,` as the final argument of the `return cls(...)` call — to each remaining classmethod in this file: `openai` (approx 160), `gemini` (approx 169), `gemini_code_assist` (approx 185), `chatgpt_codex` (approx 203), `minimax` (approx 223), `codex_cli` (approx 239), `gemini_cli` (approx 253), `ollama` (approx 267).
  (d) `chat_with`. Current code (approximate lines 331-338):
```python
        if self._max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(
                lambda: self._provider.chat(request),
                max_retries=self._max_retries,
            )
        return await self._provider.chat(request)
```
  Replace with:
```python
        if self._retry_policy.max_retries > 0:
            from motosan_ai.retry import with_retry

            return await with_retry(
                lambda: self._provider.chat(request),
                policy=self._retry_policy,
            )
        return await self._provider.chat(request)
```
  (e) `stream_with` loop header. Current code (approximate lines 369-371):
```python
        last_error: RateLimitError | None = None
        max_attempts = self._max_retries + 1 if self._max_retries > 0 else 1
        for attempt in range(max_attempts):
```
  Replace with:
```python
        policy = self._retry_policy
        last_error: MotosanError | None = None
        max_attempts = policy.max_retries + 1 if policy.max_retries > 0 else 1
        for attempt in range(max_attempts):
```
  (f) `stream_with` except handler (the `try:` body with the ThinkStripper loop is UNCHANGED). **Re-locate by symbol, not line number** — find the `except (RateLimitError, NetworkError, ProviderError) as e:` block inside `stream_with` (approx lines 390-410 at baseline, but Task 8/py-retrypolicy already rewrote it and earlier M2 tasks shift the numbers). Because Task 8 lands first, the "current code" you will edit is its **post-py-retrypolicy** form, NOT the pristine baseline: the lazy import is already `from motosan_ai.retry import RetryPolicy, _is_retryable, compute_delay`, and the delay math is already a single `compute_delay(...)` call — the old `DEFAULT_INITIAL_BACKOFF`/`DEFAULT_MAX_BACKOFF`/`_parse_retry_after` import and the `wait = min(...)` arithmetic are GONE. Current code (post-Task-8, approximate):
```python
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import RetryPolicy, _is_retryable, compute_delay

                # F1: once the stream has emitted any event, a mid-stream error
                # must propagate verbatim — retrying would replay a partially
                # consumed request and double-emit. Only connection-time
                # (pre-first-yield) failures are retried.
                if yielded or not _is_retryable(e):
                    raise
                last_error = e
                if attempt >= self._max_retries:
                    break
                wait = compute_delay(
                    RetryPolicy(max_retries=self._max_retries),
                    attempt + 1,
                    e.retry_after,
                )
                logger.warning(
                    "Retryable stream error (attempt %d/%d), retrying in %.1fs: %s",
                    attempt + 1,
                    self._max_retries,
                    wait,
                    type(e).__name__,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]
```
  Step 3(e) has already introduced `policy = self._retry_policy` in the loop header just above, so this replacement swaps Task 8's throwaway `RetryPolicy(max_retries=self._max_retries)` and the `self._max_retries` references for the threaded `policy`, and adds the shared `retry_cause` + `on_retry` firing. This keeps the retry-only-before-first-event guard (`if yielded or not _is_retryable(e): raise`) exactly as-is, so `tests/test_client_stream_with.py` stays green. Replace the whole block with:
```python
            except (RateLimitError, NetworkError, ProviderError) as e:
                from motosan_ai.retry import (
                    RetryEvent,
                    _is_retryable,
                    compute_delay,
                    retry_cause,
                )

                # F1: once the stream has emitted any event, a mid-stream error
                # must propagate verbatim — retrying would replay a partially
                # consumed request and double-emit. Only connection-time
                # (pre-first-yield) failures are retried.
                if yielded or not _is_retryable(e):
                    raise
                last_error = e
                if attempt >= policy.max_retries:
                    break
                wait = compute_delay(policy, attempt + 1, e.retry_after)
                if policy.on_retry is not None:
                    policy.on_retry(
                        RetryEvent(attempt=attempt + 1, delay=wait, cause=retry_cause(e))
                    )
                logger.warning(
                    "Retryable stream error (attempt %d/%d), retrying in %.1fs: %s",
                    attempt + 1,
                    policy.max_retries,
                    wait,
                    type(e).__name__,
                )
                await asyncio.sleep(wait)
        raise last_error  # type: ignore[misc]
```
  (g) Guarded cleanup (normally a no-op under canon). Task 8/py-retrypolicy already deletes `_parse_retry_after` and `_STATUS_5XX_RE` from `sdks/python/motosan_ai/retry.py`, rewrites `sdks/python/tests/test_retry.py`, and (its Step 3b) already migrated client.py's stream path off `_parse_retry_after` — so this task inherits a `retry.py` with neither symbol. Guard defensively: IF `retry.py` still contains `_parse_retry_after` and/or `_STATUS_5XX_RE`, delete both now and delete any surviving pinning tests from `sdks/python/tests/test_retry.py` (the `TestParseRetryAfter` class and `TestProviderErrorHttpMessageFormat.test_retry_after_prefix_is_parsed`). In the expected case they are already gone — skip this sub-step.

- [ ] **Step 4 — Run to green.** From `sdks/python`:
  - `uv run pytest tests/test_client_retry_policy.py -v` — expect `7 passed`.
  - `uv run pytest tests/test_client_stream_with.py tests/test_client_integration.py tests/test_client_chat_with.py tests/test_retry.py -v` — expect all passed (legacy `max_retries` kwarg tests and both mid-stream no-retry guards stay green).
  - Package suite: `uv run pytest` — expect all passed (tests/integration live tests self-skip without credentials).

- [ ] **Step 5 — Format and lint.** From `sdks/python`: `uv run ruff format` then `uv run ruff check motosan_ai/` (tests/ is not linted). Expect no diagnostics.

- [ ] **Step 6 — Commit** (on the M2 feature branch; lands via PR + CI):
```
feat(python): thread RetryPolicy through Client chat and stream paths

Client gains retry_policy (None builds a default from legacy max_retries).
chat_with delegates to with_retry(policy=...); stream_with replaces its
hand-rolled backoff with shared compute_delay + on_retry, keeping the
retry-only-before-first-event guard. Removes the last _parse_retry_after
message-scraping call site.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```


## T — TypeScript: requestId, jitter, one retry path

### Task 10: TS: requestId on errors, Retry-After HTTP-date + 60s cap, retry 408/409

**Files:**
- Modify: `sdks/typescript/src/error.ts` (MotosanError ~1-4, mapHttpError ~38-57, isRetryableStatus ~59-67, parseRetryAfter ~104-121; all refs approximate)
- Modify: `sdks/typescript/src/http/fetch.ts` (throwMappedError ~7-21 — the ONLY mapHttpError call site in src; all providers route through postJson/postStream, so no provider file changes)
- Test: `sdks/typescript/tests/error.test.ts` (imports ~1-9, isRetryableStatus describe ~31-54, 'handles large numbers' ~129-132)
- Test: `sdks/typescript/tests/http-fetch.test.ts` (postJson describe ~10-146)

**Interfaces:**
- Produces (D3): `MotosanError.requestId?: string`; `mapHttpError(status: number, message: string, retryAfter?: string | null, requestId?: string | null): MotosanError`
- Produces (D8): `export const RETRY_AFTER_CAP_MS = 60_000`; `parseRetryAfter(headerValue: string | null): number | undefined` (accepts integer seconds AND HTTP-date via `Date.parse`, result clamped to `[0, RETRY_AFTER_CAP_MS]`); `isRetryableStatus(status: number): boolean` = `status === 408 || status === 409 || status === 429 || status >= 500`
- Consumed by: the TS retry-engine task (D4/D7) reads `error.retryAfterMs` (now pre-capped at the source — do NOT re-cap there) and `isRetryableStatus`; the specs task (D5) documents `RETRY_AFTER_CAP_MS` as the TS spelling of `RETRY_AFTER_CAP_SECS = 60`.

- [ ] **Step 1 — Write failing tests.** In `sdks/typescript/tests/error.test.ts`:

  (a) Replace the import block (approximate lines 1-9) with:
  ```ts
  import { describe, it, expect, vi, afterEach } from 'vitest'
  import {
    StreamReadTimeoutError,
    UnsupportedFeatureError,
    isRetryableStatus,
    isRetryableNetworkError,
    parseRetryAfter,
    extractErrorMessage,
    mapHttpError,
    RETRY_AFTER_CAP_MS,
  } from '../src/error.js'
  ```
  (b) Inside `describe('isRetryableStatus', ...)`, after the `'returns true for status >= 500'` test (~line 41), insert:
  ```ts
    it('returns true for 408 (request timeout) and 409 (conflict)', () => {
      expect(isRetryableStatus(408)).toBe(true)
      expect(isRetryableStatus(409)).toBe(true)
    })
  ```
  and change the title `'returns false for 401, 400, 404, 4xx (except 429)'` (~line 43) to `'returns false for 401, 400, 404, 4xx (except 408/409/429)'` (assertions unchanged — 401/400/404/499 stay false).

  (c) Replace the `'handles large numbers'` test (approximate lines 129-132) with:
  ```ts
    it('caps integer seconds above 60 at RETRY_AFTER_CAP_MS', () => {
      const result = parseRetryAfter('3600')
      expect(result).toBe(RETRY_AFTER_CAP_MS) // 1 hour requested, capped to 60s
    })
  ```
  (d) After the closing `})` of `describe('parseRetryAfter', ...)` (~line 133), insert:
  ```ts
  describe('parseRetryAfter HTTP-date form (RFC 7231)', () => {
    afterEach(() => {
      vi.useRealTimers()
    })

    it('parses a future HTTP-date into a millisecond delay', () => {
      vi.useFakeTimers()
      vi.setSystemTime(new Date('2026-07-15T12:00:00Z'))
      const future = new Date(Date.now() + 30_000).toUTCString() // Wed, 15 Jul 2026 12:00:30 GMT
      expect(parseRetryAfter(future)).toBe(30_000)
    })

    it('clamps a past HTTP-date to 0', () => {
      vi.useFakeTimers()
      vi.setSystemTime(new Date('2026-07-15T12:00:00Z'))
      const past = new Date(Date.now() - 45_000).toUTCString()
      expect(parseRetryAfter(past)).toBe(0)
    })

    it('caps an HTTP-date more than 60s ahead at RETRY_AFTER_CAP_MS', () => {
      vi.useFakeTimers()
      vi.setSystemTime(new Date('2026-07-15T12:00:00Z'))
      const farFuture = new Date(Date.now() + 120_000).toUTCString()
      expect(parseRetryAfter(farFuture)).toBe(RETRY_AFTER_CAP_MS)
    })

    it('exports RETRY_AFTER_CAP_MS as 60000', () => {
      expect(RETRY_AFTER_CAP_MS).toBe(60_000)
    })

    it('returns undefined for strings that are neither integer nor date', () => {
      expect(parseRetryAfter('not-a-date')).toBeUndefined()
    })
  })

  describe('mapHttpError requestId', () => {
    it('populates requestId when provided', () => {
      const error = mapHttpError(429, 'rate limited', '2', 'req_abc123')
      expect(error.requestId).toBe('req_abc123')
      expect(error.status).toBe(429)
      expect(error.retryAfterMs).toBe(2000)
    })

    it('leaves requestId undefined when absent or null', () => {
      expect(mapHttpError(500, 'server error').requestId).toBeUndefined()
      expect(mapHttpError(500, 'server error', null, null).requestId).toBeUndefined()
    })
  })
  ```
  In `sdks/typescript/tests/http-fetch.test.ts`, inside `describe('postJson', ...)` after the `'throws ProviderError on 500 response'` test (~line 85), insert:
  ```ts
    it('attaches requestId from the request-id response header', async () => {
      const mockResponse = {
        ok: false,
        status: 429,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'rate limited' } })),
        headers: new Headers({ 'request-id': 'req_primary', 'x-request-id': 'req_fallback' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toMatchObject({ requestId: 'req_primary' })
    })

    it('falls back to the x-request-id header when request-id is absent', async () => {
      const mockResponse = {
        ok: false,
        status: 500,
        text: vi.fn().mockResolvedValue('oops'),
        headers: new Headers({ 'x-request-id': 'req_fallback' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toMatchObject({ requestId: 'req_fallback' })
    })
  ```

- [ ] **Step 2 — Run and confirm failure.** From `sdks/typescript`:
  ```bash
  npx vitest run tests/error.test.ts tests/http-fetch.test.ts
  ```
  Expected: `tests/error.test.ts` fails to collect with `SyntaxError: ... does not provide an export named 'RETRY_AFTER_CAP_MS'` (the constant does not exist yet); `tests/http-fetch.test.ts` reports 2 failures, both `AssertionError` from `rejects.toMatchObject` — the thrown error has no `requestId` property.

- [ ] **Step 3 — Implement.** In `sdks/typescript/src/error.ts`:

  Current code (approximate lines 1-4):
  ```ts
  export class MotosanError extends Error {
    status?: number
    retryAfterMs?: number
  }
  ```
  Replace with:
  ```ts
  export class MotosanError extends Error {
    status?: number
    retryAfterMs?: number
    requestId?: string
  }
  ```
  Current code (approximate lines 38-57, `mapHttpError`) — keep the constructor ternary and `error.status`/`retryAfterMs` lines byte-identical; change only the signature and add the `requestId` block before `return error`:
  ```ts
  export function mapHttpError(
    status: number,
    message: string,
    retryAfter?: string | null,
  ): MotosanError {
  ```
  Replace with:
  ```ts
  export function mapHttpError(
    status: number,
    message: string,
    retryAfter?: string | null,
    requestId?: string | null,
  ): MotosanError {
  ```
  and insert before `return error`:
  ```ts
    if (requestId) {
      error.requestId = requestId
    }
  ```
  Current code (approximate lines 59-67):
  ```ts
  /**
   * Determine if an HTTP status code is retryable.
   * Retryable statuses: 429 (rate limit) or >= 500 (server error).
   *
   * Mirrors Rust `is_retryable_status`.
   */
  export function isRetryableStatus(status: number): boolean {
    return status === 429 || status >= 500
  }
  ```
  Replace with:
  ```ts
  /**
   * Determine if an HTTP status code is retryable.
   * Retryable statuses: 408 (request timeout), 409 (conflict),
   * 429 (rate limit), or >= 500 (server error).
   *
   * Mirrors Rust `is_retryable_status`. See specs/retry.md.
   */
  export function isRetryableStatus(status: number): boolean {
    return status === 408 || status === 409 || status === 429 || status >= 500
  }
  ```
  Current code (approximate lines 104-121, doc comment + `parseRetryAfter` ending `return Number(trimmed) * 1000`):
  ```ts
  export function parseRetryAfter(headerValue: string | null): number | undefined {
    if (headerValue === null) {
      return undefined
    }

    const trimmed = headerValue.trim()
    if (!/^\d+$/.test(trimmed)) {
      return undefined
    }

    return Number(trimmed) * 1000
  }
  ```
  Replace (including its doc comment) with:
  ```ts
  /** Cap applied to any parsed Retry-After value (specs/retry.md: RETRY_AFTER_CAP_SECS = 60). */
  export const RETRY_AFTER_CAP_MS = 60_000

  /**
   * Parse the Retry-After header value into milliseconds.
   *
   * Accepts both RFC 7231 forms:
   * - delay-seconds (e.g. "30") -> seconds * 1000
   * - HTTP-date (e.g. "Wed, 15 Jul 2026 12:00:30 GMT") -> date minus now
   *
   * The result is clamped to [0, RETRY_AFTER_CAP_MS]. Returns undefined if the
   * header is null, empty, or unparseable. Mirrors Rust `parse_retry_after`.
   */
  export function parseRetryAfter(headerValue: string | null): number | undefined {
    if (headerValue === null) {
      return undefined
    }

    const trimmed = headerValue.trim()

    // delay-seconds form
    if (/^\d+$/.test(trimmed)) {
      return Math.min(Number(trimmed) * 1000, RETRY_AFTER_CAP_MS)
    }

    // HTTP-date form. Every RFC 7231 date contains letters (month name, "GMT");
    // requiring one keeps numeric junk like "-5" or "30.5" out of Date.parse,
    // which would otherwise interpret them as calendar dates (Node parses
    // "-5" as a valid date). Matches Rust rfc2822 / Python parsedate behavior.
    if (!/[A-Za-z]/.test(trimmed)) {
      return undefined
    }
    const parsedMs = Date.parse(trimmed)
    if (Number.isNaN(parsedMs)) {
      return undefined
    }
    return Math.max(0, Math.min(parsedMs - Date.now(), RETRY_AFTER_CAP_MS))
  }
  ```
  In `sdks/typescript/src/http/fetch.ts` — current code (approximate lines 7-21):
  ```ts
  async function throwMappedError(response: Response): Promise<never> {
    const text = await response.text()
    let payload: unknown
    try {
      payload = JSON.parse(text)
    } catch {
      payload = text
    }
    const message = extractErrorMessage(payload, `HTTP ${response.status}`)
    throw mapHttpError(
      response.status,
      message,
      response.headers?.get('retry-after') ?? null,
    )
  }
  ```
  Replace with (keep the optional chaining — test mocks have no `.headers`):
  ```ts
  async function throwMappedError(response: Response): Promise<never> {
    const text = await response.text()
    let payload: unknown
    try {
      payload = JSON.parse(text)
    } catch {
      payload = text
    }
    const message = extractErrorMessage(payload, `HTTP ${response.status}`)
    const requestId =
      response.headers?.get('request-id') ?? response.headers?.get('x-request-id') ?? null
    throw mapHttpError(
      response.status,
      message,
      response.headers?.get('retry-after') ?? null,
      requestId,
    )
  }
  ```
  (`Headers.get` is case-insensitive per the WHATWG spec, so `'request-id'` also matches `Request-Id`.)

- [ ] **Step 4 — Run to pass, then guard suites.** From `sdks/typescript`:
  ```bash
  npx vitest run tests/error.test.ts tests/http-fetch.test.ts
  ```
  Expected: all pass — 37 tests in error.test.ts (29 existing incl. 2 modified, +8 new), 13 in http-fetch.test.ts (11 existing, +2 new).
  ```bash
  npx vitest run tests/retry.test.ts tests/retry-integration.test.ts
  ```
  Expected: all pass (both use retry-after `'2'` = 2000ms, under the cap; classification consumers get 408/409 for free).
  ```bash
  npm run build && npm test
  ```
  Expected: full suite green (build first — pack-smoke needs `dist/`).

- [ ] **Step 5 — Typecheck.** From `sdks/typescript` (no prettier/eslint is configured for the TS package; typecheck is the lint gate):
  ```bash
  npm run typecheck
  ```
  Expected: exits 0 with no output.

- [ ] **Step 6 — Commit.**
  ```bash
  git add sdks/typescript/src/error.ts sdks/typescript/src/http/fetch.ts sdks/typescript/tests/error.test.ts sdks/typescript/tests/http-fetch.test.ts
  git commit -m "feat(ts): request-id on errors, Retry-After HTTP-date + 60s cap, retry 408/409" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

### Task 11: Replace TS deterministic jitter with full jitter and add onRetry hook

**Files:**
- Modify: `sdks/typescript/src/retry.ts` (whole file is 107 lines; LCG jitter at approximate lines 65-78, withRetry sleep at approximate lines 94-102)
- Modify: `sdks/typescript/src/index.ts` (approximate line 15)
- Test: `sdks/typescript/tests/retry.test.ts` (pinning block "RetryPolicy jitter (deterministic)" at approximate lines 70-105; import at line 3)

**Interfaces:**
- Consumes: nothing from other tasks. `withRetry` already uses `retryAfterMs` verbatim (no jitter) when `respectRetryAfter=true`; the 60s Retry-After cap is applied upstream by the ts-error-retryafter task inside `parseRetryAfter`/`mapHttpError` — do NOT cap here.
- Produces (D4+D7, copied verbatim): `RetryPolicyOptions.random?: () => number` (default `Math.random`); `RetryPolicyOptions.onRetry?: (evt: RetryEvent) => void`; `export interface RetryEvent { attempt: number; delayMs: number; cause: string }`; public fields `RetryPolicy.random: () => number` and `RetryPolicy.onRetry?: (evt: RetryEvent) => void`; fluent `withRandom(random: () => number): this` and `withOnRetry(onRetry: (evt: RetryEvent) => void): this`. Full jitter: `delay = random() * expDelay` where `expDelay = min(baseDelayMs * 2^(attempt-1), maxDelayMs)`; `jitter=false` returns `expDelay` exactly. `onRetry` fires inside `withRetry` before each sleep, never on terminal failure. Note: the TS LCG constant is written `1_103_515_245` (with separators) — grep accordingly.

- [ ] **Step 1 — Write failing tests.** In `sdks/typescript/tests/retry.test.ts`: (a) change line 3 to `import { RetryPolicy, withRetry, type RetryEvent } from '../src/retry.js'`; (b) DELETE the whole `describe('RetryPolicy jitter (deterministic)', ...)` block (approximate lines 70-105 — it pins LCG values 190/270/720, 191/272, and `delayForAttempt(6)` `.toBe(2000)` with jitter on) and put this in its place:

```ts
describe('RetryPolicy full jitter', () => {
  it('uses injectable random: delay = random() * expDelay', () => {
    const policy = new RetryPolicy({ random: () => 0.5 })

    expect(policy.delayForAttempt(1)).toBe(50)
    expect(policy.delayForAttempt(2)).toBe(100)
    expect(policy.delayForAttempt(3)).toBe(200)
    expect(policy.delayForAttempt(6)).toBe(1000)
  })

  it('random extremes map to 0 and the full exponential delay', () => {
    const floor = new RetryPolicy({ random: () => 0 })
    const ceil = new RetryPolicy({ random: () => 1 })

    expect(floor.delayForAttempt(3)).toBe(0)
    expect(ceil.delayForAttempt(3)).toBe(400)
    expect(ceil.delayForAttempt(6)).toBe(2000)
  })

  it('defaults random to Math.random and stays within [0, expDelay]', () => {
    const policy = RetryPolicy.default()

    expect(policy.random).toBe(Math.random)
    for (let i = 0; i < 200; i += 1) {
      const first = policy.delayForAttempt(1)
      expect(first).toBeGreaterThanOrEqual(0)
      expect(first).toBeLessThanOrEqual(100)

      const capped = policy.delayForAttempt(6)
      expect(capped).toBeGreaterThanOrEqual(0)
      expect(capped).toBeLessThanOrEqual(2000)
    }
  })

  it('jitter=false returns the exact exponential delay and ignores random', () => {
    const policy = new RetryPolicy({ jitter: false, random: () => 0.123 })

    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
    expect(policy.delayForAttempt(6)).toBe(2000)
  })
})
```

(c) append this block at the END of the file (after the `describe('withRetry with error.ts classification', ...)` block, approximate line 296):

```ts
describe('withRetry onRetry', () => {
  afterEach(() => {
    vi.restoreAllMocks()
    vi.useRealTimers()
  })

  it('fires before each sleep with attempt, delayMs, cause', async () => {
    vi.useFakeTimers()
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({
      maxRetries: 3,
      jitter: false,
      onRetry: (evt) => events.push(evt),
    })
    const op = vi.fn(async (attempt: number) => {
      if (attempt < 3) {
        const err = new Error(`boom-${attempt}`) as Error & { status?: number }
        err.status = 503
        throw err
      }
      return 'ok'
    })
    const classify = (err: unknown) => ({
      retryable: (err as { status?: number }).status === 503,
    })

    const promise = withRetry(policy, op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('ok')
    expect(events).toEqual([
      { attempt: 1, delayMs: 100, cause: 'boom-1' },
      { attempt: 2, delayMs: 200, cause: 'boom-2' },
    ])
  })

  it('reports verbatim retryAfterMs as delayMs even with jitter enabled', async () => {
    vi.useFakeTimers()
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({ onRetry: (evt) => events.push(evt) })
    const op = vi.fn(async (attempt: number) => {
      if (attempt === 1) {
        throw new Error('throttled')
      }
      return 'ok'
    })
    const classify = () => ({ retryable: true, retryAfterMs: 500 })

    const promise = withRetry(policy, op, classify)
    await vi.runAllTimersAsync()

    await expect(promise).resolves.toBe('ok')
    expect(events).toEqual([{ attempt: 1, delayMs: 500, cause: 'throttled' }])
  })

  it('does not fire for non-retryable errors', async () => {
    const events: RetryEvent[] = []
    const policy = new RetryPolicy({ onRetry: (evt) => events.push(evt) })
    const op = vi.fn(async () => {
      throw new Error('fatal')
    })

    await expect(withRetry(policy, op, () => ({ retryable: false }))).rejects.toThrow('fatal')
    expect(events).toEqual([])
  })
})
```

- [ ] **Step 2 — Run and confirm failure.** From `sdks/typescript`: `npx vitest run tests/retry.test.ts`. Expected failures (vitest strips types, so it runs; the old LCG ignores `random` and `onRetry` does not exist): 5 failures — `RetryPolicy full jitter > uses injectable random` (`AssertionError: expected 190 to be 50`); `> random extremes` (`expected 720 to be 0`); `defaults random to Math.random` (`expected undefined to be [Function ...]`); and both `withRetry onRetry` recording tests (`expected [] to deeply equal [ { attempt: 1, ... } ]`). The remaining 15 untouched tests still pass.

- [ ] **Step 3 — Implement.** Three edits in `sdks/typescript/src/retry.ts`, one in `src/index.ts`.

  (a) Current code (approximate lines 1-7):
```ts
export interface RetryPolicyOptions {
  maxRetries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  jitter?: boolean
  respectRetryAfter?: boolean
}
```
  Replace with:
```ts
/** Fired via RetryPolicy.onRetry before each retry sleep (D7). */
export interface RetryEvent {
  attempt: number
  delayMs: number
  cause: string
}

export interface RetryPolicyOptions {
  maxRetries?: number
  baseDelayMs?: number
  maxDelayMs?: number
  jitter?: boolean
  respectRetryAfter?: boolean
  /** Injectable RNG in [0, 1) for full jitter. Defaults to Math.random. */
  random?: () => number
  onRetry?: (evt: RetryEvent) => void
}
```

  (b) Current code (approximate lines 20-34, class fields + constructor):
```ts
export class RetryPolicy {
  maxRetries: number
  baseDelayMs: number
  maxDelayMs: number
  jitter: boolean
  respectRetryAfter: boolean

  constructor(opts?: RetryPolicyOptions) {
    this.maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES
    this.baseDelayMs = opts?.baseDelayMs ?? DEFAULT_BASE_DELAY_MS
    this.maxDelayMs = opts?.maxDelayMs ?? DEFAULT_MAX_DELAY_MS
    this.jitter = opts?.jitter ?? DEFAULT_JITTER
    this.respectRetryAfter =
      opts?.respectRetryAfter ?? DEFAULT_RESPECT_RETRY_AFTER
  }
```
  Replace with:
```ts
export class RetryPolicy {
  maxRetries: number
  baseDelayMs: number
  maxDelayMs: number
  jitter: boolean
  respectRetryAfter: boolean
  random: () => number
  onRetry?: (evt: RetryEvent) => void

  constructor(opts?: RetryPolicyOptions) {
    this.maxRetries = opts?.maxRetries ?? DEFAULT_MAX_RETRIES
    this.baseDelayMs = opts?.baseDelayMs ?? DEFAULT_BASE_DELAY_MS
    this.maxDelayMs = opts?.maxDelayMs ?? DEFAULT_MAX_DELAY_MS
    this.jitter = opts?.jitter ?? DEFAULT_JITTER
    this.respectRetryAfter =
      opts?.respectRetryAfter ?? DEFAULT_RESPECT_RETRY_AFTER
    this.random = opts?.random ?? Math.random
    this.onRetry = opts?.onRetry
  }
```

  (c) Current code (approximate lines 60-78, last fluent setter + delayForAttempt with the LCG — note the constant is spelled `1_103_515_245`):
```ts
  withRespectRetryAfter(enabled: boolean): this {
    this.respectRetryAfter = enabled
    return this
  }

  delayForAttempt(attempt: number): number {
    const exponent = Math.min(Math.max(attempt - 1, 0), 31)
    const expFactor = 2 ** exponent
    let delayMs = Math.min(this.baseDelayMs * expFactor, this.maxDelayMs)

    if (this.jitter) {
      const jitterSeed = attempt * 1_103_515_245 + 12_345
      const jitterPercent = jitterSeed % 100
      const jittered = delayMs + Math.floor((delayMs * jitterPercent) / 100)
      delayMs = Math.min(jittered, this.maxDelayMs)
    }

    return delayMs
  }
```
  Replace with:
```ts
  withRespectRetryAfter(enabled: boolean): this {
    this.respectRetryAfter = enabled
    return this
  }

  withRandom(random: () => number): this {
    this.random = random
    return this
  }

  withOnRetry(onRetry: (evt: RetryEvent) => void): this {
    this.onRetry = onRetry
    return this
  }

  delayForAttempt(attempt: number): number {
    const exponent = Math.min(Math.max(attempt - 1, 0), 31)
    const expDelay = Math.min(this.baseDelayMs * 2 ** exponent, this.maxDelayMs)

    if (!this.jitter) {
      return expDelay
    }

    // Full jitter (specs/retry.md): uniform in [0, expDelay).
    return this.random() * expDelay
  }
```

  (d) Current code in `withRetry` (approximate lines 94-102):
```ts
      if (attempt < policy.maxRetries && retryable) {
        attempt += 1
        const delay = policy.respectRetryAfter
          ? retryAfterMs ?? policy.delayForAttempt(attempt)
          : policy.delayForAttempt(attempt)

        await new Promise((resolve) => setTimeout(resolve, delay))
        continue
      }
```
  Replace with:
```ts
      if (attempt < policy.maxRetries && retryable) {
        attempt += 1
        // Retry-After (capped upstream in parseRetryAfter) is used VERBATIM — no jitter.
        const delay = policy.respectRetryAfter
          ? retryAfterMs ?? policy.delayForAttempt(attempt)
          : policy.delayForAttempt(attempt)

        policy.onRetry?.({
          attempt,
          delayMs: delay,
          cause: error instanceof Error ? error.message : String(error),
        })

        await new Promise((resolve) => setTimeout(resolve, delay))
        continue
      }
```

  (e) In `sdks/typescript/src/index.ts`, current code (approximate line 15):
```ts
export { RetryPolicy } from './retry.js'
```
  Replace with:
```ts
export { RetryPolicy } from './retry.js'
export type { RetryPolicyOptions, RetryEvent } from './retry.js'
```

- [ ] **Step 4 — Run and confirm pass, then package suite.** From `sdks/typescript`: `npx vitest run tests/retry.test.ts tests/retry-integration.test.ts` — expect all tests pass (retry.test.ts now has 22 tests; retry-integration is unaffected because every policy there uses `jitter: false`). Then the FULL suite: `npm run build && npm test` — expect 0 failures (pack-smoke needs `dist/`, hence build first). Provider stream loops call `delayForAttempt` directly and inherit full jitter, but their tests all pin `jitter: false` / zero delays, so nothing else changes.

- [ ] **Step 5 — Typecheck (this package has no lint/format tooling).** From `sdks/typescript`: `npm run typecheck` — expect clean exit, no output.

- [ ] **Step 6 — Commit.**
```bash
git add sdks/typescript/src/retry.ts sdks/typescript/src/index.ts sdks/typescript/tests/retry.test.ts
git commit -m "feat(typescript): full-jitter backoff with injectable RNG and onRetry hook

Replaces the deterministic LCG jitter with full jitter (uniform in
[0, expDelay)) per specs/retry.md D4, adds RetryPolicyOptions.random
for test injection, and fires RetryPolicy.onRetry(RetryEvent) before
each retry sleep per D7. Retry-After delays stay verbatim (no jitter).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

Note for the TS loop-consolidation task: the hand-rolled stream retry loops in `src/providers/{anthropic,openai,gemini,ollama,chatgpt_codex}.ts` do NOT fire `onRetry` after this task — routing them through `withRetry` is what closes that gap.

### Task 12: Route all TS provider request paths through shared withRetry classification

> **Execute AFTER Task 10 (ts-error-retryafter) and Task 11 (ts-jitter-onretry).** Those tasks modify `src/error.ts` (D8: 408/409 join `isRetryableStatus`) and `src/retry.ts` (D4/D7: full jitter, `random`, `onRetry`); Task 11 also rewrote `tests/retry.test.ts` line 3 to `import { RetryPolicy, withRetry, type RetryEvent } from '../src/retry.js'` and appended `onRetry`/`RetryEvent` tests. Neither task changes the `withRetry(policy, op, classify)` signature or touches `src/providers/*`. All line refs below are approximate (measured on the pre-M2 baseline; expect a few lines of drift in `error.ts`/`retry.ts` imports and in `tests/retry.test.ts` line 3, which is now in its **post-Task-11** state).

**Duplication map (verified against the baseline — confirm with `grep -n classifyHttpError src/providers/*.ts` from `sdks/typescript`):**
- 4 byte-identical `classifyHttpError` copies: `anthropic.ts` ~145-158, `openai.ts` ~38-51, `gemini.ts` ~45-64 (incl. its doc comment), `ollama.ts` ~17-31 (incl. its comment line).
- 5 hand-rolled pre-first-event stream retry loops: `anthropic.ts` ~268-289, `openai.ts` ~326-351, `gemini.ts` ~181-205, `ollama.ts` ~299-321, `chatgpt_codex.ts` ~225-246.
- 1 unretried request path: `openai.ts` ~226 (`chatViaResponses` bare `postJson`).
- NO loops in `src/client.ts` (only `withRetryPolicy` plumbing) or `src/providers/minimax.ts` (delegates to `AnthropicProvider`) — both untouched.

**Files:**
- Modify: `sdks/typescript/src/retry.ts` (append `classifyForRetry` at end of file, ~line 107; add one import at line 1)
- Modify: `sdks/typescript/src/providers/anthropic.ts` (~1-11, ~145-158, ~218-222, ~268-289)
- Modify: `sdks/typescript/src/providers/openai.ts` (~1, ~6, ~38-51, ~226, ~259-263, ~326-351)
- Modify: `sdks/typescript/src/providers/gemini.ts` (~1, ~6, ~45-64, ~119-123, ~181-205)
- Modify: `sdks/typescript/src/providers/ollama.ts` (~1, ~6, ~17-31, ~239-243, ~299-321)
- Modify: `sdks/typescript/src/providers/chatgpt_codex.ts` (~11, ~16, ~225-246)
- Test: `sdks/typescript/tests/retry.test.ts` (new `classifyForRetry` describe)
- Test: `sdks/typescript/tests/retry-integration.test.ts` (new ChatGPT-Codex 503→200 stream test; existing 16 tests must pass UNCHANGED — they pin chat retry, stream initial-fetch retry, and the no-mid-stream-retry guard: "Anthropic stream does not retry after the response body has been returned" ~line 221 and the OpenAI twin ~line 303)
- Test: `sdks/typescript/tests/providers-openai-responses.test.ts` (~lines 253-265, approx) — replace the `'does not retry the single Responses fallback call'` test, which pins the OLD non-retried Responses fallback, with two tests pinning the NEW retried behavior (a retryable 5xx→200 is retried through the shared path; a non-retryable 404 is NOT). All other tests in the file pass UNCHANGED.

**Interfaces:**
- Produces: `export function classifyForRetry(errOrStatus: unknown): RetryClassification` in `src/retry.ts`. Package-internal: do NOT add it to `src/index.ts` (index re-exports only `RetryPolicy` from `retry.js`; `withRetry` is likewise internal).
- Consumes: `withRetry<T>(policy: RetryPolicy, op: (attempt: number) => Promise<T>, classify: (errOrResult: unknown) => RetryClassification): Promise<T>` (src/retry.ts, unchanged); `isRetryableStatus(status: number): boolean` and `isRetryableNetworkError(error: unknown): boolean` (src/error.ts; post-D8: `status === 408 || status === 409 || status === 429 || status >= 500`).

**Placement rationale (locked):** `classifyForRetry` goes in `retry.ts`, not `error.ts`, because its return type `RetryClassification` is declared in `retry.ts` and `error.ts` is a zero-import leaf module (imported by `http/fetch.ts` and everything else). `retry.ts → error.ts` is a new one-way edge with no reverse import — cycle-free. Putting it in `error.ts` would force `error.ts`'s first-ever import, a back-edge to `retry.ts` for the type.

**Behavior notes (read before editing):** (a) The old copies `throw result` for non-retryable Errors from INSIDE classify; `classifyForRetry` instead returns `{ retryable: false }` and `withRetry` rethrows the original error — observably identical (the thrown value was the original error either way). OpenAI's 404→Responses fallback keeps working: the original error with `.status === 404` propagates out of `withRetry`. (b) Wrapping `chatViaResponses` adds retry to a previously unretried path — intended ("ALL request paths"). One test DID pin the old non-retry — `tests/providers-openai-responses.test.ts`'s `'does not retry the single Responses fallback call'` (~lines 253-265) — so Step 1 replaces it with two tests pinning the NEW retried Responses-fallback behavior. (c) Streaming: `withRetry` wraps ONLY the `postStream` call; SSE/NDJSON parsing runs outside it, so nothing retries after the first emitted event (D5(d)), preserving the guard pinned by the two "does not retry after the response body has been returned" tests.

All commands run from `sdks/typescript`.

- [ ] **Step 1 — failing tests.** In `tests/retry.test.ts`, change line 3 — which is in its **post-Task-11** state — `import { RetryPolicy, withRetry, type RetryEvent } from '../src/retry.js'` → `import { classifyForRetry, RetryPolicy, withRetry, type RetryEvent } from '../src/retry.js'` (KEEP `type RetryEvent`: Task 11 added it and its `onRetry` tests use it; dropping it would leave the file referencing an unimported type, which vitest's type-stripping and `tsc` both miss). Append at end of file:
```ts
describe('classifyForRetry', () => {
  it('classifies retryable-status errors and carries retryAfterMs', () => {
    const err = new Error('rate limited') as Error & { status?: number; retryAfterMs?: number }
    err.status = 429
    err.retryAfterMs = 1500
    expect(classifyForRetry(err)).toEqual({ retryable: true, retryAfterMs: 1500 })
  })

  it('returns retryable:false for non-retryable statuses without throwing', () => {
    const err = new Error('bad request') as Error & { status?: number }
    err.status = 400
    expect(classifyForRetry(err)).toEqual({ retryable: false })
  })

  it('classifies retryable network errors', () => {
    const err = new Error('refused') as Error & { code?: string }
    err.code = 'ECONNREFUSED'
    expect(classifyForRetry(err).retryable).toBe(true)
  })

  it('accepts a bare numeric status', () => {
    expect(classifyForRetry(503)).toEqual({ retryable: true })
    expect(classifyForRetry(404)).toEqual({ retryable: false })
  })

  it('treats non-Error, non-number values as not retryable', () => {
    expect(classifyForRetry('boom')).toEqual({ retryable: false })
  })
})
```
In `tests/retry-integration.test.ts`, add below the `AnthropicProvider` import (~line 3): `import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'`, and add inside the `describe` after the last test (~line 354):
```ts
  it('ChatGPT-Codex stream retries a 503 then succeeds before the first event (shared path)', async () => {
    let calls = 0
    const sse =
      'data: {"type":"response.output_text.delta","delta":"stream ok"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => {
        calls += 1
        return calls === 1
          ? new Response(JSON.stringify({ error: { message: 'overloaded' } }), { status: 503 })
          : new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
      }),
    )

    const provider = new ChatGptCodexProvider('tok', 'acct').withRetryPolicy(immediateRetryPolicy())
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    expect(calls).toBe(2)
    expect(events.filter((e) => !e.done).map((e) => e.content)).toContain('stream ok')
  })
```
Honesty note: this integration test also passes against the current hand-rolled codex loop — it pins 503-retry behavior ACROSS the swap; the red signal for this task is the `classifyForRetry` import failure. After Step 8, `grep -rn "while (true)" src/providers/` returning nothing proves it runs through the shared path.

In `tests/providers-openai-responses.test.ts`, replace the `'does not retry the single Responses fallback call'` test (~lines 253-265, approx — quoted below) — it pins the OLD behavior (the Responses fallback's 500 throws with no retry, so exactly 2 fetch calls):
```ts
  it('does not retry the single Responses fallback call', async () => {
    provider
      .withRetryPolicy(new RetryPolicy({ maxRetries: 3, baseDelayMs: 1, jitter: false }))
      .withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(500, { error: { message: 'Responses unavailable' } }))

    await expect(provider.chat({ messages: [{ role: 'user', content: 'test' }] })).rejects.toThrow(
      'Responses unavailable',
    )
    expect(mockFetch).toHaveBeenCalledTimes(2)
  })
```
with two tests pinning the NEW behavior (Step 6(4) now wraps `chatViaResponses` in `withRetry`, so the Responses call is routed through the shared engine). All symbols used — `jsonResponse`, `RetryPolicy`, `DEFAULT_OPENAI_RESPONSES_URL` — are already imported at the top of the file:
```ts
  it('retries the Responses fallback call on a retryable 5xx then succeeds (shared path)', async () => {
    provider
      .withRetryPolicy(new RetryPolicy({ maxRetries: 3, baseDelayMs: 1, jitter: false }))
      .withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(500, { error: { message: 'Responses unavailable' } }))
      .mockResolvedValueOnce(
        jsonResponse(200, {
          output_text: 'recovered',
          usage: { input_tokens: 1, output_tokens: 1 },
        }),
      )

    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.content).toBe('recovered')
    // 3 total fetches: the triggering 404 on chat/completions, then the
    // Responses call's 500 -> 200 retry through the shared withRetry engine
    // (the two Responses-endpoint fetches are the retried pair).
    expect(mockFetch).toHaveBeenCalledTimes(3)
    expect(String(mockFetch.mock.calls[1][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
    expect(String(mockFetch.mock.calls[2][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
  })

  it('does not retry a non-retryable 404 in the Responses fallback path', async () => {
    provider
      .withRetryPolicy(new RetryPolicy({ maxRetries: 3, baseDelayMs: 1, jitter: false }))
      .withResponsesFallback(true)
    mockFetch
      .mockResolvedValueOnce(jsonResponse(404, {}))
      .mockResolvedValueOnce(jsonResponse(404, { error: { message: 'Responses not found' } }))

    await expect(provider.chat({ messages: [{ role: 'user', content: 'test' }] })).rejects.toThrow(
      'Responses not found',
    )
    // 2 total fetches: chat/completions 404 triggers the fallback, and the
    // Responses 404 is non-retryable so withRetry rethrows without a retry.
    expect(mockFetch).toHaveBeenCalledTimes(2)
  })
```
Ordering: this replacement depends on Step 6(4) for the retried test to pass. The first new test is RED until Step 6(4) lands (at baseline the Responses 500 throws immediately, giving 2 calls + a rejection instead of `'recovered'` / 3 calls); the second passes throughout (the 404 fallback path is non-retried before and after — a stable regression pin).
- [ ] **Step 2 — run-fail.** `npx vitest run tests/retry.test.ts` → the whole file fails to load: `SyntaxError: The requested module '../src/retry.js' does not provide an export named 'classifyForRetry'`. `npx vitest run tests/retry-integration.test.ts` → 17 passed (new test green against the old loop, per the note above).
- [ ] **Step 3 — implement `classifyForRetry`.** In `src/retry.ts`: add as line 1 `import { isRetryableNetworkError, isRetryableStatus } from './error.js'` (the file currently has no imports), then append at end of file (after `withRetry`, ~line 107):
```ts
/**
 * Shared retry classification for every provider request path (chat and the
 * pre-first-event stream fetch). Replaces the four per-provider
 * classifyHttpError copies. Accepts a thrown value (usually a mapHttpError
 * Error carrying `.status` / `.retryAfterMs`) or a bare numeric HTTP status.
 * Pure — never throws; withRetry rethrows the original error on
 * `{ retryable: false }`.
 */
export function classifyForRetry(errOrStatus: unknown): RetryClassification {
  if (typeof errOrStatus === 'number') {
    return { retryable: isRetryableStatus(errOrStatus) }
  }
  if (errOrStatus instanceof Error) {
    const error = errOrStatus as { status?: number; retryAfterMs?: number }
    const status = error.status
    if (
      (status !== undefined && isRetryableStatus(status)) ||
      isRetryableNetworkError(errOrStatus)
    ) {
      return { retryable: true, retryAfterMs: error.retryAfterMs }
    }
  }
  return { retryable: false }
}
```
- [ ] **Step 4 — run-pass units.** `npx vitest run tests/retry.test.ts` → all tests pass, including the 5 new `classifyForRetry` tests (total count depends on the preceding ts-jitter-onretry task's updates to the jitter describe).
- [ ] **Step 5 — anthropic.ts.** (1) Imports: replace the `../error.js` block (~lines 1-6) `import {\n  isRetryableNetworkError,\n  isRetryableStatus,\n  ProviderError,\n  StreamError,\n} from '../error.js'` with `import { ProviderError, StreamError } from '../error.js'`; replace (~line 11) `import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'` with `import { classifyForRetry, RetryPolicy, withRetry } from '../retry.js'`. (2) Delete the local copy. Current code (approximate lines 145-158):
```ts
function classifyHttpError(result: unknown): RetryClassification {
  if (result instanceof Error) {
    const error = result as { status?: number; retryAfterMs?: number }
    const status = error.status
    if (
      (status !== undefined && isRetryableStatus(status)) ||
      isRetryableNetworkError(result)
    ) {
      return { retryable: true, retryAfterMs: error.retryAfterMs }
    }
    throw result
  }
  return { retryable: false }
}
```
Delete it entirely (no replacement). (3) `chat()` request phase — current code (approximate lines 218-222):
```ts
    const payload = await withRetry(
      this.retryPolicy,
      async () => postJson<any>(`${this.baseUrl}/v1/messages`, headers, body),
      classifyHttpError,
    )
```
Replace with the identical call but `classifyForRetry,` as the third argument (it is the sole remaining `classifyHttpError` reference in the file). (4) `streamImpl()` request phase — current code (approximate lines 268-289):
```ts
    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(`${this.baseUrl}/v1/messages`, headers, body)
        break
      } catch (error) {
        const status = (error as { status?: number }).status
        const retryable =
          (status !== undefined && isRetryableStatus(status)) ||
          isRetryableNetworkError(error)
        if (!retryable || attempt >= this.retryPolicy.maxRetries) {
          throw error
        }
        attempt += 1
        const retryAfterMs = (error as { retryAfterMs?: number }).retryAfterMs
        const delay = this.retryPolicy.respectRetryAfter
          ? retryAfterMs ?? this.retryPolicy.delayForAttempt(attempt)
          : this.retryPolicy.delayForAttempt(attempt)
        await new Promise((resolve) => setTimeout(resolve, delay))
      }
    }
```
Replace with:
```ts
    // Retry ONLY the initial fetch. parseSse below runs outside withRetry, so
    // nothing is retried after the first emitted event (pinned by
    // tests/retry-integration.test.ts "does not retry after the response body
    // has been returned").
    const responseBody = await withRetry(
      this.retryPolicy,
      async () => postStream(`${this.baseUrl}/v1/messages`, headers, body),
      classifyForRetry,
    )
```
- [ ] **Step 6 — openai.ts.** (1) Delete line 1 `import { isRetryableNetworkError, isRetryableStatus } from '../error.js'` (nothing else is imported from error.js here); replace (~line 6) the retry import exactly as in Step 5(1). (2) Delete `classifyHttpError` (~lines 38-51; byte-identical to the Step 5(2) block). (3) `chat()` (~lines 259-263, inside the `try`): swap the third `withRetry` argument `classifyHttpError,` → `classifyForRetry,` — the surrounding 404-fallback `try/catch` stays byte-identical. (4) `chatViaResponses()` — current code (approximate line 226):
```ts
    const payload = await postJson<any>(this.responsesUrl, this.headers(), body)
```
Replace with:
```ts
    const payload = await withRetry(
      this.retryPolicy,
      async () => postJson<any>(this.responsesUrl, this.headers(), body),
      classifyForRetry,
    )
```
(5) `streamImpl()` — current code (approximate lines 326-351): the same 22-line `let attempt = 0 … }` loop quoted in full in Step 5(4), except the try-body reads `responseBody = await postStream(\n          this.chatUrl,\n          this.headers(),\n          body,\n        )` across five lines. Replace the whole loop with:
```ts
    // Retry ONLY the initial fetch; no retry after the first emitted event.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () => postStream(this.chatUrl, this.headers(), body),
      classifyForRetry,
    )
```
- [ ] **Step 7 — gemini.ts + ollama.ts.** Both files: delete line 1 (`import { isRetryableNetworkError, isRetryableStatus } from '../error.js'`); replace the ~line 6 retry import as in Step 5(1). **gemini.ts:** delete ~lines 45-64 — the `/** Copied verbatim from providers/openai.ts:38-51 … */` doc comment AND the `classifyHttpError` function under it (body identical to Step 5(2)); in `chat()` (~line 122) swap `classifyHttpError,` → `classifyForRetry,`; in `streamImpl()` delete ~lines 181-205 — the three comment lines starting `// Retry ONLY the initial postStream fetch (manual status-aware loop,` plus the Step 5(4)-identical loop whose try-body is `responseBody = await postStream(url, this.headers(), body)` — and replace with:
```ts
    // Retry ONLY the initial postStream fetch via the shared engine. Once the
    // body is obtained, parseSse drives with NO mid-stream retry (gemini.rs:372-413).
    const responseBody = await withRetry(
      this.retryPolicy,
      async () => postStream(url, this.headers(), body),
      classifyForRetry,
    )
```
**ollama.ts:** delete ~lines 17-31 — the `/** Same classify shape as providers/openai.ts:38-51 / providers/minimax.ts. */` comment AND `classifyHttpError`; in `chat()` (~line 242) swap `classifyHttpError,` → `classifyForRetry,`; in `streamImpl()` delete ~lines 299-321 — the comment `// Retry ONLY the initial postStream fetch (mirrors openai.ts:326-351).` plus the Step 5(4)-identical loop whose try-body is `responseBody = await postStream(this.endpoint(), {}, body)` — and replace with:
```ts
    // Retry ONLY the initial postStream fetch via the shared engine.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () => postStream(this.endpoint(), {}, body),
      classifyForRetry,
    )
```
- [ ] **Step 8 — chatgpt_codex.ts.** (1) Replace (~line 11) `import { StreamError, isRetryableNetworkError, isRetryableStatus } from '../error.js'` with `import { StreamError } from '../error.js'`; replace (~line 16) `import { RetryPolicy } from '../retry.js'` with `import { classifyForRetry, RetryPolicy, withRetry } from '../retry.js'`. (2) `streamImpl()` request phase — current code (approximate lines 225-246):
```ts
    // Retry ONLY the initial fetch (mirrors anthropic.ts:259-288 / ollama.ts:300-321).
    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(this.baseUrl, headers, body)
        break
      } catch (error) {
        const status = (error as { status?: number }).status
        const retryable =
          (status !== undefined && isRetryableStatus(status)) || isRetryableNetworkError(error)
        if (!retryable || attempt >= this.retryPolicy.maxRetries) {
          throw error
        }
        attempt += 1
        const retryAfterMs = (error as { retryAfterMs?: number }).retryAfterMs
        const delay = this.retryPolicy.respectRetryAfter
          ? retryAfterMs ?? this.retryPolicy.delayForAttempt(attempt)
          : this.retryPolicy.delayForAttempt(attempt)
        await new Promise((resolve) => setTimeout(resolve, delay))
      }
    }
```
Replace with:
```ts
    // Retry ONLY the initial fetch via the shared engine (same guard as the
    // other providers: nothing is retried after the first emitted event).
    const responseBody = await withRetry(
      this.retryPolicy,
      async () => postStream(this.baseUrl, headers, body),
      classifyForRetry,
    )
```
- [ ] **Step 9 — verify dedupe + run-pass.** `grep -rn "classifyHttpError\|isRetryableStatus\|isRetryableNetworkError\|delayForAttempt" src/providers/ src/client.ts` → NO matches (classification and delay math now live only in `retry.ts`/`error.ts`). Then: `npx vitest run tests/retry-integration.test.ts` → 17 passed (16 existing unchanged + the Codex 503 test); `npx vitest run tests/retry.test.ts tests/providers-anthropic.test.ts tests/providers-openai.test.ts tests/providers-openai-responses.test.ts tests/providers-gemini.test.ts tests/providers-ollama.test.ts tests/providers-chatgpt-codex.test.ts tests/providers-minimax.test.ts` → all pass (this includes the two rewritten `providers-openai-responses.test.ts` fallback tests from Step 1, now green because Step 6(4) routes `chatViaResponses` through `withRetry`). Full suite: `npm run build && npm test` (build first — pack-smoke needs `dist/`) → all test files pass.
- [ ] **Step 10 — typecheck (this SDK has no lint/format script; `tsc` is the gate).** `npm run typecheck` → exits 0 with no output. TS6133-style unused-import issues cannot occur: every removed helper's imports were deleted in Steps 5-8.
- [ ] **Step 11 — commit.**
```bash
git add sdks/typescript
git commit -m "refactor(typescript): dedupe retry classification and route all provider requests through withRetry" -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```


## C — Cross-SDK conformance

### Task 13: Add cross-SDK specs/retry.md conformance test suites

> **Execute LAST before the release task — after all per-SDK M2 tasks land.** This task adds one table-driven test suite per SDK that mirrors `specs/retry.md` § classification + § Retry-After + § backoff, so future drift in any SDK fails loudly. It is a conformance-suite task: the implementations already exist, so the red-green cycle is inverted — **a failure here is a real defect in an earlier task's code (or an interface-name mismatch). Fix the implementation or align the name; never weaken these tables.**

**Files:**
- Modify: `sdks/rust/src/providers/mod.rs` — append an **in-crate** `#[cfg(test)] mod retry_conformance` at the end of the file. The consumed helpers (`is_retryable_status`, `parse_retry_after`) are `pub(crate)` and live in `providers/mod.rs` itself (NOT in a public `retry` module), so a `tests/*.rs` integration test — a separate crate — could not see them. An in-crate unit test reaches them via `use super::{...}`. The mod is gated behind the same 7-feature HTTP `cfg` as those helpers (`default = []`, so the helpers do not exist in a featureless build); it therefore compiles and runs only under `--all-features` (or any HTTP feature). No new file under `tests/`, and **no** `[[test]]` entry / visibility change in `sdks/rust/Cargo.toml`.
- Create: `sdks/python/tests/test_retry_conformance.py`
- Create: `sdks/typescript/tests/retry-conformance.test.ts`
- Test-only task, but the Rust change edits a tracked `.rs` source file (the `mod` is `#[cfg(test)]`, so it ships no runtime code): it still lands via PR + CI, never direct to main (repo rule); commit on the M2 milestone branch.

**Interfaces:** Consumes only (produces no new API):
- Rust — reached from the **in-crate** unit test, so it can use the `pub(crate)` helpers directly:
  - `pub(crate) fn is_retryable_status(status_code: u16) -> bool` in `providers/mod.rs` — post-Task-3 (D8): `status == 408 || status == 409 || status == 429 || status >= 500`. Feature-gated behind the 7 HTTP features.
  - `pub(crate) fn parse_retry_after(headers: &HeaderMap) -> Option<Duration>` in `providers/mod.rs` — post-Task-3 (D5/D8): integer delta-seconds AND RFC 2822 HTTP-date (via `chrono`), clamped to `[0, RETRY_AFTER_CAP]` where `pub(crate) const RETRY_AFTER_CAP: Duration = Duration::from_secs(60)` (spec's `RETRY_AFTER_CAP_SECS = 60`, same value). Takes a **`&HeaderMap`** — build one in the test, NOT a `&str`. A non-integer / negative / unparseable value → `None`; a past HTTP-date → `Duration::ZERO`. Feature-gated behind the 7 HTTP features.
  - `crate::retry::RetryPolicy` (compiled unconditionally): `pub fn delay_for_attempt(&self, attempt: u32) -> Duration` (full jitter, D4) and `pub(crate) fn delay_for_attempt_with_rng(&self, attempt: u32, rng: &mut fastrand::Rng) -> Duration` (seed the jitter deterministically). There is **no** closure-injection variant — the RNG is `&mut fastrand::Rng`. Fields `max_retries`, `base_delay_ms`, `max_delay_ms`, `jitter`, `respect_retry_after` (D4).
- Python `motosan_ai.retry` (D8/D9): `_is_retryable(error: Exception) -> bool` (attribute-based); `parse_retry_after_header(value: str | None) -> float | None` (integer + HTTP-date, clamped `[0.0, 60.0]`; a negative integer → `None`, only a **past HTTP-date** clamps to `0.0`); `@dataclass RetryPolicy(max_retries=3, base_delay=0.1, max_delay=2.0, jitter=True, respect_retry_after=True, on_retry=None)` — a plain dataclass with **no** delay method; the free function `compute_delay(policy: RetryPolicy, attempt: int, retry_after: float | None = None, rng: Callable[[], float] = random.random) -> float` — full jitter `rng() * min(base_delay * 2**(attempt-1), max_delay)` (D4/D8). There is **no** `RetryPolicy.delay_for_attempt` method.
- Python `motosan_ai.error` (D2): `MotosanError.__init__(self, message: str = "", *, status_code: int | None = None, retry_after: float | None = None, request_id: str | None = None)`; subclasses inherit unchanged.
- TS `src/error.ts` (D8): `isRetryableStatus(status: number): boolean`; `parseRetryAfter(headerValue: string | null): number | undefined` (ms, clamped `[0, 60_000]`). TS `src/retry.ts` (D4): `RetryPolicy` with `RetryPolicyOptions.random?: () => number` (default `Math.random`) and `delayForAttempt(attempt: number): number`.

- [ ] **Step 1 — Append the in-crate `mod retry_conformance` to `sdks/rust/src/providers/mod.rs`** (at the end of the file), exactly:

```rust
// Mirrors specs/retry.md — update BOTH or neither.
// Cross-SDK siblings assert the SAME tables:
//   sdks/python/tests/test_retry_conformance.py
//   sdks/typescript/tests/retry-conformance.test.ts
// A failure means the implementation drifted from specs/retry.md — fix the
// implementation (or the spec plus all three suites), never this file alone.
// In-crate unit test (NOT tests/): is_retryable_status / parse_retry_after are
// pub(crate) in this module and feature-gated, so an integration test could not
// import them. Gated behind the same 7-feature cfg (default = []).
#[cfg(test)]
#[cfg(any(
    feature = "anthropic",
    feature = "openai",
    feature = "minimax",
    feature = "ollama_native",
    feature = "gemini",
    feature = "gemini-code-assist",
    feature = "chatgpt-codex",
))]
mod retry_conformance {
    use super::{is_retryable_status, parse_retry_after};
    use crate::retry::RetryPolicy;
    use reqwest::header::{HeaderMap, HeaderValue};
    use std::time::Duration;

    const RETRYABLE: [u16; 8] = [408, 409, 429, 500, 502, 503, 529, 599];
    const NON_RETRYABLE: [u16; 8] = [200, 301, 400, 401, 403, 404, 422, 499];

    /// parse_retry_after takes a &HeaderMap, so wrap the raw value in one.
    fn retry_after(value: &str) -> Option<Duration> {
        let mut headers = HeaderMap::new();
        headers.insert("retry-after", HeaderValue::from_str(value).unwrap());
        parse_retry_after(&headers)
    }

    #[test]
    fn classification_retryable_statuses() {
        for status in RETRYABLE {
            assert!(
                is_retryable_status(status),
                "specs/retry.md: {status} must be retryable"
            );
        }
    }

    #[test]
    fn classification_non_retryable_statuses() {
        for status in NON_RETRYABLE {
            assert!(
                !is_retryable_status(status),
                "specs/retry.md: {status} must NOT be retryable"
            );
        }
    }

    #[test]
    fn retry_after_integer_seconds() {
        assert_eq!(retry_after("5"), Some(Duration::from_secs(5)));
        assert_eq!(retry_after("0"), Some(Duration::ZERO));
        assert_eq!(retry_after(" 7 "), Some(Duration::from_secs(7)));
    }

    #[test]
    fn retry_after_capped_at_60s() {
        assert_eq!(retry_after("61"), Some(Duration::from_secs(60)));
        assert_eq!(retry_after("86400"), Some(Duration::from_secs(60)));
    }

    #[test]
    fn retry_after_http_date_clamped() {
        // Deterministic: past date clamps to 0; far-future date clamps to the 60s cap.
        assert_eq!(
            retry_after("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(Duration::ZERO)
        );
        assert_eq!(
            retry_after("Fri, 31 Dec 2100 23:59:59 GMT"),
            Some(Duration::from_secs(60))
        );
    }

    #[test]
    fn retry_after_garbage_is_none() {
        // A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
        assert_eq!(retry_after(""), None);
        assert_eq!(retry_after("soon"), None);
        assert_eq!(retry_after("-5"), None);
    }

    #[test]
    fn full_jitter_bounded_by_capped_exponential() {
        // specs/retry.md § backoff: full jitter — uniform in
        // [0, min(base * 2^(attempt-1), max_delay)]. The Rust RNG is &mut
        // fastrand::Rng (no fractional-scale injection), so assert the ceiling
        // over a seeded run instead of an exact scaled value.
        let policy = RetryPolicy::default(); // base 100ms, max 2000ms, jitter on
        let mut rng = fastrand::Rng::with_seed(42);
        // ceilings for attempts 1..=7 (attempt 6+ capped at max_delay = 2000ms)
        let ceilings_ms = [100u64, 200, 400, 800, 1600, 2000, 2000];
        for (idx, &ceiling) in ceilings_ms.iter().enumerate() {
            let attempt = (idx + 1) as u32;
            for _ in 0..200 {
                let delay = policy.delay_for_attempt_with_rng(attempt, &mut rng);
                assert!(
                    delay <= Duration::from_millis(ceiling),
                    "specs/retry.md: attempt {attempt} delay {delay:?} exceeds {ceiling}ms ceiling"
                );
            }
        }
        // Full jitter must spread draws, not pin to the ceiling (seed-deterministic).
        let mut rng = fastrand::Rng::with_seed(42);
        let draws: Vec<Duration> = (0..64)
            .map(|_| policy.delay_for_attempt_with_rng(3, &mut rng))
            .collect();
        assert!(
            draws.iter().any(|d| d != &draws[0]),
            "specs/retry.md: full jitter must vary across draws: {draws:?}"
        );
    }

    #[test]
    fn jitter_disabled_is_pure_exponential() {
        let policy = RetryPolicy::default().jitter(false);
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(100));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(200));
        assert_eq!(policy.delay_for_attempt(5), Duration::from_millis(1600));
        assert_eq!(policy.delay_for_attempt(6), Duration::from_millis(2000));
    }

    #[test]
    fn defaults_match_spec() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.base_delay_ms, 100);
        assert_eq!(policy.max_delay_ms, 2000);
        assert!(policy.jitter);
        assert!(policy.respect_retry_after);
    }
}
```

- [ ] **Step 2 — Run the Rust suite** (from `sdks/rust`): `cargo test --all-features providers::retry_conformance` — expected: `test result: ok. 9 passed; 0 failed`. If a name fails to resolve (`cannot find function is_retryable_status in module super`, or `no method named delay_for_attempt_with_rng found for struct RetryPolicy`), the D3/D4 Rust tasks used a different name — align THAT task, then update this mod's imports to match; if an assertion fails, fix the implementation. (`default = []`, so this filter is only meaningful under `--all-features`.)

- [ ] **Step 3 — Create `sdks/python/tests/test_retry_conformance.py`** with exactly:

```python
"""Mirrors specs/retry.md — update BOTH or neither.

Cross-SDK siblings assert the SAME tables:
  sdks/rust/src/providers/mod.rs (mod retry_conformance)
  sdks/typescript/tests/retry-conformance.test.ts
A failure means the implementation drifted from specs/retry.md — fix the
implementation (or the spec plus all three suites), never this file alone.
"""

import pytest

from motosan_ai.error import (
    AuthError,
    InvalidRequestError,
    MotosanError,
    NetworkError,
    ProviderError,
    RateLimitError,
)
from motosan_ai.retry import (
    RetryPolicy,
    _is_retryable,
    compute_delay,
    parse_retry_after_header,
)

RETRYABLE_STATUSES = [408, 409, 429, 500, 502, 503, 529, 599]
NON_RETRYABLE_STATUSES = [400, 401, 403, 404, 422, 499]


def error_for_status(status: int) -> MotosanError:
    """Construct the exception map_http_error produces for this status (D1 mapping).

    429 -> RateLimitError, so its retryability is exercised via isinstance,
    matching real provider raise sites. (200/301 are not error statuses, so
    Python's attribute-based table omits them; Rust/TS classify raw codes.)
    """
    message = f"HTTP {status}: boom"
    if status == 401:
        return AuthError(message, status_code=status)
    if status == 429:
        return RateLimitError(message, status_code=status)
    if status == 400:
        return InvalidRequestError(message, status_code=status)
    return ProviderError(message, status_code=status)


class TestClassificationTable:
    @pytest.mark.parametrize("status", RETRYABLE_STATUSES)
    def test_retryable(self, status):
        assert _is_retryable(error_for_status(status)) is True

    @pytest.mark.parametrize("status", NON_RETRYABLE_STATUSES)
    def test_non_retryable(self, status):
        assert _is_retryable(error_for_status(status)) is False

    def test_network_error_is_retryable(self):
        assert _is_retryable(NetworkError("connection reset")) is True

    def test_message_text_is_ignored(self):
        # D9: classification is attribute-based; "500" in the text no longer counts.
        assert _is_retryable(ProviderError("Error code: 500 - server error")) is False

    def test_plain_exception_not_retryable(self):
        assert _is_retryable(ValueError("bad")) is False


class TestParseRetryAfterHeader:
    def test_integer_seconds(self):
        assert parse_retry_after_header("5") == 5.0
        assert parse_retry_after_header("0") == 0.0
        assert parse_retry_after_header(" 7 ") == 7.0

    def test_capped_at_60s(self):
        assert parse_retry_after_header("61") == 60.0
        assert parse_retry_after_header("86400") == 60.0

    def test_http_date_clamped(self):
        # Deterministic: past date clamps to 0; far-future date clamps to the cap.
        assert parse_retry_after_header("Wed, 21 Oct 2015 07:28:00 GMT") == 0.0
        assert parse_retry_after_header("Fri, 31 Dec 2100 23:59:59 GMT") == 60.0

    def test_garbage_is_none(self):
        # A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
        assert parse_retry_after_header(None) is None
        assert parse_retry_after_header("") is None
        assert parse_retry_after_header("soon") is None
        assert parse_retry_after_header("-5") is None


class TestPolicyMath:
    def test_full_jitter_scales_capped_exp_delay_by_rng(self):
        policy = RetryPolicy()  # base 0.1s, max 2.0s, jitter on
        # compute_delay is a free function; RetryPolicy has no delay method.
        assert compute_delay(policy, 1, rng=lambda: 0.5) == pytest.approx(0.05)
        assert compute_delay(policy, 2, rng=lambda: 0.5) == pytest.approx(0.1)
        assert compute_delay(policy, 3, rng=lambda: 0.5) == pytest.approx(0.2)
        assert compute_delay(policy, 4, rng=lambda: 0.0) == 0.0
        # attempt 6: uncapped exp = 0.1 * 2**5 = 3.2 -> capped at 2.0 BEFORE jitter.
        assert compute_delay(policy, 6, rng=lambda: 1.0) == pytest.approx(2.0)

    def test_jitter_disabled_is_pure_exponential(self):
        policy = RetryPolicy(jitter=False)
        assert compute_delay(policy, 1) == pytest.approx(0.1)
        assert compute_delay(policy, 2) == pytest.approx(0.2)
        assert compute_delay(policy, 5) == pytest.approx(1.6)
        assert compute_delay(policy, 6) == pytest.approx(2.0)

    def test_defaults_match_spec(self):
        policy = RetryPolicy()
        assert policy.max_retries == 3
        assert policy.base_delay == 0.1
        assert policy.max_delay == 2.0
        assert policy.jitter is True
        assert policy.respect_retry_after is True
```

- [ ] **Step 4 — Run the Python suite** (from `sdks/python`): `uv run pytest tests/test_retry_conformance.py -v` — expected: `24 passed` (8 retryable + 6 non-retryable parametrized + 3 other classification + 4 Retry-After + 3 policy-math). Same failure protocol as Step 2 (ImportError → align the D8/D9 task's names; AssertionError → fix the implementation).

- [ ] **Step 5 — Create `sdks/typescript/tests/retry-conformance.test.ts`** with exactly:

```ts
// Mirrors specs/retry.md — update BOTH or neither.
// Cross-SDK siblings assert the SAME tables:
//   sdks/rust/src/providers/mod.rs (mod retry_conformance)
//   sdks/python/tests/test_retry_conformance.py
// A failure means the implementation drifted from specs/retry.md — fix the
// implementation (or the spec plus all three suites), never this file alone.
import { describe, expect, it } from 'vitest'
import { isRetryableStatus, parseRetryAfter } from '../src/error.js'
import { RetryPolicy } from '../src/retry.js'

const RETRYABLE = [408, 409, 429, 500, 502, 503, 529, 599]
const NON_RETRYABLE = [200, 301, 400, 401, 403, 404, 422, 499]

describe('specs/retry.md § classification', () => {
  it.each(RETRYABLE)('status %i is retryable', (status) => {
    expect(isRetryableStatus(status)).toBe(true)
  })

  it.each(NON_RETRYABLE)('status %i is NOT retryable', (status) => {
    expect(isRetryableStatus(status)).toBe(false)
  })
})

describe('specs/retry.md § Retry-After', () => {
  it('parses integer seconds to milliseconds', () => {
    expect(parseRetryAfter('5')).toBe(5000)
    expect(parseRetryAfter('0')).toBe(0)
    expect(parseRetryAfter(' 7 ')).toBe(7000)
  })

  it('caps at 60 seconds', () => {
    expect(parseRetryAfter('61')).toBe(60_000)
    expect(parseRetryAfter('86400')).toBe(60_000)
  })

  it('parses HTTP-date, clamped to [0, 60s]', () => {
    // Deterministic: past date clamps to 0; far-future date clamps to the cap.
    expect(parseRetryAfter('Wed, 21 Oct 2015 07:28:00 GMT')).toBe(0)
    expect(parseRetryAfter('Fri, 31 Dec 2100 23:59:59 GMT')).toBe(60_000)
  })

  it('returns undefined for garbage', () => {
    // A negative integer is invalid, NOT clamp-to-0 (only past HTTP-dates clamp).
    expect(parseRetryAfter(null)).toBeUndefined()
    expect(parseRetryAfter('')).toBeUndefined()
    expect(parseRetryAfter('soon')).toBeUndefined()
    expect(parseRetryAfter('-5')).toBeUndefined()
  })
})

describe('specs/retry.md § backoff (full jitter)', () => {
  it('scales the capped exponential delay by the injected random()', () => {
    const half = new RetryPolicy({ random: () => 0.5 }) // base 100, max 2000
    expect(half.delayForAttempt(1)).toBe(50)
    expect(half.delayForAttempt(2)).toBe(100)
    expect(half.delayForAttempt(3)).toBe(200)
    expect(new RetryPolicy({ random: () => 0 }).delayForAttempt(4)).toBe(0)
    // attempt 6: uncapped exp = 100 * 2^5 = 3200 → capped at 2000 BEFORE jitter.
    expect(new RetryPolicy({ random: () => 1 }).delayForAttempt(6)).toBe(2000)
  })

  it('jitter=false is pure exponential and ignores random', () => {
    const policy = new RetryPolicy({ jitter: false, random: () => 0.123 })
    expect(policy.delayForAttempt(1)).toBe(100)
    expect(policy.delayForAttempt(2)).toBe(200)
    expect(policy.delayForAttempt(5)).toBe(1600)
    expect(policy.delayForAttempt(6)).toBe(2000)
  })

  it('defaults match spec', () => {
    const policy = RetryPolicy.default()
    expect(policy.maxRetries).toBe(3)
    expect(policy.baseDelayMs).toBe(100)
    expect(policy.maxDelayMs).toBe(2000)
    expect(policy.jitter).toBe(true)
    expect(policy.respectRetryAfter).toBe(true)
  })
})
```

- [ ] **Step 6 — Run the TS suite** (from `sdks/typescript`): `npx vitest run tests/retry-conformance.test.ts` — expected: `Test Files  1 passed`, `Tests  23 passed` (8 + 8 parametrized classification + 4 Retry-After + 3 backoff). Same failure protocol as Step 2.

- [ ] **Step 7 — Full package suites** (all three must be green):
  - `sdks/rust`: `cargo test --all-features` — expected: every test binary reports `0 failed` (the lib target now also runs `providers::retry_conformance`, alongside the updated `openai_retry` + other M2 suites).
  - `sdks/python`: `uv run pytest` — expected: all tests pass, 0 failures.
  - `sdks/typescript`: `npm run build && npm test` — expected: build succeeds, vitest reports 0 failed (pack-smoke needs `dist/`).

- [ ] **Step 8 — Format & lint:**
  - `sdks/rust`: `cargo fmt` then `cargo clippy --all-features -- -D warnings` — expected: no diffs, no warnings.
  - `sdks/python`: `uv run ruff format` then `uv run ruff check motosan_ai/` — expected: clean (`tests/` is intentionally NOT linted).
  - `sdks/typescript`: `npm run typecheck` — expected: exit 0.

- [ ] **Step 9 — Commit** (on the M2 milestone branch — the `providers/mod.rs` change is a `.rs` edit, so it lands via PR + CI, never direct to main):

```
test(retry): add cross-SDK specs/retry.md conformance suites

One table-driven suite per SDK pinning the normative classification
table (408/409/429/>=500), Retry-After parsing (integer + HTTP-date,
60s cap, negative -> none), and full-jitter backoff bounds with
injected RNG. The Rust suite is an in-crate #[cfg(test)] mod in
providers/mod.rs (the helpers are pub(crate)); Python/TS are new test
files. Drift in any SDK now fails loudly against specs/retry.md.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
```


## Release

### Task 14: Release M2 — Rust 0.23.0 / Python 0.16.0 / TypeScript 0.13.0

**ORDERING: run this task LAST, only after every other M2 task has merged.** This is a version/changelog/docs-only commit — it must contain NO source changes. Rust 0.23.0 is BREAKING (`MotosanError` enum shape, per D1); Python 0.16.0 and TypeScript 0.13.0 are additive.

**Files:**
- Modify: `sdks/rust/Cargo.toml` (~line 3), `sdks/python/pyproject.toml` (~line 3), `sdks/typescript/package.json` (~line 3)
- Modify (regenerated): `uv.lock` (root; the `motosan-ai` pin is ~line 98), `sdks/typescript/package-lock.json` (~lines 3, 9). `Cargo.lock` is GITIGNORED (`.gitignore` line 2) — NEVER staged.
- Modify: `CHANGELOG.md` (root, insert above the M1 entry ~line 5), `sdks/rust/CHANGELOG.md` (insert below `## [Unreleased]` ~line 5), `sdks/python/CHANGELOG.md` (insert above `## [0.15.0]` ~line 5), `sdks/typescript/CHANGELOG.md` (insert above `## [0.12.0]` ~line 7)
- Modify (doc version lines, all refs approximate): `AGENTS.md` (~lines 5, 11), `llms.txt` (~lines 5, 24, 923), `skills/motosan-ai/SKILL.md` (~lines 8, 25), `skills/motosan-ai/references/rust-api.md` (~line 7), `README.md` (~lines 29–31, 38), `sdks/rust/README.md` (~lines 323, 433, 497), `sdks/typescript/README.md` (~lines 485–486)
- Test: none (release task — the gate is the full suites)

**Interfaces:** None (self-contained). Consumes the merged output of all prior M2 tasks via git history; produces the release commit. NO tags, NO publishing — the maintainer tags `rust-v0.23.0` / `python-v0.16.0` / `ts-v0.13.0` after merge per `llms.txt` § Release.

Throughout: replace every `<DATE>` below with the output of `date +%F`. Before committing, `grep -rn '<DATE>' CHANGELOG.md sdks/*/CHANGELOG.md` must return nothing.

- [ ] **Step 1 — Preconditions.** From the repo root run `git log --oneline d7c06ff..HEAD`. Expected: a non-empty list containing the M2 feature commits (structured errors, retry classification, jitter, on_retry, send_with_retry, Python RetryPolicy, specs/retry.md). If EMPTY, STOP — the M2 work has not merged; this task runs last. Then run `grep -rn '0\.22\.0\|0\.15\.0\|0\.12\.0' README.md AGENTS.md llms.txt skills/motosan-ai/SKILL.md skills/motosan-ai/references/rust-api.md sdks/rust/README.md sdks/typescript/README.md` — expected: matches at approximately AGENTS.md:5,11; llms.txt:5,24,923; SKILL.md:8,25; rust-api.md:7; README.md:29-31,38; sdks/rust/README.md:323,433,497; sdks/typescript/README.md:485-486. Create the release branch if not already on it: `git checkout -b chore/m2-release`.
- [ ] **Step 2 — Bump the three manifests.** `sdks/rust/Cargo.toml` current (~line 3): `version = "0.22.0"` → `version = "0.23.0"`. `sdks/python/pyproject.toml` current (~line 3): `version = "0.15.0"` → `version = "0.16.0"`. `sdks/typescript/package.json` current (~line 3): `"version": "0.12.0",` → `"version": "0.13.0",`.
- [ ] **Step 3 — Regenerate lockfiles.** From the repo root: `uv lock --project sdks/python` — expected: `uv.lock` changes its `motosan-ai` entry (~line 98) to `version = "0.16.0"`. Then `cd sdks/typescript && npm install --package-lock-only` — expected: `package-lock.json` lines ~3 and ~9 become `"version": "0.13.0"`. Verify Cargo.lock stays out: `git status --short | grep -c Cargo.lock` → expected `0` (it is gitignored).
- [ ] **Step 4 — Root `CHANGELOG.md` entry.** Insert directly above `## [rust-0.22.0 / python-0.15.0 / ts-0.12.0] — 2026-07-15` (~line 5):

```markdown
## [rust-0.23.0 / python-0.16.0 / ts-0.13.0] — <DATE>

M2 retry release — structured error metadata, status-based retry classification, one retry engine per SDK, and a normative retry spec. **Breaking for Rust** (`MotosanError` enum shape); additive for Python and TypeScript.

### Breaking (Rust)

- **`MotosanError` HTTP variants are now struct variants** (Rust): `Auth`, `RateLimit`, `InvalidRequest`, `ProviderError` become `{ message, status_code, retry_after, request_id }`. `Display` output is byte-identical to 0.22; only pattern matches and constructions change. See `sdks/rust/CHANGELOG.md` for the migration example.

### Added

- **Structured error metadata** (Rust · Python · TypeScript): errors carry `status_code` / `retry_after` / `request_id`, with `request_id` read from the `request-id` / `x-request-id` response headers. Rust adds `status_code()` / `retry_after()` / `request_id()` accessors; Python `MotosanError` gains keyword-only attributes (additive — the M1 `"HTTP {status}: ..."` message prefixes stay); TypeScript adds `requestId` (it already had `status` / `retryAfterMs`).
- **`on_retry` observer** (Rust · Python · TypeScript): `RetryPolicy.on_retry` / `onRetry` fires before each retry sleep with `(attempt, delay, cause)`.
- **Python `RetryPolicy`** (Python): `@dataclass RetryPolicy(max_retries=3, base_delay=0.1, max_delay=2.0, jitter=True, respect_retry_after=True, on_retry=None)`; `with_retry(fn, policy=...)` accepts it while old kwargs keep working; `Client` threads a policy through both chat and stream paths.
- **`specs/retry.md`** (spec — all SDKs): the normative cross-SDK retry contract — classification table, Retry-After semantics, full-jitter backoff, stream-retry rule (only before the first emitted event), and the explicit no-transport-retry rule for CLI backends.

### Changed

- **Status-based retry classification** (Rust · Python · TypeScript): retry on HTTP 408, 409, 429, ≥500 and transport/connection errors; never on other 4xx. Python no longer scrapes messages (the `\b5\d{2}\b` regex and message-parsed Retry-After are gone); TypeScript adds 408/409.
- **Retry-After date form + cap** (Rust · Python · TypeScript): integer-seconds AND HTTP-date (RFC 7231) forms honored, clamped to [0, 60 s] independent of `max_delay`, used verbatim (no jitter) when `respect_retry_after` is set.
- **Full jitter** (Rust · Python · TypeScript): effective delay is `uniform_random(0, min(base·2^(attempt−1), max_delay))` from an injectable RNG, replacing the deterministic LCG.
- **`send_with_retry` consolidation** (Rust · TypeScript): every hand-rolled provider HTTP chat/stream request loop routes through one shared transport helper per SDK; providers keep only serialization + response handling. (Python's stream path shares the same policy math via `with_retry`/`RetryPolicy`.)

Per-SDK detail: [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md), [`sdks/python/CHANGELOG.md`](sdks/python/CHANGELOG.md), [`sdks/typescript/CHANGELOG.md`](sdks/typescript/CHANGELOG.md).
```
- [ ] **Step 5 — Per-SDK changelog entries.** (a) `sdks/rust/CHANGELOG.md`: insert below `## [Unreleased]` (~line 5), above `## [0.22.0] - 2026-07-15`:

````markdown
## [0.23.0] - <DATE>

### Breaking
- `MotosanError::Auth` / `RateLimit` / `InvalidRequest` / `ProviderError` are now struct variants `{ message: String, status_code: Option<u16>, retry_after: Option<Duration>, request_id: Option<String> }`. `Display` output is byte-identical to 0.22 (e.g. `"rate limit error: {message}"`). `Config`, `Network`, `Stream`, `StreamReadTimeout`, `UnsupportedFeature` keep their tuple form. Migration:

  ```rust
  // 0.22
  match err {
      MotosanError::RateLimit(msg) => eprintln!("rate limited: {msg}"),
      other => return Err(other),
  }
  // 0.23
  match err {
      MotosanError::RateLimit { message, retry_after, .. } => {
          eprintln!("rate limited: {message} (retry after {retry_after:?})")
      }
      other => return Err(other),
  }
  ```

### Added
- `MotosanError::status_code()` / `retry_after()` / `request_id()` accessors (`None` for non-HTTP variants); `request_id` is read from the `request-id` / `x-request-id` response headers at the `map_http_error` choke point.
- `RetryPolicy.on_retry: Option<Arc<dyn Fn(RetryEvent) + Send + Sync>>` with `RetryEvent { attempt, delay, cause }` and `RetryCause::Status(u16)` / `RetryCause::Network(String)` — fires before each retry sleep.
- `send_with_retry` transport helper in `providers/mod.rs` — all HTTP provider chat/stream request loops route through it.

### Changed
- Retry classification is status-based: retry on 408, 409, 429, ≥500 and reqwest timeout/connect errors; never on other 4xx.
- `Retry-After` accepts integer-seconds and HTTP-date (RFC 7231) forms, clamped to [0, 60 s], used verbatim (no jitter) when `respect_retry_after` is set.
- Full jitter from an injectable RNG replaces the deterministic LCG jitter.
````

(b) `sdks/python/CHANGELOG.md`: insert above `## [0.15.0] - 2026-07-15` (~line 5):

```markdown
## [0.16.0] - <DATE>

### Added
- `MotosanError` gains keyword-only `status_code`, `retry_after`, `request_id` attributes (default `None`); subclasses inherit them, providers populate them at raise sites, and `request_id` comes from the `request-id` / `x-request-id` response headers. Additive — the M1 `"HTTP {status}: ..."` message prefixes remain.
- `RetryPolicy` dataclass in `motosan_ai.retry` (`max_retries=3`, `base_delay=0.1`, `max_delay=2.0`, `jitter=True`, `respect_retry_after=True`, `on_retry=None`); `with_retry(fn, policy=...)` accepts it and the old kwargs keep working. `Client` threads a policy through both chat and stream paths; the stream path's hand-rolled backoff now uses the shared policy math.
- `on_retry` observer: `RetryEvent(attempt, delay, cause)` fired before each retry sleep.

### Changed
- Retry classification is attribute-based: `RateLimitError` / `NetworkError` always retryable; `ProviderError` retryable when `status_code` is 408, 409, or ≥500. The `\b5\d{2}\b` message regex and message-scraped `Retry-After` parsing are removed — the delay now comes from `error.retry_after`.
- `Retry-After` accepts integer-seconds and HTTP-date forms (`email.utils.parsedate_to_datetime`), clamped to [0, 60 s], used verbatim (no jitter).
- Full jitter from an injectable `rng` callable (default `random.random`) replaces the deterministic LCG jitter.
```

(c) `sdks/typescript/CHANGELOG.md`: insert above `## [0.12.0] - 2026-07-15` (~line 7):

```markdown
## [0.13.0] - <DATE>

### Added
- `MotosanError.requestId?: string`, populated by `mapHttpError` from the `request-id` / `x-request-id` response headers.
- `RetryPolicyOptions.onRetry?: (evt: RetryEvent) => void` with `RetryEvent { attempt: number; delayMs: number; cause: string }` — fires before each retry sleep.
- `RetryPolicyOptions.random?: () => number` — injectable RNG for jitter (default `Math.random`).

### Changed
- Retry classification adds 408 and 409: retry on 408 / 409 / 429 / ≥500 plus fetch transport errors (`AbortError` / `TypeError` / `ECONNREFUSED` / `ENOTFOUND` / `ETIMEDOUT`); never on other 4xx.
- `Retry-After` accepts HTTP-date alongside integer-seconds, clamped to [0, 60 s], used verbatim (no jitter).
- Full jitter from the injectable RNG replaces the deterministic LCG in `delayForAttempt`.
- Provider chat/stream retry loops collapsed onto the shared retry helper.
```
- [ ] **Step 6 — Cross-check every bullet against git (M1 Step-5 style).** Run `git log --oneline d7c06ff..HEAD`. For EVERY bullet written in Steps 4–5, identify the commit(s) implementing it and write the bullet→commit mapping into your task output. DELETE any bullet whose work did not actually merge (e.g. if the TS loop consolidation or a jitter change was cut, its bullet goes — do not keep it for symmetry). ADD a bullet for any merged M2 change the lists miss. MOVE anything the feature tasks left under `## [Unreleased]` in any per-SDK changelog into the new release section (Rust has an `## [Unreleased]` header at ~line 5; check Python/TS too), leaving the empty `## [Unreleased]` header in place in `sdks/rust/CHANGELOG.md`.
- [ ] **Step 7 — Doc version lines (grep-verified).** Make exactly these edits — leave historical narrative untouched: `AGENTS.md` ~line 5 `Rust v0.22.0 · Python v0.15.0 (PyPI) · TypeScript v0.12.0 (npm)` → `Rust v0.23.0 · Python v0.16.0 (PyPI) · TypeScript v0.13.0 (npm)`, and insert a new paragraph after the M1 paragraph (~line 11): `Rust 0.23.0 / Python 0.16.0 / TypeScript 0.13.0 are the M2 retry releases: errors carry structured metadata (status_code / retry_after / request_id; Rust HTTP variants become struct variants — **breaking**), retry classification is status-based (408/409/429/>=500 plus transport errors), Retry-After honors integer-seconds and HTTP-date capped at 60 s, full jitter replaces the deterministic LCG, RetryPolicy gains an on_retry observer (and lands in Python as a dataclass threaded through chat and stream), Rust providers share one send_with_retry helper, and specs/retry.md is the normative cross-SDK retry contract.` Then: `llms.txt` ~line 5 → `- Python 0.16.0 · TypeScript 0.13.0 · Rust 0.23.0`; ~line 24 → `motosan-ai = { version = "0.23.0", features = ["anthropic"] }`; ~line 923 tag-table example `ts-v0.12.0` → `ts-v0.13.0`. `skills/motosan-ai/SKILL.md` ~line 8 → `Multi-provider LLM SDK — Python 0.16.0 / Rust 0.23.0 / TypeScript 0.13.0`; ~line 25 → `motosan-ai = { version = "0.23.0", features = ["anthropic"] }`. `skills/motosan-ai/references/rust-api.md` ~line 7 → same `0.23.0` Cargo line. `README.md` Languages table ~lines 29–31 → `v0.23.0` / `v0.16.0` / `v0.13.0`; Install example ~line 38 → `0.23.0`. `sdks/rust/README.md` ~lines 323, 433, 497: `motosan-ai = { version = "0.22.0", features = ["claude-code"] }` (and `codex-cli` / `gemini-cli`) → `0.23.0`. `sdks/typescript/README.md` ~lines 485–486: `git tag ts-v0.12.0` / `git push origin ts-v0.12.0` → `ts-v0.13.0`. Verify: rerun the Step 1 grep — expected output is EXACTLY ONE match: the AGENTS.md M1 history paragraph (~line 11, `"...are the M1 reliability releases..."`). Any other match is a missed update — fix it.
- [ ] **Step 8 — Gate.** From the repo root: `nix develop -c check-all` (or, without nix: `cd sdks/rust && cargo fmt --check && cargo clippy --all-features -- -D warnings && cargo test --all-features`, then `cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration/`). Expected: `=== All checks passed ===` / all suites green, exit 0. Then `cd sdks/typescript && npm run typecheck && npm run build && npm test` — expected: tsc silent, build emits `dist/`, vitest all files passed, exit 0. If ANYTHING is red, STOP — this commit must not carry fixes; report instead.
- [ ] **Step 9 — Commit (no tags, no publish).** Stage exactly the release files:

```bash
git add sdks/rust/Cargo.toml sdks/python/pyproject.toml sdks/typescript/package.json \
  uv.lock sdks/typescript/package-lock.json \
  CHANGELOG.md sdks/rust/CHANGELOG.md sdks/python/CHANGELOG.md sdks/typescript/CHANGELOG.md \
  AGENTS.md llms.txt skills/motosan-ai/SKILL.md skills/motosan-ai/references/rust-api.md \
  README.md sdks/rust/README.md sdks/typescript/README.md
git status --short   # expected: ONLY the 16 files above staged; no Cargo.lock, no src/ files
git commit -m "chore(release): M2 retry + structured errors — rust-v0.23.0 / python-v0.16.0 / ts-v0.13.0" \
  -m "Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

This lands via PR + CI like every Cargo.toml change (house rule). Do NOT run `git tag`, `cargo publish`, `uv publish`, or `npm publish` — after the PR merges, the maintainer tags `rust-v0.23.0` / `python-v0.16.0` / `ts-v0.13.0` per `llms.txt` § Release (tags trigger the publish workflows).

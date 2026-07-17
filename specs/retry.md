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
| Caller-initiated cancellation (TypeScript `CancelledError`, table below) | ❌ never |
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

Read-idle timeout errors that fire mid-stream (Rust
`MotosanError::StreamReadTimeout`, TypeScript
`StreamReadTimeoutError`, and Python `StreamReadTimeoutError` —
surfaced from `httpx.ReadTimeout`) are **not retried**: they occur
after the first emitted event, so the rule above applies, even though
timeout-class transport errors are retryable during the connection
phase.

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

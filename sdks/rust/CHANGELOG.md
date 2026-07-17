# Changelog

All notable changes to `motosan-ai` Rust SDK are documented in this file.

## [Unreleased]

## [0.25.0] - 2026-07-17

### Breaking
- CLI chat/stream contract (`claude-code` / `codex-cli` / `gemini-cli`): a successfully completed CLI turn always reports `stop_reason = Some(StopReason::EndTurn)` on both `chat()` and `stream()`. CLI backends never report `ToolUse` — their tools are executed internally by the CLI; `ToolUse` means "caller must execute tools", which a CLI backend never requests, and reporting it made agent loops re-execute already-executed tools. The internal `cli_terminal_stop_reason(saw_tool_call)` helper is retired; the terminal stream event is always `done_with_stop_reason(EndTurn)`. Migration: code that branched on `StopReason::ToolUse` after a CLI turn should treat `ChatResponse.tool_calls` as the record of already-executed tools and branch on `EndTurn`.
- `chat()` for all three CLI backends is reimplemented as stream delegation (collect the provider's own `stream()`), so `tool_calls` / `thinking` / `usage` / `session_id` populate identically on both paths. `ChatResponse.tool_calls` for CLI backends is **no longer always empty** — it records the tools the CLI already executed (never a request to execute). One documented parity exception: `chat()` backfills `ChatResponse.model` from provider config when the collected value is empty. The `chat()` failure surface shifts to the stream-path variants (`StreamReadTimeout` on stalls, stream error variants on abnormal CLI exit — no longer the single-shot mappings), and `codex-cli` `chat()` no longer splits the preamble into `thinking` (the old split was a post-hoc whole-transcript heuristic, unrepresentable in a stream: content is the concatenation, `thinking` is `None`). The newly-dead single-shot invoke path was removed.

### Added
- `motosan_ai::auth` (ungated): `#[async_trait] pub trait TokenSource: Send + Sync + Debug { async fn access_token(&self) -> Result<String, MotosanError>; }` plus `StaticTokenSource`. `ChatGptCodexProvider` stores `Arc<dyn TokenSource>` — `new()` keeps its exact signature and wraps the plain token in `StaticTokenSource`; a `with_token_source` builder is added; `Debug` never prints token material — and resolves the bearer token at the top of **every retry attempt** (`send_with_retry_async_build`; the pre-existing `send_with_retry` is now a thin wrapper over it, preserving the single M2 retry engine and `on_retry`). `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)` threads a custom source through the facade. The SDK stays decoupled from the oauth crates — a refreshing `TokenSource` over the workspace `codex-oauth` crate ships as an `#[ignore]`d live test.
- `ollama-native` feature alias for `ollama_native`.

### Changed
- Feature architecture: private umbrella features `_http = [dep:reqwest, dep:chrono, dep:eventsource-stream, dep:tokio]` and `_cli = [dep:tokio, dep:async-stream]` replace the per-provider `dep:` lists; `tokio-stream` is an unconditional dependency (and `stream.rs` loses its feature gate); the HTTP-shared helpers (`send_with_retry`, `observe_and_sleep`, `parse_retry_after`, `extract_request_id`, `is_retryable_status`, `is_retryable_network_error`, `map_http_error`, `RETRY_AFTER_CAP`, `TimeoutConfig`) move from `providers/mod.rs` to `src/transport/http.rs` behind one `#[cfg(feature = "_http")]` gate. Public feature set unchanged (plus the `ollama-native` alias); resolved dependencies per pre-existing feature are identical. CI adds `cargo hack check --each-feature`.

## [0.24.0] - 2026-07-17

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

- Stream EOF semantics: a provider stream that ends **without** the provider's terminal event now yields `Err(MotosanError::IncompleteStream(_))` — `"incomplete stream: <provider> ended without a terminal event"` — instead of fabricating a final `done` event. OpenAI-wire streams complete on `[DONE]` or a `finish_reason` chunk (either suffices — EOF after a stashed `finish_reason` still emits `done` with the stop reason); truncation with neither signal yields the error. Anthropic requires `message_stop`; Gemini / chatgpt-codex require their terminal frames. This retires the v0.10.1 fabricated-`done` invariant for the neither-signal case. Handling truncation:

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
- Timeout API cleanup: `Client::stream_read_timeout()` getter is removed; `ClientBuilder::stream_read_timeout_secs` is deprecated in favor of `read_idle_timeout(Duration)`. HTTP streams now enforce a 120 s default read-idle deadline; idle expiry yields `MotosanError::StreamReadTimeout` and is never retried mid-stream.

### Added
- Unified timeout model: `ClientBuilder::connect_timeout(Duration)` (default 10 s on the shared reqwest client), `.read_idle_timeout(Duration)` (default 120 s; same per-chunk semantics as the old stream read timeout), and `.total_timeout(Duration)` (default off; bounds each blocking `chat()` attempt, never streams; expiry surfaces as retryable `MotosanError::Network`).

### Changed
- `ClientBuilder::build()` constructs the provider once with a single shared `reqwest::Client` (configured with `connect_timeout`); `dispatch_chat` / `dispatch_stream` no longer rebuild the provider per request.

### Fixed
- Pre-built `gemini_code_assist` provider honors `ClientBuilder::retry_policy` (previously silently discarded).

## [0.23.0] - 2026-07-16

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
- Cross-SDK `specs/retry.md` conformance suite: in-crate `#[cfg(test)] mod retry_conformance` in `providers/mod.rs`.

### Changed
- Retry classification is status-based: retry on 408, 409, 429, ≥500 and reqwest timeout/connect errors; never on other 4xx.
- `Retry-After` accepts integer-seconds and HTTP-date (RFC 7231) forms, clamped to [0, 60 s], used verbatim (no jitter) when `respect_retry_after` is set.
- Full jitter from an injectable RNG replaces the deterministic LCG jitter.

## [0.22.0] - 2026-07-15

### Fixed
- Retry: 5xx responses with non-JSON bodies are classified by HTTP status and retried instead of aborting the retry loop.
- Streaming: mid-stream `error` frames surface as stream errors instead of being dropped.
- Claude Code: error-subtype terminal events surface as errors instead of being dropped.
- CLI providers (`claude-code` / `codex-cli` / `gemini-cli`): child-process death mid-run surfaces as an error instead of a truncated success.
- OpenAI streaming: parallel tool calls are buffered per `tool_calls[].index` and flushed whole, so interleaved argument deltas no longer corrupt one another.
- chatgpt-codex: function-call events are correlated by `item_id` and emitted with the correct `call_id`.
- Streaming usage: later usage frames replace earlier fields instead of double-counting.

## 0.21.1 — 2026-06-13

### Added
- **`chatgpt-codex` default reasoning effort** — a provider-level fallback for `reasoning.effort`. `ChatGptCodexProvider::with_reasoning_effort(Some("high"))` / `ClientBuilder::chatgpt_codex_reasoning_effort("high")` (feature-gated `chatgpt-codex`) set a default emitted as `reasoning: {effort, summary:"auto"}` on every request that does not carry a per-request `provider_options["reasoning_effort"]` (the per-request value still wins). When neither is set the `reasoning` object stays off the body (unchanged behavior). The effort string is passed through verbatim — the backend validates it. The `chatgpt_codex(access_token, account_id, model)` signature is unchanged.

## [0.21.0]

### Added
- **`chatgpt-codex` provider** — `ChatGptCodexProvider`, a native ChatGPT-backend inference provider gated behind the `chatgpt-codex` feature. POSTs the OpenAI **Responses API** to `chatgpt.com/backend-api/codex/responses` with OAuth-token + `chatgpt-account-id` auth and the codex CLI headers, streaming typed `response.*` SSE (text, reasoning→thinking deltas, `function_call` tool lifecycle, `response.completed` usage). Construct directly via `ChatGptCodexProvider::new(access_token, account_id, model, base_url)` or `ClientBuilder::chatgpt_codex(access_token, account_id, model)`; new `Provider::OpenAiChatGpt` enum variant.

## 0.20.0 — 2026-06-10

### Added
- **CLI provider `cwd` setter** — `ClaudeCodeProvider::cwd(dir)` / `GeminiCliProvider::cwd(dir)` run the spawned child with `Command::current_dir`; `CodexCliProvider` already had `.cd()` (`--cd`).
- **CLI session continuity** — additive `StreamEvent::session_id` / `ChatResponse::session_id` surface the provider-minted session/thread id; `CodexCliProvider::resume(id)` runs `codex exec resume <id>`; Gemini `resume(id)` accepts a captured session id. (Additive, serde-skipped — no wire-format change.)
- **Per-run env injection** on all three CLI providers — `.env(k, v)` / `.envs(iter)` pass a per-run secret bundle to the child without mutating the parent env; values are redacted from `Debug` via the `RedactedEnvs` newtype.
- **CLI tool-call stream events** — `stream()` now surfaces CLI tool use as `ToolCallStart → ToolCallArgs → ToolCallEnd` (Claude `tool_use`; Codex `command_execution` / `mcp_tool_call`, the latter named `server/tool`; Gemini `tool_use`). Blocking `chat().tool_calls` stays empty.
- **Configurable per-invocation timeout** on all three CLI providers — `.timeout(dur)` / `.no_timeout()` (default = the prior per-provider const: Claude 300 s, Codex/Gemini 600 s), applied to both `chat()` and the `stream()` read loop; a per-line read-stall deadline yields `Err(MotosanError::StreamReadTimeout)`.

### Changed (BREAKING)
- `BoxStream` items are now `Result<StreamEvent, MotosanError>` instead of bare `StreamEvent`.
- `collect_stream()` now returns `Result<ChatResponse, MotosanError>` and propagates mid-stream provider errors.
- HTTP and CLI provider stream errors now surface as `Err(...)` items instead of being swallowed or converted to silent terminal events. Migration: `while let Some(ev) = stream.next().await { ... }` → `while let Some(item) = stream.next().await { let ev = item?; ... }`.

## 0.19.0 — 2026-06-02

### Changed
- Bumped the public `motosan-agent-primitives` dependency to 0.4.0 so bridge
  crates share the Reviewer-era primitive types. No SDK API shape changed.

## 0.18.0 — 2026-05-29

### Changed (BREAKING)
- `Tool` now composes `motosan_agent_primitives::ToolSchema` via
  `#[serde(flatten)]`; `description` and `input_schema` are no longer
  optional fields.
- Replaced `ChatRequestBuilder::tool_defs(&[ToolDef])` with
  `tool_schemas(&[ToolSchema])`.
- Removed the `agent-tool` feature, the optional `motosan-agent-tool`
  dependency, and the `ToolDef` compatibility conversions.

### Added
- New dependency on `motosan-agent-primitives` for the canonical tool schema.
- `Deref<Target = ToolSchema>` and `From<ToolSchema>` for `Tool`.

## 0.17.1 — 2026-05-29

### Added
- Anthropic model catalog now includes `claude-opus-4-8`.
- Live Opus 4.8 adaptive-thinking regression test (`tests/anthropic_live.rs::live_opus_4_8_adaptive_thinking`). Verified with an `sk-ant-oat01-*` OAuth token.

### Changed
- Anthropic extended thinking for Opus 4.8/4.7/4.6 now follows pi's adaptive-thinking shape (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) instead of the older budget-token shape. OAuth requests using adaptive thinking also omit the legacy `interleaved-thinking` beta header, matching pi's handling.

## 0.17.0 — 2026-05-29

BREAKING:
- motosan-agent-tool dep bumped to 0.5. `ToolDef` gained a required `internal_name: String` field (M10 D-M10-4). The `From<Tool> for ToolDef` conversion in `sdks/rust/src/tool_compat.rs` now goes through `ToolDef::new(...)`, which sets `internal_name = name` automatically — matching the SDK's "no host-side namespace" model. Round-trips `Tool → ToolDef → Tool` remain lossless. Consumers using `--features agent-tool` must bump their tool dep alongside.

NOTE: No public motosan-ai SDK signature changed. The only API surface touched is the test fixture in `tool_compat.rs` that struct-literal-constructs a `ToolDef` (the new `internal_name` field is now populated explicitly).

## 0.16.0 — 2026-05-26

BREAKING:
- motosan-agent-tool dep bumped to 0.4. Consumers using --features agent-tool must bump their tool dep alongside.

NOTE: No public SDK signature changed at the type level. Bump reflects transitive crate identity change.

## [0.15.5] - 2026-05-23

### Fixed

- **Anthropic provider sends `display: "summarized"` in the thinking config.** Without it the OAuth product surface (`sk-ant-oat01-*` tokens issued by Claude Code subscriptions) silently defaults the thinking display to `"omitted"` for all models — Anthropic accepts the request but returns zero `thinking_delta` SSE events. With the explicit `summarized` the OAuth tier behaves like direct API key callers and streams thinking content per-delta. Patch covers both non-streaming and streaming OAuth body builders (`sdks/rust/src/providers/anthropic.rs`). Verified end-to-end against `claude-sonnet-4-6` via a Claude Pro OAuth token.

## [0.15.4] - 2026-05-23

### Added

- **`StreamEventType::ThinkingDelta` and `StreamEventType::ThinkingDone`** plus matching `StreamEvent::thinking_delta(...)` / `StreamEvent::thinking_done(...)` constructors. (`sdks/rust/src/types.rs`.) Variant count goes 5 → 7. **Wire-breaking** for any downstream that does an exhaustive `match event.event_type { ... }` on `StreamEventType` without a `_ =>` arm; internal `collect_stream` and `codex_cli` test updated. Pre-1.0 we ship as patch.
- **Anthropic streaming thinking support.** `AnthropicStreamAdapter` (`sdks/rust/src/providers/anthropic.rs`) gains a `current_thinking_buf: Option<String>` accumulator. `content_block_start { type: "thinking" }` opens it. `content_block_delta { type: "thinking_delta", thinking: "..." }` accumulates the text **from `delta.thinking`** (a previous bug-by-omission read `delta.text` and silently dropped these) and emits `StreamEvent::thinking_delta`. `content_block_stop` for a thinking block emits `StreamEvent::thinking_done` carrying the full concatenated text and clears the accumulator. `signature_delta` and `redacted_thinking` blocks are silently consumed — no streaming surface for cryptographic re-feed signatures or redacted content, matching the non-streaming `ChatResponse.thinking` field's shape.
- **`collect_stream` populates `ChatResponse.thinking`** from accumulated `ThinkingDelta`s, preferring `ThinkingDone`'s authoritative payload when present. Streaming and non-streaming Anthropic responses now produce the same `ChatResponse.thinking: Option<String>` shape. (`sdks/rust/src/stream.rs`.)

### Notes

- Fourteen new tests across `types.rs`, `tests/anthropic_stream.rs`, and `src/stream.rs` lock the behavior in: variant existence + serde round-trip; constructor field shape; SSE → event mapping for thinking-only / thinking-then-text / redacted_thinking / orphan-delta / signature-delta cases; collect_stream accumulation including the `ThinkingDelta`-without-`ThinkingDone` fallback path.
- New `#[ignore]`'d live test `live_anthropic_streaming_thinking_emits_thinking_events` in `tests/anthropic_live.rs` hits the real API with `thinking(4000)` and asserts stream terminates, ThinkingDelta count > 0, ThinkingDone non-empty, answer non-empty, no content leak.
- Python SDK unchanged — Anthropic streaming thinking on the Python side is a separate plan (per `CLAUDE.md` "No FFI or shared code between SDKs"). Other providers (OpenAI, Gemini, MiniMax, Ollama, Codex CLI, Claude Code CLI) do not emit `StreamEventType::ThinkingDelta`/`ThinkingDone` — only Anthropic currently has a wire format for streaming extended thinking.

### Consumer impact

- Unblocks `motosan-agent-loop` v0.21.4's `TODO(thinking-stream)` markers at `src/motosan_ai_impl.rs:171` and `:346` — once that crate bumps its `motosan-ai` dep to `^0.15.4` and wires the two new arms, `CoreEvent::ThinkingChunk`/`ThinkingDone` will flow end-to-end from Anthropic SSE to consumers (capo TUI).

## [0.15.3] - 2026-05-17

### Fixed
- **`ThinkStripperStream` now flushes the tail buffer before forwarding a terminal `done` event.** `ThinkStripper::feed` always retains up to 6 trailing chars in case a split `<think>` open tag is arriving in pieces. The wrapper previously only called `flush()` on `Poll::Ready(None)`, but `collect_stream` (and any normal consumer) breaks on `event.done` first — so the flush was never observed. Anthropic / OpenAI dodged it because their SSE adapters emit many small text deltas; `Provider::ClaudeCode` triggered it sharply because it emits the entire assistant turn as one Text event followed by Done. Symptom: `pong` / `ok` / `yes` (≤6 chars) disappeared entirely; `Hello, world!` collapsed to `Hello, ` — last 6 chars always lost.

### Notes
- Single change in `client.rs::ThinkStripperStream::poll_next`: on a terminal `done` event, flush the stripper first; if the buffered tail is non-empty, queue the done event in a new `pending: Option<StreamEvent>` field and emit the tail as a Text event on the current poll. Mid-stream `Usage` events still pass through without flushing — flushing while the buffer holds e.g. `<thin…` would leak a partial open tag.
- Six unit tests in `think_stripper_stream_tests` lock in the regression: short reply survives, 6-char tail not lost, `<think>` still stripped when followed by Done, terminal `done` flag still observed, `Text → Usage → Done` (the actual claude-code wire order, per `providers/claude_code/stream_json.rs:91-104`) preserves the tail across the mid-stream Usage event, and `done_with_stop_reason` keeps its `stop_reason` field through the flush detour.
- `tests/client_builder.rs::integration_claude_code_short_reply_not_truncated` is a new `#[ignore]`'d live test that spawns the real `claude` CLI, sends a "reply with only 'pong'" prompt, and asserts the collected content contains `pong` end-to-end. Defends against future regressions in the spawn glue / NDJSON parser / stripper wrapper interplay.
- Python SDK (`sdks/python/motosan_ai/client.py:344-347`) already flushed-before-done correctly; no Python change needed (and `Provider::ClaudeCode` is Rust-only).

## [0.15.2] - 2026-05-17

### Fixed
- **`ollama_think("")` and `ollama_think("   ")` no longer emit `body["think"] = ""`** (which Ollama rejects as an invalid think value). Empty / whitespace-only inputs are now treated as if the field was never set, matching caller intent. Edge case left undocumented by the 0.15.1 parser fix. Closes the self-review followup from PR #178.

### Notes
- Single one-line guard in `providers/ollama.rs::build_request_body` (`if !trimmed.is_empty()` before the match block).
- New unit test `think_empty_or_whitespace_only_omits_field_entirely` covers 6 variants: empty, single space, multi-space, tab, newline, mixed whitespace.

## [0.15.1] - 2026-05-17

### Fixed
- **`ollama_think` now serializes per input value** instead of hard-coding `body["think"] = true` for any non-None input. Pre-existing bug from before 0.15.0: `ClientBuilder::ollama_think` takes a string but `providers/ollama.rs:138-140` was flattening `ollama_think("no")` to bool `true`, silently inverting caller intent. Now:
  - Truthy synonyms (`"true"` / `"yes"` / `"on"` / `"1"`, case-insensitive + trimmed) → JSON `true`
  - Falsy synonyms (`"false"` / `"no"` / `"off"` / `"0"`, case-insensitive + trimmed) → JSON `false`
  - Anything else (e.g. `"low"` / `"medium"` / `"high"`) → JSON string verbatim (so callers can opt into Ollama's newer string-valued reasoning levels)
- Backward compatible: existing `ollama_think("yes")` / `ollama_think("on")` callers still see bool `true` on the wire.

### Changed
- `.gitignore`: added macOS `.DS_Store` patterns + `.idea` / `.vscode` IDE caches. Pure repo hygiene.

### Notes
- Four unit tests in `providers::ollama::tests` lock in the new parser behavior.
- `tests/ollama_http_autoswitch.rs::live_ollama_think_string_parser_round_trip` is a new `#[ignore]`'d live test verifying the wire body is accepted by a real Ollama server.

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

## [0.14.3] - 2026-05-17

### Fixed
- **`Provider::ClaudeCode` `.stream()` emitted zero events under `claude` ≥ 2.1.x.** Two compounding bugs were silently producing empty streams for capo / motosan-agent-loop / any downstream stream consumer:
  1. **Missing `--verbose` flag.** Modern `claude` requires `--verbose` when combining `--print` with `--output-format=stream-json`; without it the CLI exits non-zero with `Error: When using --print, --output-format=stream-json requires --verbose` and emits no NDJSON. Fixed in `sdks/rust/src/providers/claude_code/mod.rs:396`.
  2. **Stale NDJSON parser.** Even with `--verbose` producing output, the parser in `claude_code/stream_json.rs` only matched the legacy `{"type":"text","text":"..."}` event shape. Modern `claude` emits assistant text inside `{"type":"assistant","message":{"content":[{"type":"text","text":"..."}]}}`, which the parser dropped as `Other`. Added an `Assistant` variant + `AssistantContentBlock` enum that walks `message.content[]`, extracts text from `text`-typed blocks, and skips `thinking` / `tool_use` via `#[serde(other)]`. Multiple text blocks in one assistant turn are concatenated.

### Added
- `tests/client_builder.rs::integration_client_dispatches_to_claude_code_stream` — live regression test (gated `#[ignore]`, requires `claude` binary + auth) that fails fast if either fix regresses.
- Three unit tests in `claude_code/stream_json.rs` covering the new assistant-event parsing (single text block, mixed thinking+text, tool-use-only).
- "Streaming vs Blocking" module-level documentation on both `providers::claude_code` and `providers::codex_cli`, clarifying that `.chat()` and `.stream()` spawn the CLI in different modes (unlike HTTP providers where they share an engine).

### Notes
- Investigation notes at `docs/superpowers/notes/2026-05-16-cli-provider-smoke-debug.md` with full repro commands.
- Codex side acquitted under motosan-ai's exact spawn args — emits the expected NDJSON shape. No codex-side change in this release.
- Deferred to a future release: `claude_code/mod.rs:452` silently discards the child process exit code via `let _ = child.wait().await`. If the stream is empty AND the child exited non-zero, we should yield a `StreamEvent::Error` — but that introduces a new event variant for callers to handle, so it's worth a small design conversation.

## [0.14.2] - 2026-05-16

### Added
- **`ClientBuilder::anthropic_base_url(...)`** — override the Anthropic base URL for staging, on-prem proxies, or Anthropic-compatible third-party endpoints. Defaults to `https://api.anthropic.com` when unset. Mirrors the existing `minimax_base_url` / `ollama_base_url` setters. The new value is forwarded to `AnthropicProvider::new(..., base_url)` in `build_anthropic_provider`.
- **`Client::anthropic_base_url() -> Option<&str>`** — getter that returns the override if one was set, `None` otherwise. Used by downstream consumers and the new round-trip test.

### Notes
- Purely additive: existing `Client::builder().provider(Provider::Anthropic).api_key(...).build()` callers see no behaviour change.
- Restores parity with M1-era capo and similar downstream wrappers that previously rolled their own `AnthropicProvider` constructor to support custom base URLs.

## [0.14.1] - 2026-04-25

### Fixed
- **Gemini default model** — `DEFAULT_GEMINI_MODEL` was `gemini-2.0-flash`, which Google deprecated for new users (returns HTTP 404: "This model models/gemini-2.0-flash is no longer available to new users"). Default bumped to `gemini-2.5-flash`. `GEMINI_MODELS` list reordered — 2.5 family first, 2.0 kept for back-compat with existing pinned callers. `tests/gemini_live.rs` no longer pins the deprecated model so it picks up the new default. README updated.

### Notes
- Verified against live API: all 7 `gemini_live.rs` tests pass with `GEMINI_API_KEY` set.
- Python SDK v0.8.2 carries the equivalent fix.

## [0.14.0] - 2026-04-21

### Breaking
- **`Provider::Minimax` now routes via Anthropic-compatible messages API** (`/anthropic/v1/messages`) using `AnthropicProvider` under the hood.
- **Removed `providers::minimax::MinimaxProvider`** and its legacy OpenAI-compatible `/chat/completions` path.
- **Removed `ClientBuilder::minimax_expose_reasoning(bool)`**.
- **Removed `DEFAULT_MINIMAX_MODEL`** export.

### Added
- **`ClientBuilder::minimax_base_url(...)`** for endpoint override (default: `https://api.minimax.io/anthropic`, CN: `https://api.minimaxi.com/anthropic`).
- **`AnthropicProvider::with_capabilities(...)`** for instance-level capability override.

### Changed
- MiniMax default model updated to `MiniMax-M2.7` (`MiniMax-M2.7-highspeed` also supported).
- `minimax` Cargo feature is now an alias to `anthropic` (`minimax = ["anthropic"]`).

### Tests
- Added `tests/anthropic_minimax_routing.rs` covering `/anthropic/v1/messages` routing and text-only capability validation.
- Updated builder/error/tool-use tests to remove legacy `MinimaxProvider` assumptions.

## [0.13.1] - 2026-04-20

### Added
- **`ProviderCapabilities`** type + constructors (`text_only()`, `with_image()`, `full()`).
- **`ProviderImpl::capabilities()`** default method (default: `text_only()`).
- **`ProviderImpl::validate_request()`** default method, now called before dispatch in `dispatch_chat` / `dispatch_stream_inner`.

### Changed
- **Framework-level multimodal validation**: unsupported image/document blocks now return `MotosanError::UnsupportedFeature(...)` before any network call, based on per-provider `capabilities()` declarations.

### Removed
- Internal `reject_document_blocks()` helper from provider modules (superseded by `validate_request()`).

### Tests
- Added Gemini vision mock tests (`tests/vision_gemini.rs`) and live image test (`tests/gemini_vision_live.rs`).

## [0.13.0] - 2026-04-20

### Added
- **`Provider::Gemini`** (feature `gemini`): native Google Generative AI HTTP provider (`generativelanguage.googleapis.com`) with chat/stream, tools, system blocks, image input, and retry integration.
- **`Provider::GeminiCodeAssist`** (feature `gemini-code-assist`, depends on `gemini`): native Google Cloud Code Assist provider (`cloudcode-pa.googleapis.com/v1internal`) using OAuth Bearer tokens and required GCP project ID (`ClientBuilder::gemini_code_assist_project_id(...)`).
- **Gemini OAuth config** in `motosan-ai-oauth` for PKCE token retrieval used by Gemini Code Assist.

### Notes
- `GeminiCodeAssist` is stream-only upstream; `chat()` is implemented as stream+collect.
- Gemini tool results require function name as `tool_call_id` (`functionResponse.name`).

## [0.12.1] - 2026-04-19

### Added
- **`ClaudeCodeProvider.bare` field + `.bare(bool)` builder** — forwards `--bare` to the spawned `claude` subprocess, which skips hooks, plugins, auto-memory, keychain reads, and user/project settings discovery. Intended for daemon / server embeddings that must not inherit the operator's interactive Claude Code state. Leave `false` (default) for workflows that should pick up `~/.claude/` configuration. Emitted in argv before `--dangerously-skip-permissions` so the two flags compose deterministically; order locked by `common_args_bare_precedes_agent_mode` and the full-loadout order test.

## [0.12.0] - 2026-04-15

### Breaking
- **`Provider` enum gained a new variant**: `Provider::GeminiCli`. Downstream code that exhaustively matches on `Provider` without a `_ =>` catch-all will no longer compile. Same mitigation as v0.11.0 — add a catch-all or handle the new variant.
- **`ClaudeCodeProvider` gained 19 new public fields** (`system_prompt`, `permission_mode`, `effort`, `fallback_model`, `add_dirs`, `allowed_tools`, `disallowed_tools`, `mcp_config`, `strict_mcp_config`, `settings`, `setting_sources`, `session_id`, `resume`, `continue_latest`, `fork_session`, `plugin_dirs`, `agent`, `no_session_persistence`, `max_budget_usd`). Struct-literal construction of `ClaudeCodeProvider { binary_path, agent_mode, model }` no longer compiles — use `ClaudeCodeProvider::new()` plus builder methods, which is what the README and docs have always recommended.
- **`claude_code::spawn::SpawnConfig` field rename**: `system_prompt` → `append_system_prompt`. The field is `pub` so direct users of `SpawnConfig` (rare — the struct is primarily an internal handoff) need to rename. A new `system_prompt` field now maps to `--system-prompt` (full replacement), distinct from append.

### Added
- **`ClaudeCodeProvider` argument surface expanded to match the `claude` CLI's SDK-relevant flag set.** The provider previously exposed only `binary_path` / `agent_mode` / `model`; this release adds builder methods for every flag that meaningfully controls a non-interactive `claude --print` session:
  - **Prompts**: `.system_prompt(...)` (`--system-prompt`, full replacement — coexists with the message-extracted `--append-system-prompt`).
  - **Permissions / effort**: `.permission_mode(PermissionMode::*)` (`--permission-mode`, 6 variants: `AcceptEdits` / `Auto` / `BypassPermissions` / `Default` / `DontAsk` / `Plan`), `.effort(EffortLevel::*)` (`--effort`, 4 variants: `Low` / `Medium` / `High` / `Max`).
  - **Model reliability**: `.fallback_model(...)` (`--fallback-model`).
  - **Workspace**: `.add_dir(path)` / `.add_dirs(vec)` (`--add-dir`, repeated).
  - **Tool control**: `.allow_tool(name)` / `.allowed_tools(vec)` (`--allowed-tools`, variadic), `.disallow_tool(name)` / `.disallowed_tools(vec)` (`--disallowed-tools`, variadic).
  - **MCP**: `.mcp_config(path_or_json)` / `.mcp_configs(vec)` (`--mcp-config`, variadic), `.strict_mcp_config(bool)` (`--strict-mcp-config`).
  - **Settings**: `.settings(path_or_json)` (`--settings`), `.setting_source(source)` / `.setting_sources(vec)` (`--setting-sources`, joined with commas).
  - **Session continuity**: `.session_id(uuid)` (`--session-id`), `.resume(value)` (`--resume`, accepts `"latest"` or a session ID), `.continue_latest(bool)` (`--continue`), `.fork_session(bool)` (`--fork-session`), `.no_session_persistence(bool)` (`--no-session-persistence`).
  - **Plugins & agents**: `.plugin_dir(path)` / `.plugin_dirs(vec)` (`--plugin-dir`, repeated), `.agent(name)` (`--agent`).
  - **Budget**: `.max_budget_usd(amount)` (`--max-budget-usd`, non-finite/negative values dropped at argv-build time).
- **New enums re-exported at the provider module root**: `motosan_ai::claude_code::{PermissionMode, EffortLevel}`. Both `#[derive(Debug, Clone, Copy, PartialEq, Eq)]`.
- **Refactor — `claude_code::spawn::common_args`**. The 3-flag argv wiring that used to live inline in both `invoke_cli` (blocking) and `ClaudeCodeProvider::stream` (streaming) is now a single pure `common_args(&SpawnConfig) -> Vec<OsString>` helper. Both paths call it after pushing their path-specific `--print` / `--output-format` prefix. This mirrors the Codex CLI / Gemini CLI provider layout and makes argv order test-coverable via `common_args_full_loadout_order_is_stable`.
- **24 new unit tests** under `providers::claude_code::spawn::tests` covering the new argv wiring: empty-config baseline, each permission-mode and effort-level variant, model / fallback model forwarding (with `default` / blank sentinel skip), system-prompt replacement + append interaction, add-dir / plugin-dir repeated flags, variadic allowed-tools / disallowed-tools / mcp-config with blank filtering, settings + setting-sources (including csv join with blank filtering), session-id / resume / continue / fork-session, budget and persistence flags (including negative / NaN / infinity skip), and a full-loadout order test that locks argv sequence against accidental reordering. Plus a `builder_methods_populate_spawn_config` round-trip test on `ClaudeCodeProvider` itself.
- **4 new live integration tests** (`#[ignore]`, gated on the installed `claude` binary) that actually spawn `claude --print` through `ClaudeCodeProvider` and verify each flag group end-to-end:
  - `integration_system_prompt_replacement` — `.system_prompt("Always reply with exactly one emoji, nothing else.")` forces an emoji-only reply; test asserts a short non-ASCII response. Proves `--system-prompt` actually shapes the model output, not just that the flag was accepted.
  - `integration_permission_effort_and_model_combo` — `.model("sonnet") + .permission_mode(PermissionMode::Plan) + .effort(EffortLevel::Low)` together on a plain Q&A, verifying three new enum-backed flags all coexist under `--print`.
  - `integration_workspace_and_budget_flags` — `.add_dir(tmp) + .no_session_persistence(true) + .max_budget_usd(2.5)` together, verifying workspace-root + session + budget flags survive argv construction.
  - `integration_tool_allow_deny_flags` — `.allow_tool("Edit").allow_tool("Read").disallow_tool("WebFetch")` verifying variadic `--allowed-tools` / `--disallowed-tools` argv encoding is accepted by Claude Code.
- All 5 Claude Code live tests (the 4 above + the pre-existing `integration_chat_roundtrip`) pass together in ~34s when run with `cargo test --features claude-code -- --ignored --test-threads=1`.

- **New CLI backend: `GeminiCliProvider`** (feature `gemini-cli`). Shells out to Google's `gemini -p "" -o stream-json` and parses the NDJSON event stream into the standard `ChatResponse` / `BoxStream` types. Lives in `providers/gemini_cli/` alongside the Claude Code / Codex CLI backends and implements the same `ProviderImpl` trait, so it's interchangeable via `Box<dyn ProviderImpl>`.
  ```rust
  use motosan_ai::gemini_cli::ApprovalMode;
  use motosan_ai::{Client, GeminiCliProvider, Message, Provider};

  let client = Client::builder()
      .provider(Provider::GeminiCli)
      .gemini_cli(
          GeminiCliProvider::new()
              .model("gemini-2.5-pro")
              .approval_mode(ApprovalMode::Yolo)
              .sandbox(true),
      )
      .build()?;  // no api_key needed — Gemini CLI uses local auth

  let response = client.chat(vec![Message::user("hi")]).await?;
  ```
- **New `ClientBuilder::gemini_cli(GeminiCliProvider)` setter** — accepts a pre-built provider instance so every provider-specific flag (model, yolo, sandbox, approval_mode) is reachable without adding dedicated builder methods. Defaults to `GeminiCliProvider::new()` with the top-level `.model()` forwarded when the setter is not called.
- **`api_key` optional for `Provider::GeminiCli`** — same relaxation v0.11.0 introduced for `ClaudeCode` / `CodexCli`. Gemini CLI handles its own auth (`gemini auth` once — personal Google account or API key).
- **`ApprovalMode` enum** (`Default` / `AutoEdit` / `Yolo` / `Plan`) mirrors Gemini CLI's `--approval-mode` choices. Re-exported from `motosan_ai::gemini_cli::ApprovalMode`. A `.yolo(true)` shorthand on `GeminiCliProvider` is also available for `--yolo`.
- **Workspace / extension / MCP / resume flags**: `.include_dir(path)` / `.include_dirs(vec)` (`--include-directories`), `.extension(name)` / `.extensions(vec)` (`-e`), `.allowed_mcp_server(name)` / `.allowed_mcp_servers(vec)` (`--allowed-mcp-server-names`), and `.resume("latest" | "5")` (`-r`). All four accept repeated flags, skip blank entries, and have a stable argv order locked by `common_args_full_loadout_order_is_stable`.
- **Argv layout**: `gemini -p "" -o stream-json [-m <model>] [--yolo] [--sandbox] [--approval-mode <mode>]`. The empty `-p` enables headless mode; the real prompt flows via stdin (Gemini CLI appends stdin to the `-p` value per `--help`), which matches how the Claude Code / Codex CLI providers hand off prompts. Avoids argv quoting and `ARG_MAX` footguns.
- **System prompts**: Gemini CLI has no `--system-prompt` flag, so `GeminiCliProvider` merges system text into the stdin payload as a blank-line-separated prefix. Matches how the CLI treats `GEMINI.md` context.
- **Streaming parser**: one NDJSON parser drives both `chat()` and `stream()`. Handles `init` (skipped), `message role:user` (stdin echo, skipped), `message role:assistant delta:true` (text chunk), and `result status:... stats:{...}` (usage + done). Non-`success` result statuses surface as `MotosanError::ProviderError`.
- **Usage mapping**: `stats.input_tokens` → `input_tokens`, `stats.output_tokens` → `output_tokens`, `stats.cached` → `cache_read_input_tokens`. Gemini CLI does not expose cache-creation tokens, so `cache_creation_input_tokens` is always `None`.
- **Env override**: `$GEMINI_CLI_PATH` points `GeminiCliProvider` at a non-default binary path, falling back to `"gemini"` in `$PATH`.
- **Unit tests**: 36 new tests under `providers::gemini_cli` covering argv construction (empty config, model forwarding, `default` sentinel handling, yolo / sandbox / approval mode flags, include-directories / extensions / allowed-mcp-server-names / resume forwarding + blank filtering, full loadout order), NDJSON parsing (assistant delta, user echo skip, non-delta skip, empty content skip, init skip, result with/without stats, error status, unknown types, malformed JSON), stream aggregation, and `ProviderImpl` dyn coercion.
- **Live integration test** (`#[ignore]`): `integration_chat_roundtrip` actually spawns `gemini` and verifies end-to-end that a turn comes back with `pong` in the content. Run with `cargo test --features gemini-cli -- --ignored`.

### Docs
- **Root `README.md`**: added a Gemini CLI row to the Providers table, bumped the Backends intro from "four ways" to "five ways", added a fifth `Client::builder()` example for Gemini CLI, updated the CLI backend limitations callout to include Gemini, and listed the new feature under Features.
- **`sdks/rust/README.md`**: new `## Gemini CLI Backend` section with Option A (via `Client::builder()`) + Option B (direct provider) examples and Notes covering argv layout, system prompt merging, streaming semantics, usage mapping, empty `tool_calls`, and model selection rules. Header tagline updated from "Claude Code CLI" to "Claude Code / Codex / Gemini CLIs".
- **`llms.txt`**: added `gemini-cli` row to the features comment block, added `GeminiCli` to the `Provider` variant list, added a Gemini CLI block to the CLI Backends dispatch example, expanded Key notes with Gemini's NDJSON schema, auth model, stats mapping, and system prompt merging behavior. Updated stale "v0.11.0" framing for CLI backends.
- **`skills/motosan-ai/SKILL.md`**: updated features comment, extended the CLI backends bullet and the Unified dispatch bullet to mention `GeminiCliProvider` / `Provider::GeminiCli` / `.gemini_cli(...)`.
- **`AGENTS.md`**: added `providers/gemini_cli/` to the CLI backends row in the Where To Find Things table; version bumped from v0.11.1 to v0.12.0.

### Notes
- Python SDK is unchanged (still v0.5.0). Gemini CLI backend is Rust-only for now; the Python side can follow using the same argv / NDJSON contract documented here if there's demand.
- Tool calls run inside Gemini CLI itself — `ChatResponse.tool_calls` is always empty on this backend, consistent with Claude Code / Codex CLI. Tool-loop use cases belong on the HTTP providers.

## [0.11.1] - 2026-04-15

### Docs
- **Root `README.md`**: added `Claude Code CLI` and `Codex CLI` rows to the Providers table; added a "Unified dispatch" bullet to the Features section highlighting that a single `Client::builder()` handles HTTP and CLI backends alike (since v0.11.0).
- **`skills/motosan-ai/SKILL.md`**: expanded the minimal Rust example with a CLI backend variant (`Client::builder().provider(Provider::CodexCli).codex_cli(...).build()?`) alongside the existing Anthropic example, so the skill teaches both paths.
- **`llms.txt`** § Rust API → Client: updated the `Provider` variant list from 4 to 6 (adds `ClaudeCode` / `CodexCli`); added a paragraph explaining that CLI backends dispatch through the same `client.chat()` / `client.stream()` API and that `api_key` is optional on the builder for those paths.

No code changes. Pure documentation patch on top of v0.11.0.

## [0.11.0] - 2026-04-14

### Breaking
- **`Provider` enum gained two new variants**: `Provider::ClaudeCode` and `Provider::CodexCli`. Downstream code that exhaustively matches on `Provider` without a `_ =>` catch-all will no longer compile.
- **Removed deprecated `*Client` type aliases** in `lib.rs`. `ClaudeCodeClient` and `CodexCliClient` were kept as `#[deprecated]` type aliases in v0.10.0 for the rename transition; they are now gone. Use `ClaudeCodeProvider` / `CodexCliProvider` directly.

### Added
- **CLI backends are now dispatchable through `Client::builder()`**, closing the gap left by v0.10.0's rename/relocate. Downstream consumers no longer need a separate code path for CLI vs HTTP backends — a single `Client` can hold either.
  ```rust
  use motosan_ai::codex_cli::SandboxMode;
  use motosan_ai::{Client, CodexCliProvider, Provider};

  let client = Client::builder()
      .provider(Provider::CodexCli)
      .codex_cli(
          CodexCliProvider::new()
              .sandbox(SandboxMode::WorkspaceWrite)
              .profile("work")
              .ephemeral(true),
      )
      .build()?;

  // Same unified API as HTTP providers:
  let response = client.chat(vec![Message::user("Hello")]).await?;
  ```
- **New `ClientBuilder` setters**: `.claude_code(ClaudeCodeProvider)` and `.codex_cli(CodexCliProvider)`. Both accept a pre-built provider instance so the full provider-specific API (sandbox / profile / add_dir / enable_feature / ...) is reachable without duplicating ~16 setters on `ClientBuilder`. If the setter is not called when the matching `Provider::*` variant is selected, a default `*Provider::new()` is used and the top-level `.model()` is forwarded.
- **`api_key` is now optional on `ClientBuilder::build()` when the selected provider is a CLI backend.** CLI backends authenticate via their own channels (local `claude` login state, `CODEX_API_KEY` env var, or `~/.codex/auth.json`). HTTP providers still require an `api_key` — a regression test guards this.
- **3 new client_builder unit tests**: `client_builder_allows_codex_cli_without_api_key`, `client_builder_allows_claude_code_without_api_key`, `client_builder_still_requires_api_key_for_http_providers`.
- **1 new live integration test** (`integration_client_dispatches_to_codex_cli`) that real-spawns `codex exec` through the `Client::builder().provider(Provider::CodexCli)` path end-to-end. Verifies the full dispatch chain, not just the struct coercion.

### Migration

**Exhaustive match on `Provider`** — add a catch-all or handle the new variants:
```rust
// Before
match provider {
    Provider::Anthropic => { ... }
    Provider::OpenAI => { ... }
    Provider::Minimax => { ... }
    Provider::Ollama => { ... }
}

// After (option A — catch-all)
match provider {
    Provider::Anthropic => { ... }
    Provider::OpenAI => { ... }
    Provider::Minimax => { ... }
    Provider::Ollama => { ... }
    _ => { /* handle CLI backends or ignore */ }
}

// After (option B — explicit)
match provider {
    Provider::Anthropic => { ... }
    Provider::OpenAI => { ... }
    Provider::Minimax => { ... }
    Provider::Ollama => { ... }
    Provider::ClaudeCode => { ... }
    Provider::CodexCli => { ... }
}
```

**Removed type aliases** — rename uses:
```rust
// Before (v0.10.x — compiles with a deprecation warning)
use motosan_ai::{ClaudeCodeClient, CodexCliClient};

// After (v0.11.0 — required)
use motosan_ai::{ClaudeCodeProvider, CodexCliProvider};
```

### Why
- v0.10.0 moved CLI backends into `providers/` and renamed them for structural consistency, but left `Client::builder()` still HTTP-only. Downstream consumers like `motosan-chat`'s `MotosanAiClient` had to maintain two separate construction paths. v0.11.0 delivers on the promise of v0.10.0 by making **any** provider (HTTP or CLI) selectable through a single `Client::builder()` call.
- Using a pre-built `CodexCliProvider` instance as the setter argument (rather than flattening all 13 codex flags into `ClientBuilder`) avoids adding 16+ new `codex_*` / `claude_code_*` setters while still giving callers the full configuration surface.
- Deprecated type aliases had their one-version grace period in v0.10.0. Removing them now keeps the public surface clean before v1.0.

### Tests
- 267 tests passing (was 264 in v0.10.1). +3 new client_builder unit tests. Live test count (ignored) goes to 5 (adds `integration_client_dispatches_to_codex_cli`).

## [0.10.1] - 2026-04-14

### Fixed
- **`OpenAIStreamAdapter` and `MinimaxStreamAdapter` now guarantee exactly one terminal `done` event**, even when the upstream provider closes the SSE connection without sending a `[DONE]` sentinel **and** without any `finish_reason` chunk. Previously such streams would terminate without ever yielding a `done==true` event, hanging callers that loop until `done` is true. Both adapters now track a `done_emitted: bool` and emit a final `done()` from the `Poll::Ready(None)` branch when needed. The `[DONE]` path also marks the flag so the EOF fallback can't double-emit.

### Added
- **EOF flush regression tests** for OpenAI and MiniMax (4 unit tests total): each provider gets one test covering the worst-case "no `finish_reason`, no `[DONE]`" SSE shape, plus one test that asserts `events.iter().filter(|e| e.done).count() == 1` for the fully-conformant shape (regression guard for the historical double-done bug fixed in v0.9.0).
- **`integration_chat_with_v0_9_2_flags` live test** for `CodexCliProvider` that real-spawns `codex exec` with `--add-dir`, `--enable fast_mode`, `--disable image_generation`, `--sandbox read-only`, and `--ephemeral` together. Catches flag-name regressions if a future Codex CLI release renames or removes any of them. The first iteration of this test failed against real codex 0.120.0 — codex validates feature names against a strict allowlist (`codex features list`) — which surfaced and corrected an incorrect assumption in the v0.9.2 docs.

### Changed
- **`codex_cli` module rustdoc example** changed from `ignore` to `no_run`, so the example is now compile-checked by `cargo test --doc`. The previous version used a non-existent `ChatRequestBuilder::new().user(...).build()` API; corrected to the real `ChatRequest::builder().message(Message::user(...)).build()` form.

### Tests
- 264 tests passing (was 259 in v0.10.0): +4 unit (EOF flush + double-done invariant) + 1 doc-test (now compile-checked instead of skipped). One additional ignored live test (`integration_chat_with_v0_9_2_flags`) brings the codex live test count to 4.

## [0.10.0] - 2026-04-14

### Breaking
- **CLI backend types renamed for naming consistency** with the HTTP providers (`AnthropicProvider`, `OpenAIProvider`, ...):
  - `ClaudeCodeClient` → **`ClaudeCodeProvider`**
  - `CodexCliClient` → **`CodexCliProvider`**
- **Source layout**: both CLI backends moved from top-level modules into `providers/` so every provider lives under one umbrella:
  - `sdks/rust/src/claude_code/` → `sdks/rust/src/providers/claude_code/`
  - `sdks/rust/src/codex_cli/` → `sdks/rust/src/providers/codex_cli/`
  - History preserved via `git mv`.

### Migration
The old type names are kept as `#[deprecated]` type aliases — existing code keeps compiling with a warning:

```rust
// v0.9.x — still works in 0.10.0 with a deprecation warning
use motosan_ai::CodexCliClient;
let c = CodexCliClient::new();

// v0.10.0 — recommended
use motosan_ai::CodexCliProvider;
let c = CodexCliProvider::new();
```

The aliases will be removed in a future release. Submodule re-exports (`motosan_ai::codex_cli::SandboxMode` etc.) are unchanged because they go through the `providers::*` re-export.

### Why
- After v0.9.1's `impl ProviderImpl for {CodexCliClient, ClaudeCodeClient}`, the only difference between HTTP providers and CLI backends was naming (`*Client` vs `*Provider`) and module path (top-level vs under `providers/`). Both differences were historical accidents from v0.6.0 / v0.7.0 when the CLI backends were deliberately built as standalone structs outside the trait hierarchy.
- v0.9.1 made them polymorphic. v0.10.0 makes them structurally identical to HTTP providers so future work (e.g. adding `Provider::CodexCli` enum variants, building `Client::builder().provider(...)` paths for CLI backends) is straightforward.
- The `CLAUDE.md` rule that previously read "HTTP provider logic goes in `providers/` only" was a post-hoc justification for the original split. Updated to reflect that **all** providers (HTTP + CLI) live in `providers/` now.

### Tests
- 259 tests passing (no count change from v0.9.2). Internal trait coercion tests use `crate::providers::ProviderImpl` (full path) since the `tests` submodule is nested one level deeper than the trait.

## [0.9.2] - 2026-04-14

### Added
- **Six new `CodexCliClient` builder methods** for `codex exec` flags that were previously only reachable via raw `config_override` strings:
  - `.add_dir(path)` — repeated `--add-dir <DIR>`, additional writable workspace roots.
  - `.enable_feature(name)` — repeated `--enable <FEATURE>`, equivalent to `config_override("features.<name>", "true")` but typed.
  - `.disable_feature(name)` — repeated `--disable <FEATURE>`.
  - `.dangerously_bypass_approvals_and_sandbox(bool)` — `--dangerously-bypass-approvals-and-sandbox`. Long name preserved intentionally; only safe inside an externally sandboxed environment.
  - `.oss(bool)` — `--oss`, use the local open-source provider stack instead of OpenAI cloud.
  - `.local_provider(LocalProvider)` — `--local-provider <p>`, picks `lmstudio` or `ollama` when `oss(true)` is set.
- **`LocalProvider` enum** (`LmStudio` / `Ollama`) re-exported from `motosan_ai::codex_cli::LocalProvider`.
- **Six matching public fields on `CodexCliClient`** so advanced callers can construct the struct directly.
- **Eight new argv-snapshot unit tests** covering each new flag in isolation plus a full-loadout test that locks the stable argv order across all 14 flag categories.

### Why
- After v0.7.0 only the most common subset of `codex exec` flags was wrapped (model / sandbox / profile / cd / ephemeral / agent_mode / config_override). Anything else required dropping into `-c key=value` config_override strings, which is awkward for typed users and bypasses TOML escaping rules.
- The 6 added flags are pure-config (string / bool / enum), so wiring them through `SpawnConfig` + `common_args` is mechanical.
- Multimodal `--image <FILE>` and `--output-schema <FILE>` are deferred — they need temp-file lifecycle handling and aren't in the immediate critical path.

### Coverage
- Every `codex exec` flag relevant to programmatic use is now reachable via a typed builder. Skipped flags are limited to: `--color` (irrelevant in JSON mode), `--output-last-message` (we read JSONL from stdout), `--image` and `--output-schema` (deferred).

## [0.9.1] - 2026-04-14

### Added
- **`CodexCliClient` and `ClaudeCodeClient` now implement `ProviderImpl`.** Both CLI backends were previously standalone structs with their own `chat()` / `stream()` inherent methods, leaving them inaccessible to any code that dispatches via `Box<dyn ProviderImpl>` or `&dyn ProviderImpl`. The trait impls forward to the existing inherent methods via fully-qualified call syntax (zero runtime overhead, zero behavior change), unlocking polymorphism for downstream consumers that want to treat HTTP and CLI backends uniformly.
- Two new compile-time + runtime trait coercion tests (`codex_cli_client_implements_provider_impl`, `claude_code_client_implements_provider_impl`) — they don't spawn a subprocess, just verify `Box<dyn ProviderImpl> = Box::new(client)` works.

### Why
- The original v0.6.0 design (when `ClaudeCodeClient` was added) deliberately kept CLI backends out of the trait hierarchy because CLI subprocess lifecycle differs from HTTP request/response. v0.7.0 (`CodexCliClient`) followed the same pattern.
- Real-world consumers (e.g. `motosan-chat` / `MotosanAiClient`) now want a single `Box<dyn ProviderImpl>` field that can hold either an HTTP provider or a CLI backend. The signatures already matched exactly — only the `impl` lines were missing.
- Pure additive change: existing `CodexCliClient::chat(req)` / `ClaudeCodeClient::chat(req)` calls still work; this just adds a second way to invoke them.

## [0.9.0] - 2026-04-14

### Added
- **`StreamEvent::stop_reason: Option<StopReason>`** — terminal stream events now carry the provider-reported stop reason. `None` on intermediate events; `Some(reason)` on the final `done` event when the provider supplies one.
- **`StreamEvent::done_with_stop_reason(reason)`** constructor for adapters that need to attach a stop reason to the terminal event.
- **All three HTTP providers propagate stop_reason through streams**:
  - **Anthropic**: `AnthropicStreamAdapter` captures `message_delta.delta.stop_reason` in adapter state, emits it on `message_stop`. Covers `end_turn` / `max_tokens` / `tool_use` / `stop_sequence` / unknown→`Other`.
  - **OpenAI**: `OpenAIStreamAdapter` stashes `choices[0].finish_reason`, emits exactly one terminal done event from the `[DONE]` sentinel (or end-of-stream EOF flush). Covers `stop` / `length` / `tool_calls`.
  - **MiniMax**: same logic as OpenAI, mapping inlined to keep `--features minimax` independent of `--features openai`.
- **`collect_stream` honors explicit stop reasons**: the existing `tool_calls.is_empty() ? EndTurn : ToolUse` heuristic is now a fallback only — used only when no provider reason was reported.

### Fixed
- **Double `done` event in OpenAI/MiniMax streams** (pre-existing bug, discovered by new live tests). Adapters used to emit two `done` events per stream — one on the `finish_reason` chunk (with stop_reason) and another on `[DONE]` (without). Callers using `events.last()` would receive the `stop_reason`-less copy. Streams now emit exactly one terminal `done` event with `stop_reason` attached. The `done` event count is asserted by new unit tests.
- **EOF flush fallback**: if a non-conformant OpenAI-compatible proxy ends the SSE stream without a `[DONE]` sentinel, the adapter now emits a final `done` event from the upstream `Poll::Ready(None)` branch, carrying any stashed `stop_reason`. Previously such streams would terminate without any `done` event at all.

### Changed
- **`StreamEvent` struct gained one public field** (`stop_reason`). Callers using struct literal construction (`StreamEvent { content: ..., done: ..., ... }`) need to add `stop_reason: None`. Callers using the constructor methods (`StreamEvent::text`, `done`, `usage`, `tool_call_*`) are unaffected.

### Tests
- 250 unit + integration tests passing (was 229 in v0.8.0).
- New mockito-based unit coverage for every stop reason variant across all three providers.
- New EOF-flush unit tests for OpenAI and MiniMax (fixture omits `[DONE]`).
- New live integration tests against real APIs (`anthropic_live.rs`, `openai_live.rs`, `minimax_live.rs`) — each forces `max_tokens=8` to trigger truncation and asserts the explicit `MaxTokens` reason flows through both the terminal stream event and the `ChatResponse` returned by `collect_stream`. All three providers verified end-to-end against production endpoints.

## [0.8.0] - 2026-04-14

### Breaking
- **`OpenAIProvider` URL configuration redesigned.** The `base_url` parameter is replaced by two independent, full-URL fields — `chat_url` and `responses_url` — set via builder methods. No more `/v1/chat/completions` auto-injection or `strip_suffix("/chat/completions")` heuristics. What you pass is what gets POSTed.
  - `OpenAIProvider::new(api_key, model, base_url)` → `OpenAIProvider::new(api_key, model)` (third parameter dropped).
  - New builder methods: `.with_chat_url(url)` and `.with_responses_url(url)`. Both trim a single trailing slash defensively; no other normalization.
  - Defaults: `DEFAULT_OPENAI_CHAT_URL = "https://api.openai.com/v1/chat/completions"`, `DEFAULT_OPENAI_RESPONSES_URL = "https://api.openai.com/v1/responses"` (exported).
  - `ClientBuilder` gains `.openai_chat_url(url)` and `.openai_responses_url(url)` setters (previously there was no way to point the OpenAI provider at a different host via `ClientBuilder` at all).
  - Internal `fn endpoint()` and `fn responses_endpoint()` deleted — providers now read `&self.chat_url` / `&self.responses_url` directly.

### Migration

```rust
// Before (v0.7.0)
OpenAIProvider::new(api_key, None, Some("https://api.groq.com/openai".to_string()))
// worked by accident because the code appended "/v1/chat/completions"

// After (v0.8.0)
OpenAIProvider::new(api_key, None)
    .with_chat_url("https://api.groq.com/openai/v1/chat/completions")
```

```rust
// Before
OpenAIProvider::new(api_key, None, None)   // defaults to https://api.openai.com
// After
OpenAIProvider::new(api_key, None)          // defaults to full OpenAI chat URL
```

Ollama integration wires `ollama_base_url` into `.with_chat_url()` internally — no change for `Client::builder().provider(Provider::Ollama)` users.

### Why

- The old heuristics silently broke for `base_url` values that already contained `/v1` (e.g. `https://api.groq.com/openai/v1` produced `.../v1/v1/chat/completions`).
- Passing a full endpoint URL (custom proxies, non-standard paths) was impossible without `strip_suffix` gymnastics.
- `endpoint()` and `responses_endpoint()` had asymmetric logic — one had a 3-branch heuristic, the other didn't — making debugging painful.
- Two independent URL fields match the `openai-python` / `openai-node` mental model: callers own the URL, the SDK just POSTs.

### Changed
- **Tests**: 28 `OpenAIProvider::new(key, model, Some(server.url()))` call sites across 7 integration test files migrated to the new `.with_chat_url(format!("{}/v1/chat/completions", server.url()))` form. The `openai_endpoint_normalizes_trailing_slash_base_url` test is renamed to `openai_with_chat_url_trims_trailing_slash` and now exercises `.with_chat_url()`'s defensive `trim_end_matches('/')`.
- **Ollama integration** (`Client::builder().provider(Provider::Ollama)`): internal wiring now computes `{ollama_base_url}/v1/chat/completions` and passes it to `.with_chat_url()`. No caller-visible change.

### Docs
- `sdks/rust/README.md` § OpenAI Provider Options — full rewrite with Groq / self-hosted proxy examples, `with_chat_url` / `with_responses_url` semantics, `ClientBuilder` setter usage.
- Root `README.md` — new blockquote under Providers table showing `.openai_chat_url(...)` for Groq / DeepSeek / Together / proxies.
- `llms.txt` § OpenAI — expanded `openai_chat_url` / `openai_responses_url` examples, documented `DEFAULT_OPENAI_CHAT_URL` / `DEFAULT_OPENAI_RESPONSES_URL` constants.
- `skills/motosan-ai/SKILL.md` — provider list amended; Key Design Decisions gains a bullet explaining the full-URL, no-`/v1`-injection policy.

## [0.7.0] - 2026-04-14

### Added
- **`codex-cli` feature**: `CodexCliClient` — shells out to OpenAI's `codex exec --json` as a fifth LLM backend, alongside the four HTTP providers and `ClaudeCodeClient`.
  - `CodexCliClient::new()` resolves the binary from `CODEX_PATH` env or `"codex"` in `PATH`.
  - `CodexCliClient::chat(request)` — spawns `codex exec --json --skip-git-repo-check -`, writes the prompt to stdin, parses the JSONL event stream, and returns a `ChatResponse`. Treats the last `agent_message` as `content` and folds prior agent messages (preamble / tool narration) into `thinking`.
  - `CodexCliClient::stream(request)` — same spawn, yields `StreamEvent`s as Codex emits them. Codex produces complete `agent_message` items (not token deltas), so each text event is one finalized message.
  - Builder flags: `.model(m)` (`--model`), `.sandbox(SandboxMode)` (`--sandbox`), `.profile(name)` (`--profile`), `.ephemeral(bool)` (`--ephemeral`), `.cd(dir)` (`--cd`), `.agent_mode(bool)` (`--full-auto`), `.config_override(key, value)` (repeatable `-c key=value`).
  - `SandboxMode` enum: `ReadOnly` / `WorkspaceWrite` / `DangerFullAccess`.
  - 600-second hard timeout on subprocess invocation, `kill_on_drop` for cancel-safety.
- **Comprehensive rustdoc** for the `codex_cli` module: module-level overview, per-field docs on `CodexCliClient`, error contracts on `chat` / `stream`, full event-schema documentation on `stream_json.rs`.

### Limitations
- `CodexCliClient` does not surface `tool_calls` — Codex runs shell, file edits, and MCP tools inside its own sandbox; those invocations are not reported as crate-level tool calls.
- Only `codex exec` is supported. `codex exec resume` (session continuation) and `codex review` are out of scope.
- Codex CLI has no native `--system` flag; system prompts are prepended to the user prompt as a labeled `[system instructions]` block.

## [0.6.0] - 2026-04-05

### Added
- **`claude-code` feature**: `ClaudeCodeClient` — shells out to the `claude` CLI binary as a fourth LLM backend.
  - `ClaudeCodeClient::new()` resolves binary from `CLAUDE_CODE_PATH` env or `"claude"` in `PATH`.
  - `ClaudeCodeClient::chat(request)` — blocking subprocess via `--print`, supports `agent_mode` with JSON output parsing.
  - `ClaudeCodeClient::stream(request)` — NDJSON streaming via `--print --output-format stream-json`, yields `StreamEvent` items.
  - `.model(model)` builder: forwards `--model <value>` when non-empty and not `"default"` (case-insensitive); skips otherwise.
  - `.agent_mode(bool)` builder: enables `--dangerously-skip-permissions`.
  - Resolves binary path from `CLAUDE_CODE_PATH` env var with fallback to `"claude"`.

### Changed
- `DEFAULT_MAX_TOKENS` raised from `4096` to `8192` for the Anthropic provider.

## [0.5.4] - 2026-03-31

### Changed
- Upgrade `motosan-agent-tool` dependency from 0.2 to 0.3.

## [0.5.3] - 2026-03-30

### Fixed
- Fix `cargo fmt` formatting in `client.rs` that blocked CI publish for v0.5.2.

## [0.5.2] - 2026-03-30

### Added
- Configurable **stream read timeout** via `ClientBuilder::stream_read_timeout_secs(secs)` — terminates SSE streams that stop sending events mid-stream, preventing indefinite hangs (#155).
- `MotosanError::StreamReadTimeout` error variant for timeout-specific error handling.

### Fixed
- `ThinkStripper`: split on UTF-8 char boundaries to avoid panic on multi-byte characters.

## [0.5.1] - 2026-03-24

### Fixed
- Merge `anthropic-beta` headers into a single header when OAuth + MCP are both active (#149).
- `has_mcp` now checks both `mcp_servers` and `mcp_tool_configs` (#150).
- `mcp_toolset` serialization uses `mcp_server_name` instead of `server_label` (#153).

## [0.5.0] - 2026-03-24

### Added
- `agent-tool` feature gate with `motosan-agent-tool` integration (`From<ToolDef> for Tool`, optional dependency).
- `collect_stream()` helper and `Client::stream_collect` methods for buffering stream into `ChatResponse`.
- `ToolChoice` enum for controlling tool selection (`Auto`, `Any`, `None`, `Specific`).
- First-class extended thinking support in `ChatRequest`.
- Server-side MCP support in `ChatRequest`.

### Fixed
- Capture usage tokens from stream events in OAuth collect path.
- Fail-fast on missing `tool_call_id` + clarify Null args handling.

## [0.4.0] - 2026-03-21

### Added
- **Vision / Multimodal content support** — send images alongside text in messages
  - `ContentBlock` enum: `Text { text }` and `Image { source }` variants
  - `ImageSource` enum: `Base64 { media_type, data }` and `Url { url }` variants
  - `Message::user_with_image(text, base64_data, media_type)` — create a message with text + base64 image
  - `Message::user_with_blocks(blocks)` — create a message with arbitrary content blocks
  - `Message.content_blocks: Vec<ContentBlock>` field (backward compatible, defaults to empty)
- **Anthropic provider**: serializes `content_blocks` as `{"type": "image", "source": {"type": "base64", ...}}` format (works with both API key and OAuth streaming path)
- **OpenAI provider**: serializes `content_blocks` as `{"type": "image_url", "image_url": {"url": "data:...;base64,..."}}` format

### Fixed
- **Anthropic OAuth streaming path**: content_blocks now correctly serialized in the OAuth streaming code path (previously only the non-streaming path handled them)

## [0.3.3] - 2026-03-18

### Fixed
- **Anthropic OAuth `chat()` tool_calls**: OAuth path now correctly collects `ToolCallStart`/`ToolCallArgs`/`ToolCallEnd` stream events into `ChatResponse.tool_calls` (previously returned empty)
- **Anthropic OAuth system prompt**: system prompt now sent as separate blocks (Claude Code prefix with `cache_control` + user system without) instead of merged single block (fixes `invalid_request_error`)
- **Mock test header matching**: OAuth `anthropic-beta` header uses regex match instead of exact string

### Added
- **Live integration tests** (`tests/anthropic_live.rs`): 7 tests hitting real Anthropic API with OAuth token — chat, stream, system prompt, temperature, tool use (single + multi-turn), stream + tool use
- **Pre-push gate** (`scripts/pre-push-gate.sh`): blocks push unless unit + live tests pass

## [0.2.0] - 2026-03-15

### Added
- **Ollama provider** (`ollama` feature): connect to local or remote Ollama instances
  - Phase 1: `Provider::Ollama` via OpenAI-compatible endpoint (`/v1/chat/completions`)
  - Phase 2: `OllamaProvider` native implementation using `POST /api/chat` with NDJSON streaming
  - `think` mode: enable reasoning on qwen3-thinking, deepseek-r1 and other thinking models
  - `keep_alive`: control how long the model stays loaded in VRAM
  - `num_ctx`: override context window size via Modelfile options
  - `ollama_base_url()` builder: point to remote Ollama instance
  - `ollama_native(true)` builder: switch to native `/api/chat` endpoint
  - `ollama_think()`, `ollama_keep_alive()`, `ollama_num_ctx()` builder methods
  - `NdjsonStream`: custom `futures::Stream` adapter for NDJSON line parsing
  - Tool calls: auto-generates `call_{idx}` id when Ollama native omits it
  - `DEFAULT_OLLAMA_MODEL = "llama3.2"` in `models.rs`
- `feature = "full"` now includes `ollama`

## [0.1.4] - 2026-03-15

### Added
- `Client::stream_with(request: ChatRequest)` — stream with full `ChatRequest` (system, max_tokens, tools, temperature)

### Fixed
- Anthropic provider: `max_tokens` now defaults to `4096` when not set (Anthropic API requires this field; previously caused HTTP 400)


## [0.1.3] - 2026-03-11

### Added
- Multi-turn tool use support (fixes #72–#75):
  - `Message.tool_calls: Vec<ToolCall>` — carry tool calls in assistant messages
  - `Message::assistant_with_tool_calls()` constructor
  - `Message::tool_result()` / `Message::tool()` constructors for `Role::Tool`
- Anthropic: serialize assistant `tool_use` blocks in multi-turn requests
- OpenAI/MiniMax: serialize assistant `tool_calls` field in multi-turn requests

### Fixed
- Multi-turn tool use conversations now correctly reconstruct conversation history

## [0.1.1] - 2026-03-11

### Added
- MiniMax compatibility improvements:
  - Migrated to OpenAI-compatible MiniMax endpoint (`/chat/completions`).
  - Added payload-level `base_resp` error mapping with better auth/rate-limit/request semantics.
  - Added optional reasoning exposure control and default `<think>...</think>` stripping.
  - Added fallback to `reasoning_content` for chat and stream parsing.
  - Merged MiniMax system prompts into first user message for better endpoint compatibility.
- OpenAI provider enhancements:
  - Structured stream error parsing and empty-stream-chunk suppression.
  - `reasoning_content` fallback for chat and stream parsing.
  - Configurable auth style (`Bearer`, `x-api-key`, custom header).
  - Optional `/v1/responses` fallback when `/v1/chat/completions` returns `404`.
- `ClientBuilder` OpenAI options:
  - `openai_auth_bearer`, `openai_auth_x_api_key`, `openai_auth_custom_header`.
  - `openai_responses_fallback`.

### Changed
- Updated MiniMax default model to `MiniMax-M2.5-highspeed`.
- Expanded `MINIMAX_MODELS` catalog with M2.5/M2.1/M2 family entries.
- Expanded Rust README with OpenAI and MiniMax advanced behavior/configuration notes.

## [0.1.0] - 2026-03-10

### Added
- Feature-gated provider support: Anthropic, OpenAI, MiniMax (`anthropic`, `openai`, `minimax`, `full`).
- Unified core types: `Message`, `ChatRequest`, `ChatResponse`, `Usage`, `StopReason`, `StreamEvent`.
- `Message` helper constructors and `ChatRequestBuilder` for ergonomic request construction.
- `Client` APIs: `chat`, `chat_with`, and `stream`.
- Provider implementations for chat + streaming on Anthropic/OpenAI/MiniMax.
- Shared provider mapping utilities and robust SSE parsing behavior.
- Integration tests for provider happy paths, streaming behavior, and error mapping.
- Configurable retry policy (`RetryPolicy`) with exponential backoff, optional jitter, and `Retry-After` support.
- Rust CI workflow (`fmt`, `clippy`, `test`).

### Changed
- Centralized model defaults and model catalog in `src/models.rs`.
- Migrated SDK error type to `thiserror`-based `MotosanError`.
- Set MSRV to Rust `1.82` and added CI lane to validate no-feature builds/tests.

### Notes
- Default model baselines are maintained in `src/models.rs` and can be overridden via `ClientBuilder::model(...)` or `ChatRequest::builder().model(...)`.

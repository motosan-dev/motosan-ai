# Changelog

All notable changes to `motosan-ai` Python SDK are documented in this file.

## [0.17.0] - 2026-07-17

### Breaking
- Stream EOF semantics: an HTTP provider stream that ends without the provider's terminal event now raises `IncompleteStreamError` — `"incomplete stream: <provider> ended without a terminal event"` — instead of ending as if the turn had completed. OpenAI-wire streams (openai, minimax) complete on `[DONE]` or a `finish_reason`-bearing chunk — either suffices; truncation with neither signal raises. Anthropic requires `message_stop`; Gemini / chatgpt-codex require their terminal frames. `IncompleteStreamError` subclasses `StreamError`, so existing `except StreamError:` handlers keep working unchanged; catch `IncompleteStreamError` first to treat truncation specially.

### Added
- `Client(..., connect_timeout=10.0, read_idle_timeout=120.0, total_timeout=None)` is threaded into every HTTP provider as `httpx.Timeout(connect=connect_timeout, read=read_idle_timeout, write=read_idle_timeout, pool=connect_timeout)`; `total_timeout` applies to blocking `chat()` / `chat_with()` only, never silently to streams.
- `StreamReadTimeoutError` (`MotosanError` subclass; mirrors Rust `StreamReadTimeout` / TypeScript `StreamReadTimeoutError`): a streaming body idle past `read_idle_timeout` raises it. Never retried — mid-stream retry would replay already-yielded deltas.
- `Client.aclose()` and `async with Client(...) as client:` close every provider `AsyncClient`.
- `cli_timeout` facade kwarg (keyword-only on `Client`; trailing parameter on `Client.codex_cli()` / `Client.gemini_cli()` — pass it by keyword): threads to `CodexCliClient` / `GeminiCliClient` `.timeout()`; `cli_timeout=None` maps to `.no_timeout()`.

### Changed
- **MiniMax timeout unified**: the hardcoded `httpx.AsyncClient(timeout=30)` outlier now uses the shared model (connect 10s, read/write 120s) - requests that previously failed at 30s idle now wait 120s.
- Default connect timeout tightened from 120s (blanket `timeout=120.0`) to 10s across all HTTP providers.
- A streaming-phase `httpx.ReadTimeout` now raises `StreamReadTimeoutError` instead of `StreamError("stream transport error: ...")` (anthropic/openai/gemini/gemini_code_assist/chatgpt_codex) or `NetworkError`/`StreamError` (minimax/ollama).
- Non-2xx error-body reads in `stream()` now sit inside the same ReadTimeout-mapping/cleanup scope as the SSE loop. gemini previously ran `await resp.aread()` before its `try`/`finally` - a `ReadTimeout` there escaped as a raw httpx exception and the response was never closed on error statuses (leak now fixed); gemini_code_assist/chatgpt_codex ran it outside the inner catch chain (raw `ReadTimeout` escaped, response was closed). All three now raise `StreamReadTimeoutError` and always close the response.

## [0.16.0] - 2026-07-16

### Added
- `MotosanError` gains keyword-only `status_code`, `retry_after`, `request_id` attributes (default `None`); subclasses inherit them, providers populate them at raise sites, and `request_id` comes from the `request-id` / `x-request-id` response headers. Additive — the M1 `"HTTP {status}: ..."` message prefixes remain.
- `RetryPolicy` dataclass in `motosan_ai.retry` (`max_retries=3`, `base_delay=0.1`, `max_delay=2.0`, `jitter=True`, `respect_retry_after=True`, `on_retry=None`); `with_retry(fn, policy=...)` accepts it and the old kwargs keep working. `Client` threads a policy through both chat and stream paths; the stream path's hand-rolled backoff now uses the shared policy math.
- `on_retry` observer: `RetryEvent(attempt, delay, cause)` fired before each retry sleep.
- Cross-SDK `specs/retry.md` conformance suite: `tests/test_retry_conformance.py`.

### Changed
- Retry classification is attribute-based: `RateLimitError` / `NetworkError` always retryable; `ProviderError` retryable when `status_code` is 408, 409, or ≥500. The `\b5\d{2}\b` message regex and message-scraped `Retry-After` parsing are removed — the delay now comes from `error.retry_after`.
- `Retry-After` accepts integer-seconds and HTTP-date forms (`email.utils.parsedate_to_datetime`), clamped to [0, 60 s], used verbatim (no jitter).
- Full jitter from an injectable `rng` callable (default `random.random`) replaces the deterministic LCG jitter.

## [0.15.0] - 2026-07-15

### Fixed
- Retry: 5xx responses with non-JSON bodies are classified by HTTP status and retried instead of aborting the retry loop.
- Streaming: mid-stream `error` frames raise `StreamError` instead of being dropped.
- Claude Code: `is_error` terminal results raise `StreamError` instead of being dropped.
- CLI providers (`claude_code` / `codex_cli` / `gemini_cli`): child-process death mid-run surfaces as an error instead of a truncated success.
- OpenAI streaming: parallel tool calls are keyed by `tool_calls[].index` (ports the TypeScript adapter), so parallel calls are no longer dropped or merged.
- chatgpt-codex: function-call events are correlated by `item_id` and emitted with the correct `call_id`.
- Streaming: turns that emit tool calls now finish with the tool-use stop reason instead of a generic end-of-turn.

## [0.14.0] - 2026-06-23

### Added
- **ChatGPT-backend Codex provider** (`ChatGptCodexProvider`, `Provider.openai_chatgpt`,
  `Client.chatgpt_codex(access_token, account_id, model, reasoning_effort=None)`): native inference
  against the OpenAI **Responses API** at `https://chatgpt.com/backend-api/codex/responses` using a
  pre-obtained ChatGPT OAuth bearer token + `chatgpt-account-id` + the codex CLI headers. Streams typed
  `response.*` SSE events (text, reasoning → thinking, function-call tool lifecycle, usage, terminal stop
  reason). Text-only (`ProviderCapabilities.text_only()`); no `api_key` required. Reasoning effort via
  per-request `provider_options["reasoning_effort"]` or a provider-level default
  (`ChatGptCodexProvider.reasoning_effort(...)`). Mirrors the Rust `ChatGptCodexProvider`.

## [0.13.0] - 2026-06-23

### Added
- **CLI working-directory setter** — `ClaudeCodeClient.cwd(dir)` and `GeminiCliClient.cwd(dir)` run the spawned subprocess in `dir`. (Codex uses its existing `cd()`/`--cd` flag.)
- **Session continuity** — `StreamEvent` and `ChatResponse` gain an additive `session_id: str | None = None`. CLI providers read it back (Claude `result.session_id`, Codex `thread.started.thread_id`, Gemini `init.session_id`); `collect_stream` captures it. `CodexCliClient.resume(thread_id)` resumes a thread (`codex exec resume <id>`); `GeminiCliClient.resume()` forwards `--resume`.
- **Per-run env injection** — `.env(key, value)` (append) / `.envs(map)` (replace) on the three CLI providers inject secrets into the child env, merged over a copy of `os.environ` (parent process env is never mutated). Values are redacted from `repr` (`<N redacted>`).
- **CLI tool-call stream events** — `stream()` on the three CLI providers now surfaces `tool_call_start → tool_call_args → tool_call_end` for tool wire events (Claude `tool_use`; Codex `command_execution` / `mcp_tool_call`, MCP name `server/tool`; Gemini `tool_use`). The terminal `stop_reason` becomes `tool_use` when a tool call was seen. Blocking `chat().tool_calls` stays empty for CLI backends.
- **Configurable timeout** — `.timeout(secs)` / `.no_timeout()` on the three CLI providers. Defaults: Claude 300s, Codex 600s, Gemini 600s. The stream read loop enforces a per-read stall deadline that raises `ProviderError` on stall; Codex and Gemini `stream()` gained a per-read deadline they previously lacked.

### Changed
- **BREAKING — fallible stream.** Every HTTP provider's `stream()` (`anthropic`, `openai`, `gemini`, `gemini_code_assist`, `minimax`, `ollama`) now RAISES `motosan_ai.error.StreamError` on a malformed SSE/NDJSON frame or a mid-stream transport fault, instead of silently swallowing it and ending. `collect_stream` propagates the raise. `Client.stream_with` no longer retries after a mid-stream raise (it would replay already-yielded deltas); only pre-first-yield connection errors are retried. `StreamError` is non-retryable. Callers that relied on a stream silently ending on error must now handle `StreamError`.

## [0.12.1] - 2026-05-29

### Added
- Live Opus 4.8 adaptive-thinking regression test (`tests/integration/test_anthropic_live.py::test_live_opus_4_8_adaptive_thinking`). Verified with an `sk-ant-oat01-*` OAuth token.

### Changed
- Anthropic extended thinking for Opus 4.8/4.7/4.6 now follows pi's adaptive-thinking shape (`thinking.type = "adaptive"`, summarized display, `output_config.effort = "high"`) instead of the older budget-token shape. OAuth requests using adaptive thinking also omit the legacy `interleaved-thinking` beta header, matching pi's handling.
- Budget-based Anthropic thinking now sends `display: "summarized"` so Python matches Rust/pi behavior for OAuth thinking streams.

## [0.12.0] - 2026-05-18

### Added
- Anthropic Claude Pro/Max OAuth: `motosan_ai.oauth.claude_pro_max_config()`
  plus `login()` / `refresh_token()` yield an `sk-ant-oat01-*` token usable
  directly with `AnthropicProvider`. See the README for the ToS disclosure.
- `OAuthConfig` gained `callback_path`, `redirect_uri_host`, `token_body`,
  `extra_auth_params`, and `state_strategy` fields, plus `TokenBodyFormat`
  and `StateStrategy` enums.

### Changed
- **Breaking:** `motosan_ai.oauth` no longer exports `google_gemini_config`;
  use `gemini_config` instead. The `oauth/` package was refactored from a
  single `google.py` module into a generic core plus per-provider config
  modules (`providers/gemini.py`, `providers/anthropic.py`).
- **Breaking:** `exchange_code()` gained a required `state` keyword argument
  (the `state` value is now echoed in the token-endpoint POST body, which
  Anthropic requires).

## [0.11.0] - 2026-04-27

### Removed (BREAKING)
- **`Client.chat_sync()`** — removed per the v0.10.0 deprecation notice. The SDK is async-only; callers should wrap `await client.chat(...)` in `asyncio.run()` from synchronous contexts. See `sdks/python/README.md#sync-usage`.

### Notes
- No other surface changes; this is a single-method removal release.
- Migration: `client.chat_sync(messages, **kwargs)` → `asyncio.run(client.chat(messages, **kwargs))`.

## [0.10.0] - 2026-04-26

### Added — Client API parity with Rust SDK (Phase 4)
- **`Client.chat_with(request: ChatRequest)`** — full ChatRequest passthrough. Use with `ChatRequest.builder()` for Phase 1 fields like `tool_choice`, `thinking`, `mcp_servers`, `system_blocks`, and `stop_sequences`.
- **`Client.stream_with(request: ChatRequest)`** — full ChatRequest passthrough for streaming with the same retry semantics as `stream()`.
- **`Client.stream_collect(messages, **kwargs)`** — drives a stream to completion and returns the assembled `ChatResponse`; convenience wrapper around `stream() + collect_stream()`.
- **`Client.stream_collect_with(request: ChatRequest)`** — streaming + collecting with full ChatRequest control.
- **`motosan_ai.collect_stream(events) -> ChatResponse`** — top-level helper for callers who want stream-to-response assembly without going through `Client`. Handles text, thinking, tool calls (start/args/end), usage, and stop reason.

### Changed
- `Client.chat()` and `Client.stream()` now delegate to `chat_with()` / `stream_with()` internally. No behavior change for existing callers.
- Both `*_with` methods fall back to `client.model` when `request.model` is None (matches Rust precedence).

### Deprecated
- **`Client.chat_sync()`** — emits `DeprecationWarning`. Wrap `await client.chat(...)` in `asyncio.run()` instead. Will be removed in v0.11.0.

### Notes
- Phase 4 closes the Rust-parity roadmap. Python SDK is now method-for-method aligned with `motosan-ai` Rust v0.14.x at the Client layer.
- See `docs/superpowers/plans/2026-04-26-python-sdk-phase4-client-api-parity.md` for the per-task TDD breakdown.

## [0.9.3] - 2026-04-25

### Added — `GeminiCodeAssistProvider` + Google OAuth (Phase 3d)
- **`GeminiCodeAssistProvider`** — new HTTP provider targeting `cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse`.
  - Wraps the existing Phase 2b `GeminiProvider._build_body` output in the Code Assist envelope (`{project, model, request, userAgent, requestId}`).
  - Auth: `Authorization: Bearer <token>` plus the `user-agent` / `x-goog-api-client` / `client-metadata` header trio Google's IDE plugins use.
  - Tool-call IDs: prefer `functionCall.id` from the API; regenerate via `{name}_{ts_ms}_{counter}` on missing/empty/duplicate.
  - Usage: `promptTokenCount - cachedContentTokenCount → input_tokens`; `cachedContentTokenCount → cache_read_input_tokens` (None when 0).
  - Capabilities: `with_image()` (matches vanilla Gemini).
- **`motosan_ai.oauth` package** — Google PKCE OAuth flow:
  - Internal `Pkce.generate()` — 64-byte verifier + S256 challenge used by `login()` (not exported from `motosan_ai.oauth`; import from `_pkce` only for custom low-level flows).
  - `OAuthConfig` + `google_gemini_config()` — public Gemini-CLI client_id/secret per Google's installed-app docs.
  - `Token.is_expired()` — 60s pre-expiry buffer.
  - `_callback_server.bind()` / `wait_for_callback()` — single-shot loopback HTTP server using stdlib `http.server`.
  - `login(config, _open_browser=...)` — full PKCE flow with state validation; 120s callback timeout.
  - `exchange_code(...)` / `refresh_token(...)` — token endpoint HTTP with `AuthError` on 4xx.
  - `save_token(...)` / `load_cached_token(...)` — JSON cache at `~/.config/motosan-ai/google-tokens.json` with `0600` mode.
  - `ensure_fresh_token(...)` — load cache, refresh-if-expired, persist, return.
- **`Provider.gemini_code_assist`** + `Client.gemini_code_assist(access_token=, project_id=, ...)` classmethod. Constructor params `access_token` and `project_id` added to `Client.__init__`.

### Notes
- No new prod dependencies. PKCE uses stdlib `secrets` + `hashlib`; loopback server uses stdlib `http.server`.
- Token cache file is created and refreshed with `0600` permissions to protect the refresh token.
- The Gemini-CLI client_id/secret are public (Google's installed-app convention) — embedded in source like Rust does.
- If Code Assist returns `401` mid-stream, the provider raises `AuthError`; callers should refresh via `ensure_fresh_token()` and retry. Automatic provider-side 401 refresh is intentionally out of scope because the HTTP provider only owns a bearer token, not OAuth config.
- Only `google_gemini_config()` ships in this phase. A Codex OAuth config helper exists in the Rust OAuth crate but is future Python work; Python Codex CLI currently delegates auth to the local `codex` binary.
- Live tests require `MOTOSAN_RUN_CODE_ASSIST_LIVE=1`, a cached token (run `login()` once), and `GOOGLE_PROJECT_ID`.

## [0.9.2] - 2026-04-25

### Added — `GeminiCliClient` (Phase 3c)
- New subprocess provider mirroring Rust's `GeminiCliProvider`. Spawns `gemini -p "" -o stream-json [...args]` and parses NDJSON events (`init`, `message`, `result`).
- 11 fluent builder methods cover the full Rust flag surface:
  - Booleans: `yolo` (`--yolo`), `sandbox` (`--sandbox`)
  - Single-value: `model` (`-m`), `approval_mode(ApprovalMode)` (`--approval-mode`), `resume` (`--resume`)
  - Repeating singular: `include_dir` (`--include-directories`), `extension` (`-e`), `allowed_mcp_server` (`--allowed-mcp-server-names`)
  - Repeating plural (replace): `include_dirs`, `extensions`, `allowed_mcp_servers`
- `ApprovalMode` `StrEnum` (`default` / `auto_edit` / `yolo` / `plan`) — values are wire flags.
- `Provider.gemini_cli` registered in `Client` dispatch; new `Client.gemini_cli()` classmethod. Reuses the `binary_path=` parameter on `Client.__init__` introduced in v0.9.1.
- `GEMINI_CLI_PATH` env var resolves the binary location (matches Rust default).
- Stream emits `StreamEvent(usage)` before terminal `done` when `result` carries `stats`; `stats.cached` maps to `Usage.cache_read_input_tokens`.
- System prompt merged into stdin payload via `\n\n` separator (matches Rust `merge_system_into_prompt`).
- Live integration tests under `tests/integration/test_gemini_cli_live.py` (two-tier gate: binary on PATH plus `MOTOSAN_RUN_GEMINI_CLI_LIVE=1`).

### Notes
- No API key required — `Provider.gemini_cli` is purely subprocess-based; the `gemini` binary handles its own auth.
- Argv composition order matches Rust `spawn.rs::common_args` byte-for-byte; pinned by `test_full_config_argv_order_matches_rust_common_args`.
- Distinct from Codex CLI: Gemini CLI takes the prompt purely via stdin with no trailing `-` argv marker.
- Phase 3d (Gemini Code Assist OAuth + HTTP) ships in v0.9.3.

## [0.9.1] - 2026-04-25

### Added — `CodexCliClient` (Phase 3b)
- New subprocess provider mirroring Rust's `CodexCliProvider`. Spawns `codex exec --json --skip-git-repo-check` and parses JSONL events (`item.completed`, `turn.completed`, `turn.failed`, `error`).
- 13 fluent builder methods cover the full Rust flag surface:
  - Booleans: `agent_mode` (`--full-auto`), `dangerously_bypass_approvals_and_sandbox`, `oss`, `ephemeral`
  - Single-value: `sandbox(SandboxMode)`, `local_provider(LocalProvider)`, `model`, `profile`, `cd`
  - Repeating: `add_dir`, `enable_feature`, `disable_feature`, `config_override(key, value)` → `-c key=value`
- `SandboxMode` (`read_only` / `workspace_write` / `danger_full_access`) and `LocalProvider` (`lmstudio` / `ollama`) `StrEnum`s — values are the wire flags.
- `Provider.codex_cli` registered in `Client` dispatch; new `Client.codex_cli()` classmethod and `binary_path=` parameter on `Client.__init__`.
- `CODEX_PATH` env var resolves the binary location (matches Rust default).
- Stream emits `StreamEvent(usage)` before terminal `done` when `turn.completed` carries usage; `cached_input_tokens` maps to `Usage.cache_read_input_tokens`.
- Live integration tests added under `tests/integration/test_codex_cli_live.py`; they are opt-in via `MOTOSAN_RUN_CODEX_LIVE=1`, use `MOTOSAN_CODEX_MODEL` (default `gpt-5.1-codex`), and skip with a preflight auth/model error when the local Codex setup is not usable.

### Notes
- No API key required — `Provider.codex_cli` is purely subprocess-based; the `codex` binary handles its own auth.
- Live Codex tests can override the default model with `MOTOSAN_CODEX_MODEL` for accounts that cannot use `gpt-5.1-codex`.
- Patch note: Codex CLI stdin prompt assembly now matches Rust wire format: `request.system` takes precedence over `Message.system(...)`, and system prompts are wrapped as `[system instructions]` blocks before the user prompt.
- Argv composition order matches Rust `spawn.rs::common_args` byte-for-byte; pinned by `test_full_config_argv_order_matches_rust_common_args`.
- Phase 3c (Gemini CLI) and 3d (Gemini Code Assist OAuth) ship in subsequent 0.9.x releases.

## [0.9.0] - 2026-04-25

### Added — ClaudeCodeClient full flag surface parity with Rust v0.12.0+
- **Builder state consolidated** into internal `_ClaudeCodeConfig` dataclass; backward-compatible property shims preserve `_binary_path` / `_model` / `_agent_mode` access.
- **26 new fluent builder methods** covering string flags, list flags, MCP config, setting sources, sessions, plugin dirs, named agents, and budget controls.
- **Stream usage events** — Claude Code NDJSON `result` events with `usage` now emit `StreamEvent(event_type="usage")` before terminal `done`, matching Rust.

### Notes
- No breaking changes to `ClaudeCodeClient()` / `.model()` / `.agent_mode()`.
- Live Claude Code tests are opt-in via `MOTOSAN_RUN_CLAUDE_CODE_LIVE=1`, matching the later CLI provider live-test gates.
- Subprocess argv composition is aligned with the Rust `ClaudeCodeProvider` flag wiring for equivalent configs.
- Covers Phase 3a of the Python SDK catch-up roadmap; Codex CLI, Gemini CLI, and Gemini Code Assist OAuth remain in later 0.9.x phases.

## [0.8.2] - 2026-04-25

### Fixed
- **Anthropic extended thinking via OAuth** — the streaming SSE adapter silently dropped `thinking_delta` events, so `ChatResponse.thinking` was always `None` on OAuth tokens (which route through stream+collect). Now emits `StreamEvent(event_type="thinking")`; the OAuth `chat()` collector accumulates into `ChatResponse.thinking`.
- **Gemini default model** — `gemini-2.0-flash` was deprecated for new users (returns HTTP 404). Default bumped to `gemini-2.5-flash`. All mock-URL references, live tests, and parity conftest updated.
- **Live vision fixtures** — replaced 1×1 transparent PNG (~67 bytes) with a 64×64 solid-red PNG (187 bytes). Anthropic and Gemini both reject sub-minimum images; the old fixture returned `HTTP 400: Could not process image` / `Unable to process input image`.

### Added
- `test_stream_emits_thinking_deltas_as_thinking_event` + `test_oauth_chat_collects_thinking_from_stream` regression tests.

## [0.8.1] - 2026-04-24

### Added
- **Test infrastructure — drift detection**
  - `tests/_snapshots.py` helper: JSON-file snapshots with `UPDATE_SNAPSHOTS=1` regenerate mode.
  - `tests/parity/` cross-provider matrix tests for simple chat, `ToolChoice`, vision, and stream event contracts.
  - `tests/test_client_integration.py` provider dispatch matrix, env-var fallback, and retry end-to-end coverage.
  - Nightly CI workflow (`.github/workflows/ci-python-nightly.yml`) runs live integration tests against real provider APIs.

### Fixed
- **OpenAI vision serialization** — `OpenAIProvider._serialize_messages` now emits `content_blocks` as `image_url` parts (base64 → data URI, URL → raw URL). Previously, `Message.user_with_image(...)` silently dropped the image.

### Notes
- Snapshots are code-review artifacts. Any diff in `tests/snapshots/*.json` is a deliberate wire-format change and should be reviewed like a schema migration.
- Nightly live-test secrets must be configured in GitHub repo settings (`ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `MINIMAX_API_KEY`).

## [0.8.0] - 2026-04-24

### Added
- **`GeminiProvider`** — native HTTP client for Google's Generative Language REST API (`generativelanguage.googleapis.com/v1beta`).
  - `Provider.gemini` registered in `Client` dispatch.
  - `Client.gemini(api_key=..., model=..., base_url=...)` classmethod.
  - `GEMINI_API_KEY` env var loaded automatically.
  - Default model: `gemini-2.0-flash`.
  - Full feature coverage: text, vision (base64 + URL), tools (`functionDeclarations`), tool choice (AUTO / ANY / allowedFunctionNames), streaming (`streamGenerateContent?alt=sse`), system prompts (`systemInstruction`), stop sequences (`stopSequences`), usage reporting (`promptTokenCount` / `candidatesTokenCount`).
  - Capabilities: `with_image()` — document blocks raise `InvalidRequestError` before any HTTP call.
  - Tool call IDs are generated client-side. By convention, `Message.tool_result(tool_call_id=<function_name>, ...)` uses the function name as the ID for Gemini round-trips.
- **Live integration tests** (`tests/integration/test_gemini_live.py`): simple chat, vision, tool use, streaming.

### Notes
- Gemini does not support document (PDF) input; calls with `ContentBlock::Document` fail at validation time.
- No cache token accounting on Gemini — `Usage.cache_creation_input_tokens` / `cache_read_input_tokens` always `None`.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.

## [0.7.0] - 2026-04-24

### Added — Anthropic wire-format parity with Rust SDK
- **Vision & PDF input** — user messages with `content_blocks` now serialize as Anthropic content-block arrays, including image/document base64 and URL sources.
- **Prompt caching** — `Message.cache`, `SystemBlock[]`, `system_cache=True`, and `Tool.cache` now emit Anthropic `cache_control`; cache creation/read usage tokens are parsed.
- **ToolChoice** — `auto`, `required` (Anthropic `any`), `none` (removes tools), and `tool(name)` are supported.
- **Extended thinking** — `ThinkingConfig` serializes as enabled thinking, forces `temperature=1.0`, and non-stream responses parse thinking blocks into `ChatResponse.thinking`.
- **MCP server-side tools** — `mcp_servers` and `mcp_tool_configs` serialize for Anthropic; `anthropic-beta: mcp-client-2025-11-20` is attached when needed.
- **Stop sequences** — `stop_sequences` serialize and `StopReason.stop_sequence` is parsed.
- **Stream enhancements** — `StreamEvent.usage` is emitted from Anthropic `message_start` / `message_delta`, and terminal done events carry `stop_reason` when provided.

### Changed
- `AnthropicProvider` now inherits `BaseProvider`; `validate_request()` runs before HTTP work in `chat()` and `stream()`.
- Anthropic request building now uses one unified serializer for OAuth and standard-key paths.

### Notes
- Only Anthropic gained Phase 2a wire-format support in this release. Other providers remain scheduled in later roadmap phases.

## [0.6.0] - 2026-04-24

### Added
- **Type foundation for Rust SDK parity** — additive-only. No wire-format changes yet.
- `ContentBlock` discriminated union (`TextBlock` / `ImageBlock` / `DocumentBlock`).
- `ImageSource` (`ImageSourceBase64` / `ImageSourceUrl`) and `DocumentSource` (`DocumentSourceBase64` / `DocumentSourceUrl`).
- `Message.user_with_image()`, `Message.user_with_blocks()`, `Message.user_with_pdf_base64()`, `Message.user_with_pdf_url()`, `Message.user_with_pdf_bytes()`.
- `Message.cache` field + `Message.user_with_cache()` + `Message.with_cache()`.
- `SystemBlock` with `SystemBlock.new()` / `SystemBlock.cached()` factories.
- `Tool.cache` field.
- `ToolChoice` with `auto()` / `required()` / `none()` / `tool(name)` factories.
- `ThinkingConfig` (budget_tokens) for extended thinking.
- `McpServerConfig` and `McpToolConfig*` (All / Allowed / Denied) for server-side MCP.
- `ChatRequest` fields: `system_blocks`, `system_cache`, `tool_choice`, `mcp_servers`, `mcp_tool_configs`, `thinking`, `stop_sequences`.
- `ChatRequest.builder()` returning `ChatRequestBuilder` (fluent API parity with Rust SDK).
- `ChatResponse.thinking` field.
- `Usage.cache_creation_input_tokens` and `Usage.cache_read_input_tokens`.
- `StopReason.stop_sequence` variant.
- `StreamEventType` enum; `StreamEvent.usage` and `StreamEvent.stop_reason` fields.
- `ProviderCapabilities` (`text_only` / `with_image` / `full`) declared on each provider.
- `BaseProvider` ABC with default `validate_request()` enforcing capabilities.

### Changed
- Capability declarations per provider: `Anthropic` = `full`, `OpenAI` = `with_image`, `Minimax` = `with_image`, `Ollama` = `text_only`, `ClaudeCodeClient` = `text_only`.

### Notes
- **No new wire-format behavior in 0.6.0.** Providers still serialize request bodies as before. Phase 2 (v0.7.0+) will wire `content_blocks`, `system_blocks`, `tool_choice`, `thinking`, and MCP config into the Anthropic and Gemini providers.
- See `docs/superpowers/plans/2026-04-24-python-sdk-catchup-roadmap.md` for the full catch-up roadmap.

## [0.5.0] - 2026-04-05

### Added
- **`ClaudeCodeClient`** — fourth LLM backend that shells out to the `claude` CLI binary (parity with Rust SDK `claude-code` feature).
  - `ClaudeCodeClient()` resolves binary from `CLAUDE_CODE_PATH` env or `"claude"` in `PATH`.
  - `ClaudeCodeClient.with_path(path)` — explicit binary path.
  - `.model(model)` — forwards `--model <value>` when non-empty and not `"default"` (case-insensitive, trimmed).
  - `.agent_mode(bool)` — enables `--dangerously-skip-permissions` + JSON output parsing.
  - `async chat(request) -> ChatResponse` — subprocess via `--print`, 300 s timeout.
  - `async stream(request) -> AsyncIterator[StreamEvent]` — NDJSON streaming via `--print --output-format stream-json --verbose`, per-line 300 s timeout, subprocess always cleaned up via `try/finally`.
  - `ClaudeCodeClient` exported from `motosan_ai` top-level.

## [0.4.2] - 2026-03-18

### Added
- **Retry with exponential backoff** for transient errors — `Client.chat()` and `Client.stream()` auto-retry on 429, 5xx, and network errors (default `max_retries=3`)
- `max_retries` parameter on `Client` constructor and all classmethods (`anthropic()`, `openai()`, `minimax()`, `ollama()`)
- `Retry-After` header parsing — uses server-suggested wait time when available
- New `motosan_ai.retry` module with `with_retry()` and `_is_retryable()` utilities

### Changed
- **Retry defaults aligned with Rust SDK** — `initial_backoff=0.1s`, `max_backoff=2.0s` (was 1s/30s)
- **Retry scope expanded** — now retries `RateLimitError` (429), `ProviderError` (5xx), and `NetworkError` (timeout/connection), matching Rust SDK behavior

## [0.4.1] - 2026-03-18

(Superseded by 0.4.2 — retry only covered 429, defaults misaligned with Rust)

## [0.4.0] - 2026-03-18

### Changed
- **Remove official SDK dependencies**: Anthropic and OpenAI providers rewritten to use `httpx` directly instead of `anthropic` and `openai` packages — zero official provider SDK dependencies
- **pyproject.toml**: `anthropic`, `openai`, `ollama` optional deps now all resolve to `httpx>=0.27`
- **All mock tests** migrated from `monkeypatch` + `FakeClient` to `respx` HTTP mocking
- **Anthropic default model** updated to `claude-sonnet-4-6`
- **OAuth streaming**: OAuth `chat()` now internally streams (Anthropic requires streaming for OAuth tokens) and collects into `ChatResponse`
- **OAuth system prompt**: sent as array of blocks with Claude Code prefix + `cache_control`
- **OAuth user messages**: serialized as content blocks (`[{"type": "text", "text": ...}]`) per Anthropic OAuth requirements

### Fixed
- **Anthropic OAuth `chat()` tool_calls**: OAuth path now correctly collects tool_call stream events into `ChatResponse.tool_calls` with proper `stop_reason=tool_use` (previously returned empty)
- **Anthropic OAuth system prompt**: system prompt now sent as separate blocks (Claude Code prefix with `cache_control` + user system without) instead of merged single string (fixes `invalid_request_error`)

### Added
- **Live integration tests** (`tests/integration/test_anthropic_live.py`): 7 tests hitting real Anthropic API with OAuth token — chat, stream, system prompt, temperature, tool use (single + multi-turn), stream + tool use
- **Pre-push gate** (`scripts/pre-push-gate.sh`): blocks push unless unit + live tests pass

## [0.3.3] - 2026-03-16

(Published to PyPI with anthropic SDK-based providers — superseded by 0.4.0)

## [0.2.1] - 2026-03-15

### Added
- **Ollama provider** (`Provider.Ollama`): connect to local or remote Ollama instances
  - Phase 1: OpenAI-compatible endpoint (`/v1/chat/completions`) — zero new dependencies
  - Phase 2: `OllamaProvider` native via `POST /api/chat` with NDJSON streaming
  - `think=True`: enable reasoning mode (qwen3-thinking, deepseek-r1, etc.)
  - `keep_alive`: control VRAM retention after request
  - `num_ctx`: override context window size
  - `ollama_base_url()`: point to remote Ollama instance (default: `http://localhost:11434`)
  - `ollama_native(True)`: switch to native `/api/chat` endpoint
  - `ollama_think()`, `ollama_keep_alive()`, `ollama_num_ctx()` builder methods
  - Tool calls: auto-generates uuid when Ollama native omits `id`
  - Default model: `llama3.2`

## [0.2.0] - 2026-03-12

### Added
- Initial Python SDK release
- `Provider.Anthropic`, `Provider.OpenAI`, `Provider.Minimax`
- Async `chat()`, `chat_with()`, `stream()` with full `ChatRequest` parameters
- `Message.assistant_with_tool_calls()`, `Message.tool_result()` for multi-turn tool use
- `ToolCall`, `StreamEvent`, `Usage`, `StopReason` types
- Pydantic-free implementation (dataclasses + TypedDict)
- `ClientBuilder` with provider, model, api_key, system, max_tokens, temperature

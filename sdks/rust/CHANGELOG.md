# Changelog

All notable changes to `motosan-ai` Rust SDK are documented in this file.

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

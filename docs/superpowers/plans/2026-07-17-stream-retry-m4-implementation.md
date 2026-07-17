# Stream/Retry M4 Implementation Plan — Spec & Parity Cleanup

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the last audit workstream (rank 8): Rust feature-architecture refactor per the Accepted spec, stream-event vocabulary unified across specs and all three SDKs, a single CLI chat-vs-stream contract, a per-attempt OAuth token-source seam for chatgpt_codex, and Python `Provider.claude_code` wiring — released as Rust 0.25.0 / Python 0.18.0 / TS 0.15.0.

**Architecture:** Six PR groups. PR-S rewrites `specs/types.md` as the normative reference (vocabulary + CLI contract + token sources). PR-F executes the Accepted feature-architecture spec (private `_http`/`_cli` umbrellas, `src/transport/http.rs` module stratum, tokio-stream promoted unconditional, cargo-hack CI). PR-R then applies the CLI contract and TokenSource seam to Rust on top of the new layout; PR-P applies vocabulary + CLI contract + token_source + claude_code wiring to Python; PR-T widens the TS chatgpt_codex token to an async source. PR-REL bumps versions and docs, no tag/publish.

**Tech Stack:** Rust (tokio, reqwest, async-trait, cargo-hack), Python (httpx, respx, pytest, uv), TypeScript (Node ≥20.3, npm), GitHub Actions tag-triggered publishing.

**Baseline:** origin/main @ `b9bcc3e` (post-M3: Rust 0.24.0 / Python 0.17.0 / TS 0.14.0). Line references throughout are approximate against this baseline — ground every edit in the real files and adapt to drift from earlier tasks in the same PR.

## Global Constraints

Locked design decisions. Deviation is wrong even if it compiles. Every task's requirements implicitly include this section.

- **F1 — Feature architecture (Rust, non-breaking):** implement `docs/superpowers/specs/2026-07-17-rust-feature-architecture-design.md` exactly. Private umbrella features `_http = ["dep:reqwest", "dep:chrono", "dep:eventsource-stream", "dep:tokio"]` and `_cli = ["dep:tokio", "dep:async-stream"]` (underscore prefix = internal, not semver-covered, never enabled directly). `tokio-stream` is **promoted to an unconditional dependency** — it leaves both umbrellas and `stream.rs` loses its cfg gate entirely (`collect_stream` compiles under `--no-default-features`). Public features: `anthropic`/`openai`/`gemini`/`chatgpt-codex` = `["_http"]`; `minimax = ["anthropic"]`; `ollama = ["openai", "dep:bytes"]`; `ollama_native = ["ollama"]`; **new** alias `ollama-native = ["ollama_native"]` (docs teach the dash form; underscore form kept forever); `gemini-code-assist = ["gemini"]`; `claude-code`/`codex-cli`/`gemini-cli` = `["_cli"]`; `full` gains `ollama-native`. Module strata: new `src/transport/http.rs` gated **once** with `#[cfg(feature = "_http")]` receives the shared HTTP items from `providers/mod.rs`; shared CLI helpers get one `_cli` gate. Resolved-deps diff (`cargo tree`) must be empty for every pre-existing feature. CI gains `cargo hack check --each-feature`. Baseline evidence at `b9bcc3e`: `grep -rn 'feature = "gemini-code-assist"' sdks/rust/src/` = **45** mentions; acceptance ≤ 3.
- **F2 — Stream-event vocabulary (all SDKs + spec):** `StreamEventType` is exactly `text | tool_call_start | tool_call_args | tool_call_end | usage | thinking_delta | thinking_done`. There is **no** `done` event type — `done` is a bool *field* on `StreamEvent` (the current spec line listing `done` as a member is a bug). `thinking_done` carries the full concatenated thinking text and precedes final-answer `text` events.
- **F3 — Python thinking migration (BREAKING):** add `thinking_delta`/`thinking_done` members to Python's `StreamEventType`; migrate every ad-hoc `event_type="thinking"` emission to `StreamEventType.thinking_delta`; Anthropic additionally emits `thinking_done` (full concatenated text) on `content_block_stop` of a thinking block, mirroring Rust; `_stream_collect` gives `thinking_done` priority over the concatenated-delta fallback (mirror Rust `stream.rs`). Consumers matching the old `"thinking"` string break — changelog BREAKING note required.
- **F4 — CLI chat/stream contract (BREAKING, Rust + Python):** a successfully completed CLI turn **always** reports `stop_reason = end_turn` on both paths. CLI backends **never** report `tool_use` — their tools are executed internally by the CLI; `tool_use` means "caller must execute tools", which CLI backends never request (reporting it makes agent loops re-execute already-executed tools). `cli_terminal_stop_reason(saw_tool_call)` is retired. `chat()` for every CLI backend (Rust `claude_code`/`codex_cli`/`gemini_cli`, Python same trio) is reimplemented as delegation to collecting the provider's own `stream()`, so `tool_calls`/`thinking`/`usage`/`session_id` parity holds by construction; `ChatResponse.tool_calls` from a CLI backend = the *record* of tools the CLI already executed. One documented parity exception: `chat()` may backfill `ChatResponse.model` from provider config when the collected value is empty. `chat()` keeps failing loudly on CLI errors and stalls, but the failure *surface* legitimately shifts to the stream-path variants and is a documented BREAKING delta (carried into F7's changelog): Rust `chat()` errors arrive as the M3 stream variants (`StreamReadTimeout` / stream errors) instead of the old single-shot mappings; Python `chat()` nonzero-exit/child-death raises `StreamError` (was `ProviderError`) and the timeout scope becomes per-read stall rather than whole-invoke. Additionally, Rust `codex_cli` `chat()` no longer splits the preamble into `thinking` — the old split was a post-hoc heuristic over the full transcript ("all but the last agent message"), which is unrepresentable in a stream; content becomes the concatenation and `thinking` is `None` (BREAKING, documented). Newly-dead single-shot invoke paths are deleted (grep-verified).
- **F5 — TokenSource seam (per-attempt):** Rust: new ungated `src/auth.rs` with `#[async_trait] pub trait TokenSource: Send + Sync + Debug { async fn access_token(&self) -> Result<String, MotosanError>; }` + `StaticTokenSource`; `ChatGptCodexProvider` stores `Arc<dyn TokenSource>` (constructor signature unchanged, wraps `StaticTokenSource`; manual `Debug` never prints token material); token fetched at the top of **every** retry attempt via new `send_with_retry_async_build` (async request-build closure) with `send_with_retry` rewritten as a thin wrapper — the single M2 retry engine and its `on_retry` choke point stay intact, proven by the unchanged retry-conformance suite. `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)`. SDKs stay decoupled from the oauth crates; the >1h live smoke (`#[ignore]`d) implements a refreshing source locally over the workspace `motosan-ai-oauth`/`codex-oauth` crates (dev-deps only). Python: `token_source: Callable[[], Awaitable[str]] | None = None` on provider + `Client.chatgpt_codex`, resolved per attempt; `openai_chatgpt` validation relaxes to access_token OR token_source. TypeScript: `accessToken: string | (() => Promise<string>)`, resolved per attempt. Each SDK proves per-attempt resolution with a 500-then-200 test asserting the source was consulted twice and attempt 2 sent the refreshed value.
- **F6 — Python `Provider.claude_code`:** add the enum member, routing arm, and `Client.claude_code(...)` classmethod mirroring `codex_cli`'s, exposing the real `ClaudeCodeClient` constructor params (the Python class is `ClaudeCodeClient`, not `ClaudeCodeProvider`). Not documented out of scope.
- **F7 — Release:** Rust **0.25.0** (BREAKING: F4), Python **0.18.0** (BREAKING: F3 + F4), TypeScript **0.15.0** (minor: F5 only). Tag-triggered CI publish only; the release PR contains no tag and no publish.

House rules (standing): tool-call field is `input` everywhere; `ChatResponse.tool_calls` is always a list, never optional; provider logic only in `providers/` (and the new `transport/` stratum); no sync wrappers in Python; `LlmClient` Protocol additive-only; Anthropic `tool_call_id` only in `content_block_start`.

CI-matching gates (run from the SDK dir):

- Rust: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features` (+ `cargo hack check --each-feature` once Task 1 lands). Clippy **must** use `--all-targets` (CI lints tests).
- Python: `uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration`. Fresh worktree: `uv sync --all-extras` from `sdks/python` before first push (pre-push hook runs the full suite).
- TypeScript: `npm run typecheck && npm run build && npm test` (build before test — pack-smoke needs `dist/`).

## PR groups & merge order

| Group | Tasks | Branch | Worktree | Prereq |
|---|---|---|---|---|
| PR-S | 2 | `docs/m4-vocab-cli-token-spec` | m4-spec | — (merges **first**; normative reference) |
| PR-F | 1 | `refactor/m4-rust-feature-arch` | m4-featarch | — (may open anytime; merges before PR-R) |
| PR-R | 3, 4 | `feat/m4-rust-cli-token` | m4-rust | PR-S **and** PR-F merged |
| PR-P | 5, 6, 7, 8 | `feat/m4-python-vocab-cli-token` | m4-python | PR-S merged |
| PR-T | 9 | `feat/m4-ts-token-source` | m4-ts | PR-S merged |
| PR-REL | 10 | `chore/m4-release` | m4-release | all of the above merged |

Tasks within a group run in ascending order on the same branch. PR-R/P/T may be *opened* before PR-S merges but must not *merge* before it.

---

### Task 1: Rust feature architecture migration (PR-F — `_http`/`_cli` umbrellas + `src/transport/`)

Implements `docs/superpowers/specs/2026-07-17-rust-feature-architecture-design.md` (Accepted, normative). Whole PR, mechanical refactor, NON-breaking: public feature set unchanged plus the new `ollama-native` alias; `cargo tree` resolved-deps diff empty for every pre-existing feature; zero behavior change; zero test-body change. Branch: `refactor/m4-rust-feature-arch` off `origin/main` @ `b9bcc3e`. All work happens in `sdks/rust/` plus one CI file and two doc files. Per house workflow, every `.rs`/`Cargo.toml` change lands via PR + CI — never direct to main.

**Files:**
- Create: `sdks/rust/src/transport/mod.rs` (ungated module root: `TimeoutConfig` + default-timeout consts moved from `sdks/rust/src/client.rs:9-32`)
- Create: `sdks/rust/src/transport/http.rs` (`#[cfg(feature = "_http")]`-gated once at the mod decl; receives from `sdks/rust/src/providers/mod.rs`: `ChatResponseBuilder` (137-227), `extract_error_message` (238-245), `map_http_error` (256-288), `is_retryable_status` (336-338), `is_retryable_network_error` (349-351), `RETRY_AFTER_CAP` (362), `parse_retry_after` (373-387), `extract_request_id` (398-405), `observe_and_sleep` (416-436, stays private), `send_with_retry` (451-484), `apply_total_timeout` (496-504), `collect_stream_with_total_timeout` (512-528), test mods `retry_after_tests` (540-631), `http_error_metadata_tests` (754-817), `retry_conformance` (819-957))
- Create: `sdks/rust/src/transport/cli.rs` (`#[cfg(feature = "_cli")]`-gated once; receives `cli_terminal_stop_reason` from `sdks/rust/src/providers/mod.rs:291-297` and test mod `cli_terminal_tests` from `sdks/rust/src/providers/mod.rs:299-325`)
- Modify: `sdks/rust/Cargo.toml` (`[features]` block lines 15-76; `tokio-stream` dep line 98; redundant dev-dep line 109)
- Modify: `sdks/rust/src/providers/mod.rs` (delete moved items + their cfg'd imports at lines 1-53; add `pub(crate) use` re-exports; re-gate `uses_http_transport` impl at 78-95 and `pub mod redacted_envs` at 642-643)
- Modify: `sdks/rust/src/client.rs` (drop local `TimeoutConfig` defs 9-32; replace 7 enumerated `cfg(any(...))` lists with `_http`; delete 2 gates entirely; verified sites listed in Step 8)
- Modify: `sdks/rust/src/stream.rs` (delete gates at 23-30 and 158-164 — file compiles fully ungated)
- Modify: `sdks/rust/src/lib.rs` (add `pub(crate) mod transport;`; delete `collect_stream` export gate at 34-39)
- Modify: `.github/workflows/ci-rust.yml` (add cargo-hack steps after line 30)
- Modify: `sdks/rust/README.md` (Features section, lines 103-115) and `AGENTS.md` (rule 4 at line 86)
- Test: no new test files; existing suites must pass byte-identical (`cargo test --all-features`); moved test mods keep byte-identical bodies

**Interfaces:**
- Consumes (current code, verified):
  - `pub(crate) async fn send_with_retry(policy: &RetryPolicy, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, MotosanError>` — `sdks/rust/src/providers/mod.rs:451`
  - `pub(crate) async fn collect_stream_with_total_timeout(stream: BoxStream, total: Option<Duration>) -> Result<ChatResponse, MotosanError>` — `sdks/rust/src/providers/mod.rs:512`
  - `pub(crate) fn cli_terminal_stop_reason(saw_tool_call: bool) -> crate::types::StopReason` — `sdks/rust/src/providers/mod.rs:291`
  - `pub(crate) struct TimeoutConfig { connect, read_idle, total }` — `sdks/rust/src/client.rs:18`
  - HTTP providers import via `use crate::providers::{...}` (verified headers: `anthropic.rs:5-8`, `openai.rs:3-6`, `ollama.rs:3-6`, `gemini.rs:5-8`, `gemini_code_assist.rs:6-9`, `chatgpt_codex.rs:2-5`); CLI backends call `super::cli_terminal_stop_reason` (`claude_code/mod.rs:617`, `codex_cli/mod.rs:598`, `gemini_cli/mod.rs:394`)
- Produces (later M4 tasks rely on these exact names):
  - Features `_http`, `_cli` (private umbrellas), `ollama-native` (public alias)
  - `sdks/rust/src/transport/http.rs` — post-Task-1 home of `send_with_retry`; Task on F5 adds `send_with_retry_async_build` in this file
  - `sdks/rust/src/transport/cli.rs::cli_terminal_stop_reason`, re-exported as `crate::providers::cli_terminal_stop_reason` — Task 3 (F4) deletes it from here
  - `crate::transport::TimeoutConfig` (pub(crate), ungated)
  - CI step `cargo hack check --each-feature --no-dev-deps` in `.github/workflows/ci-rust.yml`

**Flip list:** none. This refactor moves code and gates; no pinned assertion changes. (Three test mods become compilable in more feature combos — `transport::cli::cli_terminal_tests`, `stream::thinking_collect_tests`, `client::think_stripper_stream_tests::read_timeout_yields_error_once_then_ends` — bodies untouched.)

**Known deviations from the locked text (verified against real code — flag in PR description):**
1. `TimeoutConfig` is NOT in `providers/mod.rs` (spec says it is); it lives at `sdks/rust/src/client.rs:18` and backs the UNGATED public accessors `Client::connect_timeout/read_idle_timeout/total_timeout` (`client.rs:190-200`) and ungated builder fields (`client.rs:697-699`), all of which exist under `--no-default-features` today. Placing it in `_http`-gated `transport/http.rs` would delete public API from CLI-only builds → breaking. It therefore moves to the UNGATED `transport/mod.rs` root (still "the transport layer" per spec intent).
2. The literal `grep -rn 'feature = "gemini-code-assist"' src/ | wc -l` lands at **16** after migration, not ≤3: 15 are single-feature `#[cfg(feature = "gemini-code-assist")]` Stratum-2 gates in `client.rs` (BuiltProvider variant :56, as_impl arm :85, Client fields :132/:137, dispatch arms :453/:457/:607, builder fields/methods :706-:1107) which spec §2 explicitly leaves "unchanged", plus the module decl at `providers/mod.rs:657`. The enforced acceptance below matches the spec's stated intent ("no shared-code enumerations"): shared-code files contain exactly 1 mention and zero `any(...)` enumerations anywhere contain the feature. Do NOT restructure `client.rs` per-provider wiring to chase the literal number.

---

- [ ] **Step 1: Branch + install cargo-hack**

  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git fetch origin
  git checkout -b refactor/m4-rust-feature-arch origin/main
  git log --oneline -1   # expect: b9bcc3e chore(release): M3 stream contract + timeouts
  cargo hack --version 2>/dev/null || cargo install cargo-hack --locked
  ```
  Expected: branch created; `cargo hack 0.6.x` (or install output ending `Installed package cargo-hack ...`).

- [ ] **Step 2: Capture BEFORE evidence (the "failing test" for the whole migration)**

  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  [ -f Cargo.lock ] || cargo generate-lockfile   # pin resolution so before/after trees are comparable
  for f in anthropic openai minimax ollama ollama_native gemini gemini-code-assist chatgpt-codex claude-code codex-cli gemini-cli full; do
    cargo tree -p motosan-ai --no-default-features -F "$f" -e normal > /tmp/m4-tree-before-"$f".txt
  done
  cargo tree -p motosan-ai --no-default-features -e normal > /tmp/m4-tree-before-none.txt
  grep -rn 'feature = "gemini-code-assist"' src/ | wc -l
  grep -c 'dep:reqwest' Cargo.toml
  ```
  Expected: 13 tree files written; grep count `45` (22 in `client.rs` + 23 in `providers/mod.rs` — re-verified at b9bcc3e); `dep:reqwest` count `6`. Do not touch `Cargo.lock` again until the AFTER capture.

- [ ] **Step 3: Rewrite `[features]` and promote `tokio-stream` (chunk 1: features rewrite)**

  Replace `sdks/rust/Cargo.toml` lines 15-76 (the whole `[features]` table through the closing `]` of `full`) with exactly:

  ```toml
  [features]
  default = []

  # ---- internal umbrella features --------------------------------------------
  # Underscore prefix = private convention (sqlx `_rt-tokio` precedent).
  # Internal implementation detail, NOT covered by semver. Never enable directly.
  _http = ["dep:reqwest", "dep:chrono", "dep:eventsource-stream", "dep:tokio"]
  _cli = ["dep:tokio", "dep:async-stream"]

  # ---- public provider features -----------------------------------------------
  anthropic = ["_http"]
  openai = ["_http"]
  minimax = ["anthropic"]
  ollama = ["openai", "dep:bytes"]
  # Retained as an alias for backwards compatibility. The `ollama_native(true)`
  # runtime flag still controls explicit routing — see ClientBuilder::ollama_native.
  # As of 0.15.0 the OllamaProvider (/api/chat) is also auto-selected whenever
  # ollama_think / ollama_keep_alive / ollama_num_ctx is set, regardless of this
  # feature, because Ollama's OpenAI-compat endpoint silently drops those fields.
  ollama_native = ["ollama"]
  # Canonical dash spelling; docs teach this one. `ollama_native` stays forever.
  ollama-native = ["ollama_native"]
  gemini = ["_http"]
  gemini-code-assist = ["gemini"]
  # Self-contained ChatGPT-backend Responses transport. Shares nothing with Gemini.
  chatgpt-codex = ["_http"]
  claude-code = ["_cli"]
  codex-cli = ["_cli"]
  gemini-cli = ["_cli"]
  full = [
    "anthropic",
    "openai",
    "minimax",
    "ollama",
    "ollama_native",
    "ollama-native",
    "gemini",
    "gemini-code-assist",
    "chatgpt-codex",
  ]
  ```

  Note `tokio-stream` appears in NEITHER umbrella — it is promoted to unconditional (spec §3, locked F1). `full` deliberately still excludes the CLI trio (matches current behavior) and gains only `ollama-native`. Dependency-line diff (line 98 in `[dependencies]`, line 109 in `[dev-dependencies]`):

  ```diff
   [dependencies]
  -tokio-stream = { version = "0.1", optional = true }
  +tokio-stream = "0.1"

   [dev-dependencies]
  -tokio-stream = "0.1"
  ```
  (The dev-dep line is now redundant — test targets see the union of `[dependencies]` + `[dev-dependencies]`.)

  Verify + AFTER-tree acceptance (trees depend only on Cargo.toml, so this chunk is fully checkable now):
  ```bash
  grep -c 'dep:reqwest' Cargo.toml    # expect: 1
  cargo check --all-features && cargo check --no-default-features && cargo test --all-features
  for f in anthropic openai minimax ollama ollama_native gemini gemini-code-assist chatgpt-codex claude-code codex-cli gemini-cli full; do
    cargo tree -p motosan-ai --no-default-features -F "$f" -e normal > /tmp/m4-tree-after-"$f".txt
    diff -u /tmp/m4-tree-before-"$f".txt /tmp/m4-tree-after-"$f".txt || echo "RESOLVED-DEPS DIFF IN $f — STOP, fix [features]"
  done
  cargo tree -p motosan-ai --no-default-features -F ollama-native -e normal | diff - /tmp/m4-tree-before-ollama_native.txt
  cargo tree -p motosan-ai --no-default-features -e normal | diff -u /tmp/m4-tree-before-none.txt -
  ```
  Expected: the 12-feature diff loop prints NOTHING (empty diff for every pre-existing feature = locked acceptance); the `ollama-native` alias tree is identical to the old `ollama_native` tree; ONLY the no-features baseline diff is non-empty — it gains `tokio-stream` and its `tokio` subtree (the accepted spec-§3 cost; `stream.rs` is still gated at this commit, that is fine).

  Commit:
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git add sdks/rust/Cargo.toml
  git commit -m "refactor(rust): dedupe [features] behind _http/_cli umbrellas, promote tokio-stream

  Resolved-deps cargo tree diff verified empty for all 12 pre-existing
  features; new ollama-native alias resolves identically to ollama_native.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 4: Create `src/transport/mod.rs` and move `TimeoutConfig` out of client.rs (chunk 2 begins)**

  Failing check first: `test -f sdks/rust/src/transport/mod.rs; echo $?` → `1`.

  Write `sdks/rust/src/transport/mod.rs` (complete file):

  ```rust
  //! Transport strata shared across provider implementations (M4 Task 1).
  //!
  //! - [`http`]: helpers shared by every HTTP provider, compiled ONCE behind
  //!   the private `_http` umbrella feature.
  //! - [`cli`]: helpers shared by every CLI backend, compiled ONCE behind the
  //!   private `_cli` umbrella feature.
  //!
  //! Rule: new providers route through `_http`/`_cli` in `Cargo.toml`. Adding
  //! a per-provider `#[cfg(any(...))]` enumeration in shared code is
  //! review-blocking (see sdks/rust/README.md, "Feature architecture rules").

  #[cfg(feature = "_cli")]
  pub(crate) mod cli;
  #[cfg(feature = "_http")]
  pub(crate) mod http;

  use std::time::Duration;

  pub(crate) const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
  pub(crate) const DEFAULT_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

  /// Unified timeout model:
  /// - `connect`: TCP/TLS connect deadline on the shared reqwest client.
  /// - `read_idle`: max gap between HTTP stream chunks before
  ///   `MotosanError::StreamReadTimeout`.
  /// - `total`: opt-in wall-clock budget per blocking `chat()` attempt.
  ///
  /// Lives at the ungated transport root (not inside `http`) because the
  /// public accessors `Client::connect_timeout/read_idle_timeout/total_timeout`
  /// exist on every feature combination, including `--no-default-features`.
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

  Then:
  1. `sdks/rust/src/lib.rs` — after `pub mod think_stripper;` (line 7) add: `pub(crate) mod transport;` (crate-private: no new public API, non-breaking).
  2. `sdks/rust/src/client.rs` — DELETE lines 9-32 (`DEFAULT_CONNECT_TIMEOUT`, `DEFAULT_READ_IDLE_TIMEOUT`, `TimeoutConfig` struct + its `Default` impl — moved verbatim above) and add to the import block after line 5 (`use crate::think_stripper::ThinkStripper;`):
     ```rust
     use crate::transport::{TimeoutConfig, DEFAULT_CONNECT_TIMEOUT, DEFAULT_READ_IDLE_TIMEOUT};
     ```
     (let `cargo fmt` settle brace ordering). All references (`client.rs:114` field, `:190-200` accessors, `:1045-1048` build()) compile unchanged.

  Gate: `cd sdks/rust && cargo check --all-features && cargo check --no-default-features` — both clean. (`transport::http`/`transport::cli` don't exist yet; their mod decls are cfg'd off only when the features are off — the files must exist before `--all-features` compiles, so if check complains about missing files, proceed to Steps 5-6 and gate at the end of Step 6 instead; the commit for chunk 2 comes after Step 6 either way.)

- [ ] **Step 5: Create `src/transport/http.rs` — move all HTTP-shared items (chunk 2)**

  Failing check first: `grep -n 'pub(crate) async fn send_with_retry' sdks/rust/src/providers/mod.rs` → `451:...` (still in the old home).

  Create `sdks/rust/src/transport/http.rs` with this exact header, then the moved items:

  ```rust
  //! Shared HTTP transport helpers, compiled ONCE behind the private `_http`
  //! umbrella feature (gated at the mod decl in transport/mod.rs — items in
  //! this file carry NO feature cfg of their own). Moved verbatim from
  //! providers/mod.rs in M4 Task 1; provider files keep their old
  //! `crate::providers::*` import paths via the pub(crate) re-export there.

  use crate::error::MotosanError;
  use crate::retry::{RetryCause, RetryEvent, RetryPolicy};
  use crate::stream::BoxStream;
  use crate::types::{ChatResponse, StopReason, ToolCall, Usage};
  use reqwest::header::HeaderMap;
  use serde_json::Value;
  use std::time::Duration;
  ```

  Below the header, MOVE the following from `sdks/rust/src/providers/mod.rs`, in this order, each with its item-level `#[cfg(any(...))]` attribute DELETED and the body/doc-comments byte-identical otherwise (locate by name if lines have drifted):

  | Item | Source lines (verified @ b9bcc3e) | Edit on move |
  |---|---|---|
  | `pub(crate) struct ChatResponseBuilder` + `impl` | 137-227 (attrs 129-136, 146-153 deleted) | none |
  | `pub(crate) fn extract_error_message` | 238-245 (attr 229-237 deleted) | none |
  | `pub(crate) fn map_http_error` | 256-288 (attr 247-255 deleted) | none |
  | `pub(crate) fn is_retryable_status` | 336-338 (attr 327-335 deleted) | none |
  | `pub(crate) fn is_retryable_network_error` | 349-351 (attr 340-348 deleted) | none |
  | `pub(crate) const RETRY_AFTER_CAP` | 362 (attr 353-361 deleted) | none |
  | `pub(crate) fn parse_retry_after` | 373-387 (attr 364-372 deleted) | none |
  | `pub(crate) fn extract_request_id` | 398-405 (attr 389-397 deleted) | none |
  | `async fn observe_and_sleep` (private — do NOT re-export) | 416-436 (attr 407-415 deleted) | none |
  | `pub(crate) async fn send_with_retry` + doc comment | 438-484 (attr 442-450 deleted) | none |
  | `pub(crate) fn apply_total_timeout` + doc comment | 486-504 (attr 487-495 deleted) | none |
  | `pub(crate) async fn collect_stream_with_total_timeout` + doc comment | 506-528 (attr 507-511 deleted — this kills a 3-feature enumeration naming `gemini-code-assist`) | none |
  | `#[cfg(test)] mod retry_after_tests` | 540-631 (feature attr 531-539 deleted, `#[cfg(test)]` kept) | none |
  | `#[cfg(test)] mod http_error_metadata_tests` | 754-817 (feature attr 745-753 deleted, `#[cfg(test)]` kept) | none |
  | `#[cfg(test)] mod retry_conformance` + its spec-mirror comment block | 819-957 (feature attr 829-837 deleted, `#[cfg(test)]` kept) | replace the stale comment line 827 `// pub(crate) in this module and feature-gated, so an integration test could not` + 828 continuation: change the sentence `Gated behind the same 7-feature cfg (default = []).` to `Compiled behind the module-level _http gate (default = []).` — everything else byte-identical |

  Then in `sdks/rust/src/providers/mod.rs`:
  1. DELETE the now-orphaned cfg'd imports at lines 2-11 (`crate::retry::{RetryCause, RetryEvent, RetryPolicy}`), 14-22 (`crate::types::{StopReason, ToolCall, Usage}`), 24-33 (`reqwest::header::HeaderMap`), 34-43 (`serde_json::Value`), 44-53 (`std::time::Duration`). The surviving header is exactly:
     ```rust
     use crate::error::MotosanError;
     use crate::stream::BoxStream;
     use crate::types::{ChatRequest, ChatResponse, ContentBlock, ProviderCapabilities};
     use async_trait::async_trait;
     ```
  2. Immediately after those imports, add the compat re-export so NO provider file changes an import (this is the explicit no-import-churn mechanism — `anthropic.rs`/`openai.rs`/`ollama.rs`/`gemini.rs`/`gemini_code_assist.rs`/`chatgpt_codex.rs` keep `use crate::providers::{...}` and the `crate::providers::collect_stream_with_total_timeout(...)` qualified calls at `anthropic.rs:482`, `gemini_code_assist.rs:132`, `chatgpt_codex.rs:267` keep resolving):
     ```rust
     // Shared transport helpers moved to src/transport/ (M4 Task 1). Re-exported
     // pub(crate) at their old paths so provider files need no import churn.
     #[cfg(feature = "_http")]
     pub(crate) use crate::transport::http::{
         apply_total_timeout, collect_stream_with_total_timeout, extract_error_message,
         extract_request_id, map_http_error, parse_retry_after, send_with_retry,
         ChatResponseBuilder,
     };
     ```

  Verify:
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  grep -c 'send_with_retry' src/providers/mod.rs        # expect: 1 (the re-export)
  grep -n 'pub(crate) async fn send_with_retry' src/transport/http.rs   # expect: one hit
  ```

- [ ] **Step 6: Create `src/transport/cli.rs`, re-gate `redacted_envs` (chunk 2 ends — commit)**

  Write `sdks/rust/src/transport/cli.rs` (complete file — the fn body and test assertions are byte-identical to `providers/mod.rs:291-325`; only the cfg gates changed, since the module decl already carries `_cli` and `collect_stream` no longer needs an HTTP feature after Step 9 — the test still compiles under `--all-features` in the meantime):

  ```rust
  //! Shared helpers for the CLI backends (claude-code / codex-cli / gemini-cli),
  //! compiled ONCE behind the private `_cli` umbrella feature.

  /// Terminal stop reason for a CLI turn.
  ///
  /// NOTE: scheduled for retirement in M4 Task 3 (F4 — CLI backends always
  /// report `EndTurn`). Moved here unchanged first so this diff stays
  /// mechanical.
  pub(crate) fn cli_terminal_stop_reason(saw_tool_call: bool) -> crate::types::StopReason {
      if saw_tool_call {
          crate::types::StopReason::ToolUse
      } else {
          crate::types::StopReason::EndTurn
      }
  }

  #[cfg(test)]
  mod cli_terminal_tests {
      use crate::stream::{collect_stream, BoxStream};
      use crate::types::{StopReason, StreamEvent};
      use tokio_stream::iter;

      #[tokio::test]
      async fn tool_call_terminal_reason_collects_as_tool_use() {
          // Direct truth table for both branches (the false→EndTurn arm was untested).
          assert_eq!(super::cli_terminal_stop_reason(false), StopReason::EndTurn);
          assert_eq!(super::cli_terminal_stop_reason(true), StopReason::ToolUse);
          let events = vec![
              StreamEvent::tool_call_start("call_1", "Read"),
              StreamEvent::tool_call_args_with_id("call_1", r#"{"path":"/tmp/x"}"#),
              StreamEvent::tool_call_end_with_id("call_1"),
              StreamEvent::done_with_stop_reason(super::cli_terminal_stop_reason(true)),
          ];
          let stream: BoxStream = Box::pin(iter(events.into_iter().map(Ok)));
          let resp = collect_stream(stream).await.expect("collect");
          assert_eq!(resp.stop_reason, StopReason::ToolUse);
      }
  }
  ```

  In `sdks/rust/src/providers/mod.rs`:
  1. DELETE `cli_terminal_stop_reason` (291-297 incl. its attr at 290) and `mod cli_terminal_tests` (299-325 incl. the `cfg(all(feature = "gemini", any(...)))` attr — that `all(gemini, ...)` hack existed only because `collect_stream` was HTTP-gated; it dies here).
  2. Add next to the `_http` re-export from Step 5:
     ```rust
     #[cfg(feature = "_cli")]
     pub(crate) use crate::transport::cli::cli_terminal_stop_reason;
     ```
     (keeps `super::cli_terminal_stop_reason` resolving at `claude_code/mod.rs:617`, `codex_cli/mod.rs:598`, `gemini_cli/mod.rs:394` — zero edits in those files.)
  3. Re-gate the shared CLI env-redaction module (line 642): replace `#[cfg(any(feature = "claude-code", feature = "codex-cli", feature = "gemini-cli"))]` above `pub mod redacted_envs;` with `#[cfg(feature = "_cli")]` (public path `providers::redacted_envs` unchanged → non-breaking).

  Gate + commit chunk 2 (do NOT run cargo-hack yet — `-F _http` alone only turns green after Step 9 un-gates `stream.rs`):
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  cargo fmt
  cargo clippy --all-features --all-targets -- -D warnings
  cargo test --all-features
  cargo check --no-default-features
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git add sdks/rust/src/
  git commit -m "refactor(rust): move shared HTTP/CLI transport helpers into src/transport

  Byte-identical moves from providers/mod.rs; old import paths preserved via
  pub(crate) re-exports, so provider files carry zero import churn.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```
  Expected: clippy clean, all existing tests pass unchanged (`test result: ok.` for every suite), no-default-features check clean.

- [ ] **Step 7: Replace remaining enumerations in `providers/mod.rs` (chunk 3 begins)**

  Two sites survive in `providers/mod.rs` after the moves (locate by content — line numbers shifted):
  1. The 9-line `#[cfg(any(feature = "anthropic", ... feature = "chatgpt-codex",))]` above `impl Provider {` / `uses_http_transport` (was 78-86) → replace the whole attribute with `#[cfg(feature = "_http")]` (single-feature attribute, not an enumeration; the impl stays in `providers/mod.rs` because it is an inherent impl on `Provider`).
  2. Confirm no others: `grep -c 'cfg(any(' src/providers/mod.rs` → expect `0`.

- [ ] **Step 8: Replace enumerations in `client.rs` (chunk 3)**

  Failing check first: `grep -c 'cfg(any(' sdks/rust/src/client.rs` → `11`.

  Apply, matching each old attribute verbatim (pre-Step-4 line refs; locate by the anchor named):
  | Anchor (verified) | Old gate | New |
  |---|---|---|
  | `http: reqwest::Client` field (was :157-165) | 7-list w/ `ollama` | `#[cfg(feature = "_http")]` |
  | `pub async fn stream_collect` (was :250-256) | 5-list | DELETE the attribute — method becomes ungated (additive: now also on CLI-only/no-feature builds; calls only ungated `stream()` + `collect_stream`) |
  | `pub async fn stream_collect_with` (was :274-280) | 5-list | DELETE the attribute (same rationale) |
  | `if self.provider.uses_http_transport()` block in `dispatch_stream` (was :314-322) | 7-list w/ `ollama_native` | `#[cfg(feature = "_http")]` |
  | `let http = reqwest::Client::builder()` in `build()` (was :1051-1059) | 7-list w/ `ollama` | `#[cfg(feature = "_http")]` |
  | `http,` field init in `build()` (was :1118-1126) | 7-list w/ `ollama` | `#[cfg(feature = "_http")]` |
  | `struct ReadTimeoutStream` (was :1138-1146) | 7-list | `#[cfg(feature = "_http")]` |
  | `impl ReadTimeoutStream` (was :1154-1162) | 7-list | `#[cfg(feature = "_http")]` |
  | `impl futures_core::Stream for ReadTimeoutStream` (was :1174-1182) | 7-list | `#[cfg(feature = "_http")]` |
  | test fn `read_timeout_yields_error_once_then_ends` (was :1419-1427) | 5-list | `#[cfg(feature = "_http")]` (it constructs `ReadTimeoutStream`) |

  LEAVE UNTOUCHED: the two `#[cfg(any(feature = "minimax", feature = "ollama_native"))]` attributes on the imports inside `mod dispatch_validation_tests` (was :1438-1440) — genuine per-provider test gates, not transport enumerations; and every single-feature `#[cfg(feature = "...")]`/`#[cfg(not(feature = "..."))]` Stratum-2 gate.

  Verify: `grep -c 'cfg(any(' src/client.rs` → `2` (only the dispatch_validation_tests pair).

- [ ] **Step 9: Un-gate `stream.rs` completely + unconditional `collect_stream` export (chunk 3 ends — commit)**

  Failing check first: `cargo check --no-default-features 2>&1 | grep -c collect_stream` is not meaningful pre-change because the fn is cfg'd OUT under no features — prove it instead with:
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  cat > /tmp/m4-collect-probe.rs <<'EOF'
  // probe: collect_stream must exist under --no-default-features
  fn _probe() { let _ = motosan_ai::collect_stream; }
  EOF
  grep -n 'cfg(any(' src/stream.rs src/lib.rs
  ```
  Expected before: hits at `src/stream.rs:23`, `src/stream.rs:158`, `src/lib.rs:34`.

  Edits:
  1. `sdks/rust/src/stream.rs` — DELETE the 8-line attribute at 23-30 above `pub async fn collect_stream` and the 8-line attribute at 158-164 above `mod thinking_collect_tests` (keep the `#[cfg(test)]` at 157). `tokio_stream::StreamExt`/`iter` now resolve unconditionally.
  2. `sdks/rust/src/lib.rs` — DELETE the attribute at 34-39 so the export reads plainly `pub use stream::collect_stream;`.

  Gate — first full-matrix run + AFTER evidence (final acceptance for chunks 1-3):
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  cargo fmt
  cargo clippy --all-features --all-targets -- -D warnings
  cargo test --all-features
  cargo check --no-default-features          # must compile stream.rs incl. collect_stream
  cargo hack check --each-feature            # full matrix, run locally once
  # -- acceptance greps (spec "no shared-code enumerations") --
  grep -rn 'feature = "gemini-code-assist"' src/providers/ src/transport/ src/stream.rs src/lib.rs | wc -l
  grep -rn -A8 'cfg(any(' src/ | grep -c 'gemini-code-assist'
  grep -rn 'feature = "gemini-code-assist"' src/ | wc -l
  ```
  Expected: clippy clean; all tests pass; no-default-features check clean; `cargo hack check --each-feature` runs 16 combinations (none, `_cli`, `_http`, `anthropic`, `chatgpt-codex`, `claude-code`, `codex-cli`, `full`, `gemini`, `gemini-cli`, `gemini-code-assist`, `minimax`, `ollama`, `ollama-native`, `ollama_native`, `openai`) and every one finishes without error (dead-code WARNINGS in single-feature combos, e.g. unused `ChatResponseBuilder` under `-F chatgpt-codex`, are expected and acceptable — check exits 0). Greps: `1` (only `#[cfg(feature = "gemini-code-assist")] pub mod gemini_code_assist;`), `0` (zero enumerations name the feature anywhere), `16` (the documented Stratum-2 remainder in `client.rs` — see deviation note 2; the spec-intent number "≤3 shared-code mentions" is satisfied by the first two greps). Re-run the Step-3 tree-diff loop once more; still empty for all 12 features.

  Commit:
  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git add sdks/rust/src/
  git commit -m "refactor(rust): swap per-provider cfg enumerations for umbrella gates

  Shared HTTP code now carries one module-level _http gate, CLI helpers one
  _cli gate; stream.rs (incl. collect_stream) compiles ungated under
  --no-default-features. cargo hack check --each-feature green locally.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 10: CI — cargo-hack each-feature guard (chunk 4 — commit)**

  The Rust workflow is `.github/workflows/ci-rust.yml` (verified; jobs `rust` and `rust-msrv-no-features`). In the `rust` job, append after the `Cargo test` step (line 29-30):

  ```yaml
        - name: Install cargo-hack
          uses: taiki-e/install-action@cargo-hack
        - name: Cargo hack (each feature)
          run: cargo hack check --each-feature --no-dev-deps
  ```
  (Indentation: 6 spaces for `- name:`, matching the existing steps. `--no-dev-deps` keeps the CI matrix lean; the with-dev-deps variant already ran locally in Step 9.)

  Verify: `python3 -c "import yaml,sys; yaml.safe_load(open('.github/workflows/ci-rust.yml')); print('yaml ok')"` → `yaml ok` (or `yq . .github/workflows/ci-rust.yml >/dev/null && echo yaml ok`).

  Commit:
  ```bash
  git add .github/workflows/ci-rust.yml
  git commit -m "ci(rust): guard feature matrix with cargo-hack each-feature

  Converts a missed _http/_cli gate from 'breaks some feature combo someday'
  into 'fails every PR'.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 11: Documentation rules (chunk 5 — commit)**

  1. `sdks/rust/README.md` — replace lines 103-115 (the `## Features` heading + bullet list; the old list wrongly omitted `chatgpt-codex`) with:

     ```markdown
     ## Features

     All default-off (`default = []`). Public provider features:

     - `anthropic`
     - `openai`
     - `minimax` (routing alias — Anthropic-compatible endpoint)
     - `ollama` (OpenAI-compatible mode)
     - `ollama-native` (native `/api/chat` endpoint with NDJSON streaming)
     - `ollama_native` (permanent alias for `ollama-native`, kept for backwards compatibility)
     - `gemini` (Google Generative AI HTTP API)
     - `gemini-code-assist` (Google Cloud Code Assist HTTP API; depends on `gemini`)
     - `chatgpt-codex` (ChatGPT-backend Responses HTTP API)
     - `claude-code` (local Claude Code CLI backend)
     - `codex-cli` (local Codex CLI backend)
     - `gemini-cli` (local Gemini CLI backend)
     - `full` (every HTTP provider above)

     ### Feature architecture rules

     1. Features whose names start with an underscore (`_http`, `_cli`) are internal
        aggregation layers: an implementation detail, NOT covered by semver. Never
        enable or depend on them directly.
     2. New providers MUST route through `_http` (HTTP transports) or `_cli` (local
        CLI backends) in `[features]`. Shared transport code lives in `src/transport/`
        behind a single module-level gate — adding a new per-provider
        `#[cfg(any(...))]` enumeration in shared code is a review-blocking offense.
     3. Docs and examples teach `ollama-native`; `ollama_native` remains a permanent
        alias with identical semantics.
     ```

  2. `AGENTS.md` — replace line 86 (`4. Gate with \`#[cfg(feature = "<name>")]\` and add the feature to \`Cargo.toml\`.`) with:

     ```markdown
     4. Gate with `#[cfg(feature = "<name>")]` and add the feature to `Cargo.toml`, routed through an umbrella: HTTP transports declare `<name> = ["_http"]`, CLI backends `<name> = ["_cli"]`. Underscore features are internal-only and NOT semver-covered — never enable them directly. Adding a per-provider `#[cfg(any(...))]` enumeration in shared code (`src/transport/`, `client.rs`, `stream.rs`, `providers/mod.rs`) is review-blocking. Docs teach `ollama-native`; `ollama_native` is a permanent alias.
     ```

  Commit:
  ```bash
  git add sdks/rust/README.md AGENTS.md
  git commit -m "docs(rust): document umbrella feature rules and ollama-native alias

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 12: Full gate + PR**

  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan/sdks/rust
  cargo fmt
  cargo clippy --all-features --all-targets -- -D warnings
  cargo test --all-features
  cargo check --no-default-features
  cargo hack check --each-feature
  git status --short   # expect: clean (fmt produced no changes)
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git push -u origin refactor/m4-rust-feature-arch
  gh pr create --title "refactor(rust): feature architecture — _http/_cli umbrellas + src/transport (M4 PR-F)" --body "$(cat <<'EOF'
  Implements docs/superpowers/specs/2026-07-17-rust-feature-architecture-design.md (M4 Task 1, F1). Mechanical, NON-breaking.

  - [features] deduped: one _http dep set, one _cli dep set; tokio-stream promoted to unconditional; NEW public alias ollama-native = ["ollama_native"]; full += ollama-native
  - Shared HTTP helpers (send_with_retry & co. + retry_conformance/retry_after/http_error_metadata test mods) moved to src/transport/http.rs behind ONE module-level _http gate; cli_terminal_stop_reason to src/transport/cli.rs behind _cli; old crate::providers::* paths preserved via pub(crate) re-exports (zero provider-file import churn)
  - TimeoutConfig moved client.rs → transport/mod.rs (UNGATED root, not http.rs: its public accessors exist under --no-default-features; gating them would be breaking)
  - stream.rs fully un-gated; collect_stream exported unconditionally
  - Evidence: cargo tree resolved-deps diff EMPTY for all 12 pre-existing features; zero cfg(any(...)) provider enumerations remain (shared-code gemini-code-assist mentions: 1 = the mod decl; whole-src literal count 16 = Stratum-2 single-feature gates in client.rs which the spec leaves unchanged)
  - CI: cargo hack check --each-feature --no-dev-deps added to ci-rust.yml (16 combos green locally)
  - Docs: README + AGENTS.md feature-architecture rules; docs teach ollama-native

  🤖 Generated with [Claude Code](https://claude.com/claude-code)
  EOF
  )"
  ```
  Expected: all five gate commands clean; PR opened. Done criteria for the task = the Step 9 acceptance greps + empty tree diffs + green CI on this PR (Rust CI now includes the each-feature matrix).

---

### Task 2: specs/types.md — Stream-Event Vocabulary, CLI Contract, Token-Source Docs (PR-S, docs-only)

**Branch:** `docs/m4-vocab-cli-token-spec` (PR group PR-S, base `origin/main` @ b9bcc3e).
**NOTE:** this repo intentionally has **NO CI checks for docs-only PRs** — the deliverable proof is the diff itself. No code steps, no test steps; every "verify" step below is a grep/Read confirmation of a factual claim that the spec text asserts.

**Files:**
- Modify: `specs/types.md` — replace the `## StreamEventType` block (specs/types.md:123-125); insert two new `##` sections between `### Cancellation` (ends specs/types.md:209) and `## MotosanError (Rust)` (specs/types.md:211)
- Modify: `specs/retry.md` — one sentence after the "One retry engine per SDK" table (Rust engine row at specs/retry.md:170; insertion point between specs/retry.md:172 and the `## Error metadata` heading at specs/retry.md:174)
- Create: none. Test: none (docs-only).

**Interfaces:**
- Consumes (verified at b9bcc3e — every claim below was confirmed in source; if line numbers have drifted, re-anchor by content, do not skip verification):
  - Rust `StreamEventType` enum = 7 variants, no `Done` — `sdks/rust/src/types.rs:662-703`; `ThinkingDelta` docstring :669-690, `ThinkingDone` docstring :691-702 ("full concatenated thinking text… always precedes any Text events for the final answer"); terminal constructors `done()`/`done_with_stop_reason()` set `done: true` with default `event_type: Text` — `sdks/rust/src/types.rs:774-799`
  - Rust Anthropic emits `ThinkingDelta` at `sdks/rust/src/providers/anthropic.rs:1073` and `ThinkingDone` at :1111 (emitted **even for an empty thinking block**, comment :1104-1110)
  - Rust ChatGPT Codex emits `ThinkingDelta` at `sdks/rust/src/providers/chatgpt_codex.rs:372` (from `response.reasoning_text.delta` | `response.reasoning_summary_text.delta`, :369); never emits `ThinkingDone`; test `adapter_maps_reasoning_delta_to_thinking` filters `StreamEventType::ThinkingDelta` at :859
  - Rust collector priority: `thinking_done` buffer beats concatenated `thinking_delta` fallback — `sdks/rust/src/stream.rs:48-53`
  - TS `StreamEventType` union already has both members — `sdks/typescript/src/types.ts:118-125` (`thinking_delta` :124, `thinking_done` :125); TS Anthropic emits `thinkingDelta` at `sdks/typescript/src/providers/anthropic.ts:320` and `thinkingDone` at :339 (**suppressed for empty blocks**, :336-340); TS ChatGPT Codex emits `thinkingDelta` at `sdks/typescript/src/providers/chatgpt_codex.ts:265`; TS collector priority at `sdks/typescript/src/stream.ts:174-181, 191-196`
  - Python today emits the **untyped string** `event_type="thinking"` — `sdks/python/motosan_ai/providers/anthropic.py:512`, `sdks/python/motosan_ai/providers/chatgpt_codex.py:85`; Python `StreamEventType` StrEnum has only 5 members — `sdks/python/motosan_ai/types.py:24-29`; `_stream_collect.py:39-40` concatenates `"thinking"`. Task 5 (F3) migrates these — this spec documents the **post-M4** reality with a migration note
  - F4 facts: `cli_terminal_stop_reason` at `sdks/rust/src/providers/mod.rs:290-297` (post-Task-1 location: locate by name, likely under the `_cli`-gated transport module) — retired by the F4 code task; CLI providers are Rust `sdks/rust/src/providers/{claude_code/,codex_cli/,gemini_cli/}` and Python `sdks/python/motosan_ai/providers/{claude_code.py,codex_cli.py,gemini_cli.py}`; `sdks/typescript/src/providers/` contains only `anthropic.ts chatgpt_codex.ts gemini.ts minimax.ts ollama.ts openai.ts` — no CLI backends
  - F5 facts: TS constructor today is `private readonly accessToken: string` (`sdks/typescript/src/providers/chatgpt_codex.ts:66`); Python `openai_chatgpt` validation requires `access_token` at `sdks/python/motosan_ai/client.py:110-114`; OAuth workspace crates are `sdks/rust/crates/{anthropic-oauth,codex-oauth,motosan-ai-oauth}` (root `Cargo.toml:4-6`); `specs/retry.md:170` names `send_with_retry(policy, build)` — the request-build closure IS documented there, so the one-sentence async-build addition applies
- Produces (anchors later tasks / the changelog task reference):
  - `specs/types.md#streameventtype` — rewritten 7-value vocabulary + Emitters table + Python migration note
  - `specs/types.md#cli-backend-chatstream-contract` — normative F4 contract (cited by F4 code-task commit messages and the CHANGELOG BREAKING entries)
  - `specs/types.md#token-sources-chatgpt-codex` — normative F5 seam (cited by the F5 tasks and linked from retry.md)
  - `specs/retry.md` — one sentence permitting an async per-attempt build step (`send_with_retry_async_build`)

**Ordering note:** PR-S documents the post-M4 (Rust 0.25.0 / Python 0.18.0 / TS 0.15.0) contract and carries explicit version markers, so it may merge before or after the F4/F5 code PRs. If Task 1 already landed and updated the engine-home path on specs/retry.md:170 from `providers/mod.rs` to `transport/http.rs`, keep Task 1's path — the sentence added here is path-agnostic.

**Flip list:** none — docs-only; no test changes anywhere in this task.

---

- [ ] **Step 1: Create the branch**

  ```bash
  cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan
  git fetch origin && git checkout -b docs/m4-vocab-cli-token-spec origin/main
  ```

  Expected: `branch 'docs/m4-vocab-cli-token-spec' set up to track 'origin/main'.` (or plain `Switched to a new branch`). If PR-S stacks on Task 1's branch per the plan preamble, base on that branch instead — the edits below are anchored by content, not line numbers, so they apply either way.

- [ ] **Step 2: Verify the Rust emitter facts the spec will assert**

  ```bash
  grep -n 'StreamEvent::thinking_delta\|StreamEvent::thinking_done' sdks/rust/src/providers/anthropic.rs
  ```
  Expected: two hits — `1073:` (`StreamEvent::thinking_delta(`) and `1111:` (`StreamEvent::thinking_done(buf)`).

  ```bash
  grep -n 'StreamEvent::thinking_delta\|StreamEvent::thinking_done' sdks/rust/src/providers/chatgpt_codex.rs
  ```
  Expected: exactly one hit — `372:` (`thinking_delta`); **no** `thinking_done` hit (this is the "Codex never emits thinking_done" claim).

  ```bash
  grep -n '    ThinkingDelta,\|    ThinkingDone,' sdks/rust/src/types.rs
  grep -n 'thinking_done_buf' sdks/rust/src/stream.rs | head -3
  ```
  Expected: variants at `types.rs:690` and `:702`; `stream.rs` hits around `:50-53` (priority comment + buffer). Read `sdks/rust/src/types.rs:691-702` and confirm the ThinkingDone docstring says the event carries the full concatenated text and "always precedes any Text events for the final answer" — the spec mirrors that sentence.

- [ ] **Step 3: Verify the TS/Python emitter facts and the CLI-backend inventory**

  ```bash
  grep -n "thinking_delta\|thinking_done" sdks/typescript/src/types.ts
  grep -rn "thinkingDelta(\|thinkingDone(" sdks/typescript/src/providers/
  ```
  Expected: `types.ts` union members at `:124`/`:125`; provider emissions at `anthropic.ts:320` (`thinkingDelta`), `anthropic.ts:339` (`thinkingDone`, inside a `buf.length > 0` guard — the empty-block suppression), `chatgpt_codex.ts:265` (`thinkingDelta`). Note: `types.ts:115-116` has a stale doc comment claiming "Anthropic only" — do NOT fix it in this docs-only PR; it belongs to the TS code task.

  ```bash
  grep -n 'event_type="thinking"' sdks/python/motosan_ai/providers/anthropic.py sdks/python/motosan_ai/providers/chatgpt_codex.py
  ls sdks/typescript/src/providers/
  grep -rn "cli_terminal_stop_reason" sdks/rust/src/ | head -5
  ```
  Expected: Python untyped-string emissions at `anthropic.py:512` and `chatgpt_codex.py:85`; the TS providers listing shows six HTTP providers and no `*_cli`/`*_code` entries; `cli_terminal_stop_reason` is found (providers/mod.rs:291 pre-Task-1, or the `_cli` transport module post-Task-1 — if the F4 code task already landed, the helper is gone; that is fine, the spec documents its retirement).

- [ ] **Step 4: Edit 1 — replace the `## StreamEventType` block (specs/types.md:123-125)**

  Replace this exact block (old text):

  ```markdown
  ## StreamEventType

  `text` | `tool_call_start` | `tool_call_args` | `tool_call_end` | `usage` | `done`
  ```

  with (new text — full content, verbatim):

  ```markdown
  ## StreamEventType

  `text` | `tool_call_start` | `tool_call_args` | `tool_call_end` | `usage` | `thinking_delta` | `thinking_done`

  Seven values, identical across the SDKs (Rust enum `StreamEventType`,
  Python `StreamEventType` StrEnum, TypeScript `StreamEventType` string
  union). There is **no** `done` event type: stream termination is
  signalled by the `done: bool` **field** on `StreamEvent`, never by
  `event_type` (terminal events carry the default `event_type`, `text`).
  The set may grow additively as providers gain richer thinking wire
  formats; consumers matching on `event_type` should keep a fallback arm.

  ### Thinking events

  `thinking_delta` carries a partial extended-thinking delta in
  `content`, emitted while the model reasons before its final answer.
  `thinking_done` marks the end of a thinking block and carries the
  **full concatenated thinking text** in `content`; it is preceded by
  zero or more `thinking_delta` events for the same block and always
  precedes the `text` events of the final answer. Collectors
  (`collect_stream` / `_stream_collect` / `collectStream`) assemble
  `ChatResponse.thinking` with the `thinking_done` payload taking
  priority; concatenated `thinking_delta` content is the fallback for
  providers that never emit `thinking_done`.

  ### Emitters

  | SDK | Provider | `thinking_delta` | `thinking_done` |
  |-----|----------|------------------|-----------------|
  | Rust | Anthropic | ✅ | ✅ — emitted even for an empty thinking block |
  | Rust | ChatGPT Codex | ✅ (reasoning + reasoning-summary deltas) | ❌ |
  | Python | Anthropic | ✅ (0.18.0+) | ✅ (0.18.0+) — mirrors Rust, incl. empty blocks |
  | Python | ChatGPT Codex | ✅ (0.18.0+) | ❌ |
  | TypeScript | Anthropic | ✅ | ✅ — suppressed for an empty thinking block |
  | TypeScript | ChatGPT Codex | ✅ (reasoning + reasoning-summary deltas) | ❌ |

  No other provider emits thinking events. The empty-block divergence
  (Rust/Python emit `thinking_done` with empty `content`; TypeScript
  emits nothing) is documented reality, not a bug to fix.

  **Python migration note (0.18.0, BREAKING).** Pre-0.18.0 the Python
  Anthropic and ChatGPT Codex adapters emitted the **untyped string**
  `event_type="thinking"` — not a `StreamEventType` member — and never
  emitted `thinking_done`. 0.18.0 replaces `"thinking"` with
  `thinking_delta` (both providers) and adds `thinking_done`
  (Anthropic). Consumers matching `"thinking"` break and must migrate.
  `StreamEvent.event_type` stays annotated `str` (StrEnum members are
  `str`).
  ```

  Verify: `grep -n '`usage` | `done`' specs/types.md` returns nothing, and `grep -c 'thinking_delta' specs/types.md` returns a nonzero count.

- [ ] **Step 5: Edit 2 — insert `## CLI backend chat/stream contract` after the Cancellation section**

  Anchor: the `### Cancellation` section ends at specs/types.md:209 (`  \`CancelledError\`.`), directly followed by `## MotosanError (Rust)` at :211. Insert the new section between them, i.e. Edit with old_string:

  ```markdown
  `CancelledError`.

  ## MotosanError (Rust)
  ```

  and new_string = the same two anchor fragments with this full section in between (verbatim):

  ```markdown
  ## CLI backend chat/stream contract

  Applies to the six CLI-spawning backends: Rust
  `sdks/rust/src/providers/claude_code/`, `codex_cli/`, `gemini_cli/`
  and Python `sdks/python/motosan_ai/providers/claude_code.py`,
  `codex_cli.py`, `gemini_cli.py`. TypeScript has no CLI backends.
  Normative from Rust 0.25.0 / Python 0.18.0 (BREAKING — see CHANGELOG).

  - **`stop_reason` is always `end_turn`.** A successfully completed CLI
    turn reports `stop_reason = end_turn` on **both** the `chat()` and
    the `stream()` path. CLI backends never report `tool_use`: their
    tools are executed internally by the CLI process, and `tool_use`
    means "the caller must execute tools" — something a CLI backend
    never requests. (The pre-0.25.0 / pre-0.18.0 behavior — `tool_use`
    whenever the transcript contained a tool call — made agent loops
    re-execute already-executed tools.)
  - **`ChatResponse.tool_calls` is a record, not a request.** For a CLI
    backend it lists the tools the CLI already executed during the turn;
    callers MUST NOT execute them.
  - **`chat()` ≡ collect(`stream()`).** Every CLI backend implements
    `chat()` by collecting its own `stream()` (Rust `collect_stream`,
    Python `_stream_collect`), so `content` / `thinking` / `tool_calls`
    / `stop_reason` / `usage` / `session_id` parity holds by
    construction. The single documented parity exception: `chat()` may
    backfill `ChatResponse.model` from provider config when the
    collected value is empty.
  - CLI backends perform no transport-level retry — see
    [`retry.md` § CLI backends](./retry.md#cli-backends).
  ```

  Verify: `grep -n '^## CLI backend chat/stream contract' specs/types.md` returns one hit between the Cancellation bullets and `## MotosanError (Rust)`.

- [ ] **Step 6: Edit 3 — insert `## Token sources (ChatGPT Codex)` directly after the CLI contract section**

  Anchor on the CLI section's final line plus the `## MotosanError (Rust)` heading; insert this full section between them (verbatim):

  ```markdown
  ## Token sources (ChatGPT Codex)

  The ChatGPT Codex provider authenticates with a short-lived OAuth
  bearer token. Each SDK exposes a **token source** seam so long-running
  processes can supply a fresh token without rebuilding the client; the
  token is resolved **once per retry attempt** — a retried request never
  reuses a token fetched for an earlier attempt. Introduced in Rust
  0.25.0 / Python 0.18.0 / TypeScript 0.15.0.

  | SDK | Seam |
  |-----|------|
  | Rust | `pub trait TokenSource` in ungated `src/auth.rs` — `async fn access_token(&self) -> Result<String, MotosanError>` — plus `StaticTokenSource(String)` for fixed tokens. `ChatGptCodexProvider` stores `Arc<dyn TokenSource>`; `new()` keeps its `access_token: String` signature (wraps `StaticTokenSource`); `with_token_source` and `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)` inject a dynamic source. The per-attempt fetch runs inside the shared retry engine via its async-build variant (see [`retry.md` § One retry engine per SDK](./retry.md#one-retry-engine-per-sdk)) |
  | Python | `token_source: Callable[[], Awaitable[str]] \| None = None` on `ChatGptCodexProvider` and `Client.chatgpt_codex()`; exactly one of `access_token` / `token_source` is required; when set, the bearer token is resolved at the top of every retry attempt |
  | TypeScript | constructor `accessToken: string \| (() => Promise<string>)`; a function value is awaited once per attempt |

  - The SDKs never depend on the OAuth crates
    (`sdks/rust/crates/anthropic-oauth`, `codex-oauth`,
    `motosan-ai-oauth`): a refreshing token source is caller-supplied
    glue built on top of them.
  - Token material MUST NOT appear in `Debug` / `repr` / log output;
    the Rust provider implements a custom `Debug` that redacts it.
  ```

  Verify: `grep -n '^## Token sources (ChatGPT Codex)' specs/types.md` returns one hit; the section sits after the CLI contract and before `## MotosanError (Rust)`.

- [ ] **Step 7: specs/retry.md — one sentence permitting an async per-attempt build**

  specs/retry.md:170 documents the Rust engine as `send_with_retry(policy, build)` — the request-build closure is documented, so per the task brief add exactly one sentence. Insert it as its own paragraph between the engine table (ends specs/retry.md:172) and the `## Error metadata` heading (specs/retry.md:174), i.e. anchored after this table row:

  ```markdown
  | TypeScript | `withRetry(policy, op, classify)` in `sdks/typescript/src/retry.ts` | classification via `isRetryableStatus` / `isRetryableNetworkError` |
  ```

  New paragraph (verbatim, one sentence):

  ```markdown
  The Rust `build` closure runs once per attempt; from 0.25.0 the engine also exposes `send_with_retry_async_build` — with `send_with_retry` as a thin delegating wrapper over it — so the per-attempt build step may be `async`, e.g. to consult a `TokenSource` for a fresh bearer token (see [`types.md` § Token sources](./types.md#token-sources-chatgpt-codex)).
  ```

  If Task 1 already changed the engine home on specs/retry.md:170 from `sdks/rust/src/providers/mod.rs` to `sdks/rust/src/transport/http.rs`, keep Task 1's path — this sentence names no file path and merges cleanly.

  Verify: `grep -n 'send_with_retry_async_build' specs/retry.md` returns one hit between the engine table and `## Error metadata`.

- [ ] **Step 8: Whole-file self-check**

  Read both modified specs end-to-end (`specs/types.md`, `specs/retry.md`) and confirm: (a) the StreamEventType list has 7 values and no `done`; (b) both new `##` sections sit between `### Cancellation` and `## MotosanError (Rust)`; (c) all intra-repo links resolve (`./retry.md#cli-backends`, `./retry.md#one-retry-engine-per-sdk`, `./types.md#token-sources-chatgpt-codex`); (d) no other section was disturbed.

  ```bash
  git diff --stat
  ```
  Expected: exactly two files changed — `specs/retry.md | 2 +` (approx) and `specs/types.md` with ~100 insertions / 1 deletion. Any other file in the diff is a mistake — revert it.

- [ ] **Step 9: Commit and push (docs-only PR, PR group PR-S)**

  ```bash
  git add specs/types.md specs/retry.md
  git commit -m "docs(specs): stream event vocabulary, CLI contract, token sources (M4)

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  git push -u origin docs/m4-vocab-cli-token-spec
  ```

  Expected: one commit touching only the two spec files. Open the PR against `main` per the PR-S group instructions in the plan preamble. Reminder: this repo intentionally runs **no CI on docs-only PRs** — the reviewable diff is the deliverable proof; paste the `git diff --stat` output into the PR body.

---

### Task 3: Rust CLI chat/stream contract (PR-R first half; BREAKING — F4)

**Branch context:** PR group **PR-R**, branch `feat/m4-rust-cli-token`, created off `origin/main` AFTER PR-F (Task 1, feature architecture) and PR-S (spec) are merged. This task references two Task-1 outcomes: (a) `src/stream.rs` has lost its feature gate entirely (`tokio-stream` promoted to unconditional), so `collect_stream` is callable from CLI-feature-only builds; (b) `cli_terminal_stop_reason` may have moved from `providers/mod.rs:291` to the `_cli`-gated transport module — **locate it by name**, never by the pre-Task-1 line number.

All line numbers below were verified at `origin/main` @ `b9bcc3e` (Rust 0.24.0). Where Task 1 moves code, the step says "post-Task-1 location" and gives the grep to find it.

**Files:**

- Modify: `sdks/rust/src/providers/claude_code/mod.rs` — `chat()` at :453-471 (hardcodes `thinking: None` :464, `tool_calls: vec![]` :465, `StopReason::EndTurn` :468 from `spawn::invoke_cli` :460); stream terminal :617; `saw_tool_call` :565/:602; module doc :10-29; `agent_mode` field doc :62-64; tests mod :797+
- Modify: `sdks/rust/src/providers/claude_code/spawn.rs` — delete `build_command` :369-386, `invoke_cli` :388-471, `parse_agent_json` :473-538, dead imports :19-23, dead tests :583/:595/:612/:1058/:1069; module doc :1-13
- Modify: `sdks/rust/src/providers/codex_cli/mod.rs` — `chat()` at :427-445 (parses `thinking` via `invoke_cli` :434 but `tool_calls: vec![]` :439, `StopReason::EndTurn` :442); stream terminal :598; `saw_tool_call` :547/:587; inline stream `Command` construction :480-490; module doc :15-34; `chat` doc :409-426
- Modify: `sdks/rust/src/providers/codex_cli/spawn.rs` — `build_command` :246 gains `pub(super)` and is reused by `stream()`; delete `invoke_cli` :274-337, `parse_collected_stream` :345-397, dead imports :12/:15/:16/:18, dead tests :772/:798/:812/:827/:837
- Modify: `sdks/rust/src/providers/gemini_cli/mod.rs` — `chat()` at :244-261 (`invoke_cli` :250, `tool_calls: vec![]` :255, `StopReason::EndTurn` :258); stream terminal :394; `saw_tool_call` :344/:383; inline stream `Command` construction :277-286
- Modify: `sdks/rust/src/providers/gemini_cli/spawn.rs` — `build_command` :186 gains `pub(super)` and is reused by `stream()`; delete `invoke_cli` :214-281, `parse_collected_stream` :288-331, dead imports :19/:22/:23/:25, dead tests :649/:673/:683
- Modify: `sdks/rust/src/providers/mod.rs:290-325` (post-Task-1 location: wherever `fn cli_terminal_stop_reason` lives, likely `src/transport/` under the `_cli` gate) — delete the helper + `mod cli_terminal_tests`
- Modify: `sdks/rust/src/providers/chatgpt_codex.rs:339-342` — doc comment references the deleted helper by name; reword (no behavior change: chatgpt-codex is an HTTP provider, out of F4 scope, its callers DO execute tools)
- Test: new `#[cfg(unix)] mod chat_stream_parity` inside each of the three providers' existing `#[cfg(test)] mod tests`

**Interfaces:**

- Consumes: `crate::stream::collect_stream(stream: BoxStream) -> Result<ChatResponse, MotosanError>` (`src/stream.rs:31`; post-Task-1 UNGATED — verified in Step 0). Its terminal handling captures an explicit `event.stop_reason` from the done event (`stream.rs:60-68`) so the tool-calls heuristic at `stream.rs:128-132` never fires for CLI streams.
- Consumes: `StreamEvent::done_with_stop_reason(stop_reason: StopReason) -> StreamEvent` (`src/types.rs:789-801`).
- Consumes: each provider's `drive_lines(child, reader, read_timeout: Option<Duration>) -> BoxStream` and `SpawnConfig.timeout` (threaded from the provider's `timeout` field via `build_spawn_config`: claude :448, codex :405, gemini :239; handed to `drive_lines` at claude mod.rs:549, codex :531, gemini :328).
- Produces: **CLI chat/stream contract** — for `ClaudeCodeProvider` / `CodexCliProvider` / `GeminiCliProvider`, `chat()` ≡ `collect_stream(stream())` on content/thinking/tool_calls/stop_reason/usage/session_id; both paths terminate with `StreamEvent::done_with_stop_reason(StopReason::EndTurn)`; `ChatResponse.tool_calls` is the record of tools the CLI already executed. One documented exception: `chat()` backfills `ChatResponse.model` from request/provider config when the collected value is empty. Task 4 (Python F4) mirrors this contract; the PR-R second half and the spec task cite it.
- Deletes: `cli_terminal_stop_reason` (pub(crate) helper), `spawn::invoke_cli` × 3, claude `spawn::build_command` + `parse_agent_json`, codex/gemini `spawn::parse_collected_stream`. Public API is unchanged except behavior (BREAKING F4, released as Rust 0.25.0 by the release task — do NOT edit CHANGELOG.md in this task; the release task owns it).

**Timeout / error-mapping audit (why delegation preserves chat()'s guarantees):**

`invoke_cli` (blocking path) wrapped `child.wait_with_output()` in `tokio::time::timeout(config.timeout)` — a total-wall-clock bound (claude spawn.rs:415-439, codex spawn.rs:299-323, gemini spawn.rs:239-263). The delegated path applies the SAME `SpawnConfig.timeout` as a per-line read-idle bound inside `drive_lines` (claude mod.rs:568-577, codex :550-559, gemini :347-356), passed at claude mod.rs:549 / codex :531 / gemini :328. Mapping after delegation:

| Failure | old `chat()` (invoke_cli) | new `chat()` (collect_stream ∘ stream) |
|---|---|---|
| spawn failure | `ProviderError("failed to spawn <cli> CLI: …")` (claude spawn.rs:395-400) | identical `ProviderError` from `stream()` (claude mod.rs:508-513, codex :492-497, gemini :288-293) |
| CLI error terminal | `ProviderError` — claude agent-mode only (spawn.rs:489-502), codex (spawn.rs:377-384), gemini (spawn.rs:318-325) | `ProviderError` with the same detail via `NdjsonAction::Error` (claude stream_json.rs:142-144 → mod.rs:620-629; codex stream_json.rs:200-208 → mod.rs:601-610; gemini stream_json.rs:145-151 → mod.rs:397-406). Claude non-agent `chat()` gains error-terminal detection it never had. |
| timeout | `ProviderError("<cli> CLI timed out after N seconds")` | `MotosanError::StreamReadTimeout(N)` — same `Duration`, typed variant from the M3 stream contract |
| non-zero exit / early EOF | `ProviderError("<cli> CLI exited with <status>: <stderr>")` | `MotosanError::Stream("<cli> CLI exited unexpectedly (status N): <stderr>")` via `abnormal_exit_error` |

The timeout and abnormal-exit variants shift to the M3 stream-contract variants (`StreamReadTimeout`, `Stream`) — this is the intended F4 outcome (one pipeline, one error contract) and is pinned by the new `chat_times_out_via_stream_read_timeout` tests below. Semantics preserved: a stalled or crashed CLI can never hang `chat()`, and every failure is a typed `MotosanError`.

**Documented behavior changes beyond stop_reason/tool_calls** (must appear in the PR description):

1. codex_cli `chat()`: preamble `agent_message`s are no longer folded into `thinking` (old `parse_collected_stream`, spawn.rs:339-397). All agent messages now concatenate into `content` in arrival order and `thinking` is `None` — exactly what `stream()` has always produced. Parity by construction wins over the old heuristic split.
2. claude_code `chat()` without `agent_mode`: now spawns `--output-format stream-json --verbose` (the stream path) instead of plain `--print`, so it reports real `usage` tokens and `session_id` (previously `0/0` and `None`, mod.rs doc :16-20), and detects CLI error terminals.

---

- [ ] **Step 0: Preflight — confirm the Task-1 landscape**

  ```bash
  cd /path/to/repo/sdks/rust
  grep -n 'cfg(' src/stream.rs | head -5
  ```
  Expected: NO `#[cfg(feature = …)]` attribute on `collect_stream` (Task 1 removed the gate that sat at stream.rs:23-30). If a feature gate is still present, STOP — PR-F has not merged; this branch's prerequisite is unmet.

  ```bash
  grep -rn 'fn cli_terminal_stop_reason' src/
  ```
  Expected: exactly one definition (pre-Task-1 it was `src/providers/mod.rs:291`; post-Task-1 it lives under the `_cli` gate in the transport module). Note the file — Step 10 deletes it there. No commit for this step.

- [ ] **Step 1: claude_code — write the failing parity + timeout tests**

  The existing fake-CLI infra in `claude_code/mod.rs` tests is (a) `drive_lines` fed a `Cursor` of raw NDJSON (`stream_surfaces_provider_error_as_err_item` :960) and (b) real `sh -c` children (`premature_child_exit_surfaces_status_and_stderr` :856, `#[cfg(unix)]`). Neither exercises `chat()`, so extend the same idea one notch: write an executable `#!/bin/sh` script that drains stdin and plays back the NDJSON transcript, and point the provider at it via the existing `ClaudeCodeProvider::with_path` (mod.rs:190). Reuses the outer tests' `test_request` helper (mod.rs:1145).

  Append inside `#[cfg(test)] mod tests` in `sdks/rust/src/providers/claude_code/mod.rs`:

  ```rust
      /// F4 parity: one fake-CLI transcript through `chat()` and through
      /// `collect_stream(stream())` must agree on content / thinking /
      /// tool_calls / stop_reason / usage / session_id. `model` is the one
      /// documented exception: `chat()` backfills it from provider config.
      #[cfg(unix)]
      mod chat_stream_parity {
          use super::*;
          use crate::types::StopReason;

          const TRANSCRIPT: &str = concat!(
              r#"{"type":"assistant","message":{"content":[{"type":"text","text":"checking"},{"type":"tool_use","id":"toolu_1","name":"Read","input":{"path":"/tmp/x"}}]}}"#,
              "\n",
              r#"{"type":"assistant","message":{"content":[{"type":"text","text":" done"}]}}"#,
              "\n",
              r#"{"type":"result","result":"done","usage":{"input_tokens":7,"output_tokens":3},"session_id":"sess_42"}"#,
          );

          /// Write an executable fake `claude` that ignores its argv, drains
          /// stdin (the provider writes the prompt then closes the pipe), and
          /// plays back `body` on stdout.
          fn write_fake_cli(test_name: &str, body: &str) -> std::path::PathBuf {
              use std::io::Write;
              use std::os::unix::fs::PermissionsExt;
              let path = std::env::temp_dir().join(format!(
                  "motosan-fake-claude-{test_name}-{}",
                  std::process::id()
              ));
              let mut f = std::fs::File::create(&path).expect("create fake CLI");
              write!(f, "#!/bin/sh\ncat > /dev/null\ncat <<'NDJSON'\n{body}\nNDJSON\n")
                  .expect("write fake CLI");
              f.set_permissions(std::fs::Permissions::from_mode(0o755))
                  .expect("chmod fake CLI");
              path
          }

          #[tokio::test]
          async fn chat_equals_collected_stream_and_reports_end_turn() {
              let bin = write_fake_cli("parity", TRANSCRIPT);
              let provider = || ClaudeCodeProvider::with_path(bin.clone()).model("sonnet");

              let chat_resp = provider()
                  .chat(test_request("hi"))
                  .await
                  .expect("chat should succeed");
              let stream = provider()
                  .stream(test_request("hi"))
                  .await
                  .expect("stream should start");
              let collected = crate::stream::collect_stream(stream)
                  .await
                  .expect("collect should succeed");
              let _ = std::fs::remove_file(&bin);

              // F4: chat()'s tool_calls = the executed-tool record from the CLI.
              assert_eq!(
                  chat_resp.tool_calls.len(),
                  1,
                  "chat() must surface the CLI's executed-tool record"
              );
              assert_eq!(chat_resp.tool_calls[0].id, "toolu_1");
              assert_eq!(chat_resp.tool_calls[0].name, "Read");
              assert_eq!(
                  chat_resp.tool_calls[0].input,
                  serde_json::json!({"path": "/tmp/x"})
              );
              assert_eq!(chat_resp.tool_calls, collected.tool_calls);

              // F4: a completed CLI turn ALWAYS reports end_turn — the CLI
              // already ran its tools; tool_use would make agent loops
              // re-execute them.
              assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
              assert_eq!(collected.stop_reason, StopReason::EndTurn);

              assert_eq!(chat_resp.content, "checking done");
              assert_eq!(chat_resp.content, collected.content);
              assert_eq!(chat_resp.thinking, None);
              assert_eq!(chat_resp.thinking, collected.thinking);
              assert_eq!(chat_resp.usage.input_tokens, 7);
              assert_eq!(chat_resp.usage.output_tokens, 3);
              assert_eq!(chat_resp.usage, collected.usage);
              assert_eq!(chat_resp.session_id.as_deref(), Some("sess_42"));
              assert_eq!(chat_resp.session_id, collected.session_id);

              // Documented F4 parity exception: model backfill from config.
              assert_eq!(chat_resp.model, "sonnet");
              assert_eq!(collected.model, "");
          }

          #[tokio::test]
          async fn chat_times_out_via_stream_read_timeout() {
              // SpawnConfig.timeout must still bound chat() on the delegated
              // path — as the M3 read-idle timeout, not the old total-wall
              // ProviderError.
              let bin = write_fake_cli("stall", "");
              // Overwrite with a stalling body: never emits a line.
              std::fs::write(&bin, "#!/bin/sh\nsleep 30\n").expect("write stall script");
              let provider = ClaudeCodeProvider::with_path(bin.clone())
                  .timeout(std::time::Duration::from_millis(50));
              let result = provider.chat(test_request("hi")).await;
              let _ = std::fs::remove_file(&bin);
              match result {
                  Err(crate::error::MotosanError::StreamReadTimeout(_)) => {}
                  other => panic!("expected StreamReadTimeout, got {other:?}"),
              }
          }
      }
  ```

  Run and confirm BOTH fail for the right reason:

  ```bash
  cd sdks/rust && cargo test --all-features claude_code::tests::chat_stream_parity
  ```
  Expected output (2 failures):
  ```
  thread '...chat_equals_collected_stream_and_reports_end_turn' panicked ...
  assertion `left == right` failed: chat() must surface the CLI's executed-tool record
    left: 0
   right: 1
  ...
  thread '...chat_times_out_via_stream_read_timeout' panicked ...
  expected StreamReadTimeout, got Err(ProviderError { message: "claude CLI timed out after 0 seconds", ... })
  test result: FAILED. 0 passed; 2 failed
  ```
  No commit yet (tests + impl commit together in Step 3).

- [ ] **Step 2: claude_code — minimal implementation**

  2a. Replace `chat()` (mod.rs:452-471) — the whole body, keeping the signature:

  ```rust
      /// Send a chat request by delegating to [`Self::stream`] and collecting
      /// the events with [`crate::stream::collect_stream`].
      ///
      /// Both paths share one spawn/parse pipeline, so `content`, `thinking`,
      /// `tool_calls`, `usage`, `session_id`, and `stop_reason` are identical
      /// by construction. A successfully completed CLI turn always reports
      /// [`StopReason::EndTurn`]: the CLI executes its tools internally, so
      /// [`ChatResponse::tool_calls`] is the record of tools the CLI already
      /// ran — never a request for the caller to execute them.
      ///
      /// Documented parity exception: `model` is backfilled from the request /
      /// provider configuration because stream events carry no model name.
      pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
          let configured_model = request.model.clone().or_else(|| self.model.clone());
          let stream = self.stream(request).await?;
          let mut resp = crate::stream::collect_stream(stream).await?;
          if resp.model.is_empty() {
              resp.model = configured_model.unwrap_or_default();
          }
          Ok(resp)
      }
  ```

  2b. In `drive_lines` (mod.rs:553-636): delete `let mut saw_tool_call = false;` (:565) and `saw_tool_call = true;` (:602, the `ToolCalls` arm keeps only its `for event in events { yield Ok(event); }` loop), and replace the terminal at :617 with:

  ```rust
                          yield Ok(crate::types::StreamEvent::done_with_stop_reason(StopReason::EndTurn));
  ```
  (`StopReason` is already imported at mod.rs:54 and now used only here.)

  Run:
  ```bash
  cd sdks/rust && cargo test --all-features claude_code
  ```
  Expected: `test result: ok.` including the 2 new tests; every pre-existing `claude_code` test still passes (none pinned the old chat body — verified: the tests mod asserts builders, drive_lines errors, and `#[ignore]`d live turns only).

- [ ] **Step 3: claude_code — delete the dead single-shot invoke path, gate, commit**

  3a. Prove deadness first (grep BEFORE deleting — show this in the PR):
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_agent_json' src/providers/claude_code/
  ```
  Expected after Step 2: hits ONLY inside `spawn.rs` (definition + doc comments) and spawn.rs's own tests — no caller in `mod.rs`.

  3b. In `sdks/rust/src/providers/claude_code/spawn.rs` delete:
  - `build_command` (:369-386) — blocking-path only; `stream()` builds its own `Command` inline (mod.rs:486-506)
  - `invoke_cli` (:388-471) — includes the `--output-format json` agent-mode plumbing
  - `parse_agent_json` (:473-538)
  - now-unused imports `use tokio::io::AsyncWriteExt;` (:19), `use tokio::process::Command;` (:20), `use crate::error::MotosanError;` (:22), `use crate::types::Usage;` (:23)
  - dead tests: `build_command_uses_binary_and_print_args` (:583), `build_command_sets_current_dir_when_cwd_present` (:595), `build_command_injects_envs` (:612), `agent_json_error_subtype_without_result_is_err` (:1058), `agent_json_is_error_with_result_surfaces_message` (:1069)
  - rewrite the module doc (:1-13) to: "Flag wiring for the Claude Code CLI provider. Argv layout is built by [`common_args`] and consumed by [`ClaudeCodeProvider::stream`](super::ClaudeCodeProvider::stream), which both `chat()` and `stream()` share since chat() delegates to stream collection."

  3c. Doc updates in `mod.rs`: replace the "# Streaming vs Blocking" section (:10-29) with:

  ```rust
  //! # One pipeline for chat and stream
  //!
  //! Both [`chat`](ClaudeCodeProvider) and [`stream`](ClaudeCodeProvider)
  //! spawn `claude --print --output-format stream-json --verbose -` and
  //! parse its NDJSON. `chat()` is `collect_stream(stream())` plus a model
  //! backfill from provider config, so tool_calls / thinking / usage /
  //! session_id parity holds by construction and a completed turn always
  //! reports `stop_reason = end_turn` (the CLI executes tools internally).
  ```
  and trim the `agent_mode` field doc (:62-64) to: "Whether to pass `--dangerously-skip-permissions`." (the "switch the blocking path to `--output-format json`" half is gone with `invoke_cli`).

  3d. Confirm nothing else broke, then gate and commit:
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_agent_json\|build_command' src/providers/claude_code/
  # expected: zero hits
  cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features
  git add -A sdks/rust && git commit -m "feat(rust)!: claude_code chat() delegates to stream collection, end_turn terminal

  F4: chat() = collect_stream(stream()) + model backfill; terminal is always
  done_with_stop_reason(EndTurn); delete dead spawn::invoke_cli/build_command/
  parse_agent_json and the --output-format json plumbing.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 4: codex_cli — write the failing parity + timeout tests**

  Append inside `#[cfg(test)] mod tests` in `sdks/rust/src/providers/codex_cli/mod.rs` (reuses the outer `user_request` helper, mod.rs:914):

  ```rust
      /// F4 parity: one fake-CLI transcript through `chat()` and through
      /// `collect_stream(stream())` must agree on content / thinking /
      /// tool_calls / stop_reason / usage / session_id. `model` is the one
      /// documented exception: `chat()` backfills it from provider config.
      #[cfg(unix)]
      mod chat_stream_parity {
          use super::*;
          use crate::types::StopReason;

          const TRANSCRIPT: &str = concat!(
              r#"{"type":"thread.started","thread_id":"th_777"}"#,
              "\n",
              r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"Let me check."}}"#,
              "\n",
              r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"ls -la","exit_code":0,"status":"completed"}}"#,
              "\n",
              r#"{"type":"item.completed","item":{"id":"item_2","type":"agent_message","text":"Answer: 4"}}"#,
              "\n",
              r#"{"type":"turn.completed","usage":{"input_tokens":11,"output_tokens":5,"cached_input_tokens":2}}"#,
          );

          /// Write an executable fake `codex` that ignores its argv, drains
          /// stdin, and plays back `body` on stdout.
          fn write_fake_cli(test_name: &str, body: &str) -> std::path::PathBuf {
              use std::io::Write;
              use std::os::unix::fs::PermissionsExt;
              let path = std::env::temp_dir().join(format!(
                  "motosan-fake-codex-{test_name}-{}",
                  std::process::id()
              ));
              let mut f = std::fs::File::create(&path).expect("create fake CLI");
              write!(f, "#!/bin/sh\ncat > /dev/null\ncat <<'NDJSON'\n{body}\nNDJSON\n")
                  .expect("write fake CLI");
              f.set_permissions(std::fs::Permissions::from_mode(0o755))
                  .expect("chmod fake CLI");
              path
          }

          #[tokio::test]
          async fn chat_equals_collected_stream_and_reports_end_turn() {
              let bin = write_fake_cli("parity", TRANSCRIPT);
              let provider = || CodexCliProvider::with_path(bin.clone()).model("test-model");

              let chat_resp = provider()
                  .chat(user_request("hi"))
                  .await
                  .expect("chat should succeed");
              let stream = provider()
                  .stream(user_request("hi"))
                  .await
                  .expect("stream should start");
              let collected = crate::stream::collect_stream(stream)
                  .await
                  .expect("collect should succeed");
              let _ = std::fs::remove_file(&bin);

              // F4: chat()'s tool_calls = the executed-tool record from the CLI.
              assert_eq!(
                  chat_resp.tool_calls.len(),
                  1,
                  "chat() must surface the CLI's executed-tool record"
              );
              assert_eq!(chat_resp.tool_calls[0].id, "item_1");
              assert_eq!(chat_resp.tool_calls[0].name, "command_execution");
              assert_eq!(
                  chat_resp.tool_calls[0].input,
                  serde_json::json!({"command": "ls -la"})
              );
              assert_eq!(chat_resp.tool_calls, collected.tool_calls);

              // F4: a completed CLI turn ALWAYS reports end_turn.
              assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
              assert_eq!(collected.stop_reason, StopReason::EndTurn);

              // F4 behavior change: agent messages concatenate into content in
              // arrival order (stream semantics); the old preamble→thinking
              // split of invoke_cli is gone.
              assert_eq!(chat_resp.content, "Let me check.Answer: 4");
              assert_eq!(chat_resp.content, collected.content);
              assert_eq!(chat_resp.thinking, None);
              assert_eq!(chat_resp.thinking, collected.thinking);
              assert_eq!(chat_resp.usage.input_tokens, 11);
              assert_eq!(chat_resp.usage.output_tokens, 5);
              assert_eq!(chat_resp.usage.cache_read_input_tokens, Some(2));
              assert_eq!(chat_resp.usage, collected.usage);
              assert_eq!(chat_resp.session_id.as_deref(), Some("th_777"));
              assert_eq!(chat_resp.session_id, collected.session_id);

              // Documented F4 parity exception: model backfill from config.
              assert_eq!(chat_resp.model, "test-model");
              assert_eq!(collected.model, "");
          }

          #[tokio::test]
          async fn chat_times_out_via_stream_read_timeout() {
              let bin = write_fake_cli("stall", "");
              std::fs::write(&bin, "#!/bin/sh\nsleep 30\n").expect("write stall script");
              let provider = CodexCliProvider::with_path(bin.clone())
                  .timeout(std::time::Duration::from_millis(50));
              let result = provider.chat(user_request("hi")).await;
              let _ = std::fs::remove_file(&bin);
              match result {
                  Err(crate::error::MotosanError::StreamReadTimeout(_)) => {}
                  other => panic!("expected StreamReadTimeout, got {other:?}"),
              }
          }
      }
  ```

  Run and confirm failure:
  ```bash
  cd sdks/rust && cargo test --all-features codex_cli::tests::chat_stream_parity
  ```
  Expected (2 failures):
  ```
  assertion `left == right` failed: chat() must surface the CLI's executed-tool record
    left: 0
   right: 1
  ...
  expected StreamReadTimeout, got Err(ProviderError { message: "codex CLI timed out after 0 seconds", ... })
  test result: FAILED. 0 passed; 2 failed
  ```

- [ ] **Step 5: codex_cli — minimal implementation**

  5a. Replace `chat()` (mod.rs:409-445) — new doc comment replaces the stale "tool_calls … always empty" text at :419-420:

  ```rust
      /// Send a chat request by delegating to [`Self::stream`] and collecting
      /// the events with [`crate::stream::collect_stream`].
      ///
      /// Both paths share one `codex exec --json` spawn/parse pipeline, so
      /// `content`, `thinking`, `tool_calls`, `usage`, `session_id`, and
      /// `stop_reason` are identical by construction. Codex may emit several
      /// `agent_message` items per turn; they concatenate into
      /// [`content`](ChatResponse::content) in arrival order. A successfully
      /// completed CLI turn always reports [`StopReason::EndTurn`]:
      /// [`ChatResponse::tool_calls`] records the tools the CLI already ran —
      /// never a request for the caller to execute them.
      ///
      /// Documented parity exception: `model` is backfilled from the request /
      /// provider configuration because stream events carry no model name.
      pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
          let configured_model = request.model.clone().or_else(|| self.model.clone());
          let stream = self.stream(request).await?;
          let mut resp = crate::stream::collect_stream(stream).await?;
          if resp.model.is_empty() {
              resp.model = configured_model.unwrap_or_default();
          }
          Ok(resp)
      }
  ```

  5b. In `drive_lines`: delete `let mut saw_tool_call = false;` (:547) and `saw_tool_call = true;` (:587), and replace the terminal at :598 with:

  ```rust
                          yield Ok(crate::types::StreamEvent::done_with_stop_reason(StopReason::EndTurn));
  ```
  (`StopReason` is imported at mod.rs:94.)

  5c. Deduplicate `stream()`'s spawn construction: codex's `spawn::build_command` (spawn.rs:246-260) builds the exact argv `stream()` builds inline at mod.rs:480-490 (`envs` → `exec [resume]` → `--json --skip-git-repo-check` → common args → `-` → kill_on_drop → 3 piped stdio). Change its visibility to `pub(super) fn build_command` (spawn.rs:246) and reword its doc's "for a blocking `codex exec` call" (spawn.rs:240-245) to "for a `codex exec --json` invocation; used by [`CodexCliProvider::stream`](super::CodexCliProvider::stream) (which `chat()` delegates to)". Then replace mod.rs:480-490 with:

  ```rust
          let mut cmd = spawn::build_command(&config);
  ```
  (the `use tokio::process::Command;` inside `stream()` at :473 becomes unused — delete it; keep `AsyncWriteExt`/`BufReader`).

  Run:
  ```bash
  cd sdks/rust && cargo test --all-features codex_cli
  ```
  Expected: `test result: ok.` including the 2 new tests. (`resume_inserts_exec_resume_subcommand` :680 and `no_resume_keeps_bare_exec` :695 now cover the streaming spawn too, since they test `build_command`.)

- [ ] **Step 6: codex_cli — delete the dead single-shot invoke path, gate, commit**

  6a. Prove deadness:
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_collected_stream' src/providers/codex_cli/
  ```
  Expected after Step 5: hits ONLY inside `spawn.rs` (definitions + doc mentions + their tests).

  6b. In `sdks/rust/src/providers/codex_cli/spawn.rs` delete:
  - `invoke_cli` (:261-337, including its doc block) and `parse_collected_stream` (:339-397)
  - now-unused imports `use tokio::io::AsyncWriteExt;` (:12), `use crate::error::MotosanError;` (:15), `use crate::types::Usage;` (:16), `use super::stream_json::{self, NdjsonAction};` (:18)
  - dead tests: `last_agent_message_is_content_rest_is_thinking` (:772), `single_agent_message_has_no_thinking` (:798), `parse_collected_stream_captures_thread_id` (:812), `parse_collected_stream_surfaces_error` (:827), `parse_collected_stream_ignores_blank_lines` (:837)
  - reword the module doc's "plus the blocking [`invoke_cli`] path used by [`CodexCliProvider::chat`]" (spawn.rs:4-6) to "consumed by [`CodexCliProvider::stream`](super::CodexCliProvider::stream), which both `chat()` and `stream()` share"

  6c. Update the stale module docs in mod.rs: rewrite "# Streaming vs Blocking" (:15-34) to state both paths spawn `codex exec --json --skip-git-repo-check` and `chat()` is `collect_stream(stream())` + model backfill (same wording pattern as Step 3c).

  6d. Gate and commit:
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_collected_stream' src/providers/codex_cli/
  # expected: zero hits
  cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features
  git add -A sdks/rust && git commit -m "feat(rust)!: codex_cli chat() delegates to stream collection, end_turn terminal

  F4: chat() = collect_stream(stream()) + model backfill; agent messages
  concatenate into content (preamble->thinking split removed); terminal is
  always done_with_stop_reason(EndTurn); stream() reuses spawn::build_command;
  delete dead spawn::invoke_cli/parse_collected_stream.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 7: gemini_cli — write the failing parity + timeout tests**

  Append inside `#[cfg(test)] mod tests` in `sdks/rust/src/providers/gemini_cli/mod.rs`. Gemini's tests mod has no request helper, so the module defines its own:

  ```rust
      /// F4 parity: one fake-CLI transcript through `chat()` and through
      /// `collect_stream(stream())` must agree on content / thinking /
      /// tool_calls / stop_reason / usage / session_id. `model` is the one
      /// documented exception: `chat()` backfills it from provider config.
      #[cfg(unix)]
      mod chat_stream_parity {
          use super::*;
          use crate::types::StopReason;

          const TRANSCRIPT: &str = concat!(
              r#"{"type":"init","session_id":"sess_9"}"#,
              "\n",
              r#"{"type":"message","role":"assistant","content":"Sure, ","delta":true}"#,
              "\n",
              r#"{"type":"tool_use","tool_id":"read_1","tool_name":"read_file","parameters":{"file_path":"Cargo.toml"}}"#,
              "\n",
              r#"{"type":"message","role":"assistant","content":"done.","delta":true}"#,
              "\n",
              r#"{"type":"result","status":"success","stats":{"input_tokens":9,"output_tokens":4,"cached":1}}"#,
          );

          fn parity_request(prompt: &str) -> ChatRequest {
              ChatRequest {
                  messages: vec![Message {
                      role: Role::User,
                      content: prompt.to_string(),
                      content_blocks: vec![],
                      tool_call_id: None,
                      tool_calls: vec![],
                      cache: false,
                  }],
                  model: None,
                  system: None,
                  system_blocks: None,
                  system_cache: false,
                  temperature: None,
                  max_tokens: None,
                  tools: None,
                  tool_choice: None,
                  provider_options: None,
                  mcp_servers: None,
                  mcp_tool_configs: None,
                  thinking: None,
                  stop_sequences: None,
              }
          }

          /// Write an executable fake `gemini` that ignores its argv, drains
          /// stdin, and plays back `body` on stdout.
          fn write_fake_cli(test_name: &str, body: &str) -> std::path::PathBuf {
              use std::io::Write;
              use std::os::unix::fs::PermissionsExt;
              let path = std::env::temp_dir().join(format!(
                  "motosan-fake-gemini-{test_name}-{}",
                  std::process::id()
              ));
              let mut f = std::fs::File::create(&path).expect("create fake CLI");
              write!(f, "#!/bin/sh\ncat > /dev/null\ncat <<'NDJSON'\n{body}\nNDJSON\n")
                  .expect("write fake CLI");
              f.set_permissions(std::fs::Permissions::from_mode(0o755))
                  .expect("chmod fake CLI");
              path
          }

          #[tokio::test]
          async fn chat_equals_collected_stream_and_reports_end_turn() {
              let bin = write_fake_cli("parity", TRANSCRIPT);
              let provider = || GeminiCliProvider::with_path(bin.clone()).model("gemini-test");

              let chat_resp = provider()
                  .chat(parity_request("hi"))
                  .await
                  .expect("chat should succeed");
              let stream = provider()
                  .stream(parity_request("hi"))
                  .await
                  .expect("stream should start");
              let collected = crate::stream::collect_stream(stream)
                  .await
                  .expect("collect should succeed");
              let _ = std::fs::remove_file(&bin);

              // F4: chat()'s tool_calls = the executed-tool record from the CLI.
              assert_eq!(
                  chat_resp.tool_calls.len(),
                  1,
                  "chat() must surface the CLI's executed-tool record"
              );
              assert_eq!(chat_resp.tool_calls[0].id, "read_1");
              assert_eq!(chat_resp.tool_calls[0].name, "read_file");
              assert_eq!(
                  chat_resp.tool_calls[0].input,
                  serde_json::json!({"file_path": "Cargo.toml"})
              );
              assert_eq!(chat_resp.tool_calls, collected.tool_calls);

              // F4: a completed CLI turn ALWAYS reports end_turn.
              assert_eq!(chat_resp.stop_reason, StopReason::EndTurn);
              assert_eq!(collected.stop_reason, StopReason::EndTurn);

              assert_eq!(chat_resp.content, "Sure, done.");
              assert_eq!(chat_resp.content, collected.content);
              assert_eq!(chat_resp.thinking, None);
              assert_eq!(chat_resp.thinking, collected.thinking);
              assert_eq!(chat_resp.usage.input_tokens, 9);
              assert_eq!(chat_resp.usage.output_tokens, 4);
              assert_eq!(chat_resp.usage.cache_read_input_tokens, Some(1));
              assert_eq!(chat_resp.usage, collected.usage);
              assert_eq!(chat_resp.session_id.as_deref(), Some("sess_9"));
              assert_eq!(chat_resp.session_id, collected.session_id);

              // Documented F4 parity exception: model backfill from config.
              assert_eq!(chat_resp.model, "gemini-test");
              assert_eq!(collected.model, "");
          }

          #[tokio::test]
          async fn chat_times_out_via_stream_read_timeout() {
              let bin = write_fake_cli("stall", "");
              std::fs::write(&bin, "#!/bin/sh\nsleep 30\n").expect("write stall script");
              let provider = GeminiCliProvider::with_path(bin.clone())
                  .timeout(std::time::Duration::from_millis(50));
              let result = provider.chat(parity_request("hi")).await;
              let _ = std::fs::remove_file(&bin);
              match result {
                  Err(crate::error::MotosanError::StreamReadTimeout(_)) => {}
                  other => panic!("expected StreamReadTimeout, got {other:?}"),
              }
          }
      }
  ```

  Run and confirm failure:
  ```bash
  cd sdks/rust && cargo test --all-features gemini_cli::tests::chat_stream_parity
  ```
  Expected (2 failures):
  ```
  assertion `left == right` failed: chat() must surface the CLI's executed-tool record
    left: 0
   right: 1
  ...
  expected StreamReadTimeout, got Err(ProviderError { message: "gemini CLI timed out after 0 seconds", ... })
  test result: FAILED. 0 passed; 2 failed
  ```

- [ ] **Step 8: gemini_cli — minimal implementation**

  8a. Replace `chat()` (mod.rs:243-261):

  ```rust
      /// Send a chat request by delegating to [`Self::stream`] and collecting
      /// the events with [`crate::stream::collect_stream`].
      ///
      /// Both paths share one `gemini -p "" -o stream-json` spawn/parse
      /// pipeline, so `content`, `thinking`, `tool_calls`, `usage`,
      /// `session_id`, and `stop_reason` are identical by construction. A
      /// successfully completed CLI turn always reports
      /// [`StopReason::EndTurn`]: [`ChatResponse::tool_calls`] records the
      /// tools the CLI already ran — never a request for the caller to
      /// execute them.
      ///
      /// Documented parity exception: `model` is backfilled from the request /
      /// provider configuration because stream events carry no model name.
      pub async fn chat(&self, request: ChatRequest) -> Result<ChatResponse, MotosanError> {
          let configured_model = request.model.clone().or_else(|| self.model.clone());
          let stream = self.stream(request).await?;
          let mut resp = crate::stream::collect_stream(stream).await?;
          if resp.model.is_empty() {
              resp.model = configured_model.unwrap_or_default();
          }
          Ok(resp)
      }
  ```

  8b. In `drive_lines`: delete `let mut saw_tool_call = false;` (:344) and `saw_tool_call = true;` (:383), and replace the terminal at :394 with:

  ```rust
                      yield Ok(crate::types::StreamEvent::done_with_stop_reason(StopReason::EndTurn));
  ```
  (`StopReason` is imported at mod.rs:43.)

  8c. Deduplicate `stream()`'s spawn construction: gemini's `spawn::build_command` (spawn.rs:185-199) is exactly the inline construction at mod.rs:277-286 (`current_dir` when `cwd` set → `envs` → `common_args` → kill_on_drop → 3 piped stdio). Change visibility to `pub(super) fn build_command` (spawn.rs:186), reword its doc "for a blocking `gemini` call" (spawn.rs:185) to "for a `gemini -p \"\" -o stream-json` invocation; used by [`GeminiCliProvider::stream`](super::GeminiCliProvider::stream) (which `chat()` delegates to)", and replace mod.rs:277-286 with:

  ```rust
          let mut cmd = spawn::build_command(&config);
  ```
  (delete the now-unused `use tokio::process::Command;` inside `stream()` at :269; keep `AsyncWriteExt`/`BufReader`).

  Run:
  ```bash
  cd sdks/rust && cargo test --all-features gemini_cli
  ```
  Expected: `test result: ok.` including the 2 new tests; `build_command_sets_current_dir_when_cwd_present` (:361) and `build_command_injects_envs` (:377) survive and now cover the streaming spawn.

- [ ] **Step 9: gemini_cli — delete the dead single-shot invoke path, gate, commit**

  9a. Prove deadness:
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_collected_stream' src/providers/gemini_cli/
  ```
  Expected after Step 8: hits ONLY inside `spawn.rs` (definitions + docs + their tests).

  9b. In `sdks/rust/src/providers/gemini_cli/spawn.rs` delete:
  - `invoke_cli` (:201-281, including its doc block) and `parse_collected_stream` (:283-331)
  - now-unused imports `use tokio::io::AsyncWriteExt;` (:19), `use crate::error::MotosanError;` (:22), `use crate::types::Usage;` (:23), `use super::stream_json::{self, NdjsonAction};` (:25)
  - dead tests: `parse_collected_stream_accumulates_deltas_and_usage` (:649), `parse_collected_stream_surfaces_non_success_result` (:673), `parse_collected_stream_ignores_blank_lines` (:683)

  9c. Gate and commit:
  ```bash
  cd sdks/rust && grep -rn 'invoke_cli\|parse_collected_stream' src/providers/gemini_cli/
  # expected: zero hits
  cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features
  git add -A sdks/rust && git commit -m "feat(rust)!: gemini_cli chat() delegates to stream collection, end_turn terminal

  F4: chat() = collect_stream(stream()) + model backfill; terminal is always
  done_with_stop_reason(EndTurn); stream() reuses spawn::build_command; delete
  dead spawn::invoke_cli/parse_collected_stream.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 10: Retire `cli_terminal_stop_reason`, gate, commit**

  10a. Prove all runtime callers are gone (show this grep in the PR):
  ```bash
  cd sdks/rust && grep -rn 'cli_terminal_stop_reason' src/
  ```
  Expected after Steps 3/6/9: only (a) the `fn` definition + its `#[cfg]` attribute, (b) the adjacent `mod cli_terminal_tests`, (c) the doc-comment mention in `src/providers/chatgpt_codex.rs:340`. Pre-Task-1 the definition sat at `src/providers/mod.rs:290-297` with the test mod at :299-325; post-Task-1 use the grep result as the authoritative location (expected under the `_cli` gate in the transport module).

  10b. Delete the `fn cli_terminal_stop_reason` together with its `#[cfg(...)]` attribute and the entire `mod cli_terminal_tests` (sole test: `tool_call_terminal_reason_collects_as_tool_use`). Its EndTurn-collection coverage is superseded by the six new parity/timeout tests.

  10c. Fix the stale doc reference in `src/providers/chatgpt_codex.rs:339-342`. Replace:
  ```rust
      /// Set once any `function_call` item is observed, so `response.completed`
      /// resolves to `ToolUse` (mirrors the gated `cli_terminal_stop_reason`
      /// helper, which `chatgpt-codex` is not in scope to reach from here).
  ```
  with:
  ```rust
      /// Set once any `function_call` item is observed, so `response.completed`
      /// resolves to `ToolUse`. Unlike the CLI backends (always `EndTurn` per
      /// F4 — their tools run inside the CLI), chatgpt-codex is an HTTP
      /// provider whose caller must execute the requested tools.
  ```
  (No behavior change in chatgpt_codex — `saw_tool_call` at :491 and its test asserting `Some(StopReason::ToolUse)` at :910 stay.)

  10d. Gate and commit:
  ```bash
  cd sdks/rust && grep -rn 'cli_terminal_stop_reason' src/
  # expected: zero hits
  cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features
  git add -A sdks/rust && git commit -m "refactor(rust)!: retire cli_terminal_stop_reason — CLI terminals are always end_turn

  F4: the saw_tool_call->ToolUse terminal heuristic is gone; every CLI
  provider yields done_with_stop_reason(EndTurn). Delete the helper and its
  truth-table test; fix the stale doc reference in chatgpt_codex.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 11: Full Rust gate, including per-feature builds**

  ```bash
  cd sdks/rust
  cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features
  cargo hack check --each-feature      # added to CI by Task 1
  cargo test --features claude-code claude_code
  cargo test --features codex-cli codex_cli
  cargo test --features gemini-cli gemini_cli
  ```
  Expected: all green. The three single-feature `cargo test` runs prove `chat()`'s call into `crate::stream::collect_stream` compiles without any HTTP feature enabled (i.e. Task 1 really did ungate `stream.rs`) — this is the compile-level guard for the F1/F4 interaction. No commit unless a fix was needed; if `--each-feature` fails on `collect_stream` gating, the defect is in Task 1's stream.rs change, not here — coordinate rather than re-gating.

**Flip list:** (executors may modify/delete ONLY these existing tests; every other existing test must pass unchanged)

- `tool_call_terminal_reason_collects_as_tool_use` — `sdks/rust/src/providers/mod.rs:311` inside `mod cli_terminal_tests` :299-325 (post-Task-1: locate by name) — DELETE with the helper. It pins `cli_terminal_stop_reason(true) == StopReason::ToolUse` and a `ToolUse`-terminal collection, both abolished by F4. Replacement assertions: the six new tests in Steps 1/4/7 (`chat_equals_collected_stream_and_reports_end_turn` × 3 asserting `stop_reason == StopReason::EndTurn` on BOTH paths with a tool_use in the transcript, and `chat_times_out_via_stream_read_timeout` × 3).
- `build_command_uses_binary_and_print_args` — `sdks/rust/src/providers/claude_code/spawn.rs:583` — DELETE (tests the deleted blocking-path `build_command`).
- `build_command_sets_current_dir_when_cwd_present` — `sdks/rust/src/providers/claude_code/spawn.rs:595` — DELETE (same reason; note the same-named gemini test at `gemini_cli/spawn.rs:361` SURVIVES because gemini's `build_command` is kept and reused by `stream()`).
- `build_command_injects_envs` — `sdks/rust/src/providers/claude_code/spawn.rs:612` — DELETE (claude only; the codex :437 and gemini :377 tests of the same name SURVIVE).
- `agent_json_error_subtype_without_result_is_err` — `sdks/rust/src/providers/claude_code/spawn.rs:1058` — DELETE with `parse_agent_json` (error-terminal detection now lives on the shared stream path, already pinned by `stream_error_subtype_terminal_yields_provider_error` in claude mod.rs:986 which SURVIVES).
- `agent_json_is_error_with_result_surfaces_message` — `sdks/rust/src/providers/claude_code/spawn.rs:1069` — DELETE (same reason).
- `last_agent_message_is_content_rest_is_thinking` — `sdks/rust/src/providers/codex_cli/spawn.rs:772` — DELETE; F4 abolishes the preamble→thinking split (new pinned behavior: `chat_resp.content == "Let me check.Answer: 4"`, `thinking == None` in the codex parity test).
- `single_agent_message_has_no_thinking` — `sdks/rust/src/providers/codex_cli/spawn.rs:798` — DELETE with `parse_collected_stream`.
- `parse_collected_stream_captures_thread_id` — `sdks/rust/src/providers/codex_cli/spawn.rs:812` — DELETE (session_id capture now pinned via the parity test's `session_id == Some("th_777")` on both paths).
- `parse_collected_stream_surfaces_error` — `sdks/rust/src/providers/codex_cli/spawn.rs:827` — DELETE (error terminals pinned by codex mod.rs `stream_surfaces_provider_error_as_err_item` :889, which SURVIVES).
- `parse_collected_stream_ignores_blank_lines` — `sdks/rust/src/providers/codex_cli/spawn.rs:837` — DELETE (blank-line skipping lives in `drive_lines` :574-576, shared by both paths).
- `parse_collected_stream_accumulates_deltas_and_usage` — `sdks/rust/src/providers/gemini_cli/spawn.rs:649` — DELETE (accumulation now pinned by the gemini parity test on both paths).
- `parse_collected_stream_surfaces_non_success_result` — `sdks/rust/src/providers/gemini_cli/spawn.rs:673` — DELETE (pinned by gemini mod.rs `stream_surfaces_provider_error_as_err_item` :681, which SURVIVES).
- `parse_collected_stream_ignores_blank_lines` — `sdks/rust/src/providers/gemini_cli/spawn.rs:683` — DELETE (same rationale as the codex twin).

Not on the flip list (verified no change needed): all `common_args_*` / `model_to_forward_*` spawn tests, all `drive_lines`-based stream tests (`stream_stall_yields_timeout_error`, `premature_child_exit_surfaces_status_and_stderr`, `stream_surfaces_provider_error_as_err_item`, `stream_error_subtype_terminal_yields_provider_error`, `terminal_error_reaps_child_before_yield`, `abnormal_exit_*`), all `stream_json.rs` parser tests (they assert the parse-level `done.done` flag, which is unchanged — the stop reason is attached in `drive_lines`, not the parser), `tests/cli_defaults.rs`, and every `#[ignore]`d live integration test (live CLI turns end with a success terminal → `EndTurn` on both paths, and none asserts `tool_calls` emptiness or `ToolUse`).

---

### Task 4: Rust TokenSource seam for chatgpt_codex + per-attempt async build + live smoke (F5, PR-R second half)

Branch context: continue on `feat/m4-rust-cli-token` in the PR-R worktree, directly after Task 3. All commands run from `sdks/rust/`. Baseline evidence verified at origin/main b9bcc3e (Rust 0.24.0).

**Files:**

- Create: `sdks/rust/src/auth.rs` (ungated public module: `TokenSource`, `StaticTokenSource`, re-exported `async_trait`)
- Create: `sdks/rust/tests/live_chatgpt_codex_token_refresh.rs` (`#[ignore]`d >1h live smoke)
- Modify: `sdks/rust/src/lib.rs` (module list :1-8, re-exports :25-46 — add `pub mod auth;` + re-export)
- Modify: `sdks/rust/src/providers/mod.rs:451-484` (`send_with_retry`; **post-Task-1 location: `sdks/rust/src/transport/http.rs`** — locate by fn name) — add `send_with_retry_async_build`, rewrite `send_with_retry` as thin wrapper. Helpers consumed: `observe_and_sleep` :416-436, `is_retryable_network_error` :349-351, `is_retryable_status` :336-338, `parse_retry_after` (same file). `retry_conformance` mod :838-957 stays byte-identical.
- Modify: `sdks/rust/src/providers/chatgpt_codex.rs` — struct + `new()` :26-59 (`#[derive(Debug, Clone)]` at :26 currently leaks the token via derived Debug), `apply_auth` :90-97 (Bearer format at :92), `stream()` :274-308 (`send_with_retry` call :278-286). `chat()` :263-272 delegates to `stream()`, so `stream()` is the ONLY send call site. In-crate test mod :584-974 (`test_provider()` helper :590-592 uses token `"test-token"`).
- Modify: `sdks/rust/src/client.rs` — `Client` fields :139-154 (`#[derive(Debug, Clone)]` at :96), `build_chatgpt_codex_provider` :655-677, `ClientBuilder` fields :710-717 (`#[derive(Debug, Default, Clone)]` at :680), setters :974-996, `build()` passthrough :1109-1116. NOTE (verified): `build()` has NO chatgpt-codex access-token validation to relax — `api_key` is already waived for `Provider::OpenAiChatGpt` at :1006-1013 and the access token defaults to `""` via `unwrap_or_default()` at :668. The "waiver" is pinned by a new test, not a code change.
- Modify: `sdks/rust/Cargo.toml` — `[dev-dependencies]` :106-110 (add `codex-oauth` path dev-dep; add `"sync"` to dev tokio :108). `async-trait = "0.1"` is already an unconditional dependency (:79). No `[[test]]` registration needed: `tests/chatgpt_codex_live.rs` has none either — the new live file self-gates with `#![cfg(feature = "chatgpt-codex")]`.
- Test: `sdks/rust/src/auth.rs` (unit tests), `sdks/rust/src/providers/mod.rs` (new `async_build_engine` test mod; post-Task-1: `transport/http.rs`), `sdks/rust/src/providers/chatgpt_codex.rs` (Debug-redaction test), `sdks/rust/tests/chatgpt_codex.rs` (per-attempt + builder tests; existing retry-mock precedent `stream_fires_on_retry_via_shared_engine` :229-286 registers a 503 `expect(1)` mock then a 200 `expect(1)` mock — mockito serves them in creation order with saturation; the new tests additionally disambiguate via `match_header`), `sdks/rust/tests/live_chatgpt_codex_token_refresh.rs`.

**Interfaces:**

- Consumes: `pub(crate) async fn send_with_retry(policy: &RetryPolicy, build: impl Fn() -> reqwest::RequestBuilder) -> Result<reqwest::Response, MotosanError>` (providers/mod.rs:451; post-Task-1: transport/http.rs); `async fn observe_and_sleep(policy: &RetryPolicy, attempt: u32, retry_after: Option<Duration>, cause: RetryCause)` (:416); `pub(crate) fn is_retryable_network_error(&reqwest::Error) -> bool` (:349); `pub(crate) fn is_retryable_status(u16) -> bool` (:336); `MotosanError` (src/error.rs:5); `ChatGptCodexProvider::new(access_token: impl Into<String>, account_id: impl Into<String>, model: impl Into<String>, base_url: Option<String>) -> Self` (chatgpt_codex.rs:43-48, signature UNCHANGED); `codex_oauth::{Token, refresh}` (sdks/rust/crates/codex-oauth/src/lib.rs:21,29 — re-exports `motosan_ai_oauth::Token` with `is_expired()` at crates/motosan-ai-oauth/src/lib.rs:60-62 and wraps `refresh()` at :104 with the codex provider config baked in).
- Produces: `motosan_ai::auth::TokenSource` — `#[async_trait] pub trait TokenSource: Send + Sync + std::fmt::Debug { async fn access_token(&self) -> Result<String, MotosanError>; }`; `motosan_ai::auth::StaticTokenSource` (tuple struct over `String`, `new(impl Into<String>)`, redacting Debug); `motosan_ai::auth::async_trait` (attribute re-export so implementors need no extra dep); `pub(crate) async fn send_with_retry_async_build<F, Fut>(policy: &RetryPolicy, build: F) -> Result<reqwest::Response, MotosanError> where F: Fn() -> Fut, Fut: std::future::Future<Output = Result<reqwest::RequestBuilder, MotosanError>>`; `ChatGptCodexProvider::with_token_source(self, Arc<dyn TokenSource>) -> Self`; `ClientBuilder::chatgpt_codex_token_source(self, Arc<dyn TokenSource>) -> Self`. The Task-covering-F7 release notes list these as the Rust F5 additions (non-breaking).

**Flip list:** none. No existing test asserts the provider's Debug output or reads the `access_token` field (verified by grep over `sdks/rust/src/` and `sdks/rust/tests/`); `retry_conformance` (providers/mod.rs:838-957) and every existing chatgpt_codex/openai retry test must pass with zero edits — that is the proof the engine refactor is intact.

- [ ] **Step 1: Failing unit tests for the auth module**

  Add to `sdks/rust/src/lib.rs` a new first line (module list currently :1-8):

  ```rust
  pub mod auth;
  ```

  and after the existing `pub use client::{Client, ClientBuilder};` (:25):

  ```rust
  pub use auth::{StaticTokenSource, TokenSource};
  ```

  Create `sdks/rust/src/auth.rs` containing ONLY the test mod for now:

  ```rust
  #[cfg(test)]
  mod tests {
      use super::{StaticTokenSource, TokenSource};

      #[tokio::test]
      async fn static_source_returns_the_string() {
          let source = StaticTokenSource::new("tok-abc");
          assert_eq!(source.access_token().await.unwrap(), "tok-abc");
      }

      #[test]
      fn static_source_debug_redacts_the_token() {
          let source = StaticTokenSource::new("super-secret");
          let debug = format!("{source:?}");
          assert!(!debug.contains("super-secret"), "Debug leaked: {debug}");
          assert!(debug.contains("StaticTokenSource"));
      }
  }
  ```

  Run: `cargo test --all-features --lib auth::`
  Expected failure: compile error `error[E0432]: unresolved imports 'super::StaticTokenSource', 'super::TokenSource'` (plus the matching E0432 for the lib.rs re-export).

- [ ] **Step 2: Implement TokenSource + StaticTokenSource**

  Prepend to `sdks/rust/src/auth.rs` (above the test mod), completing the file:

  ```rust
  //! Credential seams for providers that authenticate with short-lived tokens.
  //!
  //! [`TokenSource`] decouples *how* a bearer token is obtained (static string,
  //! OAuth refresh flow, keychain, ...) from the provider that spends it. The
  //! provider asks the source for a token at the top of **every** HTTP attempt
  //! (including retries), so a refreshing source can hand out a new token
  //! mid-retry-loop.
  //!
  //! # Security
  //!
  //! Tokens are credentials: implementations MUST NOT log, `Debug`-print, or
  //! otherwise persist the strings they return. `TokenSource` requires
  //! [`std::fmt::Debug`] so providers embedding a source stay debuggable, but
  //! your `Debug` impl must redact all token material (as
  //! [`StaticTokenSource`]'s does).

  use crate::error::MotosanError;

  pub use async_trait::async_trait;

  /// Async source of bearer access tokens.
  ///
  /// Implementations must be cheap to call repeatedly: providers call
  /// [`access_token`](TokenSource::access_token) once per HTTP attempt.
  #[async_trait]
  pub trait TokenSource: Send + Sync + std::fmt::Debug {
      /// Return the bearer token to use for the next HTTP attempt.
      ///
      /// Never log the returned value.
      async fn access_token(&self) -> Result<String, MotosanError>;
  }

  /// A [`TokenSource`] that always returns the same fixed token.
  ///
  /// This is what `ChatGptCodexProvider::new` wraps its `access_token`
  /// argument in. Its `Debug` impl redacts the token.
  pub struct StaticTokenSource(String);

  impl StaticTokenSource {
      pub fn new(token: impl Into<String>) -> Self {
          Self(token.into())
      }
  }

  impl std::fmt::Debug for StaticTokenSource {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.debug_tuple("StaticTokenSource")
              .field(&"<redacted>")
              .finish()
      }
  }

  #[async_trait]
  impl TokenSource for StaticTokenSource {
      async fn access_token(&self) -> Result<String, MotosanError> {
          Ok(self.0.clone())
      }
  }
  ```

  The module is UNGATED on purpose (F5): `async-trait` (Cargo.toml:79) and `MotosanError` are unconditional, so this compiles under `--no-default-features`. Dev-dep tokio (`macros`, `rt` — Cargo.toml:108) powers `#[tokio::test]` on every feature set.

  Run: `cargo test --all-features --lib auth::`
  Expected: `test result: ok. 2 passed; 0 failed`.
  Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features`
  Commit:

  ```
  feat(auth): add TokenSource seam with redacting StaticTokenSource

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

- [ ] **Step 3: Failing tests for the per-attempt async-build retry engine**

  In the SAME file as `send_with_retry` (today `sdks/rust/src/providers/mod.rs`, append after `retry_conformance` which ends at :957; **post-Task-1: append in `sdks/rust/src/transport/http.rs` and DROP the `#[cfg(any(...))]` 7-feature block below — the file is already gated once with `#[cfg(feature = "_http")]`**), add:

  ```rust
  // Per-attempt async request construction (F5). The build future runs at the
  // top of EVERY attempt so credential lookups (crate::auth::TokenSource) are
  // re-resolved per retry. send_with_retry delegates here — single retry
  // engine, M2 choke-point decision intact.
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
  mod async_build_engine {
      use super::send_with_retry_async_build;
      use crate::error::MotosanError;
      use crate::retry::RetryPolicy;
      use std::sync::atomic::{AtomicUsize, Ordering};

      fn fast_retry(max: u32) -> RetryPolicy {
          RetryPolicy::new()
              .max_retries(max)
              .base_delay_ms(0)
              .max_delay_ms(0)
              .jitter(false)
      }

      #[tokio::test]
      async fn build_future_runs_once_per_attempt() {
          let mut server = mockito::Server::new_async().await;
          // Creation order + expect(1) saturation: attempt 1 -> 503, attempt 2
          // -> 200 (same precedent as tests/chatgpt_codex.rs
          // stream_fires_on_retry_via_shared_engine).
          server
              .mock("GET", "/probe")
              .with_status(503)
              .expect(1)
              .create_async()
              .await;
          server
              .mock("GET", "/probe")
              .with_status(200)
              .expect(1)
              .create_async()
              .await;

          let http = reqwest::Client::new();
          let url = format!("{}/probe", server.url());
          let builds = AtomicUsize::new(0);

          let response = send_with_retry_async_build(&fast_retry(1), || {
              let http = &http;
              let url = &url;
              let builds = &builds;
              async move {
                  builds.fetch_add(1, Ordering::SeqCst);
                  Ok(http.get(url))
              }
          })
          .await
          .expect("second attempt succeeds");

          assert_eq!(response.status().as_u16(), 200);
          assert_eq!(builds.load(Ordering::SeqCst), 2, "one build per attempt");
      }

      #[tokio::test]
      async fn build_error_short_circuits_without_retry() {
          let builds = AtomicUsize::new(0);
          let err = send_with_retry_async_build(&fast_retry(3), || {
              let builds = &builds;
              async move {
                  builds.fetch_add(1, Ordering::SeqCst);
                  Err::<reqwest::RequestBuilder, _>(MotosanError::Auth {
                      message: "no token".into(),
                      status_code: None,
                      retry_after: None,
                      request_id: None,
                  })
              }
          })
          .await
          .expect_err("build error propagates");

          assert!(
              matches!(err, MotosanError::Auth { ref message, .. } if message == "no token")
          );
          assert_eq!(builds.load(Ordering::SeqCst), 1, "build errors are not retried");
      }
  }
  ```

  Run: `cargo test --all-features --lib async_build_engine`
  Expected failure: `error[E0432]: unresolved import 'super::send_with_retry_async_build'`.

- [ ] **Step 4: Implement send_with_retry_async_build; send_with_retry becomes a thin wrapper**

  Replace the whole of `send_with_retry` (providers/mod.rs:438-484 including its doc comment and cfg block; post-Task-1 locate by name in transport/http.rs and drop the cfg blocks) with the following two functions. The loop body is today's :455-483 verbatim except `build().send()` becomes `build().await?.send()` split into a binding — attempt counter, `is_retryable_network_error`, `observe_and_sleep`, and Ok-on-non-success (callers keep shaping errors) are all preserved:

  ```rust
  /// One retry engine for every HTTP provider (normative contract: specs/retry.md).
  ///
  /// `build` is awaited at the top of EVERY attempt — including retries — so
  /// per-attempt credential resolution (`crate::auth::TokenSource`) sees a
  /// fresh token each time. A `build` error aborts immediately: it is a
  /// caller-side failure, not a network failure, and is never retried.
  ///
  /// Returns success and terminal non-success responses with the body
  /// untouched, leaving caller-side parsing and provider-specific error
  /// shaping intact. `on_retry` observers fire only here (M2 choke point).
  #[cfg(any(
      feature = "anthropic",
      feature = "openai",
      feature = "minimax",
      feature = "ollama_native",
      feature = "gemini",
      feature = "gemini-code-assist",
      feature = "chatgpt-codex",
  ))]
  pub(crate) async fn send_with_retry_async_build<F, Fut>(
      policy: &RetryPolicy,
      build: F,
  ) -> Result<reqwest::Response, MotosanError>
  where
      F: Fn() -> Fut,
      Fut: std::future::Future<Output = Result<reqwest::RequestBuilder, MotosanError>>,
  {
      let mut attempt: u32 = 0;
      loop {
          let request = build().await?;
          let response = match request.send().await {
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

  /// Sync-build convenience over [`send_with_retry_async_build`] — the single
  /// retry engine every HTTP provider shares (normative contract:
  /// specs/retry.md).
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
      send_with_retry_async_build(policy, || std::future::ready(Ok::<_, MotosanError>(build())))
          .await
  }
  ```

  Run: `cargo test --all-features --lib async_build_engine`
  Expected: `test result: ok. 2 passed; 0 failed`.
  Proof the engine is intact — all UNCHANGED:
  `cargo test --all-features --lib retry_conformance` -> `9 passed; 0 failed`
  `cargo test --all-features --test chatgpt_codex` -> `6 passed; 0 failed` (includes `stream_fires_on_retry_via_shared_engine`)
  `cargo test --all-features --test openai_retry` -> all pass
  `git diff --stat` must show only the engine file changed, and `git diff` inside the `retry_conformance` mod must be empty.
  Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features`
  Commit:

  ```
  feat(providers): add per-attempt async build to the shared retry engine

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

- [ ] **Step 5: Failing tests — Debug redaction + per-attempt token resolution**

  (a) In-crate, in `sdks/rust/src/providers/chatgpt_codex.rs` `mod tests` (:584+, after `per_request_reasoning_effort_wins_over_provider_default`), add:

  ```rust
      #[test]
      fn provider_debug_never_leaks_token_material() {
          let p = test_provider(); // constructed with access token "test-token"
          let debug = format!("{p:?}");
          assert!(
              !debug.contains("test-token"),
              "Debug must redact the token: {debug}"
          );
          assert!(debug.contains("ChatGptCodexProvider"));
      }
  ```

  Run: `cargo test --all-features --lib provider_debug_never_leaks_token_material`
  Expected failure (the derived Debug at :26 leaks the field):
  `assertion failed: Debug must redact the token: ChatGptCodexProvider { http: Client { ... }, access_token: "test-token", ... }` — 1 failed.

  (b) In `sdks/rust/tests/chatgpt_codex.rs`, add file-level imports after the existing ones (:12-16):

  ```rust
  use motosan_ai::auth::{async_trait, TokenSource};
  use std::sync::Arc;
  ```

  (the fn-local `use std::sync::{Arc, Mutex};` at :232 shadows harmlessly) and append:

  ```rust
  // ---------------------------------------------------------------------------
  // F5: per-attempt TokenSource resolution
  // ---------------------------------------------------------------------------

  #[derive(Default)]
  struct SequenceTokenSource {
      calls: std::sync::atomic::AtomicUsize,
  }

  impl std::fmt::Debug for SequenceTokenSource {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.debug_struct("SequenceTokenSource").finish_non_exhaustive()
      }
  }

  #[async_trait]
  impl TokenSource for SequenceTokenSource {
      async fn access_token(&self) -> Result<String, MotosanError> {
          let n = self
              .calls
              .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
          Ok(format!("tok-{}", n + 1))
      }
  }

  #[tokio::test]
  async fn token_source_is_consulted_once_per_attempt() {
      let mut server = mockito::Server::new_async().await;
      // Attempt 1 must carry the token minted for it (tok-1) and gets a
      // retryable 503; the mocks are disambiguated by the auth header, so a
      // stale-token second attempt would match NEITHER mock and fail loudly.
      let first = server
          .mock("POST", Matcher::Any)
          .match_header("authorization", "Bearer tok-1")
          .with_status(503)
          .with_body(r#"{"error":{"message":"overloaded"}}"#)
          .expect(1)
          .create_async()
          .await;
      // Attempt 2 must re-resolve and carry tok-2.
      let second = server
          .mock("POST", Matcher::Any)
          .match_header("authorization", "Bearer tok-2")
          .with_status(200)
          .with_header("content-type", "text/event-stream")
          .with_body(FIXTURE)
          .expect(1)
          .create_async()
          .await;

      let source = Arc::new(SequenceTokenSource::default());
      let provider =
          ChatGptCodexProvider::new("ignored-static", "acct-123", "gpt-5.5", Some(server.url()))
              .with_retry_policy(
                  RetryPolicy::new()
                      .max_retries(1)
                      .base_delay_ms(0)
                      .max_delay_ms(0)
                      .jitter(false),
              )
              .with_token_source(source.clone());

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
      assert_eq!(
          source.calls.load(std::sync::atomic::Ordering::SeqCst),
          2,
          "token source must be consulted exactly once per attempt"
      );
      first.assert_async().await;
      second.assert_async().await;
  }
  ```

  Run: `cargo test --all-features --test chatgpt_codex`
  Expected failure: `error[E0599]: no method named 'with_token_source' found for struct 'ChatGptCodexProvider'`.

- [ ] **Step 6: Implement the provider field swap, with_token_source, per-token apply_auth, async build call site, manual Debug**

  In `sdks/rust/src/providers/chatgpt_codex.rs`:

  1. Imports (:1-19): in the `crate::providers` import list (:2-5) replace `send_with_retry` with `send_with_retry_async_build`; add two lines:

  ```rust
  use crate::auth::{StaticTokenSource, TokenSource};
  use std::sync::Arc;
  ```

  2. Replace the struct + derive (:26-40) and add the manual Debug right below:

  ```rust
  #[derive(Clone)]
  pub struct ChatGptCodexProvider {
      http: Client,
      /// Resolved at the top of every HTTP attempt (F5). `new()` seeds a
      /// [`StaticTokenSource`]; [`Self::with_token_source`] swaps in a dynamic
      /// (e.g. refreshing) source.
      token_source: Arc<dyn TokenSource>,
      account_id: String,
      model: String,
      base_url: String,
      retry_policy: RetryPolicy,
      total_timeout: Option<Duration>,
      /// Default reasoning effort emitted as `reasoning.effort` when a request
      /// does not carry a per-request `provider_options["reasoning_effort"]`.
      /// `None` leaves the `reasoning` object off the body entirely. The string
      /// is passed through verbatim — the backend validates the value.
      reasoning_effort: Option<String>,
  }

  /// Manual impl: the pre-0.25 derived form leaked the raw access token.
  /// Never print token material here.
  impl std::fmt::Debug for ChatGptCodexProvider {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          f.debug_struct("ChatGptCodexProvider")
              .field("token_source", &"<token source>")
              .field("account_id", &self.account_id)
              .field("model", &self.model)
              .field("base_url", &self.base_url)
              .field("retry_policy", &self.retry_policy)
              .field("total_timeout", &self.total_timeout)
              .field("reasoning_effort", &self.reasoning_effort)
              .finish_non_exhaustive()
      }
  }
  ```

  3. In `new()` (:49-58) replace the field init `access_token: access_token.into(),` with:

  ```rust
              token_source: Arc::new(StaticTokenSource::new(access_token)),
  ```

  (signature at :43-48 UNCHANGED.)

  4. After `with_reasoning_effort` (:85-88) add:

  ```rust
      /// Replace the token source. The provider resolves
      /// [`TokenSource::access_token`] at the top of **every** HTTP attempt
      /// (including retries), so a refreshing source can rotate tokens
      /// mid-retry-loop. Wins over the `access_token` given to
      /// [`new`](Self::new).
      pub fn with_token_source(mut self, token_source: Arc<dyn TokenSource>) -> Self {
          self.token_source = token_source;
          self
      }
  ```

  5. Replace `apply_auth` (:90-97):

  ```rust
      fn apply_auth(&self, request: reqwest::RequestBuilder, token: &str) -> reqwest::RequestBuilder {
          request
              .header("authorization", format!("Bearer {token}"))
              .header("chatgpt-account-id", &self.account_id)
              .header("originator", ORIGINATOR)
              .header("openai-beta", "responses=experimental")
              .header("accept", "text/event-stream")
      }
  ```

  6. In `stream()` replace the request block (:278-286) — `chat()` (:263-272) delegates here, so this is the only call site:

  ```rust
          let response = send_with_retry_async_build(&self.retry_policy, || {
              let url = &url;
              let body = &body;
              async move {
                  // Per-attempt token resolution (F5): a refreshing source can
                  // hand out a new token between retries.
                  let token = self.token_source.access_token().await?;
                  Ok(self
                      .apply_auth(
                          self.http
                              .post(url)
                              .header("content-type", "application/json"),
                          &token,
                      )
                      .json(body))
              }
          })
          .await?;
  ```

  Run: `cargo test --all-features --lib providers::chatgpt_codex` -> all pass (incl. `provider_debug_never_leaks_token_material`); `cargo test --all-features --test chatgpt_codex` -> `7 passed; 0 failed` (the 6 existing + `token_source_is_consulted_once_per_attempt`).
  Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features`
  Commit:

  ```
  feat(chatgpt-codex): resolve bearer token per attempt via TokenSource

  Replaces the private access_token: String with token_source:
  Arc<dyn TokenSource> (new() wraps StaticTokenSource — signature
  unchanged) and redacts the token from Debug output.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

- [ ] **Step 7: Failing ClientBuilder tests**

  Append to `sdks/rust/tests/chatgpt_codex.rs`:

  ```rust
  // ---------------------------------------------------------------------------
  // F5: ClientBuilder::chatgpt_codex_token_source
  // ---------------------------------------------------------------------------

  #[derive(Debug)]
  struct SentinelSource;

  #[async_trait]
  impl TokenSource for SentinelSource {
      async fn access_token(&self) -> Result<String, MotosanError> {
          Err(MotosanError::Auth {
              message: "sentinel token source consulted".to_string(),
              status_code: None,
              retry_after: None,
              request_id: None,
          })
      }
  }

  #[tokio::test]
  async fn builder_token_source_wins_over_static_access_token() {
      // The builder-made provider always targets the real chatgpt.com URL
      // (client.rs passes base_url: None), so observe the seam via a sentinel
      // source that errors BEFORE any network I/O: if the static token had
      // won, chat() would have attempted a real HTTP call instead.
      let client = motosan_ai::Client::builder()
          .provider(motosan_ai::Provider::OpenAiChatGpt)
          .chatgpt_codex("static-token-should-lose", "acct-123", "gpt-5.5")
          .chatgpt_codex_token_source(Arc::new(SentinelSource))
          .build()
          .expect("build succeeds");

      let err = client
          .chat(vec![Message::user("hi")])
          .await
          .expect_err("sentinel source fails the attempt before any I/O");
      assert!(
          matches!(err, MotosanError::Auth { ref message, .. }
              if message == "sentinel token source consulted"),
          "got {err:?}"
      );
  }

  #[tokio::test]
  async fn builder_token_source_alone_is_sufficient() {
      // Pins the access-token waiver: no chatgpt_codex(access_token, ...) call
      // at all. (Verified: build() never required it — api_key is waived for
      // Provider::OpenAiChatGpt at client.rs:1006-1013 and the static token
      // defaults to "" — so this is a pin, not a behavior change.)
      let client = motosan_ai::Client::builder()
          .provider(motosan_ai::Provider::OpenAiChatGpt)
          .model("gpt-5.5")
          .chatgpt_codex_token_source(Arc::new(SentinelSource))
          .build()
          .expect("token_source alone must build");

      let err = client
          .chat(vec![Message::user("hi")])
          .await
          .expect_err("sentinel source fails the attempt before any I/O");
      assert!(matches!(err, MotosanError::Auth { .. }), "got {err:?}");
  }
  ```

  Run: `cargo test --all-features --test chatgpt_codex`
  Expected failure: `error[E0599]: no method named 'chatgpt_codex_token_source' found for struct 'ClientBuilder'`.

- [ ] **Step 8: Implement builder + client wiring**

  In `sdks/rust/src/client.rs`:

  1. `Client` struct — after `chatgpt_codex_reasoning_effort` (:154) add:

  ```rust
      /// Dynamic token source overriding `chatgpt_codex_access_token` when
      /// set. Configured via [`ClientBuilder::chatgpt_codex_token_source`].
      #[cfg(feature = "chatgpt-codex")]
      chatgpt_codex_token_source: Option<std::sync::Arc<dyn crate::auth::TokenSource>>,
  ```

  (`Client` derives `Debug, Clone` at :96 — `dyn TokenSource` is `Debug` by supertrait and `Arc` is `Clone`, so the derives keep working; same for `ClientBuilder`'s `Debug, Default, Clone` at :680.)

  2. `ClientBuilder` struct — after `chatgpt_codex_reasoning_effort` (:717) add:

  ```rust
      #[cfg(feature = "chatgpt-codex")]
      chatgpt_codex_token_source: Option<std::sync::Arc<dyn crate::auth::TokenSource>>,
  ```

  3. After `chatgpt_codex_reasoning_effort` setter (:992-996) add:

  ```rust
      /// Set a dynamic [`TokenSource`](crate::auth::TokenSource) for the
      /// ChatGPT-backend Responses provider. The provider resolves
      /// `access_token()` at the top of every HTTP attempt (including
      /// retries), so a refreshing source can rotate tokens mid-retry-loop.
      ///
      /// Wins over the static `access_token` passed to
      /// [`chatgpt_codex`](Self::chatgpt_codex) when both are set. With only a
      /// token source, calling `chatgpt_codex(...)` is not required — but
      /// `account_id` then defaults to empty, so pass it via
      /// `chatgpt_codex("", account_id, model)` when the backend requires it.
      #[cfg(feature = "chatgpt-codex")]
      pub fn chatgpt_codex_token_source(
          mut self,
          token_source: std::sync::Arc<dyn crate::auth::TokenSource>,
      ) -> Self {
          self.chatgpt_codex_token_source = Some(token_source);
          self
      }
  ```

  4. `build()` — after the `chatgpt_codex_reasoning_effort` passthrough (:1115-1116) add:

  ```rust
              #[cfg(feature = "chatgpt-codex")]
              chatgpt_codex_token_source: self.chatgpt_codex_token_source,
  ```

  5. `build_chatgpt_codex_provider` (:655-677) — replace the constructor-chain tail so the source wins when set:

  ```rust
          let provider = crate::providers::chatgpt_codex::ChatGptCodexProvider::new(
              self.chatgpt_codex_access_token.clone().unwrap_or_default(),
              self.chatgpt_codex_account_id.clone().unwrap_or_default(),
              model,
              None,
          )
          .with_retry_policy(self.retry_policy.clone())
          .with_total_timeout(self.timeouts.total)
          .with_http_client(self.http.clone())
          .with_reasoning_effort(self.chatgpt_codex_reasoning_effort.clone());
          match &self.chatgpt_codex_token_source {
              // A dynamic source wins over the static access token.
              Some(source) => provider.with_token_source(std::sync::Arc::clone(source)),
              None => provider,
          }
      }
  ```

  No validation change: `build()` already waives `api_key` for `Provider::OpenAiChatGpt` (:1006-1013) and never required the static access token (:668 `unwrap_or_default`).

  Run: `cargo test --all-features --test chatgpt_codex` -> `9 passed; 0 failed`.
  Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features`
  Commit:

  ```
  feat(client): add ClientBuilder::chatgpt_codex_token_source

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

- [ ] **Step 9: #[ignore]d >1h live smoke with a refreshing TokenSource (dev-deps only)**

  1. `sdks/rust/Cargo.toml` — replace the dev tokio line (:108) and extend `[dev-dependencies]` (:106-110):

  ```toml
  tokio = { version = "1", features = ["macros", "rt", "sync"] }
  ```

  and add below `mockito = "1"`:

  ```toml
  # Live token-refresh smoke only (tests/live_chatgpt_codex_token_refresh.rs).
  # Dev-dep ONLY: the SDK itself stays decoupled from the oauth crates (F5).
  # codex-oauth re-exports motosan_ai_oauth::{Token, Error} and bakes in the
  # codex provider config, pulling motosan-ai-oauth in transitively.
  codex-oauth = { version = "0.1", path = "crates/codex-oauth" }
  ```

  (`"sync"` powers `tokio::sync::Mutex` below; `time` is already in the union via the `chatgpt-codex` feature's tokio dep :99-104.)

  2. Create `sdks/rust/tests/live_chatgpt_codex_token_refresh.rs` — credentials come from `~/.codex/auth.json` via `HOME`, mirroring `tests/chatgpt_codex_live.rs:16-25` (that smoke uses no other env vars, so neither does this one):

  ```rust
  //! Live >1h token-refresh smoke for the `chatgpt-codex` provider (F5).
  //!
  //! Proves a refreshing [`TokenSource`] carries a chat loop across the ~1h
  //! ChatGPT access-token expiry: one chat call every 10 minutes for 70
  //! minutes, each attempt resolving the bearer token through the source and
  //! refreshing via `codex-oauth` when the token is (about to be) expired.
  //!
  //! Requires a valid `~/.codex/auth.json` with `tokens.refresh_token` and
  //! `tokens.account_id` (mint via `codex login`). Discovery is through the
  //! `HOME` env var only, mirroring `tests/chatgpt_codex_live.rs`. Token
  //! material is never printed.
  //!
  //! Run manually (takes ~70 minutes):
  //! `cargo test --features chatgpt-codex --test live_chatgpt_codex_token_refresh -- --ignored --nocapture`

  #![cfg(feature = "chatgpt-codex")]

  use motosan_ai::auth::{async_trait, TokenSource};
  use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
  use motosan_ai::providers::ProviderImpl;
  use motosan_ai::{ChatRequest, Message, MotosanError, StopReason};
  use serde_json::Value;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::Arc;
  use std::time::Duration;
  use std::{env, fs};

  const MODEL: &str = "gpt-5.5";
  const CALL_INTERVAL: Duration = Duration::from_secs(600); // 10 minutes
  const CALLS: u32 = 8; // t = 0, 10, ..., 70 minutes
  const TEST_SPAN_SECS: u64 = 4200; // 70 minutes

  /// Refreshing TokenSource built locally on the workspace `codex-oauth`
  /// crate (`Token::is_expired()` + `refresh()`). Lives in the test, not the
  /// SDK, keeping the SDK decoupled from the oauth crates (F5).
  struct RefreshingSource {
      token: tokio::sync::Mutex<codex_oauth::Token>,
      refreshes: AtomicUsize,
  }

  impl std::fmt::Debug for RefreshingSource {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          // Never print token material.
          f.debug_struct("RefreshingSource").finish_non_exhaustive()
      }
  }

  #[async_trait]
  impl TokenSource for RefreshingSource {
      async fn access_token(&self) -> Result<String, MotosanError> {
          let mut token = self.token.lock().await;
          if token.is_expired() {
              eprintln!("[refresh] access token expired — refreshing via codex-oauth");
              let refreshed = codex_oauth::refresh(&token.refresh_token)
                  .await
                  .map_err(|e| MotosanError::Auth {
                      message: format!("codex token refresh failed: {e}"),
                      status_code: None,
                      retry_after: None,
                      request_id: None,
                  })?;
              *token = refreshed;
              self.refreshes.fetch_add(1, Ordering::SeqCst);
          }
          Ok(token.access_token.clone())
      }
  }

  /// Pull `tokens.refresh_token` + `tokens.account_id` from
  /// `~/.codex/auth.json` (same discovery as
  /// `tests/chatgpt_codex_live.rs::load_codex_auth`).
  fn load_codex_refresh_auth() -> Option<(String, String)> {
      let home = env::var("HOME").ok()?;
      let path = std::path::Path::new(&home).join(".codex/auth.json");
      let raw = fs::read_to_string(path).ok()?;
      let auth: Value = serde_json::from_str(&raw).ok()?;
      let tokens = auth.get("tokens")?;
      let refresh_token = tokens.get("refresh_token")?.as_str()?.to_string();
      let account_id = tokens.get("account_id")?.as_str()?.to_string();
      Some((refresh_token, account_id))
  }

  #[tokio::test]
  #[ignore = "live: ~70 minutes of real chatgpt.com calls; needs ~/.codex/auth.json"]
  async fn live_refreshing_token_source_survives_token_expiry() {
      let Some((refresh_token, account_id)) = load_codex_refresh_auth() else {
          eprintln!(
              "skipping live test: missing ~/.codex/auth.json tokens.refresh_token/account_id"
          );
          return;
      };

      // Mint a fresh, fully-populated Token up front so issued_at/expires_in
      // are authoritative (auth.json does not persist them in oauth Token
      // form).
      let initial = codex_oauth::refresh(&refresh_token)
          .await
          .expect("initial refresh must succeed");
      let initial_expires_in = initial.expires_in;
      eprintln!("[setup] initial token expires_in = {initial_expires_in}s");

      let source = Arc::new(RefreshingSource {
          token: tokio::sync::Mutex::new(initial),
          refreshes: AtomicUsize::new(0),
      });

      // new() takes a static token we immediately override — pass "" so no
      // real credential ever sits in the (redacted) StaticTokenSource.
      let provider =
          ChatGptCodexProvider::new("", account_id, MODEL, None).with_token_source(source.clone());

      for call in 1..=CALLS {
          let elapsed_min = (call - 1) * 10;
          eprintln!("[t+{elapsed_min:>2}m] chat call {call}/{CALLS} ...");
          let response = provider
              .chat(
                  ChatRequest::builder()
                      .message(Message::user("Reply with the single word: pong"))
                      .build(),
              )
              .await
              .unwrap_or_else(|e| panic!("call {call}/{CALLS} at t+{elapsed_min}m failed: {e}"));
          assert!(
              !response.content.is_empty(),
              "call {call}: empty response content"
          );
          assert_eq!(response.stop_reason, StopReason::EndTurn, "call {call}");
          eprintln!(
              "[t+{elapsed_min:>2}m] ok: {:?} (refreshes so far: {})",
              response.content.trim(),
              source.refreshes.load(Ordering::SeqCst)
          );
          if call < CALLS {
              tokio::time::sleep(CALL_INTERVAL).await;
          }
      }

      let refreshes = source.refreshes.load(Ordering::SeqCst);
      eprintln!("[done] {CALLS} calls over 70 minutes; {refreshes} refresh(es)");
      // If the initial token expires inside the 70-minute window (~1h
      // lifetime), at least one refresh must have happened.
      if initial_expires_in < TEST_SPAN_SECS {
          assert!(
              refreshes >= 1,
              "token lifetime {initial_expires_in}s < {TEST_SPAN_SECS}s test span, \
               yet no refresh happened"
          );
      }
  }
  ```

  Compile-verify without running (the live run stays manual):
  `cargo test --features chatgpt-codex --test live_chatgpt_codex_token_refresh --no-run`
  Expected: `Compiling ... Finished` with an executable listed, no errors. Then confirm it is skipped by default:
  `cargo test --features chatgpt-codex --test live_chatgpt_codex_token_refresh`
  Expected: `test live_refreshing_token_source_survives_token_expiry ... ignored` / `0 passed; 0 failed; 1 ignored`.
  Manual live run (documented, not part of the gate):
  `cargo test --features chatgpt-codex --test live_chatgpt_codex_token_refresh -- --ignored --nocapture`
  Gate: `cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features`
  Commit:

  ```
  test(chatgpt-codex): add >1h live token-refresh smoke over codex-oauth

  RefreshingSource lives in the test (dev-dep only) so the SDK stays
  decoupled from the oauth crates per F5.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  ```

- [ ] **Step 10: Final Rust gate for the task**

  From `sdks/rust/`:

  ```sh
  cargo fmt && \
  cargo clippy --all-features --all-targets -- -D warnings && \
  cargo test --all-features && \
  cargo check --no-default-features
  ```

  (Post-Task-1 the branch also carries `cargo hack check --each-feature` — run it too if Task 1 has landed on this branch.)
  Expected: fmt silent; clippy clean; test summary all green with `live_refreshing_token_source_survives_token_expiry ... ignored`; no-default-features check clean (proves `src/auth.rs` really is ungated). If `cargo fmt` touched files, amend them into the last commit; otherwise nothing to commit.

---

### Task 5: Python StreamEventType thinking members + emission migration (PR-P, BREAKING)

Applies locked decision **F3**. Python today emits ad-hoc `event_type="thinking"` strings from two providers and matches that string in the collector; Rust already has typed `ThinkingDelta`/`ThinkingDone` (sdks/rust/src/types.rs:690 / :702, docs :669-702), the Anthropic Rust adapter emits `ThinkingDone` with the full concatenated text on `content_block_stop` of a thinking block (sdks/rust/src/providers/anthropic.rs:1097-1113, accumulator opened at :1017-1023, deltas appended at :1057-1076), and `collect_stream` gives `thinking_done` priority over concatenated deltas (sdks/rust/src/stream.rs:48-53, :114-124, assembly :134-139). This task brings Python to parity. **BREAKING**: consumers matching `event_type == "thinking"` break.

**Branch context:** PR group **PR-P**, branch `feat/m4-python-vocab-cli-token`, PREREQ: PR-S merged into main before branching. This is the FIRST quarter of PR-P — later PR-P tasks build on the enum members and collector semantics produced here. In a fresh worktree run `uv sync --all-extras` from `sdks/python` before anything else.

**Verified baseline evidence (origin/main @ b9bcc3e):**
- `StreamEventType` StrEnum with exactly 5 members: sdks/python/motosan_ai/types.py:24-29
- `StreamEvent.event_type: str = "text"`: sdks/python/motosan_ai/types.py:357 (annotation stays `str` per F3)
- Anthropic `"thinking"` emission: sdks/python/motosan_ai/providers/anthropic.py:508-513 (`delta_type == "thinking_delta"` branch); stream state vars `current_tool_id`/`current_stop_reason` at :429-430; `content_block_start` handling (tool_use only) :489-499; `content_block_stop` (tool end only) :525-533
- ChatGPT-Codex `"thinking"` emission: sdks/python/motosan_ai/providers/chatgpt_codex.py:85 (inside `_parse_sse_event`, reasoning_text/reasoning_summary_text deltas :79-85)
- Collector match: sdks/python/motosan_ai/_stream_collect.py:39 (`event.event_type == "thinking"`), assembly `thinking or None` at :86
- These are the ONLY two `event_type="thinking"` emissions in the package (ollama.py:213-217 folds thinking into plain text events — out of F3 scope, do not touch)
- Anthropic SSE test fixture pattern: `_sse_lines(*events)` helper + respx `text/event-stream` mock, tests/test_anthropic_thinking.py:128-129, :132-177
- Codex adapter test fixture pattern: `_drive(frames)` over `_parse_sse_event`, tests/test_chatgpt_codex_stream.py:43-48
- ruff config: sdks/python/ruff.toml — line-length 100, py311

**Files:**
- Modify: `sdks/python/motosan_ai/types.py:24-29` (add 2 enum members)
- Modify: `sdks/python/motosan_ai/_stream_collect.py` (buffers :21-30, dispatch :37-40, docstring :13-20, assembly :77-88)
- Modify: `sdks/python/motosan_ai/providers/anthropic.py` (types import :21-34, stream state :429-430, content_block_start :489-499, thinking delta branch :508-513, content_block_stop :525-533)
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py` (types import :22-29, emission :85)
- Test: `sdks/python/tests/test_types.py` (new test after :81)
- Test: `sdks/python/tests/test_client_stream_collect.py` (3 new tests, 1 flipped, 1 new breaking-pin test; import :12)
- Test: `sdks/python/tests/test_anthropic_thinking.py` (flip test at :132-177; import :8)
- Test: `sdks/python/tests/test_chatgpt_codex_stream.py` (flip test at :114-122; import :15)

**Interfaces:**
- Consumes: `class StreamEventType(StrEnum)` (types.py:24); `StreamEvent` dataclass with `event_type: str = "text"`, `content: str`, `done: bool` (types.py:350-360); `async def collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse` (_stream_collect.py:12); `AnthropicProvider.stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]` (anthropic.py:410); `def _parse_sse_event(data: str, state: _ChatGptCodexAdapterState) -> list[StreamEvent]` (chatgpt_codex.py:53). No Task-1 dependency: all Rust reference files cited (types.rs, stream.rs, providers/anthropic.rs) are untouched by the Task 1 module moves.
- Produces: `StreamEventType.thinking_delta = "thinking_delta"` and `StreamEventType.thinking_done = "thinking_done"` (types.py — already exported via `motosan_ai.__init__` :50/:104, no export change needed); Anthropic stream contract: zero-or-more `thinking_delta` events then exactly one `thinking_done` event (full concatenated text, emitted even when empty) per thinking block, all before any final-answer `text` events; `collect_stream` semantics: `thinking_done` text wins over concatenated `thinking_delta` fallback, explicit empty `thinking_done` → `thinking is None`. Later PR-P tasks (F4 CLI chat-as-collect) rely on these collector semantics. **Task 10 changelog entry (state verbatim there):** "BREAKING (Python): `StreamEvent.event_type` value `\"thinking\"` is replaced by `StreamEventType.thinking_delta` / `StreamEventType.thinking_done`. `AnthropicProvider.stream()` now also emits a `thinking_done` event carrying the full concatenated thinking text when a thinking content block closes; `collect_stream()` prefers `thinking_done` text over concatenated deltas. Consumers matching `event_type == \"thinking\"` must migrate."

**Flip list:** (executors may modify ONLY these existing tests; all other test edits in this task are additive)
- `tests/test_anthropic_thinking.py::test_stream_emits_thinking_deltas_as_thinking_event` — renamed to `test_stream_emits_typed_thinking_events`; delta filter flips from `e.event_type == "thinking"` to `StreamEventType.thinking_delta` (contents still `["Let me ", "reason..."]`); NEW assertions: exactly one `thinking_done` event with content `"Let me reason..."`, emitted before the first `text` event. (Full new code in Step 4.)
- `tests/test_chatgpt_codex_stream.py::test_adapter_maps_reasoning_delta_to_thinking` — filter flips from `"thinking"` to `StreamEventType.thinking_delta` (joined content still `"think more"`); NEW assertion: no `thinking_done` events (codex emits deltas only, mirroring Rust). (Full new code in Step 5.)
- `tests/test_client_stream_collect.py::test_collect_thinking_content_concatenated` — events flip from `event_type="thinking"` to `event_type=StreamEventType.thinking_delta`; assertions unchanged (`resp.thinking == "reasoning step 1 step 2"`, `resp.content == "answer"`). (Full new code in Step 6.)

Not flips (verified they pass unchanged at end state): `test_oauth_chat_collects_thinking_from_stream` (test_anthropic_thinking.py:182 — asserts `resp.thinking`/`resp.content` only, flows through the updated collector), `test_chat_surfaces_thinking` (test_chatgpt_codex_http.py:101 — same), `test_stream_thinking` (test_ollama_native.py:180 — plain text events, untouched), `test_stream_event_type_values` (test_types.py:76-81 — existing assertions remain true; new members asserted in a NEW test).

**Step ordering note:** the collector gains `thinking_delta`/`thinking_done` handling (Step 3) BEFORE the providers emit them (Steps 4-5), and the legacy `"thinking"` branch is removed LAST (Step 6). This keeps `chat()`-via-collect tests (`test_oauth_chat_collects_thinking_from_stream`, `test_chat_surfaces_thinking`) green at every commit.

- [ ] **Step 1: Verify branch + sync environment**

  ```bash
  git rev-parse --abbrev-ref HEAD
  ```
  Expected output: `feat/m4-python-vocab-cli-token`. If PR-S is not yet in the branch history, STOP and resolve the prerequisite first.

  ```bash
  cd sdks/python && uv sync --all-extras
  ```
  Expected: ends with `Resolved N packages` / `Audited N packages` (or install lines), exit code 0. Baseline gate must be green before any change:
  ```bash
  uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: `All checks passed!`, `N files already formatted`, pytest summary line ending `passed` with 0 failures. No commit this step.

- [ ] **Step 2: Enum members `thinking_delta` / `thinking_done` (TDD)**

  Write the failing test. Append to `sdks/python/tests/test_types.py` (after `test_stream_event_type_values`, :81; `StreamEventType` is already imported at :8):

  ```python
  def test_stream_event_type_thinking_members():
      assert StreamEventType.thinking_delta == "thinking_delta"
      assert StreamEventType.thinking_done == "thinking_done"
      # Full M4/F2 vocabulary. Note: NO "done" member — done is a bool field
      # on StreamEvent, never an event_type.
      assert {m.value for m in StreamEventType} == {
          "text",
          "tool_call_start",
          "tool_call_args",
          "tool_call_end",
          "usage",
          "thinking_delta",
          "thinking_done",
      }
  ```

  Run (from `sdks/python`):
  ```bash
  uv run pytest tests/test_types.py -q
  ```
  Expected failure: `FAILED tests/test_types.py::test_stream_event_type_thinking_members - AttributeError: thinking_delta`

  Implement. In `sdks/python/motosan_ai/types.py` replace :24-29 with:

  ```python
  class StreamEventType(StrEnum):
      text = "text"
      tool_call_start = "tool_call_start"
      tool_call_args = "tool_call_args"
      tool_call_end = "tool_call_end"
      usage = "usage"
      # Partial extended-thinking delta; StreamEvent.content carries the
      # delta text. Emitted by anthropic and chatgpt_codex streams.
      thinking_delta = "thinking_delta"
      # End of a thinking block; StreamEvent.content carries the FULL
      # concatenated thinking text. Emitted by anthropic on
      # content_block_stop of a thinking block, always after that block's
      # thinking_delta events and before any final-answer text events.
      thinking_done = "thinking_done"
  ```

  Run again: `uv run pytest tests/test_types.py -q` — expected: all pass (`.` for the new test, summary `N passed`).

  Gate (from `sdks/python`):
  ```bash
  uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: exit 0, all pass.

  Commit:
  ```bash
  git add motosan_ai/types.py tests/test_types.py
  git commit -m "$(cat <<'EOF'
  feat(python): add thinking_delta and thinking_done to StreamEventType

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```

- [ ] **Step 3: Collector — thinking_done priority over thinking_delta fallback (TDD, additive)**

  Write the failing tests. In `sdks/python/tests/test_client_stream_collect.py`, first extend the types import at :12 to:

  ```python
  from motosan_ai.types import (
      ChatRequest,
      Message,
      StopReason,
      StreamEvent,
      StreamEventType,
      Usage,
  )
  ```

  Then append after `test_collect_thinking_content_concatenated` (:150-159):

  ```python
  @pytest.mark.asyncio
  async def test_collect_thinking_done_takes_priority():
      # Mirrors Rust stream.rs: ThinkingDone carries the authoritative full
      # text and wins over the delta accumulator at assembly.
      events = [
          StreamEvent(content="a", done=False, event_type=StreamEventType.thinking_delta),
          StreamEvent(content="b", done=False, event_type=StreamEventType.thinking_delta),
          StreamEvent(
              content="ab-final",
              done=False,
              event_type=StreamEventType.thinking_done,
          ),
          StreamEvent(content="answer", done=False),
          StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
      ]
      resp = await collect_stream(_events_to_iter(events))
      assert resp.thinking == "ab-final"
      assert resp.content == "answer"


  @pytest.mark.asyncio
  async def test_collect_thinking_deltas_only_concatenated():
      # No thinking_done (e.g. chatgpt_codex): concatenated deltas are the
      # fallback.
      events = [
          StreamEvent(content="a", done=False, event_type=StreamEventType.thinking_delta),
          StreamEvent(content="b", done=False, event_type=StreamEventType.thinking_delta),
          StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
      ]
      resp = await collect_stream(_events_to_iter(events))
      assert resp.thinking == "ab"


  @pytest.mark.asyncio
  async def test_collect_empty_thinking_done_yields_none():
      # Explicit empty thinking block -> treat as none (Rust parity,
      # stream.rs assembly match arm Some(_) => None).
      events = [
          StreamEvent(content="", done=False, event_type=StreamEventType.thinking_done),
          StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
      ]
      resp = await collect_stream(_events_to_iter(events))
      assert resp.thinking is None
  ```

  Run:
  ```bash
  uv run pytest tests/test_client_stream_collect.py -q
  ```
  Expected failures (3): `test_collect_thinking_done_takes_priority - AssertionError: assert None == 'ab-final'`, `test_collect_thinking_deltas_only_concatenated - AssertionError: assert None == 'ab'`, `test_collect_empty_thinking_done_yields_none` passes only by accident? No — it passes (thinking stays `""` → `None`), so expected: **2 failed** as above, `test_collect_empty_thinking_done_yields_none` passes; keep it anyway as a pin.

  Implement. In `sdks/python/motosan_ai/_stream_collect.py`:

  Replace :21-30 (local state) with:
  ```python
      content = ""
      tool_calls: list[ToolCall] = []
      usage = Usage(0, 0)
      stop_reason: StopReason | None = None

      current_tc_id = ""
      current_tc_name = ""
      current_tc_args = ""
      session_id: str | None = None

      # Thinking accumulation (mirrors Rust stream.rs). thinking_delta_buf
      # collects every thinking_delta as a fallback in case the provider
      # does not emit thinking_done. thinking_done_buf holds the explicit
      # final text from the most recent thinking_done and takes priority
      # on assembly.
      thinking_delta_buf = ""
      thinking_done_buf: str | None = None
  ```

  Replace the dispatch lines :37-40 (`text` + legacy `thinking` branches) with:
  ```python
          if event.event_type == "text" and event.content:
              content += event.content
          elif event.event_type == "thinking" and event.content:
              # Legacy ad-hoc event; kept for one commit so provider
              # migration lands green. Removed in the last step of this
              # task (M4/F3 BREAKING).
              thinking_delta_buf += event.content
          elif event.event_type == "thinking_delta" and event.content:
              thinking_delta_buf += event.content
          elif event.event_type == "thinking_done":
              # thinking_done carries the full text; it wins over the delta
              # accumulator. Clear deltas so a second block starts fresh.
              thinking_done_buf = event.content
              thinking_delta_buf = ""
  ```

  Replace the assembly (:77-88, from `if stop_reason is None:` to the end of the function) with:
  ```python
      if stop_reason is None:
          stop_reason = StopReason.tool_use if tool_calls else StopReason.end_turn

      if thinking_done_buf is not None:
          # Explicit empty thinking block -> treat as none (Rust parity).
          thinking = thinking_done_buf or None
      else:
          thinking = thinking_delta_buf or None

      return ChatResponse(
          content=content,
          tool_calls=tool_calls,
          model="",
          usage=usage,
          stop_reason=stop_reason,
          thinking=thinking,
          session_id=session_id,
      )
  ```

  Run: `uv run pytest tests/test_client_stream_collect.py -q` — expected: all pass.

  Gate (same command as Step 2). Expected: exit 0.

  Commit:
  ```bash
  git add motosan_ai/_stream_collect.py tests/test_client_stream_collect.py
  git commit -m "$(cat <<'EOF'
  feat(python): thinking_done priority collection in collect_stream

  thinking_done carries the authoritative full text and wins over the
  concatenated thinking_delta fallback, mirroring Rust stream.rs. Legacy
  "thinking" events are still accepted for one commit while provider
  emissions migrate.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```

- [ ] **Step 4: Anthropic stream — typed thinking_delta + thinking_done on content_block_stop (TDD, flip 1/3)**

  Write the failing test. In `sdks/python/tests/test_anthropic_thinking.py`: extend the import at :8 to
  ```python
  from motosan_ai.types import ChatRequest, Message, StreamEventType, ThinkingConfig
  ```
  and replace the ENTIRE test `test_stream_emits_thinking_deltas_as_thinking_event` (:132-177) with:

  ```python
  @respx.mock
  @pytest.mark.asyncio
  async def test_stream_emits_typed_thinking_events(provider):
      """M4/F3: thinking blocks stream as thinking_delta events, then exactly one
      thinking_done event carrying the full concatenated text fires on
      content_block_stop — before any final-answer text events (mirrors the Rust
      Anthropic adapter).
      """
      sse = _sse_lines(
          {
              "type": "content_block_start",
              "index": 0,
              "content_block": {"type": "thinking", "thinking": ""},
          },
          {
              "type": "content_block_delta",
              "index": 0,
              "delta": {"type": "thinking_delta", "thinking": "Let me "},
          },
          {
              "type": "content_block_delta",
              "index": 0,
              "delta": {"type": "thinking_delta", "thinking": "reason..."},
          },
          {"type": "content_block_stop", "index": 0},
          {
              "type": "content_block_start",
              "index": 1,
              "content_block": {"type": "text", "text": ""},
          },
          {
              "type": "content_block_delta",
              "index": 1,
              "delta": {"type": "text_delta", "text": "42"},
          },
          {"type": "message_stop"},
      )
      respx.post("https://mock.anthropic.com/v1/messages").mock(
          return_value=httpx.Response(200, text=sse, headers={"content-type": "text/event-stream"})
      )
      req = ChatRequest(messages=[Message.user("q")], thinking=ThinkingConfig(budget_tokens=1024))
      events = [e async for e in provider.stream(req)]

      deltas = [e for e in events if e.event_type == StreamEventType.thinking_delta]
      assert [e.content for e in deltas] == ["Let me ", "reason..."]

      dones = [e for e in events if e.event_type == StreamEventType.thinking_done]
      assert [e.content for e in dones] == ["Let me reason..."]
      assert all(not e.done for e in deltas + dones)

      idx_done = next(
          i for i, e in enumerate(events) if e.event_type == StreamEventType.thinking_done
      )
      idx_first_text = next(
          i for i, e in enumerate(events) if e.event_type == StreamEventType.text and e.content
      )
      assert idx_done < idx_first_text
      assert [
          e.content for e in events if e.event_type == StreamEventType.text and e.content
      ] == ["42"]
  ```

  Run:
  ```bash
  uv run pytest tests/test_anthropic_thinking.py -q
  ```
  Expected failure: `FAILED tests/test_anthropic_thinking.py::test_stream_emits_typed_thinking_events - AssertionError: assert [] == ['Let me ', 'reason...']` (events still carry the old `"thinking"` string, so the `thinking_delta` filter is empty).

  Implement in `sdks/python/motosan_ai/providers/anthropic.py` — four edits:

  (a) Extend the types import (:21-34) — add `StreamEventType` after `StreamEvent`:
  ```python
  from motosan_ai.types import (
      ChatRequest,
      ChatResponse,
      Message,
      Role,
      StopReason,
      StreamEvent,
      StreamEventType,
      ToolCall,
      Usage,
      content_block_to_dict,
      mcp_server_config_to_dict,
      mcp_tool_config_to_dict,
      system_block_to_dict,
  )
  ```

  (b) Stream state (:429-430) becomes:
  ```python
          current_tool_id: str | None = None
          current_stop_reason: StopReason | None = None
          # Accumulates the text of an open `thinking` content block; None
          # when no thinking block is open (redacted_thinking never opens
          # it). content_block_stop drains it into a thinking_done event
          # (mirrors the Rust adapter, anthropic.rs).
          current_thinking: str | None = None
  ```

  (c) `content_block_start` branch (:489-499, now shifted by the lines added in (b)) becomes:
  ```python
                  if event_type == "content_block_start":
                      block = payload.get("content_block") or {}
                      if block.get("type") == "tool_use":
                          current_tool_id = block.get("id", "")
                          yield StreamEvent(
                              content="",
                              done=False,
                              tool_call_id=current_tool_id,
                              tool_call_name=block.get("name", ""),
                              event_type="tool_call_start",
                          )
                      elif block.get("type") == "thinking":
                          # Open the thinking accumulator. No event is
                          # emitted at start; redacted_thinking blocks
                          # intentionally leave current_thinking as None so
                          # their block_stop is a no-op.
                          current_thinking = ""
  ```

  (d) The `thinking_delta` branch (:508-513 pre-edit) becomes:
  ```python
                      elif delta_type == "thinking_delta":
                          thinking_text = delta.get("thinking", "")
                          if thinking_text:
                              if current_thinking is not None:
                                  current_thinking += thinking_text
                              yield StreamEvent(
                                  content=thinking_text,
                                  done=False,
                                  event_type=StreamEventType.thinking_delta,
                              )
  ```

  (e) The `content_block_stop` branch (:525-533 pre-edit) becomes:
  ```python
                  elif event_type == "content_block_stop":
                      if current_tool_id is not None:
                          yield StreamEvent(
                              content="",
                              done=False,
                              tool_call_id=current_tool_id,
                              event_type="tool_call_end",
                          )
                          current_tool_id = None
                      elif current_thinking is not None:
                          # Emit even when empty: presence of thinking_done
                          # tells consumers a thinking block existed (Rust
                          # parity, anthropic.rs content_block_stop).
                          yield StreamEvent(
                              content=current_thinking,
                              done=False,
                              event_type=StreamEventType.thinking_done,
                          )
                          current_thinking = None
  ```

  Run: `uv run pytest tests/test_anthropic_thinking.py -q` — expected: all pass, including `test_oauth_chat_collects_thinking_from_stream` (its stream now emits `thinking_delta` + `thinking_done "trace"`; the Step-3 collector prefers the done text → `resp.thinking == "trace"` still holds).

  Gate (same command as Step 2). Expected: exit 0.

  Commit:
  ```bash
  git add motosan_ai/providers/anthropic.py tests/test_anthropic_thinking.py
  git commit -m "$(cat <<'EOF'
  feat(python)!: anthropic stream emits typed thinking events

  BREAKING: event_type "thinking" -> StreamEventType.thinking_delta, plus
  a new thinking_done event carrying the full concatenated thinking text
  on content_block_stop of a thinking block (Rust adapter parity).

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```

- [ ] **Step 5: ChatGPT-Codex adapter — thinking_delta (TDD, flip 2/3)**

  Write the failing test. In `sdks/python/tests/test_chatgpt_codex_stream.py`: extend the import at :15 to
  ```python
  from motosan_ai.types import ChatRequest, Message, StopReason, StreamEventType
  ```
  and replace `test_adapter_maps_reasoning_delta_to_thinking` (:114-122) with:

  ```python
  def test_adapter_maps_reasoning_delta_to_thinking():
      events = _drive(
          [
              {"type": "response.reasoning_text.delta", "delta": "think "},
              {"type": "response.reasoning_summary_text.delta", "delta": "more"},
          ]
      )
      deltas = [e for e in events if e.event_type == StreamEventType.thinking_delta]
      assert "".join(e.content for e in deltas) == "think more"
      # Rust parity: the codex adapter emits ThinkingDelta only, never
      # ThinkingDone — the Responses wire has no thinking block boundary.
      assert not [e for e in events if e.event_type == StreamEventType.thinking_done]
  ```

  Run:
  ```bash
  uv run pytest tests/test_chatgpt_codex_stream.py -q
  ```
  Expected failure: `FAILED tests/test_chatgpt_codex_stream.py::test_adapter_maps_reasoning_delta_to_thinking - AssertionError: assert '' == 'think more'`

  Implement in `sdks/python/motosan_ai/providers/chatgpt_codex.py` — two edits:

  (a) Extend the types import (:22-29):
  ```python
  from motosan_ai.types import (
      ChatRequest,
      ChatResponse,
      Role,
      StopReason,
      StreamEvent,
      StreamEventType,
      Usage,
  )
  ```

  (b) Line :85 (`out.append(StreamEvent(content=delta, done=False, event_type="thinking"))`) becomes:
  ```python
          delta = chunk.get("delta")
          if isinstance(delta, str) and delta:
              out.append(
                  StreamEvent(
                      content=delta,
                      done=False,
                      event_type=StreamEventType.thinking_delta,
                  )
              )
  ```
  (This is the full body of the `elif event_type in ("response.reasoning_text.delta", "response.reasoning_summary_text.delta"):` branch at :79-85.)

  Run: `uv run pytest tests/test_chatgpt_codex_stream.py tests/test_chatgpt_codex_http.py -q` — expected: all pass (`test_chat_surfaces_thinking` stays green: chat() collects the stream and the Step-3 collector concatenates the `thinking_delta` fallback → `resp.thinking == "plan ahead"`).

  Gate (same command as Step 2). Expected: exit 0.

  Commit:
  ```bash
  git add motosan_ai/providers/chatgpt_codex.py tests/test_chatgpt_codex_stream.py
  git commit -m "$(cat <<'EOF'
  feat(python)!: chatgpt-codex stream emits thinking_delta

  BREAKING: event_type "thinking" -> StreamEventType.thinking_delta for
  response.reasoning_text.delta / response.reasoning_summary_text.delta.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```

- [ ] **Step 6: Remove legacy "thinking" from collector + flip its pinned test (TDD, flip 3/3)**

  Write the failing test. Append to `sdks/python/tests/test_client_stream_collect.py`:

  ```python
  @pytest.mark.asyncio
  async def test_collect_ignores_legacy_thinking_string():
      # BREAKING (M4/F3): the ad-hoc "thinking" event_type is gone from the
      # stream vocabulary; collect_stream must not accumulate it.
      events = [
          StreamEvent(content="legacy", done=False, event_type="thinking"),
          StreamEvent(content="", done=True, stop_reason=StopReason.end_turn),
      ]
      resp = await collect_stream(_events_to_iter(events))
      assert resp.thinking is None
  ```

  Run:
  ```bash
  uv run pytest tests/test_client_stream_collect.py -q
  ```
  Expected failure: `FAILED tests/test_client_stream_collect.py::test_collect_ignores_legacy_thinking_string - AssertionError: assert 'legacy' is None`

  Implement — two edits:

  (a) In `sdks/python/motosan_ai/_stream_collect.py` delete the legacy branch added in Step 3, leaving the dispatch as:
  ```python
          if event.event_type == "text" and event.content:
              content += event.content
          elif event.event_type == "thinking_delta" and event.content:
              thinking_delta_buf += event.content
          elif event.event_type == "thinking_done":
              # thinking_done carries the full text; it wins over the delta
              # accumulator. Clear deltas so a second block starts fresh.
              thinking_done_buf = event.content
              thinking_delta_buf = ""
  ```
  and update the function docstring (:13-20) to:
  ```python
      """Collect a stream into one ChatResponse.

      Handles text, thinking_delta/thinking_done, tool-call start/args/end,
      usage, and terminal stop_reason events. thinking_done text takes
      priority over concatenated thinking_delta fallback. Malformed streamed
      tool arguments are treated as an empty object to match provider
      collector behavior. A mid-stream provider error (StreamError /
      NetworkError / ProviderError) raised by ``events`` propagates out of
      this function uncollected.
      """
  ```

  (b) In `sdks/python/tests/test_client_stream_collect.py` replace `test_collect_thinking_content_concatenated` (:150-159 pre-edit) with:
  ```python
  @pytest.mark.asyncio
  async def test_collect_thinking_content_concatenated():
      events = [
          StreamEvent(
              content="reasoning step 1",
              done=False,
              event_type=StreamEventType.thinking_delta,
          ),
          StreamEvent(content=" step 2", done=False, event_type=StreamEventType.thinking_delta),
          StreamEvent(content="answer", done=False),
          StreamEvent(content="", done=True),
      ]
      resp = await collect_stream(_events_to_iter(events))
      assert resp.thinking == "reasoning step 1 step 2"
      assert resp.content == "answer"
  ```

  Run: `uv run pytest tests/test_client_stream_collect.py -q` — expected: all pass.

  Sanity sweep — confirm no `"thinking"` event_type remains anywhere in the package or tests:
  ```bash
  grep -rn 'event_type="thinking"' motosan_ai/ tests/ ; grep -rn 'event_type == "thinking"' motosan_ai/ tests/
  ```
  Expected: no matches from the first grep; the second may only match the string inside `test_collect_ignores_legacy_thinking_string`'s event constructor — verify by eye that the only remaining `"thinking"` literal in tests is that breaking-pin test (and SSE wire fixtures like `{"type": "thinking", ...}`, which are Anthropic wire format, not our event vocabulary).

  Full gate (from `sdks/python`):
  ```bash
  uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: `All checks passed!`, `N files already formatted`, pytest all pass, exit 0.

  Commit:
  ```bash
  git add motosan_ai/_stream_collect.py tests/test_client_stream_collect.py
  git commit -m "$(cat <<'EOF'
  feat(python)!: drop legacy "thinking" stream event from collector

  BREAKING: collect_stream no longer accumulates event_type "thinking";
  the vocabulary is thinking_delta/thinking_done (M4/F2, F3).

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```

**Done criteria:** `StreamEventType` has exactly 7 members (no `"done"`); anthropic emits `thinking_delta` per SSE delta and one `thinking_done` (full text, even when empty) per thinking block before final-answer text; chatgpt_codex emits `thinking_delta` only; `collect_stream` gives `thinking_done` priority (empty done → `None`) with delta concatenation as fallback and ignores legacy `"thinking"`; full Python gate green; 5 commits on `feat/m4-python-vocab-cli-token`. The BREAKING changelog text (see Interfaces/Produces) is recorded by Task 10 — do not touch CHANGELOG.md in this task.

---

### Task 6: Python CLI chat/stream contract — F4 delegation for claude_code / codex_cli / gemini_cli (BREAKING)

**Branch:** PR-P — continue on the branch Task 5 created, in the same worktree (`/Users/daiwanwei/Projects/wade/motosan-worktrees/m4-plan` or wherever PR-P lives). Do NOT start a new worktree. Task 5 (F3) must be committed first: it edits `motosan_ai/_stream_collect.py`, `types.py`, `anthropic.py`, `chatgpt_codex.py` — none of the three CLI provider files or their test files, so every line reference below remains valid after Task 5. If a cited line has drifted, locate the item by the quoted code, not the number.

All commands run from `sdks/python/` unless a path is shown.

**Verified evidence (re-cited at origin/main b9bcc3e / merge 197bb1f):**

- `sdks/python/motosan_ai/providers/claude_code.py:592-599` — chat() hardcodes `tool_calls=[]` (:594) and `stop_reason=StopReason.end_turn` (:597) after a single-shot `proc.communicate()` (:565-570).
- `sdks/python/motosan_ai/providers/claude_code.py:678-680` — stream terminal is conditional; the exact expression is:
  ```python
  event.stop_reason = (
      StopReason.tool_use if saw_tool_call else StopReason.end_turn
  )
  ```
  with `saw_tool_call` set at :670-675 and initialized at :635.
- `sdks/python/motosan_ai/providers/codex_cli.py:395-403` — chat() hardcodes `tool_calls=[]` (:398) and `StopReason.end_turn` (:401), collecting stdout via its own inline loop (:383-393).
- `sdks/python/motosan_ai/providers/codex_cli.py:465-468` — stream terminal: `if event.done: saw_done = True` then `if event.done and saw_tool_call: event.stop_reason = StopReason.tool_use`. Note: a text-only codex turn currently leaves the terminal `stop_reason` as `None` (only the tool_use branch assigns).
- `sdks/python/motosan_ai/providers/gemini_cli.py:361-368` — chat() hardcodes `tool_calls=[]` (:363) and `StopReason.end_turn` (:366); inline collect loop at :349-359.
- `sdks/python/motosan_ai/providers/gemini_cli.py:429-433` — stream terminal conditional:
  ```python
  event.stop_reason = (
      StopReason.tool_use if saw_tool_call else StopReason.end_turn
  )
  ```
  with `saw_tool_call` set at :427-428 (only on `tool_call_start`), initialized at :385.
- **Collector:** `async def collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse` — `sdks/python/motosan_ai/_stream_collect.py:12`. It assembles content / thinking / tool_calls / usage / session_id / stop_reason and returns `model=""` (:83), so provider chat() must backfill model — this IS the one F4 parity exception. Provider chat() can await it over `self.stream(request)`: three providers already do exactly this — `anthropic.py:350`, `chatgpt_codex.py:418`, `gemini_code_assist.py:280` (`response = await collect_stream(self.stream(request))`). Task 5 rewrites the collector's thinking handling but keeps the name and signature. CLI providers emit no thinking events (claude parser ignores thinking blocks, claude_code.py:209), so `thinking` parity is trivially `None == None`.
- **Timeout / error mapping, single-shot vs stream path (read and compared):**
  - claude_code chat: whole-invocation `asyncio.wait_for(proc.communicate(...))` (:563-570) → `ProviderError("claude CLI timed out after {timeout} seconds")` (:571-574); nonzero exit → `ProviderError` (:576-579); agent-mode error result → `StreamError` via `_parse_agent_json` (:116-141 → `_raise_on_error_result` :98-113).
  - claude_code stream: per-read `asyncio.wait_for(proc.stdout.readline(), ...)` → `ProviderError("claude CLI stream read timed out after {timeout}s")` (:642-647); early EOF / child death → `StreamError` (:648-665); error result → `StreamError` via `_parse_ndjson_line` (:214). Child always reaped in `finally` (:684-691).
  - codex_cli chat: timeout :365-372 → `ProviderError` (:373-376); nonzero exit → `ProviderError` (:378-381); `turn.failed`/`error` events → `ProviderError` via `_parse_jsonl_line` (:188-200). Stream: per-read timeout → `ProviderError` (:434-439); early EOF → `StreamError` (:440-456).
  - gemini_cli chat: timeout :331-338 → `ProviderError` (:339-342); nonzero exit → `ProviderError` (:344-347); result failure → `ProviderError` via `_parse_jsonl_line` (:163-166). Stream: per-read timeout → `ProviderError` (:400-406); early EOF → `StreamError` (:407-424).
  - **Match verdict:** timeout mapping matches (both paths raise `ProviderError`; scope shifts from whole-invocation to per-read stall — the stream docstrings already document per-read). Mid-turn CLI error results match (`StreamError` for claude, `ProviderError` for codex/gemini — same parser both paths). The one delta: nonzero-exit/child-death surfaces as `StreamError` from the stream path instead of chat's `ProviderError`. Both subclass `MotosanError` (`error.py:32-41`). This delta is accepted as part of the F4 breaking change and is pinned by exactly two tests, enumerated in the Flip list.
- `motosan_ai/error.py:32-41` — `ProviderError` and `StreamError` are sibling subclasses of `MotosanError`.
- Fake-CLI test fixtures to reuse:
  - claude: `_make_proc()` in `tests/test_claude_code_runtime.py:11-26` — AsyncMock proc whose `stdout` bytes feed BOTH `proc.communicate()` (chat) and `proc.stdout.readline()` (stream); spawn stubbed via `monkeypatch.setattr("motosan_ai.providers.claude_code.asyncio.create_subprocess_exec", AsyncMock(...))`.
  - codex: `_FakeProc` / `_stub_subprocess` in `tests/test_codex_cli_stream.py:196-223` (stubs global `"asyncio.create_subprocess_exec"`).
  - gemini: identical `_FakeProc` / `_stub_subprocess` in `tests/test_gemini_cli_stream.py:196-223`.
- Single-shot-only helpers (grep results shown in Step 5): claude's `_parse_agent_json` (claude_code.py:116-141) is called only by chat() (:584) plus direct unit tests — dead after delegation, delete it. The `elif self._config.agent_mode: args.extend(["--output-format", "json"])` branch in `_build_args` (claude_code.py:449-450) is the single-shot agent-mode wire format — unreachable once chat delegates (both paths then pass `output_format="stream-json"`), delete it. codex/gemini chat use only shared helpers (`_messages_to_prompt`, `_compose_prompt`/`_merge_system_into_prompt`, `_parse_jsonl_line`) that stream() still uses — nothing to delete there.
- Live tests (`tests/integration/test_*_cli_live.py`, env-gated, excluded from the gate) assert `resp.tool_calls == []` only for a no-tool "pong" prompt (`test_claude_code_live.py:40`) — still valid under F4; no live test asserts a `tool_use` terminal. No flips there.
- Ruff config: `sdks/python/ruff.toml` — line-length 100, isort with `known-first-party = ["motosan_ai"]` (so `motosan_ai._stream_collect` sorts before `motosan_ai.error`).

**Files:**

Modify:
- `sdks/python/motosan_ai/providers/claude_code.py` (chat :543-599 → delegation; stream :635 + :669-683 → unconditional end_turn; delete `_parse_agent_json` :116-141; delete `elif` json branch :449-450; add `collect_stream` import)
- `sdks/python/motosan_ai/providers/codex_cli.py` (chat :353-403 → delegation; stream :418 + :458-471 → unconditional end_turn; add import)
- `sdks/python/motosan_ai/providers/gemini_cli.py` (chat :316-368 → delegation; stream :385 + :426-436 → unconditional end_turn; add import)
- `sdks/python/motosan_ai/_stream_collect.py` (:33-34 — comment only: the "CLI readback uses the provider chat()/stream() first-wins path, not this" note becomes false once CLI chat delegates)

Test:
- `sdks/python/tests/test_claude_code_runtime.py` (new parity test; flip :66-84; extend imports)
- `sdks/python/tests/test_claude_code.py` (drop `_parse_agent_json` import :13; delete `TestParseAgentJson` :85-125; flip `TestBuildArgs::test_agent_mode` :389-394)
- `sdks/python/tests/test_codex_cli_stream.py` (new parity test; flip :260-264, :336-348, :352-366; extend imports)
- `sdks/python/tests/test_gemini_cli_stream.py` (new parity test; flip :260-264, :337-348, :352-365; extend imports)

**Interfaces:**

Consumes:
- `collect_stream(events: AsyncIterator[StreamEvent]) -> ChatResponse` — `motosan_ai/_stream_collect.py:12` (post-Task-5 body differs; locate by name; signature unchanged).
- `StopReason.end_turn` (`motosan_ai/types.py:16`), `ToolCall(id: str, name: str, input: dict[str, Any])` dataclass with generated `__eq__` (`types.py:105-109`), mutable `ChatResponse` dataclass (`types.py:339-347` — `response.model` is assignable).
- Fixtures: `_make_proc` (`tests/test_claude_code_runtime.py:11-26`), `_FakeProc`/`_stub_subprocess` (`tests/test_codex_cli_stream.py:196-223`, `tests/test_gemini_cli_stream.py:196-223`).

Produces (later tasks rely on these exact facts):
- `ClaudeCodeClient.chat` / `CodexCliClient.chat` / `GeminiCliClient.chat` ≡ `await collect_stream(self.stream(request))` + model backfill (`request.model or self._config.model or ""`).
- CLI stream terminal event carries `stop_reason == StopReason.end_turn` unconditionally; `StopReason.tool_use` never appears in any CLI provider (spec task documents this; F7 release CHANGELOG cites it as BREAKING).
- `ChatResponse.tool_calls` from CLI chat = record of tools the CLI already executed (populated, never a request to execute).
- `_parse_agent_json` no longer exists in `motosan_ai.providers.claude_code` (F7 notes the removal).
- chat() nonzero-exit / child-death error type is now `StreamError` (was `ProviderError`); timeout errors remain `ProviderError` with per-read-stall scope (F7 CHANGELOG BREAKING note).

**Behavioral deltas (intentional, document in CHANGELOG during F7 — do not "fix" them back):**
1. chat() timeout scope: whole-invocation → per-read stall (same `ProviderError` type).
2. chat() nonzero-exit/child-death: `ProviderError` → `StreamError` (sibling `MotosanError` subclasses).
3. claude chat() wire format: always `--output-format stream-json --verbose` (previously raw text, or `--output-format json` in agent mode); usage + session_id now populate even without `agent_mode`.

**Flip list** (executors may modify ONLY these existing tests, exactly as specified in the steps):
- `tests/test_claude_code_runtime.py::test_stream_tool_use_sets_terminal_stop_reason` → renamed `test_stream_tool_use_terminal_is_end_turn`; asserts `StopReason.end_turn`.
- `tests/test_codex_cli_stream.py::test_stream_tool_call_sets_tool_use_stop_reason` → renamed `test_stream_tool_call_terminal_is_end_turn`; asserts `StopReason.end_turn`.
- `tests/test_gemini_cli_stream.py::test_stream_tool_call_terminal_is_tool_use` → renamed `test_stream_tool_call_terminal_is_end_turn`; asserts `StopReason.end_turn`.
- `tests/test_codex_cli_stream.py::test_chat_does_not_surface_tool_calls` → renamed `test_chat_surfaces_executed_tool_record`; asserts `tool_calls == [ToolCall(id="i0", name="command_execution", input={"command": "ls"})]` and `stop_reason == StopReason.end_turn`.
- `tests/test_gemini_cli_stream.py::test_blocking_chat_tool_calls_empty` → renamed `test_chat_surfaces_executed_tool_record`; asserts `tool_calls == [ToolCall(id="t1", name="read_file", input={})]` and `stop_reason == StopReason.end_turn`.
- `tests/test_codex_cli_stream.py::test_chat_raises_on_nonzero_returncode` → expects `StreamError` (was `ProviderError`), same `match="bad config"`.
- `tests/test_gemini_cli_stream.py::test_chat_raises_on_nonzero_returncode` → expects `StreamError` (was `ProviderError`), same `match="bad config"`.
- `tests/test_claude_code.py::TestParseAgentJson` — all six tests (`test_with_usage`, `test_without_usage`, `test_invalid_json`, `test_returns_session_id`, `test_session_id_none_when_absent`, `test_error_result_raises_stream_error`) DELETED together with `_parse_agent_json` (they test the deleted helper directly).
- `tests/test_claude_code.py::TestBuildArgs::test_agent_mode` → agent mode no longer implies `--output-format json`; asserts `--dangerously-skip-permissions` present and `--output-format` absent.

Everything else stays green untouched — verified against the fixtures: `_make_proc`/`_FakeProc` feed `proc.stdout.readline()` as well as `communicate()`, so existing chat tests (cwd/env/system-prompt/session-id/usage/error-result/no-timeout) pass through the delegated path. In particular `test_claude_code_runtime.py::test_chat_agent_mode_error_result_raises_stream_error` still passes (the stream parser raises the same `StreamError` via `_raise_on_error_result`), and `test_gemini_cli_stream.py::test_chat_raises_on_result_failure` still passes (`ProviderError` from `_parse_jsonl_line` on both paths).

---

#### Claude Code provider

- [ ] **Step 1: Write the failing claude_code parity test.**
  In `sdks/python/tests/test_claude_code_runtime.py`, replace the import block (lines 1-8) with:

  ```python
  import os
  from unittest.mock import AsyncMock, patch

  import pytest

  from motosan_ai._stream_collect import collect_stream
  from motosan_ai.error import StreamError
  from motosan_ai.providers.claude_code import ClaudeCodeClient
  from motosan_ai.types import ChatRequest, Message, Role, StopReason, ToolCall
  ```

  Append at the end of the file:

  ```python
  @pytest.mark.asyncio
  async def test_chat_stream_parity_tool_turn_is_end_turn(monkeypatch):
      """F4: chat() == collect_stream(stream()) for a text + tool-call + terminal
      transcript. Both paths report stop_reason end_turn and surface the
      executed-tool record; model backfill is the one allowed difference."""
      stdout = (
          b'{"type":"assistant","message":{"content":['
          b'{"type":"text","text":"let me read it"},'
          b'{"type":"tool_use","id":"toolu_01","name":"Read","input":{"path":"/tmp/x"}}]}}\n'
          b'{"type":"result","result":"done","session_id":"sess_1",'
          b'"usage":{"input_tokens":7,"output_tokens":3}}\n'
      )
      monkeypatch.setattr(
          "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
          AsyncMock(side_effect=[_make_proc(stdout=stdout), _make_proc(stdout=stdout)]),
      )
      client = ClaudeCodeClient()
      request = ChatRequest(messages=[Message(role=Role.user, content="hi")])

      chat_resp = await client.chat(request)
      streamed = await collect_stream(client.stream(request))

      expected_tool_calls = [ToolCall(id="toolu_01", name="Read", input={"path": "/tmp/x"})]
      assert chat_resp.tool_calls == expected_tool_calls
      assert chat_resp.stop_reason == StopReason.end_turn
      assert streamed.stop_reason == StopReason.end_turn
      # Parity, field by field (model exempt: chat backfills it from config).
      assert chat_resp.content == streamed.content
      assert chat_resp.thinking == streamed.thinking
      assert chat_resp.tool_calls == streamed.tool_calls
      assert chat_resp.stop_reason == streamed.stop_reason
      assert chat_resp.usage == streamed.usage
      assert chat_resp.session_id == streamed.session_id
      assert chat_resp.content == "let me read it"
      assert chat_resp.usage.input_tokens == 7
      assert chat_resp.usage.output_tokens == 3
      assert chat_resp.session_id == "sess_1"
  ```

- [ ] **Step 2: Run it — must fail on empty tool_calls.**

  ```bash
  uv run pytest tests/test_claude_code_runtime.py::test_chat_stream_parity_tool_turn_is_end_turn -q
  ```

  Expected failure (old chat() hardcodes `tool_calls=[]`):

  ```
  E       AssertionError: assert [] == [ToolCall(id='toolu_01', name='Read', input={'path': '/tmp/x'})]
  1 failed in ...
  ```

- [ ] **Step 3: Implement delegation + unconditional end_turn in claude_code.py.**
  (a) Add the import — in `sdks/python/motosan_ai/providers/claude_code.py`, above `from motosan_ai.error import ProviderError, StreamError` (line 10), insert:

  ```python
  from motosan_ai._stream_collect import collect_stream
  ```

  (b) Replace the entire `chat` method (currently lines 543-599, from `async def chat(self, request: ChatRequest) -> ChatResponse:` through `session_id=session_id,\n        )`) with:

  ```python
      async def chat(self, request: ChatRequest) -> ChatResponse:
          """Collect :meth:`stream` into one response (F4 delegation).

          content / thinking / tool_calls / usage / session_id / stop_reason
          are identical to collecting :meth:`stream` by construction;
          ``tool_calls`` is the record of tools the CLI already executed, and
          a completed turn always reports ``StopReason.end_turn``. The one
          documented parity exception: ``model`` is backfilled from the
          request or client config because CLI transcripts do not echo a
          model name. Error mapping follows the stream path: per-read stalls
          raise ``ProviderError``; CLI error results and early child death
          raise ``StreamError``.
          """
          response = await collect_stream(self.stream(request))
          response.model = request.model or self._config.model or ""
          return response
  ```

  (c) In `stream()`, delete `saw_tool_call = False` (line 635) and replace the event loop body (lines 669-683, from `for event in _parse_ndjson_line(line):` through the `return`) with:

  ```python
                  for event in _parse_ndjson_line(line):
                      if event.done:
                          saw_done = True
                          # F4: the CLI executes tools internally; a completed
                          # turn always ends the turn — never a tool_use request.
                          event.stop_reason = StopReason.end_turn
                      yield event
                      if event.done:
                          return
  ```

  Keep `saw_done` — the EOF-before-terminal guard (:648-665) depends on it. Do NOT touch `_parse_agent_json` or `_build_args` yet (Step 5).

- [ ] **Step 4: Run — parity passes; exactly one pinned test now fails.**

  ```bash
  uv run pytest tests/test_claude_code_runtime.py::test_chat_stream_parity_tool_turn_is_end_turn -q
  ```

  Expected: `1 passed`.

  ```bash
  uv run pytest tests/test_claude_code.py tests/test_claude_code_runtime.py tests/test_claude_code_flags.py tests/test_claude_code_stream_usage.py -q
  ```

  Expected: exactly one failure, the flip-listed terminal pin:

  ```
  FAILED tests/test_claude_code_runtime.py::test_stream_tool_use_sets_terminal_stop_reason - AssertionError: assert <StopReason.end_turn: 'end_turn'> == <StopReason.tool_use: 'tool_use'>
  1 failed, ... passed
  ```

- [ ] **Step 5: Delete the dead single-shot helper + apply the claude flips.**
  (a) Prove deadness first (grep, from `sdks/python/`):

  ```bash
  grep -rn "_parse_agent_json" motosan_ai/ tests/
  ```

  Expected after Step 3: matches ONLY in `motosan_ai/providers/claude_code.py` (the def) and `tests/test_claude_code.py` (import + `TestParseAgentJson`). No other consumer → safe to delete.

  (b) In `motosan_ai/providers/claude_code.py`: delete the whole `_parse_agent_json` function (lines 116-141, `def _parse_agent_json(raw: str) -> tuple[str, Usage, str | None]:` through its `return (...)`). Then in `_build_args`, replace lines 445-450:

  ```python
          if output_format is not None:
              args.extend(["--output-format", output_format])
              if output_format == "stream-json":
                  args.append("--verbose")
          elif self._config.agent_mode:
              args.extend(["--output-format", "json"])
  ```

  with:

  ```python
          if output_format is not None:
              args.extend(["--output-format", output_format])
              if output_format == "stream-json":
                  args.append("--verbose")
  ```

  (`Usage`, `json`, `ProviderError` remain used by `_parse_ndjson_line` and the stream loop — do not remove those imports.)

  (c) In `tests/test_claude_code.py`: remove `_parse_agent_json,` from the import list (line 13); delete the whole `TestParseAgentJson` class (lines 85-125) and its section-divider comment (lines 80-83); replace `TestBuildArgs::test_agent_mode` (lines 389-394) with:

  ```python
      def test_agent_mode(self):
          client = ClaudeCodeClient().agent_mode(True)
          args = client._build_args(model=None, system_prompt=None)
          assert "--dangerously-skip-permissions" in args
          # F4: the single-shot `--output-format json` mode is gone; chat()
          # delegates to stream(), which always passes "stream-json".
          assert "--output-format" not in args
  ```

  (d) In `tests/test_claude_code_runtime.py`: replace `test_stream_tool_use_sets_terminal_stop_reason` (lines 65-84) with:

  ```python
  @pytest.mark.asyncio
  async def test_stream_tool_use_terminal_is_end_turn(monkeypatch):
      stdout = (
          b'{"type":"assistant","message":{"content":['
          b'{"type":"tool_use","id":"toolu_01","name":"Read","input":{}}]}}\n'
          b'{"type":"result","result":"done"}\n'
      )
      proc = _make_proc(stdout=stdout)
      monkeypatch.setattr(
          "motosan_ai.providers.claude_code.asyncio.create_subprocess_exec",
          AsyncMock(return_value=proc),
      )
      events = [
          ev
          async for ev in ClaudeCodeClient().stream(
              ChatRequest(messages=[Message(role=Role.user, content="hi")])
          )
      ]
      done = [e for e in events if e.done][-1]
      # F4: CLI backends execute tools internally; a completed turn is
      # always end_turn, never a tool_use request.
      assert done.stop_reason == StopReason.end_turn
  ```

  (e) In `motosan_ai/_stream_collect.py`, replace the stale comment (lines 33-34):

  ```python
          # last-wins on session_id (HTTP providers never set it, so usually None);
          # CLI readback uses the provider chat()/stream() first-wins path, not this.
  ```

  with:

  ```python
          # last-wins on session_id (HTTP providers never set it, so usually None);
          # CLI providers emit exactly one session event per stream.
  ```

- [ ] **Step 6: Claude gate + commit.**

  ```bash
  uv run pytest tests/test_claude_code.py tests/test_claude_code_runtime.py tests/test_claude_code_flags.py tests/test_claude_code_stream_usage.py tests/test_client_stream_collect.py -q
  ```

  Expected: `... passed` (0 failed).

  ```bash
  uv run ruff format motosan_ai/ tests/ && uv run ruff check motosan_ai/
  ```

  Expected: `N files reformatted/left unchanged`, `All checks passed!`

  ```bash
  git add motosan_ai/providers/claude_code.py motosan_ai/_stream_collect.py tests/test_claude_code.py tests/test_claude_code_runtime.py
  git commit -m "feat(python)!: claude_code chat delegates to stream collection, end_turn terminal" -m "F4: chat() = collect_stream(stream()) with model backfill; CLI terminals
  are always end_turn (tools are executed internally by the CLI, never a
  request for the caller). Removes the dead single-shot _parse_agent_json
  helper and the agent-mode --output-format json branch.

  BREAKING: chat() surfaces executed-tool records in tool_calls; stream
  terminals never report tool_use; chat() child-death errors are now
  StreamError; chat timeout is a per-read stall deadline.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

#### Codex CLI provider

- [ ] **Step 7: Write the failing codex_cli parity test.**
  In `sdks/python/tests/test_codex_cli_stream.py`, update imports: insert `from motosan_ai._stream_collect import collect_stream` directly above `from motosan_ai.error import ProviderError, StreamError` (line 9), and change line 16 to:

  ```python
  from motosan_ai.types import ChatRequest, Message, StopReason, ToolCall
  ```

  Append at the end of the file:

  ```python
  @pytest.mark.asyncio
  async def test_chat_stream_parity_tool_turn_is_end_turn(monkeypatch):
      """F4: chat() == collect_stream(stream()) for a text + tool-call + terminal
      transcript; both end_turn, tool record populated (model exempt)."""
      jsonl = (
          '{"type": "thread.started", "thread_id": "th_1"}\n'
          '{"type": "item.completed", "item": {"type": "agent_message", "text": "running ls"}}\n'
          '{"type": "item.completed", "item": {"id": "i0", "type": "command_execution", '
          '"command": "ls"}}\n'
          '{"type": "turn.completed", "usage": {"input_tokens": 9, "output_tokens": 4}}\n'
      )
      monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
      monkeypatch.setattr(
          "asyncio.create_subprocess_exec",
          AsyncMock(side_effect=[_FakeProc(jsonl), _FakeProc(jsonl, returncode=None)]),
      )
      client = CodexCliClient(binary_path="codex")
      request = ChatRequest(messages=[Message.user("hi")])

      chat_resp = await client.chat(request)
      streamed = await collect_stream(client.stream(request))

      expected = [ToolCall(id="i0", name="command_execution", input={"command": "ls"})]
      assert chat_resp.tool_calls == expected
      assert chat_resp.stop_reason == StopReason.end_turn
      assert streamed.stop_reason == StopReason.end_turn
      assert chat_resp.content == streamed.content
      assert chat_resp.thinking == streamed.thinking
      assert chat_resp.tool_calls == streamed.tool_calls
      assert chat_resp.stop_reason == streamed.stop_reason
      assert chat_resp.usage == streamed.usage
      assert chat_resp.session_id == streamed.session_id
      assert chat_resp.content == "running ls"
      assert chat_resp.usage.input_tokens == 9
      assert chat_resp.usage.output_tokens == 4
      assert chat_resp.session_id == "th_1"
  ```

- [ ] **Step 8: Run it — must fail on empty tool_calls.**

  ```bash
  uv run pytest tests/test_codex_cli_stream.py::test_chat_stream_parity_tool_turn_is_end_turn -q
  ```

  Expected failure:

  ```
  E       AssertionError: assert [] == [ToolCall(id='i0', name='command_execution', input={'command': 'ls'})]
  1 failed in ...
  ```

- [ ] **Step 9: Implement delegation + unconditional end_turn in codex_cli.py.**
  (a) In `sdks/python/motosan_ai/providers/codex_cli.py`, above `from motosan_ai.error import ProviderError, StreamError` (line 11), insert:

  ```python
  from motosan_ai._stream_collect import collect_stream
  ```

  (b) Replace the entire `chat` method (lines 353-403, from `async def chat(self, request: ChatRequest) -> ChatResponse:` through `session_id=session_id,\n        )`) with:

  ```python
      async def chat(self, request: ChatRequest) -> ChatResponse:
          """Collect :meth:`stream` into one response (F4 delegation).

          ``tool_calls`` is the record of tools the CLI already executed; a
          completed turn always reports ``StopReason.end_turn``. Parity
          exception: ``model`` is backfilled from the request or client
          config. Error mapping follows the stream path: per-read stalls
          raise ``ProviderError``; turn failures raise ``ProviderError`` via
          the parser; early child death raises ``StreamError``.
          """
          response = await collect_stream(self.stream(request))
          response.model = request.model or self._config.model or ""
          return response
  ```

  (c) In `stream()`, delete `saw_tool_call = False` (line 418) and replace the event loop body (lines 457-471, from `line = raw.decode().rstrip("\n")` through the `return`) with:

  ```python
                  line = raw.decode().rstrip("\n")
                  for event in _parse_jsonl_line(line):
                      if event.done:
                          saw_done = True
                          # F4: the CLI executes tools internally; a completed
                          # turn always ends the turn — never a tool_use request.
                          event.stop_reason = StopReason.end_turn
                      yield event
                      if event.done:
                          return
  ```

  (This also fixes the pre-existing gap where a text-only codex terminal carried `stop_reason=None`.)

- [ ] **Step 10: Run — parity passes; exactly three pinned tests fail.**

  ```bash
  uv run pytest tests/test_codex_cli_stream.py tests/test_codex_cli_flags.py tests/test_codex_cli_dispatch.py -q
  ```

  Expected — exactly these three flip-listed failures:

  ```
  FAILED tests/test_codex_cli_stream.py::test_chat_raises_on_nonzero_returncode - motosan_ai.error.StreamError: codex CLI exited unexpectedly (returncode 2): codex: bad config
  FAILED tests/test_codex_cli_stream.py::test_chat_does_not_surface_tool_calls - AssertionError: assert [ToolCall(id='i0', name='command_execution', input={'command': 'ls'})] == []
  FAILED tests/test_codex_cli_stream.py::test_stream_tool_call_sets_tool_use_stop_reason - AssertionError: assert <StopReason.end_turn: 'end_turn'> == <StopReason.tool_use: 'tool_use'>
  3 failed, ... passed
  ```

- [ ] **Step 11: Apply the codex flips.**
  In `tests/test_codex_cli_stream.py`:
  (a) Replace `test_chat_raises_on_nonzero_returncode` (lines 259-264) with:

  ```python
  @pytest.mark.asyncio
  async def test_chat_raises_on_nonzero_returncode(monkeypatch):
      _stub_subprocess(monkeypatch, _FakeProc("", returncode=2, stderr="codex: bad config\n"))
      client = CodexCliClient(binary_path="codex")
      # F4: chat() delegates to stream(); a child that dies without a terminal
      # event surfaces as StreamError (was ProviderError on the single-shot path).
      with pytest.raises(StreamError, match="bad config"):
          await client.chat(ChatRequest(messages=[Message.user("hi")]))
  ```

  (b) Replace `test_chat_does_not_surface_tool_calls` (lines 335-348) with:

  ```python
  @pytest.mark.asyncio
  async def test_chat_surfaces_executed_tool_record(monkeypatch):
      jsonl = (
          '{"type": "item.completed", "item": {"id": "i0", "type": "command_execution", '
          '"command": "ls"}}\n'
          '{"type": "item.completed", "item": {"type": "agent_message", "text": "done"}}\n'
          '{"type": "turn.completed"}\n'
      )
      _stub_subprocess(monkeypatch, _FakeProc(jsonl))
      resp = await CodexCliClient(binary_path="codex").chat(
          ChatRequest(messages=[Message.user("hi")])
      )
      # F4: tool_calls records what the CLI already executed — never a
      # request for the caller to execute tools.
      expected = [ToolCall(id="i0", name="command_execution", input={"command": "ls"})]
      assert resp.tool_calls == expected
      assert resp.stop_reason == StopReason.end_turn
      assert "done" in resp.content
  ```

  (c) Replace `test_stream_tool_call_sets_tool_use_stop_reason` (lines 351-366) with:

  ```python
  @pytest.mark.asyncio
  async def test_stream_tool_call_terminal_is_end_turn(monkeypatch):
      jsonl = (
          '{"type": "item.completed", "item": {"id": "i0", "type": "command_execution", '
          '"command": "ls"}}\n'
          '{"type": "turn.completed"}\n'
      )
      _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=None))
      events = [
          ev
          async for ev in CodexCliClient(binary_path="codex").stream(
              ChatRequest(messages=[Message.user("hi")])
          )
      ]
      done = [e for e in events if e.done][-1]
      # F4: CLI backends never report tool_use.
      assert done.stop_reason == StopReason.end_turn
  ```

- [ ] **Step 12: Codex gate + commit.**

  ```bash
  uv run pytest tests/test_codex_cli_stream.py tests/test_codex_cli_flags.py tests/test_codex_cli_dispatch.py -q
  ```

  Expected: `... passed` (0 failed).

  ```bash
  uv run ruff format motosan_ai/ tests/ && uv run ruff check motosan_ai/
  ```

  Expected: `All checks passed!`

  ```bash
  git add motosan_ai/providers/codex_cli.py tests/test_codex_cli_stream.py
  git commit -m "feat(python)!: codex_cli chat delegates to stream collection, end_turn terminal" -m "F4: chat() = collect_stream(stream()) with model backfill; terminal is
  always end_turn (also fixes text-only terminals that carried a None
  stop_reason).

  BREAKING: chat() surfaces executed-tool records in tool_calls; stream
  terminals never report tool_use; chat() child-death errors are now
  StreamError; chat timeout is a per-read stall deadline.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

#### Gemini CLI provider

- [ ] **Step 13: Write the failing gemini_cli parity test.**
  In `sdks/python/tests/test_gemini_cli_stream.py`, update imports: insert `from motosan_ai._stream_collect import collect_stream` directly above `from motosan_ai.error import ProviderError, StreamError` (line 9), and change line 16 to:

  ```python
  from motosan_ai.types import ChatRequest, Message, StopReason, ToolCall
  ```

  Append at the end of the file:

  ```python
  @pytest.mark.asyncio
  async def test_chat_stream_parity_tool_turn_is_end_turn(monkeypatch):
      """F4: chat() == collect_stream(stream()) for a text + tool-call + terminal
      transcript; both end_turn, tool record populated (model exempt)."""
      jsonl = (
          '{"type": "init", "session_id": "s1"}\n'
          '{"type": "message", "role": "assistant", "content": "reading", "delta": true}\n'
          '{"type": "tool_use", "tool_id": "t1", "tool_name": "read_file", '
          '"parameters": {"file_path": "Cargo.toml"}}\n'
          '{"type": "result", "status": "success", '
          '"stats": {"input_tokens": 5, "output_tokens": 2}}\n'
      )
      monkeypatch.setattr(asyncio.subprocess, "PIPE", -1, raising=False)
      monkeypatch.setattr(
          "asyncio.create_subprocess_exec",
          AsyncMock(side_effect=[_FakeProc(jsonl), _FakeProc(jsonl, returncode=None)]),
      )
      client = GeminiCliClient(binary_path="gemini")
      request = ChatRequest(messages=[Message.user("hi")])

      chat_resp = await client.chat(request)
      streamed = await collect_stream(client.stream(request))

      expected = [ToolCall(id="t1", name="read_file", input={"file_path": "Cargo.toml"})]
      assert chat_resp.tool_calls == expected
      assert chat_resp.stop_reason == StopReason.end_turn
      assert streamed.stop_reason == StopReason.end_turn
      assert chat_resp.content == streamed.content
      assert chat_resp.thinking == streamed.thinking
      assert chat_resp.tool_calls == streamed.tool_calls
      assert chat_resp.stop_reason == streamed.stop_reason
      assert chat_resp.usage == streamed.usage
      assert chat_resp.session_id == streamed.session_id
      assert chat_resp.content == "reading"
      assert chat_resp.usage.input_tokens == 5
      assert chat_resp.usage.output_tokens == 2
      assert chat_resp.session_id == "s1"
  ```

- [ ] **Step 14: Run it — must fail on empty tool_calls.**

  ```bash
  uv run pytest tests/test_gemini_cli_stream.py::test_chat_stream_parity_tool_turn_is_end_turn -q
  ```

  Expected failure:

  ```
  E       AssertionError: assert [] == [ToolCall(id='t1', name='read_file', input={'file_path': 'Cargo.toml'})]
  1 failed in ...
  ```

- [ ] **Step 15: Implement delegation + unconditional end_turn in gemini_cli.py.**
  (a) In `sdks/python/motosan_ai/providers/gemini_cli.py`, above `from motosan_ai.error import ProviderError, StreamError` (line 11), insert:

  ```python
  from motosan_ai._stream_collect import collect_stream
  ```

  (b) Replace the entire `chat` method (lines 316-368, from `async def chat(self, request: ChatRequest) -> ChatResponse:` through `session_id=session_id,\n        )`) with:

  ```python
      async def chat(self, request: ChatRequest) -> ChatResponse:
          """Collect :meth:`stream` into one response (F4 delegation).

          ``tool_calls`` is the record of tools the CLI already executed; a
          completed turn always reports ``StopReason.end_turn``. Parity
          exception: ``model`` is backfilled from the request or client
          config. Error mapping follows the stream path: per-read stalls
          raise ``ProviderError``; result failures raise ``ProviderError``
          via the parser; early child death raises ``StreamError``.
          """
          response = await collect_stream(self.stream(request))
          response.model = request.model or self._config.model or ""
          return response
  ```

  (c) In `stream()`, delete `saw_tool_call = False` (line 385) and replace the event loop body (lines 425-436, from `line = raw.decode().rstrip("\n")` through the `return`) with:

  ```python
                  line = raw.decode().rstrip("\n")
                  for event in _parse_jsonl_line(line):
                      if event.done:
                          saw_done = True
                          # F4: the CLI executes tools internally; a completed
                          # turn always ends the turn — never a tool_use request.
                          event.stop_reason = StopReason.end_turn
                      yield event
                      if event.done:
                          return
  ```

- [ ] **Step 16: Run — parity passes; exactly three pinned tests fail.**

  ```bash
  uv run pytest tests/test_gemini_cli_stream.py tests/test_gemini_cli_flags.py tests/test_gemini_cli_dispatch.py -q
  ```

  Expected — exactly these three flip-listed failures:

  ```
  FAILED tests/test_gemini_cli_stream.py::test_chat_raises_on_nonzero_returncode - motosan_ai.error.StreamError: gemini CLI exited unexpectedly (returncode 2): gemini: bad config
  FAILED tests/test_gemini_cli_stream.py::test_blocking_chat_tool_calls_empty - AssertionError: assert [ToolCall(id='t1', name='read_file', input={})] == []
  FAILED tests/test_gemini_cli_stream.py::test_stream_tool_call_terminal_is_tool_use - AssertionError: assert <StopReason.end_turn: 'end_turn'> == <StopReason.tool_use: 'tool_use'>
  3 failed, ... passed
  ```

- [ ] **Step 17: Apply the gemini flips.**
  In `tests/test_gemini_cli_stream.py`:
  (a) Replace `test_chat_raises_on_nonzero_returncode` (lines 259-264) with:

  ```python
  @pytest.mark.asyncio
  async def test_chat_raises_on_nonzero_returncode(monkeypatch):
      _stub_subprocess(monkeypatch, _FakeProc("", returncode=2, stderr="gemini: bad config\n"))
      client = GeminiCliClient(binary_path="gemini")
      # F4: chat() delegates to stream(); a child that dies without a terminal
      # event surfaces as StreamError (was ProviderError on the single-shot path).
      with pytest.raises(StreamError, match="bad config"):
          await client.chat(ChatRequest(messages=[Message.user("hi")]))
  ```

  (b) Replace `test_blocking_chat_tool_calls_empty` (lines 336-348) with:

  ```python
  @pytest.mark.asyncio
  async def test_chat_surfaces_executed_tool_record(monkeypatch):
      jsonl = (
          '{"type": "tool_use", "tool_id": "t1", "tool_name": "read_file", "parameters": {}}\n'
          '{"type": "message", "role": "assistant", "content": "hi", "delta": true}\n'
          '{"type": "result", "status": "success"}\n'
      )
      _stub_subprocess(monkeypatch, _FakeProc(jsonl))
      resp = await GeminiCliClient(binary_path="gemini").chat(
          ChatRequest(messages=[Message.user("hi")])
      )
      # F4: tool_calls records what the CLI already executed — never a
      # request for the caller to execute tools.
      assert resp.tool_calls == [ToolCall(id="t1", name="read_file", input={})]
      assert resp.stop_reason == StopReason.end_turn
      assert "hi" in resp.content
  ```

  (c) Replace `test_stream_tool_call_terminal_is_tool_use` (lines 351-365) with:

  ```python
  @pytest.mark.asyncio
  async def test_stream_tool_call_terminal_is_end_turn(monkeypatch):
      jsonl = (
          '{"type": "tool_use", "tool_id": "t1", "tool_name": "read_file", "parameters": {}}\n'
          '{"type": "result", "status": "success"}\n'
      )
      _stub_subprocess(monkeypatch, _FakeProc(jsonl, returncode=None))
      events = [
          ev
          async for ev in GeminiCliClient(binary_path="gemini").stream(
              ChatRequest(messages=[Message.user("hi")])
          )
      ]
      done = [e for e in events if e.done][-1]
      # F4: CLI backends never report tool_use.
      assert done.stop_reason == StopReason.end_turn
  ```

- [ ] **Step 18: Gemini gate + commit.**

  ```bash
  uv run pytest tests/test_gemini_cli_stream.py tests/test_gemini_cli_flags.py tests/test_gemini_cli_dispatch.py -q
  ```

  Expected: `... passed` (0 failed).

  ```bash
  uv run ruff format motosan_ai/ tests/ && uv run ruff check motosan_ai/
  ```

  Expected: `All checks passed!`

  ```bash
  git add motosan_ai/providers/gemini_cli.py tests/test_gemini_cli_stream.py
  git commit -m "feat(python)!: gemini_cli chat delegates to stream collection, end_turn terminal" -m "F4: chat() = collect_stream(stream()) with model backfill; terminal is
  always end_turn.

  BREAKING: chat() surfaces executed-tool records in tool_calls; stream
  terminals never report tool_use; chat() child-death errors are now
  StreamError; chat timeout is a per-read stall deadline.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

#### Full gate

- [ ] **Step 19: Full Python gate (CI-matching) + F4 verification greps.**

  ```bash
  uv run ruff check motosan_ai/
  uv run ruff format --check motosan_ai/ tests/
  uv run pytest tests/ -q --ignore=tests/integration
  ```

  Expected: `All checks passed!`, `N files already formatted`, `... passed` (0 failed).

  ```bash
  grep -n "StopReason.tool_use" motosan_ai/providers/claude_code.py motosan_ai/providers/codex_cli.py motosan_ai/providers/gemini_cli.py
  ```

  Expected: **no matches** (exit code 1). The only remaining `"tool_use"` strings in these files are the CLI wire-format block/event names inside the parsers (claude_code.py `btype == "tool_use"`, gemini_cli.py `event_type == "tool_use"`), which are upstream JSON vocabulary, not SDK stop reasons.

  ```bash
  grep -rn "_parse_agent_json" motosan_ai/ tests/
  ```

  Expected: **no matches** (exit code 1).

  If anything failed, fix within this task's file set only, re-run the gate, and amend the relevant commit. No push/PR here — Task 7+ (release, F7) owns CHANGELOG/version/PR mechanics; the three BREAKING deltas above must be handed to it verbatim. Optional live smoke (requires the real CLIs; not part of the gate): `MOTOSAN_RUN_CLAUDE_CODE_LIVE=1 uv run pytest tests/integration/test_claude_code_live.py -q` — its `tool_calls == []` assertion is for a no-tool prompt and remains valid.

**Done criteria:**
- All three CLI providers' `chat()` bodies are three statements: collect, backfill model, return — no subprocess code.
- `StopReason.tool_use` appears nowhere in the three CLI provider files; every terminal event sets `StopReason.end_turn`.
- Three new parity tests pass; all nine flip-list changes applied; `_parse_agent_json` and the agent-mode `--output-format json` branch deleted.
- Full Python gate green; three conventional commits on PR-P, each with the `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` trailer.

---

### Task 7: Python token_source seam for chatgpt_codex (F5, additive)

**Branch:** PR-P — continue on the existing PR-P branch after Task 6 completes. Do not create a new branch. Work from the Python SDK dir: `sdks/python/`.

**Files:**
- Modify: `sdks/python/motosan_ai/providers/chatgpt_codex.py` — imports at :4 and :11-19 (`parse_retry_after_header` import verified at :21); `__init__` at :194-217; `_headers()` at :239-247 (Bearer header at :241); `stream()` request site at :364-373; `chat()` delegation at :417-426
- Modify: `sdks/python/motosan_ai/client.py` — stdlib import at :6; `Client.__init__` keyword section at :74-85; `Provider.openai_chatgpt` branch at :110-123 (validation :111-114); `Client.chatgpt_codex` classmethod at :263-283
- Test: `sdks/python/tests/test_chatgpt_codex_http.py` (append; reuse the respx 5xx-then-200 pattern from `test_chat_502_then_200_is_retried` at :210-228)
- Test: `sdks/python/tests/test_chatgpt_codex_request.py` (append constructor tests; flip `test_auth_headers_present_and_lowercase` at :38-45)
- Test: `sdks/python/tests/test_chatgpt_codex_dispatch.py` (append Client-construction tests)

Line refs verified at origin/main b9bcc3e. Tasks 5-6 (earlier PR-P quarters) touch `chatgpt_codex.py` (F3 thinking migration at :85) and `client.py` (F6 `Provider` enum + `claude_code` classmethod), so absolute numbers WILL have drifted by execution time — locate every item by name (`_headers`, `stream`, `openai_chatgpt` branch, `chatgpt_codex` classmethod), not by line.

**Verified reality check (differs from the milestone brief):** this provider has NO inline attempt loop. Retries live at the Client layer — `client.py` `_dispatch_chat` (:441-449, `with_retry` re-invokes `provider.chat()` per attempt) and `stream_with` (:483-531, inline loop re-invokes `provider.stream()` per attempt at :487). `chat()` itself is `collect_stream(self.stream(request))` (:417-426), so `stream()`'s body (:364-373) is the ONE request/header-build site and it re-executes from the top on every attempt. Therefore `await self._bearer()` at the top of `stream()` IS per-attempt resolution for BOTH the chat and stream paths. Do not hunt for loops inside the provider; do not add one.

**Interfaces:**
- Consumes: `motosan_ai.error.ConfigError` (`error.py:28`, subclass of `MotosanError`); `motosan_ai.retry.with_retry` (`retry.py:114`) and `motosan_ai.retry.RetryPolicy` (`retry.py:68`, fields `max_retries`, `base_delay`, `jitter`); `collect_stream` delegation in `chat()`; Client retry choke points `_dispatch_chat` / `stream_with` (unchanged by this task).
- Produces (later tasks — release notes/changelog task — rely on these exact names):
  - `ChatGptCodexProvider.__init__(self, access_token: str | None = None, account_id: str | None = None, model: str | None = None, base_url: str | None = None, *, token_source: Callable[[], Awaitable[str]] | None = None, connect_timeout: float = 10.0, read_idle_timeout: float = 120.0) -> None`
  - `ChatGptCodexProvider._bearer(self) -> str` (private, async)
  - `ChatGptCodexProvider._headers(self, bearer: str) -> dict[str, str]`
  - `Client.__init__(..., *, ..., token_source: Callable[[], Awaitable[str]] | None = None, ...)`
  - `Client.chatgpt_codex(..., token_source: Callable[[], Awaitable[str]] | None = None) -> Client`
  - The param name `token_source` matches Rust `with_token_source` and is the F5 cross-SDK parity anchor. Additive only — Python 0.18.0's BREAKING notes (F3/F4) do NOT include this change; changelog gets an "Added" entry in the release task, not here.

**Flip list:** (executors may touch ONLY these existing tests)
- `tests/test_chatgpt_codex_request.py::test_auth_headers_present_and_lowercase` — `_headers()` gains a required `bearer: str` parameter (F5 moves token resolution out of header construction); update the call to `._headers("tok")`. Assertions unchanged.

---

- [ ] **Step 1: Write failing provider-level per-attempt tests**

  Append to `sdks/python/tests/test_chatgpt_codex_http.py` (file already imports `json`, `httpx`, `pytest`, `respx`, `ProviderError`, `ChatGptCodexProvider`, `ChatRequest`, `Message`; `_URL` and `_text_stream()` helpers are at :13 and :20-28):

  ```python
  @respx.mock
  @pytest.mark.asyncio
  async def test_chat_token_source_resolved_per_attempt():
      from motosan_ai.retry import with_retry

      awaited: list[str] = []

      async def source() -> str:
          tok = f"tok-{len(awaited) + 1}"
          awaited.append(tok)
          return tok

      seen_auth: list[str] = []
      replies = iter(
          [
              httpx.Response(500, json={"error": {"message": "boom"}}),
              httpx.Response(
                  200, text=_text_stream(), headers={"content-type": "text/event-stream"}
              ),
          ]
      )

      def _respond(request: httpx.Request) -> httpx.Response:
          seen_auth.append(request.headers["authorization"])
          return next(replies)

      route = respx.post(_URL).mock(side_effect=_respond)
      p = ChatGptCodexProvider(account_id="acct-123", model="gpt-5.5", token_source=source)
      resp = await with_retry(
          lambda: p.chat(ChatRequest(messages=[Message.user("hi")])),
          max_retries=2,
          initial_backoff=0.001,
      )
      assert resp.content == "Hello world."
      assert route.call_count == 2
      assert awaited == ["tok-1", "tok-2"]
      assert seen_auth == ["Bearer tok-1", "Bearer tok-2"]


  @respx.mock
  @pytest.mark.asyncio
  async def test_stream_token_source_resolved_per_call():
      awaited: list[str] = []

      async def source() -> str:
          tok = f"tok-{len(awaited) + 1}"
          awaited.append(tok)
          return tok

      seen_auth: list[str] = []
      replies = iter(
          [
              httpx.Response(500, json={"error": {"message": "boom"}}),
              httpx.Response(
                  200, text=_text_stream(), headers={"content-type": "text/event-stream"}
              ),
          ]
      )

      def _respond(request: httpx.Request) -> httpx.Response:
          seen_auth.append(request.headers["authorization"])
          return next(replies)

      respx.post(_URL).mock(side_effect=_respond)
      p = ChatGptCodexProvider(account_id="acct-123", token_source=source)

      # Each stream() invocation resolves the token anew — this is exactly what
      # Client._dispatch_chat / Client.stream_with do once per retry attempt.
      with pytest.raises(ProviderError):
          async for _ in p.stream(ChatRequest(messages=[Message.user("hi")])):
              pass

      events = [e async for e in p.stream(ChatRequest(messages=[Message.user("hi")]))]
      assert events[-1].done is True
      assert awaited == ["tok-1", "tok-2"]
      assert seen_auth == ["Bearer tok-1", "Bearer tok-2"]
  ```

  Run:
  ```bash
  cd sdks/python && uv run pytest tests/test_chatgpt_codex_http.py -q -k "token_source"
  ```
  Expected failure (red):
  ```
  FAILED tests/test_chatgpt_codex_http.py::test_chat_token_source_resolved_per_attempt - TypeError: ChatGptCodexProvider.__init__() got an unexpected keyword argument 'token_source'
  FAILED tests/test_chatgpt_codex_http.py::test_stream_token_source_resolved_per_call - TypeError: ChatGptCodexProvider.__init__() got an unexpected keyword argument 'token_source'
  2 failed, 12 deselected ...
  ```
  (Deselected count may differ if Tasks 5-6 added tests to this file.)

- [ ] **Step 2: Write failing constructor-validation tests**

  Append to `sdks/python/tests/test_chatgpt_codex_request.py`. First add the two missing imports at the top of the file (after `import json` at :3):

  ```python
  import pytest

  from motosan_ai.error import ConfigError
  ```

  (ruff isort order: `json`, blank line, `pytest`, blank line, first-party `motosan_ai.*` — the existing `from motosan_ai.providers.chatgpt_codex import ...` block stays; `ConfigError` joins the first-party group.)

  Then append the tests:

  ```python
  def test_constructor_requires_access_token_or_token_source():
      with pytest.raises(ConfigError, match="access_token or token_source"):
          ChatGptCodexProvider(account_id="acct-123")


  def test_constructor_requires_account_id():
      with pytest.raises(ConfigError, match="account_id"):
          ChatGptCodexProvider(access_token="tok")


  def test_constructor_accepts_token_source_without_access_token():
      async def source() -> str:
          return "tok"

      p = ChatGptCodexProvider(account_id="acct-123", token_source=source)
      assert p.access_token is None
      assert p.token_source is source


  def test_static_access_token_leaves_token_source_none():
      p = ChatGptCodexProvider("tok", "acct-123")
      assert p.access_token == "tok"
      assert p.token_source is None
  ```

  Run:
  ```bash
  cd sdks/python && uv run pytest tests/test_chatgpt_codex_request.py -q -k "constructor or leaves_token_source"
  ```
  Expected failure (red) — four failures, messages:
  ```
  TypeError: ChatGptCodexProvider.__init__() missing 1 required positional argument: 'access_token'
  TypeError: ChatGptCodexProvider.__init__() missing 1 required positional argument: 'account_id'
  TypeError: ChatGptCodexProvider.__init__() got an unexpected keyword argument 'token_source'
  AttributeError: 'ChatGptCodexProvider' object has no attribute 'token_source'
  ```

- [ ] **Step 3: Implement the provider seam (minimal implementation)**

  All edits in `sdks/python/motosan_ai/providers/chatgpt_codex.py`.

  3a. Imports — replace line 4:
  ```python
  from collections.abc import AsyncIterator
  ```
  with:
  ```python
  from collections.abc import AsyncIterator, Awaitable, Callable
  ```
  and add `ConfigError` to the error import block (:11-19), keeping alphabetical order:
  ```python
  from motosan_ai.error import (
      AuthError,
      ConfigError,
      IncompleteStreamError,
      NetworkError,
      ProviderError,
      RateLimitError,
      StreamError,
      StreamReadTimeoutError,
  )
  ```

  3b. Replace `__init__` (currently :194-217) in full:
  ```python
      def __init__(
          self,
          access_token: str | None = None,
          account_id: str | None = None,
          model: str | None = None,
          base_url: str | None = None,
          *,
          token_source: Callable[[], Awaitable[str]] | None = None,
          connect_timeout: float = 10.0,
          read_idle_timeout: float = 120.0,
      ) -> None:
          if access_token is None and token_source is None:
              raise ConfigError("chatgpt_codex requires access_token or token_source")
          if not account_id:
              raise ConfigError("chatgpt_codex requires account_id")
          self.access_token = access_token
          self.token_source = token_source
          self.account_id = account_id
          self.model = model or _DEFAULT_MODEL
          self.base_url = base_url or _DEFAULT_BASE_URL
          self._reasoning_effort: str | None = None
          self._read_idle_timeout = read_idle_timeout
          self._http = httpx.AsyncClient(
              timeout=httpx.Timeout(
                  connect=connect_timeout,
                  read=read_idle_timeout,
                  write=read_idle_timeout,
                  pool=connect_timeout,
              )
          )
  ```
  Note: `account_id` gains a `None` default only because Python forbids a required positional after a defaulted one; omitting it now raises `ConfigError` instead of `TypeError` (deliberate, covered by `test_constructor_requires_account_id`). Positional callers (`ChatGptCodexProvider("tok", "acct-123", ...)`) are unaffected — the constructor is additive for every existing call shape in the codebase.

  3c. Add `_bearer` and change `_headers` (currently :239-247) — replace `_headers` in full and insert `_bearer` directly above it:
  ```python
      async def _bearer(self) -> str:
          """Resolve the bearer token for the current request attempt (F5).

          When ``token_source`` is set it is awaited on every call. The retry
          loops live in ``Client`` (``_dispatch_chat`` / ``stream_with``) and
          re-enter ``stream()`` once per attempt, so each attempt fetches a
          fresh token.
          """
          if self.token_source is not None:
              return await self.token_source()
          if self.access_token is None:  # pragma: no cover — guarded in __init__
              raise ConfigError("chatgpt_codex requires access_token or token_source")
          return self.access_token

      def _headers(self, bearer: str) -> dict[str, str]:
          return {
              "authorization": f"Bearer {bearer}",
              "chatgpt-account-id": self.account_id,
              "originator": _ORIGINATOR,
              "openai-beta": "responses=experimental",
              "accept": "text/event-stream",
              "content-type": "application/json",
          }
  ```

  3d. Edit the top of `stream()` (currently :364-373) — the provider's single request site; `chat()` (:417-426) delegates here via `collect_stream`, so this one edit covers both paths. Replace:
  ```python
      async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
          self.validate_request(request)
          body = self._build_responses_body(request)
          try:
              resp = await self._http.send(
                  self._http.build_request(
                      "POST", self._stream_url(), headers=self._headers(), json=body
                  ),
                  stream=True,
              )
  ```
  with:
  ```python
      async def stream(self, request: ChatRequest) -> AsyncIterator[StreamEvent]:
          self.validate_request(request)
          body = self._build_responses_body(request)
          # F5: resolved at the top of the attempt. Client retry loops re-invoke
          # stream() per attempt (chat() delegates here too), so a token_source
          # is consulted once per attempt. Token-source failures propagate
          # verbatim — they are auth plumbing, not transport errors.
          bearer = await self._bearer()
          try:
              resp = await self._http.send(
                  self._http.build_request(
                      "POST", self._stream_url(), headers=self._headers(bearer), json=body
                  ),
                  stream=True,
              )
  ```
  Everything from `except httpx.HTTPError as exc:` (:374) to the end of the method is byte-for-byte unchanged — do not touch it.

  3e. Apply the Flip list: in `tests/test_chatgpt_codex_request.py::test_auth_headers_present_and_lowercase` (:38-45) change only the first line of the body:
  ```python
      h = ChatGptCodexProvider("tok", "acct-123")._headers("tok")
  ```

  Run (green):
  ```bash
  cd sdks/python && uv run pytest tests/test_chatgpt_codex_http.py tests/test_chatgpt_codex_request.py tests/test_chatgpt_codex_stream.py tests/test_incomplete_stream.py tests/test_client_timeouts.py -q
  ```
  Expected: all pass, 0 failed (roughly `13 + 24 + existing` — exact count varies with Tasks 5-6 additions). The pre-existing positional constructions (`ChatGptCodexProvider("tok", "acct-123", "gpt-5.5", None)` in test_client_timeouts.py:83, test_incomplete_stream.py:81,123, test_chatgpt_codex_stream.py:250) must pass untouched — if any of those fail, the constructor change is wrong; stop and fix.

- [ ] **Step 4: Provider-half gate + commit**

  ```bash
  cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: `All checks passed!`, no format diffs, full suite green. If `ruff format --check` flags the new code, run `uv run ruff format motosan_ai/ tests/` and re-run the gate.

  ```bash
  git add sdks/python/motosan_ai/providers/chatgpt_codex.py sdks/python/tests/test_chatgpt_codex_http.py sdks/python/tests/test_chatgpt_codex_request.py
  git commit -m "feat(python): per-attempt token source for chatgpt_codex

  ChatGptCodexProvider gains token_source (async callable) resolved at the
  top of stream() — the single request site both chat() and stream() share.
  Client retry loops re-enter stream() per attempt, so a fresh token is
  fetched per attempt (F5). access_token is now optional when token_source
  is provided; constructing with neither raises ConfigError.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

- [ ] **Step 5: Write failing Client wiring tests**

  5a. Append to `sdks/python/tests/test_chatgpt_codex_dispatch.py` (file already imports `pytest`, `ChatGptCodexProvider`, `Client`, `Provider`, `ConfigError`):

  ```python
  def test_client_chatgpt_codex_accepts_token_source_without_access_token():
      async def source() -> str:
          return "tok"

      c = Client.chatgpt_codex(account_id="acct-123", token_source=source)
      assert isinstance(c._provider, ChatGptCodexProvider)
      assert c._provider.token_source is source
      assert c._provider.access_token is None


  def test_client_chatgpt_codex_token_source_still_requires_account_id():
      async def source() -> str:
          return "tok"

      with pytest.raises(ConfigError, match="account_id"):
          Client.chatgpt_codex(token_source=source)


  def test_client_chatgpt_codex_error_mentions_token_source():
      with pytest.raises(ConfigError, match="access_token or token_source"):
          Client.chatgpt_codex(account_id="acct-123")
  ```

  Note: the existing `test_client_chatgpt_codex_requires_access_token` (:24-26) uses `match="access_token"`, which the new message `"openai_chatgpt requires access_token or token_source"` still satisfies — it stays green untouched and is NOT on the flip list.

  5b. Append the end-to-end retry proof to `sdks/python/tests/test_chatgpt_codex_http.py` (exercises `Client.stream_with`'s inline attempt loop, client.py:483-531):

  ```python
  @respx.mock
  @pytest.mark.asyncio
  async def test_client_stream_retry_uses_fresh_token_per_attempt():
      from motosan_ai import Client
      from motosan_ai.retry import RetryPolicy

      awaited: list[str] = []

      async def source() -> str:
          tok = f"tok-{len(awaited) + 1}"
          awaited.append(tok)
          return tok

      seen_auth: list[str] = []
      replies = iter(
          [
              httpx.Response(500, json={"error": {"message": "boom"}}),
              httpx.Response(
                  200, text=_text_stream(), headers={"content-type": "text/event-stream"}
              ),
          ]
      )

      def _respond(request: httpx.Request) -> httpx.Response:
          seen_auth.append(request.headers["authorization"])
          return next(replies)

      respx.post(_URL).mock(side_effect=_respond)
      c = Client.chatgpt_codex(
          account_id="acct-123",
          token_source=source,
          retry_policy=RetryPolicy(max_retries=1, base_delay=0.001, jitter=False),
      )
      events = [e async for e in c.stream([{"role": "user", "content": "hi"}])]
      text = "".join(e.content for e in events if not e.done)
      assert text == "Hello world."
      assert awaited == ["tok-1", "tok-2"]
      assert seen_auth == ["Bearer tok-1", "Bearer tok-2"]
  ```

  Run:
  ```bash
  cd sdks/python && uv run pytest tests/test_chatgpt_codex_dispatch.py tests/test_chatgpt_codex_http.py -q -k "token_source"
  ```
  Expected failure (red) — the three dispatch tests and the client stream test all fail with:
  ```
  TypeError: Client.chatgpt_codex() got an unexpected keyword argument 'token_source'
  ```
  (the two provider-level token_source tests from Step 1 stay green).

- [ ] **Step 6: Implement Client wiring (minimal implementation)**

  All edits in `sdks/python/motosan_ai/client.py`.

  6a. Replace line 6:
  ```python
  from collections.abc import AsyncIterator, Iterable
  ```
  with:
  ```python
  from collections.abc import AsyncIterator, Awaitable, Callable, Iterable
  ```

  6b. In `Client.__init__`'s keyword-only section (after `*,` at :74; `reasoning_effort: str | None = None,` is at :75), insert directly after `reasoning_effort`:
  ```python
          token_source: Callable[[], Awaitable[str]] | None = None,
  ```

  6c. Replace the `openai_chatgpt` branch (currently :110-123) in full:
  ```python
          elif provider_value == Provider.openai_chatgpt:
              if not access_token and token_source is None:
                  raise ConfigError("openai_chatgpt requires access_token or token_source")
              if not account_id:
                  raise ConfigError("openai_chatgpt requires account_id")
              self.api_key = ""
              self._provider = ChatGptCodexProvider(
                  access_token=access_token,
                  account_id=account_id,
                  model=model,
                  base_url=base_url,
                  token_source=token_source,
                  connect_timeout=connect_timeout,
                  read_idle_timeout=read_idle_timeout,
              ).reasoning_effort(reasoning_effort)
  ```

  6d. Replace the `chatgpt_codex` classmethod (currently :263-283) in full:
  ```python
      @classmethod
      def chatgpt_codex(
          cls,
          access_token: str | None = None,
          account_id: str | None = None,
          model: str | None = None,
          base_url: str | None = None,
          reasoning_effort: str | None = None,
          max_retries: int = 3,
          retry_policy: RetryPolicy | None = None,
          token_source: Callable[[], Awaitable[str]] | None = None,
      ) -> Client:
          return cls(
              provider=Provider.openai_chatgpt,
              access_token=access_token,
              account_id=account_id,
              model=model,
              base_url=base_url,
              reasoning_effort=reasoning_effort,
              max_retries=max_retries,
              retry_policy=retry_policy,
              token_source=token_source,
          )
  ```

  Run (green):
  ```bash
  cd sdks/python && uv run pytest tests/test_chatgpt_codex_dispatch.py tests/test_chatgpt_codex_http.py -q
  ```
  Expected: all pass, 0 failed (10 dispatch + 14 http at b9bcc3e baseline counts; may be higher after Tasks 5-6).

- [ ] **Step 7: Additive audit (house rules)**

  Run and confirm each:
  ```bash
  cd sdks/python
  # 1. No sync wrappers introduced — token_source is an async callable awaited in-provider:
  grep -n "asyncio.run" motosan_ai/providers/chatgpt_codex.py motosan_ai/client.py
  # expected: no output (exit 1)

  # 2. LlmClient Protocol untouched — it is a structural Protocol defined downstream in
  #    motosan-chat over Client's chat/stream surface; confirm those signatures did not change:
  git diff HEAD~1 -- motosan_ai/client.py | grep -E "^[-+].*(async def chat|async def stream|def chat_with|def stream_with)"
  # expected: no output — only __init__ and the chatgpt_codex classmethod changed

  # 3. BaseProvider ABC untouched:
  git diff HEAD~1 --stat -- motosan_ai/provider_base.py
  # expected: no output
  ```
  If any check produces unexpected output, the change is no longer additive — stop and fix before committing.

- [ ] **Step 8: Full Python gate + commit**

  ```bash
  cd sdks/python && uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: `All checks passed!`, no format diffs, full suite green (0 failed). If format check fails: `uv run ruff format motosan_ai/ tests/`, re-run the gate.

  ```bash
  git add sdks/python/motosan_ai/client.py sdks/python/tests/test_chatgpt_codex_dispatch.py sdks/python/tests/test_chatgpt_codex_http.py
  git commit -m "feat(python): thread token_source through Client.chatgpt_codex

  openai_chatgpt validation relaxes to access_token OR token_source;
  Client.chatgpt_codex() passes the async token source through to
  ChatGptCodexProvider. Additive — LlmClient Protocol surface unchanged.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

---

### Task 8: Python `Provider.claude_code` wiring (F6 — additive; PR-P, last task before the PR opens)

**Branch:** PR-P (run after Task 7). This task is purely additive: it touches `client.py`, one new test file, and docs. It only *reads* `providers/claude_code.py` (which Tasks 5–7 rewrite for F3/F4), so there is no code conflict — but line numbers cited below are from baseline `b9bcc3e`; where a PR-P predecessor may have shifted them, locate the item by name.

**Naming note (pre-verified):** the Python class is `ClaudeCodeClient`, not `ClaudeCodeProvider` (that is the Rust name). It is already exported from `sdks/python/motosan_ai/providers/__init__.py:3,16` and from the package root `sdks/python/motosan_ai/__init__.py:20,68`. Do not rename anything.

**Files:**
- Create: `sdks/python/tests/test_claude_code_dispatch.py` (mirrors `sdks/python/tests/test_codex_cli_dispatch.py:1-33` and `test_gemini_cli_dispatch.py:1-33`)
- Modify: `sdks/python/motosan_ai/client.py` — Provider StrEnum (:34-43), providers import block (:13-22), construction switch (codex_cli arm at :124-128 is the template), `_apply_cli_timeout` annotation (:374-383), new classmethod after `Client.codex_cli` (:303-319)
- Modify: `sdks/python/README.md` (:263 area, Claude Code Backend "Notes" list)
- Modify: `sdks/python/CHANGELOG.md` (Unreleased section)
- Test: `sdks/python/tests/test_claude_code_dispatch.py`

**Interfaces:**
- Consumes (all verified at baseline `b9bcc3e`):
  - `ClaudeCodeClient.__init__(self, binary_path: str | None = None)` — `sdks/python/motosan_ai/providers/claude_code.py:243-246`; falls back to `os.environ.get("CLAUDE_CODE_PATH", "claude")`.
  - `ClaudeCodeClient.timeout(self, secs: float) -> ClaudeCodeClient` / `no_timeout(self) -> ClaudeCodeClient` — `claude_code.py:409-417`; default `_ClaudeCodeConfig.timeout_secs = 300.0` (`claude_code.py:57` — note: 300, not the 600 of codex/gemini CLI).
  - `Client._apply_cli_timeout(cli, cli_timeout)` — `client.py:374-383`; `_UNSET_CLI_TIMEOUT` sentinel — `client.py:31`.
  - `Client.__init__` already accepts `binary_path` (`client.py:70`) and `cli_timeout` (`client.py:85`); `Client.codex_cli` classmethod shape — `client.py:303-319`.
  - Model plumbing: `Client.claude_code(model=...)` sets `self.model` (`client.py:89`), which `_build_request` puts on `ChatRequest.model` (`client.py:411-413`), which `ClaudeCodeClient._build_args` forwards as `--model` (`claude_code.py:461-465`). Same mechanism `Client.codex_cli(model=...)` uses — the classmethod does NOT call the `.model()` builder.
- Produces (the release task documents these; no later code task consumes them):
  - `Provider.claude_code = "claude_code"` StrEnum member.
  - Routing: `Client(provider="claude_code" | Provider.claude_code, binary_path=..., cli_timeout=...)` builds a `ClaudeCodeClient` with no API-key requirement.
  - `Client.claude_code(binary_path: str | None = None, model: str | None = None, max_retries: int = 3, retry_policy: RetryPolicy | None = None, cli_timeout: float | None = _UNSET_CLI_TIMEOUT) -> Client`.

**Scope guard (locked by F6):** the classmethod exposes the *real constructor params* of `ClaudeCodeClient` — which is exactly `binary_path` — plus the same Client-level knobs `codex_cli` exposes (`model`, `max_retries`, `retry_policy`, `cli_timeout`). `permission_mode`, `effort`, `agent_mode`, session flags, etc. are builder methods, not constructor params, and stay reachable via `client._provider.permission_mode(...)` — exactly as `sandbox`/`profile` are for `Client.codex_cli()`. Do not add them to the classmethod.

**Flip list:** (none — F6 is additive; no existing test asserts the Provider member set or claude_code's absence. Verified: `grep -rn "list(Provider)\|__members__\|for p in Provider" sdks/python/tests/` returns nothing at baseline.)

All commands below run from `sdks/python/`.

- [ ] **Step 1: Write the failing enum test**

  Create `sdks/python/tests/test_claude_code_dispatch.py`:

  ```python
  from __future__ import annotations

  from motosan_ai import Client, Provider
  from motosan_ai.providers.claude_code import ClaudeCodeClient


  def test_provider_enum_has_claude_code():
      assert Provider.claude_code == "claude_code"
  ```

  (`Client` and `ClaudeCodeClient` are imported now so later steps only append tests; ruff's F401 does not fire because both are used from Step 3 on — if you run ruff between steps, ignore the transient unused-import warning or add the imports in Step 3 instead.)

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `1 failed` — `AttributeError: claude_code` (StrEnum has no such member).

- [ ] **Step 2: Add the enum member — test passes**

  In `sdks/python/motosan_ai/client.py`, replace the Provider enum (baseline :34-43) with:

  ```python
  class Provider(StrEnum):
      anthropic = "anthropic"
      openai = "openai"
      minimax = "minimax"
      ollama = "ollama"
      gemini = "gemini"
      claude_code = "claude_code"
      codex_cli = "codex_cli"
      gemini_cli = "gemini_cli"
      gemini_code_assist = "gemini_code_assist"
      openai_chatgpt = "openai_chatgpt"
  ```

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `1 passed`.

- [ ] **Step 3: Write the failing routing tests**

  Append to `sdks/python/tests/test_claude_code_dispatch.py`:

  ```python
  def test_claude_code_does_not_require_api_key(monkeypatch):
      for env in ("ANTHROPIC_API_KEY", "CLAUDE_CODE_PATH"):
          monkeypatch.delenv(env, raising=False)
      client = Client(provider=Provider.claude_code)
      assert isinstance(client._provider, ClaudeCodeClient)
      assert client.api_key == ""


  def test_claude_code_routing_with_explicit_binary_path():
      client = Client(provider="claude_code", binary_path="/opt/claude")
      assert isinstance(client._provider, ClaudeCodeClient)
      assert client._provider._config.binary_path == "/opt/claude"


  def test_claude_code_path_env_var_resolved(monkeypatch):
      monkeypatch.setenv("CLAUDE_CODE_PATH", "/env/claude")
      client = Client(provider=Provider.claude_code)
      assert client._provider._config.binary_path == "/env/claude"


  def test_claude_code_routing_cli_timeout_passthrough():
      client = Client(provider=Provider.claude_code, cli_timeout=5.0)
      assert client._provider._config.timeout_secs == 5.0


  def test_claude_code_routing_cli_timeout_none_disables():
      client = Client(provider=Provider.claude_code, cli_timeout=None)
      assert client._provider._config.timeout_secs is None


  def test_claude_code_routing_default_timeout_preserved():
      client = Client(provider=Provider.claude_code)
      assert client._provider._config.timeout_secs == 300.0
  ```

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `6 failed, 1 passed`. Every new test fails with `KeyError: <Provider.claude_code: 'claude_code'>` raised from `Client._load_api_key`'s `env_map[provider]` lookup (`client.py:372` baseline) — with no routing arm, `claude_code` falls into the API-key `else` branch (`client.py:156-159`).

- [ ] **Step 4: Implement the routing arm — routing tests pass**

  Three edits to `sdks/python/motosan_ai/client.py`:

  4a. Providers import block (baseline :13-22) — add `ClaudeCodeClient` in isort order (ruff enforces `I`):

  ```python
  from motosan_ai.providers import (
      AnthropicProvider,
      ChatGptCodexProvider,
      ClaudeCodeClient,
      CodexCliClient,
      GeminiCliClient,
      GeminiCodeAssistProvider,
      GeminiProvider,
      MinimaxProvider,
      OpenAIProvider,
  )
  ```

  4b. Construction switch — insert directly ABOVE the `elif provider_value == Provider.codex_cli:` arm (baseline :124), grouping the three CLI arms:

  ```python
          elif provider_value == Provider.claude_code:
              self.api_key = ""
              self._provider = self._apply_cli_timeout(
                  ClaudeCodeClient(binary_path=binary_path), cli_timeout
              )
  ```

  4c. Widen `_apply_cli_timeout` (baseline :374-383) — `ClaudeCodeClient` has the same `timeout()`/`no_timeout()` surface, only the annotation needs it:

  ```python
      @staticmethod
      def _apply_cli_timeout(
          cli: ClaudeCodeClient | CodexCliClient | GeminiCliClient,
          cli_timeout: float | None,
      ) -> ClaudeCodeClient | CodexCliClient | GeminiCliClient:
          if cli_timeout is _UNSET_CLI_TIMEOUT:
              return cli
          if cli_timeout is None:
              return cli.no_timeout()
          return cli.timeout(cli_timeout)
  ```

  (Do NOT touch `_load_api_key`'s `env_map` — `claude_code` never reaches it, same as `codex_cli`/`gemini_cli`.)

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `7 passed`.

- [ ] **Step 5: Write the failing classmethod tests**

  Append to `sdks/python/tests/test_claude_code_dispatch.py`:

  ```python
  def test_client_claude_code_classmethod_resolves_to_provider():
      client = Client.claude_code()
      assert client.provider == Provider.claude_code
      assert isinstance(client._provider, ClaudeCodeClient)


  def test_client_claude_code_classmethod_params_pass_through():
      client = Client.claude_code(binary_path="/opt/claude", model="sonnet", cli_timeout=7.5)
      assert client._provider._config.binary_path == "/opt/claude"
      assert client.model == "sonnet"
      assert client._provider._config.timeout_secs == 7.5
  ```

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `2 failed, 7 passed` — both new tests fail with `AttributeError: type object 'Client' has no attribute 'claude_code'`.

- [ ] **Step 6: Implement `Client.claude_code()` — all 9 tests pass**

  In `sdks/python/motosan_ai/client.py`, insert directly AFTER the `codex_cli` classmethod (baseline :303-319, i.e. between `codex_cli` and `gemini_cli`), mirroring its exact shape:

  ```python
      @classmethod
      def claude_code(
          cls,
          binary_path: str | None = None,
          model: str | None = None,
          max_retries: int = 3,
          retry_policy: RetryPolicy | None = None,
          cli_timeout: float | None = _UNSET_CLI_TIMEOUT,
      ) -> Client:
          return cls(
              provider=Provider.claude_code,
              binary_path=binary_path,
              model=model,
              max_retries=max_retries,
              retry_policy=retry_policy,
              cli_timeout=cli_timeout,
          )
  ```

  Run:
  ```bash
  uv run pytest tests/test_claude_code_dispatch.py -q
  ```
  Expected: `9 passed`.

- [ ] **Step 7: Provider-exhaustiveness sweep + docs**

  7a. Confirm no exhaustive Provider-list assertion exists (none at baseline; a PR-P predecessor could in theory have added one):
  ```bash
  grep -rn "list(Provider)\|__members__\|for p in Provider" tests/ motosan_ai/
  ```
  Expected: no output. If a hit appears, add `claude_code` to that assertion's expected member list — that is the only permitted edit outside this task's file list.

  7b. `sdks/python/README.md` — in the Claude Code Backend "Notes" list, insert directly after the line `- Uses \`CLAUDE_CODE_PATH\` env var or \`claude\` in \`PATH\`.` (baseline :263), mirroring the Codex bullet at :299:

  ```markdown
  - Available through both direct `ClaudeCodeClient()` and unified `Client.claude_code()` / `Provider.claude_code` dispatch (`binary_path`, `model`, `cli_timeout`; richer flags via the `ClaudeCodeClient` builder).
  ```

  7c. `sdks/python/CHANGELOG.md` — if Tasks 5–7 already created a `## [Unreleased]` section, append this bullet under its `### Added` heading (create the `### Added` subsection if absent); otherwise insert this whole block between line 3 and the `## [0.17.0]` heading:

  ```markdown
  ## [Unreleased]

  ### Added
  - `Provider.claude_code` + `Client.claude_code(binary_path=None, model=None, max_retries=3, retry_policy=None, cli_timeout=...)`: the `claude` CLI backend is now reachable through unified `Client` dispatch (previously direct `ClaudeCodeClient()` only). `binary_path` falls back to `CLAUDE_CODE_PATH` then `claude` in `PATH`; `cli_timeout` threads to `.timeout()` / `.no_timeout()` exactly like `Client.codex_cli()` (claude default remains 300s). Richer knobs (`permission_mode`, `effort`, `agent_mode`, session flags, ...) remain builder methods on the underlying `ClaudeCodeClient`.
  ```

  (Do not touch `llms.txt` / `AGENTS.md` / version numbers here — the F7 release task owns them.)

- [ ] **Step 8: Python gate + commit**

  ```bash
  uv run ruff format motosan_ai/ tests/
  uv run ruff check motosan_ai/
  uv run ruff format --check motosan_ai/ tests/
  uv run pytest tests/ -q --ignore=tests/integration
  ```
  Expected: `ruff format` reports `2 files reformatted` or `N files left unchanged`; `ruff check` → `All checks passed!`; `format --check` → `N files already formatted`; pytest → all passed, 0 failed (count varies with Tasks 5–7).

  Commit (from `sdks/python/`):
  ```bash
  git add motosan_ai/client.py tests/test_claude_code_dispatch.py README.md CHANGELOG.md
  git commit -m "$(cat <<'EOF'
  feat(python): claude_code provider routing

  Provider.claude_code StrEnum member, construction-switch arm building
  ClaudeCodeClient (CLAUDE_CODE_PATH fallback, cli_timeout threading, no
  API key required), and Client.claude_code() classmethod mirroring
  Client.codex_cli(). F6: closes the last Provider-dispatch gap — the
  23.7K providers/claude_code.py implementation was previously reachable
  only by direct construction.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  ```
  Expected: commit created on branch PR-P; `git show --stat HEAD` lists exactly the 4 files above. PR-P is now complete and ready to open.

---

### Task 9: TypeScript token source for chatgpt_codex (PR-T; additive, minor)

Applies F5 to the TypeScript SDK: `ChatGptCodexProvider`'s `accessToken` widens from `string` to `string | (() => Promise<string>)`, resolved once per request ATTEMPT so a retry picks up a refreshed OAuth token. Additive-only — plain-string construction is untouched (TS 0.15.0 minor per F7).

**Branch context:** PR group PR-T, branch `feat/m4-ts-token-source` off `origin/main`. PREREQ: PR-S merged (verify with `git log origin/main --oneline -5` before branching). No Rust/Python files are touched in this task.

**Files:**
- Modify: `sdks/typescript/src/providers/chatgpt_codex.ts` — constructor `accessToken: string` at :65-66; `headers()` builds `authorization: Bearer ${this.accessToken}` at :101-110 (Bearer at :103); headers built ONCE at :225 before the retry loop; `withRetry(...)` attempt closure at :229-239 (M2 TS retry engine: `withRetry` src/retry.ts:104-137, `attemptWithCancellation` src/retry.ts:172-184, `classifyForRetry` src/retry.ts:147; `postStream(url, headers, body, options?)` src/http/fetch.ts:76-81)
- Modify: `sdks/typescript/src/client.ts` — import at :20; `_chatgptAccessToken?: string` field at :104; `chatgptCodex(accessToken: string, ...)` at :217-229; `buildProvider` chatgpt_codex arm at :294-303 (`this._chatgptAccessToken ?? ''` stays valid after widening — no code change in that arm)
- Modify: `sdks/typescript/src/index.ts` — explicit chatgpt_codex export block at :11-13 (helper types live in their defining module today: `RetryEvent`/`RetryPolicyOptions` in retry.ts re-exported at index.ts:16, `OpenAIAuthStyle` in providers/openai.ts — so `TokenSource` is defined in the provider module and type-re-exported from index.ts)
- Modify: `sdks/typescript/CHANGELOG.md` — new `## [Unreleased]` section above `## [0.14.0] - 2026-07-17`
- Test: `sdks/typescript/tests/providers-chatgpt-codex.test.ts` (append; fetch-mock pattern per this file's `streamFromTranscript` :196-217 and `retry-integration.test.ts` `let calls = 0` + `vi.stubGlobal('fetch', vi.fn(...))` counting at :357-380)
- Test: `sdks/typescript/tests/client-builder.test.ts` (append after the `ClientBuilder.chatgptCodex` describe at :563-586)
- Verify-only (NO change): `sdks/typescript/package.json` — engines floor stays `"node": ">=20.3"` (:15-17); scripts `typecheck`/`build`/`test` (:23-28)

**Interfaces:**
- Consumes: `withRetry(policy: RetryPolicy, op: (attempt: number) => Promise<T>, classify): Promise<T>` (src/retry.ts:104); `attemptWithCancellation(callerSignal: AbortSignal | undefined, op: () => Promise<T>): Promise<T>` (src/retry.ts:172); `classifyForRetry(errOrStatus: unknown): RetryClassification` (src/retry.ts:147); `postStream(url: string, headers: Record<string, string>, body: unknown, options?: FetchOptions): Promise<ReadableStream<Uint8Array>>` (src/http/fetch.ts:76); `isRetryableStatus` treats 500/503 as retryable (src/error.ts:100-102). No dependency on any other M4 task's code (Tasks 1-8 are Rust/Python/spec).
- Produces: `export type TokenSource = () => Promise<string>` (defined in `src/providers/chatgpt_codex.ts`, type-re-exported from `src/index.ts`); widened `ChatGptCodexProvider` constructor `(accessToken: string | TokenSource, accountId: string, model?: string, baseUrl?: string)`; widened `ClientBuilder.chatgptCodex(accessToken: string | TokenSource, accountId: string, model?: string, opts?: { reasoningEffort?: string }): this`; CHANGELOG `[Unreleased]` entry consumed by the F7 release task (TS 0.15.0, minor).

**Flip list:** none — F5 is additive. No existing test changes behavior (all existing chatgpt_codex tests construct with a plain string; the per-attempt header rebuild produces identical headers for a static string).

Note on failing-test signals: `tsconfig.json` `include` is `["src/**/*.ts"]` only (tsconfig.json:13) and vitest transforms tests with esbuild (no typechecking), so the failing tests below fail on RUNTIME assertions, not compile errors. `npm run typecheck` covers `src/` only.

- [ ] **Step 1: Create the branch**

  ```bash
  cd <repo-root> && git fetch origin && git log origin/main --oneline -5   # confirm the PR-S merge commit is present
  git checkout -b feat/m4-ts-token-source origin/main
  ```

  Expected: `Switched to a new branch 'feat/m4-ts-token-source'`.

- [ ] **Step 2: Write the failing per-attempt provider test**

  Append to `sdks/typescript/tests/providers-chatgpt-codex.test.ts` (module-scope `REQ` at :219 and the existing imports at :1-9 — `vi`, `afterEach`, `RetryPolicy`, `ChatGptCodexProvider` — are already in scope):

  ```ts
  // ---------------------------------------------------------------------------
  // F5 — per-attempt TokenSource resolution
  // ---------------------------------------------------------------------------

  describe('ChatGptCodexProvider token source (F5)', () => {
    afterEach(() => vi.unstubAllGlobals())

    const OK_SSE = 'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    function immediateRetry(): RetryPolicy {
      return new RetryPolicy({
        maxRetries: 2,
        baseDelayMs: 0,
        maxDelayMs: 0,
        jitter: false,
        respectRetryAfter: false,
      })
    }

    /** Stub fetch: 500 on the first call, SSE 200 on the second. Records each
     *  attempt's authorization header; returns a live fetch-call counter. */
    function fetch500Then200(authHeaders: string[]): () => number {
      let fetches = 0
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_url: string, options?: RequestInit) => {
          fetches += 1
          const headers = (options?.headers as Record<string, string>) ?? {}
          authHeaders.push(headers.authorization ?? '')
          return fetches === 1
            ? new Response(JSON.stringify({ error: { message: 'overloaded' } }), { status: 500 })
            : new Response(OK_SSE, {
                status: 200,
                headers: { 'content-type': 'text/event-stream' },
              })
        }),
      )
      return () => fetches
    }

    it('consults the token source once per attempt: 500-then-200 sends Bearer tok-2 on the retry', async () => {
      let tokenCalls = 0
      const source = async (): Promise<string> => {
        tokenCalls += 1
        return `tok-${tokenCalls}`
      }
      const authHeaders: string[] = []
      const fetchCount = fetch500Then200(authHeaders)

      const prov = new ChatGptCodexProvider(source, 'acct').withRetryPolicy(immediateRetry())
      for await (const _ of prov.stream(REQ)) {
        /* drain */
      }

      expect(fetchCount()).toBe(2)
      expect(tokenCalls).toBe(2)
      expect(authHeaders).toEqual(['Bearer tok-1', 'Bearer tok-2'])
    })
  })
  ```

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npx vitest run tests/providers-chatgpt-codex.test.ts
  ```

  Expected failure (the constructor stores the function; `Bearer ${fn}` stringifies the arrow source and the source is never called; both fetches happen since 500 is retryable per error.ts:100-102):

  ```
  AssertionError: expected +0 to be 2 // Object.is equality
   ❯ tests/providers-chatgpt-codex.test.ts   (at the `expect(tokenCalls).toBe(2)` line)
  Tests  1 failed | 39 passed
  ```

  (Editor/tsc would also flag `new ChatGptCodexProvider(source, …)` as a type error until Step 4 — expected; vitest does not typecheck.)

- [ ] **Step 3: Write the failing builder-path test**

  Append to `sdks/typescript/tests/client-builder.test.ts` after the `ClientBuilder.chatgptCodex` describe (ends :586; `vi`, `afterEach`, `ClientBuilder`, `RetryPolicy` already imported at :1-7):

  ```ts
  describe('ClientBuilder.chatgptCodex token source (F5)', () => {
    afterEach(() => vi.unstubAllGlobals())

    it('accepts an async token source and resolves it per attempt through Client.chat()', async () => {
      let tokenCalls = 0
      const source = async (): Promise<string> => {
        tokenCalls += 1
        return `tok-${tokenCalls}`
      }
      const authHeaders: string[] = []
      let fetches = 0
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_url: string, options?: RequestInit) => {
          fetches += 1
          const headers = (options?.headers as Record<string, string>) ?? {}
          authHeaders.push(headers.authorization ?? '')
          return fetches === 1
            ? new Response(JSON.stringify({ error: { message: 'overloaded' } }), { status: 500 })
            : new Response(
                'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
                { status: 200, headers: { 'content-type': 'text/event-stream' } },
              )
        }),
      )

      const client = new ClientBuilder()
        .chatgptCodex(source, 'acct')
        .retryPolicy(
          new RetryPolicy({
            maxRetries: 2,
            baseDelayMs: 0,
            maxDelayMs: 0,
            jitter: false,
            respectRetryAfter: false,
          }),
        )
        .build()
      const resp = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

      expect(resp.stopReason).toBe('end_turn')
      expect(fetches).toBe(2)
      expect(tokenCalls).toBe(2)
      expect(authHeaders).toEqual(['Bearer tok-1', 'Bearer tok-2'])
    })
  })
  ```

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npx vitest run tests/client-builder.test.ts
  ```

  Expected failure (same mechanism as Step 2):

  ```
  AssertionError: expected +0 to be 2 // Object.is equality
   ❯ tests/client-builder.test.ts   (at the `expect(tokenCalls).toBe(2)` line)
  ```

- [ ] **Step 4: Implement per-attempt token resolution in the provider**

  Four edits in `sdks/typescript/src/providers/chatgpt_codex.ts`.

  4a. Replace the class doc comment (:54-58) and insert the `TokenSource` type directly above it:

  ```ts
  /**
   * A caller-supplied async bearer-token source, consulted once per request
   * ATTEMPT (each retry re-resolves it, so a refreshed OAuth access token is
   * picked up mid-retry). TS mirror of Rust `motosan_ai::auth::TokenSource` (F5).
   */
  export type TokenSource = () => Promise<string>

  /**
   * No-api-key OAuth-Bearer HTTP provider over the OpenAI Responses API.
   * Constructor `(accessToken, accountId, model?, baseUrl?)` mirrors Python
   * `ChatGptCodexProvider.__init__`; `accessToken` is a static string or an
   * async `TokenSource` resolved once per attempt (F5). Text-only capabilities.
   */
  ```

  4b. Widen the constructor param (:65-70 → the only change is line :66):

  ```ts
    constructor(
      private readonly accessToken: string | TokenSource,
      private readonly accountId: string,
      model?: string,
      baseUrl: string = DEFAULT_CHATGPT_CODEX_URL,
    ) {
  ```

  4c. Replace `private headers(): Record<string, string>` (:101-110) with a token-parameterized build plus the resolver:

  ```ts
    /** Resolve the bearer token for ONE attempt: static string, or one TokenSource call. */
    private async resolveToken(): Promise<string> {
      return typeof this.accessToken === 'function' ? this.accessToken() : this.accessToken
    }

    private headers(token: string): Record<string, string> {
      return {
        authorization: `Bearer ${token}`,
        'chatgpt-account-id': this.accountId,
        originator: CHATGPT_CODEX_ORIGINATOR,
        'openai-beta': 'responses=experimental',
        accept: 'text/event-stream',
        'content-type': 'application/json',
      }
    }
  ```

  4d. In `streamImpl` (:223-239): DELETE the pre-loop `const headers = this.headers()` (:225) and rebuild headers inside the attempt closure:

  ```ts
      const model = request.model ?? this.model
      const body = this.buildResponsesBody(request, model)

      // Retry ONLY the initial fetch via the shared engine (same guard as the
      // other providers: nothing is retried after the first emitted event).
      // F5: headers are rebuilt inside the attempt closure so a TokenSource is
      // re-resolved on EVERY attempt (fresh OAuth token mid-retry). The single
      // M2 retry engine (withRetry) is untouched — onRetry still fires only there.
      const responseBody = await withRetry(
        this.retryPolicy,
        async () =>
          attemptWithCancellation(opts?.callerSignal, async () =>
            postStream(this.baseUrl, this.headers(await this.resolveToken()), body, {
              signal: opts?.signal,
              preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
            }),
          ),
        classifyForRetry,
      )
  ```

  (A throwing `TokenSource` propagates through `attemptWithCancellation` → `classifyForRetry`, which returns `{ retryable: false }` for a plain Error with no `.status` (retry.ts:147-165) — token-source failures are NOT retried. `body` stays built once: it does not depend on the token.)

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npx vitest run tests/providers-chatgpt-codex.test.ts && npm run typecheck
  ```

  Expected: `Tests  40 passed` (Step 2's test now green; all pre-existing string-token tests untouched), typecheck exits 0. The Step 3 builder test also passes at runtime now (the builder forwards the value opaquely) — its public TYPE is still wrong until Step 5.

- [ ] **Step 5: Widen the ClientBuilder config type**

  Three edits in `sdks/typescript/src/client.ts`.

  5a. Import the type (:20):

  ```ts
  import { ChatGptCodexProvider, type TokenSource } from './providers/chatgpt_codex.js'
  ```

  5b. Widen the field (:104):

  ```ts
    protected _chatgptAccessToken?: string | TokenSource
  ```

  5c. Widen `chatgptCodex` and its doc (:211-229):

  ```ts
    /**
     * Configure the no-api-key ChatGPT-Codex provider with a caller-supplied OAuth
     * `accessToken` + `accountId`. `accessToken` is a static string or an async
     * `TokenSource` (`() => Promise<string>`) resolved once per request attempt
     * (F5 — a retry picks up a refreshed token). Optional `model` overrides the
     * default (`gpt-5.5`); `opts.reasoningEffort` sets the provider-default
     * reasoning effort (per-request `providerOptions.reasoning_effort` still wins).
     */
    chatgptCodex(
      accessToken: string | TokenSource,
      accountId: string,
      model?: string,
      opts?: { reasoningEffort?: string },
    ): this {
      this._provider = 'chatgpt_codex'
      this._chatgptAccessToken = accessToken
      this._chatgptAccountId = accountId
      if (model !== undefined) this._model = model
      this._chatgptReasoningEffort = opts?.reasoningEffort
      return this
    }
  ```

  The `buildProvider` arm (:294-303) needs NO edit: `this._chatgptAccessToken ?? ''` now yields `string | TokenSource`, which the Step 4 constructor accepts.

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npm run typecheck && npx vitest run tests/client-builder.test.ts && npm run build && grep -n "accessToken" dist/client.d.ts
  ```

  Expected: typecheck exits 0; builder test file all green (Step 3's test passing); grep shows the widened public signature, e.g. `chatgptCodex(accessToken: string | TokenSource, accountId: string, model?: string, opts?: { reasoningEffort?: string; }): this;`.

- [ ] **Step 6: Add the string-compat regression test**

  Append inside the Step 2 describe (`ChatGptCodexProvider token source (F5)`), reusing its helpers:

  ```ts
    it('a plain string token still works unchanged, on every attempt (compat)', async () => {
      const authHeaders: string[] = []
      const fetchCount = fetch500Then200(authHeaders)

      const prov = new ChatGptCodexProvider('static-tok', 'acct').withRetryPolicy(immediateRetry())
      const resp = await prov.chat(REQ)

      expect(resp.stopReason).toBe('end_turn')
      expect(fetchCount()).toBe(2)
      expect(authHeaders).toEqual(['Bearer static-tok', 'Bearer static-tok'])
    })
  ```

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npx vitest run tests/providers-chatgpt-codex.test.ts
  ```

  Expected: `Tests  41 passed` (passes immediately — this is the guard that the string path and its chat() delegation are byte-identical per attempt).

- [ ] **Step 7: Export `TokenSource` from the package root**

  Replace the chatgpt_codex export block in `sdks/typescript/src/index.ts` (:11-13):

  ```ts
  // chatgpt_codex: exports ChatGptCodexProvider + DEFAULT_CHATGPT_CODEX_URL +
  // TokenSource (F5). `chatGptCodexErrorMessage` is @internal and NOT re-exported.
  export { ChatGptCodexProvider, DEFAULT_CHATGPT_CODEX_URL } from './providers/chatgpt_codex.js'
  export type { TokenSource } from './providers/chatgpt_codex.js'
  ```

  Run:

  ```bash
  cd <repo-root>/sdks/typescript && npm run build && grep -n "TokenSource" dist/index.d.ts
  ```

  Expected: one line, `export type { TokenSource } from './providers/chatgpt_codex.js';`.

- [ ] **Step 7b: Fix the stale thinking-emitters comment in types.ts**

  `sdks/typescript/src/types.ts:115-116` claims `thinking_delta`/`thinking_done` are "emitted by Anthropic only", but `chatgpt_codex.ts` also emits `thinking_delta` (reasoning deltas — verify with `grep -n "thinking_delta" src/providers/chatgpt_codex.ts`, expect a hit near :265). This contradicts the M4 spec Emitters table (`specs/types.md`, PR-S). Replace the doc comment:

  ```ts
  /**
   * The kind of a streaming event. `thinking_delta`/`thinking_done` are emitted
   * by Anthropic only; `collectStream` concatenates them into `ChatResponse.thinking`.
   */
  ```

  with:

  ```ts
  /**
   * The kind of a streaming event. Anthropic emits `thinking_delta` and
   * `thinking_done`; ChatGPT Codex emits `thinking_delta` (reasoning deltas).
   * `collectStream` concatenates them into `ChatResponse.thinking`.
   */
  ```

  Run: `npm run typecheck` — expected: clean (comment-only change).

- [ ] **Step 8: CHANGELOG entry**

  In `sdks/typescript/CHANGELOG.md`, insert directly above the `## [0.14.0] - 2026-07-17` heading:

  ```md
  ## [Unreleased]

  ### Added
  - `ChatGptCodexProvider` and `ClientBuilder.chatgptCodex` accept an async token
    source: `accessToken: string | TokenSource` where `TokenSource = () => Promise<string>`
    (exported from the package root). The source is resolved once per request
    ATTEMPT, so a retry after 5xx/429 sends a freshly resolved Bearer token.
    Plain-string tokens are unchanged. (F5)
  ```

  No version bump here — the F7 release task ships TS 0.15.0.

- [ ] **Step 9: Full TS gate, engines check, commit**

  ```bash
  cd <repo-root>/sdks/typescript && grep -n '"node"' package.json
  ```

  Expected (verify-only, MUST be unchanged): `16:    "node": ">=20.3"`.

  ```bash
  cd <repo-root>/sdks/typescript && npm run typecheck && npm run build && npm test
  ```

  Build BEFORE test — `tests/pack-smoke.test.ts` asserts `dist/index.js` + `dist/index.d.ts` exist from a prior build. Expected: typecheck and build exit 0; vitest reports all test files passed, 0 failed (3 new tests added by this task).

  ```bash
  cd <repo-root> && git add sdks/typescript/src/providers/chatgpt_codex.ts sdks/typescript/src/client.ts sdks/typescript/src/index.ts sdks/typescript/CHANGELOG.md sdks/typescript/tests/providers-chatgpt-codex.test.ts sdks/typescript/tests/client-builder.test.ts
  git commit -m "feat(ts): per-attempt token source for chatgpt_codex

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
  ```

  Expected: clean commit on `feat/m4-ts-token-source`; `git status --short` shows no unstaged SDK files. Open as PR-T against main (do not merge before PR-S).

---

### Task 10: M4 Release Prep — PR-REL (versions, changelogs, docs; NO tag, NO publish)

Applies **F7**. Final M4 task. **PREREQ: every other M4 PR (Tasks 1–9) is merged to `main`.** Branch: `chore/m4-release` off `origin/main`. This PR bumps Rust `0.24.0 → 0.25.0`, Python `0.17.0 → 0.18.0`, TypeScript `0.14.0 → 0.15.0`, writes all changelog entries, and sweeps every doc site that carries a version string or a now-false feature/behavior claim. **This PR contains NO git tag and NO publish step** — publishing is tag-triggered CI only (`publish-rust.yml` on `rust-v*`, `publish-python.yml` on `python-v*`, `publish-typescript.yml` on `ts-v*`; verified at `.github/workflows/`), and tags are pushed post-merge by the maintainer flow (AGENTS.md:103–105, llms.txt § Release).

**Files:**
- Modify: `sdks/rust/Cargo.toml:3` (`version = "0.24.0"`)
- Modify: `sdks/python/pyproject.toml:3` (`version = "0.17.0"`)
- Modify: `sdks/typescript/package.json:3` (`"version": "0.14.0"`) + `sdks/typescript/package-lock.json:3,9` (via `npm install --package-lock-only`)
- Modify: `uv.lock:96–99` (`name = "motosan-ai"` / `version = "0.17.0"`, regenerated via `uv lock`)
- Modify: `CHANGELOG.md` (new M4 entry inserted above the M3 heading at line 5)
- Modify: `sdks/rust/CHANGELOG.md` (new `[0.25.0]` section between `## [Unreleased]` at line 5 and `## [0.24.0]` at line 7)
- Modify: `sdks/python/CHANGELOG.md` (new `[0.18.0]` section above `## [0.17.0]` at line 5; this file has no `[Unreleased]` section)
- Modify: `sdks/typescript/CHANGELOG.md` (new `[0.15.0]` section above `## [0.14.0]` at line 7)
- Modify: `AGENTS.md:5` (version line) + new M4 paragraph after the M3 paragraph at line 15
- Modify: `llms.txt:5` (version bullet), new M4 bullet after line 8, `llms.txt:25` (install snippet), `llms.txt:26–28` (feature-list comment), `llms.txt:928` (TS tag example `ts-v0.14.0` in the § Release tag table)
- Modify: `skills/motosan-ai/SKILL.md:8` (header version line), `:25` (install snippet), `:26–27` (feature-list comment), `:139` (CLI-backends bullet — "Blocking `chat().tool_calls` is empty on all three" is false after F4)
- Modify: `skills/motosan-ai/references/rust-api.md:6–8` (version + feature-list comment)
- Modify: `README.md:29–31` (Languages table), `:38–40` (install snippet + feature comment), `:204` (CLI-backend limitations note — false after F4)
- Modify: `sdks/rust/README.md:103–115` (`## Features` list), `:323`, `:433`, `:497` (`version = "0.24.0"` snippets), and — only if Tasks 4–6 did not already fix them — `:423`, `:486`, `:537` ("`ChatResponse.tool_calls` is always empty" claims, false after F4)
- Modify: `sdks/typescript/README.md:486–487` (tag example `git tag ts-v0.14.0`)
- Test: none created — this task touches no source or test file; the gate is the full existing suite (`check-all` + TS typecheck/build/test)

All line numbers verified at `origin/main` @ `b9bcc3e` (pre-Tasks-1–9). Tasks 1–9 may shift them — **locate each site by the quoted text (greps are given per step), not by line number.**

**Interfaces:**
- Consumes (names shipped by earlier tasks; verify each exists in Step 1 before proceeding):
  - F1 (Task 1): Cargo features `_http`, `_cli`, `ollama-native` in `sdks/rust/Cargo.toml`; module `sdks/rust/src/transport/http.rs`; `cargo hack check --each-feature` in `.github/workflows/ci-rust.yml`
  - F3: `StreamEventType.thinking_delta` / `StreamEventType.thinking_done` in `sdks/python/motosan_ai/types.py`
  - F4: CLI `end_turn` terminal contract + `chat()`-delegates-to-`stream()` in Rust `claude_code`/`codex_cli`/`gemini_cli` and Python `claude_code.py`/`codex_cli.py`/`gemini_cli.py`
  - F5: `motosan_ai::auth::TokenSource` + `StaticTokenSource` (`sdks/rust/src/auth.rs`), `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)`, Python `token_source: Callable[[], Awaitable[str]] | None`, TS `accessToken: string | (() => Promise<string>)`
  - F6: `Provider.claude_code` + `Client.claude_code(...)` in `sdks/python/motosan_ai/client.py`
- Produces: versions `rust-0.25.0` / `python-0.18.0` / `ts-0.15.0` in manifests + all docs; commit `chore(release): M4 — Rust 0.25.0 / Python 0.18.0 / TS 0.15.0`; PR-REL on branch `chore/m4-release`. No later task consumes anything — this is the last task. The post-merge maintainer flow (NOT this PR) pushes tags `rust-v0.25.0`, `python-v0.18.0`, `ts-v0.15.0`.

**Flip list:** none. This task modifies no test and no source file. (Grep-verified: no test asserts the package version — the only `0.24.0` hits under `sdks/rust/src|tests` are the `#[deprecated(since = "0.24.0")]` attribute at `sdks/rust/src/client.rs:900` and two comments in `sdks/rust/tests/client_builder.rs:121,165`, all historical and untouched.)

**Verified baseline evidence (origin/main @ b9bcc3e):**
- `sdks/rust/Cargo.toml:3` = `version = "0.24.0"`; `sdks/python/pyproject.toml:3` = `version = "0.17.0"`; `sdks/typescript/package.json:3` = `"version": "0.14.0"`; `uv.lock:98` = `version = "0.17.0"` under `name = "motosan-ai"` (`source = { editable = "sdks/python" }`).
- Root `CHANGELOG.md:5` M3 heading format: `## [rust-0.24.0 / python-0.17.0 / ts-0.14.0] — 2026-07-17` with an intro sentence, then `### Breaking` / `### Added` / `### Changed` / `### Fixed` sections and a trailing "Per-SDK detail:" links line — the M4 entry below mirrors this structure and voice exactly.
- Per-SDK changelog heading formats: Rust `## [0.24.0] - 2026-07-17` under a `## [Unreleased]` stub (line 5); Python `## [0.17.0] - 2026-07-17` (no Unreleased stub); TS `## [0.14.0] - 2026-07-17` (Keep-a-Changelog note at line 5).
- The M3 release commit `b9bcc3e` touched exactly: `AGENTS.md`, `CHANGELOG.md`, `README.md`, `llms.txt`, per-SDK CHANGELOGs, `pyproject.toml`, `Cargo.toml`, `sdks/rust/README.md`, `sdks/typescript/{CHANGELOG.md,README.md,package.json,package-lock.json}`, `skills/motosan-ai/SKILL.md`, `skills/motosan-ai/references/rust-api.md`, `uv.lock` — this task mirrors that file set.
- `sdks/rust/src/lib.rs` has **no crate docstring**: `grep -c '^//!' sdks/rust/src/lib.rs` = 0 and it enumerates no features — nothing to update there unless Task 1 added one (checked in Step 14).
- Publish workflows exist: `.github/workflows/publish-rust.yml` (trigger `tags: ["rust-v*"]`, runs fmt/clippy/test then `cargo publish`), `publish-python.yml` (`tags: ["python-v*"]`, `uv build` + PyPI), `publish-typescript.yml` (`tags: ["ts-v*"]`, `npm ci`/build/test, verifies tag == package.json version, `npm publish --provenance`). Three additional oauth-crate publish workflows exist and are untouched by M4.
- AGENTS.md:103: "Tag `rust-vX.Y.Z` triggers `publish-rust.yml` → crates.io. Tag `python-vX.Y.Z` triggers `publish-python.yml` → PyPI. Tag `ts-vX.Y.Z` triggers `publish-typescript.yml` → npm." AGENTS.md:105: "Update before tagging: CHANGELOGs, version in `Cargo.toml`/`pyproject.toml`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`."

---

- [ ] **Step 1: Preflight — verify all M4 PRs are merged, then cut the branch**

  ```bash
  cd /path/to/motosan-ai && git fetch origin && git checkout main && git pull origin main
  # F1 landmarks:
  grep -n '^_http' sdks/rust/Cargo.toml && grep -n '^ollama-native' sdks/rust/Cargo.toml && test -f sdks/rust/src/transport/http.rs && echo F1-OK
  grep -n 'cargo hack check --each-feature' .github/workflows/ci-rust.yml
  # F3 landmark:
  grep -n 'thinking_delta = "thinking_delta"' sdks/python/motosan_ai/types.py
  # F4 landmark (no CLI backend ever reports ToolUse as its own stop reason):
  grep -rn 'done_with_stop_reason' sdks/rust/src/providers/claude_code/ | head -3
  # F5 landmarks:
  grep -n 'pub trait TokenSource' sdks/rust/src/auth.rs && grep -n 'chatgpt_codex_token_source' sdks/rust/src/client.rs
  grep -n 'token_source' sdks/python/motosan_ai/providers/chatgpt_codex.py | head -2
  grep -n 'Promise<string>' sdks/typescript/src/providers/chatgpt_codex.ts | head -2
  # F6 landmark:
  grep -n 'claude_code = "claude_code"' sdks/python/motosan_ai/client.py
  ```
  Expected: every grep prints at least one match and `F1-OK` appears. **If any grep is empty, STOP — the prerequisite PR is not merged; do not start the release branch.**
  Then:
  ```bash
  git checkout -b chore/m4-release origin/main
  grep -n 'version = "0.24.0"' sdks/rust/Cargo.toml   # expected: 3:version = "0.24.0"
  grep -n 'version = "0.17.0"' sdks/python/pyproject.toml   # expected: 3:version = "0.17.0"
  grep -n '"version": "0.14.0"' sdks/typescript/package.json   # expected: 3:  "version": "0.14.0",
  ```
  No commit yet.

- [ ] **Step 2: Bump Rust to 0.25.0**

  Failing check: `grep -n 'version = "0.25.0"' sdks/rust/Cargo.toml` → exit 1, no output.
  Edit `sdks/rust/Cargo.toml` line 3 (the `[package]` block only — do NOT touch the oauth crates under `sdks/rust/crates/`):
  ```toml
  version = "0.25.0"
  ```
  Passing check: `grep -n 'version = "0.25.0"' sdks/rust/Cargo.toml` → `3:version = "0.25.0"`. Sanity: `cargo metadata --format-version 1 --no-deps --manifest-path sdks/rust/Cargo.toml | grep -o '"name":"motosan-ai","version":"0.25.0"'` → prints the match. No commit yet (single release commit in Step 17).

- [ ] **Step 3: Bump Python to 0.18.0 and refresh uv.lock**

  Failing check: `grep -n 'version = "0.18.0"' sdks/python/pyproject.toml` → exit 1.
  Edit `sdks/python/pyproject.toml` line 3: `version = "0.18.0"`.
  Then from repo root (uv workspace root — root `pyproject.toml` is `[tool.uv.workspace] members = ["sdks/python"]`):
  ```bash
  uv lock
  git diff --stat uv.lock   # expected: exactly one file changed; the only hunk flips motosan-ai to version = "0.18.0"
  grep -n -A1 '^name = "motosan-ai"$' uv.lock | grep 'version = "0.18.0"'   # expected: one match (~line 98)
  ```

- [ ] **Step 4: Bump TypeScript to 0.15.0 and regenerate the lockfile entry**

  Failing check: `grep -n '"version": "0.15.0"' sdks/typescript/package.json` → exit 1.
  Edit `sdks/typescript/package.json` line 3: `"version": "0.15.0",`.
  Then:
  ```bash
  cd sdks/typescript && npm install --package-lock-only && cd ../..
  grep -n '"version": "0.15.0"' sdks/typescript/package-lock.json   # expected: lines 3 and 9 (root entry + packages."" entry)
  ```

- [ ] **Step 5: Root CHANGELOG.md — write the complete M4 entry**

  Set the date once: `RELDATE=$(date +%Y-%m-%d)` — every heading below shows `2026-07-17`; substitute `$RELDATE` if executing on a different day. Insert the following block into `CHANGELOG.md` immediately after line 3 (`All notable changes...` + blank line), i.e. ABOVE the existing `## [rust-0.24.0 / python-0.17.0 / ts-0.14.0] — 2026-07-17` heading, followed by one blank line:

  ```markdown
  ## [rust-0.25.0 / python-0.18.0 / ts-0.15.0] — 2026-07-17

  M4 spec-and-parity release. **Breaking for Rust and Python** (CLI backend chat/stream contract; Python typed thinking events); minor for TypeScript (async token source only).

  ### Breaking

  - **CLI chat/stream contract** (Rust · Python): a successfully completed Claude Code / Codex CLI / Gemini CLI turn now always reports `stop_reason = end_turn` on both `chat()` and `stream()`. CLI backends never report `tool_use` — their tools are executed internally by the CLI, and `tool_use` means "caller must execute tools", which a CLI backend never requests; reporting it made agent loops re-execute already-executed tools. Blocking `chat()` for every CLI backend is now implemented as stream delegation (collect the provider's own `stream()`), so `ChatResponse.tool_calls` carries the **record of tools the CLI already executed** — never a request to execute — and content / thinking / usage / session_id parity with collecting `stream()` holds by construction. One documented parity exception: `chat()` may backfill `ChatResponse.model` from provider config when the collected value is empty. The `chat()` failure surface shifts to the stream-path variants: Rust `chat()` errors now arrive as the M3 stream variants (e.g. `StreamReadTimeout`), Python `chat()` nonzero-exit raises `StreamError` (was `ProviderError`), and the timeout scope becomes per-read stall rather than whole-invoke. Rust `codex_cli` `chat()` no longer splits the preamble into `thinking` (the old split was a post-hoc whole-transcript heuristic, unrepresentable in a stream): content is the concatenation, `thinking` is `None`.
  - **Python typed thinking events** (Python): streams emit `StreamEventType.thinking_delta` / `StreamEventType.thinking_done` (string values `"thinking_delta"` / `"thinking_done"`), replacing the ad-hoc `event_type="thinking"` string previously emitted by the anthropic and chatgpt-codex providers. Anthropic additionally emits `thinking_done` carrying the full concatenated thinking text on `content_block_stop` of a thinking block, mirroring Rust; stream collection prefers the `thinking_done` buffer over the concatenated-delta fallback. Consumers matching the old `"thinking"` string break.
  - **`StreamEventType` vocabulary pinned** (spec — all SDKs): `specs/types.md` fixes the event-type set to `text | tool_call_start | tool_call_args | tool_call_end | usage | thinking_delta | thinking_done` and documents the real emitters per SDK. `done` is a boolean **field** on `StreamEvent`, never an event type — the spec line that listed it as a member was a bug.

  ### Added

  - **Per-attempt token sources for chatgpt-codex** (Rust · Python · TypeScript): Rust adds the ungated `motosan_ai::auth::TokenSource` trait (+ `StaticTokenSource`), `ChatGptCodexProvider::with_token_source`, and `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)`; Python adds `token_source: Callable[[], Awaitable[str]] | None = None` on `ChatGptCodexProvider` / `Client.chatgpt_codex()` (constructor validation accepts `access_token` OR `token_source`); TypeScript widens `accessToken` to `string | (() => Promise<string>)`. In all three SDKs the bearer token is resolved at the top of **every retry attempt**, so long-lived agents can rotate expiring ChatGPT OAuth tokens without rebuilding the client. The SDKs stay decoupled from the workspace oauth crates — a refreshing `TokenSource` over `codex-oauth` ships as an `#[ignore]`d Rust live test.
  - **`Provider.claude_code`** (Python): new `Provider` StrEnum member, provider-construction routing, and a `Client.claude_code(...)` classmethod mirroring `Client.codex_cli(...)`, exposing the real `ClaudeCodeClient` constructor parameters.
  - **`ollama-native` feature alias** (Rust): hyphenated alias for `ollama_native`, matching every other multi-word feature name.

  ### Changed

  - **Rust feature architecture** (Rust): private umbrella features `_http = [dep:reqwest, dep:chrono, dep:eventsource-stream, dep:tokio]` and `_cli = [dep:tokio, dep:async-stream]` replace the per-provider `dep:` lists; `tokio-stream` is promoted to an unconditional dependency; the HTTP-shared retry/transport helpers move from `providers/mod.rs` to `src/transport/http.rs` behind one `#[cfg(feature = "_http")]` gate. The public feature set is unchanged (plus the new `ollama-native` alias) and the resolved dependency set of every pre-existing feature is identical. CI adds `cargo hack check --each-feature`.

  Per-SDK detail: [`sdks/rust/CHANGELOG.md`](sdks/rust/CHANGELOG.md), [`sdks/python/CHANGELOG.md`](sdks/python/CHANGELOG.md), [`sdks/typescript/CHANGELOG.md`](sdks/typescript/CHANGELOG.md).
  ```

  Passing check: `grep -n '## \[rust-0.25.0 / python-0.18.0 / ts-0.15.0\]' CHANGELOG.md` → `5:...` (one match, above the M3 entry).

- [ ] **Step 6: sdks/rust/CHANGELOG.md — write the complete [0.25.0] entry**

  Insert after the `## [Unreleased]` stub (line 5) and before `## [0.24.0] - 2026-07-17`. If Tasks 1–9 left bullets under `[Unreleased]`, fold them into this entry (keep this entry's wording where they overlap) and leave `## [Unreleased]` empty:

  ```markdown
  ## [0.25.0] - 2026-07-17

  ### Breaking
  - CLI chat/stream contract (`claude-code` / `codex-cli` / `gemini-cli`): a successfully completed CLI turn always reports `stop_reason = Some(StopReason::EndTurn)` on both `chat()` and `stream()`. CLI backends never report `ToolUse` — their tools are executed internally by the CLI; `ToolUse` means "caller must execute tools", which a CLI backend never requests, and reporting it made agent loops re-execute already-executed tools. The internal `cli_terminal_stop_reason(saw_tool_call)` helper is retired; the terminal stream event is always `done_with_stop_reason(EndTurn)`. Migration: code that branched on `StopReason::ToolUse` after a CLI turn should treat `ChatResponse.tool_calls` as the record of already-executed tools and branch on `EndTurn`.
  - `chat()` for all three CLI backends is reimplemented as stream delegation (collect the provider's own `stream()`), so `tool_calls` / `thinking` / `usage` / `session_id` populate identically on both paths. `ChatResponse.tool_calls` for CLI backends is **no longer always empty** — it records the tools the CLI already executed (never a request to execute). One documented parity exception: `chat()` backfills `ChatResponse.model` from provider config when the collected value is empty. The `chat()` failure surface shifts to the stream-path variants (`StreamReadTimeout` on stalls, stream error variants on abnormal CLI exit — no longer the single-shot mappings), and `codex-cli` `chat()` no longer splits the preamble into `thinking` (the old split was a post-hoc whole-transcript heuristic, unrepresentable in a stream: content is the concatenation, `thinking` is `None`). The newly-dead single-shot invoke path was removed.

  ### Added
  - `motosan_ai::auth` (ungated): `#[async_trait] pub trait TokenSource: Send + Sync + Debug { async fn access_token(&self) -> Result<String, MotosanError>; }` plus `StaticTokenSource`. `ChatGptCodexProvider` stores `Arc<dyn TokenSource>` — `new()` keeps its exact signature and wraps the plain token in `StaticTokenSource`; a `with_token_source` builder is added; `Debug` never prints token material — and resolves the bearer token at the top of **every retry attempt** (`send_with_retry_async_build`; the pre-existing `send_with_retry` is now a thin wrapper over it, preserving the single M2 retry engine and `on_retry`). `ClientBuilder::chatgpt_codex_token_source(Arc<dyn TokenSource>)` threads a custom source through the facade. The SDK stays decoupled from the oauth crates — a refreshing `TokenSource` over the workspace `codex-oauth` crate ships as an `#[ignore]`d live test.
  - `ollama-native` feature alias for `ollama_native`.

  ### Changed
  - Feature architecture: private umbrella features `_http = [dep:reqwest, dep:chrono, dep:eventsource-stream, dep:tokio]` and `_cli = [dep:tokio, dep:async-stream]` replace the per-provider `dep:` lists; `tokio-stream` is an unconditional dependency (and `stream.rs` loses its feature gate); the HTTP-shared helpers (`send_with_retry`, `observe_and_sleep`, `parse_retry_after`, `extract_request_id`, `is_retryable_status`, `is_retryable_network_error`, `map_http_error`, `RETRY_AFTER_CAP`, `TimeoutConfig`) move from `providers/mod.rs` to `src/transport/http.rs` behind one `#[cfg(feature = "_http")]` gate. Public feature set unchanged (plus the `ollama-native` alias); resolved dependencies per pre-existing feature are identical. CI adds `cargo hack check --each-feature`.
  ```

  Passing check: `grep -n '## \[0.25.0\]' sdks/rust/CHANGELOG.md` → one match between `[Unreleased]` and `[0.24.0]`.

- [ ] **Step 7: sdks/python/CHANGELOG.md — write the complete [0.18.0] entry**

  Insert above `## [0.17.0] - 2026-07-17` (line 5):

  ```markdown
  ## [0.18.0] - 2026-07-17

  ### Breaking
  - Typed thinking events: streams emit `StreamEventType.thinking_delta` / `StreamEventType.thinking_done` (string values `"thinking_delta"` / `"thinking_done"`), replacing the ad-hoc `event_type="thinking"` string previously emitted by the anthropic and chatgpt-codex providers. Anthropic additionally emits `thinking_done` carrying the full concatenated thinking text on `content_block_stop` of a thinking block (mirroring Rust); stream collection prefers the `thinking_done` buffer over the concatenated-delta fallback. Consumers matching `event.event_type == "thinking"` must switch to the new values. `StreamEvent.event_type` stays annotated `str` (`StrEnum` members are `str`).
  - CLI chat/stream contract (`ClaudeCodeClient` / `CodexCliClient` / `GeminiCliClient`): a successfully completed CLI turn always reports `stop_reason = "end_turn"` on both `chat()` and `stream()` — never `"tool_use"` (tools are executed internally by the CLI; reporting `tool_use` made agent loops re-execute already-executed tools). `chat()` is reimplemented as stream delegation, so `ChatResponse.tool_calls` for CLI backends now records the tools the CLI already executed (previously empty) and thinking / usage / session_id parity with collecting `stream()` holds by construction. One documented parity exception: `chat()` may backfill `ChatResponse.model` from provider config when the collected value is empty. The `chat()` failure surface shifts to the stream path: nonzero-exit/child-death raises `StreamError` (was `ProviderError`) and the timeout scope becomes per-read stall rather than whole-invoke; `codex_cli`'s text-only terminal now carries `stop_reason = "end_turn"` (was `None`).

  ### Added
  - `token_source: Callable[[], Awaitable[str]] | None = None` on `ChatGptCodexProvider` and `Client.chatgpt_codex()`: when set, the bearer token is resolved at the top of **every retry attempt**, so long-lived agents can rotate expiring ChatGPT OAuth tokens without rebuilding the client. Constructor validation accepts `access_token` OR `token_source`. Note: `account_id` gains a `None` default to keep the signature legal — omitting it now raises `ConfigError` at construction (previously a `TypeError` at call time).
  - `Provider.claude_code` enum member, provider-construction routing, and a `Client.claude_code(...)` classmethod mirroring `Client.codex_cli(...)`, exposing the real `ClaudeCodeClient` constructor parameters.
  ```

  Passing check: `grep -n '## \[0.18.0\]' sdks/python/CHANGELOG.md` → `5:## [0.18.0] - ...`.

- [ ] **Step 8: sdks/typescript/CHANGELOG.md — write the complete [0.15.0] entry**

  Insert above `## [0.14.0] - 2026-07-17` (line 7):

  ```markdown
  ## [0.15.0] - 2026-07-17

  ### Added
  - `ChatGptCodexProvider` `accessToken` widens to `string | (() => Promise<string>)`. A function source is awaited at the top of **every retry attempt**, so long-lived agents can rotate expiring ChatGPT OAuth tokens without rebuilding the client. Passing a plain string behaves exactly as before — no breaking change.
  ```

  Passing check: `grep -n '## \[0.15.0\]' sdks/typescript/CHANGELOG.md` → one match above `[0.14.0]`.

- [ ] **Step 9: AGENTS.md — version line + M4 paragraph**

  Edit 1 — locate `Rust v0.24.0 · Python v0.17.0 (PyPI) · TypeScript v0.14.0 (npm)` (line 5), replace with:
  ```
  Rust v0.25.0 · Python v0.18.0 (PyPI) · TypeScript v0.15.0 (npm)
  ```
  Edit 2 — insert a new paragraph (plus blank line) immediately after the M3 paragraph (line 15, begins `Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0 are the M3 stream-contract + timeout releases:`), before `## Current Rust Tool Schema Note`:
  ```
  Rust 0.25.0 / Python 0.18.0 / TypeScript 0.15.0 are the M4 spec-and-parity releases: CLI backends (Claude Code / Codex CLI / Gemini CLI) always finish a completed turn with `stop_reason = end_turn` — never `tool_use` — and blocking `chat()` delegates to `stream()` + collect, so `ChatResponse.tool_calls` records the tools the CLI already executed (**breaking**, Rust + Python); Python thinking events are typed `thinking_delta` / `thinking_done`, replacing the ad-hoc `"thinking"` string (**breaking**); `specs/types.md` pins the `StreamEventType` vocabulary (`done` is a bool field, not an event type); chatgpt-codex gains per-attempt token sources (Rust `motosan_ai::auth::TokenSource` + `ClientBuilder::chatgpt_codex_token_source`, Python `token_source=` callable, TypeScript `accessToken` as `() => Promise<string>`); Python adds `Provider.claude_code` / `Client.claude_code()`; Rust reorganizes features around private `_http`/`_cli` umbrellas with a new `ollama-native` alias (public feature set otherwise unchanged) and CI adds `cargo hack check --each-feature`.
  ```
  Passing check: `grep -c '0.25.0' AGENTS.md` → `2` (version line + M4 paragraph).

- [ ] **Step 10: llms.txt — five sites**

  Edit 1 — line 5: `- Python 0.17.0 · TypeScript 0.14.0 · Rust 0.24.0` → `- Python 0.18.0 · TypeScript 0.15.0 · Rust 0.25.0`.
  Edit 2 — insert after the M3 bullet (line 8, begins `- Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0 (M3):`):
  ```
  - Rust 0.25.0 / Python 0.18.0 / TypeScript 0.15.0 (M4): **breaking** CLI chat/stream contract (Rust + Python) — a completed Claude Code / Codex CLI / Gemini CLI turn always reports `stop_reason = end_turn` (never `tool_use`) and `chat()` delegates to `stream()` + collect, so `tool_calls` records the tools the CLI already executed; **breaking** Python typed thinking events `thinking_delta` / `thinking_done` (replacing ad-hoc `"thinking"`); chatgpt-codex per-attempt token sources (Rust `TokenSource` / Python `token_source=` / TypeScript async `accessToken`); Python `Provider.claude_code` + `Client.claude_code()`; Rust `_http`/`_cli` feature umbrellas + `ollama-native` alias (public features otherwise unchanged).
  ```
  Edit 3 — line 25: `motosan-ai = { version = "0.24.0", features = ["anthropic"] }` → `motosan-ai = { version = "0.25.0", features = ["anthropic"] }`.
  Edit 4 — lines 26–28, replace:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native | full
  #           gemini | gemini-code-assist
  # CLI backends (Rust features; Python has built-in ClaudeCodeClient/CodexCliClient):
  ```
  with:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native (alias: ollama-native) | full
  #           gemini | gemini-code-assist | chatgpt-codex
  # CLI backends (Rust features; Python has built-in ClaudeCodeClient/CodexCliClient/GeminiCliClient):
  ```
  Edit 5 — § Release tag table (line 928): `| TypeScript   | ` + backtick-`ts-vX.Y.Z`-backtick + ` | ` + backtick-`ts-v0.14.0`-backtick + ` |` — replace the example `ts-v0.14.0` with `ts-v0.15.0` (mirrors what the M3 release commit did to this row).
  Passing check: `grep -c '0\.25\.0' llms.txt` → `3` (header bullet, M4 bullet, install snippet) and `grep -n 'ts-v0.15.0' llms.txt` → one match in the tag table.

- [ ] **Step 11: skills/motosan-ai/SKILL.md + references/rust-api.md**

  SKILL.md Edit 1 — line 8: `Multi-provider LLM SDK — Python 0.17.0 / Rust 0.24.0 / TypeScript 0.14.0` → `Multi-provider LLM SDK — Python 0.18.0 / Rust 0.25.0 / TypeScript 0.15.0`.
  SKILL.md Edit 2 — line 25: `motosan-ai = { version = "0.24.0", features = ["anthropic"] }` → `motosan-ai = { version = "0.25.0", features = ["anthropic"] }`.
  SKILL.md Edit 3 — lines 26–27, replace:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native | full
  #           gemini | gemini-code-assist
  ```
  with:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native (alias: ollama-native) | full
  #           gemini | gemini-code-assist | chatgpt-codex
  ```
  SKILL.md Edit 4 — line 139 contains the now-false fragment. Replace exactly this fragment (leave the rest of the long line intact):
  ```
  Blocking `chat().tool_calls` is empty on all three (tools run inside the CLI), but as of Rust v0.20.0 `stream()` surfaces CLI tool use as
  ```
  with:
  ```
  As of Rust 0.25.0 / Python 0.18.0, blocking `chat()` on all three delegates to `stream()` + collect — `chat().tool_calls` records the tools the CLI already executed (never a request to execute; a completed CLI turn always reports `stop_reason = end_turn`, never `tool_use`) — and `stream()` surfaces CLI tool use as
  ```
  references/rust-api.md Edit — lines 6–8, replace:
  ```
  motosan-ai = { version = "0.24.0", features = ["anthropic"] }
  # features: anthropic | openai | minimax | ollama | ollama_native | full
  #           gemini | gemini-code-assist | claude-code | codex-cli | gemini-cli
  ```
  with:
  ```
  motosan-ai = { version = "0.25.0", features = ["anthropic"] }
  # features: anthropic | openai | minimax | ollama | ollama_native (alias: ollama-native) | full
  #           gemini | gemini-code-assist | chatgpt-codex | claude-code | codex-cli | gemini-cli
  ```
  Passing check: `grep -rn '0\.24\.0' skills/motosan-ai/ | grep -v 'Rust 0.24.0 / Python 0.17.0 / TypeScript 0.14.0'` → empty (the only surviving `0.24.0` in `skills/` is the historical M3 stream-contract bullet at SKILL.md:131, which stays).

- [ ] **Step 12: root README.md — Languages table, install snippet, CLI note**

  Edit 1 — lines 29–31, replace `v0.24.0` → `v0.25.0`, `v0.17.0` → `v0.18.0`, `v0.14.0` → `v0.15.0` in the Languages table rows.
  Edit 2 — line 38: `motosan-ai = { version = "0.24.0", features = ["anthropic"] }` → `motosan-ai = { version = "0.25.0", features = ["anthropic"] }`.
  Edit 3 — lines 39–40, replace:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native | full
  #           gemini | gemini-code-assist | claude-code | codex-cli | gemini-cli
  ```
  with:
  ```
  # features: anthropic | openai | minimax | ollama | ollama_native (alias: ollama-native) | full
  #           gemini | gemini-code-assist | chatgpt-codex | claude-code | codex-cli | gemini-cli
  ```
  Edit 4 — line 204, replace the whole blockquote line:
  ```
  > **CLI backend limitations (Claude Code / Codex CLI / Gemini CLI):** Tool calls run internally by the CLI and are **not** surfaced on `ChatResponse.tool_calls` (always empty). All CLI backends require the corresponding binary installed and authenticated. In Rust, enable with `--features claude-code`, `--features codex-cli`, or `--features gemini-cli`. Python currently includes `ClaudeCodeClient` and `CodexCliClient` as built-in subprocess backends.
  ```
  with:
  ```
  > **CLI backend semantics (Claude Code / Codex CLI / Gemini CLI):** Tools run internally by the CLI. Since Rust 0.25.0 / Python 0.18.0, `ChatResponse.tool_calls` records the tools the CLI already executed — never a request to execute — and a completed CLI turn always reports `stop_reason = end_turn`. All CLI backends require the corresponding binary installed and authenticated. In Rust, enable with `--features claude-code`, `--features codex-cli`, or `--features gemini-cli`. Python includes `ClaudeCodeClient`, `CodexCliClient`, and `GeminiCliClient` as built-in subprocess backends (`Provider.claude_code` / `Provider.codex_cli` / `Provider.gemini_cli`).
  ```
  Passing check: `grep -n '0\.24\.0\|always empty' README.md` → empty.

- [ ] **Step 13: per-SDK READMEs — version snippets + stale F4 claims (conditional)**

  `sdks/rust/README.md` — unconditional version bumps at lines 323, 433, 497: replace each `motosan-ai = { version = "0.24.0", features = [...] }` with `version = "0.25.0"` (features arg unchanged: `claude-code` / `codex-cli` / `gemini-cli` respectively).
  `## Features` section (lines 103–115): replace the list body with:
  ```markdown
  - `anthropic`
  - `openai`
  - `minimax`
  - `ollama` (OpenAI-compatible mode)
  - `ollama_native` (native `/api/chat` endpoint with NDJSON streaming; `ollama-native` is an equivalent alias since 0.25.0)
  - `gemini` (Google Generative AI HTTP API)
  - `gemini-code-assist` (Google Cloud Code Assist HTTP API; depends on `gemini`)
  - `chatgpt-codex` (ChatGPT-backend Responses API; OAuth bearer token, no API key)
  - `claude-code` (local Claude Code CLI backend)
  - `codex-cli` (local Codex CLI backend)
  - `gemini-cli` (local Gemini CLI backend)
  - `full` (enables HTTP providers: `anthropic`, `openai`, `minimax`, `ollama`, `ollama_native`, `ollama-native`, `gemini`, `gemini-code-assist`, `chatgpt-codex`)
  ```
  (Keep the `full` list in exact sync with `[features] full` in `sdks/rust/Cargo.toml` as Task 1 merged it — verify with `grep -A11 '^full = ' sdks/rust/Cargo.toml`.)
  Conditional F4-claim fixes — run `grep -n 'is always empty' sdks/rust/README.md`. If the F4 PR already rewrote them the grep is empty → skip. Otherwise apply all three:
  - Line 423 old: `- Blocking `ChatResponse.tool_calls` is always empty — tools run inside the CLI and are not folded into `chat()` responses.` → new: `- Blocking `chat()` delegates to `stream()` + collect (since 0.25.0): `ChatResponse.tool_calls` records the tools the CLI already executed — never a request to execute — and a completed turn always reports `StopReason::EndTurn`.`
  - Line 486 old: `- Blocking `ChatResponse.tool_calls` is always empty — Codex tool invocations are not folded into `chat()` responses.` → new: `- Blocking `chat()` delegates to `stream()` + collect (since 0.25.0): `ChatResponse.tool_calls` records the Codex tool invocations the CLI already executed, and a completed turn always reports `StopReason::EndTurn`.`
  - Line 537 old: `- **Tool calls**: blocking `ChatResponse.tool_calls` is always empty, but `stream()` surfaces Gemini `tool_use` events as `ToolCallStart` → `ToolCallArgs` → `ToolCallEnd`. Gemini `tool_result` events are ignored.` → new: `- **Tool calls**: blocking `chat()` delegates to `stream()` + collect (since 0.25.0), so `ChatResponse.tool_calls` records already-executed Gemini `tool_use` events; `stream()` surfaces them as `ToolCallStart` → `ToolCallArgs` → `ToolCallEnd`, a completed turn always reports `StopReason::EndTurn`, and Gemini `tool_result` events are ignored.`
  `sdks/typescript/README.md` — lines 486–487 (Publishing example): `git tag ts-v0.14.0` / `git push origin ts-v0.14.0` → `git tag ts-v0.15.0` / `git push origin ts-v0.15.0` (mirrors the M3 commit's treatment of this example). Leave the historical "Since v0.14.0..." stream-contract prose at lines 209/214 untouched.
  Passing check: `grep -rn 'version = "0.24.0"\|is always empty' sdks/rust/README.md` → empty; `grep -n 'ts-v0.15.0' sdks/typescript/README.md` → 2 matches.

- [ ] **Step 14: lib.rs docstring check + repo-wide stale-version sweep**

  Crate-docs check: `grep -n '^//!' sdks/rust/src/lib.rs` — at baseline this prints nothing (lib.rs has no crate docstring and enumerates no features → no edit). If Task 1 added a `//!` feature list, it must enumerate exactly: `anthropic`, `openai`, `minimax`, `ollama`, `ollama_native` (alias `ollama-native`), `gemini`, `gemini-code-assist`, `chatgpt-codex`, `claude-code`, `codex-cli`, `gemini-cli`, `full` — update it to that exact set if it drifted.
  Sweep:
  ```bash
  grep -rn '0\.24\.0' --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=target --exclude-dir=.codegraph . | grep -v 'CHANGELOG'
  grep -rn 'Python v\?0\.17\.0\|python-v0\.17\.0' --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=target . | grep -v 'CHANGELOG'
  grep -rn 'TypeScript v\?0\.14\.0\|ts-v0\.14\.0' --exclude-dir=.git --exclude-dir=node_modules --exclude-dir=target . | grep -v 'CHANGELOG'
  ```
  Every remaining hit must be **historical**, i.e. one of: the M3 paragraph in AGENTS.md (line 15) / llms.txt M3 bullet (line 8) and llms.txt:457–458 stream-contract note / SKILL.md:131 stream-contract bullet / `sdks/rust/README.md:50` ("Since v0.24.0") / `sdks/typescript/README.md:209,214` ("Since v0.14.0") / `sdks/rust/src/client.rs:900` `#[deprecated(since = "0.24.0")]` / `sdks/rust/tests/client_builder.rs:121,165` comments. Any hit describing the CURRENT version (tables, install snippets, header lines) is a missed site — fix it with the corresponding 0.25.0 / 0.18.0 / 0.15.0 value before proceeding. (Note: `Python 0.14.0` mentions are the Python SDK's own historical release — do NOT touch them.)

- [ ] **Step 15: verify the three publish workflows exist and are untouched**

  ```bash
  ls .github/workflows/publish-rust.yml .github/workflows/publish-python.yml .github/workflows/publish-typescript.yml
  grep -H -A3 '^on:' .github/workflows/publish-rust.yml .github/workflows/publish-python.yml .github/workflows/publish-typescript.yml
  git diff origin/main -- .github/workflows/
  ```
  Expected: all three files listed; triggers read `tags: - "rust-v*"` / `tags: ["python-v*"]` / `tags: ["ts-v*"]` respectively; the `git diff` is **empty** — this PR must not modify any workflow. (`publish-typescript.yml` additionally verifies the tag equals `package.json` version before `npm publish` — the Step 4 bump is what makes the future `ts-v0.15.0` tag publishable.)

- [ ] **Step 16: full gate**

  From the repo root (nix devshell via direnv, or `nix develop -c <cmd>`):
  ```bash
  check-all && cd sdks/typescript && npm run typecheck && npm run build && npm test && cd ../..
  ```
  Expected: `check-all` exits 0 (Rust: `cargo fmt` clean, `cargo clippy --all-features --all-targets -- -D warnings` clean, `cargo test --all-features` all pass; Python: `ruff check` / `ruff format --check` clean, `pytest` all pass); TS typecheck/build exit 0 and `npm test` reports all suites passing. Version bumps and doc edits must not change any test outcome — if anything fails here, a previous M4 PR broke the gate on `main`; stop and report rather than patching code in this release PR.

- [ ] **Step 17: commit, push, open PR-REL — explicitly NO tag, NO publish**

  ```bash
  git add -A && git status --short   # review: only the files listed in this task's Files block
  git commit -m "$(cat <<'EOF'
  chore(release): M4 — Rust 0.25.0 / Python 0.18.0 / TS 0.15.0

  Release prep only: version bumps (Cargo.toml / pyproject.toml /
  package.json + lockfiles), root and per-SDK CHANGELOG entries, and
  version/feature/behavior sweeps across AGENTS.md, llms.txt,
  skills/motosan-ai, and the READMEs. NO tag, NO publish in this PR —
  publishing is tag-triggered CI, tags pushed post-merge per
  llms.txt § Release.

  Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>
  EOF
  )"
  git push -u origin chore/m4-release
  gh pr create --base main --title "chore(release): M4 — Rust 0.25.0 / Python 0.18.0 / TS 0.15.0" --body "$(cat <<'EOF'
  M4 release prep (PR-REL). Bumps Rust 0.25.0 / Python 0.18.0 / TS 0.15.0, adds root + per-SDK changelog entries, and sweeps AGENTS.md / llms.txt / skills/motosan-ai / READMEs for stale versions and pre-F4 CLI tool_calls claims.

  - NO git tag and NO publish here: publish-rust.yml / publish-python.yml / publish-typescript.yml are tag-triggered (rust-v* / python-v* / ts-v*); the maintainer pushes rust-v0.25.0, python-v0.18.0, ts-v0.15.0 after merge.
  - Prereq: all other M4 PRs merged; gates: check-all + TS typecheck/build/test green.

  🤖 Generated with [Claude Code](https://claude.com/claude-code)
  EOF
  )"
  ```
  Expected: PR URL printed; CI (ci-rust / ci-python / ci-typescript) green. **Do NOT run `git tag`, `git push --tags`, `cargo publish`, `npm publish`, `uv build`/`uv publish`, or any `workflow_dispatch`.** Final sanity: `git tag --points-at HEAD` → empty output.

---

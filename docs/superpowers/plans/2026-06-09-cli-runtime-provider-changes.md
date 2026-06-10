# CliRuntime Provider Changes (P0+P1+P2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the `motosan-ai` Rust CLI-provider capabilities that `CliRuntime` (§6.2) is gated on — process `cwd`, session continuity, env injection, tool-call stream events, and stream robustness — so an external `AgentRuntime` adapter can spawn Claude Code / Codex / Gemini CLIs with correct working-directory, resumable sessions, and per-run secrets.

**Architecture:** Six independent, separately-shippable milestones (one PR each), applied in order. Every knob is a **provider-builder field** threaded into `SpawnConfig` and consumed at the two spawn sites per provider (inline streaming in `mod.rs::stream()`, blocking in `spawn.rs`), following the existing `max_budget_usd` / `resume` / `config_overrides` idiom. Shared-type changes (`StreamEvent`, `ChatResponse`) are additive and serde-skipped so the wire format and HTTP providers are untouched. Design route for `cwd`/`session` = **(a) builder fields** (chosen by the maintainer over the `ChatRequest` route).

**Tech Stack:** Rust 2021, `tokio::process::Command`, `async_stream`, `serde`/`serde_json`, the project's `SpawnConfig`/`ProviderImpl` pattern. Gates: `cargo fmt`, `cargo clippy --all-features`, `cargo test --all-features` (`check-rust`).

**Source basis:** verified audit of motosan-ai 0.19.0 (`sdks/rust`), 2026-06-09. Requirements: `docs/cli-runtime-integration-requirements.md`.

---

## 0. How to execute this plan

### 0.1 Milestone order & what each delivers

| # | Milestone | Priority | Delivers | Depends on |
|---|---|---|---|---|
| **M1** | `cwd` setters (ClaudeCode + Gemini) | **P0** | Process working directory honored → the `cwd` contract is satisfiable; flagship ClaudeCode path unblocked | — |
| **M2** | Session continuity | **P1** | `StreamEvent.session_id` + `ChatResponse.session_id`; Codex `exec resume <id>`; Claude/Gemini id readback | M1 merged |
| **M2.5** | Fallible stream (Item → `Result`) | **infra** | `BoxStream` Item → `Result<StreamEvent, MotosanError>`; providers stop swallowing → `yield Err`; `collect_stream` → `Result`. **Breaking → 0.20.** | M2 merged; **before M4** |
| **M3** | Env injection | **P2.1** | `.env()/.envs()` per-run secret bundle into the child; redacted from `Debug` | M1, M2 merged (any order vs M2.5) |
| **M4** | Tool-call stream events | **P2.2** | `tool_use`/`command`/`mcp` wire events surfaced as `StreamEvent::tool_call_*` | **M2.5** merged |
| **M5** | Stream robustness | **P2.3** | configurable timeout, real success `stop_reason`, documented cancel (**error surfacing now lives in M2.5**) | M2.5, M4 merged |
| **M6** | Per-request override (OPTIONAL) | **P2.4** | `provider_options` override of `cwd`/`session`/`budget` | M1, M3 merged — **recommend DEFER** |

Apply in order. Each milestone is **one PR through CI** — `.rs`/`Cargo.toml` changes never go direct-to-main.

### 0.2 Line numbers are baseline-relative — anchor by context

Every line number in this plan is against the **0.19.0 baseline**. As earlier milestones merge, line numbers in later milestones **drift**. Always locate the edit by the **quoted surrounding code / symbol name** in each task's anchor, not the bare line number. Each task's "Files" block names the function/struct to find.

### 0.3 Cross-milestone coupling (read before starting)

Three structs get fields added by multiple milestones (`ClaudeCodeProvider`/`CodexCliProvider`/`GeminiCliProvider` + their `SpawnConfig` + `build_spawn_config` + `empty_config` test helper). The Rust compiler **enumerates** every missed site (struct-literal exhaustiveness), so a missed field is a compile error, never a silent bug — trust the compiler. Specific carry-overs are called out inline:

- **M2 added `StreamEvent.session_id`** to all 11 constructors (shipped). **M2.5 removes the need for any `done_error` constructor** — after M2.5, mid-stream errors are `Err` items, not a sentinel `StreamEvent`. M5 no longer adds `done_error`.
- **M3 (`envs`) and M5 (`timeout`) both add provider/SpawnConfig fields.** Each is appended at the END of the field list; update `build_spawn_config` and `empty_config` + the `*_full_loadout_*` test literal in lockstep (the compiler will demand it).
- **M2, M4, M5 all add `NdjsonAction` variants and `stream()` match arms.** Each milestone adds its own arm; later milestones see a larger match block — match on the variant name, not position.

### 0.4 Secret redaction decision (locks M3)

`envs` carries secrets. Rather than a hand-written `impl Debug` per provider (fragile: silently drops fields as M-later adds them), M3 introduces a **`RedactedEnvs` newtype** whose `Debug` prints `<N redacted>`. The providers keep `#[derive(Debug)]` and auto-redact via the field's `Debug` — **zero per-milestone Debug maintenance**. `SpawnConfig` has no `Debug` derive, so it keeps a plain `Vec<(String,String)>`.

### 0.5 Two design calls (the "is this general?" decisions)

Both were reviewed against the codebase's own conventions, not an abstract ideal:

1. **`session_id` lives as a typed `Option<String>` on the shared `StreamEvent`/`ChatResponse` (kept).** This *is* the house style: `ChatResponse.thinking` (only thinking models set it) and `Usage.cache_*_tokens` (only Anthropic sets them) are the same "provider-specific optional on a shared type" pattern. A generic `provider_metadata: Value` bag would be more "generic" but untyped (loses discoverability/type-safety); a provider accessor would need interior mutability and is awkward for streaming. Typed field wins. No change.
2. **Stream errors surface as `Err`, not a sentinel event (M2.5).** Errors belong in the type system (compiler-enforced) rather than a `done && stop_reason == Other` convention. Crucially this is *not* a CLI-only gap: every HTTP provider already swallows mid-stream errors (`Err(_) => continue`) and silently ends — and already produces `Item = Result<…>` internally, discarding the error at the boundary. M2.5 stops discarding it, for all providers. Cost: a breaking `BoxStream` item-type change → **0.20**. This replaces M5's earlier CLI-only `done_error` approach.

### 0.6 Per-milestone rhythm

Every milestone ends with the same gate. Each task below is one TDD step; the milestone wrapper is:

```bash
git switch -c <branch>           # e.g. feat/cli-cwd-setters
# ... TDD tasks (red → green → commit) ...
fmt                              # project alias: formats Rust+Python+TOML+Nix
check-rust                       # fmt-check → clippy --all-features → test --all-features  (THE gate)
git push -u origin <branch>
gh pr create --fill              # open PR; wait for CI green before next milestone
```

**`cargo fmt` after every test-adding step** — the hand-formatted code blocks and `async_stream!` macro bodies will otherwise fail the fmt gate.

---

## Milestone M1 — P0: `cwd` setters (ClaudeCode + Gemini)

**Branch:** `feat/cli-cwd-setters`
**Why:** `CliRuntime` cannot set the spawned process's working directory for ClaudeCode/Gemini (only Codex has `.cd()`). The runtime does **not** own the spawn, so there is no process-layer workaround — the setter must live in the provider. This is the mandatory gate for the `cwd` contract.

**Outcome:** `ClaudeCodeProvider::cwd(dir)` and `GeminiCliProvider::cwd(dir)` cause the child to run with `current_dir(dir)`; Codex already has `.cd()`.

### Task M1.1 — ClaudeCode: extract a testable `build_command` helper (refactor, no behavior change)

`claude_code/spawn.rs::invoke_cli` builds its `Command` inline, so the blocking path's `current_dir`/`env` wiring can't be unit-tested. Extract a `build_command` helper (mirroring codex/gemini, which already have one) so M1.2 and M3 can assert on `cmd.as_std()`.

**Files:**
- Modify: `sdks/rust/src/providers/claude_code/spawn.rs` — `invoke_cli` (≈360-385)
- Test: same file's `#[cfg(test)]` module

- [ ] **Step 1: Write the failing test**

In the `claude_code/spawn.rs` test module:

```rust
    #[test]
    fn build_command_uses_binary_and_print_args() {
        let cmd = build_command(&empty_config());
        let std_cmd = cmd.as_std();
        let argv: Vec<String> = std_cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(argv.contains(&"--print".to_string()));
        assert_eq!(argv.last().map(String::as_str), Some("-"));
    }
```

- [ ] **Step 2: Run it — expect failure**

Run: `cargo test --features claude-code -p motosan-ai claude_code::spawn::tests::build_command_uses_binary_and_print_args`
Expected: FAIL to compile — `cannot find function build_command in this scope`.

- [ ] **Step 3: Extract the helper**

In `invoke_cli`, replace the inline `Command` construction — the block from `let mut cmd = Command::new(&config.binary_path);` down through the three `cmd.stdin/stdout/stderr(...piped())` lines, **including the intervening `cmd.kill_on_drop(true);` line** (≈spawn.rs:378, between `cmd.arg("-")` and the piped lines), but NOT the `.spawn()` call — with a call to a new helper. The helper body below already contains `kill_on_drop`, so the end state is unchanged:

```rust
/// Build the blocking `claude --print ...` Command (everything except spawn).
/// Shared construction point so working-directory / env wiring is applied and
/// unit-testable in one place.
fn build_command(config: &SpawnConfig) -> Command {
    let mut cmd = Command::new(&config.binary_path);
    cmd.arg("--print");
    if config.agent_mode {
        cmd.arg("--output-format").arg("json");
    }
    cmd.args(common_args(config));
    cmd.arg("-"); // read prompt from stdin
    cmd.kill_on_drop(true);
    cmd.stdin(std::process::Stdio::piped());
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd
}
```

Then `invoke_cli` begins:

```rust
pub async fn invoke_cli(
    config: &SpawnConfig,
    prompt: &str,
) -> Result<(String, Usage), MotosanError> {
    let mut cmd = build_command(config);

    let mut child = cmd
        .spawn()
        .map_err(|e| MotosanError::ProviderError(format!("failed to spawn claude CLI: {e}")))?;
    // ... rest unchanged (stdin write, timeout, parse) ...
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --features claude-code -p motosan-ai claude_code::`
Expected: PASS (all existing `common_args_*`/`invoke` tests still green + the new one).

- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/claude_code/spawn.rs
git commit -m "refactor(claude_code): extract build_command helper for testable spawn"
```

### Task M1.2 — ClaudeCode: add the `cwd` field, builder, and `current_dir` application

**Files:**
- Modify: `sdks/rust/src/providers/claude_code/mod.rs` — struct (≈128), `new()` (≈161), builder (≈350), `build_spawn_config` (≈381), `stream()` (≈418)
- Modify: `sdks/rust/src/providers/claude_code/spawn.rs` — `SpawnConfig` (≈161), `build_command`, `empty_config` (≈494), `common_args_full_loadout_order_is_stable` literal (≈875)

> `PathBuf` is already imported in both files (`mod.rs:40`, `spawn.rs:16`).

- [ ] **Step 1: Write the failing tests**

In `claude_code/mod.rs` test module:

```rust
    #[test]
    fn cwd_builder_threads_into_spawn_config() {
        let provider = ClaudeCodeProvider::new().cwd("/work/dir");
        let cfg = provider.build_spawn_config(None, None);
        assert_eq!(cfg.cwd.as_deref(), Some(std::path::Path::new("/work/dir")));
    }
```

In `claude_code/spawn.rs` test module:

```rust
    #[test]
    fn build_command_sets_current_dir_when_cwd_present() {
        let cfg = SpawnConfig {
            cwd: Some(PathBuf::from("/work/dir")),
            ..empty_config()
        };
        let cmd = build_command(&cfg);
        assert_eq!(cmd.as_std().get_current_dir(), Some(std::path::Path::new("/work/dir")));
        // No cwd → inherits parent (None).
        assert_eq!(build_command(&empty_config()).as_std().get_current_dir(), None);
    }
```

- [ ] **Step 2: Run them — expect failure**

Run: `cargo test --features claude-code -p motosan-ai claude_code:: -- cwd build_command_sets_current_dir`
Expected: FAIL to compile — `SpawnConfig` / `ClaudeCodeProvider` have no field `cwd`; no method `cwd`.

- [ ] **Step 3: Add the field + builder + wiring**

`mod.rs` — struct field, after `pub max_budget_usd: Option<f64>,`:

```rust
    /// Working directory for the spawned `claude` process. When set, the child
    /// runs with this cwd (`Command::current_dir`) instead of inheriting the
    /// parent's. The §6.2 `CliRuntime` cwd contract requires this.
    pub cwd: Option<PathBuf>,
```

`mod.rs` — `new()`, after `max_budget_usd: None,`:

```rust
            cwd: None,
```

`mod.rs` — builder, after the `max_budget_usd` builder method:

```rust
    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }
```

`mod.rs` — `build_spawn_config`, after `max_budget_usd: self.max_budget_usd,`:

```rust
            cwd: self.cwd.clone(),
```

`mod.rs` — `stream()`, immediately after `let mut cmd = Command::new(&config.binary_path);`:

```rust
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
```

`spawn.rs` — `SpawnConfig` field, after `pub max_budget_usd: Option<f64>,`:

```rust
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
```

`spawn.rs` — inside `build_command`, immediately after `let mut cmd = Command::new(&config.binary_path);`:

```rust
    if let Some(dir) = &config.cwd {
        cmd.current_dir(dir);
    }
```

`spawn.rs` — `empty_config()`, after `max_budget_usd: None,`:

```rust
            cwd: None,
```

`spawn.rs` — `common_args_full_loadout_order_is_stable` (the explicit `SpawnConfig { ... }` literal that does NOT use `..empty_config()`), after `max_budget_usd: Some(3.0),`:

```rust
            cwd: None,
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --features claude-code -p motosan-ai claude_code::`
Expected: PASS. (`cwd` is not an argv flag, so `common_args_*` assertions are unchanged.)

- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/claude_code/
git commit -m "feat(claude_code): add cwd setter (Command::current_dir) for CliRuntime"
```

### Task M1.3 — Gemini: add the `cwd` field, builder, and `current_dir` application

Gemini already has a `build_command` helper (`gemini_cli/spawn.rs:179-188`), so no extraction is needed.

**Files:**
- Modify: `sdks/rust/src/providers/gemini_cli/mod.rs` — struct (≈60), `new()` (≈80), builder (≈158), `build_spawn_config` (≈171), `stream()` (≈208)
- Modify: `sdks/rust/src/providers/gemini_cli/spawn.rs` — `SpawnConfig` (≈92), `build_command` (≈180), `empty_config` (≈294), `common_args_full_loadout_order_is_stable` literal (≈506)

> `PathBuf` is imported in both (`mod.rs:29`, `spawn.rs:16`).

- [ ] **Step 1: Write the failing tests**

`gemini_cli/mod.rs` test module:

```rust
    #[test]
    fn cwd_builder_threads_into_spawn_config() {
        let cfg = GeminiCliProvider::new().cwd("/work/dir").build_spawn_config(None);
        assert_eq!(cfg.cwd.as_deref(), Some(std::path::Path::new("/work/dir")));
    }
```

`gemini_cli/spawn.rs` test module:

```rust
    #[test]
    fn build_command_sets_current_dir_when_cwd_present() {
        let cfg = SpawnConfig { cwd: Some(PathBuf::from("/work/dir")), ..empty_config() };
        assert_eq!(build_command(&cfg).as_std().get_current_dir(), Some(std::path::Path::new("/work/dir")));
        assert_eq!(build_command(&empty_config()).as_std().get_current_dir(), None);
    }
```

- [ ] **Step 2: Run them — expect failure**

Run: `cargo test --features gemini-cli -p motosan-ai gemini_cli:: -- cwd build_command_sets_current_dir`
Expected: FAIL to compile — no field/method `cwd`.

- [ ] **Step 3: Add the field + builder + wiring**

`mod.rs` — struct field after `pub resume: Option<String>,`:

```rust
    /// Working directory for the spawned `gemini` process. When set, the child
    /// runs with this cwd (`Command::current_dir`) instead of inheriting the parent's.
    pub cwd: Option<PathBuf>,
```

`mod.rs` — `new()` after `resume: None,`:

```rust
            cwd: None,
```

`mod.rs` — builder after the `resume` builder:

```rust
    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }
```

`mod.rs` — `build_spawn_config` after `resume: self.resume.clone(),`:

```rust
            cwd: self.cwd.clone(),
```

`mod.rs` — `stream()` immediately after `let mut cmd = Command::new(&config.binary_path);`:

```rust
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
```

`spawn.rs` — `SpawnConfig` field after `pub resume: Option<String>,`:

```rust
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,
```

`spawn.rs` — inside `build_command`, immediately after `let mut cmd = Command::new(&config.binary_path);`:

```rust
    if let Some(dir) = &config.cwd {
        cmd.current_dir(dir);
    }
```

`spawn.rs` — `empty_config()` after `resume: None,`:

```rust
            cwd: None,
```

`spawn.rs` — `common_args_full_loadout_order_is_stable` literal after `resume: Some("latest".to_string()),`:

```rust
            cwd: None,
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test --features gemini-cli -p motosan-ai gemini_cli::`
Expected: PASS.

- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/gemini_cli/
git commit -m "feat(gemini_cli): add cwd setter (Command::current_dir) for CliRuntime"
```

### Task M1.4 — Codex: confirm `.cd()` covers cwd (no code change) + capability-matrix doc update

- [ ] **Step 1:** Confirm `codex_cli` already sets cwd via `.cd()` → `--cd` (`mod.rs:225-228`, `spawn.rs:163-166`). No code change. **Note for future maintainers:** codex satisfies the cwd contract via the `--cd` *argv flag* (codex's own workspace root), NOT `Command::current_dir` — a different mechanism from ClaudeCode/Gemini's OS-level `current_dir`. Do not "fix" codex to also call `current_dir`.
- [ ] **Step 2:** Update `docs/cli-runtime-integration-requirements.md` §2 matrix "Set `cwd`" row: ClaudeCode and Gemini are now ✅ (was ❌), and §6/§7 notes ("blocked on P0.1/P0.2") to "landed". Commit:

```bash
git add docs/cli-runtime-integration-requirements.md
git commit -m "docs: cwd setters landed for ClaudeCode + Gemini (P0 done)"
```

### M1 Done Criteria

- [ ] `check-rust` green (`fmt-check` → `clippy --all-features` → `test --all-features`).
- [ ] New unit tests assert `cmd.as_std().get_current_dir()` is set for ClaudeCode + Gemini.
- [ ] PR opened, CI green, merged.

---

## Milestone M2 — P1: session continuity

**Branch:** `feat/cli-session-continuity`
**Why:** "Same-issue runs reuse the same session" is the other load-bearing `AgentRuntime` requirement. Codex has no resume surface and drops `thread.started`'s id; Claude/Gemini mint ids the parser discards. This milestone adds one additive readback channel and Codex resume.

**Design:** New additive `StreamEvent.session_id: Option<String>` (serde-skipped, mirrors `usage`/`stop_reason`) + `StreamEvent::session_started(id)` constructor; new additive `ChatResponse.session_id`. Each provider emits its minted id; Codex gains a `resume` field → `codex exec resume <id>`. Claude already self-mints `--session-id`, so its readback is convenience/symmetry.

### Task M2.1 — Shared types: add `StreamEvent.session_id` + `session_started` + `ChatResponse.session_id` + `collect_stream`

**Files:**
- Modify: `sdks/rust/src/types.rs` — `StreamEvent` struct (726-746), its 11 constructors (748-901), `ChatResponse` (613-623)
- Modify: `sdks/rust/src/stream.rs` — `collect_stream` accumulator + final literal (≈48, ≈124-136)
- Test: `sdks/rust/src/types.rs` `#[cfg(test)]`

- [ ] **Step 1: Write the failing tests** (in `types.rs` test module)

```rust
    #[test]
    fn session_started_constructor_sets_only_session_id() {
        let ev = StreamEvent::session_started("sid-1");
        assert_eq!(ev.session_id.as_deref(), Some("sid-1"));
        assert!(!ev.done);
        assert_eq!(ev.content, "");
        assert!(ev.usage.is_none());
        assert!(ev.stop_reason.is_none());
        assert!(StreamEvent::text("hi").session_id.is_none());
        assert!(StreamEvent::done().session_id.is_none());
    }

    #[test]
    fn stream_event_session_id_is_serde_skipped_when_none() {
        let json = serde_json::to_string(&StreamEvent::text("hi")).unwrap();
        assert!(!json.contains("session_id"), "None session_id must not serialize");
        let json2 = serde_json::to_string(&StreamEvent::session_started("x")).unwrap();
        assert!(json2.contains("\"session_id\":\"x\""));
    }
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test -p motosan-ai --lib types::tests::session_started_constructor_sets_only_session_id types::tests::stream_event_session_id_is_serde_skipped_when_none`
Expected: FAIL to compile — `StreamEvent` has no `session_id`; `session_started` undefined.

- [ ] **Step 3: Add the field, constructor, and ChatResponse field**

`StreamEvent` struct — append after `stop_reason`:

```rust
    /// Provider-minted session / thread id, attached to one event of a CLI turn
    /// when the backend reports one (Claude Code `result.session_id`, Codex
    /// `thread.started.thread_id`, Gemini `init.session_id`). `None` on every
    /// other event and for all HTTP providers. Persist it to resume later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
```

Add `session_id: None,` as the last field of the `Self { ... }` literal in **all 11 existing constructors** (`text`, `done`, `done_with_stop_reason`, `usage`, `tool_call_start`, `tool_call_args`, `tool_call_args_with_id`, `tool_call_end`, `tool_call_end_with_id`, `thinking_delta`, `thinking_done`). The compiler enumerates any you miss (E0063). Then add the new constructor:

```rust
    /// Build a non-terminal event announcing a provider-minted session/thread id.
    /// Emitted once per CLI turn. Carries no text and is not `done`.
    pub fn session_started(id: impl Into<String>) -> Self {
        Self {
            content: String::new(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::Text,
            usage: None,
            stop_reason: None,
            session_id: Some(id.into()),
        }
    }
```

`ChatResponse` struct — append after `stop_reason: StopReason,`:

```rust
    /// Provider-minted session / thread id captured during this turn, when the
    /// backend reports one (CLI providers). `None` for HTTP providers. Persist
    /// to resume the conversation later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
```

`stream.rs::collect_stream`: (a) near the other accumulators add `let mut session_id: Option<String> = None;`; (b) inside the `while let Some(event)` loop, before the `if event.done` block, add `if event.session_id.is_some() { session_id = event.session_id.clone(); }`; (c) in the final `ChatResponse { ... }` literal add `session_id,`. (Note: `collect_stream` is cfg-gated to the HTTP/gemini features and HTTP providers never set `session_id`, so this capture is always `None` there — it exists for symmetry; CLI session readback flows through the provider `stream()` events / `ChatResponse`, not `collect_stream`.)

> **Every `ChatResponse { ... }` construction site must set the new field** (E0063 enumerates them, but ONLY under the right features — see Step 4):
> - `providers/mod.rs` `ChatResponseBuilder::build()` (≈190) — the **single shared literal all HTTP providers return through** (anthropic/openai/gemini/ollama/minimax). Add `session_id: None,`.
> - `stream.rs::collect_stream` final literal — `session_id,` (captured above).
> - the three CLI `chat()` literals (`claude_code/mod.rs` ≈395, `codex_cli/mod.rs` ≈364, `gemini_cli/mod.rs` ≈184) — add `session_id: None,` as a **placeholder** (M2.2–M2.4 replace it with the captured id). These are behind `#[cfg(feature=…)]`, so a no-feature build will NOT flag them.

- [ ] **Step 4: Run — expect pass**

Run: `cargo test -p motosan-ai --lib types:: stream::`
Then build **with the CLI features** so the gated `chat()` literals are actually compiled and checked: `cargo build -p motosan-ai --features claude-code,codex-cli,gemini-cli`.
Expected: PASS / clean build. (A plain `cargo build -p motosan-ai` with no features will NOT catch the three CLI literals.)

- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/types.rs sdks/rust/src/stream.rs sdks/rust/src/providers/
git commit -m "feat(types): additive session_id on StreamEvent + ChatResponse for CLI session readback"
```

### Task M2.2 — Codex: capture `thread.started` id + `exec resume <id>` surface + builder

**Files:**
- Modify: `codex_cli/stream_json.rs` — `CodexStreamEvent` (24-57), `NdjsonAction` (98-113), `parse_ndjson_line` (120-160); fix `ignore_unknown_event` test (240)
- Modify: `codex_cli/spawn.rs` — `SpawnConfig` (76-106), `build_command` (225-236), `push_exec_subcommand` (new, ≈203), `invoke_cli`/`parse_collected_stream` (251-332), `empty_config` (338-355)
- Modify: `codex_cli/mod.rs` — struct (100-164), `with_path` (182-199), builder (≈312), `build_spawn_config` (318-335), `chat` (355-372), `stream()` (407-465), module docs (46-49)

- [ ] **Step 1: Write the failing tests**

`codex_cli/stream_json.rs` test module:

```rust
    #[test]
    fn thread_started_captures_thread_id() {
        let line = r#"{"type":"thread.started","thread_id":"th_abc123"}"#;
        match parse_ndjson_line(line).expect("thread.started should now parse") {
            NdjsonAction::SessionStarted(event) => {
                assert_eq!(event.session_id.as_deref(), Some("th_abc123"));
                assert!(!event.done);
                assert_eq!(event.content, "");
            }
            _ => panic!("expected SessionStarted action"),
        }
    }

    #[test]
    fn thread_started_without_id_is_ignored() {
        assert!(parse_ndjson_line(r#"{"type":"thread.started"}"#).is_none());
    }
```

`codex_cli/spawn.rs` test module:

```rust
    #[test]
    fn resume_inserts_exec_resume_subcommand() {
        let cfg = SpawnConfig { resume: Some("th_abc123".to_string()), ..empty_config() };
        let argv: Vec<String> = build_command(&cfg).as_std()
            .get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&argv[0..4], &["exec", "resume", "th_abc123", "--json"]);
        assert_eq!(argv.last().map(String::as_str), Some("-"));
    }

    #[test]
    fn no_resume_keeps_bare_exec() {
        let argv: Vec<String> = build_command(&empty_config()).as_std()
            .get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(&argv[0..3], &["exec", "--json", "--skip-git-repo-check"]);
        assert!(!argv.iter().any(|a| a == "resume"));
    }

    #[test]
    fn parse_collected_stream_captures_thread_id() {
        let raw = concat!(
            r#"{"type":"thread.started","thread_id":"th_xyz"}"#, "\n",
            r#"{"type":"item.completed","item":{"id":"i1","type":"agent_message","text":"pong"}}"#, "\n",
            r#"{"type":"turn.completed"}"#, "\n",
        );
        let (content, _thinking, _usage, session_id) = parse_collected_stream(raw).expect("parse");
        assert_eq!(content, "pong");
        assert_eq!(session_id.as_deref(), Some("th_xyz"));
    }
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test --features codex-cli -p motosan-ai codex_cli:: -- thread_started resume_inserts no_resume parse_collected_stream_captures`
Expected: FAIL to compile — no `SessionStarted` variant, no `resume` field, `parse_collected_stream` returns a 3-tuple.

- [ ] **Step 3: stream_json — model + capture the thread id**

`CodexStreamEvent` — add before `#[serde(other)] Other`:

```rust
    /// Emitted once at the start of a turn. Carries the persistent thread id
    /// used to resume via `codex exec resume <thread_id>`.
    #[serde(rename = "thread.started")]
    ThreadStarted {
        #[serde(default)]
        thread_id: Option<String>,
    },
```

`NdjsonAction` — add after `Text(StreamEvent)`:

```rust
    /// The turn announced its persistent thread id (`thread.started`),
    /// already converted to a [`StreamEvent::session_started`] event.
    SessionStarted(StreamEvent),
```

`parse_ndjson_line` — add an arm before `CodexStreamEvent::Other => None`:

```rust
        CodexStreamEvent::ThreadStarted { thread_id } => thread_id
            .filter(|id| !id.is_empty())
            .map(|id| NdjsonAction::SessionStarted(StreamEvent::session_started(id))),
```

> **Fix the existing `ignore_unknown_event` test (≈240)**: it uses `thread.started` as its "unknown" fixture. Repoint it to a still-unmodeled type, e.g. `{"type":"item.started"}`, or it will now return `Some` and fail.

- [ ] **Step 4: spawn — resume subcommand + capture on blocking path**

`SpawnConfig` — add after `pub local_provider: Option<LocalProvider>,`:

```rust
    /// Thread id to resume, forwarded as the `resume <id>` subcommand of
    /// `codex exec` (`codex exec resume <id> --json ...`). Blank → fresh thread.
    pub resume: Option<String>,
```

Add the shared subcommand helper near `apply_common_args`:

```rust
/// Push the `exec` subcommand (plus `resume <id>` when resuming) onto `cmd`.
/// Shared by the blocking and streaming spawn sites so the surface is identical.
pub(crate) fn push_exec_subcommand(cmd: &mut Command, config: &SpawnConfig) {
    cmd.arg("exec");
    if let Some(ref id) = config.resume {
        let trimmed = id.trim();
        if !trimmed.is_empty() {
            cmd.arg("resume");
            cmd.arg(trimmed);
        }
    }
}
```

`build_command` — replace the `cmd.arg("exec")` line with the helper:

```rust
    let mut cmd = Command::new(&config.binary_path);
    push_exec_subcommand(&mut cmd, config);
    cmd.arg("--json").arg("--skip-git-repo-check");
    apply_common_args(&mut cmd, config);
    // ... rest unchanged ...
```

`invoke_cli` return type → `Result<(String, Option<String>, Usage, Option<String>), MotosanError>` (content, thinking, usage, session_id). `parse_collected_stream` — change signature + capture:

```rust
fn parse_collected_stream(
    raw: &str,
) -> Result<(String, Option<String>, Usage, Option<String>), MotosanError> {
    let mut agent_messages: Vec<String> = Vec::new();
    let mut session_id: Option<String> = None;
    let mut usage = Usage { input_tokens: 0, output_tokens: 0, cache_creation_input_tokens: None, cache_read_input_tokens: None };
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() { continue; }
        match stream_json::parse_ndjson_line(line) {
            Some(NdjsonAction::Text(event)) => agent_messages.push(event.content),
            Some(NdjsonAction::SessionStarted(event)) => {
                if session_id.is_none() { session_id = event.session_id; }
            }
            Some(NdjsonAction::Done { usage: Some(event), .. }) => {
                if let Some(collected) = event.usage { usage = collected; }
            }
            Some(NdjsonAction::Done { usage: None, .. }) => {}
            Some(NdjsonAction::Error(msg)) => {
                return Err(MotosanError::ProviderError(format!("codex CLI: {msg}")));
            }
            None => {}
        }
    }
    let content = agent_messages.pop().unwrap_or_default();
    let thinking = if agent_messages.is_empty() { None } else { Some(agent_messages.join("\n\n")) };
    Ok((content, thinking, usage, session_id))
}
```

`empty_config()` — add `resume: None,`.

> **Compiler carry-over (do not skip — these are E0063/E0308 build-breakers):**
> - Add `resume: None,` to the `common_args_full_loadout_order_is_stable` `SpawnConfig` literal (≈`spawn.rs:592`; it has no `..empty_config()` spread).
> - `parse_collected_stream` went 3-tuple → 4-tuple. Update the **three** existing tests that destructure it — `last_agent_message_is_content_rest_is_thinking` (≈665), `single_agent_message_has_no_thinking` (≈684), `parse_collected_stream_ignores_blank_lines` (≈702) — to `let (content, thinking, usage, _session_id) = parse_collected_stream(raw)?;` (use `_` for unused trailing elements).
> - `invoke_cli`'s return type also widened; update its caller in `chat()` (Step 5) to bind the 4th element.

- [ ] **Step 5: mod — provider field, builder, chat/stream wiring, doc**

`CodexCliProvider` struct — add after `pub local_provider: Option<LocalProvider>,`:

```rust
    /// Thread id to resume. When set, each turn runs `codex exec resume <id>`
    /// instead of starting a fresh thread. Capture from [`ChatResponse::session_id`]
    /// or a streamed [`StreamEvent::session_id`]. Blank → new thread.
    pub resume: Option<String>,
```

`with_path` defaults literal — add `resume: None,`. Builder after `local_provider`:

```rust
    /// Resume a previous Codex thread (`codex exec resume <id>`).
    pub fn resume(mut self, thread_id: impl Into<String>) -> Self {
        self.resume = Some(thread_id.into());
        self
    }
```

`build_spawn_config` — add `resume: self.resume.clone(),`. `chat()` — capture the 4th tuple element:

```rust
    let (content, thinking, usage, session_id) = spawn::invoke_cli(&config, &composed).await?;
    Ok(ChatResponse {
        content,
        thinking,
        tool_calls: vec![],
        model: config.model.unwrap_or_default(),
        usage,
        stop_reason: StopReason::EndTurn,
        session_id,
    })
```

`stream()` spawn site — replace the bare `exec` lines with the helper:

```rust
    let mut cmd = Command::new(&config.binary_path);
    spawn::push_exec_subcommand(&mut cmd, &config);
    cmd.arg("--json").arg("--skip-git-repo-check");
    spawn::apply_common_args(&mut cmd, &config);
    cmd.arg("-");
```

`stream()` loop — add after the `Text` arm:

```rust
                        stream_json::NdjsonAction::SessionStarted(event) => {
                            yield event;
                        }
```

Module docs (46-49) — replace the "resume is out of scope" note:

```rust
//! `codex exec` (one-shot) and `codex exec resume <thread_id>` (continue a prior
//! thread, via [`CodexCliProvider::resume`]) are supported. The thread id is
//! captured from `thread.started` and surfaced on [`ChatResponse::session_id`] /
//! [`StreamEvent::session_id`]. The `review` subcommand is still out of scope.
```

- [ ] **Step 6: Run — expect pass**

Run: `cargo test --features codex-cli -p motosan-ai codex_cli::`
Expected: PASS (including the repointed `ignore_unknown_event`).

- [ ] **Step 7: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/codex_cli/
git commit -m "feat(codex_cli): capture thread.started id + exec resume <id> session continuity"
```

### Task M2.3 — Claude: read back `result.session_id`

**Files:**
- Modify: `claude_code/stream_json.rs` — `ClaudeStreamEvent::Result` (19-29), `NdjsonAction` (60-66), `parse_ndjson_line` Result arm (91-104); fix `parse_result_with_usage`/`parse_result_without_usage` destructuring
- Modify: `claude_code/mod.rs` — `stream()` loop (466-479), `chat()` (393-402); `spawn::invoke_cli`/`parse_agent_json` for blocking readback

- [ ] **Step 1: Write the failing test** (`claude_code/stream_json.rs`)

```rust
    #[test]
    fn result_event_surfaces_session_id() {
        let line = r#"{"type":"result","result":"done","session_id":"sess_99"}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::Result { session_id, .. } => {
                assert_eq!(session_id.as_deref(), Some("sess_99"));
            }
            _ => panic!("expected Result action"),
        }
    }
```

- [ ] **Step 2: Run — expect failure** (`NdjsonAction::Result` has no `session_id`).

Run: `cargo test --features claude-code -p motosan-ai claude_code::stream_json::tests::result_event_surfaces_session_id`

- [ ] **Step 3: Implement**

`ClaudeStreamEvent::Result` — add `#[serde(default)] session_id: Option<String>,`. `NdjsonAction::Result` — add `session_id: Option<String>,`. `parse_ndjson_line` Result arm:

```rust
        ClaudeStreamEvent::Result { usage, session_id, .. } => {
            let usage_event = usage.map(|u| StreamEvent::usage(Usage {
                input_tokens: u.input_tokens.unwrap_or(0) as u32,
                output_tokens: u.output_tokens.unwrap_or(0) as u32,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }));
            Some(NdjsonAction::Result {
                usage: usage_event,
                done: StreamEvent::done(),
                session_id: session_id.filter(|s| !s.is_empty()),
            })
        }
```

> Update `parse_result_with_usage` / `parse_result_without_usage` to destructure `{ usage, done, .. }`.

`mod.rs::stream()` Result arm — yield the id first:

```rust
                        stream_json::NdjsonAction::Result { usage, done, session_id } => {
                            if let Some(id) = session_id {
                                yield crate::types::StreamEvent::session_started(id);
                            }
                            if let Some(usage_event) = usage { yield usage_event; }
                            yield done;
                            break;
                        }
```

Blocking `chat()`: extend `spawn::parse_agent_json` to also return the top-level `session_id` (`v.get("session_id").and_then(|s| s.as_str()).map(str::to_string)`) and thread it out of `invoke_cli` (return `(String, Usage, Option<String>)`); non-agent text path returns `None`. Then `chat()` sets `session_id` on the `ChatResponse`.

> **Acceptable simplification:** if you keep `invoke_cli`'s 2-tuple, leave Claude's blocking `session_id` as `None` (it self-mints `--session-id`, so the caller already holds the id; the stream path is the load-bearing readback). Pick one and keep it consistent — the M2.1 `ChatResponse.session_id` field defaults to `None`.

- [ ] **Step 4: Run — expect pass.** `cargo test --features claude-code -p motosan-ai claude_code::`
- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/claude_code/
git commit -m "feat(claude_code): surface result.session_id on stream path"
```

### Task M2.4 — Gemini: read back `init.session_id`; document `resume(id)` for a specific session

**Files:**
- Modify: `gemini_cli/stream_json.rs` — `GeminiStreamEvent::Init` (26-27), `NdjsonAction` (71-83), `parse_ndjson_line` (93-125); fix `skip_init_event` test (165)
- Modify: `gemini_cli/spawn.rs` — `parse_collected_stream` (246-278), `invoke_cli` (203-239); fix the two destructuring tests (551, 582)
- Modify: `gemini_cli/mod.rs` — `chat()` (176-192), `stream()` loop (246-265), `resume` doc (154-159)

- [ ] **Step 1: Write the failing tests** (`gemini_cli/stream_json.rs`)

```rust
    #[test]
    fn init_event_captures_session_id() {
        let line = r#"{"type":"init","session_id":"sess_42","model":"auto-gemini-3"}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::SessionStarted(event) => assert_eq!(event.session_id.as_deref(), Some("sess_42")),
            _ => panic!("expected SessionStarted"),
        }
    }

    #[test]
    fn init_event_without_session_id_is_ignored() {
        assert!(parse_ndjson_line(r#"{"type":"init","model":"auto-gemini-3"}"#).is_none());
    }
```

- [ ] **Step 2: Run — expect failure.** `cargo test --features gemini-cli -p motosan-ai gemini_cli::stream_json::tests::init_event_captures_session_id gemini_cli::stream_json::tests::init_event_without_session_id_is_ignored`

- [ ] **Step 3: Implement**

`GeminiStreamEvent::Init` — model the field:

```rust
    #[serde(rename = "init")]
    Init {
        #[serde(default)]
        session_id: Option<String>,
    },
```

`NdjsonAction` — add after `Text`:

```rust
    /// The `init` event announced the session id used by `--resume <id>`.
    SessionStarted(StreamEvent),
```

`parse_ndjson_line` — add an explicit `Init` arm (after the `Message` arm, before the catch-all):

```rust
        GeminiStreamEvent::Init { session_id } => session_id
            .filter(|s| !s.is_empty())
            .map(|id| NdjsonAction::SessionStarted(StreamEvent::session_started(id))),
```

> **Fix `skip_init_event` (165)**: its fixture already has `"session_id":"abc"`, so it must now expect `Some(NdjsonAction::SessionStarted(_))`; `init_event_without_session_id_is_ignored` replaces its "returns None" role.

`spawn.rs::parse_collected_stream` → `Result<(String, Usage, Option<String>), MotosanError>`. The new `SessionStarted` variant makes its `match` **non-exhaustive** (E0004) — add the arm explicitly:

```rust
            Some(NdjsonAction::SessionStarted(event)) => {
                if session_id.is_none() { session_id = event.session_id; }
            }
```

`invoke_cli` signature → 3-tuple. Update the two existing destructuring tests `parse_collected_stream_accumulates_deltas_and_usage` (≈551) and `parse_collected_stream_ignores_blank_lines` (≈582) to `(content, usage, _session_id)`. (The first test's `init` fixture carries `session_id:"s"`, which now parses to `SessionStarted` — fine for the collector.)

`mod.rs::chat()` — capture and set `session_id`. `stream()` loop — add `Some(stream_json::NdjsonAction::SessionStarted(event)) => { yield event; }`. Extend the `resume` doc to note it forwards a concrete captured session id verbatim to `--resume <value>` (not just `latest`/index).

> **P1.3 caveat:** whether the `gemini` CLI actually *resumes by an arbitrary session id* (vs only `latest`/numeric index) is an **unverified behavioral assumption** — the flag forwarding is correct, but a doc change can't prove the CLI honors it. Treat P1.3 as satisfied only once a live `gemini --resume <session_id>` run confirms it; add this to M2.5's `#[ignore]` live test, or soften the doc wording until then.

- [ ] **Step 4: Run — expect pass.** `cargo test --features gemini-cli -p motosan-ai gemini_cli::`
- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/gemini_cli/
git commit -m "feat(gemini_cli): surface init.session_id + document resume(id) for specific session"
```

### Task M2.5 — `#[ignore]` live round-trip test (Codex resume) + matrix doc

- [ ] **Step 1:** Add an `#[ignore]` integration test (mirroring existing `integration_*` tests) that does a Codex turn, captures `ChatResponse.session_id`, runs a second turn with `.resume(that_id)`, and asserts success. This is the only check that pins the real `codex exec resume` flag name against a CLI rename. Document the run command: `cargo test --features codex-cli -p motosan-ai -- --ignored codex_resume_roundtrip`.
- [ ] **Step 2:** Update `docs/cli-runtime-integration-requirements.md` §2 matrix "Session continuity" row (Codex ❌→✅ resume; all three now surface ids) and §6/§7. Commit.

### M2 Done Criteria

- [ ] `check-rust` green (`test --all-features` is what compiles the feature-gated CLI `chat()` literals — a no-feature `cargo build` does NOT catch them). Also confirm `cargo build -p motosan-ai --features claude-code,codex-cli,gemini-cli` is clean.
- [ ] Unit tests prove id capture for all three providers + Codex `exec resume` argv.
- [ ] `#[ignore]` Codex resume round-trip documented in Done Criteria.
- [ ] PR opened, CI green, merged.

---

## Milestone M2.5 — Fallible stream: `BoxStream` Item → `Result` (BREAKING → 0.20)

**Branch:** `feat/fallible-stream`
**Why:** Today `BoxStream = Stream<Item = StreamEvent>` has no slot for an error, so **every** provider silently swallows mid-stream transport/parse errors (`Err(_) => continue` in the HTTP `poll_next` impls; `break`/bare `done()` in the CLI `stream!` loops) and the stream just ends. A consumer (especially CliRuntime) can't tell "finished" from "died". This makes the item fallible so errors surface as `Err`, **compiler-enforced** — and **replaces** M5's earlier CLI-only `done_error` plan.

**Scope:** the stream item type, all provider stream impls, `collect_stream`, the two stream wrappers, `Client`, every consumer/test. **Breaking public-API change → release as 0.20.**

> The HTTP providers already produce `Item = Result<…>` internally and discard the error at the boundary; this just stops discarding it (see §0.5). Error variants already exist: `MotosanError::Stream(String)` (`error.rs`) and `MotosanError::StreamReadTimeout(u64)` (`error.rs:19`).

### Task M2.5.1 — Stream item type + `collect_stream` signature

**Files:** `sdks/rust/src/stream.rs` (`BoxStream` :5, `collect_stream` :29, test helpers :167/180/197).

- [ ] **Step 1: change the alias** (`stream.rs:5`), adding `use crate::error::MotosanError;` if needed:

```rust
pub type BoxStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, MotosanError>> + Send>>;
```

- [ ] **Step 2: `collect_stream` becomes fallible.** Signature → `pub async fn collect_stream(mut stream: BoxStream) -> Result<crate::types::ChatResponse, MotosanError>`. Unwrap each item, propagating `Err`; the final literal is wrapped in `Ok`:

```rust
    while let Some(item) = stream.next().await {
        let event = item?; // a mid-stream provider error aborts collection
        if event.session_id.is_some() { session_id = event.session_id.clone(); }
        if event.done {
            if let Some(reason) = event.stop_reason { explicit_stop_reason = Some(reason); }
            break;
        }
        match event.event_type { /* ...unchanged... */ }
    }
    // ...
    Ok(ChatResponse { content, thinking, model: String::new(), usage: Usage { .. }, stop_reason, session_id, tool_calls })
```

- [ ] **Step 3: test helpers/mocks** that build a `BoxStream` from `Vec<StreamEvent>` must `Ok`-wrap, and `collect_stream(...).await` callers in tests add `.unwrap()`:

```rust
    let stream: BoxStream = Box::pin(iter(events.into_iter().map(Ok)));   // stream.rs:167/180/197 + thinking_collect_tests
```

(The crate won't fully compile until M2.5.2/.3 update the providers — this whole milestone is one PR.)

### Task M2.5.2 — HTTP providers: stop swallowing, yield `Err`

Each HTTP provider's final `impl Stream` has `type Item = StreamEvent` and an `Err(_) => continue` arm that drops transport errors. Flip both.

**Sites (`type Item` + the drop arm):** `anthropic.rs` (:893, :1114) · `openai.rs` (:780, :799) · `gemini.rs` (:434, :530) · `ollama.rs` (:423, :490) · `gemini_code_assist.rs` (:190) · `minimax` routes via anthropic.

- [ ] Per provider: `type Item = StreamEvent;` → `type Item = Result<StreamEvent, MotosanError>;`. Every `return Poll::Ready(Some(<ev>))` → `Poll::Ready(Some(Ok(<ev>)))`. The drop arm:

```rust
    Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(MotosanError::Stream(e.to_string())))),
```

- [ ] **TDD per provider:** feed a mock inner stream whose item is `Err(...)` and assert the outer stream yields `Some(Err(_))` (it previously silently ended).

### Task M2.5.3 — CLI providers: yield `Ok`/`Err` in `stream!` (subsumes old `done_error`)

The three CLI `stream!` loops `yield <StreamEvent>` and, on error, `break`/`yield done()`. Wrap successes in `Ok`; route errors to `Err`.

- [ ] Every `yield <ev>;` in the CLI stream loops → `yield Ok(<ev>);`.
- [ ] **Codex** (`mod.rs` ~457 `NdjsonAction::Error(_msg) => yield done()`) → `yield Err(MotosanError::ProviderError(msg));`. **Gemini** (~257 `Error(_msg) => break`) → `yield Err(MotosanError::ProviderError(msg));`. **Claude** — model the `result` event's `is_error`/`subtype` → a `NdjsonAction::Error(String)` variant and `yield Err(MotosanError::ProviderError(msg));` (this is the old M5.3(a) work, now landing here). Confirm Claude's `is_error` field names against a real binary (was M5.4).
- [ ] Success terminal: `yield Ok(StreamEvent::done_with_stop_reason(StopReason::EndTurn))` for all three (the old M5 `EndTurn` work moves here, since it's the same `stream!` edit).
- [ ] **TDD:** drive the CLI loop with canned ndjson ending in a provider error → assert the stream yields `Err`. (The `drive_lines` extraction + read-timeout stay in M5; here just make the existing loop yield `Result`.)

### Task M2.5.4 — `Client`, the two wrappers, consumers, tests

- [ ] **`ReadTimeoutStream`** (`client.rs:1072`): `type Item` → `Result<StreamEvent, MotosanError>`; pass items through (reset the deadline on any `Ok`/`Err` item); on deadline elapse, yield a real error then end (track a `done: bool` so it doesn't re-fire):

```rust
    Poll::Pending => match self.deadline.as_mut().poll(cx) {
        Poll::Ready(()) => { self.done = true; Poll::Ready(Some(Err(MotosanError::StreamReadTimeout(self.timeout.as_secs())))) }
        Poll::Pending => Poll::Pending,
    },
```

- [ ] **`ThinkStripperStream`** (`client.rs:1112`): `type Item` → `Result<…>`; pass `Err` through untouched, run the strip logic only on `Ok(event)`; `pending` becomes `Option<Result<StreamEvent, MotosanError>>` (or wrap on emit):

```rust
    Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(e))),
    Poll::Ready(Some(Ok(event))) => { /* existing logic; every yielded StreamEvent wrapped in Ok */ }
```

- [ ] **`Client::stream`/`stream_with`** (`client.rs:107/117`): outer signature unchanged (`Result<BoxStream, _>` — the outer `Result` is *creation* errors; only the item type changed). `wrap_with_think_stripper`/`dispatch_stream` pass `BoxStream` through. Update the `client.rs` test mocks (`mock` :1161, `collect_until_done` :1165) to `Ok`-wrap and handle `Err`.
- [ ] **TDD:** `Client::stream` over a mock provider that errors mid-stream yields `Some(Err(_))`; `collect_stream` over it returns `Err`.

### Task M2.5.5 — Release as 0.20 + docs

- [ ] Bump `sdks/rust/Cargo.toml` → `0.20.0`. **CHANGELOG (BREAKING):** `BoxStream` item is now `Result<StreamEvent, MotosanError>`; `collect_stream` returns `Result`; mid-stream errors surface as `Err` instead of silently ending. Migration: `while let Some(ev) = s.next().await { … }` → `… { let ev = ev?; … }`. Update `AGENTS.md` / `llms.txt` / `skills/motosan-ai/SKILL.md` stream examples (per the release checklist).
- [ ] Update `docs/cli-runtime-integration-requirements.md` §2 matrix "Stream error surfacing" row → "errors surface as `Err` in the stream (0.20)".

### M2.5 Done Criteria

- [ ] `check-rust` green — the whole crate compiles against the new item type (the compiler enumerates every `Poll::Ready(Some(...))` / `while let Some(ev)` site).
- [ ] Per-provider tests prove a mid-stream error yields `Err` (no longer swallowed); `collect_stream` returns `Err` on a failing stream.
- [ ] Version bumped to `0.20.0`; CHANGELOG BREAKING entry; stream examples in AGENTS/llms/SKILL updated.
- [ ] PR opened, CI green, merged.

---

## Milestone M3 — P2.1: env injection (per-run secret bundle)

**Branch:** `feat/cli-env-injection`
**Why:** v1's `SecretResolver → ctx.secrets` model needs a way to hand a per-run secret to the child. Today the child only inherits the parent env; there is no SDK injection path.

**Design:** `envs` provider-builder field (a `RedactedEnvs` newtype — see §0.4) + `.env(k,v)`/`.envs(iter)` builders, threaded into each `SpawnConfig` as a plain `Vec<(String,String)>`, applied via `cmd.envs(...)` at **all six** spawn sites. `cmd.envs(&config.envs)` does **not** compile — use `cmd.envs(config.envs.iter().map(|(k, v)| (k, v)))`.

### Task M3.1 — Shared `RedactedEnvs` newtype

**Files:**
- Create: `sdks/rust/src/providers/redacted_envs.rs`
- Modify: `sdks/rust/src/providers/mod.rs` — declare the module. It MUST be **`pub`** (the type is a `pub` field on the public, re-exported provider structs; `pub(crate)` → **E0446 private-in-public**, won't compile). Feature-gate it to match the existing CLI-module style:

```rust
#[cfg(any(feature = "claude-code", feature = "codex-cli", feature = "gemini-cli"))]
pub mod redacted_envs;
```

- [ ] **Step 1: Write the failing test** (in the new file's `#[cfg(test)]`)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_values_keeps_count() {
        let mut e = RedactedEnvs::default();
        e.push("API_KEY", "sk-super-secret");
        e.push("FOO", "bar");
        let dbg = format!("{e:?}");
        assert!(!dbg.contains("sk-super-secret"), "must not leak value: {dbg}");
        assert!(dbg.contains("<2 redacted>"), "got: {dbg}");
        assert_eq!(e.to_vec(), vec![
            ("API_KEY".to_string(), "sk-super-secret".to_string()),
            ("FOO".to_string(), "bar".to_string()),
        ]);
    }
}
```

- [ ] **Step 2: Run — expect failure** (module/type does not exist).

Run: `cargo test -p motosan-ai --lib providers::redacted_envs::tests::debug_redacts_values_keeps_count`

- [ ] **Step 3: Implement the newtype**

```rust
//! A secret-bearing env-var collection whose `Debug` never prints values.

/// Ordered environment variables injected into a spawned CLI subprocess.
///
/// Values are **secrets** (e.g. API keys). `Debug` renders only the key count
/// (`<N redacted>`), so providers can keep `#[derive(Debug)]` without leaking.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct RedactedEnvs(Vec<(String, String)>);

impl RedactedEnvs {
    /// Append one variable (repeatable; OS env semantics decide duplicate keys).
    pub fn push(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.0.push((key.into(), value.into()));
    }

    /// Replace all variables from an iterator of `(key, value)` pairs.
    pub fn replace_from<I, K, V>(&mut self, vars: I)
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.0 = vars.into_iter().map(|(k, v)| (k.into(), v.into())).collect();
    }

    /// Owned clone of the pairs (for threading into `SpawnConfig` / `Command::envs`).
    pub fn to_vec(&self) -> Vec<(String, String)> {
        self.0.clone()
    }
}
// NOTE: only `push`/`replace_from`/`to_vec` are added — they are the only methods
// the wiring calls. Do NOT add `as_slice`/`len`/`is_empty`: nothing calls them and
// `check-rust`'s `clippy --all-features -- -D warnings` fails on the dead_code lint.
// `Debug` reads `self.0.len()` via field access, so no `len()` method is needed.

impl std::fmt::Debug for RedactedEnvs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<{} redacted>", self.0.len())
    }
}
```

- [ ] **Step 4: Run — expect pass.** `cargo test -p motosan-ai --lib providers::redacted_envs::`
- [ ] **Step 5: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/redacted_envs.rs sdks/rust/src/providers/mod.rs
git commit -m "feat(providers): add RedactedEnvs newtype (Debug-redacting secret env collection)"
```

### Task M3.2 — Wire `envs` into each provider (ClaudeCode, Codex, Gemini)

> Do all three providers as one PR. They are structurally identical: provider field (`RedactedEnvs`), `.env()`/`.envs()` builders, default in `new()`/`with_path()`, thread `self.envs.to_vec()` into `SpawnConfig.envs: Vec<(String,String)>`, apply at both spawn sites, update `empty_config` + the `*_full_loadout_*` literal. Below is ClaudeCode in full; Codex/Gemini follow the same shape at their own anchors.

**Files (ClaudeCode):**
- Modify: `claude_code/mod.rs` — struct (≈128), `new()` (≈161), builders (≈350), `build_spawn_config` (≈381)
- Modify: `claude_code/spawn.rs` — `SpawnConfig` (≈161), `build_command` (M1.1) + `stream()` site (`mod.rs` ≈418), `empty_config` (≈494), `full_loadout` literal (≈875)

- [ ] **Step 1: Write the failing tests** (per provider; ClaudeCode shown)

`claude_code/spawn.rs`:

```rust
    #[test]
    fn build_command_injects_envs() {
        let cfg = SpawnConfig {
            envs: vec![("ANTHROPIC_API_KEY".to_string(), "sk-secret".to_string())],
            ..empty_config()
        };
        let std_cmd = build_command(&cfg);
        let std_cmd = std_cmd.as_std();
        let found = std_cmd.get_envs().any(|(k, v)| {
            k.to_string_lossy() == "ANTHROPIC_API_KEY"
                && v.map(|v| v.to_string_lossy().into_owned()) == Some("sk-secret".to_string())
        });
        assert!(found, "env must be injected");
        let argv: Vec<String> = std_cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(!argv.iter().any(|a| a.contains("sk-secret")), "secret must not leak into argv");
    }
```

`claude_code/mod.rs`:

```rust
    #[test]
    fn env_builder_threads_and_debug_redacts() {
        let p = ClaudeCodeProvider::new().env("ANTHROPIC_API_KEY", "sk-super-secret");
        assert_eq!(p.build_spawn_config(None, None).envs, vec![
            ("ANTHROPIC_API_KEY".to_string(), "sk-super-secret".to_string())
        ]);
        let dbg = format!("{p:?}");
        assert!(!dbg.contains("sk-super-secret"), "Debug leaked secret: {dbg}");
        assert!(dbg.contains("<1 redacted>"), "got: {dbg}");
    }
```

- [ ] **Step 2: Run — expect failure** (no `envs`/`env`/`envs()`).

Run: `cargo test --features claude-code -p motosan-ai claude_code:: -- env`

- [ ] **Step 3: Implement (ClaudeCode)**

`mod.rs` — import: `use crate::providers::redacted_envs::RedactedEnvs;`. Struct field after `max_budget_usd`:

```rust
    /// Extra environment variables injected into the spawned `claude` child,
    /// in insertion order. Use for a per-run secret bundle (e.g. ANTHROPIC_API_KEY)
    /// without mutating the parent environment. Values are secrets (redacted in Debug).
    pub envs: RedactedEnvs,
```

`new()` after `max_budget_usd: None,`: `envs: RedactedEnvs::default(),`. (The `RedactedEnvs` field keeps `#[derive(Debug, Clone)]` working — no change to the derive.) Builders after the `max_budget_usd` builder:

```rust
    /// Inject one environment variable into the spawned subprocess (repeatable).
    /// The value is a secret and is never logged.
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.envs.push(key, value);
        self
    }

    /// Replace the full set of injected environment variables.
    pub fn envs<I, K, V>(mut self, vars: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.envs.replace_from(vars);
        self
    }
```

`build_spawn_config` after `max_budget_usd: self.max_budget_usd,`: `envs: self.envs.to_vec(),`.

`spawn.rs` — `SpawnConfig` field after `max_budget_usd`:

```rust
    /// Extra environment variables for the spawned subprocess, applied via
    /// `Command::envs`. Carries per-run secrets; do not log values.
    pub envs: Vec<(String, String)>,
```

Inside `build_command` (after any `current_dir` line):

```rust
    cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
```

`mod.rs::stream()` site — after the `current_dir` block:

```rust
        cmd.envs(config.envs.iter().map(|(k, v)| (k, v)));
```

`empty_config()` after `max_budget_usd: None,`: `envs: Vec::new(),`. `full_loadout` literal: `envs: Vec::new(),`.

- [ ] **Step 4: Repeat for Codex and Gemini**

Same six edits each, at:
- **Codex** `mod.rs`: field after `local_provider` (≈163); default in `with_path` after `local_provider: None,` (≈197); builders after the `local_provider` builder (≈312); `build_spawn_config` after `local_provider: self.local_provider,` (≈333). `spawn.rs`: `SpawnConfig.envs` after `local_provider` (≈105); `cmd.envs(...)` inside `build_command` and at the `stream()` site (`mod.rs` ≈410); `empty_config` after `local_provider: None,` (≈353); `full_loadout` literal after `config_overrides: vec![...]` (≈606).
- **Gemini** `mod.rs`: field after `resume` (≈60); default in `new()` after `resume: None,` (≈80); builders after the `resume` builder (≈159); `build_spawn_config` after `resume: self.resume.clone(),` (≈171). `spawn.rs`: `SpawnConfig.envs` after `resume` (≈92); `cmd.envs(...)` inside `build_command` and at the `stream()` site (`mod.rs` ≈209); `empty_config` after `resume: None,` (≈294); `full_loadout` literal after `resume: Some("latest".to_string()),` (≈506).

(`envs` is not an argv flag, so no `common_args_*` assertion changes.)

- [ ] **Step 5: Run — expect pass** (all three features)

Run: `cargo test --features claude-code,codex-cli,gemini-cli -p motosan-ai -- env`
Expected: PASS.

- [ ] **Step 6: `cargo fmt` and commit**

```bash
cargo fmt
git add sdks/rust/src/providers/
git commit -m "feat(cli): per-run env injection (.env/.envs) across all three CLI providers"
```

### M3 Done Criteria

- [ ] `check-rust` green.
- [ ] Per provider: `get_envs()` proves injection; `Debug` of a provider with a secret env contains `<N redacted>` and not the value; secret not in argv.
- [ ] PR opened, CI green, merged.

---

## Milestone M4 — P2.2: tool-call stream events

**Branch:** `feat/cli-tool-call-events`
**Why:** All three providers drop tool-use wire events, so a runtime cannot observe/gate tool use. `StreamEvent` already models tool calls (`tool_call_start/args_with_id/end_with_id`) — **no public-API change**; this is pure provider-side NDJSON parsing.

**Design:** add `NdjsonAction::ToolCalls(Vec<StreamEvent>)` to each provider; parse `tool_use` (Claude) / `command_execution`+`mcp_tool_call` (Codex) / `tool_call` (Gemini) into a `start → args_with_id → end_with_id` triplet; yield them in `stream()`; blocking paths explicitly ignore the variant (`ChatResponse.tool_calls` stays empty on CLI backends).

> **Post-M2.5:** the CLI stream item is `Result<StreamEvent, MotosanError>`, so every tool-event yield below is `yield Ok(event);` (not `yield event;`).

> **Confidence:** ClaudeCode wire shape is verified (fixture at `stream_json.rs:206` + `anthropic.rs:996-1008`). **Codex and Gemini wire shapes are inferred** (no captured fixture) — the serde models are conservative (all `Option`/defaulted) so a mismatch degrades to today's drop, never a crash. **Task M4.4 captures real transcripts to confirm before merge.**

### Task M4.1 — ClaudeCode: surface `tool_use` blocks (verified)

**Files:** `claude_code/stream_json.rs` (`AssistantContentBlock` 40-51, `NdjsonAction` 59-66, Assistant arm 76-90), `claude_code/mod.rs` (`stream()` loop 466-479).

- [ ] **Step 1: Failing tests**

```rust
    #[test]
    fn tool_use_block_yields_tool_call_events() {
        use crate::types::StreamEventType;
        let line = r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_01","name":"Read","input":{"path":"/tmp/x"}}]}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event_type, StreamEventType::ToolCallStart);
        assert_eq!(events[0].tool_call_id.as_deref(), Some("toolu_01"));
        assert_eq!(events[0].tool_call_name.as_deref(), Some("Read"));
        assert_eq!(events[1].event_type, StreamEventType::ToolCallArgs);
        assert_eq!(events[1].tool_call_args_delta.as_deref(), Some(r#"{"path":"/tmp/x"}"#));
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
        assert_eq!(events[2].tool_call_id.as_deref(), Some("toolu_01"));
    }

    #[test]
    fn assistant_text_only_still_yields_text() {
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"pong"}]}}"#;
        match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::Text(event) => assert_eq!(event.content, "pong"),
            _ => panic!("expected Text"),
        }
    }

    #[test]
    fn assistant_text_and_tool_use_preserves_text() {
        use crate::types::StreamEventType;
        // Mixed turn: narration text + a tool_use block in one content[].
        let line = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"let me read it"},{"type":"tool_use","id":"toolu_2","name":"Read","input":{"path":"/x"}}]}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events[0].event_type, StreamEventType::Text);
        assert_eq!(events[0].content, "let me read it"); // text MUST NOT be dropped
        assert_eq!(events[1].event_type, StreamEventType::ToolCallStart);
        assert_eq!(events[1].tool_call_name.as_deref(), Some("Read"));
    }
```

- [ ] **Step 2: Run — expect failure.** `cargo test --features claude-code -p motosan-ai claude_code::stream_json::tests::tool_use_block_yields_tool_call_events`

- [ ] **Step 3: Implement.** `AssistantContentBlock` — add the `ToolUse` variant:

```rust
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
```

`NdjsonAction` — add `ToolCalls(Vec<StreamEvent>)`. Replace the Assistant arm:

```rust
        ClaudeStreamEvent::Assistant { message } => {
            // A mixed turn interleaves text and tool_use blocks in one content[].
            // Preserve narration text (do NOT drop it) by flushing it as Text
            // events in order, before each tool triplet and for any trailing text.
            let mut events: Vec<StreamEvent> = Vec::new();
            let mut text = String::new();
            let mut had_tool = false;
            for block in &message.content {
                match block {
                    AssistantContentBlock::Text { text: t } => text.push_str(t),
                    AssistantContentBlock::ToolUse { id, name, input } => {
                        if !text.is_empty() {
                            events.push(StreamEvent::text(std::mem::take(&mut text)));
                        }
                        had_tool = true;
                        events.push(StreamEvent::tool_call_start(id.clone(), name.clone()));
                        let args = serde_json::to_string(input).unwrap_or_else(|_| "{}".to_string());
                        events.push(StreamEvent::tool_call_args_with_id(id.clone(), args));
                        events.push(StreamEvent::tool_call_end_with_id(id.clone()));
                    }
                    AssistantContentBlock::Other => {}
                }
            }
            if !text.is_empty() {
                events.push(StreamEvent::text(text)); // trailing / text-only turn
            }
            if had_tool {
                Some(NdjsonAction::ToolCalls(events))
            } else {
                // No tool blocks → at most one Text event (or None).
                events.pop().map(NdjsonAction::Text)
            }
        }
```

> **Behavior note:** unlike the baseline (which only ever yielded text), a tool-using turn now yields tool events; a *mixed* turn yields text **and** tool events in wire order (text is no longer dropped). A pure-text turn is unchanged.

`mod.rs::stream()` loop — add after the `Text` arm:

```rust
                        stream_json::NdjsonAction::ToolCalls(events) => {
                            for event in events {
                                yield Ok(event); // post-M2.5: item is Result
                            }
                        }
```

(The blocking `--output-format json` path has no tool surface — no change there.)

- [ ] **Step 4: Run — expect pass.** `cargo test --features claude-code -p motosan-ai claude_code::`
- [ ] **Step 5: `cargo fmt` and commit** — `feat(claude_code): surface tool_use blocks as tool_call stream events`

### Task M4.2 — Codex: surface `command_execution` / `mcp_tool_call` items (inferred)

**Files:** `codex_cli/stream_json.rs` (`CodexItem` 66-75, `NdjsonAction`, `ItemCompleted` arm 123-134), `codex_cli/mod.rs` (`stream()` loop), `codex_cli/spawn.rs` (`parse_collected_stream` — add ignore arm).

- [ ] **Step 1: Failing tests** (`stream_json.rs`)

```rust
    #[test]
    fn command_execution_item_yields_tool_call_events() {
        use crate::types::StreamEventType;
        let line = r#"{"type":"item.completed","item":{"id":"item_5","type":"command_execution","command":"ls -la"}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events,
            _ => panic!("expected ToolCalls"),
        };
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].tool_call_name.as_deref(), Some("command_execution"));
        assert_eq!(events[1].tool_call_args_delta.as_deref(), Some(r#"{"command":"ls -la"}"#));
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
    }

    #[test]
    fn mcp_tool_call_item_uses_tool_name() {
        let line = r#"{"type":"item.completed","item":{"id":"item_7","type":"mcp_tool_call","tool":"search","arguments":{"q":"rust"}}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events, _ => panic!(),
        };
        assert_eq!(events[0].tool_call_name.as_deref(), Some("search"));
        assert_eq!(events[1].tool_call_args_delta.as_deref(), Some(r#"{"q":"rust"}"#));
    }

    #[test]
    fn non_tool_non_message_item_still_dropped() {
        // The `_ => None` catch-all must still drop any non-tool, non-message
        // subtype. (Repointed off `reasoning`, which the existing
        // `ignore_non_agent_item` test already covers, to broaden coverage.)
        let line = r#"{"type":"item.completed","item":{"id":"item_1","type":"file_changes","text":""}}"#;
        assert!(parse_ndjson_line(line).is_none());
    }
```

- [ ] **Step 2: Run — expect failure.**
- [ ] **Step 3: Implement.** Extend `CodexItem` with `#[serde(default)]` `id: Option<String>`, `tool: Option<String>`, `command: Option<String>`, `arguments: Option<serde_json::Value>`. Add `NdjsonAction::ToolCalls(Vec<StreamEvent>)`. Replace the `ItemCompleted` arm:

```rust
        CodexStreamEvent::ItemCompleted { item } => match item.item_type.as_str() {
            "agent_message" => {
                let text = item.text.unwrap_or_default();
                if text.is_empty() { None } else { Some(NdjsonAction::Text(StreamEvent::text(text))) }
            }
            "command_execution" | "mcp_tool_call" => {
                let id = item.id.clone().unwrap_or_default();
                let name = item.tool.clone().unwrap_or_else(|| item.item_type.clone());
                let args = match item.arguments {
                    Some(ref v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
                    None => match item.command {
                        Some(ref cmd) => serde_json::json!({ "command": cmd }).to_string(),
                        None => "{}".to_string(),
                    },
                };
                Some(NdjsonAction::ToolCalls(vec![
                    StreamEvent::tool_call_start(id.clone(), name),
                    StreamEvent::tool_call_args_with_id(id.clone(), args),
                    StreamEvent::tool_call_end_with_id(id),
                ]))
            }
            _ => None,
        },
```

`mod.rs::stream()` — add the `ToolCalls(events) => for event in events { yield event; }` arm. `spawn.rs::parse_collected_stream` — add `Some(NdjsonAction::ToolCalls(_)) => {}` (exhaustiveness; tool calls are not folded into `ChatResponse`).

- [ ] **Step 4: Run — expect pass.** `cargo test --features codex-cli -p motosan-ai codex_cli::`
- [ ] **Step 5: `cargo fmt` and commit** — `feat(codex_cli): surface command/mcp tool items as tool_call stream events (wire shape inferred)`

### Task M4.3 — Gemini: surface `tool_call` events (inferred)

**Files:** `gemini_cli/stream_json.rs` (`GeminiStreamEvent` 23-52, `NdjsonAction` 71-83, `parse_ndjson_line`), `gemini_cli/mod.rs` (`stream()` loop), `gemini_cli/spawn.rs` (blocking match — add ignore arm if exhaustive).

- [ ] **Step 1: Failing test**

```rust
    #[test]
    fn tool_call_event_yields_tool_call_events() {
        use crate::types::StreamEventType;
        // NOTE: wire shape INFERRED — confirm against a real `gemini -o stream-json` transcript (M4.4).
        let line = r#"{"type":"tool_call","id":"call_1","name":"read_file","args":{"path":"/tmp/x"}}"#;
        let events = match parse_ndjson_line(line).expect("parse") {
            NdjsonAction::ToolCalls(events) => events, _ => panic!(),
        };
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].tool_call_name.as_deref(), Some("read_file"));
        assert_eq!(events[1].tool_call_args_delta.as_deref(), Some(r#"{"path":"/tmp/x"}"#));
        assert_eq!(events[2].event_type, StreamEventType::ToolCallEnd);
    }
```

- [ ] **Step 2: Run — expect failure.**
- [ ] **Step 3: Implement.** Add the `ToolCall` variant to `GeminiStreamEvent` (before `Other`):

```rust
    /// A tool invocation. **Wire shape INFERRED** — confirm against a real
    /// `gemini -o stream-json` transcript before relying on it.
    #[serde(rename = "tool_call")]
    ToolCall {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        args: Option<serde_json::Value>,
    },
```

Add `NdjsonAction::ToolCalls(Vec<StreamEvent>)`. Add a `parse_ndjson_line` arm (after `Message`, before the catch-all):

```rust
        GeminiStreamEvent::ToolCall { id, name, args } => {
            let call_id = id.unwrap_or_default();
            let call_name = name.unwrap_or_else(|| "tool_call".to_string());
            let args_json = match args {
                Some(ref v) => serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string()),
                None => "{}".to_string(),
            };
            Some(NdjsonAction::ToolCalls(vec![
                StreamEvent::tool_call_start(call_id.clone(), call_name),
                StreamEvent::tool_call_args_with_id(call_id.clone(), args_json),
                StreamEvent::tool_call_end_with_id(call_id),
            ]))
        }
```

`mod.rs::stream()` — add the `Some(NdjsonAction::ToolCalls(events)) => for event in events { yield event; }` arm. In `spawn.rs`, **read the blocking collector first**: if it matches `NdjsonAction` exhaustively, add `Some(NdjsonAction::ToolCalls(_)) => {}`; if it uses `if let Some(NdjsonAction::Text(..))`, no change.

- [ ] **Step 4: Run — expect pass.** `cargo test --features gemini-cli -p motosan-ai gemini_cli::`
- [ ] **Step 5: `cargo fmt` and commit** — `feat(gemini_cli): surface tool_call events as tool_call stream events (wire shape inferred)`

### Task M4.4 — Confirm inferred wire shapes + matrix doc (Done-Criteria gate)

- [ ] Capture a real `codex exec --json` tool turn and a real `gemini -p '...' -o stream-json` tool turn. Confirm the `type` strings and sub-field names. **Specifically resolve the singular/plural ambiguity:** the `CodexItem` module doc (`codex_cli/stream_json.rs:62-64`) lists subtypes as `mcp_tool_calls` / `file_changes` (**plural**), but M4.2 matches `"mcp_tool_call"` (**singular**) — if the real per-item `type` is the plural form, the M4.2 arm never matches and events silently drop. Confirm the exact strings and `tool_call` vs `function_call` for Gemini. If they differ, update the serde models (M4.2/M4.3) and the test fixtures. **Do not merge M4 until both are confirmed** (Claude is already verified).
- [ ] Update `docs/cli-runtime-integration-requirements.md` §2 matrix "ToolCall / ToolResult events" row (now surfaced on the stream path; results still dropped — none of the three CLIs emit a separable tool-result event). Note `ChatResponse.tool_calls` stays empty (blocking path unchanged). Update CHANGELOG/AGENTS/llms.txt at release.

### Task M4.5 — Make the CLI terminal `stop_reason` tool-call-aware (M2.5 carry-over)

> **From the M2.5 review:** all three CLI `stream!` loops currently hardcode the terminal to `Ok(done_with_stop_reason(StopReason::EndTurn))`. That was correct pre-M4 (CLI streams emitted no tool calls), but once this milestone surfaces tool-call events, a turn that *ends in a tool call* should report `ToolUse`, not `EndTurn` — otherwise `collect_stream`'s stop-reason heuristic (which infers `ToolUse` when `tool_calls` is non-empty) is overridden by the explicit `EndTurn` and reports the wrong reason.

- [ ] When the turn yielded any `ToolCalls`, emit the terminal as `done_with_stop_reason(StopReason::ToolUse)` (track a `saw_tool_call` flag in the loop), else keep `EndTurn`. Add a test: a stream ending in a tool-call triplet collects to `stop_reason == ToolUse`.

### M4 Done Criteria

- [ ] `check-rust` green; per-provider unit tests assert the 3-event triplet.
- [ ] Codex + Gemini wire shapes confirmed against real transcripts (M4.4) — or the inferred models updated to match.
- [ ] `reasoning`/non-tool items still dropped (regression tests green).
- [ ] CLI terminal reports `ToolUse` when the turn ended in a tool call (M4.5).
- [ ] PR opened, CI green, merged.

---

## Milestone M5 — P2.3: stream robustness

**Branch:** `feat/cli-stream-robustness`
**Why:** timeouts are hardcoded and cover only the blocking path (CLI stream loops are unbounded); cancellation is `kill_on_drop` only. *(Mid-stream **error surfacing** and the **success `stop_reason`** both moved to **M2.5** — after M2.5, CLI streams already `yield Err` on failure and `Ok(done_with_stop_reason(EndTurn))` on success; this milestone no longer adds `done_error`.)*

**Depends on M2.5:** the CLI `stream!` loops already yield `Result<StreamEvent, _>` and the per-line read-timeout below yields `Err(MotosanError::StreamReadTimeout(..))` rather than a sentinel event.

### Task M5.1 — *(removed — folded into M2.5)*

The `done_error` constructor and in-band error surfacing are gone. M2.5 made stream errors first-class `Err` items, so there is nothing to add here. Tasks renumber from M5.2.

### Task M5.2 — Configurable timeout (all three providers)

> Same shape ×3. Provider gets `timeout: Option<Duration>` + `.timeout(dur)` + `.no_timeout()`, defaulting to a new `pub const DEFAULT_TIMEOUT: Duration` in `spawn.rs` (replacing the private `TIMEOUT_SECS`). `SpawnConfig` gets the field; `invoke_cli` uses `config.timeout` (skipping the wrapper when `None`). ClaudeCode default 300s; Codex/Gemini 600s.

**Files (per provider):** `mod.rs` (struct, default, builder, `build_spawn_config`) + `spawn.rs` (`DEFAULT_TIMEOUT` const, `SpawnConfig` field, `invoke_cli` timeout block, `empty_config`).

- [ ] **Step 1: Failing test** (Codex shown; replicate for the other two)

```rust
    #[test]
    fn default_timeout_is_set_and_overridable() {
        use std::time::Duration;
        let p = CodexCliProvider::new();
        assert_eq!(p.timeout, Some(spawn::DEFAULT_TIMEOUT));
        let cfg = p.timeout(Duration::from_secs(5)).build_spawn_config(None);
        assert_eq!(cfg.timeout, Some(Duration::from_secs(5)));
        assert_eq!(CodexCliProvider::new().no_timeout().build_spawn_config(None).timeout, None);
    }
```

> **`build_spawn_config` arity differs by provider** when replicating this test: Codex and Gemini take one arg (`build_spawn_config(None)`); **ClaudeCode takes two** (`build_spawn_config(None, None)` — `request_model, append_system_prompt`). The Codex test above uses the 1-arg form; adjust for Claude.

- [ ] **Step 2: Run — expect failure** (no `timeout` field/builder; `DEFAULT_TIMEOUT` private const).
- [ ] **Step 3: Implement (per provider).** `spawn.rs`: replace `const TIMEOUT_SECS: u64 = N;` with `pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(N);` (ensure `use std::time::Duration;`). **Also update the two stale rustdoc intra-doc links** ``[`TIMEOUT_SECS`]`` → ``[`DEFAULT_TIMEOUT`]`` in `codex_cli/spawn.rs` (≈249) and `gemini_cli/spawn.rs` (≈201) — a dangling intra-doc link warns under `cargo doc` (claude has no such link). Add `pub timeout: Option<Duration>,` to `SpawnConfig` and `timeout: Some(DEFAULT_TIMEOUT),` to `empty_config` (and to the `*_full_loadout_*` literal). Replace the `invoke_cli` timeout block:

```rust
    let result = match config.timeout {
        Some(dur) => tokio::time::timeout(dur, child.wait_with_output())
            .await
            .map_err(|_| MotosanError::ProviderError(format!(
                "codex CLI timed out after {} seconds", dur.as_secs()
            )))?
            .map_err(|e| MotosanError::ProviderError(format!("codex CLI process error: {e}")))?,
        None => child.wait_with_output().await
            .map_err(|e| MotosanError::ProviderError(format!("codex CLI process error: {e}")))?,
    };
```

`mod.rs` (add `use std::time::Duration;`): struct field `pub timeout: Option<Duration>,`; default `timeout: Some(spawn::DEFAULT_TIMEOUT),` in `new()`/`with_path()`; builders:

```rust
    /// Override the per-invocation timeout (applies to `chat()` and the
    /// `stream()` read loop).
    pub fn timeout(mut self, dur: Duration) -> Self {
        self.timeout = Some(dur);
        self
    }

    /// Disable the invocation timeout (run until the child exits).
    pub fn no_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }
```

`build_spawn_config`: `timeout: self.timeout,`. (Anchors: Claude after `max_budget_usd`/`cwd`/`envs`; Codex after `local_provider`; Gemini after `resume` — all field lists have grown across M1–M3, so locate by the last field.)

- [ ] **Step 4: Run — expect pass** (all three features).
- [ ] **Step 5: `cargo fmt` and commit** — `feat(cli): configurable per-invocation timeout (default = prior const) across CLI providers`

### Task M5.3 — Per-line read timeout on the CLI stream loop (+ `drive_lines` extraction)

> After M2.5, the CLI `stream!` loops already yield `Result<StreamEvent, MotosanError>` (`Ok` events; `Err(ProviderError)` on a provider error; `Ok(done_with_stop_reason(EndTurn))` on success). The remaining robustness gap: the read loop is otherwise **unbounded** — a stalled child hangs the consumer forever. Add a per-line read deadline that yields `Err(StreamReadTimeout)` and ends. To test it without a subprocess, extract the loop into a `drive_lines` helper that **owns the child** (`kill_on_drop` would otherwise SIGKILL it the instant `stream()` returns; pass `None` for the child in the test).

**Files:** all three `mod.rs::stream()`.

- [ ] **Step 1: Failing test** (Gemini shown — drives the extracted helper, no subprocess):

```rust
    #[tokio::test]
    async fn stream_stall_yields_timeout_error() {
        use std::time::Duration;
        use tokio::io::BufReader;
        use tokio_stream::StreamExt;
        // Keep the write half (`_w`) alive so the read half stays open and never
        // produces a line → the per-line deadline must fire.
        let (_w, r) = tokio::io::duplex(64);
        let reader = BufReader::new(r);
        let mut s = super::drive_lines(None::<tokio::process::Child>, reader, Some(Duration::from_millis(50)));
        match s.next().await {
            Some(Err(crate::error::MotosanError::StreamReadTimeout(_))) => {}
            other => panic!("expected StreamReadTimeout, got {other:?}"),
        }
    }

    // M2.5 carry-over coverage: the M2.5 review found the CLI loop's error→Err
    // wiring was only tested at the parser level. Now that drive_lines is
    // extractable, assert end-to-end that a provider-error ndjson line becomes a
    // `Some(Err(ProviderError))` STREAM item (Gemini shown; mirror per provider).
    #[tokio::test]
    async fn stream_surfaces_provider_error_as_err_item() {
        use std::io::Cursor;
        use tokio::io::BufReader;
        use tokio_stream::StreamExt;
        let raw = b"{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"partial\",\"delta\":true}\n{\"type\":\"result\",\"status\":\"failed\"}\n";
        let mut s = super::drive_lines(None::<tokio::process::Child>, BufReader::new(Cursor::new(&raw[..])), None);
        let mut last = None;
        while let Some(item) = s.next().await { last = Some(item); }
        assert!(matches!(last, Some(Err(crate::error::MotosanError::ProviderError(_)))),
            "a provider-error line must surface as a terminal Err item, got {last:?}");
    }
```

- [ ] **Step 2: Run — expect failure** (`drive_lines` undefined / no timeout arm).
- [ ] **Step 3: Implement.** Extract each `mod.rs::stream()` inline `async_stream!` body into:

```rust
pub(crate) fn drive_lines<R>(
    mut child: Option<tokio::process::Child>,
    reader: R,
    read_timeout: Option<std::time::Duration>,
) -> crate::stream::BoxStream
where
    R: tokio::io::AsyncBufRead + Unpin + Send + 'static,
{
    Box::pin(async_stream::stream! {
        let mut lines = reader.lines();
        loop {
            let next = match read_timeout {
                Some(dur) => match tokio::time::timeout(dur, lines.next_line()).await {
                    Ok(res) => res,
                    Err(_) => {
                        yield Err(crate::error::MotosanError::StreamReadTimeout(dur.as_secs()));
                        break;
                    }
                },
                None => lines.next_line().await,
            };
            let line = match next {
                Ok(Some(line)) => line.trim().to_string(),
                Ok(None) => break,
                Err(_) => break,
            };
            if line.is_empty() { continue; }
            // ... the per-provider match from M2.5.3: yields Ok(Text/SessionStarted/ToolCalls/...),
            //     Err(ProviderError) on NdjsonAction::Error, Ok(done_with_stop_reason(EndTurn)) on success.
        }
        if let Some(mut c) = child.take() {
            let _ = c.wait().await; // reap; production passes Some(child), test passes None
        }
    })
}
```

Then `stream()` spawns the child, takes `child.stdout`, builds the `BufReader`, and returns `drive_lines(Some(child), reader, config.timeout)`. The child is **moved into** `drive_lines` (reaped at the tail), so `kill_on_drop` does not fire early.

- [ ] **Step 4: Run — expect pass** (all three features).
- [ ] **Step 5: `cargo fmt` and commit** — `feat(cli): per-line read-timeout (StreamReadTimeout) + drive_lines extraction`

### Task M5.4 — Document the cancel contract + matrix doc

- [ ] Add a `# Cancellation` section to each provider's `mod.rs` module doc: no explicit handle; `kill_on_drop(true)` means dropping the `BoxStream`/`chat()` future SIGKILLs and reaps the child; use the `timeout` field to bound runtime (per-line stall deadline → `Err(StreamReadTimeout)`).
- [ ] Update `docs/cli-runtime-integration-requirements.md` §2 matrix "Stop reason" row (CLI success terminals now `EndTurn`, from M2.5) and the timeout bullet (now configurable). ("Stream error surfacing" was already updated in M2.5.)

### M5 Done Criteria

- [ ] `check-rust` green; the stream-stall test yields `Err(StreamReadTimeout)` per provider; `drive_lines` owns the child.
- [ ] Configurable `timeout` + `no_timeout()` tested per provider (M5.2).
- [ ] PR opened, CI green, merged.

---

## Milestone M6 — P2.4: per-request override (OPTIONAL — recommend DEFER)

**Recommendation: defer.** Route (a) (builder fields) + the providers' `#[derive(Clone)]` already let a caller vary `cwd`/`session`/`budget` per call via `provider.clone().cwd(x)` at trivial cost. P2.4 adds a parallel, stringly-typed, silently-ignored-on-typo config surface (`provider_options` keys) that must be kept in sync with every builder field across three providers. **Implement only if a concrete caller needs to vary these on a `&Provider` it does not own and cannot clone.**

If built: read CLI-only keys (`cwd`, `session_id`, `max_budget_usd`) out of `ChatRequest.provider_options` (the existing untyped `Option<Value>`), via a small pure `cli_overrides(&Option<Value>) -> CliOverrides` helper per provider's `spawn.rs`; `build_spawn_config` gains a `&Option<Value>` param and applies `override.or_else(|| self.field.clone())` (request > builder), mirroring the existing `request_model.or_else(...)`. Pass `&request.provider_options` at both call sites (chat + stream). Full code is in the P2.4 draft (`draft:p2-perreq`). TDD: a `provider_options` `cwd` overrides the builder `cwd` in the resulting `SpawnConfig`; precedence-boundary test with `&None` falls back to the builder. **No `ChatRequest` shape change** (no new typed fields) → HTTP providers / `LlmClient` / motosan-chat unaffected.

### M6 Done Criteria (if built)

- [ ] `check-rust` green; override-precedence tests green per provider; recognised keys documented in the builder rustdoc.
- [ ] PR opened, CI green, merged.

---

## Self-Review (run before handing off)

**Spec coverage** (against `docs/cli-runtime-integration-requirements.md` §4):
- P0.1 ClaudeCode cwd → M1.2 ✅ · P0.2 Gemini cwd → M1.3 ✅ · Codex cwd already present → M1.4 ✅
- P1.1 Codex resume + thread_id capture → M2.2 ✅ · P1.2 surface session ids (all three) → M2.1/M2.2/M2.3/M2.4 ✅ · P1.3 Gemini readable session_id → M2.4 ✅
- Fallible stream (errors as `Err`) → **M2.5** ✅ · P2.1 env injection → M3 ✅ · P2.2 tool events → M4 ✅ · P2.3 timeout+success-stop_reason+cancel → M5 (+M2.5) ✅ · P2.4 per-request override → M6 (deferred) ✅
- No-`env` blocker, provider-builder route (a), two-spawn-site application, secret redaction → covered.

**Placeholder scan:** every code step shows complete code; the only intentionally-not-pasted blocks are the repetitive Codex/Gemini variants in M3.2/M5.2 (explicitly enumerated by anchor) and the P2.4 body (deferred, pointer to the draft). No `TODO`/`fill in`/"add error handling" steps.

**Type consistency:** `RedactedEnvs` (M3) → `SpawnConfig.envs: Vec<(String,String)>` via `to_vec()`; `StreamEvent.session_id`/`session_started`/`ChatResponse.session_id` (M2) used consistently by M2.2–M2.4; after **M2.5** the stream item is `Result<StreamEvent, MotosanError>` everywhere (`BoxStream`, `collect_stream → Result`, both wrappers, all 8 provider impls) — errors are `Err`, not a sentinel event; `NdjsonAction::{SessionStarted,ToolCalls,Error}` added additively per provider and matched by name. `DEFAULT_TIMEOUT` replaces `TIMEOUT_SECS` consistently in M5.2.

**Known inference risks carried into the plan:** Codex/Gemini tool-call wire shapes (M4.4 gate), Claude `result.is_error` field names (M5.4), and whether Codex `exec resume`/Gemini `--resume <id>` resume-by-id actually work (M2.5 `#[ignore]` live tests) are confirmed against real binaries before their milestone merges; conservative serde models degrade to drop-not-crash if wrong.

**Adversarial review log (2026-06-09):** a 6-reviewer pass against the real 0.19.0 source found and **fixed** 5 build-breakers and 2 behavior bugs that would have failed `check-rust`: (1) `pub(crate) mod redacted_envs` + `pub` field = E0446 → now `pub mod`, feature-gated (M3.1); (2) `RedactedEnvs::{as_slice,len,is_empty}` dead-code under `-D warnings` → removed (M3.1); (3) `drive_lines(reader,timeout)` couldn't own `child`, so `kill_on_drop` SIGKILLed it instantly → now `drive_lines(child: Option<Child>, …)` (M5.3); (4) codex 4-tuple broke 3 destructuring tests + the `full_loadout` literal (E0308/E0063) → fix steps added (M2.2); (5) M2.1 verified with no features missed the 3 CLI `chat()` literals → enumerated + build `--all-features` (M2.1); (6) claude arm dropped narration text in mixed turns → text-preserving rewrite + mixed test (M4.1); (7) codex success terminal lacked `EndTurn` → added (M5.3). M1 verified build-ready as-is (only nits).

**Design revision (2026-06-10, after M1+M2 merged):** a "is this general?" pass settled two calls (see §0.5). (1) `session_id` on shared types is **kept** — it matches the existing `thinking`/`cache_*_tokens` "provider-specific optional" house style. (2) Stream errors move from the planned CLI-only `done_error` sentinel to a **fallible stream** (`BoxStream` Item → `Result`), because the swallow-and-end behavior was SDK-wide (every HTTP provider does `Err(_) => continue`), so the general, type-enforced fix is to stop swallowing everywhere. This added **M2.5** (breaking → 0.20) and shrank M5 (removed `done_error`; M5 now = timeout + cancel + the read-timeout, which yields `Err(StreamReadTimeout)`).

---

## Appendix — quick command reference

```bash
fmt                                            # format all (Rust+Python+TOML+Nix)
check-rust                                     # THE gate: fmt-check → clippy --all-features → test --all-features
cargo test --features claude-code -p motosan-ai claude_code::
cargo test --features codex-cli  -p motosan-ai codex_cli::
cargo test --features gemini-cli -p motosan-ai gemini_cli::
cargo build -p motosan-ai --features claude-code,codex-cli,gemini-cli  # compiles the gated CLI chat() literals (no-feature build does NOT catch them)
cargo test --features codex-cli -p motosan-ai -- --ignored   # live round-trip (M2.5)
```

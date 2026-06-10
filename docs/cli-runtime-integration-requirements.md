# CliRuntime (§6.2) — motosan-ai Integration Requirements

> **Status:** planning record for the **deferred** v1.x `CliRuntime`. Not implemented in v1.
> **Basis:** verified source audit of **motosan-ai 0.20.0** (`../motosan-ai/sdks/rust`), 2026-06-10.
> **Purpose:** capture exactly what `motosan-ai`'s CLI providers must add before `CliRuntime` is viable, so the work can be scheduled / handed to the motosan-ai maintainers. CliRuntime's feasibility is **gated on these provider capabilities** — that is the concrete reason v1 shipped `LoopRuntime` first and pushed CliRuntime to v1.x.

## 1. What CliRuntime is, and why it depends on motosan-ai

`CliRuntime` is the §6.2 `AgentRuntime` adapter that — unlike v1's in-process `LoopRuntime` (Anthropic/OpenAI HTTP API via loop's `Engine`) — **spawns an external coding-agent CLI as a subprocess** (Claude Code / Codex CLI / Gemini CLI). This is Paperclip's model: the agent *is* the CLI, inheriting its full tool set; the org just orchestrates the black box.

The §6 `AgentRuntime` contract has two load-bearing requirements that LoopRuntime gets for free but CliRuntime must obtain from each CLI provider:

1. **`cwd` is the cross-run state boundary** — the spawned process MUST run with actual working directory `== ctx.cwd`.
2. **Same-issue runs reuse the same session** — conversation continuity across heartbeats (recorded in `HeartbeatRun.session_ref`).

Both depend entirely on what each provider's flags expose. The three providers' capabilities are **inconsistent**, and (per the audit) **no single provider currently satisfies both `cwd` and `session` through the SDK**.

## 2. Verified capability matrix (motosan-ai 0.20.0)

| Capability | **ClaudeCode** (`claude-code`) | **CodexCli** (`codex-cli`) | **GeminiCli** (`gemini-cli`) |
|---|---|---|---|
| Set `cwd` | ✅ `.cwd()` → `Command::current_dir` (`--add-dir` only adds *extra* roots) | ✅ `.cd()` → `--cd` (Codex workspace-root flag, not OS cwd) | ✅ `.cwd()` → `Command::current_dir` (`--include-directories` only adds roots) |
| Session continuity | ✅ can **set** `--session-id <uuid>` / `--resume` and surfaces `result.session_id` | ✅ captures `thread.started.thread_id` and supports `codex exec resume <id>` | ⚠️ surfaces `init.session_id` and forwards `.resume(id)` to `--resume <id>`; arbitrary-id resume is still live-unverified |
| Per-call budget cap | ✅ `--max-budget-usd` | ❌ none | ❌ none |
| Permission mode | ✅ `--permission-mode` (6 modes) + `--dangerously-skip-permissions` | ⚠️ coarse only (`--full-auto` / `--sandbox` / bypass); **no interactive approval channel** | ✅ `--approval-mode` (default/auto_edit/yolo/plan) + `--yolo`/`--sandbox` |
| Text stream events | ✅ | ✅ | ✅ |
| Usage stream event (**tokens only — no $ cost field**) | ✅ (`result.usage`) | ✅ (`turn.completed`) | ✅ (`result.stats`) |
| ToolCall / ToolResult events | ✅ stream path surfaces Claude `tool_use` as tool-call start/args/end (verified live); no separate ToolResult | ✅ `command_execution` + `mcp_tool_call` both verified live (codex-cli 0.130.0; MCP name surfaced as `server/tool`, e.g. `node_repl/js`); results stay out of `ChatResponse` | ✅ Gemini `tool_use` (`tool_id`/`tool_name`/`parameters`) verified live (gemini 0.38.1); `tool_result` ignored |
| Thinking events | ❌ dropped | ❌ dropped | n/a (headless emits none) |
| `started` synthetic event | ❌ (none in SDK) | ❌ | ❌ |
| Stream error surfacing | ✅ errors surface as `Err` in the stream (0.20) | ✅ errors surface as `Err` in the stream (0.20) | ✅ errors surface as `Err` in the stream (0.20) |
| Stop reason | ✅ stream success terminal is `ToolUse` after any tool-call event, else `EndTurn`; blocking chat remains `EndTurn` | ✅ stream success terminal is `ToolUse` after any tool-call event, else `EndTurn`; blocking chat remains `EndTurn` | ✅ stream success terminal is `ToolUse` after any tool-call event, else `EndTurn`; blocking chat remains `EndTurn` |
| Timeouts | ✅ configurable `.timeout(Duration)` / `.no_timeout()`; default 300s; `chat()` timeout + stream per-line deadline yield `Err(StreamReadTimeout)` on stalls | ✅ configurable `.timeout(Duration)` / `.no_timeout()`; default 600s; `chat()` timeout + stream per-line deadline yield `Err(StreamReadTimeout)` on stalls | ✅ configurable `.timeout(Duration)` / `.no_timeout()`; default 600s; `chat()` timeout + stream per-line deadline yield `Err(StreamReadTimeout)` on stalls |
| Cancellation | ⚠️ no explicit cancel handle; `kill_on_drop(true)` plus stream driver owns/reaps child | ⚠️ no explicit cancel handle; `kill_on_drop(true)` plus stream driver owns/reaps child | ⚠️ no explicit cancel handle; `kill_on_drop(true)` plus stream driver owns/reaps child |

**Cross-cutting facts (all three providers):**
- Every agent-shaping knob (`cwd`, session, budget, permission, sandbox) is a **provider-builder field, not a `ChatRequest` field** — **except `model`**, which IS honored per-request via `ChatRequest::model` (all three providers call `build_spawn_config(request.model)`). Varying `cwd`/session **per run** means **rebuilding the provider each call** (providers are `Clone`, so cheap, but it is structural friction — and `model` shows the per-request override path already exists; see P2.4 + the §4 design note).
- **`env`/`envs()` injection landed** — callers can inject per-run secret bundles into spawned children without mutating the parent environment.
- Cancellation is **`kill_on_drop` only** — no clean cancel handle; stream drivers own and reap the child at the tail.
- Timeouts are **configurable provider-builder fields** (`timeout(Duration)` / `no_timeout()`), defaulting to Claude 300s and Codex/Gemini 600s. Blocking `chat()` wraps child completion; streams apply the same deadline to each line read and yield `Err(StreamReadTimeout)` on stalls.

## 3. Three findings that change the design (vs spec §6.2)

**Finding 1 — the spec's "process-layer `cwd` workaround" does not exist.**
§6.2 says when a provider lacks a cwd setter (ClaudeCode/Gemini), `CliRuntime` should "set the working directory at the process layer via `Command::current_dir(ctx.cwd)`, bypassing the provider builder." **That assumes the runtime owns the spawn.** It does not — the motosan-ai **provider** owns `Command::new(...).spawn()` (`claude_code/mod.rs:418`, `claude_code/spawn.rs:364`; `gemini_cli/mod.rs:208`, `gemini_cli/spawn.rs:180`). The runtime cannot inject `current_dir`, and there is no env passthrough to smuggle it either. `std::env::set_current_dir` is process-global — it mutates the whole process's cwd, so concurrent runs race on it — and therefore not an option. **Consequence:** `cwd` correctness for ClaudeCode/Gemini **requires an SDK change** (or bypassing motosan-ai's spawn entirely). This promotes "add a `current_dir` setter" from *nice-to-have* to **mandatory** (→ P0).

**Finding 2 — ClaudeCode session is "set-only", but mintable, so continuity is achievable today.**
The provider accepts `--session-id <uuid>` (you supply the id) and `--resume <id>`, but its stream/result parser reads only `result` + `usage` and **discards the session id the CLI mints**. So you cannot *learn* a fresh id — **but you don't need to**: the runtime can **generate its own deterministic UUID** (the CliRuntime analog of `session_key`), pass it as `--session-id` on run 1, then `--resume <same-uuid>` on subsequent runs. ClaudeCode session continuity is therefore solvable **without an SDK change**. Codex and Gemini have **no** such escape hatch.

**Finding 3 — provider-level config + no env injection breaks the per-run secret model.**
v1's security model is `SecretResolver → ctx.secrets` (per-run key, never persisted). With CLI providers, all knobs are provider-level and **there is no `env()` injection**, so a per-run secret cannot reach the child through the SDK. CliRuntime would have to rely on ambient env (weaker) until motosan-ai exposes env injection (→ P2).

## 4. Required motosan-ai changes (prioritized)

### P0 — makes the `cwd` contract satisfiable (without this, even the flagship is non-compliant)
- **P0.1 — `ClaudeCodeProvider`: `cwd` / `current_dir` setter landed.** `--add-dir` remains an extra-roots mechanism, not a substitute.
- **P0.2 — `GeminiCliProvider`: `cwd` / `current_dir` setter landed.** `--include-directories` remains an extra-roots mechanism, not a substitute.
- CodexCli already has `.cd()` → `--cd` — nothing needed; CliRuntime just calls `.cd(ctx.cwd)`.

### P1 — makes "same-issue session continuity" satisfiable
- **P1.1 — `CodexCliProvider`: session resume landed.** Captures `thread.started.thread_id`, surfaces it on `ChatResponse::session_id` / streamed `StreamEvent::session_id`, and supports `codex exec resume <thread_id>`. A `#[ignore]` live round-trip test (`codex_resume_roundtrip`) pins the CLI subcommand shape.
- **P1.2 — created session ids surface on all three.** ClaudeCode `result.session_id`, Codex `thread.started.thread_id`, and Gemini `init.session_id` now flow through the additive SDK `session_id` fields.
- **P1.3 — `GeminiCliProvider`: readable `session_id` surfaced; arbitrary-id resume unverified.** The SDK forwards `.resume(id)` verbatim to `--resume <id>`, but whether the Gemini CLI honors arbitrary captured ids (vs only `latest`/index) still needs live CLI verification.

### P2 — agent-backend quality
- **P2.1 — `env`/`envs()` injection** on all providers, so a per-run `SecretBundle` can reach the child (aligns with the org's `SecretResolver` model).
- **P2.2 — ToolCall stream events landed.** CLI stream paths surface provider tool-use events as tool-call start/args/end triplets so a runtime can observe/gate tool use. Separate ToolResult events remain out of scope for the current shared stream API.
- **P2.3 — stream robustness landed.** Success terminals carry real `stop_reason`, timeouts are configurable and cover both blocking completion and per-line stream reads, and the cancel contract is documented (`kill_on_drop`, no explicit handle). Stream errors surface as `Err` items in Rust 0.20.
- **P2.4 — (structural) allow per-`ChatRequest` overrides** for `cwd`/session/budget, to avoid rebuilding the provider every run.

> **Design note — where `cwd`/session should live (fork for the motosan-ai maintainers).**
> The §5 patches add `cwd` as a **provider-builder field**. But `model` already rides on `ChatRequest` and is honored per-request (`build_spawn_config(request.model)` in all three providers) — that is exactly the per-request-override pattern P2.4 asks for, *already shipped for one knob*. So there are two routes:
> - **(a) builder field** — the §5 patches as written. Smallest diff; keeps the "rebuild the provider to vary `cwd`/session per run" friction this doc flags.
> - **(b) `ChatRequest` / `provider_options`** — put `cwd`/session alongside `model`. More consistent, removes the per-run rebuild, and **folds P2.4 into P0** instead of deferring it.
>
> The §5 drafts take route (a) because it is the minimal change that unblocks the `cwd` contract. If motosan-ai would rather not entrench the builder-field pattern, route (b) is the place to start — decide this before applying §5.

## 5. P0 patch drafts (ready to apply — motosan-ai 0.19.0)

Both providers spawn at **two** sites (a blocking path and an inline streaming path); the `current_dir` call must be added at **both**, fed by a new `SpawnConfig.cwd` threaded from a new provider field. Anchors are 0.19.0 line numbers.

### 5.1 `ClaudeCodeProvider` — add `cwd`

**`src/providers/claude_code/mod.rs`**
```rust
// (1) struct field — after `max_budget_usd` (~line 128):
    /// Working directory for the spawned `claude` process. When set, the child
    /// runs with this cwd instead of inheriting the parent's.
    pub cwd: Option<PathBuf>,

// (2) ClaudeCodeProvider::new() — after `max_budget_usd: None,` (~line 161):
            cwd: None,

// (3) builder — next to `max_budget_usd(...)` (`pub fn` at line 347):
    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

// (4) build_spawn_config — after `max_budget_usd: self.max_budget_usd,` (line 381):
            cwd: self.cwd.clone(),

// (5) stream() — right after `let mut cmd = Command::new(&config.binary_path);` (line 418):
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
```

**`src/providers/claude_code/spawn.rs`**
```rust
// (6) SpawnConfig field — after `binary_path` (~line 93):
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,

// (7) invoke_cli — right after `let mut cmd = Command::new(&config.binary_path);` (line 364):
    if let Some(dir) = &config.cwd {
        cmd.current_dir(dir);
    }
```
(`PathBuf` is already imported in both files.)

### 5.2 `GeminiCliProvider` — add `cwd`

**`src/providers/gemini_cli/mod.rs`**
```rust
// (1) struct field — after `resume` (line 60):
    /// Working directory for the spawned `gemini` process. When set, the child
    /// runs with this cwd instead of inheriting the parent's.
    pub cwd: Option<PathBuf>,

// (2) GeminiCliProvider::new() — after `resume: None,` (~line 80):
            cwd: None,

// (3) builder — alongside the other setters:
    /// Set the working directory for the spawned process (`Command::current_dir`).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

// (4) build_spawn_config — after `resume: self.resume.clone(),` (line 171):
            cwd: self.cwd.clone(),

// (5) stream() — right after `let mut cmd = Command::new(&config.binary_path);` (line 208):
        if let Some(dir) = &config.cwd {
            cmd.current_dir(dir);
        }
```

**`src/providers/gemini_cli/spawn.rs`**
```rust
// (6) SpawnConfig field — after `binary_path` (~line 64):
    /// Working directory for the spawned process; `None` inherits the parent's.
    pub cwd: Option<PathBuf>,

// (7) build_command — right after `let mut cmd = Command::new(&config.binary_path);` (line 180):
    if let Some(dir) = &config.cwd {
        cmd.current_dir(dir);
    }
```

> **Tests:** add a `common_args`/spawn test asserting the child's `current_dir` is set when `cwd` is `Some` (mirror the existing `common_args_*` argv tests). Note `cwd` is **not** an argv flag, so it won't appear in `common_args` output — assert on the built `Command` instead, or expose a small `cwd()` accessor on `SpawnConfig` for the test.

## 6. Viable paths today (with the degradations)

- **ClaudeCode** — cwd ✅ (`.cwd(ctx.cwd)` landed), session ✅ (self-minted UUID via `--session-id`/`--resume`), budget ✅, permission ✅, tool events ❌ (synthesize `Started`/`Finished`). This is the intended flagship path; P0 is landed.
- **CodexCli** — cwd ✅ (`.cd(ctx.cwd)` → `--cd`, not OS `current_dir`), session ✅ (`thread.started.thread_id` + `.resume(id)`), budget/permission weak. The ignored `codex_resume_roundtrip` live test verifies the real CLI subcommand when run.
- **GeminiCli** — cwd ✅ (`.cwd(ctx.cwd)` landed), session readback ✅ (`init.session_id`), and `.resume(id)` forwarding ✅; arbitrary-id resume remains a live CLI assumption until verified.

→ With P0/P1 and P2.1–P2.3 landed, CLI session/cwd feasibility and the main quality/security surfaces are no longer blocked in the SDK. Remaining work is structural: optional per-`ChatRequest` overrides (P2.4) plus live provider checks such as Gemini arbitrary-id resume.

## 7. What CliRuntime (org side) does once P0/P1 land

For reference, the org-side adapter is a thin `AgentRuntime` over a `motosan_ai::Client` configured for `Provider::ClaudeCode`/`CodexCli`/`GeminiCli` (feature `cli-runtime`):
- Resolve provider + flags from `AdapterRef.config`; set `cwd` via the new setter (`.cwd(ctx.cwd)` / `.cd(ctx.cwd)`).
- Session: use provider-specific continuity. ClaudeCode may mint/derive a deterministic id (the `session_key` analog), set it with `--session-id` on run 1, then resume it thereafter. CodexCli must capture `ChatResponse::session_id` / streamed `StreamEvent::session_id` from `thread.started.thread_id` on run 1 and pass that captured id to `.resume(id)` on later runs. GeminiCli captures `init.session_id` and forwards `.resume(id)` verbatim, but arbitrary captured-id resume remains a live CLI assumption until verified. Record the usable provider session id in `HeartbeatRun.session_ref`.
- Events: synthesize `Started`/`Finished` (the SDK has neither), map `Text` + `usage` → `RunEvent::Text` + one `CostDelta`, and map streamed tool-call start/args/end triplets to tool-use events. **Note:** the SDK's usage events carry **token counts only — there is no $ cost field** (`ClaudeStreamUsage`/`CodexUsage`/`GeminiStats` are all tokens), so the `CostDelta` must be **computed from tokens × model price**, not read off the stream.
- Conclusion: parse the §8 JSON block from the CLI's final output (same mechanism as LoopRuntime); parse failure → `Progress`.
- Budget: map `ctx.budget_remaining_minor` → `--max-budget-usd` where available (ClaudeCode only) as double-insurance; orchestrator stage-2b hard-stop is the real gate elsewhere.

With P0/P1 landed, v1 can evaluate CliRuntime behind its planned feature gate; **LoopRuntime** remains the conservative default until P2 quality/security surfaces land and live CLI resume checks (including Gemini arbitrary ids) are verified.

# CliRuntime (§6.2) — motosan-ai 0.20 Capability Reference & Integration Notes

> **Status (2026-06-10): motosan-ai 0.20.0 shipped every capability this doc originally asked for.** CliRuntime is now **unblocked at the provider level**. This file has been rewritten from a "requirements ask" (vs 0.19.0) into a current-state reference + the integration gotchas found while wiring the org workspace to 0.20.
> **Still deferred:** the org's `CliRuntime` adapter itself remains a v1.x plan; v1 ships `LoopRuntime`. What changed is that the *provider* blockers are gone.

## 1. What CliRuntime is

The §6.2 `AgentRuntime` adapter that **spawns an external coding-agent CLI as a subprocess** (Claude Code / Codex CLI / Gemini CLI), vs v1's in-process `LoopRuntime` (Anthropic/OpenAI HTTP API). The agent inherits the CLI's full tool set (Paperclip's model). It needs two things from each motosan-ai CLI provider: run the child with `cwd == ctx.cwd`, and resume the same session across heartbeats. In 0.19 no provider offered both; **0.20 fixed that.**

## 2. Capability matrix — motosan-ai 0.20.0 (all gaps closed)

| Capability | **ClaudeCode** | **CodexCli** | **GeminiCli** |
|---|---|---|---|
| Set `cwd` | ✅ `.cwd(dir)` *(M1, new)* | ✅ `.cd(dir)` | ✅ `.cwd(dir)` *(M1, new)* |
| Session continuity | ✅ `--session-id`/`--resume` **+ minted id surfaced** *(M2)* | ✅ `.resume(id)` → `codex exec resume` *(M2, new)* | ✅ `.resume(id)` **+ id surfaced** *(M2)* |
| Per-run env injection | ✅ `.env()/.envs()` *(M3, new)* | ✅ `.env()/.envs()` *(M3, new)* | ✅ `.env()/.envs()` *(M3, new)* |
| Tool-call stream events | ✅ `ToolCallStart/Args/End` *(M4, new)* | ✅ *(M4, new)* | ✅ *(M4, new)* |
| Stream errors surfaced | ✅ `Err(..)` items *(M2.5)* | ✅ *(M2.5)* | ✅ *(M2.5)* |
| Configurable timeout | ✅ `.timeout()/.no_timeout()` *(M5)* | ✅ *(M5)* | ✅ *(M5)* |
| Text / usage stream | ✅ / ✅ | ✅ / ✅ | ✅ / ✅ |
| Per-call budget cap | ✅ `--max-budget-usd` | ❌ (Codex CLI has none) | ❌ (Gemini CLI has none) |
| Permission mode | ✅ `--permission-mode` (6) | ⚠️ coarse (`--full-auto`/`--sandbox`) | ✅ `--approval-mode` (4) |

The only remaining "❌"s (Codex/Gemini per-call budget; Codex coarse permissions) are **upstream CLI limitations**, not motosan-ai gaps — the orchestrator's stage-2b hard-stop is the budget gate for those providers.

## 3. What 0.20 delivered (this doc's former P0/P1/P2 asks)

motosan-ai 0.20.0 closed the gap analysis essentially one-to-one (see `motosan-ai` commits #193–#199 / its `sdks/rust/CHANGELOG.md` 0.20.0):

| 0.20 milestone | This doc's former ask |
|---|---|
| **M1** — CLI `cwd` setters (ClaudeCode + Gemini) | P0.1 / P0.2 |
| **M2** — CLI session continuity (all three; session id surfaced) | P1.1 / P1.2 / P1.3 |
| **M2.5** — fallible stream (`BoxStream` item → `Result`) | P2.3 (error surfacing) |
| **M3** — per-run env injection | P2.1 |
| **M4** — CLI tool-call stream events | P2.2 |
| **M5** — configurable timeout / read deadline | P2.3 (timeout) |

→ The **P0 `current_dir` patch drafts in the old version of this doc are obsolete** — motosan-ai shipped them (`ClaudeCodeProvider::cwd` / `GeminiCliProvider::cwd`). They have been removed.

## 4. Integration gotchas found wiring the workspace to 0.20 (IMPORTANT)

Upgrading the local workspace to motosan-ai 0.20 surfaced two issues that any consumer of the `loop + motosan-ai` stack must handle:

### 4.1 The one breaking change (M2.5) — `BoxStream` item → `Result`
`motosan_ai::BoxStream` items are now `Result<StreamEvent, MotosanError>` (and `collect_stream()` returns `Result<ChatResponse, _>`). Any code iterating the stream must `let ev = item?;`. **`motosan-agent-loop`'s `MotosanAiClient` bridge** (`src/motosan_ai_impl.rs`, both `Client::chat_stream` and `MotosanAiClient::chat_stream`) needed exactly this one-line adaptation. The org crate is unaffected directly (it uses the non-streaming `chat()` callback path).

### 4.2 The primitives source skew (the subtle one)
motosan-ai 0.20 declares `motosan-agent-primitives = "0.4.0"` (**crates.io**, no path), while `motosan-agent-loop` and `motosan-agent-tool` use the **local path** crate of the same version. Once primitives 0.4.0 is published, cargo stops unifying the two sources, so the graph contains **two distinct `motosan_agent_primitives::ToolSchema` types** → loop's `req.tool_schemas(&schemas)` fails to compile (E0308: `motosan_ai::ToolSchema` vs `motosan_agent_tool::ToolSchema`). It also makes the org crate **fully unbuildable at every feature level** (the optional path-dep version requirement is evaluated during resolution).

**Two fixes:**
- **Consumer-side (applied in org):** a `[patch.crates-io] motosan-agent-primitives = { path = "../motosan-agent-primitives" }` in `motosan-agent-org/Cargo.toml` forces every reference onto the one path instance. Self-contained; org-only.
- **Root fix (recommended for the workspace):** change motosan-ai's primitives dep to `motosan-agent-primitives = { path = "../../../motosan-agent-primitives", version = "0.4.0" }`. This unifies for *all* local consumers (org **and** loop-standalone) without any `[patch]`, and still publishes correctly (the path is stripped on publish, the version is used). With this, org could drop its `[patch]` and loop's own `--features motosan-ai` tests would build standalone.

### 4.3 Version pins bumped
- `motosan-agent-org/Cargo.toml`: `motosan-ai` `0.19` → `0.20`.
- `motosan-agent-loop/Cargo.toml`: `motosan-ai` (dep + dev-dep) `0.19.0` → `0.20.0`; plus the M2.5 stream adaptation and an additive `StreamEvent.session_id` test-literal fix. loop's **lib** compiles against 0.20 (org consumes it green); loop's own `--features motosan-ai` **test suite** needs §4.2 resolved (the org patch covers org; loop-standalone needs the root fix or its own patch).

## 5. Viable CLI paths now

All three providers are now capability-complete for the §6 contract:
- **ClaudeCode** (recommended flagship) — cwd ✅, session ✅, budget ✅, permission ✅, tool events ✅.
- **CodexCli** — cwd ✅, session ✅ (`codex exec resume`), tool events ✅; no per-call budget (orchestrator-gated), coarse permissions.
- **GeminiCli** — cwd ✅, session ✅, tool events ✅; no per-call budget (orchestrator-gated).

The "no provider has both cwd and session" blocker that justified the v1.x deferral **no longer applies.**

## 6. Org-side CliRuntime sketch (unchanged; now buildable)

A thin `AgentRuntime` over a `motosan_ai::Client` configured for `Provider::ClaudeCode`/`CodexCli`/`GeminiCli` (feature `cli-runtime`):
- Resolve provider + flags from `AdapterRef.config`; set `cwd` via `.cwd(ctx.cwd)` / `.cd(ctx.cwd)`; inject secrets via `.envs(ctx.secrets)` (M3) instead of relying on ambient env.
- Session: derive a deterministic id (the `session_key` analog), set it on run 1, resume thereafter; record the surfaced session id in `HeartbeatRun.session_ref`.
- Events: map CLI `Text` + `usage` + the new `ToolCallStart/Args/End` (M4) → `RunEvent::{Text, ToolCall, ToolResult, CostDelta}`; the stream is now fallible so provider errors propagate (M2.5).
- Conclusion: parse the §8 JSON block from the CLI's final output (same mechanism as LoopRuntime); parse failure → `Progress`.
- Budget: map `ctx.budget_remaining_minor` → `--max-budget-usd` where available (ClaudeCode); orchestrator stage-2b is the real gate elsewhere.

**Next step when prioritized:** a `CliRuntime` plan (brainstorm → exact-code plan → adversarial review → build), now that the provider layer is ready.

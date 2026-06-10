# CliRuntime Provider Changes — Subagent Prompt Sheet

Copy-paste one block per milestone into a fresh subagent. Run milestones **in order**; each assumes the prior is merged. Plan: `docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md`.

## Standing rules (apply to every prompt)

- Every `.rs`/`Cargo.toml` change goes through a **PR + CI** (never direct-to-main). One PR per milestone.
- **TDD**: write the failing test → run it red → implement → run it green → `cargo fmt` → commit. Small commits.
- Gate before PR: `fmt` then `check-rust` (fmt-check → `clippy --all-features` → `test --all-features`). Must be green.
- **Line numbers in the plan are 0.19.0-baseline-relative and drift as milestones merge** — locate every edit by the quoted surrounding code / symbol, not the bare line number.
- `cwd`/`env`/`timeout` are NOT argv flags — assert via `cmd.as_std().get_current_dir()` / `get_envs()` / the resulting `SpawnConfig`, never via `common_args`.
- Trust the compiler on struct-literal exhaustiveness (E0063): it lists every `SpawnConfig`/`ChatResponse`/`empty_config`/`*_full_loadout_*` site you must update.

---

## M1 — P0: cwd setters (ClaudeCode + Gemini)

```
Implement Milestone M1 (P0: cwd setters) from docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md, tasks M1.1–M1.4, on branch feat/cli-cwd-setters.

Follow the plan's TDD steps exactly. Key points:
- M1.1: extract a `build_command(config) -> Command` helper in claude_code/spawn.rs (claude builds its blocking Command inline; codex/gemini already have build_command). No behavior change.
- M1.2/M1.3: add a `cwd: Option<PathBuf>` builder field to ClaudeCodeProvider and GeminiCliProvider, thread into SpawnConfig, and call `cmd.current_dir(dir)` at BOTH spawn sites (mod.rs stream() + spawn.rs build_command). PathBuf is already imported in all four files.
- Test via `cmd.as_std().get_current_dir()`; cwd is not an argv flag so common_args_* assertions are unchanged. Update each spawn.rs empty_config() and the common_args_full_loadout_order_is_stable literal.
- M1.4: Codex needs no code (already has .cd()); update the §2 capability matrix in docs/cli-runtime-integration-requirements.md.

Gate: fmt → check-rust green. Then open a PR and report the PR URL. Do NOT start M2.
```

---

## M2 — P1: session continuity

```
Implement Milestone M2 (P1: session continuity) from docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md, tasks M2.1–M2.5, on branch feat/cli-session-continuity (M1 must be merged first).

Key points:
- M2.1: add additive `session_id: Option<String>` (serde-skipped) to StreamEvent AND ChatResponse, plus a `StreamEvent::session_started(id)` constructor. Add `session_id: None` to ALL 11 existing StreamEvent constructors. ChatResponse literals to fix: the SHARED `ChatResponseBuilder::build()` in providers/mod.rs (HTTP providers), collect_stream in stream.rs, AND the three feature-gated CLI chat() literals (placeholder `session_id: None`, replaced in M2.2-M2.4). The CLI literals are behind #[cfg(feature=...)] so a no-feature build does NOT catch them — verify with `cargo build -p motosan-ai --features claude-code,codex-cli,gemini-cli`.
- M2.2 (Codex): model `thread.started.thread_id` (currently dropped to Other) → NdjsonAction::SessionStarted; add a `resume` builder field → `codex exec resume <id>` via a shared `push_exec_subcommand` helper used at both spawn sites; widen invoke_cli/parse_collected_stream to return the captured id (3-tuple → 4-tuple). COMPILER CARRY-OVERS (build-breakers): repoint `ignore_unknown_event` (uses thread.started as its 'unknown' fixture → e.g. item.started); add `resume: None` to the `common_args_full_loadout_order_is_stable` SpawnConfig literal (no ..empty_config() spread → E0063); update the THREE tests that destructure parse_collected_stream's tuple (last_agent_message_is_content_rest_is_thinking ~665, single_agent_message_has_no_thinking ~684, parse_collected_stream_ignores_blank_lines ~702) to the 4-tuple. Update the stale module docs (resume no longer out of scope).
- M2.3 (Claude): read `result.session_id` (currently ignored); yield session_started on the stream path. Blocking readback is best-effort (Claude self-mints --session-id) — either thread it via parse_agent_json or leave blocking session_id = None; pick one consistently. Update parse_result_* test destructuring to `{ usage, done, .. }`.
- M2.4 (Gemini): model `init.session_id` (Init{} is currently empty) → SessionStarted; FIX the existing `skip_init_event` test (its fixture has session_id:"abc" — it must now expect SessionStarted); document that resume(id) accepts a concrete captured id (satisfies P1.3). Widen parse_collected_stream/invoke_cli to return the id; update the two destructuring tests.
- M2.5: add an `#[ignore]` Codex resume round-trip integration test (the only check that pins the real `codex exec resume` flag); update the §2 matrix.

Gate: check-rust green + `cargo build -p motosan-ai` (no features) green. Open a PR, report the URL. Do NOT start M3.
```

---

## M2.5 — Fallible stream (Item → `Result`) — BREAKING, 0.20

```
You are implementing one milestone of a Rust SDK change in the motosan-ai repo. Full plan: docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md — read §0 and the M2.5 section first. You are on branch feat/fallible-stream (M1+M2 merged). This is a BREAKING change → release 0.20.

STANDING RULES apply. The whole crate won't compile until every site is updated — that's expected; this is ONE PR. The compiler enumerates every site.

TASK — Milestone M2.5 (fallible stream), tasks M2.5.1–M2.5.5:
- M2.5.1: change BoxStream to `Pin<Box<dyn Stream<Item = Result<StreamEvent, MotosanError>> + Send>>` (stream.rs:5). `collect_stream` → returns `Result<ChatResponse, MotosanError>`, unwrapping each item with `?`. Test helpers/mocks `Ok`-wrap events (`iter(events.into_iter().map(Ok))`); their `collect_stream(...).await` callers add `.unwrap()`.
- M2.5.2: each HTTP provider's final `impl Stream`: `type Item = StreamEvent` → `Result<StreamEvent, MotosanError>`; every `Poll::Ready(Some(ev))` → `Some(Ok(ev))`; the `Err(_) => continue` drop arm → `Err(e) => Poll::Ready(Some(Err(MotosanError::Stream(e.to_string()))))`. Sites: anthropic(893,1114), openai(780,799), gemini(434,530), ollama(423,490), gemini_code_assist(190); minimax via anthropic. TDD: a mock inner stream that errors now yields Some(Err(_)).
- M2.5.3: CLI `stream!` loops: every `yield <ev>` → `yield Ok(<ev>)`; codex `Error(_msg)=>yield done()` and gemini `Error(_msg)=>break` → `yield Err(MotosanError::ProviderError(msg))`; claude — model result.is_error/subtype → NdjsonAction::Error → `yield Err(...)` (confirm field names vs a real binary). Success terminal → `yield Ok(done_with_stop_reason(EndTurn))` for all three. TDD: a loop ending in a provider error yields Err.
- M2.5.4: ReadTimeoutStream (client.rs:1072) — item→Result, on deadline yield `Err(StreamReadTimeout)` once then end (track a done bool). ThinkStripperStream (client.rs:1112) — item→Result, pass Err through untouched, strip only on Ok(event). Client::stream/stream_with outer signature unchanged (Result<BoxStream,_>). Update client.rs test mocks.
- M2.5.5: bump Cargo.toml → 0.20.0; CHANGELOG BREAKING entry (migration: `while let Some(ev)` → `let ev = ev?`); update AGENTS/llms.txt/SKILL stream examples; update §2 matrix "Stream error surfacing" → "errors surface as Err (0.20)".

Gate: check-rust green; per-provider mid-stream-error tests yield Err; collect_stream returns Err on a failing stream. Open a PR, report the URL. Do NOT start M3/M4.
```

---

## M3 — P2.1: env injection

```
Implement Milestone M3 (P2.1: env injection) from docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md, tasks M3.1–M3.2, on branch feat/cli-env-injection (M1, M2 merged).

Key points:
- M3.1: create src/providers/redacted_envs.rs with a `RedactedEnvs(Vec<(String,String)>)` newtype whose Debug prints `<N redacted>` (so providers keep #[derive(Debug)] and never leak secret values). Methods: ONLY push, replace_from, to_vec — do NOT add as_slice/len/is_empty (nothing calls them → clippy -D warnings dead_code FAILS the gate). Register as `#[cfg(any(feature="claude-code",feature="codex-cli",feature="gemini-cli"))] pub mod redacted_envs;` — it MUST be `pub mod` (NOT pub(crate)): the type is a pub field on the public providers, so pub(crate) = E0446 private-in-public, won't compile.
- M3.2: on all three providers add `envs: RedactedEnvs` field + `.env(k,v)` / `.envs(iter)` builders, default in new()/with_path(), thread `self.envs.to_vec()` into `SpawnConfig.envs: Vec<(String,String)>`, and apply at BOTH spawn sites with `cmd.envs(config.envs.iter().map(|(k, v)| (k, v)))`. NOTE: `cmd.envs(&config.envs)` does NOT compile — use the .iter().map form. Update each empty_config() and the *_full_loadout_* literal (envs: Vec::new()).
- Tests: per provider, assert get_envs() injection + secret NOT in argv + Debug of a provider with a secret env shows `<N redacted>` and not the value.

Gate: fmt → check-rust green (all features). Open a PR, report the URL. Do NOT start M4.
```

---

## M4 — P2.2: tool-call stream events

```
Implement Milestone M4 (P2.2: tool-call stream events) from docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md, tasks M4.1–M4.5, on branch feat/cli-tool-call-events (M1+M2+M2.5+M3 merged — the CLI stream loops already yield Result, so your tool-call yields are `yield Ok(event)`).

NO public-API change — StreamEvent already has tool_call_start/args_with_id/end_with_id. Per provider add `NdjsonAction::ToolCalls(Vec<StreamEvent>)`, parse the wire tool event into a start→args_with_id→end_with_id triplet, yield them in stream(), and add an ignore arm in the blocking path (ChatResponse.tool_calls stays empty).
- M4.1 (Claude, VERIFIED shape): model the tool_use content block (id/name/input), serialize input to a JSON-string args delta. PRESERVE narration text: a MIXED turn (text + tool_use in one content[]) must yield the text AND the tool events in wire order — do NOT drop text when tool_use is present. Add a mixed-content regression test. Keep the pure-text path unchanged.
- M4.2 (Codex, INFERRED shape): handle item.completed with item_type command_execution | mcp_tool_call; keep agent_message and drop reasoning/etc.
- M4.3 (Gemini, INFERRED shape): handle a `tool_call` event; read the blocking collector first to decide if it needs a ToolCalls ignore arm.
- M4.4 (GATE): capture a REAL `codex exec --json` and `gemini -o stream-json` tool turn and confirm the type strings + field names (esp. mcp_tool_call singular vs mcp_tool_calls plural); update the serde models + fixtures if they differ. Do NOT merge until Codex+Gemini shapes are confirmed (Claude is already verified). Update the §2 matrix.
- M4.5 (M2.5 carry-over): the CLI stream terminal currently hardcodes `Ok(done_with_stop_reason(EndTurn))`. When a turn ended in a tool call, emit `done_with_stop_reason(ToolUse)` instead (track a saw_tool_call flag) — else collect_stream reports EndTurn for a tool-use turn. Add a test: a stream ending in a tool-call triplet collects to stop_reason == ToolUse.

Gate: check-rust green. Open a PR (after M4.4 confirmation), report the URL. Do NOT start M5.
```

---

## M5 — P2.3: stream robustness

```
Implement Milestone M5 (P2.3: stream robustness) from docs/superpowers/plans/2026-06-09-cli-runtime-provider-changes.md, tasks M5.2–M5.4, on branch feat/cli-stream-robustness (M2.5 + M4 merged).

NOTE: M5.1 (done_error) is GONE — M2.5 already made stream errors first-class `Err` items, and M2.5 also moved the CLI success-terminal `done_with_stop_reason(EndTurn)` and claude `is_error` modeling. M5 is now just timeout + cancel + the read-timeout. The CLI `stream!` loops already yield `Result<StreamEvent, _>`.
- M5.2: per provider, replace the private `const TIMEOUT_SECS` with `pub const DEFAULT_TIMEOUT: Duration`; add a `timeout: Option<Duration>` field + `.timeout(dur)` + `.no_timeout()` builders defaulting to DEFAULT_TIMEOUT; thread into SpawnConfig; invoke_cli uses config.timeout (skip the wrapper when None). Claude 300s, Codex/Gemini 600s.
- M5.3: extract each mod.rs stream() loop into a testable `pub(crate) fn drive_lines<R: AsyncBufRead+Unpin+Send+'static>(child: Option<tokio::process::Child>, reader, read_timeout) -> BoxStream`. CRITICAL: drive_lines MUST own the child and reap it at the tail (`if let Some(mut c)=child.take(){ let _=c.wait().await; }`) — kill_on_drop(true) means a dropped child is SIGKILLed instantly → empty/early EOF. Production passes Some(child); the test passes None::<tokio::process::Child>. Wrap each per-line read in tokio::time::timeout; on stall → `yield Err(MotosanError::StreamReadTimeout(dur.as_secs())); break;` (NOT done_error — items are Result now). TDD: a never-producing reader (tokio::io::duplex, keep the write half alive) yields Some(Err(StreamReadTimeout)).
- M5.4: document the cancel contract (kill_on_drop — dropping the stream reaps the child) in each module doc; update the §2 matrix Stop-reason + timeout rows (error-surfacing row was done in M2.5).

Gate: check-rust green; stream-stall test yields Err(StreamReadTimeout) per provider. Open a PR, report the URL.
```

---

## M6 — P2.4: per-request override (OPTIONAL — recommend DEFER)

```
Only implement Milestone M6 (P2.4) if a concrete caller must vary cwd/session/budget on a &Provider it does not own and cannot clone. Otherwise DEFER (route (a) + provider.clone().cwd(x) already covers the common case at trivial cost).

If building: read CLI-only keys (cwd/session_id/max_budget_usd) from ChatRequest.provider_options (existing untyped Option<Value>) via a pure `cli_overrides(&Option<Value>) -> CliOverrides` helper per provider's spawn.rs; add a `&Option<Value>` param to build_spawn_config and apply `override.or_else(|| self.field.clone())` (request > builder), mirroring the existing request_model.or_else(...). Pass &request.provider_options at both call sites. NO ChatRequest shape change. Full code: see the P2.4 draft in the plan's source notes. TDD: provider_options cwd overrides builder cwd; &None falls back to builder. Document recognised keys in the builder rustdoc.

Gate: check-rust green. Open a PR, report the URL.
```

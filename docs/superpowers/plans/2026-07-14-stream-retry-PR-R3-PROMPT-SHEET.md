# PR-R3 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 13, 14, 21 of the M1 plan — Rust streamed tool-call integrity + usage merge: OpenAI index-aware tool buffering, chatgpt-codex `item_id`→`call_id` mapping, cumulative-usage replace semantics. One branch, one PR.

One fresh subagent per task, in order 13→14→21 (the three tasks touch disjoint files — order is convention, not a hard dependency). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Runs in parallel with PR-P2 / PR-T2 (zero shared files). #211/#214 are already on main — branching off current main means no conflicts.

---

## Setup (run once, before Task 13 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/rust-toolcall-integrity origin/main -b fix/rust-toolcall-integrity
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-toolcall-integrity`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/rust-toolcall-integrity):
/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-toolcall-integrity
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → cargo fmt → cargo clippy --all-features -- -D warnings → commit.
  This is TDD; do not skip the red step.
- All cargo commands run from sdks/rust/.
- The plan was authored against origin/main @ 3e3f413; PRs #211/#214 have merged since, so line
  numbers have drifted (regions are distant from your edits, but ALWAYS ground edits in the real
  files, not the plan's quoted line numbers).
- Emitted StreamEvents MUST stay sequential per tool call (start A, args A…, end A, start B, …) —
  the single-accumulator collector in stream.rs depends on it. Never emit an empty-id args event.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/fmt/clippy step. Never claim success without
  showing green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 13 — OpenAI index-aware tool buffering**
```
Execute "### Task 13: Make Rust OpenAI stream adapter index-aware for parallel tool calls" from
the plan. The adapter currently ignores tool_calls[].index and does tc_id.unwrap_or("") — under
OpenAI's default parallel tool calls this drops call A and emits a phantom empty call. Implement
the plan's per-index buffering (BTreeMap keyed by index; args buffered per index; whole calls
flushed sequentially at finish_reason/[DONE] — this is STRONGER than the TS close-on-switch
reference and genuinely handles interleaved arg deltas; the plan's fixture interleaves them on
purpose). Do NOT modify sdks/rust/src/stream.rs in this task (Task 21 owns it). Tests: extend
sdks/rust/tests/openai_provider.rs (mockito SSE fixture, two interleaved indexes → exactly 2
tool_calls with correct ids/names/assembled JSON input) AND extend
sdks/rust/tests/openai_live.rs per the plan (self-skipping when OPENAI_API_KEY is unset — this
repo's live convention, no #[ignore]).
```

**Task 14 — chatgpt-codex item_id → call_id map**
```
Execute "### Task 14: Fix chatgpt-codex streamed tool-call arg fragments keyed by wire item_id
instead of call_id" from the plan. On the real wire, response.function_call_arguments.delta is
keyed by the ITEM id (fc_…) while start/end use call_id (call_…) — every real streamed codex
tool call currently emits orphaned arg fragments. Record item.id → call_id in a HashMap on
output_item.added (leave the existing seen_tool_ids untouched — the new map sits beside it, per
the task) and translate item_id before emitting tool_call_args_with_id (pass-through fallback).
CRITICAL: also update the existing inline fixtures that use item_id == call_id (that identical-id
coincidence is exactly what masked this bug) to DISTINCT ids (fc_001 / call_001). Tests: extend
mod tests::adapter_tests in the same file AND create the NEW
sdks/rust/tests/chatgpt_codex_live.rs #[ignore] live smoke exactly as the plan writes it — this
live smoke is an M1 milestone exit criterion, do not skip it.
```

**Task 21 — cumulative-usage replace merge**
```
Execute "### Task 21: Fix cumulative-usage double counting in Rust collect_stream" from the plan.
The StreamEventType::Usage arm in sdks/rust/src/stream.rs currently does `+=` on every usage
event; Anthropic's message_delta usage is CUMULATIVE, so output tokens get double-counted
(billing-visible). Replace with the replace-with-fallback merge that MIRRORS Python
_stream_collect.py exactly — including the cache token fields and the keep-earlier-nonzero
nuance the plan spells out. OpenAI-style single-final-usage streams must stay correct. Test:
extend sdks/rust/tests/collect_stream.rs — Usage{input:100,output:5} then Usage{input:100,
output:50} → final usage is 100/50 (NOT 200/55), plus the single-usage case unchanged.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-toolcall-integrity.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) emitted events stay sequential per call and no empty-id args event can be emitted
(Tasks 13/14); fixtures use DISTINCT fc_/call_ ids (Task 14); the usage merge mirrors Python's
replace-with-fallback including cache fields (Task 21); (3) stream.rs untouched by Tasks 13/14;
seen_tool_ids untouched by Task 14; (4) cargo fmt no diff + cargo clippy --all-features -- -D
warnings clean (paste output); (5) cargo test --all-features green for the touched tests (paste
output); the Task 14 live smoke exists and compiles under #[ignore]; (6) no scope creep; (7) the
commit exists with the task's conventional message. Report any deviation. Do not fix — just
report, so I decide.
```

## After Task 21 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-toolcall-integrity/sdks/rust
cargo fmt && cargo clippy --all-features -- -D warnings && cargo test --all-features
cd .. && git push -u origin fix/rust-toolcall-integrity
gh pr create \
  --title "fix(stream): index-aware tool buffering, codex call-id mapping, usage replace-merge" \
  --body "$(cat <<'EOF'
## Summary
- OpenAI stream adapter now keys parallel tool calls by `tool_calls[].index`, buffering argument
  deltas per index and flushing whole calls sequentially — parallel calls are no longer dropped,
  merged, or emitted as phantom empty-id calls (previously the adapter ignored `index` entirely).
- chatgpt-codex: argument deltas keyed by wire `item_id` (`fc_…`) are translated to the call's
  `call_id` (`call_…`) via an item→call map, so real streamed tool calls assemble their
  arguments; the identical-id fixtures that masked the bug now use distinct ids, and a new
  `#[ignore]` live smoke (`chatgpt_codex_live.rs`) verifies the fix against the real wire.
- `collect_stream` usage merge switches from summing to Python's replace-with-fallback semantics —
  Anthropic's cumulative `message_delta` usage no longer double-counts output tokens
  (billing-visible fix).
- New fixtures: interleaved two-index parallel tool calls; distinct `fc_`/`call_` ids; cumulative
  usage events.

M1 plan Tasks 13, 14, 21 (PR-R3): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. Remove the worktree once merged.

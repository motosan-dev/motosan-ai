# PR-T1 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 11, 12, 17 of the M1 plan — TypeScript stream error surfacing + codex id correlation: Anthropic mid-stream `error` frames, chatgpt_codex `error`/`response.failed` frames, codex `item_id`→`call_id` mapping. One branch, one PR.

One fresh subagent per task, **in order 11→12→17** (Task 17 MUST come after Task 12 — both edit `providers/chatgpt_codex.ts` AND the same test file; Task 17's line refs assume Task 12 landed). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Runs in parallel with PR-R2 / PR-P1 (zero shared files) and independent of PR-T2 (no shared files with sse/ndjson/stream.ts changes).

---

## Setup (run once, before Task 11 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/ts-stream-error-surfacing origin/main -b fix/ts-stream-error-surfacing
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/ts-stream-error-surfacing`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/ts-stream-error-surfacing):
/Users/daiwanwei/Projects/wade/motosan-worktrees/ts-stream-error-surfacing
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → typecheck/build → commit. This is TDD; do not skip the red step.
- All commands run from sdks/typescript/. Targeted tests: `npx vitest run tests/<file>`.
  The FULL suite is `npm run build && npm test` — pack-smoke.test.ts requires dist/, so ALWAYS
  build before npm test. There is no ESLint/Prettier gate; the static gate is `npm run typecheck`.
- The test directory is tests/ (NOT test/). Relative imports in src/ end in `.js` (NodeNext).
- The plan was authored against origin/main @ 3e3f413; ALL line numbers are approximate. Ground
  every edit in the real files — if code drifted from a quoted hunk, adapt to reality and say so.
- M1 boundary: surface explicit error FRAMES only. Do NOT remove the fabricated clean-done on
  EOF (that is milestone M3) — the edge-cases test pinning EOF behavior must still pass. No
  public API changes.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/typecheck/build step. Never claim success without
  showing green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 11 — Anthropic mid-stream error frames**
```
Execute "### Task 11: Surface Anthropic mid-stream error frames (TS)" from the plan. Add the
missing error-frame branch to the SSE event switch tail in providers/anthropic.ts, throwing
StreamError (already exported from src/error.ts) with the plan's message format. Scope guard:
ONLY explicit `error` frames — the EOF fabricated-done path stays untouched, and the existing
edge-cases test "stream that ends without message_stop terminates silently with a partial
response" MUST still pass. Test: extend the `describe('AnthropicProvider stream')` block in
tests/providers-anthropic.test.ts — fixture streams message_start, a text delta, then an
overloaded_error frame → iterating rejects with StreamError containing "overloaded_error"; the
text delta was still emitted first.
```

**Task 12 — chatgpt_codex error/response.failed frames**
```
Execute "### Task 12: Stop swallowing chatgpt_codex error/response.failed frames (TS)" from the
plan. The adapter currently bare-returns on error / response.failed frames (Rust/Python raise —
TS returns a truncated success). REUSE the shipped-but-unused chatGptCodexErrorMessage helper
from the SAME file to build the message; throw StreamError. The task also updates the module
docstring, helper docstring, adapter comment, and a stale comment in src/index.ts — do all of it.
Tests: FLIP the two silent-termination pinning tests in tests/providers-chatgpt-codex.test.ts
(they currently assert the swallow behavior; the task specifies their new expectations) and
extend the import line. The four existing chatGptCodexErrorMessage unit tests stay unchanged.
```

**Task 17 — codex item_id → call_id correlation**
```
Execute "### Task 17: Fix TS chatgpt_codex item_id/call_id mismatch in argument deltas" from the
plan. Prerequisite: Task 12 is already committed on this branch (same file + same test file;
this task's line refs assume it). On response.output_item.added (function_call), record
item.id → call_id in a Map on the adapter state; translate item_id through the map in the
response.function_call_arguments.delta branch before emitting toolCallArgsWithId (fallback:
pass through unchanged if unknown). Do NOT touch the output_item.done branch — it already keys
by call_id. Tests: update the two existing fixtures to DISTINCT ids (item.id "fc_001" /
call_id "call_001" — the current identical-id fixtures are exactly what masked this bug) and
add the one new test the task specifies. Full-suite check: npm run build && npm test.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/ts-stream-error-surfacing.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) only explicit error FRAMES are surfaced — the EOF fabricated-done path is untouched
and the edge-cases EOF pinning test still passes (Tasks 11/12); the output_item.done branch is
untouched (Task 17); (3) StreamError is used (no new error classes) and chatGptCodexErrorMessage
was reused, not reimplemented (Task 12); (4) npm run typecheck clean and the touched vitest files
green (paste output); (5) fixtures use distinct fc_/call_ ids (Task 17); (6) no scope creep; (7)
the commit exists with the task's conventional message. Report any deviation. Do not fix — just
report, so I decide.
```

## After Task 17 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/ts-stream-error-surfacing/sdks/typescript
npm run typecheck && npm run build && npm test
cd .. && git push -u origin fix/ts-stream-error-surfacing
gh pr create \
  --title "fix(ts): surface stream error frames and correlate codex call ids" \
  --body "$(cat <<'EOF'
## Summary
- Anthropic stream: mid-stream `error` frames (e.g. `overloaded_error` on HTTP 200) now reject
  with `StreamError` instead of being ignored — truncated streams no longer read as clean
  successes. EOF behavior is deliberately unchanged (M3 scope).
- chatgpt_codex: `error` / `response.failed` frames now throw (via the previously-unused
  `chatGptCodexErrorMessage` helper) instead of silently ending the stream — brings TS to parity
  with Rust/Python; the two tests that pinned the silent behavior are flipped.
- chatgpt_codex: argument deltas keyed by wire `item_id` (`fc_…`) are now translated to the
  call's `call_id` (`call_…`) via an item→call map, so streamed tool calls assemble their
  arguments on the real wire; fixtures updated to distinct ids (identical-id fixtures were
  masking the bug).
- Out of scope (deliberate): EOF-without-terminal-event contract (M3), SSE parser hygiene (PR-T2).

M1 plan Tasks 11, 12, 17 (PR-T1): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. PR-T2 (Tasks 18–20) is independent and may run concurrently in its own worktree. Remove the worktree once merged.

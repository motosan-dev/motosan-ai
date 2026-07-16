# PR-P2 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 15, 16 of the M1 plan — Python streamed tool-call integrity: OpenAI index-keyed buffering + stream `stop_reason`, chatgpt_codex `item_id`→`call_id` mapping. One branch, one PR.

One fresh subagent per task, in order 15→16 (different files — order is convention). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Prerequisite SATISFIED: PR-P1 (#213) is merged — it edited the error paths of the SAME `openai.py` / `chatgpt_codex.py` files, so this plan's line refs have drifted; ground every edit in the real files. Runs in parallel with PR-R3 / PR-T2 (zero shared files).

---

## Setup (run once, before Task 15 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/py-toolcall-integrity origin/main -b fix/py-toolcall-integrity
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/py-toolcall-integrity`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/py-toolcall-integrity):
/Users/daiwanwei/Projects/wade/motosan-worktrees/py-toolcall-integrity
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → format/lint → commit. This is TDD; do not skip the red step.
- All commands run from sdks/python/ via uv: `uv run pytest tests/<file> -v`,
  `uv run ruff format <files>`, `uv run ruff check motosan_ai/` — lint scope is motosan_ai/ ONLY
  (tests/ is not linted by CI; do not "fix" pre-existing findings there).
- The plan was authored against origin/main @ 3e3f413; PR #213 has merged since and edited the
  error paths of openai.py and chatgpt_codex.py, so line numbers HAVE drifted. Ground every edit
  in the real files; the plan's quoted hunks are the intent, not byte-exact anchors.
- Emitted StreamEvents MUST stay sequential per tool call (start A, args A…, end A, start B, …).
  Never emit an args event without the buffered tool_call_id.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/lint step. Never claim success without showing
  green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 15 — OpenAI index-keyed buffering + stream stop_reason**
```
Execute "### Task 15: Fix Python OpenAI streamed parallel tool calls and stream stop_reason" from
the plan. Three defects, one task: (1) stream() ignores tool_calls[].index — only the LAST
parallel call survives and one anonymous end is emitted; port the TS toolBuffer semantics per the
plan (dict keyed by index, one call open at a time, close-on-index-switch, flush at
finish_reason/[DONE]); (2) the terminal StreamEvent(done=True) carries no stop_reason — map
finish_reason onto it (reuse the existing finish-reason map near the top of openai.py); (3)
_stream_collect.py gets the tool-use fallback (no explicit stop_reason + non-empty tool_calls →
"tool_use", else "end_turn" — mirrors Rust stream.rs). READ the task's Scope note and respect it:
this is TS-parity close-on-index-switch, correct for OpenAI's real emission pattern; do NOT
attempt cross-index interleaving re-serialization (that is Rust Task 13's design) and do NOT
claim interleaving support anywhere. Tests: extend tests/test_openai.py (two-index parallel
fixture → 2 tool_calls with correct ids/names/assembled input) and
tests/test_client_stream_collect.py (finish_reason "tool_calls" → collected stop_reason
"tool_use").
```

**Task 16 — chatgpt_codex item_id → call_id map**
```
Execute "### Task 16: Fix Python chatgpt_codex arg-delta item_id/call_id mismatch" from the plan.
Same bug as Rust/TS (already fixed there in #214-era work and #212): arg deltas keyed by wire
item_id (fc_…) while start/end use call_id (call_…) — real streamed codex tool calls emit
orphaned fragments. Record item.id → call_id in the _ChatGptCodexAdapterState dataclass on
response.output_item.added and translate in the arguments.delta branch of _parse_sse_event
(pass-through fallback if unknown). CRITICAL: replace the masking fixtures (item_id == call_id)
in BOTH test files with DISTINCT ids (fc_001 / call_001) per the plan: tests/
test_chatgpt_codex_stream.py AND tests/test_chatgpt_codex_http.py, plus the new test the task
specifies.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/py-toolcall-integrity.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) events stay sequential per call and every args event carries the buffered id (Task 15);
fixtures in BOTH test files use DISTINCT fc_/call_ ids (Task 16); (3) the Task 15 Scope note is
respected — close-on-index-switch only, no interleaving claims in code comments or docstrings;
the _stream_collect fallback matches Rust semantics; (4) lint ran as `uv run ruff check
motosan_ai/` only and is clean, ruff format applied (paste output); (5) `uv run pytest` green for
every touched test file (paste output); (6) no scope creep beyond the three defects / the id map;
(7) the commit exists with the task's conventional message. Report any deviation. Do not fix —
just report, so I decide.
```

## After Task 16 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/py-toolcall-integrity/sdks/python
uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
cd .. && git push -u origin fix/py-toolcall-integrity
gh pr create \
  --title "fix(python): index-keyed streamed tool calls, codex call-id mapping, tool-use stop reason" \
  --body "$(cat <<'EOF'
## Summary
- OpenAI stream(): parallel tool calls are now keyed by `tool_calls[].index` (ports the proven
  TypeScript toolBuffer semantics) — previously only the last call survived and a single
  anonymous end was emitted.
- OpenAI stream(): the terminal event now carries a `finish_reason`-derived `stop_reason`, and
  `collect_stream` gains the tool-use fallback — streamed tool turns report `tool_use` instead of
  `end_turn` (parity with Rust/TS; agent loops no longer silently terminate).
- chatgpt_codex: argument deltas keyed by wire `item_id` (`fc_…`) are translated to the call's
  `call_id` (`call_…`) via an item→call map; the identical-id fixtures that masked the bug now
  use distinct ids in both test files.
- Scope note: OpenAI buffering is close-on-index-switch (TS parity) — correct for OpenAI's actual
  emission pattern; cross-index interleaving re-serialization is Rust-only by design (M1 plan).

M1 plan Tasks 15–16 (PR-P2): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. Remove the worktree once merged.

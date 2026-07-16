# PR-P1 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 4, 8, 9, 10 of the M1 plan — Python retry visibility + stream error surfacing: HTTP status embedded in provider errors, Anthropic mid-stream `error` frames, claude_code error terminals, CLI child death / premature EOF. One branch, one PR.

One fresh subagent per task, **in order 4→8→9→10** (Task 10 MUST come after Task 9 — both edit `providers/claude_code.py` INCLUDING the same import lines). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Runs in parallel with PR-R2 / PR-T1 (zero shared files). Do NOT start PR-P2 (Tasks 15–16) until this PR merges — it edits the same `openai.py` / `chatgpt_codex.py` files.

---

## Setup (run once, before Task 4 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/py-retry-error-surfacing origin/main -b fix/py-retry-error-surfacing
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/py-retry-error-surfacing`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/py-retry-error-surfacing):
/Users/daiwanwei/Projects/wade/motosan-worktrees/py-retry-error-surfacing
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → format/lint → commit. This is TDD; do not skip the red step.
- All commands run from sdks/python/ via uv: `uv run pytest tests/<file> -v`,
  `uv run ruff format <files>`, `uv run ruff check motosan_ai/` — lint scope is motosan_ai/ ONLY
  (tests/ has pre-existing findings and is not linted by CI; do not "fix" them).
- Tests are pytest-asyncio (asyncio_mode=auto) + respx for HTTP mocking + the existing fake-CLI
  fixtures for CLI providers — match the neighboring tests' style exactly.
- The plan was authored against origin/main @ 3e3f413; ALL line numbers are approximate. Ground
  every edit in the real files — if code drifted from a quoted hunk, adapt to reality and say so.
- M1 boundary: surface errors that are silently swallowed TODAY; minimal message-format changes
  (structured error fields are milestone M2). No public API changes. Do NOT touch the per-read
  asyncio.wait_for timeouts in CLI providers (already correct).
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/lint step. Never claim success without showing
  green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 4 — HTTP status visible to the retry classifier**
```
Execute "### Task 4: Make Python OpenAI/MiniMax/ChatGPT-Codex 5xx errors visible to the retry
classifier" from the plan. Today these three providers raise ProviderError(<raw body>) with no
status, so retry.py's regex classifier never retries their genuine 5xx, and codex drops
Retry-After. Embed "HTTP {status}: ..." + the Retry-After value into the raised message using the
EXACT format anthropic.py already uses (the task quotes it). Note the task's minimax nuance: its
chat() currently parses response.json() BEFORE the status check, so the hunk also reorders that
(non-JSON 5xx would crash with JSONDecodeError otherwise). Extend all four test files listed in
the task (test_openai.py, test_minimax.py, test_chatgpt_codex_http.py, test_retry.py): respx 502
with an HTML body then 200 → exactly 2 calls recorded → success; plus the _is_retryable /
_parse_retry_after unit assertions.
```

**Task 8 — Anthropic mid-stream error frames**
```
Execute "### Task 8: Surface Anthropic mid-stream SSE error frames as StreamError (Python)" from
the plan. Add the missing "error" branch to the SSE event dispatch in AnthropicProvider.stream()
(the dispatch has branches for every normal event type but error frames fall through silently).
Raise StreamError with the plan's exact message format (error type + message). Test: extend
sdks/python/tests/test_anthropic_stream_usage.py — it already imports StreamError, has the _sse
fixture helper, and a precedent mid-stream-raise test to copy the shape from. Fixture: message_start,
one text delta, then an overloaded_error frame → iterating raises StreamError containing
"overloaded_error"; the text delta before the error was still yielded.
```

**Task 9 — claude_code error terminal results**
```
Execute "### Task 9: Raise StreamError on Claude Code CLI error terminal results (Python)" from
the plan. Terminal result lines with is_error:true (subtypes error_max_turns /
error_during_execution) currently emit a clean done=True. The raise lives in the two shared parse
helpers so BOTH paths are covered: _parse_ndjson_line (stream path) and _parse_agent_json
(blocking path). Rust parity note from the task: Rust already errors on is_error:true — this
closes a Python-only gap. Tests: extend test_claude_code_runtime.py (fake-CLI _make_proc fixture)
and test_claude_code.py (TestParseAgentJson + TestParseNdjsonLine unit classes) per the task.
```

**Task 10 — CLI child death / premature EOF (all three providers)**
```
Execute "### Task 10: Raise StreamError on CLI child death / premature EOF in Python CLI
providers" from the plan. Prerequisite: Task 9 is already committed on this branch — it edited
providers/claude_code.py INCLUDING the import line; if StreamError is already imported there,
MERGE the import edit instead of duplicating it, and adapt surrounding line refs. In each of
codex_cli.py, gemini_cli.py, claude_code.py stream() loops: EOF currently breaks silently without
checking whether a terminal event arrived; track terminal receipt, and on EOF-without-terminal
await process.wait() (bounded), read captured stderr, and raise StreamError with returncode +
stderr excerpt per the plan's format. Do NOT touch the per-read asyncio.wait_for timeouts. Tests:
extend test_codex_cli_stream.py, test_gemini_cli_stream.py, test_claude_code_runtime.py — fake CLI
prints one valid event line then exits 1 with stderr "boom" → the event is yielded, then
StreamError containing "boom" and the returncode.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/py-retry-error-surfacing.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) error MESSAGES follow the anthropic.py format exactly (Task 4) / StreamError is raised
with the plan's format (Tasks 8-10) — no new exception classes, no public API changes; (3) lint
ran as `uv run ruff check motosan_ai/` only and is clean, ruff format applied (paste output);
(4) `uv run pytest` green for every touched test file (paste output); (5) no scope creep (per-read
timeouts untouched, no M2 structured-field work); (6) the commit exists with the task's
conventional message. Report any deviation. Do not fix — just report, so I decide.
```

## After Task 10 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/py-retry-error-surfacing/sdks/python
uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration
cd .. && git push -u origin fix/py-retry-error-surfacing
gh pr create \
  --title "fix(python): make 5xx retryable and surface swallowed stream errors" \
  --body "$(cat <<'EOF'
## Summary
- openai / minimax / chatgpt_codex now embed `HTTP {status}` + `Retry-After` in raised
  `ProviderError`s (anthropic.py's format), so `retry.py` actually retries their genuine 5xx —
  previously these providers were NEVER retried; codex also stops dropping the Retry-After header.
  MiniMax additionally checks status before parsing the body (non-JSON 5xx no longer crashes).
- Anthropic stream: mid-stream `error` frames (e.g. `overloaded_error` on HTTP 200) raise
  `StreamError` instead of falling through silently.
- claude_code: `is_error: true` terminal results raise in BOTH stream and blocking paths
  (closes a Python-only gap — Rust already errored).
- codex_cli / gemini_cli / claude_code streams: child death / EOF-without-terminal-event raises
  `StreamError` with returncode + stderr instead of ending as a clean truncated stream.
- Regression tests: respx non-JSON 5xx retry per provider; mid-stream error fixture; fake-CLI
  children dying mid-stream per backend.
- Out of scope (deliberate): structured error fields (M2), read-timeout knobs (M3).

M1 plan Tasks 4, 8–10 (PR-P1): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. PR-P2 (Tasks 15–16) starts only AFTER this merges (same files). Remove the worktree once merged.

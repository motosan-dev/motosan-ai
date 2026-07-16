# M2 Long-Task Executor Prompt (one full PR group per run)

Paste this whole block as ONE prompt to a long-running executor agent (or follow it directly in a goal-gated session), filling in `GROUP =`.
Current state: PR-R1 (#220), PR-R2 (#221), PR-P (#222) are MERGED. Runnable now: PR-S, PR-T (parallel). Then PR-C (after T), then PR-REL.
Multi-group runs: when instructed to execute several GROUPs sequentially in one session (e.g. PR-S then PR-T), complete each group fully — its own worktree, branch, gate, and opened PR — before starting the next; groups never share a branch.

```
You are executing ONE full PR group of a written implementation plan, end-to-end and autonomously,
in the motosan-ai monorepo. Deliver an opened PR; do not stop between tasks for confirmation.

FILL THIS IN:
  GROUP = PR-T         # ← pick one: PR-S | PR-T | PR-C | PR-REL   (PR-R1, PR-R2, PR-P already merged)

GROUP TABLE (use the row for your GROUP):
  PR-S    task 1         branch docs/m2-retry-spec          worktree m2-spec         gate: (docs only — no gate)
  PR-T    tasks 10,11,12 branch feat/m2-ts-retry            worktree m2-ts           gate: npm run typecheck && npm run build && npm test   (from sdks/typescript)
  PR-C    task 13        branch test/m2-retry-conformance   worktree m2-conformance  gate: (rust) cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features ; (python) uv run ruff check motosan_ai/ && uv run pytest tests/test_retry_conformance.py -v ; (ts) npm run typecheck && npm run build && npm test   [PREREQ: PR-T merged (R2+P already are)]
  PR-REL  task 14        branch chore/m2-release            worktree m2-release      gate: check-all && (cd sdks/typescript && npm run typecheck && npm run build && npm test)   [PREREQ: PR-C merged; do NOT tag or publish]

SETUP (run once):
  cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
  git worktree add ../motosan-worktrees/<worktree> origin/main -b <branch>
  # if the group touches Python (PR-C, PR-REL): cd ../motosan-worktrees/<worktree>/sdks/python && uv sync --all-extras
  Work entirely inside /Users/daiwanwei/Projects/wade/motosan-worktrees/<worktree>.

PLAN (read these first, in the worktree):
  docs/superpowers/plans/2026-07-15-stream-retry-m2-implementation.md   — the tasks (read "## Global Constraints" then ONLY your group's tasks)
  docs/superpowers/plans/2026-07-15-stream-retry-M2-PROMPT-SHEET.md     — per-task notes + close-out for your group

PROCEDURE — for EACH task in your group, in ascending order (sequential; each consumes the prior task's interfaces):
  1. Read the task section fully. Line refs are approximate (baseline origin/main @ d7c06ff; R1/R2/P have merged since) — ground every edit in the REAL files; if code drifted from an earlier task or a merged PR, adapt and note it.
  2. TDD strictly: write the failing test → run it and CONFIRM it fails with the expected signature → implement the minimal change → run it and confirm it passes → run the touched package's suite → format/lint. Never skip the red step; paste real command output at every run.
  3. Honor the locked design (do not deviate even if it compiles): RETRY_AFTER cap 60s; full jitter = uniform [0, exp_delay]; retry-after used verbatim (no jitter); retryable statuses 408/409/429/≥500; on_retry lives ON RetryPolicy and fires only inside the shared engine; CLI backends get NO transport retry; Rust MotosanError Display strings stay byte-identical; Python is additive-only (LlmClient Protocol untouched; with_retry old positional callers still work; decimal Retry-After preserved, negative→None); Rust retry internals stay pub(crate) (Task 13 Rust conformance is an in-crate #[cfg(test)] mod, not tests/).
  4. M1 REGRESSION CONTRACT (hard gate): the M1 retry tests must pass unchanged — non-JSON 5xx → exactly 2 requests → success, and stream retry only before the first emitted event. Modifying or deleting existing tests to make them pass is unacceptable. If one breaks, you changed behavior — STOP and report.
  5. Commit exactly per the task's final step (conventional message + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`).
  6. SELF-REVIEW before the next task: red-shown-before-green, the locked-design points hold, no scope creep, gate-relevant commands green. If a task's own text contradicts "## Global Constraints", Global Constraints win.

CI-MATCHING GATES (critical — a prior PR went red here):
  - Rust clippy MUST use `--all-targets` so test code is linted (CI does; a bare clippy passes locally while CI fails on e.g. clippy::await_holding_lock). Never hold a MutexGuard across an `.await` in a test — drain into a local first.
  - TS full suite is always `npm run build && npm test` (pack-smoke needs dist/).
  - Fresh Python worktree: `uv sync --all-extras` in sdks/python before the first push (pre-push hook runs the full Python suite; missing respx → push blocked).

AFTER THE LAST TASK IN THE GROUP:
  - Run the group's full gate (from the table). Must be green — paste the output.
  - Push in the BACKGROUND (pre-push hook runs the full Py+Rust+live suite, ~1–2 min > a 2-min foreground limit): git push -u origin <branch>
  - Open the PR: gh pr create --title "<the close-out title from the PROMPT-SHEET for this group>" --body "<summary of the tasks + 'M2 <GROUP>: docs/superpowers/plans/2026-07-15-stream-retry-m2-implementation.md' + a 🤖 Generated with [Claude Code](https://claude.com/claude-code) line>"
  - Do NOT merge. Do NOT tag or publish (even PR-REL — the maintainer tags/publishes).
  - Report back: the PR number/URL, commit SHAs, gate-output summary, and anything you adapted vs. the plan.

STOP CONDITIONS (report and halt, do not improvise): a step is blocked; the plan conflicts with the real code in a way the Global Constraints don't resolve; the M1 regression tests fail; a gate command fails after a genuine fix attempt; or your group's PREREQ is not yet merged into origin/main.
```

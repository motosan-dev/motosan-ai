# M3 Long-Task Executor Prompt (one or more PR groups per run)

Paste as ONE prompt to an executor (or follow directly in a goal-gated session), filling `GROUP =`. Multi-group runs: complete each group fully (own worktree, branch, gate, opened PR) before the next; groups never share a branch.
M3 is BREAKING (stream EOF semantics + Rust enum variant). Merge ordering: PR-S merges FIRST (its spec is the normative terminal-event reference); PR-R/P/T may be OPENED before S merges but MUST NOT merge before it; PR-C after R+P+T merge; PR-REL last.

```
You are executing PR group(s) of a written implementation plan, end-to-end and autonomously,
in the motosan-ai monorepo. Deliver opened PR(s); do not stop between tasks for confirmation.

FILL THIS IN:
  GROUP = PR-R        # ← one of: PR-S | PR-R | PR-P | PR-T | PR-C | PR-REL (or a stated sequence)

GROUP TABLE:
  PR-S    task 1       branch docs/m3-stream-contract     worktree m3-spec   gate: docs only — NO CI checks exist for docs-only PRs in this repo (by design); the deliverable proof is the diff itself
  PR-R    tasks 2,3,4  branch feat/m3-rust-stream-timeout worktree m3-rust   gate: cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features   (from sdks/rust)
  PR-P    tasks 5,6    branch feat/m3-python-stream-timeout worktree m3-python gate: uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration   (from sdks/python)
  PR-T    tasks 7,8    branch feat/m3-ts-stream-timeout   worktree m3-ts     gate: npm run typecheck && npm run build && npm test   (from sdks/typescript)
  PR-C    task 9       branch test/m3-stream-conformance  worktree m3-conf   gate: all three SDK gates above   [PREREQ: PR-R + PR-P + PR-T merged]
  PR-REL  task 10      branch chore/m3-release            worktree m3-release gate: check-all && (cd sdks/typescript && npm run typecheck && npm run build && npm test)   [PREREQ: PR-C merged; NO tag, NO publish]

SETUP (once per group):
  cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
  git worktree add ../motosan-worktrees/<worktree> origin/main -b <branch>
  # Python-touching groups (PR-P, PR-C, PR-REL): cd ../motosan-worktrees/<worktree>/sdks/python && uv sync --all-extras
  Work entirely inside the worktree. The plan files are COMMITTED — read them from the worktree.

PLAN:
  docs/superpowers/plans/2026-07-16-stream-retry-m3-implementation.md   — read "## Global Constraints" (locked E1–E9 + blessed E4 narrowing + length-cap waiver) then ONLY your group's tasks
  docs/superpowers/plans/2026-07-14-stream-retry-milestones.md § M3     — scope context

PROCEDURE — per task, ascending order (sequential within a group):
  1. Read the task fully. Line refs approximate (baseline acf5d7f); ground every edit in REAL files; adapt to drift from earlier tasks in the same PR and say so.
  2. TDD strictly: failing test → confirm red with the expected signature → minimal implement → green → package suite → format/lint. Paste real output every run.
  3. Locked design (deviation = wrong even if it compiles): IncompleteStream (Rust) / IncompleteStreamError subclass-of-StreamError (Py/TS) / CancelledError (TS) / StreamReadTimeoutError (Py); adapter-level EOF enforcement; OpenAI terminal is STRICTLY [DONE] (finish_reason alone is truncation); timeouts connect 10s / read-idle 120s (streaming reads ONLY — blessed narrowing) / total None (chat-only); Rust build-once providers + shared reqwest client; TS caller-signal → CancelledError never retried, fetch-internal AbortError stays retryable; CancelledError conformance row is TS-ONLY.
  4. PINNED-BEHAVIOR FLIPS: M3 deliberately retires the v0.10.1 "exactly one done on truncated EOF" invariant. ONLY the tests named in your task's flip list may change; every other M1/M2 test (retry, conformance, stream guards) MUST pass unchanged. Editing a test not in the flip list to make it pass is unacceptable — stop and report instead.
  5. Commit per the task's final step (conventional + Co-Authored-By line).
  6. Self-review before the next task: red-was-shown, E-conformance, flip list respected, no scope creep, gate commands green.

CI-MATCHING GATES: Rust clippy MUST use --all-targets (CI lints tests; never hold a MutexGuard across an await in a test). TS full suite always builds first (pack-smoke needs dist/). Fresh Python worktree needs uv sync --all-extras before first push (pre-push hook runs the full suite).

AFTER THE LAST TASK OF EACH GROUP:
  - Run the group's full gate — green, paste output.
  - Push in the BACKGROUND (pre-push hook takes 1–2 min): git push -u origin <branch>
  - gh pr create — titles: PR-S "docs(specs): stream termination contract + cancellation semantics (M3)"; PR-R "feat(rust)!: IncompleteStream, build-once providers, timeout model (M3, BREAKING)"; PR-P "feat(python): IncompleteStreamError, timeout model, client lifecycle (M3)"; PR-T "feat(ts): IncompleteStreamError, timeouts, AbortSignal cancellation (M3)"; PR-C "test(stream): M3 termination + read-idle conformance gates"; PR-REL "chore(release): M3 — Rust 0.24.0 / Python 0.17.0 / TS 0.14.0". Body: task summary + "M3 <GROUP>: docs/superpowers/plans/2026-07-16-stream-retry-m3-implementation.md" + 🤖 Generated with [Claude Code](https://claude.com/claude-code)
  - Do NOT merge. Do NOT tag or publish. Note in the PR body that PR-R/P/T must not merge before PR-S.
  - Report: PR number/URL, commit SHAs, gate summary, adaptations vs the plan.

STOP CONDITIONS (report and halt): blocked step; plan-vs-code conflict Global Constraints can't resolve; a non-flip-list M1/M2 test fails; gate fails after a genuine fix attempt; PREREQ not merged.
```

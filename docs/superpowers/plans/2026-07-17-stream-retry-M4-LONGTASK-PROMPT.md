# M4 Long-Task Executor Prompt (one or more PR groups per run)

Paste as ONE prompt to an executor (or follow directly in a goal-gated session), filling `GROUP =`. Multi-group runs: complete each group fully (own worktree, branch, gate, opened PR) before the next; groups never share a branch.
M4 is BREAKING for Rust and Python (CLI chat/stream contract; Python typed thinking events), minor for TS. Merge ordering: PR-S merges FIRST (normative spec reference); PR-F merges BEFORE PR-R (PR-R's code targets the post-refactor `src/transport/` layout); PR-R/P/T may be OPENED before their prereqs merge but MUST NOT merge before them; PR-REL last.

```
You are executing PR group(s) of a written implementation plan, end-to-end and autonomously,
in the motosan-ai monorepo. Deliver opened PR(s); do not stop between tasks for confirmation.

FILL THIS IN:
  GROUP = PR-F        # ← one of: PR-S | PR-F | PR-R | PR-P | PR-T | PR-REL (or a stated sequence)

GROUP TABLE:
  PR-S    task 2          branch docs/m4-vocab-cli-token-spec     worktree m4-spec     gate: docs only — NO CI checks exist for docs-only PRs in this repo (by design); the deliverable proof is the diff itself
  PR-F    task 1          branch refactor/m4-rust-feature-arch    worktree m4-featarch gate: cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features && cargo check --no-default-features && cargo hack check --each-feature   (from sdks/rust; install cargo-hack first)
  PR-R    tasks 3,4       branch feat/m4-rust-cli-token           worktree m4-rust     gate: same Rust gate incl. cargo hack   [PREREQ: PR-S and PR-F merged]
  PR-P    tasks 5,6,7,8   branch feat/m4-python-vocab-cli-token   worktree m4-python   gate: uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration   (from sdks/python)   [PREREQ: PR-S merged]
  PR-T    task 9          branch feat/m4-ts-token-source          worktree m4-ts       gate: npm run typecheck && npm run build && npm test   (from sdks/typescript; build BEFORE test — pack-smoke needs dist/)   [PREREQ: PR-S merged]
  PR-REL  task 10         branch chore/m4-release                 worktree m4-release  gate: check-all && (cd sdks/typescript && npm run typecheck && npm run build && npm test)   [PREREQ: all other M4 PRs merged; NO tag, NO publish]

SETUP (once per group):
  cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
  git worktree add ../motosan-worktrees/<worktree> origin/main -b <branch>
  # Python-touching groups (PR-P, PR-REL): cd ../motosan-worktrees/<worktree>/sdks/python && uv sync --all-extras
  Work entirely inside the worktree. The plan files are COMMITTED — read them from the worktree.

PLAN:
  docs/superpowers/plans/2026-07-17-stream-retry-m4-implementation.md   — read "## Global Constraints" (locked F1–F7) then ONLY your group's tasks
  docs/superpowers/specs/2026-07-17-rust-feature-architecture-design.md — normative for task 1 (incl. its two Amendments)
  docs/superpowers/plans/2026-07-14-stream-retry-milestones.md § M4     — scope context

PROCEDURE — per task, ascending order (sequential within a group):
  1. Read the task fully. Line refs are approximate (baseline b9bcc3e); ground every edit in REAL files; adapt to drift from earlier tasks in the same PR and say so.
  2. TDD strictly: failing test → confirm red with the expected signature → minimal implement → green → package suite → format/lint. Paste real output every run.
  3. Locked design (deviation = wrong even if it compiles): F1 umbrella features `_http`/`_cli` + `src/transport/` strata + tokio-stream unconditional; F2/F3 StreamEventType vocabulary `thinking_delta`/`thinking_done` (no `done` member; Python migrates off the ad-hoc "thinking" string and adds thinking_done emission + collector priority); F4 CLI backends ALWAYS terminate with end_turn on both paths, NEVER tool_use, chat() = collect(stream()) delegation, tool_calls = record of CLI-executed tools, model-backfill is the one parity exception, failure surface shifts to stream-path error variants (documented BREAKING); F5 per-attempt token sources (Rust `auth::TokenSource` + `send_with_retry_async_build`, Python `token_source` callable, TS `accessToken: string | (() => Promise<string>)`), SDKs stay decoupled from the oauth crates; F6 Python `Provider.claude_code` wired (class is ClaudeCodeClient); F7 versions 0.25.0/0.18.0/0.15.0, no tag/publish in PR-REL.
  4. PINNED-BEHAVIOR FLIPS: only the tests named in YOUR task's "Flip list" block may be modified/renamed/deleted, exactly as the task specifies; every other M1/M2/M3 test (retry, stream termination, conformance) MUST pass unchanged. If any test outside your flip list fails and a genuine fix attempt doesn't resolve it, STOP and report — do NOT edit it to pass.
  5. Commit per the task's steps (conventional + Co-Authored-By line).
  6. Self-review before the next task: red-was-shown, F-conformance, flip list respected, no scope creep, gate commands green.

CI-MATCHING GATES: Rust clippy MUST use --all-targets (CI lints tests; never hold a MutexGuard across an await in a test). TS full suite always builds first. Fresh Python worktree needs uv sync --all-extras before first push (pre-push hook runs the full suite, takes 1–2 min).

AFTER THE LAST TASK OF EACH GROUP:
  - Run the group's full gate — green, paste output.
  - Push in the BACKGROUND (pre-push hook takes 1–2 min): git push -u origin <branch>
  - gh pr create — titles: PR-S "docs(specs): stream event vocabulary, CLI contract, token sources (M4)"; PR-F "refactor(rust): _http/_cli umbrella features + transport strata (M4)"; PR-R "feat(rust)!: CLI end_turn contract + per-attempt TokenSource (M4, BREAKING)"; PR-P "feat(python)!: typed thinking events, CLI end_turn contract, token_source, claude_code (M4, BREAKING)"; PR-T "feat(ts): per-attempt token source for chatgpt_codex (M4)"; PR-REL "chore(release): M4 — Rust 0.25.0 / Python 0.18.0 / TS 0.15.0". Body: task summary + "M4 <GROUP>: docs/superpowers/plans/2026-07-17-stream-retry-m4-implementation.md" + 🤖 Generated with [Claude Code](https://claude.com/claude-code)
  - Do NOT merge. Do NOT tag or publish. Note in the PR body which prereq PRs must merge first (per the ordering above).
  - Report: PR number/URL, commit SHAs, gate summary, flip-list items applied, adaptations vs the plan.

STOP CONDITIONS (report and halt): blocked step; plan-vs-code conflict Global Constraints can't resolve; a test outside your flip list fails after a genuine fix attempt; gate fails after a genuine fix attempt; PREREQ not merged.
```

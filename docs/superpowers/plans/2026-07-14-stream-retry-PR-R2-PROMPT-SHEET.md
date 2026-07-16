# PR-R2 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 5–7 of the M1 plan — Rust stream error surfacing: Anthropic mid-stream `error` frames, claude_code error-subtype terminals, CLI child crash / premature EOF. One branch, one PR.

One fresh subagent per task, **in order 5→6→7** (Task 7 MUST come after Task 6 — both edit `claude_code/mod.rs`). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Runs in parallel with PR-P1 / PR-T1 (zero shared files). Overlaps PR #211 only by FILE (`anthropic.rs` — distant regions: #211 touched `chat()` ~492-516, this PR touches the stream adapter ~916-1116); if GitHub reports a conflict at merge time, rebase on main after #211 merges.

---

## Setup (run once, before Task 5 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/rust-stream-error-surfacing origin/main -b fix/rust-stream-error-surfacing
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-stream-error-surfacing`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/rust-stream-error-surfacing):
/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-stream-error-surfacing
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → cargo fmt → cargo clippy --all-features -- -D warnings → commit.
  This is TDD; do not skip the red step.
- All cargo commands run from sdks/rust/.
- The plan was authored against origin/main @ 3e3f413; ALL line numbers are approximate. Ground
  every edit in the real files — if code drifted from a quoted hunk, adapt to reality and say so.
- M1 boundary: surface errors that are silently swallowed TODAY. Do NOT change the variant of any
  path that already errors, do NOT touch clean-EOF fabricated-done behavior (that is milestone M3),
  do NOT touch cancellation/kill_on_drop (already correct).
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/fmt/clippy step. Never claim success without
  showing green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 5 — Anthropic mid-stream error frames**
```
Execute "### Task 5: Surface Anthropic mid-stream SSE error frames as stream errors (Rust)" from
the plan. Add an "error" arm to the poll_next event-type match in AnthropicStreamAdapter (the edit
point is right before the `_ => continue` catch-all, near the "message_stop" arm): extract
error.type + error.message from the payload and return
Poll::Ready(Some(Err(MotosanError::Stream(...)))) with the plan's exact message format. Pattern
source (already proven + tested): chatgpt_codex.rs ~482-541. Test: APPEND to
sdks/rust/tests/anthropic_stream.rs — stream sends message_start, one text delta, then an
overloaded_error frame → collecting yields Err containing "overloaded_error", AND the text delta
before the error was still delivered. Do not touch chat(), message_stop, or the EOF path.
```

**Task 6 — claude_code error-subtype terminals**
```
Execute "### Task 6: Surface claude_code error-subtype terminal events instead of silently
dropping them" from the plan. Three files: stream_json.rs (make the Result variant's `result`
field #[serde(default)] so error_max_turns / error_during_execution terminals no longer fail
serde and vanish; branch on is_error/subtype in the Result match arm), spawn.rs
(parse_agent_json gets the same is_error/subtype branch for the blocking path), mod.rs is NOT
edited (drive_lines Error arm stays as-is — read the task's 3c/3d notes). CRITICAL variant
boundary (stated in the task): every claude_code error terminal surfaces as the EXISTING
MotosanError::ProviderError — the previously-vanishing cases newly REACH that variant; do NOT
introduce MotosanError::Stream anywhere in this task. Tests: extend the in-file #[cfg(test)]
mod tests blocks of stream_json.rs and spawn.rs (no separate test file exists), including the
{"type":"result","subtype":"error_max_turns","is_error":true} line WITHOUT a result field.
```

**Task 7 — CLI child crash / premature EOF (all three backends)**
```
Execute "### Task 7: Surface CLI child crash / premature EOF as stream errors in all three Rust
CLI backends" from the plan. Prerequisite: Task 6 is already committed on this branch (same file
claude_code/mod.rs, different regions). In each of claude_code/mod.rs, codex_cli/mod.rs,
gemini_cli/mod.rs: the drive_lines read loop currently breaks SILENTLY on read Err and on
EOF-without-terminal-event; track terminal receipt, and on those paths await the child (bounded),
capture exit status + buffered stderr, and yield Err(MotosanError::Stream(...)) with the plan's
message format. Do NOT touch kill_on_drop/reap logic. The plan writes out ALL THREE backend tests
in full (fake `sh -c` children that print one valid event line then exit 1 with stderr) — each
backend's event-line format differs, use them verbatim. Run with --all-features so every backend
compiles.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-stream-error-surfacing.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) errors are SURFACED, not remapped — no pre-existing error path changed its variant
(Task 6: everything is ProviderError; Tasks 5/7: new errors use MotosanError::Stream); (3) no
M3 scope creep — clean-EOF fabricated-done and cancellation code untouched; (4) cargo fmt no diff
+ cargo clippy --all-features -- -D warnings clean (paste output); (5) cargo test --all-features
green for the touched tests (paste output); (6) the commit exists with the task's conventional
message. Report any deviation. Do not fix — just report, so I decide.
```

## After Task 7 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-stream-error-surfacing/sdks/rust
cargo fmt && cargo clippy --all-features -- -D warnings && cargo test --all-features
cd .. && git push -u origin fix/rust-stream-error-surfacing
gh pr create \
  --title "fix(stream): surface swallowed mid-stream errors in Rust adapters" \
  --body "$(cat <<'EOF'
## Summary
- Anthropic stream adapter: mid-stream `error` frames (e.g. `overloaded_error` on HTTP 200) now
  surface as `MotosanError::Stream` instead of being dropped by the catch-all — truncated streams
  no longer collect as clean successes.
- claude_code: terminal results with error subtypes (`error_max_turns`, `error_during_execution`)
  no longer vanish on serde (`result` is now `#[serde(default)]`); they reach the SAME
  `ProviderError` variant the existing `is_error` path already used (no variant changes).
- All three CLI backends (claude_code / codex_cli / gemini_cli): read errors and
  EOF-without-terminal-event now await the child and yield a typed stream error carrying exit
  status + stderr, instead of ending the stream as if it completed.
- New regression tests per adapter, including fake `sh -c` children that die mid-stream.
- Out of scope (deliberate, M3): clean-EOF fabricated-done behavior, cancellation paths.

M1 plan Tasks 5–7 (PR-R2): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. If GitHub flags an `anthropic.rs` conflict with #211, rebase on main after #211 merges (regions are distant; auto-merge normally succeeds). Remove the worktree once merged.

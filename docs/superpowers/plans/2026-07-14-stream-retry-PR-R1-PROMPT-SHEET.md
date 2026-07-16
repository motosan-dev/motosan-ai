# PR-R1 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 1–3 of the M1 plan — the three Rust `chat()` retry parse-order fixes (anthropic / openai / ollama). One branch, one PR.

One fresh subagent per task, in order (1→2→3). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks before moving on. The three tasks touch disjoint files but share a branch — do not parallelize.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Milestone context: `docs/superpowers/plans/2026-07-14-stream-retry-milestones.md` (this is M1 / workstream W1, Rust half)

---

## Setup (run once, before Task 1 — you, not a subagent)

The main working tree may be on another branch; per house rule, implement in a worktree off the **current** `origin/main`:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/rust-retry-parse-order origin/main -b fix/rust-retry-parse-order
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-retry-parse-order`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/rust-retry-parse-order):
/Users/daiwanwei/Projects/wade/motosan-worktrees/rust-retry-parse-order
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
- Do NOT expand scope beyond your task. Do NOT touch stream() in any provider — the streaming
  paths already handle retry correctly. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/fmt/clippy step. Never claim success without
  showing green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 1 — anthropic.rs chat() parse-order**
```
Execute "### Task 1: Fix Anthropic chat() parsing response body before retryable-status check"
from the plan. Restructure the chat() retry loop so status + Retry-After decide retryability
BEFORE any body parse; parse the body only on success (parse failure still propagates as
ProviderError) or when building the terminal error (json!({}) fallback so extract_error_message
still works). Step 1 also updates the test-file import to add RetryPolicy (rustfmt-wrapped, see
the plan). The mockito .expect(1) on BOTH mocks plus assert_async() is what proves exactly 2
requests were made — keep it. Note: Provider::Minimax routes through AnthropicProvider, so this
fix covers MiniMax too; no extra code needed. Test: sdks/rust/tests/anthropic_chat.rs.
```

**Task 2 — openai.rs chat() parse-order**
```
Execute "### Task 2: Fix Rust OpenAI chat() retry order: decide retry before parsing error body"
from the plan. Same restructure as Task 1 but in OpenAIProvider::chat() (approx lines 501-526 at
baseline). The correct reference is in the SAME file: stream() (approx 617-634) already checks
status first — mirror its shape, and do NOT modify stream(). Extend
sdks/rust/tests/openai_retry.rs, which already has 5 mockito-based retry tests — match their
style exactly (tiny-delay RetryPolicy, .expect(N) mocks, assert_async()).
```

**Task 3 — ollama.rs chat() parse-order**
```
Execute "### Task 3: Fix Rust Ollama chat() retry parse-order bug (non-JSON 5xx aborts retry)"
from the plan. Same restructure in OllamaProvider::chat() (approx lines 266-285 at baseline);
the helpers (is_retryable_status / parse_retry_after / sleep_before_retry / extract_error_message
/ map_http_error) are already imported — no import changes. This provider is gated by the
ollama_native feature: run tests with --all-features (as the plan's commands do) so the file is
compiled. Test: sdks/rust/tests/ollama_native_provider.rs (extend the existing mockito tests).
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-retry-parse-order.
Verify, with evidence: (1) the failing test was written and shown RED before the implementation,
and is now green; (2) the restructure decides retryability from status + Retry-After BEFORE any
body parse, and the terminal error path uses a tolerant body parse (json!({}) fallback); (3)
stream() was NOT touched; (4) cargo fmt produced no diff and cargo clippy --all-features -- -D
warnings is clean (paste output); (5) cargo test --all-features for the touched test file is green
(paste output); (6) no scope creep; (7) the commit exists with the task's conventional message.
Report any deviation. Do not fix — just report, so I decide.
```

## After Task 3 (PR close-out)

Run the full gate from `sdks/rust/`, then open the PR (all `.rs` changes land via PR + CI — never direct to main):

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/rust-retry-parse-order/sdks/rust
cargo fmt && cargo clippy --all-features -- -D warnings && cargo test --all-features
cd .. && git push -u origin fix/rust-retry-parse-order
gh pr create \
  --title "fix(retry): decide retryability before parsing chat response bodies" \
  --body "$(cat <<'EOF'
## Summary
- anthropic/openai/ollama `chat()` parsed the response body as JSON **before** the
  `is_retryable_status` check, so a 502/503/529 with an HTML or empty body (the canonical
  proxy/LB failure) aborted on attempt 1 with a misleading JSON-decode error instead of retrying.
- Restructured all three retry loops to the proven `gemini.rs` shape: status + Retry-After decide
  retryability first; the body is parsed only on success or (tolerantly) for the terminal error.
- Also covers `Provider::Minimax` (routes through `AnthropicProvider`). `stream()` paths were
  already correct and are untouched.
- New regression tests: non-JSON 5xx body → exactly 2 requests → success
  (`anthropic_chat.rs`, `openai_retry.rs`, `ollama_native_provider.rs`).

M1 plan Tasks 1–3 (PR-R1): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
Audit context: docs/superpowers/plans/2026-07-14-stream-retry-milestones.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. After merge: PR-R2 (Tasks 5–7) and PR-P1 (Tasks 4, 8–10) are independent of this PR and can start next; remove the worktree with `git worktree remove ../motosan-worktrees/rust-retry-parse-order` once merged.

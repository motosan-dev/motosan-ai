# Subagent Prompts — Anthropic OAuth Implementation

Copy-paste prompts for handing the implementation plan
(`2026-05-18-anthropic-oauth.md`) to an external subagent task-by-task.

**Tasks must run sequentially** — each one depends on type/struct changes
from the previous one. Do not parallelize.

---

## Step 0: One-time setup (run yourself, NOT the subagent)

The subagent will refuse to start unless the branch already exists. Do this
once before dispatching Task 1:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai

# Push any pending docs commits on main (spec/plan files) to origin
git push origin main

# Create the implementation branch and push so the subagent can push commits later
git checkout -b feat/anthropic-oauth
git push -u origin feat/anthropic-oauth

# Confirm
git status --short --branch
# Expected: ## feat/anthropic-oauth...origin/feat/anthropic-oauth
```

---

## Master prompt template

Use this for every task. Replace `<TASK_NUMBER>` and `<STEP_RANGE>` per the
task table below.

```
You are implementing a feature in motosan-ai. Read these before starting:

  Spec:  docs/superpowers/specs/2026-05-18-anthropic-oauth-design.md
  Plan:  docs/superpowers/plans/2026-05-18-anthropic-oauth.md
  Rules: CLAUDE.md  (build/test commands, what NOT to do)

Hard rules — ABORT and report if any would be violated:
- Work on branch `feat/anthropic-oauth`. Never push to main. Never force-push.
- Never `git commit --amend`. If a step fails, fix and create a NEW commit.
- Never skip hooks (--no-verify) or signing.
- Every code task ends with `cargo fmt --all` BEFORE the commit step.
- Use exact code from the plan. Do not "improve" it. Do not refactor outside the task scope.
- If a step's expected output does not match, STOP and report — do not proceed to the next step.

Workflow:
1. Read the spec + plan top-to-bottom once.
2. Verify you are on `feat/anthropic-oauth` (`git status`); if not, abort.
3. Execute Task <TASK_NUMBER>'s steps in order, exactly as written.
4. After the final commit in Task <TASK_NUMBER>, run `git log --oneline -3` and report:
   - The commit SHA + subject
   - Output of the test command in the last verification step
   - Any deviations you had to make (ideally none)
5. STOP. Do not start Task <TASK_NUMBER + 1>.

Execute Task <TASK_NUMBER> (steps <STEP_RANGE>) of the plan.
```

---

## Task table

| Task | Step range | One-line summary | Extra prompt notes |
|------|------------|------------------|--------------------|
| 1    | 1–10       | Add `extra_auth_params` + `TokenBodyFormat` enum; update codex/gemini configs to preserve current behavior | — |
| 2    | 1–8        | Add `redirect_uri_host` field + `build_redirect_uri` helper | — |
| 3    | 1–10       | Parametrize `callback_path` in `lib.rs::login` and `server.rs::is_callback_request` | — |
| 4    | 1–9        | Parametrize token body format (`Form` vs `Json`) in `exchange.rs`; mockito tests | — |
| 5    | 1–7        | Add Anthropic provider config (feature-gated `anthropic`) | — |
| 6    | 1–7        | New `anthropic-oauth` crate (Cargo + lib + refresh integration test) | — |
| 7    | 1–15       | Docs + release tooling: 3 per-crate CHANGELOGs, `publish-anthropic-oauth.yml`, llms.txt updates × 5, README, AGENTS, version bumps | See Task 7 note below |
| 8    | 1–5        | Live `#[ignore]`'d login smoke test (compile only) | See Task 8 note below |
| 9    | 1–4        | Full workspace check + open PR | See Task 9 note below |

---

## Per-task extra prompt notes

Append these to the master prompt for the relevant task.

### Task 7

```
Task 7 has 15 steps and touches many files. Do not consolidate steps or skip
the explanation paragraphs in the plan — they contain "do NOT touch
sdks/rust/CHANGELOG.md" guidance that is easy to violate. The single
commit at Step 15 should include exactly the file list shown in that step,
no more, no less.
```

### Task 8

```
The live login test opens a real browser and requires a Claude Pro/Max
subscription. Do NOT run `cargo test --ignored` — only verify the test
compiles. The human will run the live test manually before merge.
```

### Task 9

```
Before pushing, ensure the branch is up to date and CI will be triggered
by the push. If `gh pr create` fails (e.g., already exists), report and
stop — do not retry with different flags.
```

---

## Between-task verification (you run, not the subagent)

After each task's subagent report, verify before dispatching the next:

```bash
git log --oneline -1                                  # commit message matches task subject
git diff HEAD~1 --stat                                # files match plan File Map
cargo check --workspace --all-features 2>&1 | tail -5 # still compiles
```

If anything looks off, paste the failure output back to the subagent and ask
it to investigate and fix in a new commit (not amend).

---

## Live login test (manual, before merge)

After Task 8 lands but before merging the PR, run the live test yourself:

```bash
cargo test -p anthropic-oauth --test login_live -- --ignored
```

A browser will open to `claude.ai/oauth/authorize`. Log in with your
Claude Pro/Max account. The test asserts the returned `access_token` starts
with `sk-ant-oat01-` and `refresh_token` is non-empty. Paste the test's
stderr output into a PR comment as evidence.

---

## After Task 9: PR review and merge

1. Review the PR diff yourself or via `/ultrareview <PR#>`.
2. Wait for CI to go green on the `feat/anthropic-oauth` branch.
3. Add a PR comment with the live login test output (from the previous
   section).
4. Squash-merge or merge — your preference; the plan's commit history is
   already linear and meaningful per task.

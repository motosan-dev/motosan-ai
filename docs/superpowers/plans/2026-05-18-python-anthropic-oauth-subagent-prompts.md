# Subagent Prompts — Python Anthropic OAuth Implementation

Copy-paste prompts for handing the implementation plan
(`2026-05-18-python-anthropic-oauth.md`) to an external subagent task-by-task.

**Tasks must run sequentially** — Tasks 2–5 each add an `OAuthConfig` field
the next task builds on; Task 6 needs all five knobs. Do not parallelize.

---

## Step 0: One-time setup (run yourself, NOT the subagent)

The subagent will refuse to start unless the branch already exists. Do this
once before dispatching Task 1:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git checkout main && git pull
git checkout -b feat/python-anthropic-oauth
git push -u origin feat/python-anthropic-oauth

# Confirm
git status --short --branch
# Expected: ## feat/python-anthropic-oauth...origin/feat/python-anthropic-oauth
```

---

## Master prompt template

Use this for every task. Replace `<TASK_NUMBER>` and `<STEP_RANGE>` per the
task table below.

```
You are implementing a feature in motosan-ai. Read these before starting:

  Spec:  docs/superpowers/specs/2026-05-18-python-anthropic-oauth-design.md
  Plan:  docs/superpowers/plans/2026-05-18-python-anthropic-oauth.md
  Rules: CLAUDE.md  (build/test commands, what NOT to do)

Hard rules — ABORT and report if any would be violated:
- Work on branch `feat/python-anthropic-oauth`. Never push to main. Never force-push.
- Never `git commit --amend`. If a step fails, fix and create a NEW commit.
- Never skip hooks (--no-verify) or signing.
- Every code task ends with `ruff check` + `ruff format` BEFORE the commit step.
- Use exact code from the plan. Do not "improve" it. Do not refactor outside the task scope.
- If a step's expected output does not match, STOP and report — do not proceed to the next step.

Workflow:
1. Read the spec + plan top-to-bottom once.
2. Verify you are on `feat/python-anthropic-oauth` (`git status`); if not, abort.
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
| 1    | 1–12       | Refactor `oauth/google.py` → generic `_flow.py` + `providers/gemini.py`; rename `google_gemini_config` → `gemini_config`; update consumers. Pure move. | See Task 1 note |
| 2    | 1–10       | Add `callback_path` knob — `_callback_server.py` + `login` + `OAuthConfig`; update `test_oauth_callback.py` | — |
| 3    | 1–9        | Add `redirect_uri_host` + `extra_auth_params` knobs; generalize `_build_auth_url` + redirect URI | — |
| 4    | 1–8        | Add `TokenBodyFormat` + `token_body`; generalize `_post_token` (form vs JSON) | — |
| 5    | 1–10       | Add `StateStrategy` + `state_strategy`; `state` derived in `login`; `exchange_code` gains `state` param echoed in token body | — |
| 6    | 1–8        | Add `providers/anthropic.py` (`claude_pro_max_config`); export from `__init__.py`; tests | — |
| 7    | 1–10       | Live test (skipped), README/ToS, CHANGELOG, version bump 0.11.0→0.12.0, llms.txt, PR | See Task 7 note |

---

## Per-task extra prompt notes

Append these to the master prompt for the relevant task.

### Task 1

```
Task 1 is a pure refactor (move + one rename), not a feature. There is no
new behavior. Success = the test suite has the SAME pass count before
(Step 1) and after (Step 10). The renamed test file test_oauth_gemini.py
keeps all its tests. If the pass count drops, STOP and report — something
was lost in the move, do not "fix" it by deleting tests.
```

### Task 7

```
The live login test (test_anthropic_oauth_live.py) is skipped by default.
Do NOT run it with MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 — only verify it is
collected and skipped (Step 2 expects "1 skipped"). The human runs the live
test manually before merge. When opening the PR, if `gh pr create` fails
(e.g. PR already exists), report and stop — do not retry with different flags.
```

---

## Between-task verification (you run, not the subagent)

After each task's subagent report, verify before dispatching the next:

```bash
git log --oneline -1                  # commit message matches task subject
git diff HEAD~1 --stat                # files match the plan's File Map
cd sdks/python && uv run pytest tests/ -q --ignore=tests/integration/   # suite green
```

If anything looks off, paste the failure back to the subagent and ask it to
investigate and fix in a NEW commit (not amend).

---

## Live login test (manual, before merge)

After Task 7 lands but before merging the PR, run the live test yourself:

```bash
cd sdks/python
MOTOSAN_RUN_ANTHROPIC_OAUTH_LIVE=1 uv run pytest \
    tests/integration/test_anthropic_oauth_live.py -v -s
```

A browser opens to `claude.ai/oauth/authorize`. Log in with a Claude Pro/Max
account and approve **promptly** (120 s timeout). The test asserts the token
starts with `sk-ant-oat01-`. Paste the `Live login OK. expires_in=Ns` line
into a PR comment as evidence.

Note from the Rust run: the OAuth token endpoint rate-limits aggressively —
do not retry the live test many times in quick succession. If you hit
HTTP 429, wait before retrying.

---

## After Task 7: PR review and merge

1. Review the PR diff yourself or via `/ultrareview <PR#>`.
2. Wait for CI to go green on the `feat/python-anthropic-oauth` branch.
3. Add a PR comment with the live login test output.
4. Merge (the Rust counterpart used a merge commit to keep per-task history).

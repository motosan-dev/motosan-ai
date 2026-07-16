# PR-REL Execution — Copy-Paste Subagent Prompt Sheet (Task 22, Release)

**Scope:** Task 22 of the M1 plan — the M1 release: bump Rust 0.21.1→**0.22.0**, Python 0.14.0→**0.15.0**, TypeScript 0.11.0→**0.12.0**; changelogs; doc version lines; full gate. One branch, one PR, ONE subagent (it is a single task).

Prerequisite SATISFIED: all 7 M1 code PRs (#211–#217) are merged. This PR touches `Cargo.toml` → lands via PR + CI per house rule. Publishing (tags, crates.io/PyPI/npm) stays with you — the subagent must NOT tag or publish.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md` § "### Task 22"

---

## Setup (run once — you, not the subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/m1-release origin/main -b chore/m1-release
# fresh-worktree gotcha: the pre-push hook runs the FULL Python suite; sync dev deps first
cd ../motosan-worktrees/m1-release/sdks/python && uv sync --all-extras
```

Worktree path used below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m1-release`

---

## Task prompt (single subagent)

```
You are executing the RELEASE task of a written plan. Work in this worktree (branch chore/m1-release):
/Users/daiwanwei/Projects/wade/motosan-worktrees/m1-release
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Read the plan's "## Global Constraints" section, then execute "### Task 22" step by step, exactly
as written (Steps 1-9). Key points, all spelled out in the task — follow them, do not improvise:

- Step 1 (preflight): verify the CURRENT versions really are Rust 0.21.1 / Python 0.14.0 /
  TS 0.11.0 and capture `git log --oneline 3e3f413..HEAD` — that commit range holds the 7 merged
  M1 PRs (#211-#217) and is the ground truth for every changelog bullet.
- Step 3 (lockfiles): uv.lock lives at the REPO ROOT (run `uv lock --project sdks/python` from
  the root and `git add uv.lock`); the TS package-lock updates via `npm install
  --package-lock-only`; Cargo.lock is GITIGNORED — never `git add` it.
- Step 4 (changelogs): use the pre-drafted entries in the task, replacing <DATE> with today's
  date. The per-SDK bullets were deliberately corrected for accuracy — TypeScript's changelog
  must NOT claim the retry-status or OpenAI-index fixes (TS baseline already had both); do not
  "restore" them for symmetry.
- Step 5 (cross-check): for EVERY bullet, find its commit in the Step 1 log; delete any bullet
  whose fix did not actually merge, add a bullet for any merged M1 fix the lists miss, and move
  any bullets that PRs left under "## [Unreleased]" into the new release sections. Show your
  bullet→commit mapping in the output.
- Step 6 (doc version lines): AGENTS.md, llms.txt (version line + Install example + tag-
  convention table), skills/motosan-ai/SKILL.md (version line + Install example), README.md
  (Languages table + the Install example at ~line 38 that still says 0.18.0). Leave historical
  mentions alone, per the task.
- Step 7 (gate): run check-all inside nix develop if available, otherwise the task's spelled-out
  equivalents — note the TS line requires `npm run build` before `npm test`, and the Python ruff
  check scope is motosan_ai/ only.
- Step 8 (commit): exactly the task's chore(release) message, Co-Authored-By included.
- Step 9: STOP after committing. Do NOT create tags, do NOT publish to crates.io/PyPI/npm —
  report that publishing is handed back to the maintainer per llms.txt § Release.

All line numbers in the task are approximate — ground every edit in the real files. Show the
actual command output for the gate; never claim success without green output. If any step is
blocked or the plan text no longer matches reality, STOP and report the exact problem.
```

## Review prompt (run after the task, before pushing)

```
Review the release commit against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task 22" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/m1-release.
Verify, with evidence: (1) exactly three version bumps — Cargo.toml 0.22.0, pyproject.toml 0.15.0,
package.json 0.12.0 — and the matching lockfile updates (root uv.lock + TS package-lock; Cargo.lock
NOT staged); (2) every changelog bullet maps to a commit in `git log --oneline 3e3f413..HEAD`
(paste the mapping) and NO bullet claims unmerged work — in particular the TS changelog contains
NO retry-status/OpenAI-index bullets; no leftover "<DATE>" placeholders; nothing remains under
"## [Unreleased]" that belongs to this release; (3) doc version lines updated in AGENTS.md,
llms.txt, SKILL.md, README.md (including the 0.18.0 Install example) with historical mentions
untouched; (4) the full gate output is green (check-all or equivalents, TS built before tested);
(5) no source code changed — this commit touches ONLY version/changelog/doc files; (6) no tags
were created and nothing was published. Report any deviation. Do not fix — just report, so I decide.
```

## Close-out (you)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/m1-release
git push -u origin chore/m1-release
gh pr create \
  --title "chore(release): M1 reliability release — Rust 0.22.0 / Python 0.15.0 / TS 0.12.0" \
  --body "$(cat <<'EOF'
## Summary
- Version bumps for the M1 stream/retry reliability release: Rust 0.21.1 → 0.22.0,
  Python 0.14.0 → 0.15.0, TypeScript 0.11.0 → 0.12.0.
- Root + per-SDK changelog entries for the 7 merged M1 PRs (#211–#217), cross-checked
  bullet-by-bullet against `git log 3e3f413..HEAD`.
- Doc version lines updated: AGENTS.md, llms.txt, skills/motosan-ai/SKILL.md, README.md.
- No source changes. Tagging + publishing to follow per llms.txt § Release after merge.

M1 plan Task 22 (PR-REL): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

After CI is green and the PR merges: tag + publish per `llms.txt` § Release (`rust-v0.22.0`, `py-v0.15.0`, `ts-v0.12.0`), optionally run the new `#[ignore]` codex live smoke (`cargo test --features chatgpt-codex --test chatgpt_codex_live -- --ignored`, needs `~/.codex/auth.json`) as a final sanity pass, then remove the worktree. **M1 is then complete** — next up is the M2 implementation plan (structured error metadata + retry consolidation).

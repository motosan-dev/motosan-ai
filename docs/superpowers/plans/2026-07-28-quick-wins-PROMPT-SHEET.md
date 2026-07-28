# Quick-Wins Batch — Goal Prompt Sheet

Paste-ready goal texts for the six tasks of
`docs/superpowers/plans/2026-07-28-correctness-quick-wins.md` (committed on main).
Umbrella issue: **#242**. One goal session per task.

**Parallelism / merge order:** all six are parallel-safe. Zero-conflict merge order is
1 → 2 (shared Rust CHANGELOG `[Unreleased]`) and 3 → 4 (shared Python CHANGELOG +
client.py/minimax.py in non-adjacent hunks); if merged out of order, the later PR
needs a trivial CHANGELOG rebase. Tasks 5 and 6 merge any time.

**Lessons baked in (do not re-litigate):** Tasks 5/6 PRs trigger ZERO CI checks
(all PR workflows are path-filtered to `sdks/**`) — their goal texts contain no
"checks SUCCESS" condition, and executors must not wait for checks that will never
appear. Every push is SHA-verified (`ls-remote | cut -f1` vs local HEAD); a bare
`ls-remote` exit 0 proves nothing. Nobody merges: PRs are opened and reported.

---

## Goal 1 — ThinkStripper UTF-8 panic + zero-copy (PR-A)

```text
GOAL: Execute Task 1 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; create a fresh worktree off origin/main; branch fix/think-stripper-utf8. Read the plan's Global Constraints section AND Task 1 in full before acting; follow the steps exactly and in order (TDD: the multibyte panic test MUST be observed failing before the fix). Umbrella issue already exists: #242. Run `uv sync --all-extras` in sdks/python/ before any push (pre-push hook requirement).

Done when ALL hold (machine-checkable):
1. `cargo test think_stripper::tests` → 10/10 pass (8 existing + 2 new; the panic test observed RED first).
2. Gates green: cargo fmt --check; cargo clippy --all-features --all-targets -- -D warnings; the credential-stripped `env -u … cargo test --all-features` exactly as written in the plan.
3. Commit message is exactly the plan's (bare `fix:` + `(#242)` + Co-Authored-By trailer); push SHA-verified by the plan's test/ls-remote line.
4. PR open against main with the plan's title; `gh pr checks` shows the Rust CI check SUCCESS.
5. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on the same step, or any need to alter an EXISTING test's assertions → STOP and report the blocker verbatim. Never edit tests to force green; never push with --no-verify; never git reset --hard.
```

## Goal 2 — Native model-stream termination contract (PR-B)

```text
GOAL: Execute Task 2 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; fresh worktree off origin/main; branch fix/native-stream-termination. Read the plan's Global Constraints AND Task 2 in full; follow steps in order (the two new EOF tests MUST first fail on the message assertion — if they fail any other way, STOP and report). Umbrella issue: #242. `uv sync --all-extras` in sdks/python/ before any push.

Done when ALL hold:
1. Both new EOF conformance tests pass (openai + chatgpt-codex; asserted payloads "openai ended without a terminal event" / "chatgpt-codex ended without a terminal event"), observed RED first on the message assertion.
2. specs/types.md amended exactly per Step 5: two legacy row labels + one new native row + the "Stream termination (native)" subsection; no other spec claims added.
3. Gates green: fmt / clippy --all-features --all-targets -D warnings / credential-stripped full cargo test as written.
4. Commit per plan (bare `fix:` + `(#242)` + trailer); push SHA-verified; PR open with the plan's title; Rust CI check SUCCESS.
5. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or any EXISTING test needing assertion changes → STOP and report. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 3 — Python central capability enforcement (PR-C)

```text
GOAL: Execute Task 3 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; fresh worktree off origin/main; branch fix/python-capability-enforcement. Read the plan's Global Constraints AND Task 3 in full; follow steps in order. The red phase is deterministic: the five monkeypatch-guarded tests fail with DID-NOT-RAISE instantly, touching no network and no CLI — if anything hangs or hits the network, STOP (the harness is wrong). Umbrella issue: #242. `uv sync --all-extras` in sdks/python/ before any push.

Done when ALL hold:
1. tests/test_capability_enforcement.py: 6/6 pass (5 rejected tests observed RED as DID NOT RAISE first; the no-caps-provider test green throughout).
2. test_provider_capabilities.py flipped: test_minimax_is_text_only passes; full suite `uv run pytest tests/ -q --ignore=tests/integration/` green with ZERO unplanned test flips (any other failing test → STOP and report, per plan Step 6).
3. Gates green: ruff check (with --fix fallback per plan), ruff format --check motosan_ai/ tests/, pytest.
4. Commit per plan (bare `fix:` + BREAKING CHANGE body paragraph + `(#242)` + trailer); push SHA-verified; PR open with the plan's title; Python CI check SUCCESS.
5. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step → STOP and report. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 4 — Typed Python package: mypy-clean + py.typed (PR-D)

```text
GOAL: Execute Task 4 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; fresh worktree off origin/main; branch chore/python-py-typed. Read the plan's Global Constraints AND Task 4 in full; follow steps in order. The exact fix set is pre-verified in the plan — apply it as written; red first: `uv run mypy motosan_ai/` must show the 20 baseline errors before fixing. Umbrella issue: #242. `uv sync --all-extras` in sdks/python/ before any push.

Done when ALL hold:
1. `uv run mypy motosan_ai/` → "Success: no issues found in 26 source files" (observed 20 errors RED first).
2. `uv run pytest tests/ -q --ignore=tests/integration/` green (all fixes behavior-neutral); ruff check + ruff format --check green.
3. Wheel verified: `unzip -l dist/*.whl` shows motosan_ai/py.typed (dist/ then deleted, not committed).
4. treefmt run from repo root after the pyproject edit (taplo WILL expand the dev array — keep its output); `treefmt --fail-on-change` exits 0 at gate time.
5. ci-python.yml has the Type check step; the repo-root uv.lock diff is COMMITTED (uv workspace lockfile — required).
6. Commit per plan (bare `feat:` + `(#242)` + trailer); push SHA-verified; PR open with the plan's title; Python CI check SUCCESS (including the new Type check step).
7. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or mypy reporting errors NOT in the plan's list of 20 (baseline moved) → STOP and report. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 5 — Pre-push gate rewrite + Rust nightly live CI (PR-E)

```text
GOAL: Execute Task 5 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; fresh worktree off origin/main; branch chore/pre-push-path-gate. Read the plan's Global Constraints AND Task 5 in full; the new scripts/pre-push-gate.sh and .github/workflows/ci-rust-nightly.yml contents are given verbatim in the plan — transcribe them exactly. Umbrella issue: #242. `uv sync --all-extras` in sdks/python/ before any push.

Done when ALL hold:
1. Behavior tests from Step 2 pass: deletion-only stdin → skip; docs-only historical range → "no SDK paths" skip (found via the plan's history-scan snippet — NEVER create commits or reset --hard for this); rust-only range origin/main~2..origin/main → only [rust] runs and passes.
2. Gates green: bash -n; shellcheck scripts/pre-push-gate.sh (zero findings); actionlint .github/workflows/ci-rust-nightly.yml (zero findings); treefmt --fail-on-change exit 0. Tools missing → nix develop, never skip.
3. Commit per plan (bare `fix:` + `(#242)` + trailer); push SHA-verified.
4. PR open with the plan's title. This PR triggers ZERO CI checks by design (path filters) — do NOT wait for any check to appear; PR body includes the four points the plan's Step 4 lists.
5. Report the PR URL. Do NOT merge. (Post-merge nightly dispatch is handled outside this goal.)

Stop-loss: 3 consecutive failures on one step → STOP and report. No --no-verify; no reset --hard.
```

## Goal 6 — Publish workflow guards (PR-F)

```text
GOAL: Execute Task 6 of docs/superpowers/plans/2026-07-28-correctness-quick-wins.md and open its PR.

Setup: git fetch origin; fresh worktree off origin/main; branch chore/publish-workflow-guards. Read the plan's Global Constraints AND Task 6 in full; the publish-python.yml rewrite and the publish-typescript.yml verify-step replacement are given verbatim — transcribe exactly. Umbrella issue: #242. `uv sync --all-extras` in sdks/python/ before any push.

Done when ALL hold:
1. YAML parse check passes ("yaml ok"); actionlint on both changed workflow files → zero findings; treefmt --fail-on-change exit 0. Tools missing → nix develop, never skip.
2. Commit per plan (bare `fix:` + `(#242)` + trailer); push SHA-verified.
3. PR open with the plan's title. ZERO CI checks will appear (path filters; publish workflows are tag/dispatch-only) — do NOT wait; PR body notes the workflows only execute on the next python-v*/ts-v* tag and that review is a diff-eyeball against publish-rust.yml's proven pattern.
4. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step → STOP and report. No --no-verify; no reset --hard.
```

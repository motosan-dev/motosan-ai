# Freeform Parity — Goal-Run Prompt Sheet

Paste-ready goal texts for the two implementation plans of milestone **#270**. One goal session per PR.

| # | PR | Branch | Plan | Tasks | CI checks that will appear |
|---|---|---|---|---|---|
| 1 | **S** | `docs/freeform-spec-widen` | Python | 1 | `metadata` only |
| 2 | **P1** | `feat/freeform-python-types` | Python | 2–9 | `metadata`, `python` |
| 3 | **P2** | `feat/freeform-python-providers` | Python | 10–15 | `metadata`, `python` |
| 4 | **C-PY** | `test/freeform-python-conformance` | Python | 16 | `metadata`, `python` |
| 5 | **T1** | `feat/ts-native-model-types` | TypeScript | 1–4 | `metadata`, `typescript` |
| 6 | **T2** | `feat/ts-native-model-providers` | TypeScript | 5–10 | `metadata`, `typescript` |
| 7 | **C-TS** | `feat/ts-freeform-conformance` | TypeScript | 11 | `metadata`, `typescript` |
| 8 | **C-RS** | `feat/rust-freeform-conformance` | TypeScript | 12 | `metadata`, `rust`, `rust-msrv-no-features` |

Plans: `docs/superpowers/plans/2026-07-28-freeform-parity-{python,typescript}-implementation.md`.
Milestone and locked decisions: `docs/superpowers/plans/2026-07-28-freeform-parity-milestones.md`.

**Ordering.** S merges first — it is the normative anchor both tracks are written against. Then the two
tracks run **in parallel**: P1 → P2 → C-PY and T1 → T2 → C-TS, each strictly sequential within its track
because every PR builds on the merged previous one. **C-RS is independent** of everything (it only adds a
Rust test file for behaviour that already ships) and can run any time after S.

**`ci-metadata` has no path filter**, so every PR gets a `metadata` check — including the spec-only one.
That is why each goal below can require a named check rather than "all checks pass".

**Nobody merges.** Each goal opens its PR and stops. Pre-merge diff verification is done outside the run.

---

## Goal 1 — PR S: widen the spec

```text
GOAL: Execute Task 1 of docs/superpowers/plans/2026-07-28-freeform-parity-python-implementation.md and open its PR.

Setup: git fetch origin; create a fresh worktree off origin/main; branch docs/freeform-spec-widen. Read the plan's Global Constraints and Task 1 in full before acting, and read docs/superpowers/plans/2026-07-28-freeform-parity-milestones.md for the locked decisions D1-D10 — they are settled, do not re-litigate them. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

This task widens specs/types.md ONLY. It must NOT claim Python or TypeScript ship the API — decision D10 splits the spec change in two, and this is step one: the normative contract becomes cross-SDK while an implementation-status line still says the ports are in progress.

Done when ALL hold:
1. Every edit in Task 1 applied exactly as written; the task's own verification script passes.
2. Gates green: `treefmt --fail-on-change` from the repo root, and `python3 scripts/check-versions.py`.
3. Commit message exactly as the plan gives it (bare `feat:`, ends with `(#270)`, Co-Authored-By as a second -m).
4. Push verified by SHA: `test "$(git ls-remote origin refs/heads/docs/freeform-spec-widen | cut -f1)" = "$(git rev-parse HEAD)"`.
5. PR open against main; `gh pr checks` shows `metadata` SUCCESS. No other check will appear — do not wait for one.
6. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on the same step → STOP and report the blocker verbatim. Never edit a test to force it green; never push with --no-verify; never git reset --hard.
```

## Goal 2 — PR P1: Python types, codec, exports

```text
GOAL: Execute Tasks 2-9 of docs/superpowers/plans/2026-07-28-freeform-parity-python-implementation.md and open PR P1.

Setup: git fetch origin; fresh worktree off origin/main (PR S must already be merged); branch feat/freeform-python-types. Read the plan's Global Constraints and Tasks 2-9 in full, plus the milestone doc's locked decisions. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

Follow the tasks in order and honour TDD: each task's Step 1 test is observed FAILING at Step 2 before its Step 3 implementation exists. If a test passes when the plan says it should fail, STOP — the premise moved.

Done when ALL hold:
1. Tasks 2-9 complete, each red-then-green in order.
2. Python gates green from sdks/python/: `uv run ruff check motosan_ai/`, `uv run ruff format --check motosan_ai/ tests/`, `uv run mypy motosan_ai/`, `uv run pytest tests/ -q --ignore=tests/integration/`. mypy is CI-enforced and the package is currently clean — it must stay clean.
3. Repo-wide: `treefmt --fail-on-change`, `python3 scripts/check-versions.py`.
4. Every new public symbol is importable from `motosan_ai` and listed in `__all__`; tests/test_public_exports.py passes.
5. One commit per task with the plan's exact messages (bare `feat:`, `(#270)`, Co-Authored-By second -m); push SHA-verified against refs/heads/feat/freeform-python-types.
6. PR open; `gh pr checks` shows `metadata` and `python` SUCCESS.
7. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on the same step, or any need to change an EXISTING test's assertions beyond the plan's stated expected flips → STOP and report verbatim. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 3 — PR P2: Python providers, capabilities, client

```text
GOAL: Execute Tasks 10-15 of docs/superpowers/plans/2026-07-28-freeform-parity-python-implementation.md and open PR P2.

Setup: git fetch origin; fresh worktree off origin/main (PR P1 must already be merged); branch feat/freeform-python-providers. Read the plan's Global Constraints and Tasks 10-15 in full, plus the milestone doc. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

Three traps this PR must not fall into, all documented in the plan: `BaseProvider` is subclassed by only 4 of 11 providers, so `Client` must duck-type via getattr rather than rely on the ABC; the `openai_responses_api` flag goes only to the `Provider.openai` construction branch, never to `Provider.ollama` which also builds an OpenAIProvider; and Codex's body must convert `reasoning_effort` into `reasoning={effort, summary:"auto"}` AND delete the raw top-level key that the providerOptions merge injects.

Done when ALL hold:
1. Tasks 10-15 complete, each red-then-green in order.
2. Python gates green (ruff check, ruff format --check, mypy, pytest) and repo-wide gates green.
3. `ProviderCapabilities.full()` still reports `supports_freeform_tools is False` — decision D5.
4. Existing capability tests updated only where the plan lists them as expected flips; any other test change → STOP.
5. Commits per the plan; push SHA-verified against refs/heads/feat/freeform-python-providers.
6. PR open; `gh pr checks` shows `metadata` and `python` SUCCESS.
7. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or an unplanned existing-test change → STOP and report verbatim. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 4 — PR C-PY: Python conformance suite

```text
GOAL: Execute Task 16 of docs/superpowers/plans/2026-07-28-freeform-parity-python-implementation.md and open PR C-PY.

Setup: git fetch origin; fresh worktree off origin/main (PR P2 must already be merged); branch test/freeform-python-conformance. Read the plan's Global Constraints and Task 16 in full. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

This suite gates behaviour P1/P2 already shipped, so it passes as soon as it is written. That makes it worthless unless it is proven non-vacuous: follow the task's mutation checks exactly — temporarily break the source in each named way, confirm the suite fails with the expected message, then restore. A conformance suite that has never been seen failing is decoration.

Done when ALL hold:
1. The suite exists as the plan specifies and passes.
2. Every mutation check in the task was performed and each produced the expected failure; the source is restored afterwards (`git status --porcelain` clean apart from the new test file).
3. Python gates and repo-wide gates green.
4. Commit per the plan (bare `feat:`, `(#270)`, Co-Authored-By); push SHA-verified against refs/heads/test/freeform-python-conformance.
5. PR open; `gh pr checks` shows `metadata` and `python` SUCCESS.
6. Report the PR URL, and list which mutations you ran and the failure each produced. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or a mutation that does NOT make the suite fail (that means the suite is vacuous) → STOP and report verbatim. No --no-verify; no reset --hard.
```

## Goal 5 — PR T1: TypeScript types and Responses codec

```text
GOAL: Execute Tasks 1-4 of docs/superpowers/plans/2026-07-28-freeform-parity-typescript-implementation.md and open PR T1.

Setup: git fetch origin; fresh worktree off origin/main (PR S must already be merged); branch feat/ts-native-model-types. Read the plan's Global Constraints and Tasks 1-4 in full, plus docs/superpowers/plans/2026-07-28-freeform-parity-milestones.md for the locked decisions. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push (the pre-push hook needs it even for a TS-only branch).

Honour the TS house rules stated at the top of src/types.ts: discriminated unions on a tag key, optional fields omitted rather than `undefined`, camelCase, and wire serialization only in serialize/*.ts — never in types.ts. Decision D2 requires the `kind` tag on ModelToolCall/ModelToolOutput because the model shape and wire shape disagree, following the McpToolConfig precedent.

Done when ALL hold:
1. Tasks 1-4 complete, each red-then-green in order.
2. Gates green from sdks/typescript/: `npm ci`, `npm run build`, `npm run test` — build before test, the pack-smoke test needs dist/.
3. Repo-wide: `treefmt --fail-on-change`, `python3 scripts/check-versions.py`.
4. Commits per the plan; push SHA-verified against refs/heads/feat/ts-native-model-types.
5. PR open; `gh pr checks` shows `metadata` and `typescript` SUCCESS.
6. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or any unplanned change to an existing test → STOP and report verbatim. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 6 — PR T2: TypeScript providers, client, exports

```text
GOAL: Execute Tasks 5-10 of docs/superpowers/plans/2026-07-28-freeform-parity-typescript-implementation.md and open PR T2.

Setup: git fetch origin; fresh worktree off origin/main (PR T1 must already be merged); branch feat/ts-native-model-providers. Read the plan's Global Constraints and Tasks 5-10 in full, plus the milestone doc. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

The single easiest way to ship a broken port is `asDispatchProvider` in client.ts: it rebuilds a plain object exposing only capabilities/chat/stream, so the new model methods are silently dropped unless that shim forwards them. Task 9 covers it — verify by test, not by reading. Also: `ProviderImpl.modelChat`/`modelStream` must be OPTIONAL, because third parties implement that structural contract, and `withResponsesApi` must stay clearly distinct from the existing 404-recovery `withResponsesFallback`.

Done when ALL hold:
1. Tasks 5-10 complete, each red-then-green in order.
2. TypeScript gates green (npm ci, npm run build, npm run test) and repo-wide gates green.
3. `fullCaps()` still reports `supportsFreeformTools: false` — decision D5.
4. A test proves a model method survives `asDispatchProvider`.
5. Commits per the plan; push SHA-verified against refs/heads/feat/ts-native-model-providers.
6. PR open; `gh pr checks` shows `metadata` and `typescript` SUCCESS.
7. Report the PR URL. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or an unplanned existing-test change beyond the plan's stated flips → STOP and report verbatim. Never edit tests to force green; no --no-verify; no reset --hard.
```

## Goal 7 — PR C-TS: TypeScript conformance suite

```text
GOAL: Execute Task 11 of docs/superpowers/plans/2026-07-28-freeform-parity-typescript-implementation.md and open PR C-TS.

Setup: git fetch origin; fresh worktree off origin/main (PR T2 must already be merged); branch feat/ts-freeform-conformance. Read the plan's Global Constraints and Task 11 in full. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push.

Task 11 carries a deliberate, labelled format deviation: the suite gates already-shipped behaviour, so it cannot fail first. Instead it must be proven non-vacuous by the three source mutations the task names — apply each, confirm the expected failure, restore. Do not skip this; an unfalsifiable gate is worse than none.

Done when ALL hold:
1. The suite exists as specified and passes.
2. All three mutations were performed and each produced its expected failure; source restored afterwards (`git status --porcelain` clean apart from the new test file).
3. TypeScript gates and repo-wide gates green.
4. Commit per the plan; push SHA-verified against refs/heads/feat/ts-freeform-conformance.
5. PR open; `gh pr checks` shows `metadata` and `typescript` SUCCESS.
6. The suite's module docstring records the three mutations and the test each one fails, so the gate can be re-proven after a future refactor. A suite whose non-vacuity proof exists only in a chat log is unmaintainable.
7. Report the PR URL and list each mutation with the failure it produced. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or a mutation that does NOT make the suite fail → STOP and report verbatim. No --no-verify; no reset --hard.
```

## Goal 8 — PR C-RS: Rust conformance suite

```text
GOAL: Execute Task 12 of docs/superpowers/plans/2026-07-28-freeform-parity-typescript-implementation.md and open PR C-RS.

Setup: git fetch origin; fresh worktree off origin/main; branch feat/rust-freeform-conformance. Read the plan's Global Constraints and Task 12 in full. Tracking issue: #270. Run `uv sync --all-extras` in sdks/python/ before any push. This PR is INDEPENDENT of the Python and TypeScript tracks — it only adds a Rust test file for behaviour Rust already ships. It needs no version bump and must not touch any Rust source file.

Done when ALL hold:
1. The Rust conformance test file exists as specified and passes.
2. Every mutation check the task names was performed and produced its expected failure; source restored afterwards (`git status --porcelain` clean apart from the new test file, and `git diff --stat origin/main -- sdks/rust/src` is EMPTY).
3. Rust gates green from sdks/rust/: `cargo fmt --all -- --check`; `cargo clippy --all-features --all-targets -- -D warnings`; and the credential-stripped full suite `env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST cargo test --all-features` — the suite contains env-gated live tests that must not fire.
4. Repo-wide: `treefmt --fail-on-change`, `python3 scripts/check-versions.py` (versions must be untouched).
5. Commit per the plan; push SHA-verified against refs/heads/feat/rust-freeform-conformance.
6. PR open; `gh pr checks` shows `metadata`, `rust`, and `rust-msrv-no-features` SUCCESS.
7. The suite's file-level doc comment records the three mutations and the test each one fails, so the gate can be re-proven after a future refactor.
8. Report the PR URL and list each mutation with its failure. Do NOT merge.

Stop-loss: 3 consecutive failures on one step, or any diff appearing under sdks/rust/src → STOP and report verbatim. No --no-verify; no reset --hard.
```

---

## After all eight land

Release with the scripted flow — it needs no plan:

```bash
python3 scripts/bump-version.py --python 0.20.0 --ts 0.16.0   # --dry-run first
# write the root CHANGELOG entry and the AGENTS.md release paragraph
# PR labelled release:python and release:ts, then merge; release-tag.yml tags and publishes
```

Then relabel `specs/types.md` § Native Model API from the in-progress line to the shipped versions —
decision D10's step two — and close #270.

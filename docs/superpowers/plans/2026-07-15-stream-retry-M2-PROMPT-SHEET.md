# M2 Execution — Copy-Paste Subagent Prompt Sheet (all 7 PR groups)

Plan: `docs/superpowers/plans/2026-07-15-stream-retry-m2-implementation.md` (14 tasks, Codex-reviewed).
Baseline: `origin/main` @ `d7c06ff` (post-M1). One fresh subagent per task; paste the group's **shared preamble** + the **task prompt** together; run the **review prompt** between tasks.

## Dependency graph & parallelization

```
PR-S (Task 1, spec)  ─┐
PR-R1 (Tasks 2→3→4)  ─┤  ← these 4 start in parallel (spec is docs; R1/P/T touch disjoint SDKs)
PR-P  (Tasks 7→8→9)  ─┤
PR-T  (Tasks 10→11→12)┘
        │
PR-R1 merged ──→ PR-R2 (Tasks 5→6)     [R2 consumes R1's on_retry/RetryEvent + 4-arg map_http_error]
        │
(PR-R2 + PR-P + PR-T all merged) ──→ PR-C (Task 13, conformance)
        │
PR-C merged ──→ PR-REL (Task 14, release)   [strictly last]
```

- **Start now in parallel:** PR-S, PR-R1, PR-P, PR-T (four separate worktrees, zero shared files).
- **After PR-R1 merges:** PR-R2.
- **After PR-R2 + PR-P + PR-T merge:** PR-C.
- **Last:** PR-REL.
- Within each group, tasks are strictly sequential (each consumes the previous task's interfaces) — do NOT parallelize inside a group.
- Fresh Python worktrees (PR-P): run `uv sync --all-extras` in `sdks/python/` before the first push (pre-push hook runs the full Python suite).
- TS full suite (PR-T): always `npm run build && npm test` (pack-smoke needs `dist/`).

---

## Shared preamble (prepend to EVERY task prompt in every group)

```
You are implementing ONE task of a written plan. Work in the worktree named in your group's Setup block.
Plan file (inside the worktree): docs/superpowers/plans/2026-07-15-stream-retry-m2-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST (it carries the locked D1–D9 decisions and the
  Cross-SDK consistency rules) — it overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → format/lint → commit. TDD; do not skip the red step.
- The plan was authored against origin/main @ d7c06ff; ALL line refs are approximate. Ground every
  edit in the real files; if code drifted (esp. from an earlier task in your own PR), adapt and say so.
- Honor the locked design: RETRY_AFTER cap 60s; full jitter = uniform [0, exp_delay]; retry-after used
  verbatim (no jitter); retryable statuses 408/409/429/≥500; on_retry lives ON RetryPolicy; CLI backends
  get NO transport retry. A change deviating from these is wrong even if it compiles.
- M1 regression contract: the M1 retry tests (non-JSON 5xx → exactly 2 requests → success; stream retry
  only before the first event) MUST pass unchanged. If one breaks, you changed behavior — stop and report.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report the
  exact problem + failing output — do not improvise a different design.
- Show the actual command output for every test/lint/build step. Never claim success without green output.
- Commit exactly per your task's Step 6/last step (conventional message + Co-Authored-By line).
```

---

## PR-S — Task 1 (specs/retry.md)

**Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-spec origin/main -b docs/m2-retry-spec
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-spec`

**Task prompt:**
```
Execute "### Task 1: Write specs/retry.md — the normative cross-SDK retry contract". This is a
docs-only task (no test cycle — the plan says so; do NOT fabricate tests). Create specs/retry.md
with the full content given in the task verbatim, and add the one pointer line to specs/types.md.
Proofread against the task's own § reference (classification 408/409/429/≥500 + transport table,
Retry-After integer+HTTP-date/60s cap/verbatim-no-jitter, full-jitter formula, streaming
first-event rule, CLI no-retry, on_retry, out-of-scope note). Then commit.
```
**Close-out:** `git push -u origin docs/m2-retry-spec` → `gh pr create --title "docs(specs): normative cross-SDK retry contract (M2)" --body "M2 Task 1 — specs/retry.md. The conformance suites (PR-C) mirror this file. Plan: docs/superpowers/plans/2026-07-15-stream-retry-m2-implementation.md"` (append the `🤖 Generated with…` line).

---

## PR-R1 — Tasks 2 → 3 → 4 (Rust error metadata + Retry-After/status + jitter/on_retry)

**Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-rust-errors origin/main -b feat/m2-rust-error-metadata
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-rust-errors`. Commands from `sdks/rust`.

**Task 2 prompt:**
```
Execute "### Task 2: Rust structured error metadata (D1)". Convert the four HTTP-mapped MotosanError
variants (Auth/RateLimit/InvalidRequest/ProviderError) to struct variants {message, status_code,
retry_after, request_id}; keep Display strings byte-identical; add the three accessors; change
map_http_error to the 4-arg form and add extract_request_id in providers/mod.rs. Sweep EVERY
construction + pattern-match site listed in the task (the plan counted them — verify against the real
tree). Note the red-test snippet needs `use std::time::Duration;`. Step 4 also runs
`cargo test --all-features --test error_mapping` explicitly (the "error" filter misses those names).
This is the FIRST Rust task — everything downstream consumes the 4-arg map_http_error.
```
**Task 3 prompt:**
```
Execute "### Task 3: Harden Rust Retry-After parsing and retryable status set (D8)". Add chrono as an
OPTIONAL direct dep (verify it is absent at baseline; default-features=false, features=["clock"],
wired into the HTTP feature lists), extend parse_retry_after to accept HTTP-date + clamp to
[0, RETRY_AFTER_CAP] (const = 60s), and add 408/409 to is_retryable_status. Tests use fixed dates via
chrono arithmetic, not wall-clock strings.
```
**Task 4 prompt:**
```
Execute "### Task 4: Rust: full-jitter backoff + on_retry hook on RetryPolicy". SCOPE: only
src/retry.rs, src/lib.rs, sdks/rust/Cargo.toml (+ the one `on_retry: None` fix to gemini.rs's
#[cfg(test)] RetryPolicy struct-literal). Full jitter in delay_for_attempt via fastrand (add
`fastrand = "2"`; verify absent at baseline); add RetryEvent/RetryCause and the on_retry field +
builder; manual Debug skipping on_retry; keep Clone (Arc). DO NOT change sleep_before_retry's
signature (stays 3-arg) and do NOT make it fire on_retry — that happens in PR-R2's send_with_retry.
```
**Review prompt (between tasks):**
```
Review the just-completed task against the plan "### Task N" in the worktree
/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-rust-errors. Verify with evidence: (1) red test
shown failing before impl, now green; (2) D1 Display strings byte-identical / D8 60s clamp + 408/409 /
D4 jitter bounds with injected RNG as applicable; (3) Task 4 did NOT touch sleep_before_retry's
signature or providers; (4) cargo fmt no diff + cargo clippy --all-features --all-targets -- -D warnings clean
(paste); (5) cargo test --all-features green incl. the M1 regression tests (paste); (6) commit exists.
Report deviations only; do not fix.
```
**Close-out:** full gate (`cargo fmt && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features`) → push `feat/m2-rust-error-metadata` → `gh pr create --title "feat(rust): structured error metadata + Retry-After/jitter/on_retry (M2, BREAKING)"`. Body: note the MotosanError enum shape change is BREAKING (four HTTP variants → struct variants; Display strings unchanged); ships in 0.23.0 (bump happens in PR-REL). **PR-R2 branches off this after it merges.**

---

## PR-R2 — Tasks 5 → 6 (send_with_retry engine + all six provider migrations)

**Prerequisite:** PR-R1 merged. **Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-rust-engine origin/main -b feat/m2-rust-send-with-retry
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-rust-engine`. Commands from `sdks/rust`.

**Task 5 prompt:**
```
Execute "### Task 5: Extract Rust send_with_retry engine; migrate Anthropic + OpenAI onto it". Add
send_with_retry to providers/mod.rs with the 7-feature #[cfg(any(...))] attribute; it is the SOLE
place on_retry fires (compute delay → fire on_retry(RetryEvent{attempt,delay,cause}) → sleep inline).
Migrate anthropic + openai chat AND stream request phases AND openai chat_via_responses (the Responses
fallback — status-first, was previously raw .send()). Terminal handling stays caller-side: tolerant
body parse + 4-arg map_http_error(status, msg, parse_retry_after(headers), extract_request_id(headers))
+ anthropic auth-hint. Preserve M1 status-first behavior; the M1 tests (anthropic_chat.rs,
openai_retry.rs non-JSON-5xx) MUST pass unchanged. providers/mod.rs imports {RetryCause, RetryEvent,
RetryPolicy}; provider files import only RetryPolicy.
```
**Task 6 prompt:**
```
Execute "### Task 6: Migrate ollama, gemini, gemini_code_assist, chatgpt_codex onto send_with_retry".
Same rules: terminal blocks call the 4-arg map_http_error; provider retry imports simplify to
`use crate::retry::RetryPolicy;`; delete dead local retry loops. After the last migration, DELETE
sleep_before_retry (send_with_retry sleeps inline, so it now has zero callers — confirm with
`grep -rn "sleep_before_retry(" sdks/rust/src`) and confirm `cargo clippy --all-features --all-targets -- -D warnings`
passes (dead code would otherwise fail it). ollama is feature ollama_native — run `cargo test
--all-features`. Existing provider tests pass unchanged.
```
**Review prompt:** as PR-R1's, plus: verify chat_via_responses now routes through send_with_retry (Task 5) and sleep_before_retry is deleted with zero remaining callers (Task 6); on_retry fires only inside send_with_retry.
**Close-out:** full gate incl. `cargo check --no-default-features` (catches missing-cfg regressions `--all-features` hides) → push → `gh pr create --title "feat(rust): one send_with_retry engine; migrate all six HTTP providers (M2)"`.

---

## PR-P — Tasks 7 → 8 → 9 (Python error attrs → RetryPolicy → client threading)

**Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-python origin/main -b feat/m2-python-retry
cd ../motosan-worktrees/m2-python/sdks/python && uv sync --all-extras
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-python`. Commands from `sdks/python`; lint scope `motosan_ai/` only.

**Task 7 prompt:**
```
Execute "### Task 7: Add structured HTTP metadata to Python errors and populate at provider raise
sites". Rewrite the MotosanError base per D2 (additive: message + keyword-only status_code/retry_after/
request_id; subclasses stay one-liners; LlmClient Protocol untouched). Add parse_retry_after_header +
RETRY_AFTER_CAP_SECS=60.0 to retry.py (integer OR decimal seconds → float, e.g. "1.5"→1.5; HTTP-date via
email.utils.parsedate_to_datetime; NEGATIVE numeric → None; only a PAST http-date clamps to 0.0; clamp
valid values to [0,60]). Populate status_code/retry_after/request_id ("request-id" then "x-request-id")
at every provider raise site where the response is in scope. Keep the M1 "HTTP {status}: ..." message
prefixes.
```
**Task 8 prompt:**
```
Execute "### Task 8: Python RetryPolicy: full-jitter backoff and attribute-based retry classification".
PRESERVE parse_retry_after_header + RETRY_AFTER_CAP_SECS from Task 7 verbatim. Add @dataclass RetryPolicy
+ RetryEvent; rewrite _is_retryable to attribute-based (RateLimitError/NetworkError → retry; ProviderError
→ status_code in {408,409} or ≥500); DELETE _STATUS_5XX_RE + the old message-scraping; add compute_delay(
policy, attempt, retry_after=None, rng=random.random) with full jitter and retry_cause(error)->str. Make
with_retry ADDITIVE: keep the legacy positional params (max_retries, initial_backoff, max_backoff) in
order; add policy + rng as KEYWORD-ONLY at the end (after *), building a policy from legacy kwargs when
policy is None. Produces exactly: RetryPolicy, RetryEvent, compute_delay, retry_cause, with_retry.
```
**Task 9 prompt:**
```
Execute "### Task 9: Thread RetryPolicy through Python Client chat and stream paths". Consume exactly
`from motosan_ai.retry import RetryPolicy, RetryEvent, compute_delay, retry_cause, with_retry`. Client
gains retry_policy: RetryPolicy | None = None (None → default from any legacy max_retries so existing
callers are unchanged); chat + stream_with both use the shared policy math; the stream path keeps its
retry-only-before-first-event guard (tests/test_client_stream_with.py stays green).
```
**Review prompt:**
```
Review against the plan "### Task N" in /Users/daiwanwei/Projects/wade/motosan-worktrees/m2-python.
Verify with evidence: (1) red→green; (2) NO public API break — MotosanError additive, LlmClient Protocol
untouched, with_retry old positional callers still work (Task 8); (3) decimal Retry-After 1.5 preserved,
negative→None (Task 7); (4) lint ran as `uv run ruff check motosan_ai/` only + ruff format (paste);
(5) `uv run pytest` green incl. M1 non-JSON-5xx + stream-guard tests (paste); (6) commit exists. Report
only; do not fix.
```
**Close-out:** `uv run ruff check motosan_ai/ && uv run ruff format --check motosan_ai/ tests/ && uv run pytest tests/ -q --ignore=tests/integration` → push → `gh pr create --title "feat(python): structured errors + RetryPolicy + attribute-based classification (M2)"`.

---

## PR-T — Tasks 10 → 11 → 12 (TS requestId → jitter → withRetry routing)

**Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-ts origin/main -b feat/m2-ts-retry
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-ts`. Commands from `sdks/typescript`; full suite = `npm run build && npm test`.

**Task 10 prompt:**
```
Execute "### Task 10: TS: requestId on errors, Retry-After HTTP-date + 60s cap, retry 408/409". Add
requestId?: string to MotosanError; mapHttpError gains a requestId param populated from "request-id"
then "x-request-id"; parseRetryAfter accepts integer AND HTTP-date (Date.parse; the HTTP-date branch
requires ≥1 ASCII letter so "-5" doesn't parse as a valid date; clamp [0, 60000]ms; export
RETRY_AFTER_CAP_MS=60_000); isRetryableStatus adds 408/409. Thread request-id through http/fetch.ts +
providers. Step 4 expects 37 tests in error.test.ts and 13 in http-fetch.test.ts.
```
**Task 11 prompt:**
```
Execute "### Task 11: Replace TS deterministic jitter with full jitter and add onRetry hook". retry.ts:
full jitter in delayForAttempt via injectable random?: () => number (default Math.random); add onRetry?:
(evt: RetryEvent) => void fired inside withRetry before each sleep; retryAfterMs (pre-capped) used
verbatim without jitter. Update the pinning tests in tests/retry.test.ts (the deterministic-LCG describe
block). Step 4 expects retry.test.ts now has 22 tests.
```
**Task 12 prompt:**
```
Execute "### Task 12: Route all TS provider request paths through shared withRetry classification".
Extract ONE classifyForRetry helper (in retry.ts, cycle-free) and delete the 4 classifyHttpError copies;
route every provider chat/stream request through withRetry(policy, op, classifyForRetry), preserving the
retry-only-before-first-event guard. Update tests/providers-openai-responses.test.ts's "does not retry
the single Responses fallback call" test to the NEW retried behavior (chatViaResponses now wrapped:
retryable 5xx→200 retried, non-retryable 404 not). Import edit quotes the post-Task-11 line (RetryEvent
already imported). Full suite: npm run build && npm test.
```
**Review prompt:**
```
Review against the plan "### Task N" in /Users/daiwanwei/Projects/wade/motosan-worktrees/m2-ts. Verify:
(1) red→green; (2) 408/409 + HTTP-date/60s cap + requestId (T10); full jitter bounds with stubbed random
+ onRetry fires (T11); the 4 classifyHttpError copies gone + all provider loops route through withRetry,
first-event guard intact (T12); (3) npm run typecheck clean + touched vitest files green + full
`npm run build && npm test` green (paste); (4) commit exists. Report only; do not fix.
```
**Close-out:** `npm run typecheck && npm run build && npm test` → push → `gh pr create --title "feat(ts): requestId, full jitter, onRetry, one withRetry path (M2)"`.

---

## PR-C — Task 13 (cross-SDK conformance)

**Prerequisite:** PR-R2 + PR-P + PR-T all merged. **Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-conformance origin/main -b test/m2-retry-conformance
cd ../motosan-worktrees/m2-conformance/sdks/python && uv sync --all-extras
```
Worktree: `/Users/daiwanwei/Projects/wade/motosan-worktrees/m2-conformance`.

**Task prompt:**
```
Execute "### Task 13: Add cross-SDK specs/retry.md conformance test suites". Three table-driven suites
mirroring specs/retry.md: Rust as an IN-CRATE #[cfg(test)] mod retry_conformance appended to
providers/mod.rs (use super::{is_retryable_status, parse_retry_after}; use crate::retry::RetryPolicy —
these are pub(crate), unreachable from an integration test), run via `cargo test --all-features
providers::retry_conformance`; Python tests/test_retry_conformance.py using compute_delay(policy, n,
rng=<stub>) and parse_retry_after_header (incl. "-5"→None); TS tests/retry-conformance.test.ts. Each
header-commented "Mirrors specs/retry.md — update BOTH or neither". Expected counts per the task
(Python 24). Since this touches .rs it lands via PR + CI.
```
**Review prompt:** verify each suite matches the real merged public/internal surface (no nonexistent methods), the Rust suite is in-crate (not tests/retry_conformance.rs), all three green (paste), commit exists.
**Close-out:** full gate across all three SDKs → push → `gh pr create --title "test(retry): cross-SDK conformance suites mirroring specs/retry.md (M2)"`.

---

## PR-REL — Task 14 (release, LAST)

**Prerequisite:** PR-C merged (all M2 code in). **Setup:**
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && git fetch origin main
git worktree add ../motosan-worktrees/m2-release origin/main -b chore/m2-release
cd ../motosan-worktrees/m2-release/sdks/python && uv sync --all-extras
```

**Task prompt:**
```
Execute "### Task 14: Release M2 — Rust 0.23.0 / Python 0.16.0 / TypeScript 0.13.0". Follow llms.txt
§ Release exactly. Bump the three manifests; regenerate root uv.lock (uv lock --project sdks/python from
repo root; git add uv.lock) + TS package-lock (npm install --package-lock-only); Cargo.lock is gitignored
(never stage). Write the root + three per-SDK changelog entries — Rust 0.23.0 carries a BREAKING migration
note for the MotosanError struct-variant change (show one before/after match example). Update version
lines in AGENTS.md, llms.txt (header + Install + tag table), skills/motosan-ai/SKILL.md (header + Install),
README.md (Languages + Install), AND — M1 lesson — the per-SDK README install snippets (sdks/rust/README.md
Cargo examples) + skills/motosan-ai/references/rust-api.md Cargo snippet: grep-verify every one for the
0.22.0/0.15.0/0.12.0 strings and bump. Cross-check every changelog bullet against `git log d7c06ff..HEAD`;
delete any unmerged claim. Gate: check-all + (cd sdks/typescript && npm run typecheck && npm run build &&
npm test). Commit chore(release); do NOT tag or publish (maintainer does that).
```
**Review prompt:** verify exactly three version bumps + lockfiles (Cargo.lock NOT staged); every changelog bullet maps to a `git log d7c06ff..HEAD` commit (paste the mapping) with no unmerged claims and the Rust BREAKING note present; all doc version lines + per-SDK README/rust-api.md snippets bumped; full gate green; no source changed; no tags/publish.
**Close-out:** push → `gh pr create --title "chore(release): M2 — Rust 0.23.0 / Python 0.16.0 / TS 0.13.0"`. After merge, maintainer tags `rust-v0.23.0` / `python-v0.16.0` / `ts-v0.13.0` (annotated, on the merged HEAD incl. any post-release doc fixes) and lets CI publish per llms.txt § Release. **M2 done.**

---

## Notes carried from M1 (apply throughout)

- Pushing branches OR tags fires the pre-push hook (full Py+Rust+live suite, ~1–2 min > the 2-min foreground limit) — push in the background.
- The CI `CARGO_REGISTRY_TOKEN` secret can drift stale independently of the local `~/.cargo` token — if a crates.io publish 403s, update the secret from the local token (`gh secret set … via stdin`) and rerun the publish workflow; do not hand-publish.
- crates.io `/api/v1/me` rejects API tokens — never use it to judge token validity.
- Remove each worktree after its PR merges (`git worktree remove …`; `--force` only after confirming no unique untracked files); delete the merged local branch with `git branch -d`.

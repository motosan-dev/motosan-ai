# PR-T2 Execution — Copy-Paste Subagent Prompt Sheet

**Scope:** Tasks 18, 19, 20 of the M1 plan — TS stream hygiene: `reader.cancel()` on early exit, WHATWG-correct SSE terminators + field parsing, replace-semantics usage merge. One branch, one PR.

One fresh subagent per task, **in order 18→19→20** (Task 19 MUST come after Task 18 — both edit `http/sse.ts`; Task 19's expected test counts assume Task 18's test landed). Paste the **shared preamble** + the **task prompt** together. Run the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md`
Runs in parallel with PR-R3 / PR-P2 (zero shared files). #212 merged but touched only providers — no overlap with sse/ndjson/stream.ts.

---

## Setup (run once, before Task 18 — you, not a subagent)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai
git fetch origin main
git worktree add ../motosan-worktrees/ts-sse-hygiene origin/main -b fix/ts-sse-hygiene
```

Worktree path used by every prompt below: `/Users/daiwanwei/Projects/wade/motosan-worktrees/ts-sse-hygiene`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Work in this worktree (branch fix/ts-sse-hygiene):
/Users/daiwanwei/Projects/wade/motosan-worktrees/ts-sse-hygiene
Plan file (inside the worktree): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

Rules:
- Read the plan's "## Global Constraints" section FIRST. It overrides anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written:
  write the failing test → run it (confirm it FAILS with the expected signature) → implement →
  run it (confirm it passes) → typecheck/build → commit. This is TDD; do not skip the red step.
- All commands run from sdks/typescript/. Targeted tests: `npx vitest run tests/<file>`.
  The FULL suite is `npm run build && npm test` — pack-smoke.test.ts requires dist/, so ALWAYS
  build before npm test. The static gate is `npm run typecheck` (no ESLint/Prettier).
- The test directory is tests/ (NOT test/). Relative imports in src/ end in `.js` (NodeNext).
- The plan was authored against origin/main @ 3e3f413; sse.ts/ndjson.ts/stream.ts are untouched
  since, so line refs should be close — but ALWAYS ground edits in the real files.
- Public signatures are unchanged: parseSse / parseNdjson / collectStream keep their exact
  signatures. No public API changes.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report
  the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for every test/typecheck/build step. Never claim success without
  showing green output.
- Commit exactly per your task's Step 6 (conventional message, Co-Authored-By line included).
```

## Task prompts

**Task 18 — reader.cancel() on early exit (sse + ndjson)**
```
Execute "### Task 18: Cancel the underlying reader on early stream exit in TS SSE and NDJSON
parsers" from the plan. Both parsers' finally blocks currently only call reader.releaseLock() —
an abandoned/early-exited consumer leaves the HTTP body stream open, pinning a socket per
abandoned stream (this is the NORMAL consumption pattern for chat streaming). In BOTH
src/http/sse.ts and src/http/ndjson.ts: await reader.cancel() (wrapped so cancel errors are
swallowed) BEFORE releaseLock. Tests: append to tests/http.sse.test.ts and
tests/http.ndjson.test.ts per the plan — a ReadableStream whose cancel() sets a flag; consume one
event, break out of the for-await loop → the flag is true.
```

**Task 19 — WHATWG terminators + single-leading-space field parsing**
```
Execute "### Task 19: WHATWG-correct SSE line terminators and field parsing in the TS SSE parser"
from the plan. Prerequisite: Task 18 is already committed on this branch (same file). Two spec
violations today: (1) event boundaries are detected ONLY as \n\n — a spec-valid CRLF stream
(\r\n\r\n) yields ZERO events (and the adapter then fabricates a clean done: silent total loss);
(2) parseEventText trims whole lines and strips arbitrary whitespace after the field colon — the
spec says remove AT MOST ONE leading space and preserve the rest verbatim. Implement the plan's
normalization (\r\n and bare \r → \n before splitting, with a trailing-\r carry so a \r\n pair
split across chunk boundaries still parses) and the single-leading-space field rule. Expected
counts after this task: the suite for this file is 15 tests (9 baseline + 1 from Task 18 + 5 new;
red step shows 3 failed | 12 passed (15)). Tests per the plan: CRLF byte-equivalence with the \n
version, bare-CR, chunk-boundary \r\n split, and data-payload whitespace preservation.
```

**Task 20 — replace-semantics usage merge in collectStream**
```
Execute "### Task 20: Replace-semantics usage merge in TS collectStream" from the plan. The
`case 'usage':` branch in src/stream.ts sums usage events; Anthropic's message_delta usage is
CUMULATIVE, so output tokens are double-counted (billing-visible). Mirror Python
_stream_collect.py's replace-with-fallback semantics exactly, including the cache token fields.
CRITICAL: the existing test 'sums cache tokens with lazy initialization' in tests/stream.test.ts
ENCODES the bug — REWRITE it per the plan (do not just delete it). New test:
Usage{inputTokens:100,outputTokens:5} then {inputTokens:100,outputTokens:50} → final 100/50
(NOT 200/55); single-usage case unchanged. Full-suite check: npm run build && npm test.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md
"### Task N" in the worktree /Users/daiwanwei/Projects/wade/motosan-worktrees/ts-sse-hygiene.
Verify, with evidence: (1) the failing test was shown RED before the implementation and is now
green; (2) reader.cancel() precedes releaseLock and cancel errors are swallowed, in BOTH parsers
(Task 18); the \r\n chunk-boundary carry works and data payloads survive byte-identically
(Task 19); the merge matches Python's replace-with-fallback incl. cache fields and the old
summing test was REWRITTEN, not deleted (Task 20); (3) parseSse/parseNdjson/collectStream
signatures unchanged; (4) npm run typecheck clean and the touched vitest files green (paste
output — Task 19 total should be 15); (5) no scope creep; (6) the commit exists with the task's
conventional message. Report any deviation. Do not fix — just report, so I decide.
```

## After Task 20 (PR close-out)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-worktrees/ts-sse-hygiene/sdks/typescript
npm run typecheck && npm run build && npm test
cd .. && git push -u origin fix/ts-sse-hygiene
gh pr create \
  --title "fix(ts): cancel abandoned stream readers, CRLF-correct SSE parsing, usage replace-merge" \
  --body "$(cat <<'EOF'
## Summary
- parseSse / parseNdjson now `reader.cancel()` before releasing the lock on early exit — an
  abandoned stream (the normal consumption pattern) no longer pins its HTTP connection open.
- SSE parser accepts all WHATWG line terminators (`\r\n`, `\r`, `\n` — including a `\r\n` pair
  split across chunk boundaries) — a spec-valid CRLF stream previously yielded ZERO events and
  read as a clean empty completion. Field parsing now strips at most one leading space after the
  colon and preserves payloads verbatim, per spec.
- collectStream usage merge switches from summing to Python's replace-with-fallback semantics —
  Anthropic's cumulative `message_delta` usage no longer double-counts output tokens
  (billing-visible fix); the old test that encoded the summing behavior is rewritten.

M1 plan Tasks 18–20 (PR-T2): docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

CI must be green before merge. After this + PR-R3 + PR-P2 all merge, only Task 22 (Release) remains — it gets its own run per the plan (`docs/superpowers/plans/2026-07-14-stream-retry-m1-implementation.md` § Task 22), executed from a fresh worktree off the merged main. Remove the worktree once merged.

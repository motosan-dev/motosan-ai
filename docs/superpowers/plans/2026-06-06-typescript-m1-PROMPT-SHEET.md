# M1 Execution — Copy-Paste Subagent Prompt Sheet

One fresh subagent per task, in order (1→9). Paste the **shared preamble** + the **task prompt** together. Review with the **review prompt** between tasks before moving on. Each task depends only on earlier ones, so do not parallelize.

Plan: `docs/superpowers/plans/2026-06-06-typescript-m1-foundation.md`
Spec (context): `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md`

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Repo: /Users/daiwanwei/Projects/wade/motosan-ai
Plan file: docs/superpowers/plans/2026-06-06-typescript-m1-foundation.md

Rules:
- Read the plan's "## Conventions" and "## Canonical symbol homes" sections AND the "## Cross-task implementation notes" section first. They override anything ambiguous.
- Then read ONLY your assigned task section and execute its steps in order, exactly as written: write the failing test → run it (confirm it fails) → implement → run it (confirm it passes) → `npm run build` → commit. This is TDD; do not skip the red step.
- All commands run from sdks/typescript/. Relative imports MUST end in `.js` (NodeNext). Source in src/, tests in tests/.
- Do NOT re-declare any symbol that the canonical-homes table assigns to an earlier task — import it.
- Do NOT expand scope beyond your task. If a step is blocked or the plan is wrong, STOP and report the exact problem and the failing output — do not improvise a different design.
- Show the actual command output for each run/build step. Do not claim success without showing green output.
```

## Task prompts

**Task 1 — types.ts**
```
Execute "### Task 1: Rewrite types.ts — structured type system" from the plan. This creates the complete src/types.ts (the type contract every later task imports) and rewrites tests/types.test.ts. For the red step, use the plan's corrected `tsc --noEmit ... tests/types.test.ts` command because Vitest does not type-check `import type`. If build exposes old provider stream literals missing the newly required `eventType`, apply only the plan-approved minimal compatibility addition `eventType: 'text'` in those existing provider literals. Done when npm run build and the types test are green and tests/types.test.ts no longer imports MessageFactory.
```

**Task 2 — error.ts**
```
Execute "### Task 2: error.ts extensions + classification utils". Extend the EXISTING src/error.ts (it already has MotosanError/mapHttpError/etc.) with StreamReadTimeoutError, UnsupportedFeatureError, isRetryableStatus, isRetryableNetworkError, parseRetryAfter, extractErrorMessage. Do not touch other files.
```

**Task 3 — message.ts**
```
Execute "### Task 3: message.ts factory (multimodal + cache helpers)". Step 1 only VERIFIES the Task 1 types exist (do not re-declare them). Create src/message.ts with the standalone factory functions (user, assistant, toolResult, userWithImage, userWithPdf*, withCache, …) and tests/message.test.ts.
```

**Task 4 — http/sse.ts + http/ndjson.ts**
```
Execute "### Task 4: SSE + NDJSON streaming parsers". Create src/http/sse.ts (parseSse — [DONE] is ADVISORY, not terminating) and src/http/ndjson.ts (parseNdjson skeleton), plus their tests. parseSse must yield SseEvent { event?, data }.
```

**Task 5 — http/fetch.ts**
```
Execute "### Task 5: http/fetch.ts raw fetch wrapper". Create src/http/fetch.ts exporting FetchOptions, postJson, postStream. It IMPORTS extractErrorMessage + mapHttpError from ../error.js — it must NOT redefine them. Add tests/http-fetch.test.ts.
```

**Task 6 — stream.ts**
```
Execute "### Task 6: stream.ts: BoxStream + StreamEvent constructors + collectStream". Step 1 only VERIFIES Task 1 types exist (do not re-declare). Create src/stream.ts with BoxStream, the event constructor helpers, and collectStream (three-way thinking logic: prefer non-empty thinkingDone; empty → undefined; delta buffer only if thinkingDone never fired). Add tests/stream.test.ts.
```

**Task 7 — serialize/anthropic.ts**
```
Execute "### Task 7: serialize/anthropic.ts request serializer". Create src/serialize/anthropic.ts exporting serializeAnthropicRequest. cache_control goes on the LAST content block / LAST tool; systemCache=true forces system-as-array even for non-OAuth (cf39e8c invariant). Add the serializer test.
```

**Task 8 — providers/anthropic.ts**
```
Execute "### Task 8: providers/anthropic.ts rewrite (self-implemented chat + stream)". Rewrite src/providers/anthropic.ts to import postJson/postStream (../http/fetch.js), parseSse (../http/sse.js), serializeAnthropicRequest (../serialize/anthropic.js), and the stream.ts constructor helpers — NO @anthropic-ai/sdk, NO local parseSse/streamEvent. The streaming test uses a HAND-AUTHORED inline SSE transcript string. Live test is env-gated with it.skipIf(!process.env.ANTHROPIC_API_KEY). If build reports the BoxStream/ProviderLike stream seam, apply only the plan-approved minimal src/client.ts type widening to AsyncIterable<StreamEvent>; do not change routing.
```

**Task 9 — wire-up + drop @anthropic-ai/sdk**
```
Execute "### Task 9: wire up index.ts / client.ts / package.json; drop @anthropic-ai/sdk". MINIMAL: index.ts public re-exports (NOT http/* or serialize/*); route client.ts provider:'anthropic' to the new self-hosted AnthropicProvider; remove @anthropic-ai/sdk from package.json (peer/peerMeta/dev), KEEP openai. Done when `grep -rn '@anthropic-ai/sdk' src` is empty and npm run build + npm run test are green.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-06-06-typescript-m1-foundation.md "### Task N".
Verify, with evidence: (1) every step's test was written before its implementation and now passes; (2) npm run build is green (paste output); (3) no symbol was re-declared outside its canonical home; (4) no scope creep beyond the task; (5) the commit exists with a conventional message. Report any deviation. Do not fix — just report, so I decide.
```

## After Task 9 (milestone close)

Run the plan's "## Milestone Done Criteria" checklist. When all green, this is v0.4.0 — open the PR (code lands via PR + CI, not direct-to-main). M2 (OpenAI + serialization split) is the next milestone in the spec.

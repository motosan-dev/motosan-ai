# M2 Execution — Copy-Paste Subagent Prompt Sheet

One fresh subagent per task, in order (1→6). Paste the **shared preamble** + the **task prompt** together. Use the **review prompt** between tasks. Tasks 3 and 4 both depend on Task 2.

Plan: `docs/superpowers/plans/2026-06-06-typescript-m2-openai-serialization.md`
Spec (context): `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M2)
Depends on: M1 (PR #185). Branch M2 off `feat/typescript-m1-foundation` (or `main` once M1 merges).

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan. Repo: /Users/daiwanwei/Projects/wade/motosan-ai
Plan file: docs/superpowers/plans/2026-06-06-typescript-m2-openai-serialization.md

Rules:
- Read the plan's "## Conventions", "## Built on M1", and "## Canonical symbol homes" sections first. They override anything ambiguous. Note the "## Deferred to M3" list — do NOT build Responses-API fallback or auth-style matrix in M2.
- Then read ONLY your assigned task section and execute its steps in order, TDD: failing test → run (fails) → implement → run (passes) → `npm run build` → commit. Do not skip the red step.
- All commands run from sdks/typescript/. Relative imports MUST end in `.js`. Source in src/, tests in tests/.
- This builds on M1 (already implemented). Import M1's existing symbols (types.ts, error.ts, http/fetch.ts, http/sse.ts, stream.ts, serialize/anthropic.ts) — never re-declare them.
- The Anthropic-vs-OpenAI serialization divergence is the #1 bug source: OpenAI = flat function tools + stringified tool_calls.arguments + role:system message; Anthropic = nested input_schema + tool_use blocks + top-level system. Do not mix them up.
- Do NOT expand scope. If blocked or the plan is wrong, STOP and report the exact problem + failing output — do not improvise.
- Show actual command output for run/build steps. Do not claim success without green output.
```

## Task prompts

**Task 1 — Anthropic tool_choice**
```
Execute "### Task 1: extend serialize/anthropic.ts with tool_choice". Add tool_choice to the EXISTING serializeAnthropicRequest: auto→{type:'auto'}, required→{type:'any'}, none→remove the tools array entirely, tool→{type:'tool',name}. Add tests to tests/serialize.anthropic.test.ts.
```

**Task 2 — serialize/openai.ts**
```
Execute "### Task 2: serialize/openai.ts request serializer". Create src/serialize/openai.ts exporting serializeOpenAiRequest(req, model). Prove the divergence vs Anthropic: system-as-message, flat function tools, stringified tool_calls.arguments, image_url content, role:tool, OpenAI tool_choice strings ({type:function,function:{name}} for named), stop. Test file tests/serialize.openai.test.ts. This is the canonical serializer that Tasks 3 and 4 import.
```

**Task 3 — providers/openai.ts rewrite**
```
Execute "### Task 3: providers/openai.ts rewrite (self-implemented chat + stream)". Remove the 'openai' npm import. Use postJson/postStream (../http/fetch.js), parseSse (../http/sse.js), serializeOpenAiRequest (../serialize/openai.js), and the stream.ts constructors. Bearer auth, baseUrl default https://api.openai.com/v1 (trailing-slash trimmed), optional baseUrl constructor param. Stream: buffer indexed tool_calls and emit a sequence collectStream reassembles. Tests in tests/providers-openai.test.ts incl a hand-authored SSE transcript and an env-gated live test (it.skipIf(!process.env.OPENAI_API_KEY)).
```

**Task 4 — minimax interim rewire**
```
Execute "### Task 4: Rewire providers/minimax.ts onto serialize/openai.ts (interim)". Replace OpenAIProvider.serializeMessages() with serializeOpenAiRequest(request, resolvedModel) — which returns the COMPLETE body; do NOT re-add max_tokens/temperature/tools/providerOptions (would double-wrap tools). Keep MiniMax's own endpoint + Bearer auth. stream() adds body.stream=true. Keep response/SSE parsing unchanged. Mocked-fetch test asserts the body still serializes correctly.
```

**Task 5 — collectStream OpenAI test**
```
Execute "### Task 5: collectStream OpenAI-style accumulation test". Test-only (stream.ts is READ ONLY — confirm no fix needed). Add a test to tests/stream.test.ts feeding a synthetic OpenAI-style sequence with TWO sequential tool calls (start/args/end for A, then B) + doneWithStopReason, asserting collectStream returns both tool calls with JSON-parsed inputs, summed usage, and stopReason.
```

**Task 6 — drop openai + wire-up**
```
Execute "### Task 6: Drop the openai npm package; finalize wire-up". Verify no src imports 'openai' or '@anthropic-ai/sdk'; remove openai from package.json (peer/peerMeta/dev → all empty peer deps); npm install; add the two client routing tests (provider:'openai' and 'minimax' reach their endpoints via raw fetch with Bearer). Done when both greps are empty, peerDependencies is {}, and npm run build + npm run test are green.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-06-06-typescript-m2-openai-serialization.md "### Task N".
Verify with evidence: (1) test written before implementation and now passes; (2) npm run build green (paste output); (3) no symbol re-declared outside its canonical home (esp. serializeOpenAiRequest lives only in serialize/openai.ts); (4) no scope creep (no Responses-API fallback / auth-style matrix — those are M3); (5) commit exists with a conventional message. Report deviations; do not fix.
```

## After Task 6 (milestone close)

Run the plan's "## Milestone Done Criteria". When green, this is v0.5.0 — open a PR (CI runs the full gate; from a worktree use `git push --no-verify` after manual verification, per the M1 note). Next: M3 (ClientBuilder + routing + RetryPolicy + ProviderCapabilities + think_stripper + models registry), which is also where the deferred Responses-API fallback + auth-style matrix land.
```

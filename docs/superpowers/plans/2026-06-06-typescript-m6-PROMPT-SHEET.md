# M6 Execution — Copy-Paste Subagent Prompt Sheet

One fresh subagent per task, **in order 1→6** (T3 imports T1+T2; T5 imports T3 + the T4 union token). Paste the **shared preamble** + the **task prompt** together. Use the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-06-06-typescript-m6-gemini.md`
Spec (context): `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M6)
Depends on: M1–M5 merged (M4 added `supportsMcp`; M5 added `'ollama'` to the Provider union + `ToolCall.input` is now `unknown`). Work in a worktree off `main`.

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan, working in a git worktree.

Working dir: /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/typescript-m6-gemini
(branch off main; M1–M5 already merged in.)
All shell commands run from: <working dir>/sdks/typescript
Plan file: docs/superpowers/plans/2026-06-06-typescript-m6-gemini.md (relative to the working dir)

Rules:
- Read the plan's "## Conventions", "## Built on M1–M5", and "## Canonical homes & cross-task contract" sections first (incl. the **Binding rules**). They override anything ambiguous.
- Then read ONLY your assigned "### Task N" and execute its steps in order, TDD: write the failing test → run it (confirm it FAILS) → implement → run it (confirm it PASSES) → npm run build → commit. Do not skip the red step.
- Relative imports MUST end in .js (NodeNext). Source in src/, tests in tests/ (run by vitest, NOT tsc-checked). There is NO `npm run format` script (gate = `npm run build` + `npm run test`).
- This builds on merged M1–M5. Import existing symbols (provider.ts, client.ts, serialize/openai.ts, http/*, stream.ts, error.ts, retry.ts, models.ts) — never re-declare them. A symbol that looks "missing" from a file is usually the pre-M6 state your plan adds — trust the plan; the code is here verbatim. NOTE: ToolCall.input is `unknown` on main (M5 widened it).
- BINDING RULES you must honor (all from Rust gemini.rs; do NOT use gemini_code_assist/gemini_cli):
  * Endpoints/auth: base https://generativelanguage.googleapis.com/v1beta; model in the URL PATH (/models/{model}:generateContent and :streamGenerateContent?alt=sse); auth header `x-goog-api-key` (NOT Bearer, NOT query param).
  * Request: contents[] role mapping user→user, assistant→model, tool→a {role:user, parts:[{functionResponse:{name,response}}]} message; system NOT in contents → top-level `systemInstruction`. parts: {text}; base64 image→{inlineData:{mimeType,data}}; url image→{fileData:{fileUri}}; document→throw. functionCall part wire key is `args` (from tc.input). generationConfig always {maxOutputTokens: maxTokens??8192} + temperature?/stopSequences?. tools[].functionDeclarations; tool_choice→toolConfig.functionCallingConfig.mode (auto→AUTO, required→ANY, none→remove tools+no toolConfig, tool→ANY+allowedFunctionNames). providerOptions merged last.
  * Response (chat): candidates[0].content.parts → text concat + functionCall→toolCalls with CLIENT-GENERATED call_N ids; finishReason STOP→end_turn (NOT 'stop'!), MAX_TOKENS→max_tokens, tool→tool_use, else other; usageMetadata promptTokenCount→inputTokens / candidatesTokenCount→outputTokens; model = payload.modelVersion ?? resolvedModel.
  * SSE: each data line is a FULL GenerateContentResponse JSON chunk; NO [DONE] sentinel — terminate on stream end / finishReason; emit textEvent + synthesized toolCallStart/Args/End (client-generated call_N) + usageEvent + done with mapped finishReason.
  * capabilities() = withImage + supportsMcp:false (image yes, document no). Gemini REQUIRES an API key — it IS in HTTP_PROVIDERS (unlike keyless ollama); ENV_KEY_BY_PROVIDER.gemini='GEMINI_API_KEY'.
  * Retry mirrors OpenAI — withRetryPolicy setter, status-aware classifyHttpError, chat retries whole call, stream retries initial fetch only.
- Do NOT expand scope. If blocked or the plan looks wrong, STOP and report the exact problem + failing output — do not improvise.
- Show actual command output for every run/build step. Do not claim success without green output.
```

## Task prompts

**Task 1 — models.ts**
```
Execute "### Task 1: Add Gemini model constants to models.ts". Add DEFAULT_GEMINI_MODEL = 'gemini-2.5-flash' and GEMINI_MODELS (8 ids per the plan, from Rust models.rs). Extend tests/models.test.ts.
```

**Task 2 — serialize/gemini.ts**
```
Execute "### Task 2: serialize/gemini.ts — Gemini request serializer". Create serializeGeminiRequest(req, _model) (the _model param is intentionally unused — Gemini puts the model in the URL path). Build contents[] (role user→user/assistant→model/tool→functionResponse), systemInstruction (separate top-level field), generationConfig (maxOutputTokens always), tools.functionDeclarations + toolConfig.functionCallingConfig tool_choice, inlineData/fileData images, document→throw, providerOptions merged last. Test file tests/serialize.gemini.test.ts (prove role mapping, systemInstruction separate, images, functionResponse, tool_choice table).
```

**Task 3 — providers/gemini.ts**
```
Execute "### Task 3: providers/gemini.ts — GeminiProvider (chat + SSE stream)". generateContent chat() + streamGenerateContent?alt=sse stream() via parseSse; x-goog-api-key auth; base default generativelanguage.googleapis.com/v1beta, model in URL path. Response parsing: candidates parts → content + functionCall toolCalls with CLIENT-GENERATED call_N ids; finishReason STOP→end_turn (NOT stop); usageMetadata. SSE: full-JSON-per-chunk, NO [DONE], terminate on finishReason; synthesized tool-call events. capabilities() withImage + supportsMcp:false. withRetryPolicy + classifyHttpError (chat whole / stream initial-fetch-only). Test file tests/providers-gemini.test.ts (mocked-fetch chat request-body + response, SSE stream w/ text+functionCall+finish, image inlineData, document rejected, env-gated live on GEMINI_API_KEY).
```

**Task 4 — provider.ts**
```
Execute "### Task 4: provider.ts — add 'gemini' to the Provider union". Only add the 'gemini' token to the Provider union.
```

**Task 5 — client.ts**
```
Execute "### Task 5: client.ts — ClientBuilder + buildProvider Gemini wiring". Add geminiBaseUrl setter + _geminiBaseUrl field; buildProvider gemini arm (new GeminiProvider(apiKey, this._model, this._geminiBaseUrl).withRetryPolicy(...)); add 'gemini' to HTTP_PROVIDERS (do NOT add 'ollama' — main excludes it); ENV_KEY_BY_PROVIDER.gemini='GEMINI_API_KEY'; legacy ctor: INSERT a gemini arm before the minimax fall-through, KEEP the existing ollama arm, use `resolvedApiKey` (not apiKey) and `opts.minimaxBaseUrl` (not minimaxEndpoint). Extend tests/client-builder.test.ts.
```

**Task 6 — index.ts + smoke**
```
Execute "### Task 6: index.ts exports + Done-criteria smoke test". Export GeminiProvider + DEFAULT_GEMINI_MODEL (+ GEMINI_MODELS) from index.ts (no internal http/serialize leak). Smoke test: a ChatRequest with mcpServers + thinking... (per plan) — Client.builder().provider('gemini').apiKey(...).build() round-trips; a document block rejected; an MCP request rejected. Done when npm run build + npm run test green.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-06-06-typescript-m6-gemini.md "### Task N".
Verify with evidence: (1) test written before implementation and now passes; (2) npm run build green (paste output); (3) no symbol re-declared outside its canonical home; (4) for Task 2/3: Gemini wire matches the binding rules (role assistant→model, systemInstruction separate, finishReason STOP→end_turn, SSE no-[DONE], client-generated call_N ids, x-goog-api-key); for Task 5: HTTP_PROVIDERS adds ONLY gemini (ollama stays excluded), legacy ctor keeps ollama arm + uses resolvedApiKey/minimaxBaseUrl; (5) no scope creep; (6) commit exists with a conventional message. Report deviations; do not fix.
```

## After Task 6 (milestone close)

Run the plan's "## Milestone Done Criteria". When green, this is v0.9.0 — open a PR (from the worktree use `git push --no-verify` after verifying `npm run build` + `npm run test` locally; CI runs the full gate). All HTTP providers (anthropic/openai/minimax/ollama/gemini) are then complete. Next: M7 (hardening → 1.0: full test matrix, edge cases, README/CHANGELOG, ESM packaging, npm publish).

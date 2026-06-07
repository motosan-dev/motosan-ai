# M5 Execution — Copy-Paste Subagent Prompt Sheet

One fresh subagent per task, **in order 1→6** (T3 imports T1+T2; T5 imports T3 + the T4 union token). Paste the **shared preamble** + the **task prompt** together. Use the **review prompt** between tasks.

Plan: `docs/superpowers/plans/2026-06-06-typescript-m5-ollama.md`
Spec (context): `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M5)
Depends on: M1–M4 merged (M4 added `supportsMcp` to `ProviderCapabilities`). Work in a worktree off `main`.

---

## Shared preamble (prepend to EVERY task prompt)

```
You are implementing ONE task of a written plan, working in a git worktree.

Working dir: /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/typescript-m5-ollama
(branch off main; M1–M4 already merged in.)
All shell commands run from: <working dir>/sdks/typescript
Plan file: docs/superpowers/plans/2026-06-06-typescript-m5-ollama.md (relative to the working dir)

Rules:
- Read the plan's "## Conventions", "## Built on M1–M4", and "## Canonical homes & cross-task contract" sections first (incl. the **Binding rules**). They override anything ambiguous.
- Then read ONLY your assigned "### Task N" and execute its steps in order, TDD: write the failing test → run it (confirm it FAILS) → implement → run it (confirm it PASSES) → npm run build → commit. Do not skip the red step.
- Relative imports MUST end in .js (NodeNext). Source in src/, tests in tests/ (run by vitest, NOT tsc-checked). There is NO `npm run format` script (gate = `npm run build` + `npm run test`).
- This builds on merged M1–M4. Import existing symbols (provider.ts, client.ts, serialize/openai.ts, http/*, stream.ts, error.ts, retry.ts, models.ts) — never re-declare them. A symbol that looks "missing" from a file is usually the pre-M5 state your plan adds — trust the plan; the code is here verbatim.
- BINDING RULES you must honor:
  * Native /api/chat wire: flat {role,content} messages (NOT content blocks); `think` coercion (true/yes/on/1→true, false/no/off/0→false, else verbatim trimmed string, omit when blank); `keep_alive` verbatim; `num_ctx` INSIDE options; assistant tool_calls use function.arguments as the PARSED OBJECT (not a JSON string), no id/type; tool messages carry no tool_call_id. NDJSON terminates on done:true (adapter emits a plain doneEvent()). Tool calls get generated call_N ids when absent.
  * Auto-routing (T5 buildProvider): native when ollamaNative is true OR any of ollamaThink/ollamaKeepAlive/ollamaNumCtx is set; else OpenAI-compat. SAME decision for chat and stream (provider built once with a native flag).
  * Ollama capabilities() includes supportsMcp:false (M4 made it a required field).
  * Ollama needs NO API key — keep it OUT of HTTP_PROVIDERS; build() must not require a key for it.
  * Validation (T5): a tuning field (ollamaThink/ollamaKeepAlive/ollamaNumCtx) on a non-ollama provider throws ConfigError at build() naming the field.
  * Retry: mirror OpenAI — withRetryPolicy setter, status-aware classifyHttpError, chat retries whole call, stream retries initial fetch only.
- Do NOT expand scope. If blocked or the plan looks wrong, STOP and report the exact problem + failing output — do not improvise.
- Show actual command output for every run/build step. Do not claim success without green output.
```

## Task prompts

**Task 1 — models.ts**
```
Execute "### Task 1: Add DEFAULT_OLLAMA_MODEL to models.ts". Add DEFAULT_OLLAMA_MODEL = 'llama3.2' (no OLLAMA_MODELS array — Rust has none). Extend tests/models.test.ts.
```

**Task 2 — ndjson.ts**
```
Execute "### Task 2: Verify + harden http/ndjson.ts for the Ollama adapter". The M1 parseNdjson is likely already sufficient — this is a verification + targeted hardening + tests task, NOT a rewrite. Confirm it handles the final no-trailing-newline line and done:true objects. Extend tests/http.ndjson.test.ts.
```

**Task 3 — providers/ollama.ts**
```
Execute "### Task 3: providers/ollama.ts — native /api/chat NDJSON provider". Create OllamaProvider with BOTH the native /api/chat NDJSON path (think/keep_alive/num_ctx, thinking extraction, 3-event tool-call streaming with generated call_N ids, done:true terminator → plain doneEvent) AND the OpenAI-compat path (serializeOpenAiRequest + parseSse). capabilities() includes supportsMcp:false. withRetryPolicy setter + classifyHttpError + status-aware retry (chat whole / stream initial-fetch-only). Default model from Task 1. Test file tests/providers-ollama.test.ts (mocked-fetch native + compat, NDJSON tool-call sequence, think coercion, env-gated live on OLLAMA_BASE_URL / local instance).
```

**Task 4 — provider.ts**
```
Execute "### Task 4: provider.ts — add 'ollama' to the Provider union". Only add the 'ollama' token to the Provider union (and capabilities wiring if the plan specifies). Do NOT add the dispatch routing — that lives in T5's buildProvider.
```

**Task 5 — client.ts**
```
Execute "### Task 5: ClientBuilder ollama setters + routing + validation". Add ClientBuilder setters ollamaNative/ollamaThink/ollamaKeepAlive/ollamaNumCtx/ollamaBaseUrl; buildProvider ollama arm constructs OllamaProvider ONCE with the native flag (auto-routing: native if ollamaNative OR any tuning field set, else compat) + tuning + base + .withRetryPolicy(); keep ollama OUT of HTTP_PROVIDERS (no api key required in build()); validation throws ConfigError when a tuning field is set on a non-ollama provider. Extend tests/client-builder.test.ts.
```

**Task 6 — index.ts + smoke**
```
Execute "### Task 6: index.ts exports + done-criteria smoke test". Export OllamaProvider + DEFAULT_OLLAMA_MODEL (no internal http/serialize leak). Smoke test: Client.builder().provider('ollama')...build() round-trips both native and compat routing. Done when npm run build + npm run test green.
```

## Review prompt (run between tasks)

```
Review the just-completed task against docs/superpowers/plans/2026-06-06-typescript-m5-ollama.md "### Task N".
Verify with evidence: (1) test written before implementation and now passes; (2) npm run build green (paste output); (3) no symbol re-declared outside its canonical home; (4) for Task 3: native wire matches the binding rules (think coercion, parsed-object tool args, done:true→plain doneEvent, call_N ids), capabilities includes supportsMcp:false; for Task 5: auto-routing decision correct + same for chat/stream, ollama not in HTTP_PROVIDERS (no key required), tuning-on-non-ollama throws ConfigError; (5) no scope creep; (6) commit exists with a conventional message. Report deviations; do not fix.
```

## After Task 6 (milestone close)

Run the plan's "## Milestone Done Criteria". When green, this is v0.8.0 — open a PR (from the worktree use `git push --no-verify` after verifying `npm run build` + `npm run test` locally; CI runs the full gate). Next: M6 (Gemini — plan written; review before executing).

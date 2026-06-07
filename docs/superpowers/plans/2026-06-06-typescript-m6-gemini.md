# Milestone 6 — Gemini Provider (generativelanguage REST) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the public Gemini provider with full request/response serialization (`contents`/`parts`, `systemInstruction`, `functionDeclarations`, `toolConfig`), `generateContent` + `streamGenerateContent?alt=sse` with synthesized tool-call events, retry, and image content-block support — shipping v0.9.0.

**Architecture:** Builds on merged M1–M5 (on `main`). A net-new `serialize/gemini.ts` + `providers/gemini.ts` implement the Gemini wire; `models.ts` gains `DEFAULT_GEMINI_MODEL`/`GEMINI_MODELS`; the `Provider` union gains `'gemini'`; `ClientBuilder` gains Gemini support + `GEMINI_API_KEY`. Reuses M1's `parseSse` — which treats `[DONE]` as advisory, exactly right for Gemini's finishReason-terminated SSE (no `[DONE]` sentinel).

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, raw `fetch`. Reference: Rust `sdks/rust/src/providers/gemini.rs` (NOT gemini_code_assist / gemini_cli — those are OAuth/deferred).

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M6). **Depends on:** M1–M3 + **M4** (adds `supportsMcp` to `ProviderCapabilities`) + **M5** (adds `'ollama'` to the `Provider` union + the `HTTP_PROVIDERS`/`ENV_KEY_BY_PROVIDER` extension pattern). **Branch M6 off `main` AFTER M4 + M5 merge.** (M5 and M6 are independent siblings off the M4 base; if both are in flight, expect to rebase the `Provider` union + `ClientBuilder` + `index.ts` against whichever merges first.)

---

## Conventions (apply to EVERY task — override anything ambiguous in a task body)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. Package: `sdks/typescript/`. **Commands run from `sdks/typescript/`**. Paths repo-relative.
- **Workflow:** feature branch, land via **PR + CI**. Commit after each task. (From a git worktree the pre-push hook can't run Rust — verify `npm run build` + `npm run test` locally and `git push --no-verify`; CI runs the full gate.)
- **Module system:** strict + NodeNext. Every relative import ends in `.js`.
- **Layout:** source in `src/`, tests in `tests/` (NOT tsc-checked — run by vitest). Mock fetch via `vi.stubGlobal`. Live tests env-gated (`GEMINI_API_KEY`). **No `npm run format` script** (gate = `npm run build` + `npm run test`).
- **Trust the plan:** a symbol that looks "missing" from a file is usually the pre-M6 state this plan (or a dependency milestone M4/M5) adds — the code is here verbatim.

## Built on M1–M5 (import, never redefine)

`http/sse.ts` (`parseSse` — `[DONE]` advisory, does NOT terminate; Gemini terminates on stream-end/finishReason); `http/fetch.ts` (postJson, postStream — inject content-type); `stream.ts` (BoxStream + ctors + collectStream); `serialize/openai.ts` (serializer structure to mirror); `provider.ts` (Provider union `anthropic|openai|minimax|ollama` after M5, ProviderImpl, dispatchChat/dispatchStream, readTimeoutStream, ProviderCapabilities **{supportsImage, supportsDocument, supportsMcp}** after M4, textOnly/withImage/fullCaps, validateRequest); `providers/openai.ts` (classifyHttpError + status-aware retry + withRetryPolicy setter — **mirror these**); `client.ts` (ClientBuilder + buildProvider + HTTP_PROVIDERS + ENV_KEY_BY_PROVIDER); `models.ts` (DEFAULT_*_MODEL); `error.ts` (mapHttpError WITH .status, isRetryableStatus/Network, UnsupportedFeatureError); `retry.ts` (RetryPolicy, withRetry).

## Canonical homes & cross-task contract

| Symbol(s) | Home | Owner |
|---|---|---|
| `DEFAULT_GEMINI_MODEL` (= `'gemini-2.5-flash'`), `GEMINI_MODELS` | `src/models.ts` | **T1** |
| `serializeGeminiRequest(req, model)` | `src/serialize/gemini.ts` | **T2** |
| `GeminiProvider` (generateContent chat + streamGenerateContent SSE, capabilities incl `supportsMcp:false`, `withRetryPolicy`, `classifyHttpError`, x-goog-api-key, geminiBaseUrl) | `src/providers/gemini.ts` | **T3** |
| `'gemini'` added to the `Provider` union | `src/provider.ts` | **T4** |
| ClientBuilder gemini support + `geminiBaseUrl` + `gemini` in HTTP_PROVIDERS + `ENV_KEY_BY_PROVIDER.gemini='GEMINI_API_KEY'` + buildProvider gemini arm | `src/client.ts` | **T5** |
| `GeminiProvider`/`DEFAULT_GEMINI_MODEL` exports + smoke | `src/index.ts` | **T6** |

**Binding rules (wire — all from Rust `gemini.rs`):**
- **Endpoints/auth:** base `https://generativelanguage.googleapis.com/v1beta`; model in the URL PATH — chat `…/models/{model}:generateContent`, stream `…/models/{model}:streamGenerateContent?alt=sse`; auth header `x-goog-api-key` (NOT Bearer, NOT query param).
- **Request (serialize/gemini.ts):** `contents[]` with role mapping **user→user, assistant→model, tool→a user message with a `functionResponse` part** (system is NOT in contents — it goes to `systemInstruction`). parts: `{text}`, `{inlineData:{mimeType,data}}` (base64 image), `{fileData:{fileUri}}` (url image), `{functionCall:{name,args}}` (assistant tool call — wire key is `args`), `{functionResponse:{name,response}}` (tool result; `name`=`toolCallId`, `response`=JSON.parse(content) or `{result:content}`). **document blocks throw** (rejected by capability validation first). `systemInstruction` is a separate top-level field. `generationConfig` always has `maxOutputTokens` (= maxTokens ?? 8192), plus temperature/stopSequences when set. `tools:[{functionDeclarations:[{name,description,parameters}]}]` when tools present; tool_choice → `toolConfig.functionCallingConfig.mode` (AUTO/ANY/NONE + allowedFunctionNames for named); `none` REMOVES tools + sets no toolConfig. `providerOptions` merged last.
- **Response (chat):** `candidates[0].content.parts` → text concat + `functionCall`→toolCalls with **client-generated `call_N` ids** (Gemini omits ids); finishReason → stopReason (**`STOP`→`end_turn`** not `stop`, `MAX_TOKENS`→`max_tokens`, tool→`tool_use`, else `other`); usage from `usageMetadata.promptTokenCount`/`candidatesTokenCount`; model from `payload.modelVersion ?? resolvedModel`.
- **SSE stream:** each `data:` line is a FULL `GenerateContentResponse` JSON chunk; **no `[DONE]`** — terminate on stream end / finishReason. Emit textEvent for text parts, synthesized toolCallStart/Args/End (client-generated `call_N`) for functionCall parts, usageEvent from usageMetadata, then done with mapped finishReason.
- **Capabilities:** `withImage` (image yes, document no) + `supportsMcp:false`. Gemini requires an API key (in HTTP_PROVIDERS).
- **Retry:** mirror OpenAI — `withRetryPolicy` setter, status-aware `classifyHttpError`, chat retries whole call, stream retries initial fetch only.

**Dependency order:** 1 models → 2 serialize → 3 provider → 4 Provider union → 5 builder → 6 exports+smoke. (T3 imports T1+T2; T5 imports T3 + the T4 union token; T6 exports T3.)

---

### Task 1: Add Gemini model constants to models.ts

Adds `DEFAULT_GEMINI_MODEL` and the `GEMINI_MODELS` tuple to `models.ts`, grounded in the contract §1 and Rust `models.rs:22, 24-33`. This is the foundation Task 3 (provider default model) and Task 6 (index export) depend on. STRICT OWNERSHIP: this task touches ONLY `src/models.ts` and extends `tests/models.test.ts`. Do NOT add `DEFAULT_GEMINI_CODE_ASSIST_MODEL`/`GEMINI_CODE_ASSIST_BASE_URL` (deferred/OAuth, `models.rs:35-36`).

**Files:**
- `sdks/typescript/src/models.ts` (MODIFY — append Gemini constants)
- `sdks/typescript/tests/models.test.ts` (MODIFY — add a Gemini describe block)

All commands run FROM `sdks/typescript/`.

- [ ] **Step 1: Write the failing test first (TDD).**
  Append a new `describe` block to `sdks/typescript/tests/models.test.ts`. The existing file imports from `../src/models.js`; add the two new symbols to that existing import statement and add the index re-export import. First, extend the top-of-file import block. The existing import is:
  ```ts
  import {
    ANTHROPIC_MODELS,
    DEFAULT_ANTHROPIC_MODEL,
    OPENAI_MODELS,
    DEFAULT_OPENAI_MODEL,
    MINIMAX_MODELS,
    DEFAULT_MINIMAX_MODEL,
  } from '../src/models.js'
  ```
  Change it to add the two Gemini symbols:
  ```ts
  import {
    ANTHROPIC_MODELS,
    DEFAULT_ANTHROPIC_MODEL,
    OPENAI_MODELS,
    DEFAULT_OPENAI_MODEL,
    MINIMAX_MODELS,
    DEFAULT_MINIMAX_MODEL,
    GEMINI_MODELS,
    DEFAULT_GEMINI_MODEL,
  } from '../src/models.js'
  ```
  Then append this `describe` block at the END of the file (after the last existing block, inside the top-level `describe('models', ...)` if the file is structured that way — match the surrounding nesting; the existing per-provider blocks are nested under a top-level `describe('models')`, so nest this one the same way):
  ```ts
  describe('GEMINI_MODELS', () => {
    it('contains the eight Gemini model IDs (models.rs:24-33)', () => {
      expect(GEMINI_MODELS).toEqual([
        'gemini-2.5-flash',
        'gemini-2.5-flash-lite',
        'gemini-2.5-pro',
        'gemini-flash-latest',
        'gemini-2.0-flash',
        'gemini-2.0-flash-lite',
        'gemini-1.5-pro',
        'gemini-1.5-flash',
      ])
    })

    it('is a readonly const tuple (length 8)', () => {
      expect(GEMINI_MODELS.length).toBe(8)
    })
  })

  describe('DEFAULT_GEMINI_MODEL', () => {
    it('is gemini-2.5-flash (models.rs:22)', () => {
      expect(DEFAULT_GEMINI_MODEL).toBe('gemini-2.5-flash')
    })

    it('is a member of GEMINI_MODELS', () => {
      expect(GEMINI_MODELS).toContain(DEFAULT_GEMINI_MODEL)
    })
  })
  ```
  Run the test and watch it FAIL (symbols not yet exported):
  ```
  npm run test -- tests/models.test.ts
  ```
  Expected output: vitest reports a failure — either a compile/import error `No "GEMINI_MODELS" export is defined` or the new assertions throwing. (Tests in `tests/` are NOT tsc-checked, so the failure surfaces at runtime as an undefined import.)

- [ ] **Step 2: Add the constants to make the test pass.**
  Append to the END of `sdks/typescript/src/models.ts` (after `DEFAULT_MINIMAX_MODEL` on line 28):
  ```ts

  /** Gemini model IDs (models.rs:24-33) */
  export const GEMINI_MODELS = [
    'gemini-2.5-flash',
    'gemini-2.5-flash-lite',
    'gemini-2.5-pro',
    'gemini-flash-latest',
    'gemini-2.0-flash',
    'gemini-2.0-flash-lite',
    'gemini-1.5-pro',
    'gemini-1.5-flash',
  ] as const

  /** Default Gemini model (models.rs:22) */
  export const DEFAULT_GEMINI_MODEL = 'gemini-2.5-flash'
  ```
  Run the test and watch it PASS:
  ```
  npm run test -- tests/models.test.ts
  ```
  Expected output: all model tests pass, including the new `GEMINI_MODELS` and `DEFAULT_GEMINI_MODEL` blocks (4 new assertions green).

- [ ] **Step 3: Build the package (TS strict gate).**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0 with no errors. The new exports are plain `as const` arrays + a string literal — no new imports, no `Option`-style types, so strict mode is satisfied.

- [ ] **Step 4: Commit (conventional).**
  ```
  git add src/models.ts tests/models.test.ts
  git commit -m "feat(models): add DEFAULT_GEMINI_MODEL and GEMINI_MODELS"
  ```
  (Commit message ends with the project's required Co-Authored-By trailer per CLAUDE.md.)

NOTE for downstream tasks: Task 3 imports `DEFAULT_GEMINI_MODEL` from `'../models.js'`; Task 6 re-exports both `DEFAULT_GEMINI_MODEL` and `GEMINI_MODELS` from `index.ts`. Neither re-declares these symbols.

---

### Task 2: serialize/gemini.ts — Gemini request serializer

Creates `serializeGeminiRequest(req, model)`, the pure function that projects a provider-agnostic `ChatRequest` onto the Gemini `generateContent`/`streamGenerateContent` request body. Mirrors the structure of `serialize/openai.ts` but with Gemini's wire shape (contract §3, Rust `gemini.rs:80-237`): `contents[]` with `role: 'model'` for assistant, `systemInstruction` as a SEPARATE top-level field (NOT a message), `generationConfig`, `tools[].functionDeclarations`, `toolConfig.functionCallingConfig`, `inlineData`/`fileData` for images, `functionResponse` for tool messages. The `model` param is accepted for signature symmetry with `serializeOpenAiRequest` but is NOT placed in the body (Gemini puts the model in the URL path — Task 3 owns the URL).

STRICT OWNERSHIP: this task touches ONLY `src/serialize/gemini.ts` (NEW) and `tests/serialize.gemini.test.ts` (NEW). It imports `ChatRequest`, `ContentBlock`, `Message` from `../types.js`. It declares NO provider class, NO model constants, NO exports beyond `serializeGeminiRequest`.

**Files:**
- `sdks/typescript/src/serialize/gemini.ts` (CREATE)
- `sdks/typescript/tests/serialize.gemini.test.ts` (CREATE)

All commands run FROM `sdks/typescript/`.

- [ ] **Step 1: Write the failing test file first (TDD).**
  Create `sdks/typescript/tests/serialize.gemini.test.ts`. This proves the load-bearing wire transformations from contract §14 / `gemini.rs:556-939`. Tests are NOT tsc-checked.
  ```ts
  import { describe, it, expect } from 'vitest'
  import { serializeGeminiRequest } from '../src/serialize/gemini.js'
  import type { ChatRequest } from '../src/types.js'

  const MODEL = 'gemini-2.5-flash'

  describe('serializeGeminiRequest — contents & role mapping', () => {
    it('serializes a simple user message (gemini.rs:565-569)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'Hello' }] }
      const body = serializeGeminiRequest(req, MODEL)
      const contents = body.contents as any[]
      expect(contents[0].role).toBe('user')
      expect(contents[0].parts[0].text).toBe('Hello')
    })

    it('maps assistant role to "model" (gemini.rs:571-576)', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: 'Hi' },
          { role: 'assistant', content: 'Hello back' },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const contents = body.contents as any[]
      expect(contents[1].role).toBe('model')
      expect(contents[1].parts[0].text).toBe('Hello back')
    })

    it('serializes assistant text + tool calls; functionCall uses wire field "args" (gemini.rs:914-928)', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: 'go' },
          {
            role: 'assistant',
            content: 'Let me check.',
            toolCalls: [{ id: 'c1', name: 'foo', input: { x: 1 } }],
          },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const parts = (body.contents as any[])[1].parts
      expect((body.contents as any[])[1].role).toBe('model')
      expect(parts[0].text).toBe('Let me check.')
      expect(parts[1].functionCall.name).toBe('foo')
      expect(parts[1].functionCall.args.x).toBe(1)
    })

    it('does NOT place model in the body (URL path owns it)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      const body = serializeGeminiRequest(req, MODEL)
      expect(body.model).toBeUndefined()
    })
  })

  describe('serializeGeminiRequest — systemInstruction (separate top-level field)', () => {
    it('extracts a system MESSAGE to systemInstruction; contents excludes it (gemini.rs:578-587)', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'system', content: 'Be concise.' },
          { role: 'user', content: 'Hi' },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.systemInstruction as any).parts[0].text).toBe('Be concise.')
      expect((body.contents as any[]).length).toBe(1)
      expect((body.contents as any[])[0].role).toBe('user')
    })

    it('uses req.system when set (no system message)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'Hi' }], system: 'Be brief.' }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.systemInstruction as any).parts[0].text).toBe('Be brief.')
    })

    it('joins systemBlocks with \\n (gemini.rs:870-883)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        systemBlocks: [{ text: 'Block 1.' }, { text: 'Block 2.' }],
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.systemInstruction as any).parts[0].text).toBe('Block 1.\nBlock 2.')
    })

    it('systemBlocks take priority over system field (gemini.rs:885-898)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        system: 'fallback',
        systemBlocks: [{ text: 'from blocks' }],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const text = (body.systemInstruction as any).parts[0].text
      expect(text).toBe('from blocks')
      expect(text).not.toContain('fallback')
    })

    it('empty systemBlocks falls back to system field (gemini.rs:900-912)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        system: 'use this',
        systemBlocks: [],
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.systemInstruction as any).parts[0].text).toBe('use this')
    })

    it('omits systemInstruction entirely when empty', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      const body = serializeGeminiRequest(req, MODEL)
      expect(body.systemInstruction).toBeUndefined()
    })
  })

  describe('serializeGeminiRequest — image content blocks', () => {
    it('base64 image -> inlineData.mimeType/data (gemini.rs:712-725)', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: 'look at this',
            contentBlocks: [
              { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
            ],
          },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const parts = (body.contents as any[])[0].parts
      // content 'look at this' becomes parts[0]; image is parts[1]
      const img = parts.find((p: any) => p.inlineData)
      expect(img.inlineData.mimeType).toBe('image/png')
      expect(img.inlineData.data).toBe('abc123')
    })

    it('url image -> fileData.fileUri (gemini.rs:727-737)', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'url', url: 'https://example.com/img.jpg' } },
            ],
          },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const part = (body.contents as any[])[0].parts[0]
      expect(part.fileData.fileUri).toBe('https://example.com/img.jpg')
    })

    it('throws on a document block (defensive — validateRequest rejects first) (gemini.rs:112)', () => {
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'document', source: { type: 'url', url: 'https://example.com/doc.pdf' } },
            ],
          },
        ],
      }
      expect(() => serializeGeminiRequest(req, MODEL)).toThrow(
        'Gemini does not support document content blocks',
      )
    })
  })

  describe('serializeGeminiRequest — tool (functionResponse) messages', () => {
    it('tool message -> role:user functionResponse; toolCallId is the NAME; JSON content parsed (gemini.rs:589-596)', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: '?' },
          { role: 'tool', toolCallId: 'get_weather', content: '{"result": "sunny"}' },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const part = (body.contents as any[])[1].parts[0]
      expect((body.contents as any[])[1].role).toBe('user')
      expect(part.functionResponse.name).toBe('get_weather')
      expect(part.functionResponse.response.result).toBe('sunny')
    })

    it('non-JSON tool content wraps as { result } (gemini.rs:599-605)', () => {
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: '?' },
          { role: 'tool', toolCallId: '', content: 'done' },
        ],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const part = (body.contents as any[])[1].parts[0]
      expect(part.functionResponse.name).toBe('')
      expect(part.functionResponse.response.result).toBe('done')
    })
  })

  describe('serializeGeminiRequest — generationConfig', () => {
    it('always emits maxOutputTokens, defaulting to 8192 (gemini.rs:174-175)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.generationConfig as any).maxOutputTokens).toBe(8192)
    })

    it('maxTokens maps to maxOutputTokens (gemini.rs:847-855)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], maxTokens: 256 }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.generationConfig as any).maxOutputTokens).toBe(256)
    })

    it('temperature emitted only when set (gemini.rs:645-656)', () => {
      const withTemp = serializeGeminiRequest(
        { messages: [{ role: 'user', content: 'hi' }], temperature: 0.3 },
        MODEL,
      )
      expect((withTemp.generationConfig as any).temperature).toBeCloseTo(0.3, 6)
      const without = serializeGeminiRequest({ messages: [{ role: 'user', content: 'hi' }] }, MODEL)
      expect((without.generationConfig as any).temperature).toBeUndefined()
    })

    it('non-empty stopSequences emitted (gemini.rs:835-845)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        stopSequences: ['END', 'STOP'],
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.generationConfig as any).stopSequences).toEqual(['END', 'STOP'])
    })

    it('does NOT emit thinkingConfig (parity — gemini.rs has none)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        thinking: { budgetTokens: 1024 },
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.generationConfig as any).thinkingConfig).toBeUndefined()
    })
  })

  describe('serializeGeminiRequest — tools & toolConfig', () => {
    const tool = (name: string, description = '', inputSchema?: Record<string, unknown>) => ({
      name,
      description,
      inputSchema,
    })

    it('emits functionDeclarations with name + parameters (gemini.rs:740-791)', () => {
      const schema = { type: 'object', properties: { q: { type: 'string' } }, required: ['q'] }
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'find' }],
        tools: [tool('search', '', schema)],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const decls = (body.tools as any[])[0].functionDeclarations
      expect(decls[0].name).toBe('search')
      expect(decls[0].parameters).toEqual(schema)
    })

    it('defaults missing description to "" and missing schema to {type:object,properties:{}} (serialize/openai.ts:151-152)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [{ name: 'bare' }],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const decl = (body.tools as any[])[0].functionDeclarations[0]
      expect(decl.description).toBe('')
      expect(decl.parameters).toEqual({ type: 'object', properties: {} })
    })

    it('multiple tools become multiple declarations (gemini.rs:740-768)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [tool('search', 'Search'), tool('calc', 'Calculate')],
      }
      const body = serializeGeminiRequest(req, MODEL)
      const decls = (body.tools as any[])[0].functionDeclarations
      expect(decls.length).toBe(2)
      expect(decls[0].name).toBe('search')
      expect(decls[1].name).toBe('calc')
    })

    it('toolChoice undefined -> AUTO (gemini.rs:210)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [tool('t')],
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.toolConfig as any).functionCallingConfig.mode).toBe('AUTO')
    })

    it('toolChoice auto -> AUTO (gemini.rs:794-810)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [tool('t')],
        toolChoice: { type: 'auto' },
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.toolConfig as any).functionCallingConfig.mode).toBe('AUTO')
    })

    it('toolChoice required -> ANY (gemini.rs:607-624)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'find it' }],
        tools: [tool('search', 'Search', { type: 'object', properties: {} })],
        toolChoice: { type: 'required' },
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect((body.toolConfig as any).functionCallingConfig.mode).toBe('ANY')
    })

    it('toolChoice tool(name) -> ANY + allowedFunctionNames (gemini.rs:812-833)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [tool('special')],
        toolChoice: { type: 'tool', name: 'special' },
      }
      const body = serializeGeminiRequest(req, MODEL)
      const fc = (body.toolConfig as any).functionCallingConfig
      expect(fc.mode).toBe('ANY')
      expect(fc.allowedFunctionNames).toEqual(['special'])
    })

    it('toolChoice none REMOVES tools and emits NO toolConfig (gemini.rs:627-643)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        tools: [tool('search')],
        toolChoice: { type: 'none' },
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect(body.tools).toBeUndefined()
      expect(body.toolConfig).toBeUndefined()
    })

    it('empty tools omits both tools and toolConfig (gemini.rs:930-939)', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], tools: [] }
      const body = serializeGeminiRequest(req, MODEL)
      expect(body.tools).toBeUndefined()
      expect(body.toolConfig).toBeUndefined()
    })
  })

  describe('serializeGeminiRequest — providerOptions merge', () => {
    it('merges providerOptions at the top level last (gemini.rs:857-867)', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        providerOptions: { safetySettings: [{ category: 'ALL', threshold: 'BLOCK_NONE' }] },
      }
      const body = serializeGeminiRequest(req, MODEL)
      expect(body.safetySettings).toBeDefined()
    })
  })
  ```
  Run and watch it FAIL (module does not exist):
  ```
  npm run test -- tests/serialize.gemini.test.ts
  ```
  Expected output: vitest fails to resolve `../src/serialize/gemini.js` (Cannot find module / failed to load).

- [ ] **Step 2: Implement the serializer to make the test pass.**
  Create `sdks/typescript/src/serialize/gemini.ts`. This mirrors the structure of `serialize/openai.ts` (same `serialize*Request(req, model)` export shape, same `providerOptions` Object.assign-last idiom, same defensive document throw). Every branch is grounded in `gemini.rs:80-237`.
  ```ts
  import type { ChatRequest, ContentBlock, Message } from '../types.js'

  /**
   * Gemini generateContent / streamGenerateContent request serializer.
   *
   * Mirrors Rust `GeminiProvider::build_request` (gemini.rs:80-237). Projects the
   * provider-agnostic ChatRequest onto the Gemini REST wire, which diverges from
   * OpenAI/Anthropic in several load-bearing ways (contract §3):
   *
   *   1. The MODEL is NOT in the body — it lives in the URL path. The `model`
   *      param is accepted for signature symmetry with serializeOpenAiRequest but
   *      is never written to the returned object.
   *   2. systemInstruction is a SEPARATE top-level field (NOT a role:system
   *      message in `contents`). (gemini.rs:151-192)
   *   3. Assistant role serializes to `'model'` (gemini.rs:120,136).
   *   4. Tool calls use the WIRE field `args` (functionCall.args) even though the
   *      SDK type field is `input`. (gemini.rs:129) Images use `inlineData`
   *      (camelCase, key `mimeType`) / `fileData.fileUri`. Tool declarations use
   *      `parameters` (NOT `input_schema`). (gemini.rs:99-110,196-207)
   *   5. Tool messages become a `role:'user'` part with `functionResponse`, and
   *      `toolCallId` carries the FUNCTION NAME by this SDK's convention.
   *      (gemini.rs:138-147)
   *
   * Capability validation (provider.ts validateRequest) rejects document blocks
   * BEFORE serialization, so the document branch throws defensively (mirrors
   * serialize/openai.ts:58 and Rust's `unreachable!()` at gemini.rs:112).
   */

  const DEFAULT_MAX_TOKENS = 8192

  function serializeUserPart(block: ContentBlock): Record<string, unknown> {
    if (block.type === 'text') {
      return { text: block.text }
    }
    if (block.type === 'image') {
      const source = block.source
      if (source.type === 'base64') {
        // Field is inlineData (camelCase); key mimeType (NOT media_type),
        // maps from TS source.mediaType. (gemini.rs:99-107)
        return { inlineData: { mimeType: source.mediaType, data: source.data } }
      }
      // source.type === 'url' (gemini.rs:108-110)
      return { fileData: { fileUri: source.url } }
    }
    // Document blocks are not supported by Gemini; capability validation rejects
    // them before serialization (gemini.rs:112 `unreachable!()`).
    throw new Error('Gemini does not support document content blocks')
  }

  // `_model` is accepted for signature symmetry with the other serializers but
  // is unused here — Gemini puts the model in the URL path, not the body.
  export function serializeGeminiRequest(
    req: ChatRequest,
    _model: string,
  ): Record<string, unknown> {
    const contents: Record<string, unknown>[] = []
    let extractedSystem: string | undefined

    for (const message of req.messages) {
      switch (message.role) {
        case 'system': {
          // NOT pushed to contents; captured for systemInstruction fallback
          // (last system message wins). (gemini.rs:86-88)
          extractedSystem = message.content
          break
        }
        case 'user': {
          const parts: Record<string, unknown>[] = []
          if (message.content !== '') {
            parts.push({ text: message.content }) // (gemini.rs:91-93)
          }
          for (const block of message.contentBlocks ?? []) {
            parts.push(serializeUserPart(block)) // (gemini.rs:94-114)
          }
          if (parts.length === 0) {
            parts.push({ text: '' }) // (gemini.rs:115-117)
          }
          contents.push({ role: 'user', parts })
          break
        }
        case 'assistant': {
          const parts: Record<string, unknown>[] = []
          if (message.content !== '') {
            parts.push({ text: message.content }) // (gemini.rs:122-124)
          }
          for (const tc of message.toolCalls ?? []) {
            // Wire uses `args` for functionCall input; SDK type field is `input`.
            // (gemini.rs:125-132)
            parts.push({ functionCall: { name: tc.name, args: tc.input } })
          }
          if (parts.length === 0) {
            parts.push({ text: '' }) // (gemini.rs:133-135)
          }
          // Assistant role serializes to 'model'. (gemini.rs:136)
          contents.push({ role: 'model', parts })
          break
        }
        case 'tool': {
          // toolCallId holds the function name by this SDK's convention.
          // (gemini.rs:139-140)
          const name = message.toolCallId ?? ''
          let response: unknown
          try {
            response = JSON.parse(message.content) // (gemini.rs:141)
          } catch {
            response = { result: message.content } // (gemini.rs:142)
          }
          contents.push({
            role: 'user',
            parts: [{ functionResponse: { name, response } }],
          })
          break
        }
      }
    }

    // systemInstruction resolution priority (gemini.rs:151-172):
    // 1. systemBlocks joined with '\n' (when present AND non-empty)
    // 2. else req.system ?? extractedSystem ?? ''
    let systemText: string
    if (req.systemBlocks !== undefined) {
      const joined = req.systemBlocks.map((b) => b.text).join('\n')
      systemText = joined !== '' ? joined : req.system ?? extractedSystem ?? ''
    } else {
      systemText = req.system ?? extractedSystem ?? ''
    }

    // generationConfig is ALWAYS present. (gemini.rs:174-188)
    const generationConfig: Record<string, unknown> = {
      maxOutputTokens: req.maxTokens ?? DEFAULT_MAX_TOKENS,
    }
    if (req.temperature !== undefined) {
      generationConfig.temperature = req.temperature // (gemini.rs:176-178)
    }
    if (req.stopSequences && req.stopSequences.length > 0) {
      generationConfig.stopSequences = req.stopSequences // (gemini.rs:179-183)
    }

    const body: Record<string, unknown> = {
      contents,
      generationConfig,
    }

    // systemInstruction: SEPARATE top-level field, emitted only when non-empty.
    // (gemini.rs:190-192)
    if (systemText !== '') {
      body.systemInstruction = { parts: [{ text: systemText }] }
    }

    // tools + toolConfig only when req.tools is non-empty. (gemini.rs:194-195)
    if (req.tools && req.tools.length > 0) {
      const declarations = req.tools.map((tool) => ({
        name: tool.name,
        description: tool.description ?? '',
        parameters: tool.inputSchema ?? { type: 'object', properties: {} },
      }))
      body.tools = [{ functionDeclarations: declarations }]

      // tool_choice -> toolConfig.functionCallingConfig.mode. (gemini.rs:209-224)
      let mode: 'AUTO' | 'ANY' | 'NONE'
      const choice = req.toolChoice
      if (choice === undefined || choice.type === 'auto') {
        mode = 'AUTO'
      } else if (choice.type === 'required') {
        mode = 'ANY'
      } else if (choice.type === 'none') {
        // NONE: remove tools, emit NO toolConfig. (gemini.rs:212-215,218)
        delete body.tools
        mode = 'NONE'
      } else {
        // choice.type === 'tool'
        mode = 'ANY'
      }

      if (mode !== 'NONE') {
        const fcConfig: Record<string, unknown> = { mode }
        if (choice && choice.type === 'tool') {
          fcConfig.allowedFunctionNames = [choice.name] // (gemini.rs:220-222)
        }
        body.toolConfig = { functionCallingConfig: fcConfig }
      }
    }

    // providerOptions merge LAST (top-level). (gemini.rs:228-234,
    // serialize/openai.ts:181-183)
    if (req.providerOptions && typeof req.providerOptions === 'object') {
      Object.assign(body, req.providerOptions)
    }

    return body
  }
  ```
  NOTE: the `_model` param is unused on purpose (signature symmetry with `serializeOpenAiRequest`; Gemini puts the model in the URL path, not the body). The `_`-prefix is the convention for an intentionally-unused param; `tsc` does not error on it, so the `build` gate passes.

  Run and watch it PASS:
  ```
  npm run test -- tests/serialize.gemini.test.ts
  ```
  Expected output: all serialize-gemini assertions pass (contents/role mapping, systemInstruction priority, image inlineData/fileData, functionResponse tool messages, generationConfig, tools/toolConfig tool_choice table, providerOptions merge).

- [ ] **Step 3: Build (TS strict gate).**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0. All maps return `Record<string, unknown>`; discriminated `ContentBlock`/`ToolChoice` narrowing is exhaustive; relative imports end in `.js` (NodeNext). No `any` casts in the source.

- [ ] **Step 4: Commit (conventional).**
  ```
  git add src/serialize/gemini.ts tests/serialize.gemini.test.ts
  git commit -m "feat(serialize): add Gemini request serializer"
  ```

NOTE for downstream tasks: Task 3 imports `serializeGeminiRequest` from `'../serialize/gemini.js'`; Task 6 may optionally re-export it from `index.ts`. This task declares no other public symbols.

---

### Task 3: providers/gemini.ts — GeminiProvider (chat + SSE stream)

Creates `GeminiProvider`, the structural `ProviderImpl` for Google's `generativelanguage` REST API. Implements `chat()` (POST `:generateContent`), `stream()` (POST `:streamGenerateContent?alt=sse` driven by `parseSse`), response parsing (candidates parts -> content + functionCall toolCalls with client-generated `call_N` ids, finishReason -> StopReason mapping, usageMetadata), synthesized 3-event tool-call stream triplets, `capabilities()` (returns `withImage()`, which post-M4 yields `supportsMcp:false`), `withRetryPolicy`, OpenAI-mirrored `classifyHttpError` + status-aware retry (chat whole / stream initial-fetch-only), `x-goog-api-key` auth, default model from Task 1, and a `baseUrl` override. All wire behavior mirrors Rust `gemini.rs:24-536`; infra idioms mirror `providers/openai.ts`.

STRICT OWNERSHIP: this task touches ONLY `src/providers/gemini.ts` (NEW) and `tests/providers-gemini.test.ts` (NEW), plus the OPTIONAL `tests/gemini-live.test.ts` (env-gated). It IMPORTS `DEFAULT_GEMINI_MODEL` (Task 1) and `serializeGeminiRequest` (Task 2); it does NOT re-declare them. It does NOT touch `provider.ts` (Task 4), `client.ts` (Task 5), or `index.ts` (Task 6).

DEPENDS ON: Task 1 (`DEFAULT_GEMINI_MODEL`), Task 2 (`serializeGeminiRequest`), and M4 (`withImage()` returns `{ supportsImage:true, supportsDocument:false, supportsMcp:false }`).

**Files:**
- `sdks/typescript/src/providers/gemini.ts` (CREATE)
- `sdks/typescript/tests/providers-gemini.test.ts` (CREATE)
- `sdks/typescript/tests/gemini-live.test.ts` (CREATE, optional, env-gated)

All commands run FROM `sdks/typescript/`.

- [ ] **Step 1: Write the failing test file first (TDD).**
  Create `sdks/typescript/tests/providers-gemini.test.ts`. Mocks fetch via `vi.stubGlobal` returning a `Response` (whose `.body` is a real `ReadableStream`, which `parseSse` consumes) — exactly the idiom in `tests/providers-openai.test.ts:205-218`. Grounded in contract §14 / `gemini.rs:1056-1154`. Note: tool-call ids are process-global; assert with the regex `^call_\d+$`, never exact values (contract §9).
  ```ts
  import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
  import { GeminiProvider } from '../src/providers/gemini.js'
  import { collectStream } from '../src/stream.js'
  import { UnsupportedFeatureError } from '../src/error.js'
  import { validateRequest } from '../src/provider.js'
  import type { ChatRequest, StreamEvent } from '../src/types.js'

  const ID_RE = /^call_\d+$/

  describe('GeminiProvider — capabilities', () => {
    it('supports image, not document, not MCP (gemini.rs:315-317; post-M4 withImage)', () => {
      const caps = new GeminiProvider('key').capabilities()
      expect(caps.supportsImage).toBe(true)
      expect(caps.supportsDocument).toBe(false)
      expect(caps.supportsMcp).toBe(false)
    })
  })

  describe('GeminiProvider — chat URL, auth, and text parse', () => {
    let captured: { url: string; headers: Record<string, string>; body: any } | null = null
    beforeEach(() => {
      captured = null
    })
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('uses :generateContent path, x-goog-api-key auth, and model in the URL (gemini.rs:64-77)', async () => {
      const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
        captured = {
          url,
          headers: (options?.headers as Record<string, string>) ?? {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response(
          JSON.stringify({
            candidates: [
              {
                content: { parts: [{ text: 'Hi!' }], role: 'model' },
                finishReason: 'STOP',
              },
            ],
            usageMetadata: { promptTokenCount: 5, candidatesTokenCount: 2 },
            modelVersion: 'gemini-2.5-flash',
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new GeminiProvider('fake-key')
      const resp = await provider.chat({ messages: [{ role: 'user', content: 'Hello' }] })

      expect(captured?.url).toBe(
        'https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent',
      )
      expect(captured?.headers['x-goog-api-key']).toBe('fake-key')
      expect(captured?.headers['authorization']).toBeUndefined()
      // model lives in URL path, NOT in body
      expect(captured?.body.model).toBeUndefined()
      expect(resp.content).toBe('Hi!')
      expect(resp.stopReason).toBe('end_turn') // STOP -> end_turn (NOT 'stop')
      expect(resp.model).toBe('gemini-2.5-flash')
      expect(resp.usage.inputTokens).toBe(5)
      expect(resp.usage.outputTokens).toBe(2)
      expect(resp.toolCalls).toEqual([])
    })

    it('request.model overrides the provider default in the URL path (gemini.rs:64,69)', async () => {
      const mockFetch = vi.fn(async (url: string) => {
        captured = { url, headers: {}, body: null }
        return new Response(
          JSON.stringify({ candidates: [{ content: { parts: [{ text: 'ok' }] }, finishReason: 'STOP' }] }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      })
      vi.stubGlobal('fetch', mockFetch)
      const provider = new GeminiProvider('k')
      await provider.chat({ messages: [{ role: 'user', content: 'hi' }], model: 'gemini-2.5-pro' })
      expect(captured?.url).toContain('/models/gemini-2.5-pro:generateContent')
    })

    it('parses a functionCall into a ToolCall with a client-generated id (gemini.rs:677-695)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [
              {
                content: {
                  parts: [{ functionCall: { name: 'search', args: { q: 'rust' } } }],
                  role: 'model',
                },
                finishReason: 'STOP',
              },
            ],
            usageMetadata: { promptTokenCount: 8, candidatesTokenCount: 3 },
            modelVersion: 'gemini-2.5-flash',
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({
        messages: [{ role: 'user', content: 'find' }],
      })
      expect(resp.toolCalls.length).toBe(1)
      expect(resp.toolCalls[0].name).toBe('search')
      expect(resp.toolCalls[0].input.q).toBe('rust') // input from wire `args`
      expect(resp.toolCalls[0].id).toMatch(ID_RE)
      expect(resp.stopReason).toBe('tool_use') // STOP + tool calls
    })

    it('MAX_TOKENS finishReason -> max_tokens (gemini.rs:697-708)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [
              { content: { parts: [{ text: 'truncated' }] }, finishReason: 'MAX_TOKENS' },
            ],
            usageMetadata: { promptTokenCount: 5, candidatesTokenCount: 100 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.stopReason).toBe('max_tokens')
    })

    it('SAFETY finishReason (no tool calls) -> other (gemini.rs:1032-1043)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [{ content: { parts: [{ text: '' }] }, finishReason: 'SAFETY' }],
            usageMetadata: { promptTokenCount: 2, candidatesTokenCount: 0 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.stopReason).toBe('other')
    })

    it('multiple tool calls get unique non-empty ids (gemini.rs:943-983)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [
              {
                content: {
                  parts: [
                    { functionCall: { name: 'a', args: {} } },
                    { functionCall: { name: 'b', args: {} } },
                  ],
                },
                finishReason: 'STOP',
              },
            ],
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.toolCalls.length).toBe(2)
      expect(resp.toolCalls[0].id).toMatch(ID_RE)
      expect(resp.toolCalls[1].id).toMatch(ID_RE)
      expect(resp.toolCalls[0].id).not.toBe(resp.toolCalls[1].id)
    })

    it('mixed text + functionCall -> tool_use, content preserved (gemini.rs:985-1004)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [
              {
                content: {
                  parts: [
                    { text: 'Let me search for that.' },
                    { functionCall: { name: 'search', args: { q: 'test' } } },
                  ],
                },
                finishReason: 'STOP',
              },
            ],
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.content).toBe('Let me search for that.')
      expect(resp.toolCalls.length).toBe(1)
      expect(resp.stopReason).toBe('tool_use')
    })

    it('missing usageMetadata yields zero tokens (gemini.rs:1020-1030)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [{ content: { parts: [{ text: 'hi' }] }, finishReason: 'STOP' }],
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.usage.inputTokens).toBe(0)
      expect(resp.usage.outputTokens).toBe(0)
    })

    it('falls back to the resolved request model when modelVersion absent (gemini.rs:298-302)', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(
          JSON.stringify({
            candidates: [{ content: { parts: [{ text: 'hi' }] }, finishReason: 'STOP' }],
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      )
      vi.stubGlobal('fetch', mockFetch)
      const resp = await new GeminiProvider('k', 'gemini-1.5-pro').chat({
        messages: [{ role: 'user', content: 'hi' }],
      })
      expect(resp.model).toBe('gemini-1.5-pro')
    })

    it('sends a base64 image as inlineData (no validation error, image cap)', async () => {
      const mockFetch = vi.fn(async (_url: string, options?: RequestInit) => {
        captured = {
          url: _url,
          headers: {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response(
          JSON.stringify({ candidates: [{ content: { parts: [{ text: 'seen' }] }, finishReason: 'STOP' }] }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      })
      vi.stubGlobal('fetch', mockFetch)
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc' } },
            ],
          },
        ],
      }
      const resp = await new GeminiProvider('k').chat(req)
      const part = captured?.body.contents[0].parts[0]
      expect(part.inlineData.mimeType).toBe('image/png')
      expect(part.inlineData.data).toBe('abc')
      expect(resp.content).toBe('seen')
    })
  })

  describe('GeminiProvider — document rejection (validateRequest gate)', () => {
    it('document blocks are rejected by validateRequest before any HTTP call', () => {
      const provider = new GeminiProvider('k')
      const req: ChatRequest = {
        messages: [
          {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'document', source: { type: 'url', url: 'https://x.com/d.pdf' } },
            ],
          },
        ],
      }
      // The provider's own capabilities drive validateRequest (provider.ts:59-71).
      expect(() => validateRequest(req, provider.capabilities())).toThrow(UnsupportedFeatureError)
    })
  })

  describe('GeminiProvider — SSE stream (no [DONE]; finishReason terminates)', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('emits text events then a finishReason-driven done; collectStream reassembles (gemini.rs:1127-1154)', async () => {
      const mockFetch = vi.fn(async () => {
        const sse = [
          'data: {"candidates":[{"content":{"parts":[{"text":"Hi"}],"role":"model"}}]}\n\n',
          'data: {"candidates":[{"content":{"parts":[{"text":" there"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":2}}\n\n',
        ].join('')
        return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new GeminiProvider('fake-key')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Hello' }] })) {
        events.push(evt)
      }

      const texts = events.filter((e) => e.eventType === 'text' && !e.done)
      expect(texts.map((e) => e.content).join('')).toBe('Hi there')
      const last = events[events.length - 1]
      expect(last.done).toBe(true)
      expect(last.stopReason).toBe('end_turn')

      // Round-trip through collectStream (re-drive a fresh stream).
      const resp = await collectStream(
        provider.stream({ messages: [{ role: 'user', content: 'Hello' }] }),
      )
      expect(resp.content).toBe('Hi there')
      expect(resp.stopReason).toBe('end_turn')
    })

    it('synthesizes start/argsWithId/end for a streamed functionCall in ONE args chunk (gemini.rs:477-493)', async () => {
      const mockFetch = vi.fn(async () => {
        const sse = [
          'data: {"candidates":[{"content":{"parts":[{"functionCall":{"name":"search","args":{"q":"x"}}}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":1,"candidatesTokenCount":1}}\n\n',
        ].join('')
        return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new GeminiProvider('k')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'find' }] })) {
        events.push(evt)
      }

      const start = events.find((e) => e.eventType === 'tool_call_start')
      const args = events.find((e) => e.eventType === 'tool_call_args')
      const end = events.find((e) => e.eventType === 'tool_call_end')
      expect(start?.toolCallId).toMatch(ID_RE)
      expect(start?.toolCallName).toBe('search')
      // args is the WHOLE JSON serialized in one shot (not incremental)
      expect(args?.toolCallArgsDelta).toBe('{"q":"x"}')
      expect(args?.toolCallId).toBe(start?.toolCallId)
      expect(end?.toolCallId).toBe(start?.toolCallId)
      const last = events[events.length - 1]
      expect(last.done).toBe(true)
      expect(last.stopReason).toBe('tool_use') // STOP + tool calls

      // collectStream reassembles the ToolCall with input from the serialized args.
      const resp = await collectStream(
        provider.stream({ messages: [{ role: 'user', content: 'find' }] }),
      )
      expect(resp.toolCalls.length).toBe(1)
      expect(resp.toolCalls[0].name).toBe('search')
      expect(resp.toolCalls[0].input.q).toBe('x')
    })

    it('skips a defensive [DONE] line and does not fabricate a done on EOF (gemini.rs:447-449,531)', async () => {
      const mockFetch = vi.fn(async () => {
        // No finishReason anywhere; a stray [DONE] must be ignored; stream ends on EOF.
        const sse = [
          'data: {"candidates":[{"content":{"parts":[{"text":"partial"}],"role":"model"}}]}\n\n',
          'data: [DONE]\n\n',
        ].join('')
        return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new GeminiProvider('k')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(evt)
      }
      // Exactly one text event; NO fabricated done (Gemini adapter never emits a
      // defensive EOF done — only finishReason drives it).
      expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
        'partial',
      ])
      expect(events.some((e) => e.done)).toBe(false)

      // collectStream still produces a response (tolerates a missing done).
      const resp = await collectStream(
        provider.stream({ messages: [{ role: 'user', content: 'hi' }] }),
      )
      expect(resp.content).toBe('partial')
      expect(resp.stopReason).toBe('end_turn') // fabricated from toolCalls.length === 0
    })

    it('emits a usage event from usageMetadata (gemini.rs:496-511)', async () => {
      const mockFetch = vi.fn(async () => {
        const sse =
          'data: {"candidates":[{"content":{"parts":[{"text":"x"}],"role":"model"},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":7,"candidatesTokenCount":4}}\n\n'
        return new Response(sse, { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
      vi.stubGlobal('fetch', mockFetch)
      const provider = new GeminiProvider('k')
      const events: StreamEvent[] = []
      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(evt)
      }
      const usage = events.find((e) => e.eventType === 'usage')
      expect(usage?.usage?.inputTokens).toBe(7)
      expect(usage?.usage?.outputTokens).toBe(4)
    })
  })

  describe('GeminiProvider — retry', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('chat retries on 429 then succeeds (gemini.rs:1086-1125)', async () => {
      let calls = 0
      const mockFetch = vi.fn(async () => {
        calls += 1
        if (calls === 1) {
          return new Response(JSON.stringify({ error: { message: 'rate limited' } }), {
            status: 429,
            headers: { 'content-type': 'application/json' },
          })
        }
        return new Response(
          JSON.stringify({
            candidates: [{ content: { parts: [{ text: 'ok' }] }, finishReason: 'STOP' }],
            usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const { RetryPolicy } = await import('../src/retry.js')
      const provider = new GeminiProvider('k').withRetryPolicy(
        new RetryPolicy({ maxRetries: 2, baseDelayMs: 1, maxDelayMs: 10, jitter: false }),
      )
      const resp = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(resp.content).toBe('ok')
      expect(calls).toBe(2)
    })

    it('chat throws a mapped error on a non-retryable 400', async () => {
      const mockFetch = vi.fn(async () =>
        new Response(JSON.stringify({ error: { message: 'bad' } }), {
          status: 400,
          headers: { 'content-type': 'application/json' },
        }),
      )
      vi.stubGlobal('fetch', mockFetch)
      await expect(
        new GeminiProvider('k').chat({ messages: [{ role: 'user', content: 'hi' }] }),
      ).rejects.toMatchObject({ status: 400 })
    })
  })
  ```
  Run and watch it FAIL (module does not exist yet):
  ```
  npm run test -- tests/providers-gemini.test.ts
  ```
  Expected output: vitest cannot resolve `../src/providers/gemini.js`.

- [ ] **Step 2: Implement GeminiProvider to make the test pass.**
  Create `sdks/typescript/src/providers/gemini.ts`. Imports mirror `providers/openai.ts:1-18`. The chat retry uses `withRetry` exactly like `openai.ts:259-263`; the stream uses the manual status-aware loop from `openai.ts:326-351`; `classifyHttpError` is copied verbatim from `openai.ts:38-51` (the existing per-provider duplication pattern). The module-level `genToolCallId` mirrors Rust's `static AtomicU64` (`gemini.rs:27-32`). The SSE loop mirrors `gemini.rs:444-528` with the chunk pending-queue ordering (text -> tool triplets -> usage -> done) and NO defensive EOF done (contrast `openai.ts:460-474`).
  ```ts
  import { isRetryableNetworkError, isRetryableStatus } from '../error.js'
  import { postJson, postStream } from '../http/fetch.js'
  import { parseSse } from '../http/sse.js'
  import { DEFAULT_GEMINI_MODEL } from '../models.js'
  import { withImage, type ProviderCapabilities } from '../provider.js'
  import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'
  import { serializeGeminiRequest } from '../serialize/gemini.js'
  import {
    BoxStream,
    doneWithStopReason,
    textEvent,
    toolCallArgsWithId,
    toolCallEndWithId,
    toolCallStart,
    usageEvent,
  } from '../stream.js'
  import type { ChatRequest, ChatResponse, StopReason, ToolCall } from '../types.js'

  /** Default generativelanguage REST base (gemini.rs:25). */
  const BASE_URL = 'https://generativelanguage.googleapis.com/v1beta'

  /**
   * Module-level monotonic tool-call id generator. Gemini omits call ids on
   * functionCall, so we synthesize `call_${n}`. Mirrors Rust's `static AtomicU64`
   * (gemini.rs:27-32). Process-global (shared across instances) is intentional —
   * tests assert ids via the regex /^call_\d+$/, never exact values.
   */
  let toolCallCounter = 0
  function genToolCallId(): string {
    return `call_${toolCallCounter++}`
  }

  /**
   * Map a Gemini finishReason to the SDK StopReason union (gemini.rs:280-286,
   * 513-520). CRITICAL: "STOP" maps to 'end_turn', NOT 'stop' — do NOT reuse
   * OpenAI's FINISH_REASON_MAP.
   */
  function mapFinishReason(reason: string, hasToolCalls: boolean): StopReason {
    if (reason === 'STOP') return hasToolCalls ? 'tool_use' : 'end_turn'
    if (reason === 'MAX_TOKENS') return 'max_tokens'
    // SAFETY / RECITATION / ... — tool_use if tools were produced, else other.
    return hasToolCalls ? 'tool_use' : 'other'
  }

  /**
   * Copied verbatim from providers/openai.ts:38-51 (the existing per-provider
   * duplication pattern). The mapped HTTP error already carries `.status`
   * (error.ts:51, set inside http/fetch.ts:41 throwMappedError) and
   * `.retryAfterMs` (error.ts:54), so this works unchanged.
   */
  function classifyHttpError(result: unknown): RetryClassification {
    if (result instanceof Error) {
      const error = result as { status?: number; retryAfterMs?: number }
      const status = error.status
      if (
        (status !== undefined && isRetryableStatus(status)) ||
        isRetryableNetworkError(result)
      ) {
        return { retryable: true, retryAfterMs: error.retryAfterMs }
      }
      throw result
    }
    return { retryable: false }
  }

  /**
   * Gemini provider over the generativelanguage REST API (generateContent +
   * streamGenerateContent?alt=sse). Structurally satisfies ProviderImpl. All
   * wire behavior mirrors Rust gemini.rs; infra idioms mirror providers/openai.ts.
   */
  export class GeminiProvider {
    private readonly model: string
    private readonly baseUrl: string
    private retryPolicy: RetryPolicy

    constructor(
      private readonly apiKey: string,
      model?: string,
      baseUrl = BASE_URL,
    ) {
      this.model = model ?? DEFAULT_GEMINI_MODEL // (gemini.rs:52)
      this.baseUrl = baseUrl.replace(/\/+$/, '') // trim trailing slash(es), like OpenAIProvider (openai.ts:68)
      this.retryPolicy = RetryPolicy.default()
    }

    withRetryPolicy(policy: RetryPolicy): this {
      this.retryPolicy = policy
      return this
    }

    capabilities(): ProviderCapabilities {
      // Post-M4 withImage() returns { supportsImage:true, supportsDocument:false,
      // supportsMcp:false } (gemini.rs:316).
      return withImage()
    }

    private resolveModel(request: ChatRequest): string {
      return request.model ?? this.model // (gemini.rs:64,69)
    }

    private generateUrl(model: string): string {
      return `${this.baseUrl}/models/${model}:generateContent` // (gemini.rs:65)
    }

    private streamUrl(model: string): string {
      return `${this.baseUrl}/models/${model}:streamGenerateContent?alt=sse` // (gemini.rs:71-72)
    }

    private headers(): Record<string, string> {
      // content-type is injected by http/fetch.ts postJson/postStream (fetch.ts:31,55).
      return { 'x-goog-api-key': this.apiKey } // (gemini.rs:77)
    }

    async chat(request: ChatRequest): Promise<ChatResponse> {
      const model = this.resolveModel(request)
      const url = this.generateUrl(model)
      const body = serializeGeminiRequest(request, model)

      const payload = await withRetry(
        this.retryPolicy,
        async () => postJson<any>(url, this.headers(), body),
        classifyHttpError,
      )

      return this.parseResponse(payload, model)
    }

    private parseResponse(payload: any, defaultModel: string): ChatResponse {
      const candidate = payload?.candidates?.[0] ?? {}
      const parts: any[] = candidate?.content?.parts ?? []

      let content = ''
      const toolCalls: ToolCall[] = []

      for (const part of parts) {
        if (typeof part?.text === 'string') {
          content += part.text // (gemini.rs:257-259)
        }
        if (part?.functionCall) {
          const fc = part.functionCall
          toolCalls.push({
            id: genToolCallId(), // client-generated; no id on the wire (gemini.rs:268)
            name: typeof fc?.name === 'string' ? fc.name : '',
            input:
              fc?.args && typeof fc.args === 'object'
                ? (fc.args as Record<string, unknown>)
                : {}, // input from wire `args` (gemini.rs:266)
          })
        }
      }

      const finishReason =
        typeof candidate?.finishReason === 'string' ? candidate.finishReason : 'STOP' // (gemini.rs:275-278)
      const stopReason = mapFinishReason(finishReason, toolCalls.length > 0)

      const usageMeta = payload?.usageMetadata ?? {}
      const inputTokens = Number(usageMeta?.promptTokenCount ?? 0)
      const outputTokens = Number(usageMeta?.candidatesTokenCount ?? 0)

      const model =
        typeof payload?.modelVersion === 'string' ? payload.modelVersion : defaultModel // (gemini.rs:298-302)

      return {
        content,
        toolCalls,
        model,
        usage: { inputTokens, outputTokens },
        stopReason,
      }
    }

    stream(request: ChatRequest): BoxStream {
      return this.streamImpl(request)
    }

    private async *streamImpl(request: ChatRequest) {
      const model = this.resolveModel(request)
      const url = this.streamUrl(model)
      const body = serializeGeminiRequest(request, model)

      // Retry ONLY the initial postStream fetch (manual status-aware loop,
      // mirrors openai.ts:326-351). Once the body is obtained, parseSse drives
      // with NO mid-stream retry (gemini.rs:372-413).
      let attempt = 0
      let responseBody: ReadableStream<Uint8Array>
      while (true) {
        try {
          responseBody = await postStream(url, this.headers(), body)
          break
        } catch (error) {
          const status = (error as { status?: number }).status
          const retryable =
            (status !== undefined && isRetryableStatus(status)) ||
            isRetryableNetworkError(error)
          if (!retryable || attempt >= this.retryPolicy.maxRetries) {
            throw error
          }
          attempt += 1
          const retryAfterMs = (error as { retryAfterMs?: number }).retryAfterMs
          const delay = this.retryPolicy.respectRetryAfter
            ? retryAfterMs ?? this.retryPolicy.delayForAttempt(attempt)
            : this.retryPolicy.delayForAttempt(attempt)
          await new Promise((resolve) => setTimeout(resolve, delay))
        }
      }

      // Drive parseSse. Gemini sends NO [DONE]; each data: line is a full
      // GenerateContentResponse chunk. Termination is finishReason-driven (emit
      // doneWithStopReason) or stream EOF. NO defensive EOF done (gemini.rs:531;
      // contrast openai.ts:460-474). (sse.ts [DONE] is advisory and never
      // terminates — sse.ts:7-9,134-139.)
      for await (const evt of parseSse(responseBody)) {
        const data = evt.data

        // Defensive: skip a stray [DONE] or empty data (gemini.rs:447-449).
        if (data === '[DONE]' || !data) continue
        if (typeof data !== 'object') continue

        const candidate = data?.candidates?.[0]
        if (!candidate) continue // (gemini.rs:455-458)

        const parts: any[] = candidate?.content?.parts ?? [] // (gemini.rs:460-465)
        const finishReason =
          typeof candidate?.finishReason === 'string' ? candidate.finishReason : undefined

        let hasToolCalls = false

        // Chunk pending-queue order: text events, then tool-call triplets
        // (gemini.rs:471-494).
        for (const part of parts) {
          if (typeof part?.text === 'string' && part.text !== '') {
            yield textEvent(part.text) // (gemini.rs:472-476)
          }
          if (part?.functionCall) {
            hasToolCalls = true
            const fc = part.functionCall
            const name = typeof fc?.name === 'string' ? fc.name : ''
            const args = fc?.args && typeof fc.args === 'object' ? fc.args : {}
            const id = genToolCallId()
            // Synthesize three events; args serialized in ONE shot (gemini.rs:485,490).
            yield toolCallStart(id, name)
            yield toolCallArgsWithId(id, JSON.stringify(args))
            yield toolCallEndWithId(id)
          }
        }

        // usage AFTER tool triplets (gemini.rs:496-511).
        const usageMeta = data?.usageMetadata
        if (usageMeta) {
          yield usageEvent({
            inputTokens: Number(usageMeta?.promptTokenCount ?? 0),
            outputTokens: Number(usageMeta?.candidatesTokenCount ?? 0),
          })
        }

        // done LAST, only when finishReason present — the ONLY terminator
        // (gemini.rs:513-523).
        if (finishReason !== undefined) {
          yield doneWithStopReason(mapFinishReason(finishReason, hasToolCalls))
        }
      }
      // EOF: generator ends naturally. NO fabricated done (gemini.rs:531).
    }
  }
  ```
  Run and watch it PASS:
  ```
  npm run test -- tests/providers-gemini.test.ts
  ```
  Expected output: all GeminiProvider tests pass — capabilities (image/!document/!mcp), chat URL/auth/parse, finishReason mapping (end_turn/tool_use/max_tokens/other), unique ids, image inlineData, document rejection via validateRequest, SSE text+done, synthesized tool triplet (one args chunk), `[DONE]` skip + no fabricated EOF done, usage event, and 429 retry + 400 mapped error.

- [ ] **Step 3 (optional): env-gated live test.**
  Create `sdks/typescript/tests/gemini-live.test.ts`. Skips entirely unless `GEMINI_API_KEY` is set (mirrors `tests/integration.*`).
  ```ts
  import { describe, it, expect } from 'vitest'
  import { GeminiProvider } from '../src/providers/gemini.js'

  const KEY = process.env.GEMINI_API_KEY
  const live = KEY ? describe : describe.skip

  live('GeminiProvider (live)', () => {
    it('completes a simple chat', async () => {
      const provider = new GeminiProvider(KEY as string)
      const resp = await provider.chat({
        messages: [{ role: 'user', content: 'Reply with the single word: ok' }],
        maxTokens: 16,
      })
      expect(resp.content.length).toBeGreaterThan(0)
      expect(['end_turn', 'max_tokens', 'other']).toContain(resp.stopReason)
    }, 30_000)
  })
  ```
  Run (skipped without the key):
  ```
  npm run test -- tests/gemini-live.test.ts
  ```
  Expected output: `1 skipped` when `GEMINI_API_KEY` is unset; green when set.

- [ ] **Step 4: Build (TS strict gate).**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0. `streamImpl` is an `async *` generator returning `BoxStream`-compatible `AsyncIterable<StreamEvent>`; all stream ctor calls match `stream.ts` signatures; relative imports end in `.js`. `RetryPolicy.maxRetries`, `.respectRetryAfter`, `.delayForAttempt` match the `openai.ts` usage (already compiling on main).

- [ ] **Step 5: Run the full suite to confirm no regressions.**
  ```
  npm run test
  ```
  Expected output: all tests green (existing + the new gemini provider tests; live skipped).

- [ ] **Step 6: Commit (conventional).**
  ```
  git add src/providers/gemini.ts tests/providers-gemini.test.ts tests/gemini-live.test.ts
  git commit -m "feat(providers): add Gemini provider (generateContent + SSE stream)"
  ```

NOTE for downstream tasks: Task 5 imports `GeminiProvider` from `'./providers/gemini.js'` and constructs it as `new GeminiProvider(apiKey, this._model, this._geminiBaseUrl).withRetryPolicy(this._retryPolicy)`. Task 6 re-exports `GeminiProvider` from `index.ts`. This task's only public export is the `GeminiProvider` class.

---

### Task 4: provider.ts — add 'gemini' to the Provider union

Extends the `Provider` string-tagged discriminator in `provider.ts` to include `'gemini'`. Contract §12 / scope notes: the union comment already says "Extend with 'ollama' | 'gemini'". This is the single-line type change that unlocks Task 5's `buildProvider` gemini arm and `HTTP_PROVIDERS`/`ENV_KEY_BY_PROVIDER` extensions to type-check.

This task BUILDS ON M5, which already added `'ollama'` to the union. After M5 the line reads `export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama'`; M6 appends `| 'gemini'`. (The M4 change to `ProviderCapabilities` — adding `supportsMcp` — is owned by M4, NOT this task; do NOT touch `ProviderCapabilities`/`withImage`/`textOnly`/`fullCaps` here.)

STRICT OWNERSHIP: this task touches ONLY `src/provider.ts` (the union line) and, if a dedicated provider-union test exists, may extend it. It does NOT touch `client.ts`, `models.ts`, `serialize/`, `providers/`, or `index.ts`.

**Files:**
- `sdks/typescript/src/provider.ts` (MODIFY — one union line)
- `sdks/typescript/tests/capabilities.test.ts` (OPTIONAL — only if it already asserts the union membership; otherwise no test change is needed — the type-level extension is exercised by Task 5/Task 6 tests)

All commands run FROM `sdks/typescript/`.

- [ ] **Step 1: Extend the Provider union.**
  In `sdks/typescript/src/provider.ts`, the union after M5 is (around line 73-74):
  ```ts
  /** String-tagged provider discriminator. Extend with 'gemini' in M6. */
  export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama'
  ```
  Change it to add `'gemini'` and update the comment:
  ```ts
  /** String-tagged provider discriminator. */
  export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama' | 'gemini'
  ```
  (If M5 left the comment as "Extend with 'gemini' in M6", drop the now-completed instruction. Match the exact post-M5 text when editing; the only load-bearing change is appending `| 'gemini'` to the union.)

- [ ] **Step 2: Build (TS strict gate) — verify the union compiles standalone.**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0. NOTE: if Task 5 has NOT yet landed, `client.ts`'s `ENV_KEY_BY_PROVIDER: Record<ProviderName, string>` will now be MISSING the `gemini` key and `tsc` will error with "Property 'gemini' is missing in type ... but required in type 'Record<Provider, string>'". This is EXPECTED and CORRECT — it proves the union change has teeth. Sequence this task to land together with (or immediately before, in the same branch as) Task 5, OR if landing this task alone, temporarily verify with `tsc --noEmit` understanding that the `Record<Provider,...>` exhaustiveness gap is resolved by Task 5. The clean `npm run build` happens once Task 5's `ENV_KEY_BY_PROVIDER.gemini` is present.

  Recommended ordering for a clean per-commit gate: include the `provider.ts` union change in the SAME commit/branch as Task 5's `client.ts` changes, since `ENV_KEY_BY_PROVIDER` is typed `Record<ProviderName, string>` and the two are coupled by the type system. If the workflow requires strictly one-file-per-task commits, land Task 4 and Task 5 back-to-back and only assert the green `npm run build` after Task 5.

- [ ] **Step 3 (optional): assert the union value-side in an existing test.**
  If `tests/capabilities.test.ts` (or `tests/client-builder.test.ts`, owned by Task 5) iterates a provider list, ensure `'gemini'` is a valid `Provider` by a compile-time check. A minimal value-level assertion that does not require Task 5 (pure type acceptance):
  ```ts
  import { describe, it, expect } from 'vitest'
  import type { Provider } from '../src/provider.js'

  describe('Provider union', () => {
    it("accepts 'gemini'", () => {
      const p: Provider = 'gemini'
      expect(p).toBe('gemini')
    })
  })
  ```
  (Only add this if it does not duplicate a Task 5/Task 6 assertion — to respect non-overlapping ownership, prefer leaving union coverage to Task 6's smoke test and Task 5's builder test. This optional step exists only for the case where Task 4 must demonstrate independent value.)
  Run:
  ```
  npm run test -- tests/capabilities.test.ts
  ```
  Expected output: green.

- [ ] **Step 4: Commit (conventional).**
  ```
  git add src/provider.ts
  git commit -m "feat(provider): add 'gemini' to the Provider union"
  ```
  (If landing coupled with Task 5 for a clean build gate, this `git add` may be combined into Task 5's commit — coordinate at integration time; the OWNED change here is strictly the union line in `provider.ts`.)

NOTE for downstream tasks: Task 5 relies on `'gemini'` being a member of `Provider` so `HTTP_PROVIDERS` (typed `ReadonlySet<ProviderName>`) and `ENV_KEY_BY_PROVIDER` (typed `Record<ProviderName, string>`) can include it without a type error. Task 6's smoke test calls `.provider('gemini')`.

---

### Task 5: client.ts — ClientBuilder + buildProvider Gemini wiring

Wires `GeminiProvider` into `client.ts`: adds `'gemini'` to `HTTP_PROVIDERS` (so `build()` enforces an API key), adds `gemini: 'GEMINI_API_KEY'` to `ENV_KEY_BY_PROVIDER`, adds a `buildProvider` gemini arm that constructs `new GeminiProvider(apiKey, this._model, this._geminiBaseUrl).withRetryPolicy(this._retryPolicy)`, adds the `_geminiBaseUrl` field + `geminiBaseUrl(u)` setter (mirroring `anthropicBaseUrl`, client.ts:62,93-96), imports `GeminiProvider`, and adds the legacy options-object constructor gemini arm (matching how M5 wired ollama). Contract §12.

STRICT OWNERSHIP: this task touches ONLY `src/client.ts` and `tests/client-builder.test.ts`. It IMPORTS `GeminiProvider` (Task 3) and relies on `'gemini'` being in the `Provider` union (Task 4). It does NOT re-declare any of those. It does NOT touch `provider.ts`, `models.ts`, `serialize/`, `providers/`, or `index.ts`.

DEPENDS ON: Task 3 (`GeminiProvider`), Task 4 (`Provider` union += 'gemini'). Because `ENV_KEY_BY_PROVIDER` is typed `Record<ProviderName, string>`, this task's `gemini` key is REQUIRED for a clean `npm run build` once Task 4's union lands — land Task 4 + Task 5 together (or back-to-back in one branch).

**Files:**
- `sdks/typescript/src/client.ts` (MODIFY)
- `sdks/typescript/tests/client-builder.test.ts` (MODIFY — extend the builder describe + env reset)

All commands run FROM `sdks/typescript/`. On main (post-M5): `HTTP_PROVIDERS = new Set(['anthropic','openai','minimax'])` — **Ollama is NOT in it** (Ollama is keyless; `build()` and the legacy ctor special-case `provider !== 'ollama'`). `ENV_KEY_BY_PROVIDER` HAS an `ollama` entry, `buildProvider` HAS an `ollama` arm, and the legacy ctor HAS an `ollama` arm (OpenAI-compat via `.withChatUrl(...).withAuthStyle({kind:'bearer'})`). M6 EXTENDS these (adds `gemini`) without removing the `ollama` handling. Note the legacy ctor uses the local `resolvedApiKey` (not `apiKey`) and the minimax field is `opts.minimaxBaseUrl` (not `minimaxEndpoint`).

- [ ] **Step 1: Write the failing tests first (TDD).**
  Edit `sdks/typescript/tests/client-builder.test.ts`. Add `GEMINI_API_KEY` to the `beforeEach` env-reset block and add gemini assertions. The existing reset block (around lines 9-13) currently deletes ANTHROPIC/OPENAI/MINIMAX (and, post-M5, OLLAMA). Add the gemini deletion:
  ```ts
  beforeEach(() => {
    delete process.env.ANTHROPIC_API_KEY
    delete process.env.OPENAI_API_KEY
    delete process.env.MINIMAX_API_KEY
    delete process.env.OLLAMA_API_KEY
    delete process.env.GEMINI_API_KEY
  })
  ```
  Then add these tests inside the `describe('ClientBuilder', ...)` block:
  ```ts
  it('builds a Client for the gemini provider with an explicit key', () => {
    const client = new ClientBuilder().provider('gemini').apiKey('test-key').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('throws ConfigError when GEMINI_API_KEY is missing for gemini', () => {
    const builder = new ClientBuilder().provider('gemini')
    expect(() => builder.build()).toThrowError(ConfigError)
    expect(() => builder.build()).toThrowError('Missing API key for provider gemini')
  })

  it('uses GEMINI_API_KEY from the environment for gemini', () => {
    process.env.GEMINI_API_KEY = 'gemini-env-key'
    const client = new ClientBuilder().provider('gemini').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('supports a geminiBaseUrl override fluently', () => {
    const client = new ClientBuilder()
      .provider('gemini')
      .apiKey('test-key')
      .geminiBaseUrl('https://gemini.proxy.internal/v1beta')
      .retryPolicy(new RetryPolicy({ maxRetries: 1 }))
      .model('gemini-2.5-pro')
      .build()
    expect(client).toBeInstanceOf(Client)
  })
  ```
  If the existing `it('builds a Client for each HTTP provider', ...)` test iterates a provider tuple, extend that tuple to include `'gemini'`:
  ```ts
  it('builds a Client for each HTTP provider', () => {
    for (const p of ['anthropic', 'openai', 'minimax', 'ollama', 'gemini'] as const) {
      const client = new ClientBuilder().provider(p).apiKey('test-key').build()
      expect(client).toBeInstanceOf(Client)
    }
  })
  ```
  Run and watch it FAIL:
  ```
  npm run test -- tests/client-builder.test.ts
  ```
  Expected output: the gemini cases fail — `build()` for `'gemini'` currently falls through `buildProvider` to the minimax default (constructing the wrong provider) or `ENV_KEY_BY_PROVIDER['gemini']` is `undefined`, and there is no `geminiBaseUrl` method (runtime `TypeError: ...geminiBaseUrl is not a function`).

- [ ] **Step 2: Import GeminiProvider in client.ts.**
  Add the import alongside the existing provider imports (after the OpenAI import, client.ts:15):
  ```ts
  import { GeminiProvider } from './providers/gemini.js'
  ```

- [ ] **Step 3: Add 'gemini' to HTTP_PROVIDERS.**
  On main the set is `new Set(['anthropic', 'openai', 'minimax'])` — Ollama is INTENTIONALLY excluded (it is keyless; `build()` and the legacy ctor special-case `provider !== 'ollama'`). Add ONLY `'gemini'` (Gemini needs `x-goog-api-key`); do NOT add `'ollama'`:
  ```ts
  const HTTP_PROVIDERS: ReadonlySet<ProviderName> = new Set([
    'anthropic',
    'openai',
    'minimax',
    'gemini',
  ])
  ```

- [ ] **Step 4: Add the gemini env key.**
  Update `ENV_KEY_BY_PROVIDER` (client.ts:33-37, post-M5 it has an `ollama` key). Because the record is typed `Record<ProviderName, string>`, the `gemini` key is REQUIRED now that Task 4 added `'gemini'` to the union:
  ```ts
  const ENV_KEY_BY_PROVIDER: Record<ProviderName, string> = {
    anthropic: 'ANTHROPIC_API_KEY',
    openai: 'OPENAI_API_KEY',
    minimax: 'MINIMAX_API_KEY',
    ollama: 'OLLAMA_API_KEY',
    gemini: 'GEMINI_API_KEY',
  }
  ```
  (Match the exact ollama key name M5 chose for that line; only the `gemini` entry is OWNED by this task.)

- [ ] **Step 5: Add the _geminiBaseUrl field and geminiBaseUrl setter (mirror anthropicBaseUrl).**
  Add the protected field next to the other base-url fields (near client.ts:61-62):
  ```ts
  protected _geminiBaseUrl?: string
  ```
  Add the setter next to `anthropicBaseUrl` (after client.ts:93-96):
  ```ts
  geminiBaseUrl(u: string): this {
    this._geminiBaseUrl = u
    return this
  }
  ```

- [ ] **Step 6: Add the buildProvider gemini arm.**
  In `buildProvider` (client.ts:139-163), add a gemini arm BEFORE the trailing minimax default `return` (so the fall-through stays minimax). After the `openai` block and before the final `return new MinimaxProvider(...)`:
  ```ts
  if (provider === 'gemini') {
    return new GeminiProvider(apiKey, this._model, this._geminiBaseUrl).withRetryPolicy(
      this._retryPolicy,
    )
  }
  ```
  (If M5 added an `if (provider === 'ollama') { ... }` arm here, place the gemini arm adjacent to it; the relative order among the explicit arms is not load-bearing as long as each is matched before the minimax fall-through.)

- [ ] **Step 7: Add the legacy options-object constructor gemini arm (match M5's ollama choice).**
  The legacy `Client` constructor on main already has explicit arms for anthropic / openai / **ollama** / (minimax fall-through), using the local `resolvedApiKey` and `opts.minimaxBaseUrl`. INSERT a `gemini` arm before the minimax fall-through — KEEP the existing ollama arm intact, and match main's variable/field names exactly (use `resolvedApiKey`, NOT `apiKey`; `opts.minimaxBaseUrl`, NOT `opts.minimaxEndpoint`). The resulting tail is:
  ```ts
  if (provider === 'anthropic') {
    this.provider = new AnthropicProvider(resolvedApiKey, opts.model)
  } else if (provider === 'openai') {
    this.provider = new OpenAIProvider(resolvedApiKey, opts.model)
  } else if (provider === 'ollama') {
    // Legacy ctor has no tuning fields → OpenAI-compat path (empty key,
    // Bearer) against the default Ollama base URL. For native/tuning, use
    // Client.builder().
    this.provider = new OpenAIProvider(resolvedApiKey, opts.model ?? DEFAULT_OLLAMA_MODEL)
      .withChatUrl('http://localhost:11434/v1/chat/completions')
      .withAuthStyle({ kind: 'bearer' })
  } else if (provider === 'gemini') {
    this.provider = new GeminiProvider(resolvedApiKey, opts.model)
  } else {
    this.provider = new MinimaxProvider(resolvedApiKey, opts.model, opts.minimaxBaseUrl)
  }
  ```
  (Only the `gemini` branch is new; do NOT alter or remove the ollama branch. `DEFAULT_OLLAMA_MODEL` is already imported on main.)

- [ ] **Step 8: Build (TS strict gate).**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0. `ENV_KEY_BY_PROVIDER` now satisfies `Record<Provider, string>` exhaustiveness (resolving the gap Task 4's union introduced); `GeminiProvider` is structurally a `DispatchProvider` (has `capabilities`/`chat`/`stream`); `withRetryPolicy` returns `this`; `_geminiBaseUrl` is `string | undefined`, accepted by the `GeminiProvider(apiKey, model?, baseUrl?)` constructor's optional third param.

- [ ] **Step 9: Run the builder test suite — confirm green.**
  ```
  npm run test -- tests/client-builder.test.ts
  ```
  Expected output: all builder tests pass, including the four new gemini cases (explicit key build, missing-key ConfigError, env-key build, geminiBaseUrl fluent chain) and the extended HTTP-provider loop.

- [ ] **Step 10: Commit (conventional).**
  ```
  git add src/client.ts tests/client-builder.test.ts
  git commit -m "feat(client): wire Gemini provider into ClientBuilder and buildProvider"
  ```
  (If coordinating a clean per-commit build gate with Task 4, this commit may also include `src/provider.ts`; see Task 4 Step 2/4.)

NOTE for downstream tasks: Task 6's smoke test calls `Client.builder().provider('gemini').apiKey('k').build()`, which exercises this task's `buildProvider` gemini arm + `HTTP_PROVIDERS` gate.

---

### Task 6: index.ts exports + Done-criteria smoke test

Exposes the M6 public surface from `index.ts` and adds an end-to-end smoke test proving `Client.builder().provider('gemini').apiKey(...).build()` round-trips and that the new symbols are reachable from the package root. Mirrors the existing export surface (contract §13): re-export `GeminiProvider`, `DEFAULT_GEMINI_MODEL`, `GEMINI_MODELS`, and `serializeGeminiRequest`. This task is the integration capstone and verifies the full `npm run build && npm run test` gate.

STRICT OWNERSHIP: this task touches ONLY `src/index.ts` and `tests/index.test.ts` (extend) plus, if preferred, a small new `tests/gemini-smoke.test.ts`. It IMPORTS/re-exports symbols OWNED by Task 1 (`DEFAULT_GEMINI_MODEL`, `GEMINI_MODELS`), Task 2 (`serializeGeminiRequest`), and Task 3 (`GeminiProvider`). It re-declares NONE of them and modifies no other source file.

DEPENDS ON: Tasks 1, 2, 3, 4, 5 all merged (this is the LAST M6 task).

**Files:**
- `sdks/typescript/src/index.ts` (MODIFY — add re-exports)
- `sdks/typescript/tests/index.test.ts` (MODIFY — assert the new exports) OR `sdks/typescript/tests/gemini-smoke.test.ts` (CREATE — if you prefer to keep the smoke test isolated). Use ONE; the steps below extend `tests/index.test.ts`.

All commands run FROM `sdks/typescript/`.

- [ ] **Step 1: Write the failing smoke/export test first (TDD).**
  Edit `sdks/typescript/tests/index.test.ts`. Add a describe block that imports the M6 surface from the package root (`../src/index.js`) and round-trips a builder. (The existing `index.test.ts` already imports default-model constants from the root; follow its import style.)
  ```ts
  import { describe, it, expect, afterEach, vi } from 'vitest'
  import {
    Client,
    GeminiProvider,
    DEFAULT_GEMINI_MODEL,
    GEMINI_MODELS,
    serializeGeminiRequest,
  } from '../src/index.js'

  describe('M6 Gemini public surface (index re-exports)', () => {
    it('re-exports DEFAULT_GEMINI_MODEL and GEMINI_MODELS from the root', () => {
      expect(DEFAULT_GEMINI_MODEL).toBe('gemini-2.5-flash')
      expect(GEMINI_MODELS).toContain('gemini-2.5-flash')
      expect(GEMINI_MODELS.length).toBe(8)
    })

    it('re-exports the GeminiProvider class from the root', () => {
      const provider = new GeminiProvider('test-key')
      const caps = provider.capabilities()
      expect(caps.supportsImage).toBe(true)
      expect(caps.supportsDocument).toBe(false)
      expect(caps.supportsMcp).toBe(false)
    })

    it('re-exports serializeGeminiRequest from the root', () => {
      const body = serializeGeminiRequest(
        { messages: [{ role: 'user', content: 'Hello' }] },
        DEFAULT_GEMINI_MODEL,
      )
      const contents = (body as any).contents
      expect(contents[0].role).toBe('user')
      expect(contents[0].parts[0].text).toBe('Hello')
      expect((body as any).model).toBeUndefined()
    })
  })

  describe('M6 Gemini done-criteria smoke (builder round-trip)', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
      delete process.env.GEMINI_API_KEY
    })

    it('Client.builder().provider("gemini").apiKey(...).build() returns a Client', () => {
      const client = Client.builder().provider('gemini').apiKey('smoke-key').build()
      expect(client).toBeInstanceOf(Client)
    })

    it('a built gemini Client performs a chat through a mocked fetch (end-to-end)', async () => {
      const mockFetch = vi.fn(async (url: string) => {
        expect(url).toContain('/models/gemini-2.5-flash:generateContent')
        return new Response(
          JSON.stringify({
            candidates: [
              { content: { parts: [{ text: 'pong' }], role: 'model' }, finishReason: 'STOP' },
            ],
            usageMetadata: { promptTokenCount: 1, candidatesTokenCount: 1 },
            modelVersion: 'gemini-2.5-flash',
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const client = Client.builder().provider('gemini').apiKey('smoke-key').build()
      const resp = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
      expect(resp.content).toBe('pong')
      expect(resp.stopReason).toBe('end_turn')
      expect(mockFetch).toHaveBeenCalledTimes(1)
    })
  })
  ```
  Run and watch it FAIL (root re-exports not present yet):
  ```
  npm run test -- tests/index.test.ts
  ```
  Expected output: import resolution fails for `GeminiProvider` / `DEFAULT_GEMINI_MODEL` / `GEMINI_MODELS` / `serializeGeminiRequest` (`No "..." export is defined`), so the new describe blocks error out.

- [ ] **Step 2: Add the re-exports to index.ts.**
  Edit `sdks/typescript/src/index.ts`. Add the provider re-export alongside the other `providers/*` re-exports (after the openai/minimax lines, index.ts:6-8):
  ```ts
  export * from './providers/gemini.js'
  ```
  (`providers/gemini.ts` exports only `GeminiProvider`, so `export *` is clean and mirrors the existing anthropic/openai/minimax lines.)

  Add the model constants to the existing `models.js` re-export block (index.ts:12-19). Change:
  ```ts
  export {
    DEFAULT_ANTHROPIC_MODEL,
    DEFAULT_OPENAI_MODEL,
    DEFAULT_MINIMAX_MODEL,
    ANTHROPIC_MODELS,
    OPENAI_MODELS,
    MINIMAX_MODELS,
  } from './models.js'
  ```
  to (append the gemini constants; keep any ollama entries M5 may have added):
  ```ts
  export {
    DEFAULT_ANTHROPIC_MODEL,
    DEFAULT_OPENAI_MODEL,
    DEFAULT_MINIMAX_MODEL,
    DEFAULT_GEMINI_MODEL,
    ANTHROPIC_MODELS,
    OPENAI_MODELS,
    MINIMAX_MODELS,
    GEMINI_MODELS,
  } from './models.js'
  ```

  Add the serializer re-export. `index.ts` on main does not re-export `serializeOpenAiRequest`/`serializeAnthropicRequest` by default — confirm the existing surface. If serializers are NOT part of the public surface, expose `serializeGeminiRequest` explicitly so the test resolves it (this is the contract's listed surface — "export ... serializeGeminiRequest ... — match existing export surface"). Add after the models block:
  ```ts
  export { serializeGeminiRequest } from './serialize/gemini.js'
  ```
  (If the project already re-exports the other serializers via a `serialize/index.js` barrel, add `serializeGeminiRequest` there instead and re-export from `index.ts` to match the existing pattern — but do NOT modify Task 2's `serialize/gemini.ts`; only re-export it.)

- [ ] **Step 3: Run the smoke/export test — confirm green.**
  ```
  npm run test -- tests/index.test.ts
  ```
  Expected output: all M6 surface + smoke assertions pass — root re-exports resolve, `GeminiProvider` caps are image/!document/!mcp, `serializeGeminiRequest` round-trips, the builder returns a `Client`, and the mocked end-to-end chat returns `pong` with `stopReason: 'end_turn'` hitting the `:generateContent` URL.

- [ ] **Step 4: Build (TS strict gate).**
  ```
  npm run build
  ```
  Expected output: `tsc` exits 0. Re-exports are pure (no new logic); `export * from './providers/gemini.js'` adds `GeminiProvider` to the package types; the named `models.js`/`serialize/gemini.js` re-exports resolve. NodeNext `.js` extensions are present on every relative specifier.

- [ ] **Step 5: Run the FULL suite — the M6 done-criteria gate.**
  ```
  npm run build && npm run test
  ```
  Expected output: a clean build followed by a fully green test run across the whole SDK — Task 1 (models), Task 2 (serialize.gemini), Task 3 (providers-gemini, live skipped without `GEMINI_API_KEY`), Task 5 (client-builder gemini cases), and this task's index/smoke tests — with NO regressions in the pre-existing anthropic/openai/minimax/ollama suites.

- [ ] **Step 6: Commit (conventional).**
  ```
  git add src/index.ts tests/index.test.ts
  git commit -m "feat(index): export Gemini public surface and add done-criteria smoke test"
  ```

DONE CRITERIA (M6 complete): `npm run build && npm run test` is green from `sdks/typescript/`; `Client.builder().provider('gemini').apiKey(k).build().chat(...)` round-trips against a mocked fetch hitting `https://generativelanguage.googleapis.com/v1beta/models/<model>:generateContent` with the `x-goog-api-key` header; `DEFAULT_GEMINI_MODEL`, `GEMINI_MODELS`, `GeminiProvider`, and `serializeGeminiRequest` are importable from the package root.

---

## Milestone Done Criteria (verify all before tagging v0.9.0)

- [ ] Gemini `chat()` request-body test asserts `contents[].role` uses `model` (not `assistant`), `inlineData.mimeType`+`data` for base64 images, `fileData.fileUri` for url images, `functionResponse` for tool results, `systemInstruction` separate from `contents`, and `toolConfig.functionCallingConfig.mode` maps Auto/Required(ANY)/None correctly.
- [ ] Gemini SSE streaming test parses full-JSON-per-`data` chunks (no `[DONE]`), emits synthesized tool-call sequences (client-generated `call_N`), `usageEvent`, and done with `finishReason` `STOP`→`end_turn` / `MAX_TOKENS`→`max_tokens`.
- [ ] Image content block works (`inlineData`/`fileData`); document block rejected with `UnsupportedFeatureError` (caps `supportsImage:true, supportsDocument:false, supportsMcp:false`).
- [ ] `ClientBuilder.provider('gemini').apiKey(...).build()` works; `build()` requires `GEMINI_API_KEY` (gemini in HTTP_PROVIDERS); `geminiBaseUrl` override honored; auth header is `x-goog-api-key`.
- [ ] `index.ts` exports `GeminiProvider` + `DEFAULT_GEMINI_MODEL` (+ `GEMINI_MODELS`); env-gated live Gemini test passes for chat + stream + tool use; `npm run build` + `npm run test` green.

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet accompanies this plan):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks (superpowers:subagent-driven-development).
2. **Inline:** execute tasks in-session with checkpoints (superpowers:executing-plans).

# Milestone 5 — Ollama Provider (native /api/chat + OpenAI-compat + auto-routing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the Ollama provider with both the native `/api/chat` NDJSON path (`think`/`keep_alive`/`num_ctx`, thinking extraction, 3-event tool-call streaming) and the OpenAI-compatible path, plus dispatch auto-routing (native when `ollamaNative` or any tuning field is set, else OpenAI-compat) and builder tuning-field validation — shipping v0.8.0.

**Architecture:** Builds on merged M1–M4 (on `main`). A net-new `providers/ollama.ts` holds both wire paths; `models.ts` gains `DEFAULT_OLLAMA_MODEL`; the `Provider` union gains `'ollama'`; `ClientBuilder` gains the ollama tuning setters + auto-routing + validation; `http/ndjson.ts` (built in M1) is verified/hardened for the Ollama adapter. Ollama needs no API key (the reserved optional-api-key seam).

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, raw `fetch`. Reference: Rust `sdks/rust/src/providers/ollama.rs` + `mod.rs` + `client.rs`.

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M5). **Depends on:** M1 (#185) + M2 (#186) + M3 (#187) + **M4** (MiniMax/MCP — adds `supportsMcp` to `ProviderCapabilities` and `minimaxBaseUrl`). **Branch M5 off `main` AFTER M4 merges.**

---

## Conventions (apply to EVERY task — override anything ambiguous in a task body)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. Package: `sdks/typescript/`. **Commands run from `sdks/typescript/`**. Paths repo-relative.
- **Workflow:** feature branch, land via **PR + CI**. Commit after each task. (From a git worktree the pre-push hook can't run Rust — verify `npm run build` + `npm run test` locally and `git push --no-verify`; CI runs the full gate.)
- **Module system:** strict + NodeNext. Every relative import ends in `.js`.
- **Layout:** source in `src/`, tests in `tests/` (NOT tsc-checked — run by vitest). Mock fetch via `vi.stubGlobal`. Live tests env-gated (`OLLAMA_BASE_URL` / a local instance). **No `npm run format` script** (gate = `npm run build` + `npm run test`).
- **Trust the plan:** a symbol that looks "missing" from a file is usually the pre-M5 state this plan adds — the code is here verbatim.

## Built on M1–M4 (import, never redefine)

`http/ndjson.ts` (working `parseNdjson` from M1); `http/fetch.ts` (postJson, postStream); `http/sse.ts` (parseSse); `stream.ts` (BoxStream + ctors + collectStream); `serialize/openai.ts` (serializeOpenAiRequest — the OpenAI-compat path reuses it); `provider.ts` (Provider union, ProviderImpl, dispatchChat/dispatchStream, readTimeoutStream, ProviderCapabilities **{supportsImage, supportsDocument, supportsMcp}** after M4, textOnly/withImage/fullCaps, validateRequest); `providers/openai.ts` (classifyHttpError + status-aware retry + withRetryPolicy setter — **mirror these**); `client.ts` (ClientBuilder + buildProvider + HTTP_PROVIDERS api-key gate); `models.ts` (DEFAULT_*_MODEL); `error.ts` (mapHttpError WITH .status, isRetryableStatus/Network, UnsupportedFeatureError); `retry.ts` (RetryPolicy, withRetry).

## Canonical homes & cross-task contract

| Symbol(s) | Home | Owner |
|---|---|---|
| `DEFAULT_OLLAMA_MODEL` (= `'llama3.2'`) | `src/models.ts` | **T1** |
| `parseNdjson` (verify/harden for `done:true` adapter use) | `src/http/ndjson.ts` | **T2** |
| `OllamaProvider` (native `/api/chat` NDJSON + OpenAI-compat, capabilities incl `supportsMcp:false`, `withRetryPolicy`, `classifyHttpError`, tuning fields) | `src/providers/ollama.ts` | **T3** |
| `'ollama'` added to the `Provider` union | `src/provider.ts` | **T4** |
| ClientBuilder `ollamaNative`/`ollamaThink`/`ollamaKeepAlive`/`ollamaNumCtx`/`ollamaBaseUrl` setters + buildProvider ollama arm (**auto-routing**) + `build()` validation + api-key-not-required for ollama | `src/client.ts` | **T5** |
| `OllamaProvider`/`DEFAULT_OLLAMA_MODEL` exports + smoke | `src/index.ts` | **T6** |

**Binding rules:**
- **Auto-routing (T5 buildProvider):** native when `ollamaNative` is true OR any of `ollamaThink`/`ollamaKeepAlive`/`ollamaNumCtx` is set; else OpenAI-compat. The SAME decision drives both chat and stream (the provider is constructed once with a `native` flag). The `Provider` union (T4) only adds the `'ollama'` token; the routing logic lives in T5, not the dispatch layer.
- **Native wire (T3):** flat `{role, content}` messages (NOT content blocks); `think` coercion (`true/yes/on/1`→`true`, `false/no/off/0`→`false`, else verbatim trimmed string, omit when blank); `keep_alive` verbatim; `num_ctx` inside `options`; assistant `tool_calls` use `function.arguments` as the **parsed object** (not a JSON string), no id/type; `tool` messages carry no `tool_call_id`. NDJSON terminates on `done:true` (the adapter decides, emitting a plain `doneEvent()`). Tool calls get generated `call_N` ids when absent.
- **Capabilities:** Ollama `capabilities()` includes `supportsMcp:false` (M4 made it a required field). Confirm image support per the contract (native text-only vs compat).
- **No API key:** Ollama is NOT in `HTTP_PROVIDERS`; `build()` must not require a key for it.
- **Validation (T5):** setting `ollamaThink`/`ollamaKeepAlive`/`ollamaNumCtx` on a non-ollama provider throws `ConfigError` at `build()` naming the field.
- **Retry:** mirror OpenAI — `withRetryPolicy` setter, status-aware `classifyHttpError`, chat retries whole call, stream retries initial fetch only.

**Dependency order:** 1 models → 2 ndjson → 3 ollama provider → 4 Provider union → 5 builder routing+validation → 6 exports+smoke. (T3 imports T1+T2; T5 imports T3 + the T4 union token; T6 exports T3.)

---

### Task 1: Add DEFAULT_OLLAMA_MODEL to models.ts

Adds the single Ollama default-model constant the rest of M5 imports. Per the
contract (§1) there is NO `OLLAMA_MODELS` array in Rust `models.rs` (only
ANTHROPIC/OPENAI/MINIMAX/GEMINI lists exist), so this task adds ONLY the
default constant — do NOT invent an array with no Rust source.

**Owns:** `src/models.ts` (the new `DEFAULT_OLLAMA_MODEL` symbol) + the new
`models.test.ts` cases for it. Touches NOTHING else. Every other task IMPORTS
`DEFAULT_OLLAMA_MODEL` from `../models.js`; this task is the sole declarer.

**Files:**
- `sdks/typescript/src/models.ts` (MODIFY — append one constant)
- `sdks/typescript/tests/models.test.ts` (MODIFY — extend)

Existing `models.ts` ends at the MiniMax block (verified, lines 24-28):

```ts
/** MiniMax model IDs (models.rs:20) */
export const MINIMAX_MODELS = ['MiniMax-M2.7', 'MiniMax-M2.7-highspeed'] as const

/** Default MiniMax model (models.rs convention, first element) */
export const DEFAULT_MINIMAX_MODEL = 'MiniMax-M2.7'
```

Rust source of truth (`sdks/rust/src/models.rs:3`):
`pub const DEFAULT_OLLAMA_MODEL: &str = "llama3.2";`

---

#### Steps

- [ ] **Step 1: Write the failing test first.**
  Open `sdks/typescript/tests/models.test.ts`. It already imports the other
  defaults from `../src/models.js` and from `../src/index.js`. Add
  `DEFAULT_OLLAMA_MODEL` to the `../src/models.js` import block (lines 2-9):

  ```ts
  import {
    ANTHROPIC_MODELS,
    DEFAULT_ANTHROPIC_MODEL,
    OPENAI_MODELS,
    DEFAULT_OPENAI_MODEL,
    MINIMAX_MODELS,
    DEFAULT_MINIMAX_MODEL,
    DEFAULT_OLLAMA_MODEL,
  } from '../src/models.js'
  ```

  Then add a new `describe` block. Place it right after the existing
  `describe('DEFAULT_MINIMAX_MODEL', ...)` block (after its closing `})` near
  line 103), BEFORE `describe('cross-provider consistency', ...)`:

  ```ts
  describe('DEFAULT_OLLAMA_MODEL', () => {
    it('should be a non-empty string', () => {
      expect(typeof DEFAULT_OLLAMA_MODEL).toBe('string')
      expect(DEFAULT_OLLAMA_MODEL.length).toBeGreaterThan(0)
    })

    it('should match the value from models.rs:3', () => {
      expect(DEFAULT_OLLAMA_MODEL).toBe('llama3.2')
    })

    it('has no OLLAMA_MODELS array (no Rust source — contract §1)', async () => {
      const mod = await import('../src/models.js')
      expect((mod as Record<string, unknown>).OLLAMA_MODELS).toBeUndefined()
    })
  })
  ```

  Run it and watch it FAIL (constant not yet exported):

  ```
  npm run test -- tests/models.test.ts
  ```
  Expected: a failure referencing `DEFAULT_OLLAMA_MODEL` being `undefined`
  (the `.toBe('llama3.2')` assertion fails, or the import is `undefined`).

- [ ] **Step 2: Add the constant to make the test pass.**
  In `sdks/typescript/src/models.ts`, append after the `DEFAULT_MINIMAX_MODEL`
  line (mirror the existing one-line-comment style exactly):

  ```ts

  /** Default Ollama model (models.rs:3) */
  export const DEFAULT_OLLAMA_MODEL = 'llama3.2'
  ```

  Do NOT add an `OLLAMA_MODELS` array (contract §1: no Rust source).

  Re-run:

  ```
  npm run test -- tests/models.test.ts
  ```
  Expected: all `DEFAULT_OLLAMA_MODEL` cases PASS (green), including the
  no-array guard.

- [ ] **Step 3: Build (typecheck) — confirms the export compiles under strict.**

  ```
  npm run build
  ```
  Expected: exit 0, no TS errors. (`models.ts` has no imports, so this is a
  pure typecheck of the new `const`.)

  NOTE: The index-export of `DEFAULT_OLLAMA_MODEL` (so `import { ... } from
  '@motosan-ai/sdk'` works) is OWNED BY TASK 6, not this task. The
  `../src/index.js` re-export test for it lives in Task 6's smoke test. Do NOT
  edit `index.ts` here.

- [ ] **Step 4: Commit.**

  ```
  git add sdks/typescript/src/models.ts sdks/typescript/tests/models.test.ts
  git commit -m "feat(ts): add DEFAULT_OLLAMA_MODEL constant"
  ```

---

### Task 2: Verify + harden http/ndjson.ts for the Ollama adapter

The contract (scope notes) is explicit: `parseNdjson` on main is **WORKING and
SUFFICIENT AS-IS** for Ollama. It yields each parsed JSON object and skips
malformed lines; it MUST NOT special-case `done:true` — termination on
`{done:true}` is the Ollama stream ADAPTER's job (Task 3), mirroring Rust where
`NdjsonStream` only splits lines and `OllamaStreamAdapter` decides done. **This
task is a VERIFICATION + targeted hardening + tests — NOT a rewrite.** The header
comment's "finalized in M5" is satisfied by confirming behavior and adding the
Ollama-shaped regression tests, plus a one-line comment refresh. **Do not change
the parsing logic.**

**Owns:** `src/http/ndjson.ts` (the `parseNdjson` generator) + the
`tests/http.ndjson.test.ts` cases. No other file. Task 3 IMPORTS `parseNdjson`
from `../http/ndjson.js` and never modifies it.

**Files:**
- `sdks/typescript/src/http/ndjson.ts` (VERIFY; comment-only refresh, NO logic change)
- `sdks/typescript/tests/http.ndjson.test.ts` (MODIFY — add Ollama-shaped cases)

Current `parseNdjson` (verified, `src/http/ndjson.ts:18-65`) already:
1. Decodes UTF-8 across chunk boundaries via `TextDecoder({stream:true})`.
2. Buffers incomplete lines; splits on `\n`; `JSON.parse`es each non-empty line.
3. Silently skips malformed lines (`parseJsonLine` returns `undefined`).
4. Flushes a trailing line with no final newline on `done`.

This matches Rust `NdjsonStream` (`ollama.rs:500-551`): split on newline, trim,
skip empty, flush trailing buffer on EOF — and crucially it does NOT inspect
`done`. So no rewrite is needed. We only PROVE it handles Ollama transcripts.

---

#### Steps

- [ ] **Step 1: Read the parser and confirm the invariants (no edit yet).**
  Open `sdks/typescript/src/http/ndjson.ts`. Confirm by reading:
  - It yields raw parsed objects (no `done` inspection anywhere — grep yields
    nothing).
  - Malformed lines → `undefined` → skipped (lines 56-59, 71-80).
  - Trailing no-newline line flushed at EOF (lines 33-43).

  ```
  grep -n "done" sdks/typescript/src/http/ndjson.ts
  ```
  Expected: matches ONLY `const { done, value } = await reader.read()` and the
  `if (done)` EOF-flush branch — NEVER a `payload.done` / `obj.done` inspection.
  This is the load-bearing fact: the parser is provider-agnostic.

- [ ] **Step 2: Add Ollama-shaped regression tests FIRST (they should pass on
  the unchanged parser — proving sufficiency).**
  In `sdks/typescript/tests/http.ndjson.test.ts`, the existing file imports
  `parseNdjson` and uses the `ReadableStream` + `TextEncoder` pattern (verified
  lines 1-3). Add these cases inside the existing `describe('parseNdjson', ...)`
  block, after the last `it(...)` (after line 146):

  ```ts
  it('yields Ollama /api/chat objects verbatim including the done:true terminator', async () => {
    // A representative Ollama native NDJSON transcript: text deltas, then a
    // final {done:true} stats object. The PARSER must yield ALL of them,
    // including the done:true object — termination is the ADAPTER's job.
    const input =
      '{"model":"llama3.2","message":{"role":"assistant","content":"Hel"},"done":false}\n' +
      '{"model":"llama3.2","message":{"role":"assistant","content":"lo"},"done":false}\n' +
      '{"model":"llama3.2","message":{"role":"assistant","content":""},"done":true,"prompt_eval_count":9,"eval_count":4}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      },
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0].message.content).toBe('Hel')
    expect(objects[1].message.content).toBe('lo')
    // The parser MUST NOT swallow / stop on done:true — it just yields it.
    expect(objects[2].done).toBe(true)
    expect(objects[2].prompt_eval_count).toBe(9)
    expect(objects[2].eval_count).toBe(4)
  })

  it('splits an Ollama transcript delivered as one byte chunk per object', async () => {
    // Ollama streams one JSON object per flush; assert buffering across
    // chunk boundaries that land mid-object.
    const lines = [
      '{"message":{"content":"a"},"done":false}\n',
      '{"message":{"content":"b"},"done":false}\n',
      '{"done":true}\n',
    ]
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const line of lines) {
          // split each line in half to force mid-line buffering
          const mid = Math.floor(line.length / 2)
          controller.enqueue(new TextEncoder().encode(line.slice(0, mid)))
          controller.enqueue(new TextEncoder().encode(line.slice(mid)))
        }
        controller.close()
      },
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects.map((o) => o.message?.content ?? null)).toEqual(['a', 'b', null])
    expect(objects[2].done).toBe(true)
  })

  it('flushes a final Ollama object with no trailing newline (EOF without done)', async () => {
    // Mirrors Rust NdjsonStream flushing the trailing buffer line at EOF;
    // here the stream ends WITHOUT a done:true object and no final newline.
    const input = '{"message":{"content":"partial"},"done":false}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      },
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0].message.content).toBe('partial')
    expect(objects[0].done).toBe(false)
  })

  it('skips a malformed Ollama line and keeps the surrounding valid objects', async () => {
    // Transport hiccup mid-stream: a truncated/garbage line is dropped, the
    // valid objects on either side survive (M3 mid-stream-resilience contract).
    const input =
      '{"message":{"content":"ok1"},"done":false}\n' +
      '{"message":{"content":"ok2",broken}\n' +
      '{"done":true}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      },
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0].message.content).toBe('ok1')
    expect(objects[1].done).toBe(true)
  })
  ```

  Run:

  ```
  npm run test -- tests/http.ndjson.test.ts
  ```
  Expected: ALL cases PASS on the UNCHANGED parser. This is the verification
  result that justifies "finalized in M5" — the parser already satisfies the
  Ollama adapter's needs.

- [ ] **Step 3: Comment-only refresh (NO logic change).**
  The current header (lines 1-10) says "Basic implementation for M1; finalized
  in M5." Replace ONLY that closing sentence to record the verification
  outcome. In `sdks/typescript/src/http/ndjson.ts`, change line 9:

  From:
  ```ts
   * Basic implementation for M1; finalized in M5.
  ```
  To:
  ```ts
   * Finalized in M5: verified sufficient for Ollama native /api/chat NDJSON.
   * The parser is provider-agnostic — it yields each parsed object (including
   * any {done:true} object) and never inspects `done`. Termination on
   * {done:true} is the Ollama stream adapter's responsibility (providers/ollama.ts),
   * mirroring Rust where NdjsonStream only splits lines (ollama.rs:500-551) and
   * OllamaStreamAdapter decides done (ollama.rs:441-447).
  ```

  Do NOT touch any line inside `parseNdjson` or `parseJsonLine`.

- [ ] **Step 4: Build + full ndjson test pass.**

  ```
  npm run build
  npm run test -- tests/http.ndjson.test.ts
  ```
  Expected: build exit 0; all ndjson tests (original 7 + new 4) green.

- [ ] **Step 5: Commit.**

  ```
  git add sdks/typescript/src/http/ndjson.ts sdks/typescript/tests/http.ndjson.test.ts
  git commit -m "test(ts): verify parseNdjson handles Ollama NDJSON; finalize M5 doc"
  ```

---

### Task 3: providers/ollama.ts — native /api/chat NDJSON provider (net-new)

The big net-new file. `OllamaProvider` implements the native `/api/chat` path
(chat NDJSON-as-single-object + stream NDJSON adapter with the 3-event tool-call
pattern), `capabilities()` (`textOnly()`), `with*` setters, and the same
status-aware retry pattern as `OpenAIProvider`. The OpenAI-compat path is NOT a
new class — it reuses `OpenAIProvider` and is wired in Task 5's `buildProvider`,
so this file only contains the NATIVE provider.

**Owns:** `src/providers/ollama.ts` (the new `OllamaProvider` class) +
`tests/providers-ollama.test.ts`. Touches NOTHING else. Imports
`DEFAULT_OLLAMA_MODEL` from `../models.js` (Task 1), `parseNdjson` from
`../http/ndjson.js` (Task 2), and existing `stream.ts` / `error.ts` / `retry.ts`
/ `http/fetch.ts` / `provider.ts` symbols. Does NOT touch the `Provider` union
(Task 4), `client.ts` (Task 5), or `index.ts` (Task 6).

**Files:**
- `sdks/typescript/src/providers/ollama.ts` (NEW)
- `sdks/typescript/tests/providers-ollama.test.ts` (NEW)

Grounding (read before writing):
- Rust native wire: `sdks/rust/src/providers/ollama.rs` (entire file is the spec).
- Retry/classify pattern to mirror: `src/providers/openai.ts:38-51` (classify),
  `:253-273` (chat whole-call retry), `:326-351` (stream initial-fetch-only retry).
- Ctors used: `textEvent`, `doneEvent`, `toolCallStart`, `toolCallArgsWithId`,
  `toolCallEndWithId` from `src/stream.ts`.
- `postJson` / `postStream` from `src/http/fetch.ts` (they inject `content-type`).
- Ollama sends NO auth header — headers = `{}`.

---

#### Steps

- [ ] **Step 1: Write the failing test file FIRST (TDD).**
  Create `sdks/typescript/tests/providers-ollama.test.ts`. Mirror the
  `providers-openai.test.ts` mock-fetch + `vi.stubGlobal` style. NDJSON
  responses are returned as a plain string body (postStream reads `response.body`
  as a `ReadableStream`; `new Response(string)` provides one).

  ```ts
  import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
  import { OllamaProvider } from '../src/providers/ollama.js'
  import { DEFAULT_OLLAMA_MODEL } from '../src/models.js'
  import { RetryPolicy } from '../src/retry.js'
  import type { ChatRequest, StreamEvent } from '../src/types.js'

  const BASE = 'http://localhost:11434'

  describe('OllamaProvider chat (native /api/chat)', () => {
    let captured: { url: string; headers: Record<string, string>; body: any } | null = null

    beforeEach(() => {
      captured = null
    })
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    function stubOk(payload: unknown) {
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string, options?: RequestInit) => {
          captured = {
            url,
            headers: (options?.headers as Record<string, string>) ?? {},
            body: options?.body ? JSON.parse(String(options.body)) : null,
          }
          return new Response(JSON.stringify(payload), {
            status: 200,
            headers: { 'content-type': 'application/json' },
          })
        }),
      )
    }

    it('posts to {base}/api/chat with no auth header and flat messages', async () => {
      stubOk({
        model: 'llama3.2',
        message: { role: 'assistant', content: 'Hello!' },
        done: true,
        prompt_eval_count: 9,
        eval_count: 3,
      })
      const provider = new OllamaProvider('llama3.2', BASE)
      const req: ChatRequest = {
        system: '  you are helpful  ',
        messages: [{ role: 'user', content: 'hi' }],
      }
      const res = await provider.chat(req)

      expect(captured?.url).toBe('http://localhost:11434/api/chat')
      // no auth header on the native path
      expect(captured?.headers['authorization']).toBeUndefined()
      expect(captured?.headers['x-api-key']).toBeUndefined()
      expect(captured?.body.model).toBe('llama3.2')
      expect(captured?.body.stream).toBe(false)
      // system trimmed + first; flat {role,content}
      expect(captured?.body.messages).toEqual([
        { role: 'system', content: 'you are helpful' },
        { role: 'user', content: 'hi' },
      ])
      expect(res.content).toBe('Hello!')
      expect(res.model).toBe('llama3.2')
      expect(res.stopReason).toBe('stop')
      expect(res.usage.inputTokens).toBe(9)
      expect(res.usage.outputTokens).toBe(3)
      expect(res.thinking).toBeUndefined()
    })

    it('trims trailing slashes off the base URL', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'x' }, done: true })
      const provider = new OllamaProvider('llama3.2', 'http://localhost:11434///')
      await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(captured?.url).toBe('http://localhost:11434/api/chat')
    })

    it('serializes assistant tool_calls with arguments as a parsed OBJECT (not a JSON string)', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      const req: ChatRequest = {
        messages: [
          { role: 'user', content: 'weather?' },
          {
            role: 'assistant',
            content: '',
            toolCalls: [{ id: 'x', name: 'get_weather', input: { city: 'NYC' } }],
          },
          { role: 'tool', content: '{"temp":70}', toolCallId: 'x' },
        ],
      }
      await provider.chat(req)
      const msgs = captured?.body.messages
      const assistant = msgs.find((m: any) => m.role === 'assistant')
      // arguments is the parsed object directly — contrast OpenAI's JSON string
      expect(assistant.tool_calls).toEqual([
        { function: { name: 'get_weather', arguments: { city: 'NYC' } } },
      ])
      // assistant tool_calls have NO top-level id/type on the native path
      expect(assistant.tool_calls[0].id).toBeUndefined()
      expect(assistant.tool_calls[0].type).toBeUndefined()
      // tool message has NO tool_call_id on the native path
      const tool = msgs.find((m: any) => m.role === 'tool')
      expect(tool).toEqual({ role: 'tool', content: '{"temp":70}' })
    })

    it('puts num_ctx inside options and keep_alive at root; tools only when non-empty', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
        .withNumCtx(4096)
        .withKeepAlive('5m')
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        temperature: 0.5,
        stopSequences: ['STOP'],
        tools: [{ name: 't', description: 'd', inputSchema: { type: 'object', properties: {} } }],
      }
      await provider.chat(req)
      expect(captured?.body.keep_alive).toBe('5m')
      expect(captured?.body.options).toEqual({
        temperature: 0.5,
        num_ctx: 4096,
        stop: ['STOP'],
      })
      expect(captured?.body.tools).toEqual([
        {
          type: 'function',
          function: { name: 't', description: 'd', parameters: { type: 'object', properties: {} } },
        },
      ])
    })

    it('omits the options object entirely when empty', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(captured?.body.options).toBeUndefined()
      expect(captured?.body.keep_alive).toBeUndefined()
      expect(captured?.body.tools).toBeUndefined()
    })

    it('folds thinking into <think> wrapper when content is also present', async () => {
      stubOk({
        model: 'llama3.2',
        message: { content: 'answer', thinking: 'reasoning' },
        done: true,
      })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(res.content).toBe('<think>reasoning</think>\n\nanswer')
      // native chat() never populates ChatResponse.thinking
      expect(res.thinking).toBeUndefined()
    })

    it('uses thinking AS content when content is empty', async () => {
      stubOk({ model: 'llama3.2', message: { content: '', thinking: 'only-reasoning' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(res.content).toBe('only-reasoning')
    })

    it('extracts tool calls (string + object args) and reports tool_use', async () => {
      stubOk({
        model: 'llama3.2',
        message: {
          content: '',
          tool_calls: [
            { function: { name: 'a', arguments: '{"x":1}' } },
            { function: { name: 'b', arguments: { y: 2 } } },
          ],
        },
        done: true,
      })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(res.stopReason).toBe('tool_use')
      expect(res.toolCalls).toEqual([
        { id: 'call_0', name: 'a', input: { x: 1 } },
        { id: 'call_1', name: 'b', input: { y: 2 } },
      ])
    })

    it('keeps a raw string when tool-call string args fail to parse', async () => {
      stubOk({
        model: 'llama3.2',
        message: { content: '', tool_calls: [{ id: 'real', function: { name: 'a', arguments: 'not json' } }] },
        done: true,
      })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      // Rust keeps Value::String(s) on parse failure; preserve the raw string.
      expect(res.toolCalls[0]).toEqual({ id: 'real', name: 'a', input: 'not json' })
    })

    it('falls back to DEFAULT_OLLAMA_MODEL when payload.model is missing', async () => {
      stubOk({ message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(res.model).toBe(DEFAULT_OLLAMA_MODEL)
    })

    it('reports stop=other when done is falsy and no tool calls', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'partial' }, done: false })
      const provider = new OllamaProvider('llama3.2', BASE)
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(res.stopReason).toBe('other')
    })

    it('prefers systemBlocks over system string (joined + trimmed)', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      await provider.chat({
        system: 'ignored',
        systemBlocks: [{ text: ' a ' }, { text: 'b ' }],
        messages: [{ role: 'user', content: 'hi' }],
      })
      expect(captured?.body.messages[0]).toEqual({ role: 'system', content: 'a \nb' })
    })

    it('merges providerOptions into the root last', async () => {
      stubOk({ model: 'llama3.2', message: { content: 'ok' }, done: true })
      const provider = new OllamaProvider('llama3.2', BASE)
      await provider.chat({
        messages: [{ role: 'user', content: 'hi' }],
        providerOptions: { format: 'json', seed: 7 },
      })
      expect(captured?.body.format).toBe('json')
      expect(captured?.body.seed).toBe(7)
    })
  })

  describe('OllamaProvider think coercion (mirrors ollama.rs:564-635)', () => {
    let captured: { body: any } | null = null
    afterEach(() => vi.unstubAllGlobals())

    function bodyForThink(think: string | undefined): Promise<any> {
      captured = null
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_url: string, options?: RequestInit) => {
          captured = { body: options?.body ? JSON.parse(String(options.body)) : null }
          return new Response(JSON.stringify({ model: 'llama3.2', message: { content: 'x' }, done: true }), {
            status: 200,
          })
        }),
      )
      const provider = new OllamaProvider('llama3.2', BASE).withThink(think)
      return provider
        .chat({ messages: [{ role: 'user', content: 'hi' }] })
        .then(() => captured!.body)
    }

    it('coerces truthy synonyms to boolean true', async () => {
      for (const input of ['true', 'yes', 'on', '1', 'YES', 'True', '  yes  ']) {
        const body = await bodyForThink(input)
        expect(body.think).toBe(true)
      }
    })

    it('coerces falsy synonyms to boolean false', async () => {
      for (const input of ['false', 'no', 'off', '0', 'NO', 'False']) {
        const body = await bodyForThink(input)
        expect(body.think).toBe(false)
      }
    })

    it('passes other values through as the trimmed verbatim string', async () => {
      const body = await bodyForThink('  low  ')
      expect(body.think).toBe('low')
      const body2 = await bodyForThink('high')
      expect(body2.think).toBe('high')
    })

    it('omits think for empty / whitespace-only / unset', async () => {
      for (const input of ['', ' ', '   ', '\t', '\n', '\t  \n']) {
        const body = await bodyForThink(input)
        expect(body.think).toBeUndefined()
      }
      const unset = await bodyForThink(undefined)
      expect(unset.think).toBeUndefined()
    })
  })

  describe('OllamaProvider stream (native NDJSON adapter)', () => {
    afterEach(() => vi.unstubAllGlobals())

    function stubNdjson(lines: string[]) {
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_url: string, options?: RequestInit) => {
          // assert stream:true is set on the streaming body
          const body = options?.body ? JSON.parse(String(options.body)) : null
          expect(body.stream).toBe(true)
          return new Response(lines.join(''), {
            status: 200,
            headers: { 'content-type': 'application/x-ndjson' },
          })
        }),
      )
    }

    it('emits text events then terminates on done:true (plain doneEvent, no stop_reason)', async () => {
      stubNdjson([
        '{"message":{"content":"Hel"},"done":false}\n',
        '{"message":{"content":"lo"},"done":false}\n',
        '{"done":true}\n',
      ])
      const provider = new OllamaProvider('llama3.2', BASE)
      const events: StreamEvent[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(e)
      }
      expect(events.map((e) => ({ t: e.eventType, c: e.content, done: e.done }))).toEqual([
        { t: 'text', c: 'Hel', done: false },
        { t: 'text', c: 'lo', done: false },
        { t: 'text', c: '', done: true },
      ])
      // plain doneEvent carries NO stopReason
      expect(events[events.length - 1].stopReason).toBeUndefined()
    })

    it('surfaces streamed thinking as a plain text event', async () => {
      stubNdjson([
        '{"message":{"content":"","thinking":"pondering"},"done":false}\n',
        '{"message":{"content":"answer"},"done":false}\n',
        '{"done":true}\n',
      ])
      const provider = new OllamaProvider('llama3.2', BASE)
      const texts: string[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        if (e.eventType === 'text' && !e.done) texts.push(e.content)
      }
      expect(texts).toEqual(['pondering', 'answer'])
    })

    it('emits the 3-event tool-call pattern (start, argsWithId, endWithId) with generated call_N ids', async () => {
      stubNdjson([
        '{"message":{"content":"","tool_calls":[{"function":{"name":"get_weather","arguments":{"city":"NYC"}}}]},"done":false}\n',
        '{"done":true}\n',
      ])
      const provider = new OllamaProvider('llama3.2', BASE)
      const events: StreamEvent[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(e)
      }
      const tool = events.filter((e) => e.eventType.startsWith('tool_call'))
      expect(tool).toHaveLength(3)
      expect(tool[0]).toMatchObject({
        eventType: 'tool_call_start',
        toolCallId: 'call_0',
        toolCallName: 'get_weather',
      })
      expect(tool[1]).toMatchObject({
        eventType: 'tool_call_args',
        toolCallId: 'call_0',
        toolCallArgsDelta: '{"city":"NYC"}',
      })
      expect(tool[2]).toMatchObject({ eventType: 'tool_call_end', toolCallId: 'call_0' })
      expect(events[events.length - 1].done).toBe(true)
    })

    it('ends WITHOUT synthesizing a done event on EOF without done:true', async () => {
      stubNdjson([
        '{"message":{"content":"a"},"done":false}\n',
        '{"message":{"content":"b"},"done":false}\n',
      ])
      const provider = new OllamaProvider('llama3.2', BASE)
      const events: StreamEvent[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(e)
      }
      // No terminal done event synthesized (matches Rust Poll::Ready(None)).
      expect(events.every((e) => !e.done)).toBe(true)
      expect(events.map((e) => e.content)).toEqual(['a', 'b'])
    })
  })

  describe('OllamaProvider capabilities + retry', () => {
    afterEach(() => vi.unstubAllGlobals())

    it('is text-only and reports supportsMcp:false', () => {
      const caps = new OllamaProvider('llama3.2', BASE).capabilities()
      expect(caps.supportsImage).toBe(false)
      expect(caps.supportsDocument).toBe(false)
      // M4 adds supportsMcp to every factory; textOnly() must carry it.
      expect((caps as { supportsMcp: boolean }).supportsMcp).toBe(false)
    })

    it('retries a retryable 503 on chat() then succeeds', async () => {
      let calls = 0
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => {
          calls += 1
          if (calls === 1) {
            return new Response(JSON.stringify({ error: { message: 'overloaded' } }), { status: 503 })
          }
          return new Response(JSON.stringify({ model: 'llama3.2', message: { content: 'ok' }, done: true }), {
            status: 200,
          })
        }),
      )
      const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
        new RetryPolicy({ maxRetries: 2, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
      )
      const res = await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(calls).toBe(2)
      expect(res.content).toBe('ok')
    })

    it('does NOT retry a non-retryable 400 on chat()', async () => {
      let calls = 0
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => {
          calls += 1
          return new Response(JSON.stringify({ error: { message: 'bad' } }), { status: 400 })
        }),
      )
      const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
        new RetryPolicy({ maxRetries: 3, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
      )
      await expect(provider.chat({ messages: [{ role: 'user', content: 'hi' }] })).rejects.toThrow('bad')
      expect(calls).toBe(1)
    })

    it('retries the INITIAL stream fetch on 503 then streams', async () => {
      let calls = 0
      vi.stubGlobal(
        'fetch',
        vi.fn(async () => {
          calls += 1
          if (calls === 1) return new Response('upstream down', { status: 503 })
          return new Response('{"message":{"content":"hi"},"done":false}\n{"done":true}\n', { status: 200 })
        }),
      )
      const provider = new OllamaProvider('llama3.2', BASE).withRetryPolicy(
        new RetryPolicy({ maxRetries: 2, baseDelayMs: 0, maxDelayMs: 0, jitter: false }),
      )
      const events: StreamEvent[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        events.push(e)
      }
      expect(calls).toBe(2)
      expect(events[0].content).toBe('hi')
      expect(events[events.length - 1].done).toBe(true)
    })
  })

  // Env-gated live test (no mock). Requires a local Ollama with llama3.2 pulled.
  const liveBase = process.env.OLLAMA_BASE_URL
  describe.skipIf(!liveBase)('OllamaProvider live (env-gated)', () => {
    it('streams a real native /api/chat response', async () => {
      const provider = new OllamaProvider('llama3.2', liveBase as string)
      let text = ''
      let sawDone = false
      for await (const e of provider.stream({
        messages: [{ role: 'user', content: 'Say "ok" and nothing else.' }],
      })) {
        if (e.eventType === 'text' && !e.done) text += e.content
        if (e.done) sawDone = true
      }
      expect(sawDone).toBe(true)
      expect(text.length).toBeGreaterThan(0)
    }, 30000)
  })
  ```

  Run (will FAIL — module does not exist yet):

  ```
  npm run test -- tests/providers-ollama.test.ts
  ```
  Expected: failure resolving `../src/providers/ollama.js`.

- [ ] **Step 2: Implement `src/providers/ollama.ts` to pass the tests.**
  Create `sdks/typescript/src/providers/ollama.ts`. The shape below is grounded
  line-by-line in `ollama.rs` and mirrors `OpenAIProvider`'s retry idioms.

  ```ts
  import { isRetryableNetworkError, isRetryableStatus } from '../error.js'
  import { postJson, postStream } from '../http/fetch.js'
  import { parseNdjson } from '../http/ndjson.js'
  import { DEFAULT_OLLAMA_MODEL } from '../models.js'
  import { textOnly, type ProviderCapabilities } from '../provider.js'
  import { RetryPolicy, withRetry, type RetryClassification } from '../retry.js'
  import {
    doneEvent,
    textEvent,
    toolCallArgsWithId,
    toolCallEndWithId,
    toolCallStart,
    type BoxStream,
  } from '../stream.js'
  import type { ChatRequest, ChatResponse, StopReason, ToolCall } from '../types.js'

  /** Same classify shape as providers/openai.ts:38-51 / providers/minimax.ts. */
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
   * A native-API tool call before id-defaulting. Mirrors Rust ToolCall but
   * `input` may be any JSON value (string fallback on parse failure), matching
   * Rust's `Value::String(s)` keep-raw behavior (ollama.rs:225-227).
   */
  interface NativeToolCall {
    id: string
    name: string
    input: unknown
  }

  /**
   * Native Ollama provider hitting `/api/chat` with NDJSON streaming.
   * Mirrors Rust `OllamaProvider` (providers/ollama.rs). Text-only; no auth header.
   * The OpenAI-compat path is NOT here — it reuses OpenAIProvider (wired in
   * client.ts buildProvider). Constructor `(model, baseUrl)` mirrors
   * `OllamaProvider::new` (ollama.rs:29-39).
   */
  export class OllamaProvider {
    private readonly model: string
    private readonly baseUrl: string
    private think?: string
    private keepAlive?: string
    private numCtx?: number
    private retryPolicy: RetryPolicy

    constructor(model: string, baseUrl: string) {
      this.model = model
      this.baseUrl = baseUrl.replace(/\/+$/, '') // trim trailing slash(es)
      this.retryPolicy = RetryPolicy.default()
    }

    withThink(think?: string): this {
      this.think = think
      return this
    }

    withKeepAlive(keepAlive?: string): this {
      this.keepAlive = keepAlive
      return this
    }

    withNumCtx(numCtx?: number): this {
      this.numCtx = numCtx
      return this
    }

    withRetryPolicy(policy: RetryPolicy): this {
      this.retryPolicy = policy
      return this
    }

    capabilities(): ProviderCapabilities {
      // text_only(); supportsMcp:false comes for free via the M4 factory.
      return textOnly()
    }

    private endpoint(): string {
      return `${this.baseUrl}/api/chat`
    }

    /** Native /api/chat body. stream=false for chat(), true for stream(). */
    private buildRequestBody(req: ChatRequest, stream: boolean): Record<string, unknown> {
      const model = req.model ?? this.model
      const messages: Array<Record<string, unknown>> = []

      // 1. System first: systemBlocks priority over system string.
      if (req.systemBlocks) {
        const joined = req.systemBlocks.map((b) => b.text).join('\n').trim()
        if (joined) messages.push({ role: 'system', content: joined })
      } else if (req.system) {
        const trimmed = req.system.trim()
        if (trimmed) messages.push({ role: 'system', content: trimmed })
      }

      // 2. Then each message by role.
      for (const message of req.messages) {
        switch (message.role) {
          case 'system': {
            const trimmed = message.content.trim()
            if (trimmed) messages.push({ role: 'system', content: trimmed })
            break
          }
          case 'user': {
            // Native path is text-only: flat string content (images rejected
            // by validate before reaching here).
            messages.push({ role: 'user', content: message.content })
            break
          }
          case 'assistant': {
            if (!message.toolCalls || message.toolCalls.length === 0) {
              messages.push({ role: 'assistant', content: message.content })
            } else {
              const toolCalls = message.toolCalls.map((tc) => ({
                // arguments is the PARSED OBJECT tc.input directly (NOT a JSON
                // string — contrast OpenAI). No top-level id/type.
                function: { name: tc.name, arguments: tc.input },
              }))
              messages.push({
                role: 'assistant',
                content: message.content,
                tool_calls: toolCalls,
              })
            }
            break
          }
          case 'tool': {
            // NO tool_call_id on the native path (contrast OpenAI compat).
            messages.push({ role: 'tool', content: message.content })
            break
          }
        }
      }

      const body: Record<string, unknown> = { model, messages, stream }

      // think coercion (ollama.rs:145-157): trim; omit if blank.
      if (this.think !== undefined) {
        const trimmed = this.think.trim()
        if (trimmed) {
          switch (trimmed.toLowerCase()) {
            case 'true':
            case 'yes':
            case 'on':
            case '1':
              body.think = true
              break
            case 'false':
            case 'no':
            case 'off':
            case '0':
              body.think = false
              break
            default:
              // verbatim ORIGINAL trimmed casing (e.g. "low"/"medium"/"high")
              body.think = trimmed
          }
        }
      }

      // keep_alive verbatim string at root.
      if (this.keepAlive !== undefined) {
        body.keep_alive = this.keepAlive
      }

      // options object — OMIT entirely if empty.
      const options: Record<string, unknown> = {}
      if (req.temperature !== undefined) options.temperature = req.temperature
      if (this.numCtx !== undefined) options.num_ctx = this.numCtx
      if (req.stopSequences && req.stopSequences.length > 0) options.stop = req.stopSequences
      if (Object.keys(options).length > 0) body.options = options

      // tools — only set if non-empty. TS Tool fields optional; match the
      // serialize/openai.ts defaults (openai.ts:151-152).
      if (req.tools) {
        const mapped = req.tools.map((tool) => ({
          type: 'function',
          function: {
            name: tool.name,
            description: tool.description ?? '',
            parameters: tool.inputSchema ?? { type: 'object', properties: {} },
          },
        }))
        if (mapped.length > 0) body.tools = mapped
      }

      // providerOptions merged into root LAST (Object.assign).
      if (req.providerOptions && typeof req.providerOptions === 'object') {
        Object.assign(body, req.providerOptions)
      }

      return body
    }

    /** Shared by chat + stream (ollama.rs:212-242). */
    private static extractToolCalls(message: any): NativeToolCall[] {
      const calls = message?.tool_calls
      if (!Array.isArray(calls)) return []
      const out: NativeToolCall[] = []
      calls.forEach((call: any, idx: number) => {
        const fn = call?.function
        if (!fn) return // skip if missing
        const name = fn?.name
        if (typeof name !== 'string') return // skip if missing
        const args = fn?.arguments
        let input: unknown
        if (typeof args === 'string') {
          try {
            input = JSON.parse(args)
          } catch {
            input = args // keep raw string on parse failure (Value::String)
          }
        } else if (args === undefined || args === null) {
          input = {}
        } else {
          input = args
        }
        const id =
          typeof call?.id === 'string' && call.id.length > 0 ? call.id : `call_${idx}`
        out.push({ id, name, input })
      })
      return out
    }

    async chat(req: ChatRequest): Promise<ChatResponse> {
      const body = this.buildRequestBody(req, false)
      const payload = await withRetry(
        this.retryPolicy,
        async () => postJson<any>(this.endpoint(), {}, body),
        classifyHttpError,
      )

      const message = payload?.message
      const content =
        typeof message?.content === 'string' ? message.content : ''
      const thinking =
        typeof message?.thinking === 'string' ? message.thinking : ''

      // final_content thinking-fold (ollama.rs:302-309).
      let finalContent: string
      if (content === '' && thinking !== '') {
        finalContent = thinking
      } else if (thinking !== '') {
        finalContent = `<think>${thinking}</think>\n\n${content}`
      } else {
        finalContent = content
      }

      const native = message ? OllamaProvider.extractToolCalls(message) : []
      const toolCalls: ToolCall[] = native.map((tc) => ({
        id: tc.id,
        name: tc.name,
        // extractToolCalls keeps the RAW string when JSON.parse fails (Rust
        // Value::String fallback, ollama.rs), so tc.input may be a string. The
        // cast is a deliberate escape hatch; ToolCall.input stays Record<...>
        // (a post-M5 widening of ToolCall.input to unknown is the clean fix).
        input: tc.input as Record<string, unknown>,
      }))

      let stopReason: StopReason
      if (toolCalls.length > 0) {
        stopReason = 'tool_use'
      } else {
        stopReason = payload?.done === true ? 'stop' : 'other'
      }

      const model = String(payload?.model ?? DEFAULT_OLLAMA_MODEL)
      const inputTokens = Number(payload?.prompt_eval_count ?? 0)
      const outputTokens = Number(payload?.eval_count ?? 0)

      return {
        content: finalContent,
        // native chat() folds thinking into content via <think>; it does NOT
        // populate ChatResponse.thinking (ollama.rs never calls .thinking()).
        thinking: undefined,
        toolCalls,
        model,
        usage: { inputTokens, outputTokens },
        stopReason,
      }
    }

    stream(req: ChatRequest): BoxStream {
      return this.streamImpl(req)
    }

    private async *streamImpl(req: ChatRequest) {
      const body = this.buildRequestBody(req, true)

      // Retry ONLY the initial postStream fetch (mirrors openai.ts:326-351).
      let attempt = 0
      let responseBody: ReadableStream<Uint8Array>
      while (true) {
        try {
          responseBody = await postStream(this.endpoint(), {}, body)
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

      // Adapter over parseNdjson: decide termination on done:true.
      for await (const obj of parseNdjson(responseBody)) {
        const done = (obj as { done?: unknown }).done === true
        if (done) {
          // Rust StreamEvent::done() carries NO stop_reason — plain doneEvent.
          yield doneEvent()
          return
        }

        const message = (obj as { message?: any }).message
        const content =
          typeof message?.content === 'string' ? message.content : ''
        const thinking =
          typeof message?.thinking === 'string' ? message.thinking : ''

        // text selection (ollama.rs:459-465). Streamed thinking is surfaced as
        // a plain textEvent (folded into the text stream).
        let text: string
        if (thinking !== '' && content === '') {
          text = thinking
        } else if (content !== '') {
          text = content
        } else {
          text = ''
        }
        if (text !== '') {
          yield textEvent(text)
        }

        // 3-event pattern per tool call (ollama.rs:471-483).
        if (message) {
          for (const tc of OllamaProvider.extractToolCalls(message)) {
            yield toolCallStart(tc.id, tc.name)
            yield toolCallArgsWithId(tc.id, JSON.stringify(tc.input))
            yield toolCallEndWithId(tc.id)
          }
        }
      }
      // EOF without done:true — let the generator end (NO synthesized done),
      // matching Rust Poll::Ready(None). collectStream fabricates a stop_reason.
    }
  }
  ```

  Run the unit tests:

  ```
  npm run test -- tests/providers-ollama.test.ts
  ```
  Expected: all non-live cases PASS; the live `describe.skipIf` block is
  skipped (no `OLLAMA_BASE_URL`).

- [ ] **Step 3: Build (strict typecheck).**

  ```
  npm run build
  ```
  Expected: exit 0. Note `src/providers/ollama.ts` is compiled; the test file
  is NOT tsc-checked (tests/ excluded). If the build flags `supportsMcp` not
  existing on `ProviderCapabilities`, that means the M4 merge is missing — STOP
  and confirm the M4-merged tree (contract: ProviderCapabilities must be
  `{supportsImage, supportsDocument, supportsMcp}`). `textOnly()` returning
  `supportsMcp:false` is an M4 dependency; this task adds NO supportsMcp code.

- [ ] **Step 4: Optional live verification (skipped in CI).**
  If a local Ollama is running with `llama3.2` pulled:

  ```
  OLLAMA_BASE_URL=http://localhost:11434 npm run test -- tests/providers-ollama.test.ts
  ```
  Expected: the live streaming case passes (real NDJSON, terminal done event).

- [ ] **Step 5: Commit.**

  ```
  git add sdks/typescript/src/providers/ollama.ts sdks/typescript/tests/providers-ollama.test.ts
  git commit -m "feat(ts): add native Ollama provider (/api/chat NDJSON + tool-call streaming)"
  ```

---

### Task 4: Extend the Provider union with 'ollama' in provider.ts

`provider.ts` owns the `Provider` string-union discriminator. M5 adds `'ollama'`
to it. Per the contract (§4), the AUTO-ROUTING decision (native vs OpenAI-compat)
does NOT live in `dispatchChat`/`dispatchStream` — it happens at
provider-CONSTRUCTION time in `ClientBuilder.buildProvider` (Task 5), because the
builder is the only place that knows all the tuning fields. `dispatchChat`/
`dispatchStream` stay provider-agnostic (validate → call) and need NO ollama arm.
So this task is a ONE-LINE union change plus a comment, plus a type-level test.

**Owns:** `src/provider.ts` (the `Provider` type union — provider.ts:74) +
the new union-shape test. Touches NOTHING else. Task 3 owns the provider class;
Task 5 owns the builder/routing; this task owns ONLY the union token.

**Files:**
- `sdks/typescript/src/provider.ts` (MODIFY — one line + comment)
- `sdks/typescript/tests/capabilities.test.ts` (MODIFY — add a union type-assertion case)

Current (verified, `src/provider.ts:73-74`):

```ts
/** String-tagged provider discriminator. Extend with 'ollama' | 'gemini' in M5/M6. */
export type Provider = 'anthropic' | 'openai' | 'minimax'
```

IMPORTANT — do NOT touch `ProviderCapabilities` here. The contract states M4
already added `supportsMcp` to it (this task builds on the M4-merged tree).
`dispatchChat`/`dispatchStream`/`validateRequest`/`textOnly`/`withImage`/
`fullCaps` are unchanged by M5. The ONLY edit is the `Provider` union.

---

#### Steps

- [ ] **Step 1: Write a failing type-level + runtime test FIRST.**
  Open `sdks/typescript/tests/capabilities.test.ts` (it already imports from
  `../src/provider.js`). Add a new `describe` block at the end of the file:

  ```ts
  describe('Provider union includes ollama (M5)', () => {
    it('accepts "ollama" as a Provider value', () => {
      // Type-level: this assignment only compiles once 'ollama' is in the union.
      const p: Provider = 'ollama'
      expect(p).toBe('ollama')
    })

    it('still accepts the M1-M4 provider tokens', () => {
      const all: Provider[] = ['anthropic', 'openai', 'minimax', 'ollama']
      expect(all).toContain('ollama')
      expect(all).toHaveLength(4)
    })
  })
  ```

  Ensure `Provider` is imported at the top of the file. If it is not already
  imported, add to the existing `../src/provider.js` import:

  ```ts
  import type { Provider } from '../src/provider.js'
  ```

  Run:

  ```
  npm run test -- tests/capabilities.test.ts
  ```
  Expected: the runtime assertions pass even before the union change (string
  equality), BUT the build will fail on the type assignment. The load-bearing
  gate is the build in Step 3 — the type error `Type '"ollama"' is not
  assignable to type 'Provider'` proves the test is meaningful. (If you want a
  red test first, run `npm run build` now and observe that exact error.)

  ```
  npm run build
  ```
  Expected (BEFORE the fix): TS2322 on `const p: Provider = 'ollama'` —
  `"ollama"` not assignable to `Provider`.

- [ ] **Step 2: Add 'ollama' to the union.**
  In `sdks/typescript/src/provider.ts`, edit lines 73-74:

  ```ts
  /**
   * String-tagged provider discriminator. 'ollama' added in M5. Extend with
   * 'gemini' in M6. Auto-routing between Ollama's native /api/chat and the
   * OpenAI-compat path is decided at provider-construction time in
   * ClientBuilder.buildProvider (see client.ts) — dispatchChat/dispatchStream
   * stay provider-agnostic, so NO 'ollama' arm is added here.
   */
  export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama'
  ```

  Do NOT add an `'ollama'` branch to `dispatchChat`/`dispatchStream` — they
  validate-then-call and are provider-agnostic by design (contract §4). The
  routing lives in Task 5's `buildProvider`.

- [ ] **Step 3: Build + test green.**

  ```
  npm run build
  npm run test -- tests/capabilities.test.ts
  ```
  Expected: build exit 0 (the `const p: Provider = 'ollama'` assignment now
  compiles); both new cases pass.

  NOTE: `client.ts`'s `ENV_KEY_BY_PROVIDER: Record<ProviderName, string>` will
  now MISS the `'ollama'` key and FAIL to compile — that is EXPECTED and is
  Task 5's responsibility to resolve (add `ollama: ''`). If you run the FULL
  build here it may surface that error in `client.ts`. That is the documented
  cross-task seam; do not "fix" client.ts from this task. Scope this task's
  build check to type-checking provider.ts in isolation if needed, but the
  canonical full-build green happens after Task 5 lands. State this explicitly
  in the PR/commit so the reviewer expects the transient client.ts error
  between Task 4 and Task 5.

- [ ] **Step 4: Commit.**

  ```
  git add sdks/typescript/src/provider.ts sdks/typescript/tests/capabilities.test.ts
  git commit -m "feat(ts): add 'ollama' to the Provider union"
  ```

---

### Task 5: ClientBuilder ollama setters + routing + validation + api-key seam in client.ts

`client.ts` owns the `ClientBuilder` (setters, `buildProvider`, `build()`), the
`HTTP_PROVIDERS` seam, and `ENV_KEY_BY_PROVIDER`. M5 adds:
1. Five tuning setters (`ollamaBaseUrl`/`ollamaNative`/`ollamaThink`/
   `ollamaKeepAlive`/`ollamaNumCtx`) + their protected fields.
2. A `buildProvider` ollama arm that computes `needsNative` (§4) and constructs
   either the native `OllamaProvider` (Task 3) or a configured `OpenAIProvider`
   (compat).
3. The api-key-NOT-required seam: keep `'ollama'` OUT of `HTTP_PROVIDERS`, add
   `ollama: ''` to `ENV_KEY_BY_PROVIDER`, and skip the legacy-constructor throw.
4. `build()` validation throwing `ConfigError` when a tuning field is set on a
   non-ollama provider.

**Owns:** `src/client.ts` + `tests/client-builder.test.ts`. Touches NOTHING
else. IMPORTS `OllamaProvider` from `./providers/ollama.js` (Task 3),
`DEFAULT_OLLAMA_MODEL` from `./models.js` (Task 1), and relies on the `'ollama'`
`Provider` union token (Task 4). Does NOT modify `provider.ts`, `models.ts`,
`providers/ollama.ts`, or `index.ts`.

**Files:**
- `sdks/typescript/src/client.ts` (MODIFY)
- `sdks/typescript/tests/client-builder.test.ts` (MODIFY — add ollama describe blocks)

Grounding:
- Rust routing: `client.rs:245-266` (chat) / `:413-434` (stream) — identical
  `needs_native`; `:610-621` (`build_ollama_native_provider`); `:588-608`
  (`build_ollama_provider` compat); `:980-997` (tuning-field validation);
  `:961-967` (api_key optional seam); default base `http://localhost:11434`
  (`:1009-1011`).
- Current `client.ts`: `HTTP_PROVIDERS` (line 31), `ENV_KEY_BY_PROVIDER` (33-37),
  `buildProvider` (139-163), `build()` (166-179), legacy ctor key throw (227-230).

The OpenAI-compat arm constructs `new OpenAIProvider('', model)` then
`.withChatUrl(...)`, `.withAuthStyle({kind:'bearer'})`, `.withRetryPolicy(...)`
— exactly Rust `build_ollama_provider` (client.rs:599-607). It declares
`withImage()` capabilities (OpenAIProvider default). The native arm is
text-only. The SAME constructed instance serves both chat and stream (routing
at construction time), structurally satisfying the "never split" invariant.

---

#### Steps

- [ ] **Step 1: Write failing tests FIRST.**
  In `sdks/typescript/tests/client-builder.test.ts`, the file already imports
  `Client`, `ClientBuilder`, `ConfigError`, `UnsupportedFeatureError`,
  `RetryPolicy`, and `ChatRequest`. Add these `describe` blocks at the end of
  the file. They cover: routing (native vs compat in BOTH chat and stream),
  tuning-field validation, api-key-not-required, and image rejection on the
  native route.

  ```ts
  describe('ClientBuilder Ollama routing (native vs compat)', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    function nativeOk() {
      const urls: string[] = []
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string, options?: RequestInit) => {
          urls.push(url)
          // Return an Ollama native NDJSON-ish body that also parses as a
          // single JSON object for chat() (chat uses postJson; one object).
          const isStream = options?.body ? JSON.parse(String(options.body)).stream : false
          if (isStream) {
            return new Response('{"message":{"content":"hi"},"done":false}\n{"done":true}\n', {
              status: 200,
            })
          }
          return new Response(
            JSON.stringify({ model: 'llama3.2', message: { content: 'hi' }, done: true }),
            { status: 200 },
          )
        }),
      )
      return urls
    }

    function compatOk() {
      const urls: string[] = []
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string, options?: RequestInit) => {
          urls.push(url)
          const isStream = options?.body ? JSON.parse(String(options.body)).stream : false
          if (isStream) {
            return new Response(
              'data: {"choices":[{"index":0,"delta":{"content":"hi"},"finish_reason":"stop"}]}\n\ndata: [DONE]\n\n',
              { status: 200, headers: { 'content-type': 'text/event-stream' } },
            )
          }
          return new Response(
            JSON.stringify({
              model: 'llama3.2',
              choices: [{ index: 0, message: { content: 'hi' }, finish_reason: 'stop' }],
              usage: { prompt_tokens: 1, completion_tokens: 1 },
            }),
            { status: 200 },
          )
        }),
      )
      return urls
    }

    it('routes to native /api/chat for BOTH chat and stream when ollamaNative(true)', async () => {
      const urls = nativeOk()
      const client = new ClientBuilder()
        .provider('ollama')
        .ollamaBaseUrl('http://localhost:11434')
        .ollamaNative(true)
        .build()

      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      for await (const _e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        // drain
      }

      // SAME instance, SAME endpoint for chat and stream (never split).
      expect(urls).toEqual([
        'http://localhost:11434/api/chat',
        'http://localhost:11434/api/chat',
      ])
    })

    it('routes to native when ANY tuning field is set (ollamaThink)', async () => {
      const urls = nativeOk()
      const client = new ClientBuilder()
        .provider('ollama')
        .ollamaThink('high')
        .build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(urls[0]).toBe('http://localhost:11434/api/chat')
    })

    it('routes to native when ollamaKeepAlive is set', async () => {
      const urls = nativeOk()
      const client = new ClientBuilder().provider('ollama').ollamaKeepAlive('5m').build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(urls[0]).toBe('http://localhost:11434/api/chat')
    })

    it('routes to native when ollamaNumCtx is set', async () => {
      const urls = nativeOk()
      const client = new ClientBuilder().provider('ollama').ollamaNumCtx(4096).build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(urls[0]).toBe('http://localhost:11434/api/chat')
    })

    it('routes to OpenAI-compat /v1/chat/completions by default (no tuning, no native flag)', async () => {
      const urls = compatOk()
      const client = new ClientBuilder()
        .provider('ollama')
        .ollamaBaseUrl('http://localhost:11434')
        .build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      for await (const _e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        // drain
      }
      expect(urls).toEqual([
        'http://localhost:11434/v1/chat/completions',
        'http://localhost:11434/v1/chat/completions',
      ])
    })

    it('compat path sends Bearer with an empty key (harmless against Ollama)', async () => {
      let authHeader: string | undefined
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_url: string, options?: RequestInit) => {
          authHeader = (options?.headers as Record<string, string>)?.authorization
          return new Response(
            JSON.stringify({
              model: 'llama3.2',
              choices: [{ index: 0, message: { content: 'hi' }, finish_reason: 'stop' }],
              usage: { prompt_tokens: 1, completion_tokens: 1 },
            }),
            { status: 200 },
          )
        }),
      )
      const client = new ClientBuilder().provider('ollama').build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(authHeader).toBe('Bearer ')
    })

    it('defaults the base URL to http://localhost:11434 when unset', async () => {
      const urls = compatOk()
      const client = new ClientBuilder().provider('ollama').build()
      await client.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(urls[0]).toBe('http://localhost:11434/v1/chat/completions')
    })
  })

  describe('ClientBuilder Ollama api-key-not-required seam', () => {
    beforeEach(() => {
      delete process.env.OLLAMA_API_KEY
    })

    it('build() succeeds for provider("ollama") with NO apiKey', () => {
      const client = new ClientBuilder()
        .provider('ollama')
        .ollamaBaseUrl('http://localhost:11434')
        .build()
      expect(client).toBeInstanceOf(Client)
    })

    it('build() succeeds for provider("ollama") with no apiKey and no base url', () => {
      const client = new ClientBuilder().provider('ollama').build()
      expect(client).toBeInstanceOf(Client)
    })
  })

  describe('ClientBuilder Ollama tuning-field validation (non-ollama provider)', () => {
    beforeEach(() => {
      delete process.env.OPENAI_API_KEY
    })

    it('throws ConfigError naming ollama_think on a non-ollama provider', () => {
      const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaThink('high')
      expect(() => builder.build()).toThrowError(ConfigError)
      expect(() => builder.build()).toThrowError('ollama_think can only be used with Provider::Ollama')
    })

    it('throws ConfigError naming ollama_keep_alive on a non-ollama provider', () => {
      const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaKeepAlive('5m')
      expect(() => builder.build()).toThrowError('ollama_keep_alive can only be used with Provider::Ollama')
    })

    it('throws ConfigError naming ollama_num_ctx on a non-ollama provider', () => {
      const builder = new ClientBuilder().provider('openai').apiKey('sk-test').ollamaNumCtx(4096)
      expect(() => builder.build()).toThrowError('ollama_num_ctx can only be used with Provider::Ollama')
    })

    it('joins multiple misused tuning fields in Rust order', () => {
      const builder = new ClientBuilder()
        .provider('openai')
        .apiKey('sk-test')
        .ollamaThink('high')
        .ollamaKeepAlive('5m')
        .ollamaNumCtx(4096)
      // Rust pushes keep_alive, num_ctx, think in that order (client.rs:982-990).
      expect(() => builder.build()).toThrowError(
        'ollama_keep_alive, ollama_num_ctx, ollama_think can only be used with Provider::Ollama',
      )
    })

    it('does NOT validate ollamaNative or ollamaBaseUrl on a non-ollama provider', () => {
      // Rust only guards the three TUNING fields, not native/base_url.
      const client = new ClientBuilder()
        .provider('openai')
        .apiKey('sk-test')
        .ollamaNative(true)
        .ollamaBaseUrl('http://x')
        .build()
      expect(client).toBeInstanceOf(Client)
    })
  })

  describe('ClientBuilder Ollama native route rejects image input before HTTP', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    const imageReq: ChatRequest = {
      messages: [
        {
          role: 'user',
          content: '',
          contentBlocks: [
            { type: 'image', source: { type: 'url', url: 'https://example.com/i.jpg' } },
          ],
        },
      ],
    }

    it('chat() throws UnsupportedFeatureError on the native route (tuning field set)', async () => {
      const fetchSpy = vi.fn()
      vi.stubGlobal('fetch', fetchSpy)
      const client = new ClientBuilder().provider('ollama').ollamaThink('high').build()
      await expect(client.chat(imageReq)).rejects.toThrow(UnsupportedFeatureError)
      await expect(client.chat(imageReq)).rejects.toThrow('provider does not support image input')
      expect(fetchSpy).not.toHaveBeenCalled()
    })

    it('stream() throws UnsupportedFeatureError on the native route (ollamaNative)', async () => {
      const fetchSpy = vi.fn()
      vi.stubGlobal('fetch', fetchSpy)
      const client = new ClientBuilder().provider('ollama').ollamaNative(true).build()
      await expect(async () => {
        for await (const _e of client.stream(imageReq)) {
          // drain
        }
      }).rejects.toThrow('provider does not support image input')
      expect(fetchSpy).not.toHaveBeenCalled()
    })

    it('compat route ALLOWS image input (OpenAIProvider withImage())', async () => {
      vi.stubGlobal(
        'fetch',
        vi.fn(
          async () =>
            new Response(
              JSON.stringify({
                model: 'llama3.2',
                choices: [{ index: 0, message: { content: 'ok' }, finish_reason: 'stop' }],
                usage: { prompt_tokens: 1, completion_tokens: 1 },
              }),
              { status: 200 },
            ),
        ),
      )
      const client = new ClientBuilder().provider('ollama').build() // no tuning → compat
      const res = await client.chat(imageReq)
      expect(res.content).toBe('ok')
    })
  })
  ```

  Run (FAIL — setters/routing not implemented; also `ollamaThink` etc. are not
  methods, and `provider('ollama')` may throw "Missing API key"):

  ```
  npm run test -- tests/client-builder.test.ts
  ```
  Expected: failures referencing missing `ollamaThink`/`ollamaNative`/etc.
  methods and/or a thrown "Missing API key for provider ollama".

- [ ] **Step 2: Add the import + tuning fields + setters.**
  In `sdks/typescript/src/client.ts`, add the imports near the existing provider
  imports (after line 15):

  ```ts
  import { OllamaProvider } from './providers/ollama.js'
  import { DEFAULT_OLLAMA_MODEL } from './models.js'
  ```

  Add protected fields to `ClientBuilder` after `_openaiResponsesUrl` (line 66):

  ```ts
    protected _ollamaBaseUrl?: string
    protected _ollamaNative?: boolean
    protected _ollamaThink?: string
    protected _ollamaKeepAlive?: string
    protected _ollamaNumCtx?: number
  ```

  Add the setters after `openaiResponsesUrl` (after line 131), mirroring Rust
  `client.rs:797-885`:

  ```ts
    ollamaBaseUrl(u: string): this {
      this._ollamaBaseUrl = u
      return this
    }

    ollamaNative(native: boolean): this {
      this._ollamaNative = native
      return this
    }

    ollamaThink(think: string): this {
      this._ollamaThink = think
      return this
    }

    ollamaKeepAlive(duration: string): this {
      this._ollamaKeepAlive = duration
      return this
    }

    ollamaNumCtx(tokens: number): this {
      this._ollamaNumCtx = tokens
      return this
    }
  ```

- [ ] **Step 3: Add the `buildProvider` ollama arm (routing at construction).**
  In `buildProvider` (currently lines 139-163), add an `'ollama'` branch BEFORE
  the final MiniMax `return`. Insert after the `if (provider === 'openai') { ... }`
  block (after line 159):

  ```ts
      if (provider === 'ollama') {
        const ollamaBaseUrl = this._ollamaBaseUrl ?? 'http://localhost:11434'

        // Auto-routing (contract §4 / client.rs:245-248). The SAME constructed
        // instance serves chat and stream, so routing can never split.
        const needsNative =
          this._ollamaNative === true ||
          this._ollamaKeepAlive !== undefined ||
          this._ollamaNumCtx !== undefined ||
          this._ollamaThink !== undefined

        if (needsNative) {
          // Native /api/chat (text-only). Mirrors build_ollama_native_provider
          // (client.rs:610-621).
          const model = this._model ?? DEFAULT_OLLAMA_MODEL
          return new OllamaProvider(model, ollamaBaseUrl)
            .withThink(this._ollamaThink)
            .withKeepAlive(this._ollamaKeepAlive)
            .withNumCtx(this._ollamaNumCtx)
            .withRetryPolicy(this._retryPolicy)
        }

        // OpenAI-compat /v1/chat/completions (reuses OpenAIProvider, empty key,
        // Bearer). Mirrors build_ollama_provider (client.rs:599-607). Declares
        // withImage() capabilities (model-dependent accuracy).
        const model = this._model ?? DEFAULT_OLLAMA_MODEL
        return new OpenAIProvider('', model)
          .withChatUrl(`${ollamaBaseUrl.replace(/\/+$/, '')}/v1/chat/completions`)
          .withAuthStyle({ kind: 'bearer' })
          .withRetryPolicy(this._retryPolicy)
      }
  ```

  (`buildProvider` returns `DispatchProvider`; both `OllamaProvider` and
  `OpenAIProvider` structurally satisfy it — `capabilities`/`chat`/`stream`.)

- [ ] **Step 4: api-key-not-required seam + ENV_KEY entry.**
  Do NOT add `'ollama'` to `HTTP_PROVIDERS` (line 31) — leaving it out makes
  `apiKeyRequired` false (contract §6: Ollama is the first consumer of the
  reserved optional-key seam). Add the forced `ENV_KEY_BY_PROVIDER` entry
  (the `Record<ProviderName, string>` type now requires it once 'ollama' is in
  the union from Task 4). Edit lines 33-37:

  ```ts
  const ENV_KEY_BY_PROVIDER: Record<ProviderName, string> = {
    anthropic: 'ANTHROPIC_API_KEY',
    openai: 'OPENAI_API_KEY',
    minimax: 'MINIMAX_API_KEY',
    // Ollama needs no env key; '' keeps the Record total and
    // process.env[''] yields undefined (harmless — apiKey not required).
    ollama: '',
  }
  ```

  `build()` (166-179) already does `apiKeyRequired = HTTP_PROVIDERS.has(provider)`
  → false for ollama → no throw; and `apiKey = this._apiKey ?? process.env['']`
  (undefined) → passes `apiKey ?? ''` (empty string) into `buildProvider`. No
  change needed to the existing `build()` key logic; only the validation block
  below is added.

- [ ] **Step 5: Tuning-field validation in build().**
  Insert this guard in `build()` AFTER the api-key check (after line 175,
  before `const provider = this.buildProvider(...)`). Mirrors Rust
  `client.rs:980-997` exactly — guards ONLY the three TUNING fields (NOT
  `ollamaNative`/`ollamaBaseUrl`), uses the Rust field-name strings, joins in
  Rust order (keep_alive, num_ctx, think):

  ```ts
      if (this._provider !== 'ollama') {
        const misused: string[] = []
        if (this._ollamaKeepAlive !== undefined) misused.push('ollama_keep_alive')
        if (this._ollamaNumCtx !== undefined) misused.push('ollama_num_ctx')
        if (this._ollamaThink !== undefined) misused.push('ollama_think')
        if (misused.length > 0) {
          throw new ConfigError(`${misused.join(', ')} can only be used with Provider::Ollama`)
        }
      }
  ```

- [ ] **Step 6: Legacy constructor — skip the throw for ollama.**
  The legacy `Client` constructor (lines 226-238) throws on a missing key for
  string providers and routes only to anthropic/openai/minimax. The contract
  (§6) says: route ollama through `ClientBuilder` only OR special-case the
  legacy path to skip the throw. Keep it minimal and total: guard the throw so
  `'ollama'` does not blow up, and route it through `OpenAIProvider` compat (the
  legacy ctor has no tuning fields, so compat is the only legacy-reachable
  path). Edit lines 226-238:

  ```ts
      const provider = opts.provider
      const apiKey = opts.apiKey ?? process.env[ENV_KEY_BY_PROVIDER[provider]]
      if (!apiKey && provider !== 'ollama') {
        throw new ConfigError(`Missing API key for provider ${provider}`)
      }

      if (provider === 'anthropic') {
        this.provider = new AnthropicProvider(apiKey, opts.model)
      } else if (provider === 'openai') {
        this.provider = new OpenAIProvider(apiKey, opts.model)
      } else if (provider === 'ollama') {
        // Legacy ctor has no tuning fields → OpenAI-compat path (empty key,
        // Bearer) against the default Ollama base URL. For native/tuning, use
        // Client.builder().
        this.provider = new OpenAIProvider(apiKey ?? '', opts.model ?? DEFAULT_OLLAMA_MODEL)
          .withChatUrl('http://localhost:11434/v1/chat/completions')
          .withAuthStyle({ kind: 'bearer' })
      } else {
        this.provider = new MinimaxProvider(apiKey, opts.model, opts.minimaxEndpoint)
      }
  ```

  (`apiKey` is `string | undefined` here; the anthropic/openai/minimax arms
  already assume it is set because of the throw — the `provider !== 'ollama'`
  guard preserves that, and the ollama arm uses `apiKey ?? ''`.)

- [ ] **Step 7: Build + test green.**

  ```
  npm run build
  npm run test -- tests/client-builder.test.ts
  ```
  Expected: build exit 0 (this is the first full-build green AFTER Task 4's
  transient `ENV_KEY_BY_PROVIDER` gap is closed); all new ollama describe
  blocks pass. Confirm specifically: routing (native & compat) hits the right
  endpoint for BOTH chat and stream; validation throws with the exact Rust
  message strings; `provider('ollama')` builds with no apiKey; native route
  rejects images before fetch.

- [ ] **Step 8: Commit.**

  ```
  git add sdks/typescript/src/client.ts sdks/typescript/tests/client-builder.test.ts
  git commit -m "feat(ts): wire Ollama into ClientBuilder (routing, tuning, no-key seam, validation)"
  ```

---

### Task 6: index.ts exports + done-criteria smoke test

`index.ts` is the package entrypoint. M5 must export the net-new public symbols:
`OllamaProvider` (Task 3's class) and `DEFAULT_OLLAMA_MODEL` (Task 1's constant),
mirroring how the other providers/models are exported. This task also owns the
M5 done-criteria smoke test: `Client.builder().provider('ollama')...build()`
round-trips for BOTH the native and compat routes, and the new symbols are
reachable from the package root. This is the LAST task — it runs the full
build + test gate.

**Owns:** `src/index.ts` (the new export lines) + `tests/index.ollama.test.ts`
(NEW smoke test). Touches NOTHING else. IMPORTS — via `index.ts` re-exports —
`OllamaProvider` (Task 3), `DEFAULT_OLLAMA_MODEL` (Task 1), and exercises the
`ClientBuilder` ollama wiring (Task 5). Does NOT modify any other source file.

**Files:**
- `sdks/typescript/src/index.ts` (MODIFY — add two exports)
- `sdks/typescript/tests/index.ollama.test.ts` (NEW smoke test)

Current `index.ts` (verified, lines 1-22): re-exports `./providers/openai.js`
etc. via `export *`, and re-exports model defaults explicitly (lines 12-19).

---

#### Steps

- [ ] **Step 1: Write the failing smoke test FIRST.**
  Create `sdks/typescript/tests/index.ollama.test.ts`:

  ```ts
  import { afterEach, describe, expect, it, vi } from 'vitest'

  describe('index.ts Ollama public exports (M5)', () => {
    it('re-exports OllamaProvider and DEFAULT_OLLAMA_MODEL from the package root', async () => {
      const mod = await import('../src/index.js')
      expect(typeof mod.OllamaProvider).toBe('function')
      expect(mod.DEFAULT_OLLAMA_MODEL).toBe('llama3.2')
    })

    it('constructs an OllamaProvider via the package root export (text-only caps)', async () => {
      const { OllamaProvider } = await import('../src/index.js')
      const provider = new OllamaProvider('llama3.2', 'http://localhost:11434')
      const caps = provider.capabilities()
      expect(caps.supportsImage).toBe(false)
      expect(caps.supportsDocument).toBe(false)
      expect((caps as { supportsMcp: boolean }).supportsMcp).toBe(false)
    })
  })

  describe('Client.builder().provider("ollama") done-criteria smoke', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('round-trips the NATIVE route end to end (no apiKey, /api/chat)', async () => {
      const { Client } = await import('../src/index.js')
      let url: string | undefined
      vi.stubGlobal(
        'fetch',
        vi.fn(async (u: string) => {
          url = u
          return new Response(
            JSON.stringify({ model: 'llama3.2', message: { content: 'pong' }, done: true }),
            { status: 200 },
          )
        }),
      )

      const client = Client.builder()
        .provider('ollama')
        .ollamaBaseUrl('http://localhost:11434')
        .ollamaNative(true)
        .build()

      const res = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
      expect(url).toBe('http://localhost:11434/api/chat')
      expect(res.content).toBe('pong')
      expect(res.stopReason).toBe('stop')
    })

    it('round-trips the COMPAT route end to end (no apiKey, /v1/chat/completions)', async () => {
      const { Client } = await import('../src/index.js')
      let url: string | undefined
      vi.stubGlobal(
        'fetch',
        vi.fn(async (u: string) => {
          url = u
          return new Response(
            JSON.stringify({
              model: 'llama3.2',
              choices: [{ index: 0, message: { content: 'pong' }, finish_reason: 'stop' }],
              usage: { prompt_tokens: 1, completion_tokens: 1 },
            }),
            { status: 200 },
          )
        }),
      )

      const client = Client.builder()
        .provider('ollama')
        .ollamaBaseUrl('http://localhost:11434')
        .build() // no tuning → compat

      const res = await client.chat({ messages: [{ role: 'user', content: 'ping' }] })
      expect(url).toBe('http://localhost:11434/v1/chat/completions')
      expect(res.content).toBe('pong')
    })

    it('round-trips a native STREAM through the Client (stripThink + terminal done)', async () => {
      const { Client } = await import('../src/index.js')
      vi.stubGlobal(
        'fetch',
        vi.fn(
          async () =>
            new Response(
              '{"message":{"content":"hel"},"done":false}\n{"message":{"content":"lo"},"done":false}\n{"done":true}\n',
              { status: 200 },
            ),
        ),
      )

      const client = Client.builder().provider('ollama').ollamaNative(true).build()
      let text = ''
      let sawDone = false
      for await (const e of client.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        if (e.eventType === 'text' && !e.done) text += e.content
        if (e.done) sawDone = true
      }
      expect(text).toBe('hello')
      expect(sawDone).toBe(true)
    })
  })
  ```

  Run (FAIL — `OllamaProvider`/`DEFAULT_OLLAMA_MODEL` not exported from index):

  ```
  npm run test -- tests/index.ollama.test.ts
  ```
  Expected: failures — `mod.OllamaProvider` is `undefined` /
  `mod.DEFAULT_OLLAMA_MODEL` is `undefined`. (The routing assertions also fail
  until the index re-exports are present, since `Client` resolves the same way.)

- [ ] **Step 2: Add the exports to index.ts.**
  In `sdks/typescript/src/index.ts`, add the provider re-export alongside the
  other providers (after line 8, `export * from './providers/minimax.js'`):

  ```ts
  export * from './providers/ollama.js'
  ```

  Add `DEFAULT_OLLAMA_MODEL` to the explicit models re-export block (lines
  12-19). Change the block to include it:

  ```ts
  export {
    DEFAULT_ANTHROPIC_MODEL,
    DEFAULT_OPENAI_MODEL,
    DEFAULT_MINIMAX_MODEL,
    DEFAULT_OLLAMA_MODEL,
    ANTHROPIC_MODELS,
    OPENAI_MODELS,
    MINIMAX_MODELS,
  } from './models.js'
  ```

  (`export * from './providers/ollama.js'` surfaces `OllamaProvider`. The
  `Provider`/`ProviderCapabilities` type re-exports at lines 20-21 already cover
  the union — Task 4 widened `Provider` to include `'ollama'`, so no change
  needed there.)

- [ ] **Step 3: Smoke test green.**

  ```
  npm run test -- tests/index.ollama.test.ts
  ```
  Expected: all cases pass — exports reachable; native + compat routes
  round-trip to the right endpoints with no apiKey; native stream produces
  "hello" and a terminal done.

- [ ] **Step 4: FULL build + FULL test gate (M5 done-criteria).**
  This is the final gate — `npm run build` + `npm run test` (no format script,
  per conventions). Run the WHOLE suite, not just the ollama files, to confirm
  no cross-task regressions (Tasks 1-5 + this one all integrated).

  ```
  npm run build
  npm run test
  ```
  Expected: build exit 0; the entire vitest suite green, including:
  - `tests/models.test.ts` (Task 1 — DEFAULT_OLLAMA_MODEL)
  - `tests/http.ndjson.test.ts` (Task 2 — Ollama NDJSON)
  - `tests/providers-ollama.test.ts` (Task 3 — native provider; live block skipped)
  - `tests/capabilities.test.ts` (Task 4 — Provider union)
  - `tests/client-builder.test.ts` (Task 5 — routing/validation/seam)
  - `tests/index.ollama.test.ts` (this task — exports + done-criteria smoke)
  - all pre-existing tests unchanged.

  If anything fails, it indicates a cross-task integration gap — diagnose
  before claiming done (verification-before-completion).

- [ ] **Step 5: Optional env-gated live confirmation.**
  With a local Ollama (llama3.2 pulled):

  ```
  OLLAMA_BASE_URL=http://localhost:11434 npm run test -- tests/providers-ollama.test.ts
  ```
  Expected: Task 3's live streaming case passes against the real server.

- [ ] **Step 6: Commit.**

  ```
  git add sdks/typescript/src/index.ts sdks/typescript/tests/index.ollama.test.ts
  git commit -m "feat(ts): export OllamaProvider + DEFAULT_OLLAMA_MODEL; M5 done-criteria smoke"
  ```

---

## Milestone Done Criteria (verify all before tagging v0.8.0)

- [ ] Ollama native `chat()` posts to `{base}/api/chat` with `stream:false`, correct `options.num_ctx`/`keep_alive`/`think` (truthy/falsy/string-verbatim coercion) per mocked-fetch assertions.
- [ ] NDJSON streaming reconstructs text deltas, thinking, and 3-event tool-call sequences (start+args+end, generated `call_N` ids), terminating on `{done:true}`.
- [ ] Auto-routing: setting `ollamaThink`/`ollamaKeepAlive`/`ollamaNumCtx` (or `ollamaNative`) routes to the native path in BOTH chat and stream; otherwise the OpenAI-compat path is used.
- [ ] Builder validation: a tuning field on `provider:'anthropic'` (etc.) throws `ConfigError` naming the field + Ollama; `build()` does NOT require an API key for `provider:'ollama'`.
- [ ] `OllamaProvider.capabilities()` returns `supportsMcp:false`; an MCP request on Ollama is rejected (via M4's `validateRequest`).
- [ ] `index.ts` exports `OllamaProvider` + `DEFAULT_OLLAMA_MODEL`; env-gated live Ollama test passes when a local instance is present; `npm run build` + `npm run test` green.

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet accompanies this plan):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks (superpowers:subagent-driven-development).
2. **Inline:** execute tasks in-session with checkpoints (superpowers:executing-plans).

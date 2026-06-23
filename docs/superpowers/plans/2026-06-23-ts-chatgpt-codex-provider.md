# Plan: Port the ChatGPT-Codex provider to the TypeScript SDK (release 0.11.0)

## ⚠️ CORRECTIONS (apply these; they refine the tasks below)

Adversarial plan review found the plan sound (the two "needs-rework" verdicts were FALSE — the reviewers
checked the branch for a not-yet-written implementation; that's expected at plan stage). Apply these few
real refinements while implementing:

- **R1 — drop `seenToolIds`.** The adapter-state `seenToolIds` Set is write-only (never read; only
  `sawToolCall` drives the terminal stop_reason). Do NOT add it — keep only `sawToolCall`. If parity intent
  matters, a one-line comment suffices.
- **R2 — keep `chatGptCodexErrorMessage()` out of the public surface.** It's used only for the
  precedence unit test (the stream path silently terminates and discards the message). Do NOT re-export it
  from `src/index.ts`; the test imports it directly from the provider module path. Mark it `@internal`.
- **R3 — annotate the `Provider` union edit.** When adding `'chatgpt_codex'` to the union in
  `src/provider.ts`, append a one-line note to the preceding provenance comment (mirroring the existing
  "ollama added in M5 / gemini in M6" annotations), e.g. `// chatgpt_codex added in 0.11.0 — built only
  via ClientBuilder.chatgptCodex (no env-key dispatch arm)`.
- Keep the provider-level re-export of `DEFAULT_CHATGPT_CODEX_MODEL` (load-bearing: the T1 test imports it
  from the provider module). `any` for wire bodies is fine — it matches the anthropic.ts/openai.ts
  precedent. **Mid-stream error handling = SILENT TERMINATE (return), NOT a throw** — this is correct and
  evidenced by ollama.ts + providers-ollama.test.ts:377; do not change it to a Python-style raise.

---

## Goal

Add a `ChatGptCodexProvider` to the Motosan TypeScript SDK that mirrors the verified Python
`chatgpt_codex.py` (itself a port of authoritative Rust `chatgpt_codex.rs`). The provider POSTs the
OpenAI Responses API to `https://chatgpt.com/backend-api/codex/responses` with a caller-supplied OAuth
`accessToken` + `accountId` (plus codex CLI headers), streams typed `response.*` SSE into the existing
TS `StreamEvent` taxonomy (text, reasoning→`thinking_delta`, function_call tool lifecycle, usage,
terminal stop), and exposes a non-streaming `chat()` via `collectStream`. Wire it into the
`Provider` union and `ClientBuilder.chatgptCodex(...)` with NO api-key required. Release as TS **0.11.0**
(purely additive).

## Architecture

- **One new provider file** `src/providers/chatgpt_codex.ts` containing: the constants, the `ChatGptCodexProvider`
  class, an inline `buildResponsesBody(request, model)` body builder (no `serialize/` file — the OpenAI
  Responses precedent builds inline too), and an inline `response.*` SSE adapter inside `streamImpl`.
  This obeys the project rule "provider logic goes in `providers/` only."
- **Streaming** uses `async function*` over the existing `parseSse(responseBody)` helper, which already
  JSON-parses the SSE `data` payload. The Responses discriminator is `data.type` (INSIDE the JSON), not
  the SSE `event:` line — so the adapter switches on `evt.data.type`.
- **Initial-fetch retry** copies the `while(true)` retry skeleton verbatim from `anthropic.ts:259-288` /
  `ollama.ts:300-321` (retry only `postStream`; status-based error mapping is free via `postStream` →
  `throwMappedError` → `mapHttpError`).
- **Mid-stream errors terminate the stream silently** (TS convention — see Global Constraints §C). The
  Python `raise StreamError(state.error)` path is intentionally NOT ported; we wrap the SSE loop in
  `try { … } catch { return }` exactly like `ollama.ts:362-366`.
- **`chat()`** = `collectStream(this.stream(request))` then set `.model` (because `collectStream` returns
  `model: ''`), mirroring `anthropic.ts` chat semantics and the Python/Rust `chat()`.
- **Wiring**: new `Provider` variant `'chatgpt_codex'`, `ClientBuilder.chatgptCodex(...)` builder + a
  `buildProvider` arm, an `ENV_KEY_BY_PROVIDER` entry (`''`, like ollama), KEPT OUT of `HTTP_PROVIDERS`
  so no api-key is required, exported from `index.ts`.
- **Release**: `package.json` 0.10.0→0.11.0 (publish guard verifies tag == version), CHANGELOG 0.11.0
  entry, README + root AGENTS/llms/SKILL TS line.

## Tech Stack

- TypeScript (ESM, `NodeNext`), Node >= 18, native `fetch` + `ReadableStream`.
- Tests: **vitest** (`vitest run`), mocking the global `fetch` via `vi.stubGlobal('fetch', …)` and
  returning `new Response(ReadableStream, { status, headers })`.
- Gates (run in `sdks/typescript`): `npm run build` (tsc) → `npm run typecheck` (tsc --noEmit) →
  `npm test` (vitest run).
- One branch / one PR: `feat/ts-chatgpt-codex`.

---

## Global Constraints

### Locked decisions

1. **Auth = pre-obtained token.** `accessToken` + `accountId` are supplied directly to the constructor /
   builder. There is NO OAuth login flow and NO api-key for this provider (mirrors the Anthropic
   setup-token conceptual seam). `accessToken` → `authorization: Bearer …`; `accountId` →
   `chatgpt-account-id`.
2. **TypeScript only**, target **0.11.0**, purely additive. Do NOT touch `sdks/python` or `sdks/rust`.
3. **Mid-stream error handling = TS silent-terminate (NOT the Python mid-stream throw).** This is the one
   place the Python blueprint is deliberately not followed; rationale and proof in §C below.
4. **Reasoning-effort precedence**: per-request `request.providerOptions?.reasoning_effort` (only when a
   `string`) WINS → provider-level default `_reasoningEffort` → else OMIT the `reasoning` object entirely.
   A non-string per-request value (e.g. `5`) is ignored and falls through to the default.

### Gates (run in the worktree, in `sdks/typescript`)

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/ts-chatgpt-codex/sdks/typescript
npm run build      # tsc -p tsconfig.json  (pack-smoke needs dist/ — build FIRST)
npm run typecheck  # tsc -p tsconfig.json --noEmit
npm test           # vitest run
```

During TDD, scope vitest to the new file for fast red/green cycles, e.g.
`npx vitest run tests/providers-chatgpt-codex.test.ts`, then run the full `npm test` before each commit.

### A. Constants (exact)

| Name | Value |
|---|---|
| `DEFAULT_CHATGPT_CODEX_URL` (full POST endpoint, used verbatim — NOT a prefix, NO path appended) | `https://chatgpt.com/backend-api/codex/responses` |
| `DEFAULT_CHATGPT_CODEX_MODEL` (distinct from `DEFAULT_OPENAI_MODEL` = `gpt-5.3-codex`) | `gpt-5.5` |
| `CHATGPT_CODEX_ORIGINATOR` (header value) | `codex_cli_rs` |
| `Provider` union string + builder key | `chatgpt_codex` |

### B. Authoritative Responses request body (`buildResponsesBody(request, model)`)

Model precedence: `request.model ?? this.model`.

Instructions precedence (system prompt → TOP-LEVEL `instructions` STRING, never into `input`):
`systemBlocks` (joined `\n\n`, each `.trim()`ed, empties dropped) WINS over `system` string
(mutually exclusive — if `systemBlocks` is present, `system` is ignored). Then each `role: 'system'`
message's trimmed content is appended (joined `\n\n`). Fallback when nothing collected:
`'You are a helpful assistant.'`.

Static body fields (ALWAYS present):

```ts
{
  model,
  store: false,                              // HARD-REQUIRED by backend
  stream: true,
  instructions,                              // string (see above)
  input: inputItems,                         // see input-item table below
  include: ['reasoning.encrypted_content'],
  tool_choice: 'auto',
  parallel_tool_calls: true,
}
```

Input-item shapes (non-system messages, in message order):

| `message.role` | Emitted input item(s) |
|---|---|
| `user` | `{ type: 'message', role: 'user', content: [{ type: 'input_text', text: message.content }] }` |
| `assistant` (text, when `message.content` non-empty) | `{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: message.content }] }` |
| `assistant` (per tool call in `message.toolCalls ?? []`) | `{ type: 'function_call', call_id: tc.id, name: tc.name, arguments: JSON.stringify(tc.input) }` (`arguments` is a JSON-**encoded string**; `tc.input` is the field name) |
| `tool` (only when `message.toolCallId` is present) | `{ type: 'function_call_output', call_id: message.toolCallId, output: message.content }` |

Assistant text and assistant tool-calls are independent: text-only emits one `message` item; tool-only
(content `''`) emits ONLY `function_call` item(s) with NO `message` item; text+tools emits the `message`
item first, then each `function_call`.

Conditional fields:

- **tools** — only when `request.tools` is non-null AND maps to ≥1 entry (an empty `tools: []` OMITS
  the key). Flat Responses tool shape per tool:
  `{ type: 'function', name: tool.name, description: tool.description ?? null, parameters: tool.inputSchema ?? null, strict: null }`.
  `strict` is always JSON `null`. TS `Tool` has optional `description`/`inputSchema`; emit `?? null` so the
  wire matches the Python passthrough when present and is `null` when absent. (`strict: null` and the
  JSON-encoded `arguments` string are load-bearing wire details.)
- **reasoning** — per §Locked-decision 4: build `effort` = (string `request.providerOptions?.reasoning_effort`)
  ?? `this._reasoningEffort`; if `effort !== undefined` set `reasoning: { effort, summary: 'auto' }`, else omit.
- **temperature** — only when `request.temperature !== undefined`: `temperature: request.temperature`.

### Auth headers (EXACT, all names lowercase) + endpoint

```
authorization:      Bearer {accessToken}
chatgpt-account-id: {accountId}
originator:         codex_cli_rs
openai-beta:        responses=experimental
accept:             text/event-stream
content-type:       application/json
```

Endpoint: `POST {baseUrl}` — the base URL IS the full endpoint; nothing is appended. (`postStream` also
injects `content-type: application/json` and spreads caller headers, so passing all six is correct and
parity-safe.)

### C. TS streaming convention (the mid-stream-error decision — LOCKED to silent-terminate)

**Decision: on an `error` / `response.failed` SSE frame, and on any post-start stream-body error, the
generator TERMINATES SILENTLY (no throw, no synthesized terminal `done`).** The Python
`raise StreamError(state.error)` is intentionally NOT ported.

Why this is the correct TS port (evidence, not preference):

- LOCKED DECISION 3 says: mirror what the existing TS providers do; do NOT invent a Python-style
  mid-stream throw "unless the TS providers already throw there." They do not.
- `ollama.ts:362-366` wraps its entire post-fetch loop in `try { … } catch { return }` and comments
  "Ignore post-start stream body errors … end without synthesizing a terminal done event."
- `providers-ollama.test.ts:377` asserts this end-to-end: a 200 body that `controller.error()`s after one
  chunk makes the `for await` consumer's promise `.resolves.toBeUndefined()` — i.e. the generator ENDS,
  it does NOT reject.
- `anthropic.ts` / `openai.ts` / `gemini.ts` have NO mid-stream SSE-error inspection at all; they only
  `throw` on the INITIAL fetch (via `postStream` → `throwMappedError`) and otherwise always reach a
  terminal `done`.
- `provider.ts:133-138` (`readTimeoutStream`) documents the contract: "SILENTLY terminates … does NOT
  throw, matching the M1 mid-stream-failure swallow contract."

Concrete shape for `streamImpl`:

- Retry ONLY the initial `postStream` (the `while(true)` loop). After it succeeds, iterate
  `for await (const evt of parseSse(responseBody))` inside a `try { … } catch { return }`.
- On `evt.data.type === 'error' || 'response.failed'`: `return` from the generator (silent terminate).
  We do NOT track or surface the message, do NOT throw `StreamError`. (We still PARSE the message in a
  pure helper for a unit test that asserts extraction precedence, but the stream path discards it.)
- The single terminal `done` event is emitted ONLY on `response.completed` (followed by `return`).
- Add the defensive trailing terminal after the loop (`doneEvent()`) for EOF-without-`response.completed`,
  mirroring `anthropic.ts:386-391`. (`response.completed` `return`s before this; the trailing terminal
  only fires when the backend closes cleanly without a completed frame.)

### D. `response.*` SSE event → `StreamEvent` mapping (EXHAUSTIVE)

Driver: for each `evt` from `parseSse`, let `data = evt.data`; skip if `!data || data === '[DONE]' ||
typeof data !== 'object'`; then `switch (data.type)`. Adapter state across frames: `sawToolCall: boolean`
(and a `seenToolIds: Set<string>` for parity with Rust/Python; not load-bearing for any TS assertion).

| `data.type` | Condition | Emits (helper from `src/stream.ts`) / side effect |
|---|---|---|
| `response.output_text.delta` | `typeof data.delta === 'string' && data.delta` | `textEvent(data.delta)` |
| `response.reasoning_text.delta` **or** `response.reasoning_summary_text.delta` | `typeof data.delta === 'string' && data.delta` | `thinkingDelta(data.delta)` — eventType `'thinking_delta'` (TS has NO `'thinking'` type) |
| `response.output_item.added` | `data.item?.type === 'function_call'` AND `data.item.call_id` truthy | `toolCallStart(item.call_id, item.name ?? '')`; set `sawToolCall = true`; `seenToolIds.add(call_id)` |
| `response.function_call_arguments.delta` | TOP-LEVEL `data.item_id` truthy AND `typeof data.delta === 'string'` | `toolCallArgsWithId(data.item_id, data.delta)` (empty-string delta still emits; `item_id`, NOT `item.call_id`) |
| `response.output_item.done` | `data.item?.type === 'function_call'` AND `data.item.call_id` truthy | `toolCallEndWithId(item.call_id)` |
| `response.completed` | always | usage event (if `data.response.usage` is an object) then terminal `done` — see below — then `return` |
| `error` **or** `response.failed` | always | SILENT TERMINATE: `return` (no event, no throw — §C) |
| anything else (`response.created`, `response.in_progress`, `content_part.*`, `response.output_text.done`, reasoning item add/done, etc.) | — | ignored, no event |

`response.completed` detail:

```ts
const response = (typeof data.response === 'object' && data.response) || {}
const usage = response.usage
if (usage && typeof usage === 'object') {
  const inputTokens  = Number(usage.input_tokens  ?? 0)
  const outputTokens = Number(usage.output_tokens ?? 0)
  const cached       = Number(usage.input_tokens_details?.cached_tokens ?? 0)
  const u: Usage = { inputTokens, outputTokens }
  if (cached > 0) u.cacheReadInputTokens = cached     // surfaced AS-IS, NOT subtracted
  // cacheCreationInputTokens left undefined (omitted)
  yield usageEvent(u)
}
const status = response.status ?? 'completed'
const stop: StopReason =
  sawToolCall ? 'tool_use' : status === 'incomplete' ? 'max_tokens' : 'end_turn'
yield doneWithStopReason(stop)
return
```

- Usage event emitted ONLY when `response.usage` is an object; otherwise just the terminal `done`.
- `cached_tokens` surfaced as-is on `cacheReadInputTokens` only when `> 0`; `cacheCreationInputTokens`
  always undefined.
- Stop precedence: any tool call seen ⇒ `tool_use` (overrides status) → `incomplete` ⇒ `max_tokens`
  → else `end_turn`.

Error-message extraction (used ONLY by the pure-helper unit test, NOT by the stream path — first non-empty
wins): top-level `data.message` (string) → `data.response.error.message` (string) → `data.error.message`
(string) → fallback `'ChatGPT-backend stream error'`.

### E. Cross-file divergences a porter must respect

1. No `'thinking'` StreamEventType in TS — reasoning deltas map to `thinkingDelta()` (eventType
   `'thinking_delta'`); never emit `thinking_done` (this provider has no thinking-block-close frame;
   `collectStream` concatenates `thinking_delta` into `ChatResponse.thinking`).
2. Mid-stream error → SILENT TERMINATE (no throw). §C.
3. URL is the full endpoint — no `/v1/...` suffix, no model-in-path.
4. `DEFAULT_CHATGPT_CODEX_MODEL = 'gpt-5.5'` is a NEW constant, distinct from `DEFAULT_OPENAI_MODEL`.
5. No api-key — keep `'chatgpt_codex'` OUT of `HTTP_PROVIDERS`; builder requires `accessToken` +
   `accountId` (validated with `ConfigError`).
6. SSE test fixtures MUST separate frames with `\n\n` (the TS `parseSse` splits on `\n\n`).
7. `strict: null` on tools and `arguments` as a JSON-encoded string on `function_call` items.
8. Tool id wire keys differ by frame: `item.call_id` on `output_item.added`/`.done`; top-level `item_id`
   on `function_call_arguments.delta` — use `toolCallArgsWithId(item_id, delta)`.
9. `ENV_KEY_BY_PROVIDER` is a TOTAL `Record<ProviderName, string>` — adding the variant FORCES a key
   entry (`chatgpt_codex: ''`) or typecheck fails.

---

## Tasks (one branch `feat/ts-chatgpt-codex`)

Each code task is a full TDD cycle: write the failing vitest test → run it (RED) → write the full TS
implementation (no placeholders) → run it (GREEN) → run full gates → commit. Branch first:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/ts-chatgpt-codex
git checkout -b feat/ts-chatgpt-codex
```

All tests live in `sdks/typescript/tests/providers-chatgpt-codex.test.ts` (one file, multiple `describe`
blocks), and `sdks/typescript/tests/client-builder.test.ts` (T4 additions). All implementation lives in
`sdks/typescript/src/providers/chatgpt_codex.ts` (T1–T3) plus the wiring files (T4) and release files (T5).

---

### T1 — Responses body builder + reasoning-effort resolution

**Files:** `src/providers/chatgpt_codex.ts` (new), `tests/providers-chatgpt-codex.test.ts` (new).

**Step 1 (RED).** Create `tests/providers-chatgpt-codex.test.ts` with a `describe('ChatGptCodexProvider buildResponsesBody')`
block. To assert the body without HTTP, expose the builder for tests via a tiny capture: construct the
provider and call a test-visible method. Implementation choice: make `buildResponsesBody(request, model)`
a **public** method on the class (the OpenAI provider similarly exposes serialization-ish internals; making
it public is the cleanest test seam and matches how Python tests call `_build_responses_body` directly).

Add these tests (mirror Python `test_chatgpt_codex_request.py` + Rust `#[cfg(test)]`):

```ts
import { describe, it, expect } from 'vitest'
import { ChatGptCodexProvider, DEFAULT_CHATGPT_CODEX_MODEL } from '../src/providers/chatgpt_codex.js'
import type { ChatRequest } from '../src/types.js'

function p(): ChatGptCodexProvider {
  return new ChatGptCodexProvider('tok', 'acct')
}

describe('ChatGptCodexProvider buildResponsesBody', () => {
  it('uses default model gpt-5.5 and default base URL', () => {
    const prov = p()
    const body = prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, prov.modelId())
    expect(DEFAULT_CHATGPT_CODEX_MODEL).toBe('gpt-5.5')
    expect(body.model).toBe('gpt-5.5')
    expect(prov.endpointUrl()).toBe('https://chatgpt.com/backend-api/codex/responses')
  })

  it('per-request model overrides the default', () => {
    const prov = p()
    const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }], model: 'gpt-x' }
    const body = prov.buildResponsesBody(req, req.model ?? prov.modelId())
    expect(body.model).toBe('gpt-x')
  })

  it('sets the required codex fields and a single input_text user item', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(typeof body.instructions).toBe('string')
    expect(Array.isArray(body.input)).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.tool_choice).toBe('auto')
    expect(body.parallel_tool_calls).toBe(true)
    expect(body.input).toHaveLength(1)
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'user',
      content: [{ type: 'input_text', text: 'hi' }],
    })
    expect(body.tools).toBeUndefined()
    expect(body.reasoning).toBeUndefined()
    expect(body.temperature).toBeUndefined()
  })

  it('falls back to the default instructions when nothing is supplied', () => {
    const body = p().buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5')
    expect(body.instructions).toBe('You are a helpful assistant.')
  })

  it('routes a system message to instructions, not input', () => {
    const req: ChatRequest = {
      messages: [
        { role: 'system', content: 'be terse' },
        { role: 'user', content: 'hi' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect(body.input[0].role).toBe('user')
  })

  it('uses the system field for instructions', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], system: 'sys here' },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('sys here')
  })

  it('prefers systemBlocks over system, joined with \\n\\n', () => {
    const body = p().buildResponsesBody(
      {
        messages: [{ role: 'user', content: 'hi' }],
        system: 'ignored',
        systemBlocks: [{ text: 'a' }, { text: '  ' }, { text: 'b' }],
      },
      'gpt-5.5',
    )
    expect(body.instructions).toBe('a\n\nb')
  })

  it('emits an output_text item for assistant text', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'assistant', content: 'prior answer' }] },
      'gpt-5.5',
    )
    expect(body.input[0]).toEqual({
      type: 'message',
      role: 'assistant',
      content: [{ type: 'output_text', text: 'prior answer' }],
    })
  })

  it('emits function_call + function_call_output for a tool round trip', () => {
    const req: ChatRequest = {
      messages: [
        {
          role: 'assistant',
          content: '',
          toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Paris' } }],
        },
        { role: 'tool', content: '{"temp":20}', toolCallId: 'call_1' },
      ],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.input[0]).toEqual({
      type: 'function_call',
      call_id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
    expect(body.input[1]).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: '{"temp":20}',
    })
  })

  it('maps tools to the flat Responses shape with strict:null', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      tools: [{ name: 'get_weather', description: 'gets weather', inputSchema: { type: 'object' } }],
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.tools).toEqual([
      {
        type: 'function',
        name: 'get_weather',
        description: 'gets weather',
        parameters: { type: 'object' },
        strict: null,
      },
    ])
  })

  it('omits the tools key for an empty tools list', () => {
    const body = p().buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], tools: [] },
      'gpt-5.5',
    )
    expect(body.tools).toBeUndefined()
  })

  it('emits reasoning and temperature when supplied', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      temperature: 0.3,
      providerOptions: { reasoning_effort: 'high' },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect(body.temperature).toBe(0.3)
  })

  it('omits reasoning when the per-request effort is not a string', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      providerOptions: { reasoning_effort: 5 },
    }
    const body = p().buildResponsesBody(req, 'gpt-5.5')
    expect(body.reasoning).toBeUndefined()
  })

  it('emits the provider-default reasoning effort, overridden by a per-request value', () => {
    const def = p().reasoningEffort('medium')
    expect(def.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toEqual({ effort: 'medium', summary: 'auto' })
    // per-request wins
    const body = def.buildResponsesBody(
      { messages: [{ role: 'user', content: 'hi' }], providerOptions: { reasoning_effort: 'high' } },
      'gpt-5.5',
    )
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('reasoningEffort(undefined) clears the default and the setter returns this', () => {
    const prov = p()
    expect(prov.reasoningEffort('high')).toBe(prov) // returns this
    prov.reasoningEffort(undefined)
    expect(prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-5.5').reasoning)
      .toBeUndefined()
  })
})
```

Run `npx vitest run tests/providers-chatgpt-codex.test.ts` — RED (module does not exist).

**Step 2 (GREEN).** Create `src/providers/chatgpt_codex.ts` with the constants, the class skeleton, the
PUBLIC `buildResponsesBody`, the `modelId()` / `endpointUrl()` test accessors, and `reasoningEffort()`.
Full implementation:

```ts
import {
  isRetryableNetworkError,
  isRetryableStatus,
} from '../error.js'
import { postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { textOnly, type ProviderCapabilities } from '../provider.js'
import { RetryPolicy } from '../retry.js'
import {
  collectStream,
  doneEvent,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxStream,
} from '../stream.js'
import type {
  ChatRequest,
  ChatResponse,
  StopReason,
  StreamEvent,
  Usage,
} from '../types.js'
import { DEFAULT_CHATGPT_CODEX_MODEL } from '../models.js'

export const DEFAULT_CHATGPT_CODEX_URL = 'https://chatgpt.com/backend-api/codex/responses'
const CHATGPT_CODEX_ORIGINATOR = 'codex_cli_rs'

export { DEFAULT_CHATGPT_CODEX_MODEL }

export class ChatGptCodexProvider {
  private readonly model: string
  private readonly baseUrl: string
  private retryPolicy: RetryPolicy
  private _reasoningEffort?: string

  constructor(
    private readonly accessToken: string,
    private readonly accountId: string,
    model?: string,
    baseUrl: string = DEFAULT_CHATGPT_CODEX_URL,
  ) {
    this.model = model ?? DEFAULT_CHATGPT_CODEX_MODEL
    this.baseUrl = baseUrl
    this.retryPolicy = RetryPolicy.default()
  }

  withRetryPolicy(policy: RetryPolicy): this {
    this.retryPolicy = policy
    return this
  }

  /** Set the provider-default reasoning effort. Pass undefined to clear. Returns this. */
  reasoningEffort(effort: string | undefined): this {
    this._reasoningEffort = effort
    return this
  }

  /** Test/introspection accessor: the resolved default model id. */
  modelId(): string {
    return this.model
  }

  /** The full POST endpoint (base URL verbatim). */
  endpointUrl(): string {
    return this.baseUrl
  }

  capabilities(): ProviderCapabilities {
    return textOnly()
  }

  private headers(): Record<string, string> {
    return {
      authorization: `Bearer ${this.accessToken}`,
      'chatgpt-account-id': this.accountId,
      originator: CHATGPT_CODEX_ORIGINATOR,
      'openai-beta': 'responses=experimental',
      accept: 'text/event-stream',
      'content-type': 'application/json',
    }
  }

  /** Build the OpenAI Responses request body. Public for unit tests. */
  buildResponsesBody(request: ChatRequest, model: string): Record<string, any> {
    const instructionsParts: string[] = []
    if (request.systemBlocks !== undefined) {
      for (const block of request.systemBlocks) {
        const trimmed = block.text.trim()
        if (trimmed) instructionsParts.push(trimmed)
      }
    } else if (request.system !== undefined) {
      const trimmed = request.system.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }

    const inputItems: Array<Record<string, any>> = []
    for (const message of request.messages) {
      switch (message.role) {
        case 'system': {
          const trimmed = message.content.trim()
          if (trimmed) instructionsParts.push(trimmed)
          break
        }
        case 'user':
          inputItems.push({
            type: 'message',
            role: 'user',
            content: [{ type: 'input_text', text: message.content }],
          })
          break
        case 'assistant': {
          if (message.content) {
            inputItems.push({
              type: 'message',
              role: 'assistant',
              content: [{ type: 'output_text', text: message.content }],
            })
          }
          for (const tc of message.toolCalls ?? []) {
            inputItems.push({
              type: 'function_call',
              call_id: tc.id,
              name: tc.name,
              arguments: JSON.stringify(tc.input),
            })
          }
          break
        }
        case 'tool':
          if (message.toolCallId !== undefined) {
            inputItems.push({
              type: 'function_call_output',
              call_id: message.toolCallId,
              output: message.content,
            })
          }
          break
      }
    }

    const instructions =
      instructionsParts.length > 0 ? instructionsParts.join('\n\n') : 'You are a helpful assistant.'

    const body: Record<string, any> = {
      model,
      store: false,
      stream: true,
      instructions,
      input: inputItems,
      include: ['reasoning.encrypted_content'],
      tool_choice: 'auto',
      parallel_tool_calls: true,
    }

    if (request.tools !== undefined) {
      const mapped = request.tools.map((tool) => ({
        type: 'function',
        name: tool.name,
        description: tool.description ?? null,
        parameters: tool.inputSchema ?? null,
        strict: null,
      }))
      if (mapped.length > 0) body.tools = mapped
    }

    let effort: string | undefined
    const candidate = request.providerOptions?.reasoning_effort
    if (typeof candidate === 'string') effort = candidate
    if (effort === undefined) effort = this._reasoningEffort
    if (effort !== undefined) body.reasoning = { effort, summary: 'auto' }

    if (request.temperature !== undefined) body.temperature = request.temperature

    return body
  }

  // chat()/stream() land in T2/T3.
}
```

Add the model constant in `src/models.ts` so the import resolves (this is the T4 `models.ts` edit pulled
forward; that's fine — it's additive):

```ts
/** ChatGPT-Codex model IDs */
export const CHATGPT_CODEX_MODELS = ['gpt-5.5'] as const

/** Default ChatGPT-Codex model (distinct from DEFAULT_OPENAI_MODEL = 'gpt-5.3-codex') */
export const DEFAULT_CHATGPT_CODEX_MODEL = 'gpt-5.5'
```

Run `npx vitest run tests/providers-chatgpt-codex.test.ts` — GREEN.

**Step 3.** Run full gates (`npm run build && npm run typecheck && npm test`). Commit:
`feat(ts-chatgpt-codex): Responses body builder + reasoning-effort resolution`.

**Done criteria:** all T1 tests pass; `npm run typecheck` clean; body matches §B exactly (store:false,
stream:true, include, tool_choice, parallel_tool_calls; instructions precedence; input item shapes;
tools/reasoning/temperature conditionals; reasoning precedence + clear).

---

### T2 — `response.*` SSE adapter (text / thinking / tool triplet / usage / terminal / error)

**Files:** `src/providers/chatgpt_codex.ts` (add the adapter + a pure `parseErrorMessage` helper),
`tests/providers-chatgpt-codex.test.ts` (add `describe('ChatGptCodexProvider SSE adapter')`).

The adapter logic lives inside `streamImpl` (T3), but T2 tests it end-to-end through `provider.stream(req)`
with a mocked fetch (this is how the existing providers test their adapters — there is no standalone
adapter export). So T2 lands BOTH the adapter switch AND a minimal `streamImpl` that does the initial
`postStream` + the SSE loop. T3 then adds `chat()` and the header/body-on-the-wire assertions and the
retry/HTTP-error tests. (The split keeps each test focused; both touch the same file.)

**Step 1 (RED).** Add to the test file the harness (copied verbatim from `providers-anthropic.test.ts:230-251`)
and the adapter tests:

```ts
import { describe, it, expect, afterEach, vi } from 'vitest'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

function streamFromTranscript(
  sse: string,
  onRequest?: (url: string, options?: RequestInit) => void,
): void {
  const bytes = new TextEncoder().encode(sse)
  const mockStream = new ReadableStream<Uint8Array>({
    start(controller) {
      controller.enqueue(bytes)
      controller.close()
    },
  })
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string, options?: RequestInit) => {
      onRequest?.(url, options)
      return new Response(mockStream, {
        status: 200,
        headers: { 'content-type': 'text/event-stream' },
      })
    }),
  )
}

const REQ: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }

async function collect(sse: string): Promise<StreamEvent[]> {
  streamFromTranscript(sse)
  const prov = new ChatGptCodexProvider('tok', 'acct')
  const events: StreamEvent[] = []
  for await (const e of prov.stream(REQ)) events.push(e)
  return events
}

describe('ChatGptCodexProvider SSE adapter', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('ignores empty/[DONE]/malformed/unknown frames and empty text deltas', async () => {
    const sse =
      'data: \n\n' +
      'data: [DONE]\n\n' +
      'data: {not json}\n\n' +
      'data: {"type":"response.unknown"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":""}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    // only the terminal done
    expect(events).toHaveLength(1)
    expect(events[0].done).toBe(true)
    expect(events[0].stopReason).toBe('end_turn')
  })

  it('concatenates text deltas and ends with end_turn', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"Hello, "}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"world"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    expect(events.filter((e) => e.eventType === 'text').map((e) => e.content)).toEqual([
      'Hello, ',
      'world',
    ])
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'end_turn' })
  })

  it('emits a usage event from response.completed (cacheRead omitted when absent)', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":12,"output_tokens":7}}}\n\n'
    const events = await collect(sse)
    const usage = events.find((e) => e.eventType === 'usage')
    expect(usage?.usage).toEqual({ inputTokens: 12, outputTokens: 7 })
    expect(usage?.usage?.cacheReadInputTokens).toBeUndefined()
  })

  it('surfaces cached_tokens as-is (not subtracted)', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":100,"output_tokens":5,"input_tokens_details":{"cached_tokens":30}}}}\n\n'
    const events = await collect(sse)
    const usage = events.find((e) => e.eventType === 'usage')?.usage
    expect(usage?.inputTokens).toBe(100)
    expect(usage?.cacheReadInputTokens).toBe(30)
  })

  it('maps both reasoning delta types to thinking_delta', async () => {
    const sse =
      'data: {"type":"response.reasoning_text.delta","delta":"plan "}\n\n' +
      'data: {"type":"response.reasoning_summary_text.delta","delta":"ahead"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    expect(events.filter((e) => e.eventType === 'thinking_delta').map((e) => e.content)).toEqual([
      'plan ',
      'ahead',
    ])
  })

  it('runs the function_call lifecycle and ends with tool_use', async () => {
    const sse =
      'data: {"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_42","name":"get_weather"}}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"call_42","delta":"{\\"city\\":"}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"call_42","delta":"\\"Paris\\"}"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_42"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    const events = await collect(sse)
    const tool = events.filter((e) => e.eventType.startsWith('tool_call'))
    expect(tool[0]).toMatchObject({
      eventType: 'tool_call_start',
      toolCallId: 'call_42',
      toolCallName: 'get_weather',
    })
    expect(tool[1]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_42' })
    expect(tool[2]).toMatchObject({ eventType: 'tool_call_args', toolCallId: 'call_42' })
    expect(tool[3]).toMatchObject({ eventType: 'tool_call_end', toolCallId: 'call_42' })
    const argText = tool
      .filter((e) => e.eventType === 'tool_call_args')
      .map((e) => e.toolCallArgsDelta)
      .join('')
    expect(argText).toBe('{"city":"Paris"}')
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'tool_use' })
  })

  it('maps status:"incomplete" to max_tokens', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"incomplete"}}\n\n'
    const events = await collect(sse)
    expect(events[events.length - 1]).toMatchObject({ done: true, stopReason: 'max_tokens' })
  })

  it('terminates silently (no throw) on a top-level error frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"error","message":"rate limited"}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    await expect(
      (async () => {
        for await (const e of prov.stream(REQ)) events.push(e)
      })(),
    ).resolves.toBeUndefined()
    // the partial text was yielded; NO terminal done; NO throw
    expect(events).toEqual([{ content: 'partial', done: false, eventType: 'text' }])
  })

  it('terminates silently on a response.failed frame', async () => {
    streamFromTranscript(
      'data: {"type":"response.failed","response":{"error":{"message":"boom"}}}\n\n',
    )
    const prov = new ChatGptCodexProvider('tok', 'acct')
    const events: StreamEvent[] = []
    await expect(
      (async () => {
        for await (const e of prov.stream(REQ)) events.push(e)
      })(),
    ).resolves.toBeUndefined()
    expect(events).toHaveLength(0)
  })
})
```

Run `npx vitest run tests/providers-chatgpt-codex.test.ts` — RED (`stream` is not yet implemented).

**Step 2 (GREEN).** Add the adapter + minimal `streamImpl`/`stream` to `chatgpt_codex.ts`. Append inside
the class:

```ts
  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  private async *streamImpl(request: ChatRequest): AsyncGenerator<StreamEvent> {
    const model = request.model ?? this.model
    const body = this.buildResponsesBody(request, model)
    const headers = this.headers()

    // Retry ONLY the initial fetch (mirrors anthropic.ts:259-288 / ollama.ts:300-321).
    let attempt = 0
    let responseBody: ReadableStream<Uint8Array>
    while (true) {
      try {
        responseBody = await postStream(this.baseUrl, headers, body)
        break
      } catch (error) {
        const status = (error as { status?: number }).status
        const retryable =
          (status !== undefined && isRetryableStatus(status)) || isRetryableNetworkError(error)
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

    let sawToolCall = false
    const seenToolIds = new Set<string>()

    // Mid-stream body errors terminate the stream SILENTLY (TS convention;
    // ollama.ts:362-366, providers-ollama.test.ts:377). NO mid-stream throw.
    try {
      for await (const evt of parseSse(responseBody)) {
        const data = evt.data
        if (!data || data === '[DONE]' || typeof data !== 'object') continue

        switch (data.type) {
          case 'response.output_text.delta': {
            const delta = data.delta
            if (typeof delta === 'string' && delta) yield textEvent(delta)
            break
          }
          case 'response.reasoning_text.delta':
          case 'response.reasoning_summary_text.delta': {
            const delta = data.delta
            if (typeof delta === 'string' && delta) yield thinkingDelta(delta)
            break
          }
          case 'response.output_item.added': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              sawToolCall = true
              seenToolIds.add(String(item.call_id))
              yield toolCallStart(String(item.call_id), String(item.name ?? ''))
            }
            break
          }
          case 'response.function_call_arguments.delta': {
            const itemId = data.item_id
            const delta = data.delta
            if (itemId && typeof delta === 'string') {
              yield toolCallArgsWithId(String(itemId), delta)
            }
            break
          }
          case 'response.output_item.done': {
            const item = data.item
            if (item && item.type === 'function_call' && item.call_id) {
              yield toolCallEndWithId(String(item.call_id))
            }
            break
          }
          case 'response.completed': {
            const response =
              data.response && typeof data.response === 'object' ? data.response : {}
            const usage = response.usage
            if (usage && typeof usage === 'object') {
              const u: Usage = {
                inputTokens: Number(usage.input_tokens ?? 0),
                outputTokens: Number(usage.output_tokens ?? 0),
              }
              const cached = Number(usage.input_tokens_details?.cached_tokens ?? 0)
              if (cached > 0) u.cacheReadInputTokens = cached
              yield usageEvent(u)
            }
            const status = response.status ?? 'completed'
            const stop: StopReason = sawToolCall
              ? 'tool_use'
              : status === 'incomplete'
                ? 'max_tokens'
                : 'end_turn'
            yield doneWithStopReason(stop)
            return
          }
          case 'error':
          case 'response.failed':
            // Silent terminate (TS convention). The Python `raise StreamError`
            // path is intentionally NOT ported. See plan §C.
            return
          default:
            break
        }
      }
    } catch {
      // Ignore post-start stream-body errors; end without a terminal done
      // (mirrors ollama.ts:362-366).
      return
    }

    // Defensive terminal for a clean EOF without response.completed
    // (mirrors anthropic.ts:386-391). response.completed returns earlier.
    yield doneEvent()
  }
```

Add a pure error-message extraction helper (exported for a focused unit test that documents the
precedence — even though the stream path discards the message):

```ts
/**
 * Extract the error message from an `error` / `response.failed` Responses frame.
 * First non-empty wins: top-level `message` → `response.error.message` →
 * `error.message` → fallback. Pure; used by tests (the stream path silently
 * terminates without surfacing this — plan §C).
 */
export function chatGptCodexErrorMessage(chunk: any): string {
  if (typeof chunk?.message === 'string' && chunk.message) return chunk.message
  const nested = chunk?.response?.error?.message
  if (typeof nested === 'string' && nested) return nested
  const top = chunk?.error?.message
  if (typeof top === 'string' && top) return top
  return 'ChatGPT-backend stream error'
}
```

Add the matching unit test (no fetch needed):

```ts
import { chatGptCodexErrorMessage } from '../src/providers/chatgpt_codex.js'

describe('chatGptCodexErrorMessage', () => {
  it('prefers the top-level message', () => {
    expect(chatGptCodexErrorMessage({ type: 'error', message: 'rate limited' })).toBe('rate limited')
  })
  it('reads the nested response.error.message', () => {
    expect(
      chatGptCodexErrorMessage({ type: 'response.failed', response: { error: { message: 'boom' } } }),
    ).toBe('boom')
  })
  it('reads the error.message branch', () => {
    expect(chatGptCodexErrorMessage({ type: 'error', error: { message: 'nope' } })).toBe('nope')
  })
  it('falls back when no message is present', () => {
    expect(chatGptCodexErrorMessage({ type: 'error' })).toBe('ChatGPT-backend stream error')
  })
})
```

Run `npx vitest run tests/providers-chatgpt-codex.test.ts` — GREEN.

**Step 3.** Full gates, then commit: `feat(ts-chatgpt-codex): response.* SSE adapter (silent-terminate on error)`.

**Done criteria:** all T2 tests pass — text concat, both reasoning delta types → `thinking_delta`, tool
triplet with concatenated args, usage (cacheRead absent vs as-is), `incomplete`→`max_tokens`,
`tool_use` override, AND the two silent-terminate tests (`error` and `response.failed`) resolving to
`undefined` (NO throw, exactly mirroring `providers-ollama.test.ts:377`). Error-message extraction unit
tests pass.

---

### T3 — Provider class: auth headers, base URL, capabilities, chat()=collectStream(stream()), HTTP errors

**Files:** `src/providers/chatgpt_codex.ts` (add `chat()`), `tests/providers-chatgpt-codex.test.ts`
(add `describe('ChatGptCodexProvider HTTP')`).

**Step 1 (RED).** Add tests for: headers/body on the wire, `chat()` collecting the stream, `chat()`
thinking, `chat()` tool-call lifecycle, capabilities text-only, and HTTP-error mapping (401/429/500 +
transport error). Use the harness from T2 for streaming and a JSON/error mock for errors. For error
tests, set a zero-retry policy so 429/500 surface immediately without real delays.

```ts
import { AuthError, RateLimitError, ProviderError, NetworkError } from '../src/error.js'
import { RetryPolicy } from '../src/retry.js'

describe('ChatGptCodexProvider HTTP', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('sends the six codex headers and the responses body', async () => {
    let url = ''
    let headers: Record<string, string> = {}
    let body: any = null
    streamFromTranscript(
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
      (u, opts) => {
        url = u
        headers = (opts?.headers as Record<string, string>) ?? {}
        body = JSON.parse(String(opts?.body ?? '{}'))
      },
    )
    const prov = new ChatGptCodexProvider('my-token', 'acct-123')
    for await (const _ of prov.stream(REQ)) { /* drain */ }

    expect(url).toBe('https://chatgpt.com/backend-api/codex/responses')
    expect(headers.authorization).toBe('Bearer my-token')
    expect(headers['chatgpt-account-id']).toBe('acct-123')
    expect(headers.originator).toBe('codex_cli_rs')
    expect(headers['openai-beta']).toBe('responses=experimental')
    expect(headers.accept).toBe('text/event-stream')
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.input[0].type).toBe('message')
    expect(body.input[0].content[0].type).toBe('input_text')
  })

  it('has text-only capabilities', () => {
    expect(new ChatGptCodexProvider('t', 'a').capabilities()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
    })
  })

  it('chat() collects the stream into a ChatResponse', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_text.delta","delta":"Hi there"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":2}}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.content).toBe('Hi there')
    expect(resp.usage).toEqual({ inputTokens: 4, outputTokens: 2 })
    expect(resp.model).toBe('gpt-5.5')
    expect(resp.stopReason).toBe('end_turn')
  })

  it('chat() honors the per-request model in the result', async () => {
    streamFromTranscript(
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat({ ...REQ, model: 'gpt-x' })
    expect(resp.model).toBe('gpt-x')
  })

  it('chat() surfaces thinking', async () => {
    streamFromTranscript(
      'data: {"type":"response.reasoning_text.delta","delta":"plan "}\n\n' +
        'data: {"type":"response.reasoning_summary_text.delta","delta":"ahead"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.thinking).toBe('plan ahead')
  })

  it('chat() yields a tool call from the lifecycle', async () => {
    streamFromTranscript(
      'data: {"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_9","name":"lookup"}}\n\n' +
        'data: {"type":"response.function_call_arguments.delta","item_id":"call_9","delta":"{\\"q\\":\\"x\\"}"}\n\n' +
        'data: {"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_9"}}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const resp = await new ChatGptCodexProvider('t', 'a').chat(REQ)
    expect(resp.stopReason).toBe('tool_use')
    expect(resp.toolCalls).toHaveLength(1)
    expect(resp.toolCalls[0]).toMatchObject({ id: 'call_9', name: 'lookup', input: { q: 'x' } })
  })

  function stubError(status: number): void {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(JSON.stringify({ error: { message: `err ${status}` } }), {
          status,
          headers: { 'content-type': 'application/json' },
        }),
      ),
    )
  }

  const noRetry = new RetryPolicy({ maxRetries: 0 })

  it('maps 401 to AuthError', async () => {
    stubError(401)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) { /* drain */ }
      })(),
    ).rejects.toBeInstanceOf(AuthError)
  })

  it('maps 429 to RateLimitError', async () => {
    stubError(429)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) { /* drain */ }
      })(),
    ).rejects.toBeInstanceOf(RateLimitError)
  })

  it('maps 500 to ProviderError', async () => {
    stubError(500)
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) { /* drain */ }
      })(),
    ).rejects.toBeInstanceOf(ProviderError)
  })

  it('propagates a transport error', async () => {
    vi.stubGlobal('fetch', vi.fn(async () => {
      throw new NetworkError('socket hangup')
    }))
    const prov = new ChatGptCodexProvider('t', 'a').withRetryPolicy(noRetry)
    await expect(
      (async () => {
        for await (const _ of prov.stream(REQ)) { /* drain */ }
      })(),
    ).rejects.toBeInstanceOf(NetworkError)
  })
})
```

Run — RED (`chat` not yet implemented; capability/header tests may pass already from T2's `stream`, but
`chat` tests fail).

**Step 2 (GREEN).** Add `chat()` to the class:

```ts
  async chat(request: ChatRequest): Promise<ChatResponse> {
    const model = request.model ?? this.model
    const response = await collectStream(this.stream(request))
    if (!response.model) response.model = model
    return response
  }
```

Run — GREEN.

Notes on the error tests: `postStream` calls `throwMappedError` on non-2xx, producing
`AuthError`(401)/`RateLimitError`(429)/`ProviderError`(500) with `.status` set. With `maxRetries: 0`,
the retry loop rethrows immediately (401 is non-retryable anyway; 429/500 are retryable but the
`attempt >= 0` guard rethrows on the first failure). The transport `NetworkError` is thrown by the mocked
`fetch` and, with `maxRetries: 0`, rethrown by the loop. (`NetworkError` is `instanceof Error`; the
`isRetryableNetworkError` check sees its `name === 'NetworkError'`, not retryable — but `maxRetries: 0`
short-circuits regardless.)

**Step 3.** Full gates, then commit: `feat(ts-chatgpt-codex): provider class chat()/headers + HTTP error mapping`.

**Done criteria:** all T3 tests pass — six headers + responses body on the wire, `chat()` content/usage/
model/stopReason, per-request model in result, `chat()` thinking, `chat()` tool call (`input` parsed from
concatenated args), `textOnly()` capabilities, and 401→AuthError / 429→RateLimitError / 500→ProviderError /
transport→NetworkError.

---

### T4 — Wiring: Provider variant, ClientBuilder.chatgptCodex(...), no-api-key, exports + dispatch tests

**Files:** `src/provider.ts`, `src/client.ts`, `src/index.ts`, `src/models.ts` (export the new constants),
`tests/client-builder.test.ts` (add a `describe`), `tests/index.test.ts` (export assertion).

**Step 1 (RED).** Add wiring tests to `tests/client-builder.test.ts`:

```ts
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'

describe('ClientBuilder.chatgptCodex', () => {
  it('builds a Client without an api key', () => {
    const client = new ClientBuilder().chatgptCodex('tok', 'acct').build()
    expect(client).toBeInstanceOf(Client)
  })

  it('constructs a ChatGptCodexProvider with the given token/accountId/model', () => {
    const prov = new ClientBuilder()
      .chatgptCodex('tok', 'acct', 'gpt-x')
      .buildProviderForTest()
    expect(prov).toBeInstanceOf(ChatGptCodexProvider)
    expect((prov as ChatGptCodexProvider).modelId()).toBe('gpt-x')
  })

  it('threads the reasoning effort default', () => {
    const prov = new ClientBuilder()
      .chatgptCodex('tok', 'acct', undefined, { reasoningEffort: 'high' })
      .buildProviderForTest() as ChatGptCodexProvider
    const body = prov.buildResponsesBody({ messages: [{ role: 'user', content: 'hi' }] }, prov.modelId())
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('does not require an api key (no ConfigError)', () => {
    expect(() => new ClientBuilder().chatgptCodex('tok', 'acct').build()).not.toThrow()
  })
})
```

> `buildProviderForTest()` does not exist yet. Add a tiny public test seam to `ClientBuilder` (or reuse an
> existing one if present). Implementation in Step 2 exposes
> `buildProviderForTest(): DispatchProvider { return this.buildProvider(this._provider!, this._apiKey ?? '') }`.
> (If the team prefers no test-only method, assert via `.build()` succeeding plus a `provider.stream`
> smoke with a mocked fetch asserting the codex headers — but the explicit seam keeps the dispatch test
> focused, matching how `client-builder.test.ts` already inspects built providers.)

Add to `tests/index.test.ts` an export assertion:

```ts
import * as sdk from '../src/index.js'
it('exports ChatGptCodexProvider and its default model', () => {
  expect(typeof sdk.ChatGptCodexProvider).toBe('function')
  expect(sdk.DEFAULT_CHATGPT_CODEX_MODEL).toBe('gpt-5.5')
})
```

Run `npx vitest run tests/client-builder.test.ts tests/index.test.ts` — RED.

**Step 2 (GREEN).** Apply the wiring edits:

**`src/provider.ts:100`** — extend the union:
```ts
export type Provider = 'anthropic' | 'openai' | 'minimax' | 'ollama' | 'gemini' | 'chatgpt_codex'
```

**`src/models.ts`** — already added `CHATGPT_CODEX_MODELS` / `DEFAULT_CHATGPT_CODEX_MODEL` in T1; no
further change (verify they exist).

**`src/client.ts`:**
1. Import: `import { ChatGptCodexProvider } from './providers/chatgpt_codex.js'`.
2. `ENV_KEY_BY_PROVIDER` — add `chatgpt_codex: ''` (keeps the total `Record<ProviderName, string>` valid;
   no env key, like `ollama`). DO NOT add to `HTTP_PROVIDERS`.
3. Add builder fields near the other protected fields:
   ```ts
   protected _chatgptAccessToken?: string
   protected _chatgptAccountId?: string
   protected _chatgptReasoningEffort?: string
   ```
4. Add the fluent builder method (alongside the other setters):
   ```ts
   chatgptCodex(
     accessToken: string,
     accountId: string,
     model?: string,
     opts?: { reasoningEffort?: string },
   ): this {
     this._provider = 'chatgpt_codex'
     this._chatgptAccessToken = accessToken
     this._chatgptAccountId = accountId
     if (model !== undefined) this._model = model
     this._chatgptReasoningEffort = opts?.reasoningEffort
     return this
   }
   ```
5. Add a `buildProvider` arm BEFORE the final `return new MinimaxProvider(...)`:
   ```ts
   if (provider === 'chatgpt_codex') {
     const codex = new ChatGptCodexProvider(
       this._chatgptAccessToken ?? '',
       this._chatgptAccountId ?? '',
       this._model,
     ).withRetryPolicy(this._retryPolicy)
     return this._chatgptReasoningEffort !== undefined
       ? codex.reasoningEffort(this._chatgptReasoningEffort)
       : codex
   }
   ```
6. Add the test seam (public method on `ClientBuilder`):
   ```ts
   /** Test-only: build the provider without wrapping it in a Client. */
   buildProviderForTest(): DispatchProvider {
     if (!this._provider) throw new ConfigError('provider is required')
     return this.buildProvider(this._provider, this._apiKey ?? '')
   }
   ```
   (`ChatGptCodexProvider` structurally satisfies `DispatchProvider` via `capabilities()/chat()/stream()`,
   so the arm returning it typechecks. The legacy `Client` constructor needs NO `chatgpt_codex` arm — the
   builder is the supported construction path, like Ollama tuning.)

**`src/index.ts`** — add the export and the explicit constants re-export:
```ts
export * from './providers/chatgpt_codex.js'
```
And add `DEFAULT_CHATGPT_CODEX_MODEL` / `CHATGPT_CODEX_MODELS` to the `from './models.js'` block:
```ts
export {
  // ...existing...
  DEFAULT_CHATGPT_CODEX_MODEL,
  CHATGPT_CODEX_MODELS,
} from './models.js'
```

Run `npx vitest run tests/client-builder.test.ts tests/index.test.ts` — GREEN.

**Step 3.** Full gates (the union change forces `ENV_KEY_BY_PROVIDER` totality — typecheck proves it).
Commit: `feat(ts-chatgpt-codex): wire Provider variant + ClientBuilder.chatgptCodex + exports`.

**Done criteria:** `Provider` union includes `'chatgpt_codex'`; `ENV_KEY_BY_PROVIDER` total (typecheck
clean); `'chatgpt_codex'` NOT in `HTTP_PROVIDERS` (builds without an api key — no `ConfigError`);
`ClientBuilder.chatgptCodex(...)` builds a `ChatGptCodexProvider` with token/accountId/model + reasoning
effort threaded; `ChatGptCodexProvider` + `DEFAULT_CHATGPT_CODEX_MODEL` exported from `index.ts`. Full
`npm test` green.

---

### T5 — Release 0.11.0

**Files:** `sdks/typescript/package.json`, `sdks/typescript/CHANGELOG.md`, `sdks/typescript/README.md`,
root `AGENTS.md`, root `llms.txt`, `skills/motosan-ai/SKILL.md`. (Docs/release files — no code; per the
project's PR rules these may ship in the same `feat/ts-chatgpt-codex` PR alongside the code.)

**Step 1.** Bump `package.json:3`:
```json
"version": "0.11.0",
```
(The publish guard in `.github/workflows/publish-typescript.yml:28-32` verifies `ts-v0.11.0` ==
`package.json` version; this bump is mandatory before tagging.)

**Step 2.** Prepend a CHANGELOG entry above `## [0.10.0] - 2026-06-07`:
```markdown
## [0.11.0] - 2026-06-23

### Added

- `ChatGptCodexProvider`: streams the OpenAI Responses API at
  `https://chatgpt.com/backend-api/codex/responses` using a caller-supplied OAuth `accessToken` +
  `accountId` (codex CLI headers; no api key required). Maps typed `response.*` SSE to the
  `StreamEvent` taxonomy (text, reasoning → `thinking_delta`, function-call tool lifecycle, usage,
  terminal stop). Mid-stream error frames terminate the stream silently (TS convention).
- New `Provider` variant `'chatgpt_codex'` and `Client.builder().chatgptCodex(accessToken, accountId,
  model?, { reasoningEffort? })`. Per-request `providerOptions.reasoning_effort` (string) overrides the
  provider default; otherwise `reasoning` is omitted.
- `DEFAULT_CHATGPT_CODEX_MODEL` (`gpt-5.5`) and `CHATGPT_CODEX_MODELS` exported.
```

**Step 3.** README additions:
- Intro provider list / "providers" count: add ChatGPT Codex (five → six providers).
- Add a `### ChatGPT Codex` block under `## Providers` showing
  `Client.builder().chatgptCodex(accessToken, accountId).build()` and noting no api key, text-only, the
  default model `gpt-5.5`, and the reasoning-effort option.
- Default-models table: add `chatgpt_codex` → `gpt-5.5`.
- Release example line `ts-v0.10.0` → `ts-v0.11.0` (freshness; not load-bearing).

**Step 4.** Root docs:
- `AGENTS.md`: add the TS `chatgpt_codex.ts` reference to the HTTP-providers row next to Python's
  `chatgpt_codex.py`.
- `llms.txt`: bump the TS release-tag example to `ts-v0.11.0`; add a TS ChatGptCodex one-liner near the
  Python one.
- `skills/motosan-ai/SKILL.md`: bump the TS version line to `TypeScript 0.11.0` and add a TS ChatGPT-codex
  bullet.

**Step 5.** Final full gates in the worktree (pack-smoke needs `dist/`, so build first):
```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/.worktrees/ts-chatgpt-codex/sdks/typescript
npm run build && npm run typecheck && npm test
```

Commit: `chore(ts-chatgpt-codex): release 0.11.0`.

**Done criteria:** `package.json` version is `0.11.0` (== the eventual `ts-v0.11.0` tag → publish guard
passes); CHANGELOG has a `## [0.11.0]` entry; README documents the provider + bumps the provider count and
default-models table; root `AGENTS.md`/`llms.txt`/`SKILL.md` reference the TS ChatGPT-codex provider at
0.11.0; `npm run build && npm run typecheck && npm test` all green; `pack-smoke.test.ts` passes against the
fresh `dist/`.

---

## Spec gaps / decisions flagged

1. **Mid-stream error: silent-terminate vs throw — RESOLVED to silent-terminate.** R1 (reference contract)
   recommended throwing `StreamError`; R2 (TS integration contract) + LOCKED DECISION 3 mandate silent
   termination, and the codebase evidence is decisive (`ollama.ts:362-366` `try/catch { return }` +
   `providers-ollama.test.ts:377` asserting `.resolves.toBeUndefined()`). This plan locks in
   silent-terminate and ships the `error`/`response.failed` → `return` tests accordingly. The Python
   end-to-end "raises StreamError" test is intentionally NOT ported; instead we ship the silent-terminate
   tests AND a pure `chatGptCodexErrorMessage` unit test so the extraction-precedence coverage from the
   reference is retained even though the stream path discards the message. (If the team later wants the
   message surfaced, the cleanest non-breaking option is to add it to a future `StreamReadTimeoutError`-style
   typed terminal — out of scope for 0.11.0.)
2. **`Provider` union string = `'chatgpt_codex'`** (snake-case, matching the Python wire name and the file
   name), not the prompt's first suggestion `'openai_chatgpt'`. The prompt explicitly allowed either; R2
   recommends `'chatgpt_codex'`. Used consistently in the union, `ENV_KEY_BY_PROVIDER`, the builder, and docs.
3. **Tools `description`/`parameters` passthrough.** TS `Tool.description`/`Tool.inputSchema` are optional;
   the Rust/Python `ToolSchema` always carries both. This plan emits `description ?? null` and
   `parameters ?? null` (so `strict:null`-style `null`s appear when a TS caller omits them). This is a
   deliberate, documented passthrough choice — Python passes the values through directly because its `Tool`
   always has them.
4. **`buildProviderForTest()` test seam.** A small public method is added to `ClientBuilder` so the dispatch
   test can assert the constructed provider's type/model/reasoning without a Client or a mocked fetch,
   matching how `client-builder.test.ts` already inspects built providers. If the team disallows test-only
   methods, fall back to a `.build()` + mocked-fetch header smoke (noted inline in T4).
5. **`buildResponsesBody` is public.** Exposed for unit testing (the Python tests call
   `_build_responses_body` directly). If a stricter API surface is required, it can be marked
   `@internal` in TSDoc; it is not part of the documented public contract.
```

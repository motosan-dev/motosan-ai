# Milestone 1 — TypeScript SDK Foundation (Anthropic raw wire) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the `@anthropic-ai/sdk` wrapper with a self-implemented raw `fetch` + SSE Anthropic provider built on a full structured type system, the `StreamEvent` taxonomy, and a `collectStream` helper — leaving the Anthropic provider fully functional end-to-end with zero `@anthropic-ai/sdk` dependency. Ships as v0.4.0.

**Architecture:** Self-implement the wire protocol (raw `fetch` + SSE parsing + own serialization), mirroring the Rust SDK v0.19.0. Domain types live in `types.ts`; serialization in `serialize/`; transport in `http/`; cross-cutting stream helpers in `stream.ts`. The Anthropic provider composes these. (OpenAI/MiniMax stay on their current official-SDK wrappers until M2/M4 — this milestone only rewrites Anthropic and drops `@anthropic-ai/sdk`.)

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, raw `fetch` (no official LLM SDKs). Reference: Rust SDK at `sdks/rust/src/`.

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§5 = this milestone).

---

## Conventions (apply to EVERY task — these override anything ambiguous in a task body)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. SDK package: `sdks/typescript/`. **All commands run from `sdks/typescript/`** unless a step says otherwise.
- **Workflow:** code changes go on a feature branch off `main` and land via **PR + CI** (not direct-to-main). Commit after each task.
- **Module system:** `strict` + `NodeNext`. **Every relative import MUST end in `.js`** (e.g. `import { Role } from './types.js'`) or `tsc` fails.
- **Layout:** source in `src/`, tests in `tests/` (NOT co-located). Existing tests use dot-style names (`providers.serialization.test.ts`); new test files may use either dot or hyphen style — each is self-contained.
- **Per-task quality gate:** `npm run build` (tsc strict) passes **and** the task's `npx vitest run tests/<file>` is green. There is **no TS formatter yet** (prettier arrives in M7) — do not add a fmt step.
- **Live tests** are env-gated and skip cleanly: `it.skipIf(!process.env.ANTHROPIC_API_KEY)(...)`.
- **Tool-call argument field is `input`** (never `args`/`params`), everywhere.

## Canonical symbol homes (single source of truth — never re-declare a symbol outside its home)

| Symbol(s) | Home file | Created by |
|---|---|---|
| `Role`, `ContentBlock`, `ImageSource`, `DocumentSource`, `ToolCall`, `Tool`, `ToolChoice`, `ThinkingConfig`, `SystemBlock`, `Usage`, `StopReason`, `StreamEventType`, `StreamEvent`, `Message`, `ChatRequest`, `ChatResponse` | `src/types.ts` | **Task 1** (file is created COMPLETE; later tasks IMPORT, never extend it) |
| `MotosanError` + subclasses, `mapHttpError`, `StreamReadTimeoutError`, `UnsupportedFeatureError`, `isRetryableStatus`, `isRetryableNetworkError`, `parseRetryAfter`, `extractErrorMessage` | `src/error.ts` | **Task 2** (extends the existing file) |
| message factory (`user`, `userWithCache`, `assistant`, `assistantWithToolCalls`, `system`, `tool`, `toolResult`, `userWithImage`, `userWithBlocks`, `userWithPdfBase64`, `userWithPdfUrl`, `userWithPdfBytes`, `withCache`) | `src/message.ts` | **Task 3** |
| `SseEvent`, `parseSse`; `parseNdjson` | `src/http/sse.ts`, `src/http/ndjson.ts` | **Task 4** |
| `FetchOptions`, `postJson`, `postStream` | `src/http/fetch.ts` | **Task 5** |
| `BoxStream`, `textEvent`, `doneEvent`, `doneWithStopReason`, `usageEvent`, `toolCallStart`, `toolCallArgs`, `toolCallArgsWithId`, `toolCallEnd`, `toolCallEndWithId`, `thinkingDelta`, `thinkingDone`, `collectStream` | `src/stream.ts` | **Task 6** |
| `serializeAnthropicRequest` | `src/serialize/anthropic.ts` | **Task 7** |
| `AnthropicProvider` (self-implemented) | `src/providers/anthropic.ts` | **Task 8** |
| public exports / routing / package deps | `src/index.ts`, `src/client.ts`, `package.json` | **Task 9** |

**Dependency order (execute top to bottom):** 1 types → 2 error → 3 message → 4 http/sse → 5 http/fetch → 6 stream → 7 serialize → 8 provider → 9 wire-up. Each task depends only on earlier ones.

---

### Task 1: Rewrite `types.ts` — structured type system

Replace the flat string-only types with the full structured type system mirroring the Rust SDK's `types.rs`: discriminated-union content blocks, image/document sources, expanded `StreamEvent`/`StreamEventType`, structured `Message`/`ChatRequest`/`ChatResponse`, `Usage` with optional cache tokens, `Tool`, `ToolChoice` (placeholder), `ThinkingConfig`, `SystemBlock`, `StopReason`, `Role`. This is the foundation every later M1 file imports from, so it is done first (TDD).

**Files:**
- `sdks/typescript/src/types.ts` (rewrite — replaces the flat types + the `MessageFactory` literal, whose per-method helpers move to `message.ts` in Task 3 as standalone exported functions: `user()`, `assistant()`, etc.)
- `sdks/typescript/tests/types.test.ts` (rewrite — JSON-roundtrip + optional-omission assertions)

> Context: the current `tests/types.test.ts` imports `MessageFactory` from `../src/types.js`. After this task `types.ts` is types-only. Rewrite the test file in this task so it no longer imports `MessageFactory` (its helpers return as standalone functions in Task 3's `message.ts`); the other existing test files (`client.test.ts`, `providers.serialization.test.ts`, the two `integration.*` files) are NOT touched here and keep compiling because they import only from `client`/provider modules. If `npm run build`/`npm run test` surfaces a stale `MessageFactory` reference from another file during this task, that file is updated in Task 9 (wire-up) — do not edit it here unless the build breaks; if it breaks, the minimal fix is to keep a temporary `MessageFactory` re-export shim. Verify with the build step below.

- [ ] **Step 1: Write the failing test** — full JSON-roundtrip + optional-omission coverage for every type the rewrite introduces.

  Overwrite `sdks/typescript/tests/types.test.ts` with:

  ```ts
  import { describe, expect, it } from 'vitest'
  import type {
    ChatRequest,
    ChatResponse,
    ContentBlock,
    DocumentSource,
    ImageSource,
    Message,
    StopReason,
    StreamEvent,
    StreamEventType,
    Tool,
    ToolCall,
    ToolChoice,
    Usage,
  } from '../src/types.js'

  // JSON roundtrip helper: structural equality after a serialize/parse cycle.
  const roundtrip = <T>(value: T): T => JSON.parse(JSON.stringify(value)) as T

  describe('ContentBlock variants roundtrip', () => {
    it('text block', () => {
      const block: ContentBlock = { type: 'text', text: 'hello' }
      expect(roundtrip(block)).toEqual(block)
    })

    it('image base64 + url sources', () => {
      const b64: ImageSource = { type: 'base64', mediaType: 'image/png', data: 'AAAA' }
      const url: ImageSource = { type: 'url', url: 'https://example.com/x.png' }
      const imgB64: ContentBlock = { type: 'image', source: b64 }
      const imgUrl: ContentBlock = { type: 'image', source: url }
      expect(roundtrip(imgB64)).toEqual(imgB64)
      expect(roundtrip(imgUrl)).toEqual(imgUrl)
    })

    it('document base64 + url sources', () => {
      const b64: DocumentSource = { type: 'base64', mediaType: 'application/pdf', data: 'JVBERi0' }
      const url: DocumentSource = { type: 'url', url: 'https://example.com/d.pdf' }
      const docB64: ContentBlock = { type: 'document', source: b64 }
      const docUrl: ContentBlock = { type: 'document', source: url }
      expect(roundtrip(docB64)).toEqual(docB64)
      expect(roundtrip(docUrl)).toEqual(docUrl)
    })
  })

  describe('Message with contentBlocks roundtrips', () => {
    it('preserves blocks, content, and tool fields', () => {
      const tc: ToolCall = { id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }
      const msg: Message = {
        role: 'user',
        content: 'look at this',
        contentBlocks: [
          { type: 'text', text: 'look at this' },
          { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'AAAA' } },
        ],
        toolCalls: [tc],
        cache: true,
      }
      expect(roundtrip(msg)).toEqual(msg)
    })
  })

  describe('StreamEvent for each eventType', () => {
    const cases: Array<{ kind: StreamEventType; event: StreamEvent }> = [
      { kind: 'text', event: { content: 'hi', done: false, eventType: 'text' } },
      {
        kind: 'tool_call_start',
        event: { content: '', done: false, eventType: 'tool_call_start', toolCallId: 't1', toolCallName: 'f' },
      },
      {
        kind: 'tool_call_args',
        event: { content: '', done: false, eventType: 'tool_call_args', toolCallArgsDelta: '{"a":' },
      },
      { kind: 'tool_call_end', event: { content: '', done: false, eventType: 'tool_call_end' } },
      {
        kind: 'usage',
        event: { content: '', done: false, eventType: 'usage', usage: { inputTokens: 1, outputTokens: 2 } },
      },
      { kind: 'thinking_delta', event: { content: 'mm', done: false, eventType: 'thinking_delta' } },
      { kind: 'thinking_done', event: { content: 'done', done: false, eventType: 'thinking_done' } },
    ]

    for (const { kind, event } of cases) {
      it(`roundtrips ${kind}`, () => {
        expect(roundtrip(event)).toEqual(event)
        expect(event.eventType).toBe(kind)
      })
    }

    it('terminal done event carries a stopReason', () => {
      const done: StreamEvent = { content: '', done: true, eventType: 'text', stopReason: 'end_turn' }
      expect(roundtrip(done)).toEqual(done)
    })
  })

  describe('Usage with and without cache tokens', () => {
    it('without cache tokens omits the optional keys', () => {
      const usage: Usage = { inputTokens: 10, outputTokens: 5 }
      const json = JSON.parse(JSON.stringify(usage))
      expect(json).toEqual({ inputTokens: 10, outputTokens: 5 })
      expect('cacheCreationInputTokens' in json).toBe(false)
      expect('cacheReadInputTokens' in json).toBe(false)
    })

    it('with cache tokens preserves them', () => {
      const usage: Usage = {
        inputTokens: 10,
        outputTokens: 5,
        cacheCreationInputTokens: 3,
        cacheReadInputTokens: 7,
      }
      expect(roundtrip(usage)).toEqual(usage)
    })
  })

  describe('undefined optionals are omitted by JSON serialization', () => {
    it('StreamEvent: only required keys survive when optionals are unset', () => {
      const event: StreamEvent = { content: 'x', done: false, eventType: 'text' }
      const json = JSON.parse(JSON.stringify(event))
      expect(json).toEqual({ content: 'x', done: false, eventType: 'text' })
      for (const k of ['toolCallId', 'toolCallName', 'toolCallArgsDelta', 'usage', 'stopReason']) {
        expect(k in json).toBe(false)
      }
    })

    it('Message: optional blocks/tool fields/cache omitted', () => {
      const msg: Message = { role: 'assistant', content: 'ok' }
      const json = JSON.parse(JSON.stringify(msg))
      expect(json).toEqual({ role: 'assistant', content: 'ok' })
      for (const k of ['contentBlocks', 'toolCallId', 'toolCalls', 'cache']) {
        expect(k in json).toBe(false)
      }
    })

    it('ChatResponse: thinking omitted when unset', () => {
      const resp: ChatResponse = {
        content: 'hi',
        toolCalls: [],
        model: 'm',
        usage: { inputTokens: 1, outputTokens: 1 },
        stopReason: 'end_turn',
      }
      const json = JSON.parse(JSON.stringify(resp))
      expect('thinking' in json).toBe(false)
    })

    it('ChatRequest: minimal request omits every optional', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      const json = JSON.parse(JSON.stringify(req))
      expect(Object.keys(json)).toEqual(['messages'])
    })
  })

  describe('Tool, ToolChoice, StopReason shapes', () => {
    it('Tool optional fields omitted', () => {
      const tool: Tool = { name: 'f' }
      const json = JSON.parse(JSON.stringify(tool))
      expect(json).toEqual({ name: 'f' })
    })

    it('ToolChoice variants roundtrip', () => {
      const choices: ToolChoice[] = [{ type: 'auto' }, { type: 'required' }, { type: 'none' }, { type: 'tool', name: 'f' }]
      for (const c of choices) expect(roundtrip(c)).toEqual(c)
    })

    it('StopReason union values are usable', () => {
      const reasons: StopReason[] = ['end_turn', 'max_tokens', 'tool_use', 'stop', 'stop_sequence', 'other']
      expect(reasons).toContain('tool_use')
    })
  })
  ```

- [ ] **Step 2: Run the test — it must fail.** The new file imports types (`ContentBlock`, `ImageSource`, `DocumentSource`, `StreamEventType`, `ToolChoice`, expanded `StreamEvent`/`Message`/`ChatResponse`, etc.) that do not yet exist in the flat `types.ts`.

  ```bash
  npx vitest run tests/types.test.ts
  ```

  Expected: FAIL — vitest reports unresolved/missing type exports from `../src/types.js` (e.g. `ContentBlock`, `ImageSource`, `DocumentSource`, `StreamEventType`, `ToolChoice` are not exported), and/or type errors on the expanded `StreamEvent`/`Message` shapes. (Confirms the test is exercising the new contract, not the old types.)

- [ ] **Step 3: Implement — overwrite `sdks/typescript/src/types.ts`** with the locked structured type system.

  Overwrite the file with exactly this content:

  ```ts
  /**
   * Core structured type system for the Motosan AI TypeScript SDK.
   *
   * Mirrors the Rust SDK's `types.rs` (source of truth) with idiomatic TS:
   * discriminated unions for content blocks / stream events, optional fields
   * OMITTED (not `undefined`) when absent, camelCase mapping Rust's snake_case.
   * Wire serialization lives in `serialize/*.ts`, NOT here.
   */

  /** Conversation role. Serialized lowercase on the wire (handled by serializers). */
  export type Role = 'user' | 'assistant' | 'system' | 'tool'

  /** Source for an image content block. Discriminated on `type`. */
  export type ImageSource =
    | { type: 'base64'; mediaType: string; data: string }
    | { type: 'url'; url: string }

  /** Source for a document content block (e.g. PDF). Anthropic-only. */
  export type DocumentSource =
    | { type: 'base64'; mediaType: string; data: string }
    | { type: 'url'; url: string }

  /** A single piece of structured message content. Discriminated on `type`. */
  export type ContentBlock =
    | { type: 'text'; text: string }
    | { type: 'image'; source: ImageSource }
    | { type: 'document'; source: DocumentSource }

  /**
   * A tool/function call requested by the model. The arguments field is `input`
   * (NOT `args`/`params`) per project convention, kept as a parsed object.
   */
  export interface ToolCall {
    id: string
    name: string
    input: Record<string, unknown>
  }

  /** A tool definition exposed to the model. */
  export interface Tool {
    name: string
    description?: string
    inputSchema?: Record<string, unknown>
  }

  /**
   * Controls how the model selects which tool (if any) to call.
   * Placeholder for M1 — full wire serialization lands in M2.
   */
  export type ToolChoice =
    | { type: 'auto' }
    | { type: 'required' }
    | { type: 'none' }
    | { type: 'tool'; name: string }

  /** Configuration for extended thinking (Anthropic). */
  export interface ThinkingConfig {
    budgetTokens: number
  }

  /** A system prompt block with optional cache control (Anthropic ephemeral cache). */
  export interface SystemBlock {
    text: string
    cacheControl?: boolean
  }

  /** Token usage accounting. Cache fields are Anthropic-only and optional. */
  export interface Usage {
    inputTokens: number
    outputTokens: number
    cacheCreationInputTokens?: number
    cacheReadInputTokens?: number
  }

  /** Why the model stopped generating. */
  export type StopReason =
    | 'end_turn'
    | 'max_tokens'
    | 'tool_use'
    | 'stop'
    | 'stop_sequence'
    | 'other'

  /**
   * The kind of a streaming event. `thinking_delta`/`thinking_done` are emitted
   * by Anthropic only; `collectStream` concatenates them into `ChatResponse.thinking`.
   */
  export type StreamEventType =
    | 'text'
    | 'tool_call_start'
    | 'tool_call_args'
    | 'tool_call_end'
    | 'usage'
    | 'thinking_delta'
    | 'thinking_done'

  /** A single streaming event. */
  export interface StreamEvent {
    content: string
    done: boolean
    eventType: StreamEventType
    toolCallId?: string
    toolCallName?: string
    toolCallArgsDelta?: string
    usage?: Usage
    stopReason?: StopReason
  }

  /**
   * A conversation message. `content` is a flat string (first text block) for
   * backward compat; `contentBlocks` holds the structured multimodal form.
   */
  export interface Message {
    role: Role
    content: string
    contentBlocks?: ContentBlock[]
    toolCallId?: string
    toolCalls?: ToolCall[]
    cache?: boolean
  }

  /** A chat request. Provider-agnostic; serializers project it to each wire format. */
  export interface ChatRequest {
    messages: Message[]
    tools?: Tool[]
    system?: string
    systemBlocks?: SystemBlock[]
    systemCache?: boolean
    toolChoice?: ToolChoice
    thinking?: ThinkingConfig
    stopSequences?: string[]
    model?: string
    maxTokens?: number
    temperature?: number
    providerOptions?: Record<string, unknown>
  }

  /** A non-streaming chat response (or the reassembly of a stream via collectStream). */
  export interface ChatResponse {
    content: string
    thinking?: string
    toolCalls: ToolCall[]
    model: string
    usage: Usage
    stopReason: StopReason
  }
  ```

- [ ] **Step 4: Run the test — it must pass.**

  ```bash
  npx vitest run tests/types.test.ts
  ```

  Expected: PASS — all `describe` blocks green (ContentBlock variants, Message with contentBlocks, every StreamEvent eventType + terminal done, Usage with/without cache tokens, undefined-optionals-omitted for StreamEvent/Message/ChatResponse/ChatRequest, Tool/ToolChoice/StopReason shapes).

- [ ] **Step 5: Type-check the whole package under strict mode (the M1 quality gate).**

  ```bash
  npm run build
  ```

  Expected: exit 0, no `tsc` errors. If `tsc` reports that another module still imports `MessageFactory` from `./types.js` (e.g. `client.ts`/`index.ts`), add a minimal temporary re-export shim at the bottom of `types.ts` — `export { MessageFactory } from './message.js'` is NOT available yet, so instead leave the consumer untouched and let Task 3/Task 9 own it; the only acceptable change in THIS task is to `types.ts` and `tests/types.test.ts`. If the build cannot pass without touching a consumer, stop and note it for Task 3/Task 9 rather than expanding this task's scope. (Per the existing layout, `client.ts`/`index.ts` import the provider/client surface, not `MessageFactory`, so the build is expected to pass clean.)

- [ ] **Step 6: Run the full suite to confirm no regression in sibling test files.**

  ```bash
  npm run test
  ```

  Expected: PASS overall, or — if a pre-existing sibling test references the now-moved `MessageFactory` — those specific failures are EXPECTED and resolved in Task 3 (`message.ts`) / Task 9 (wire-up). Record any such failure; do not fix it here. The new `tests/types.test.ts` must be fully green.

- [ ] **Step 7: Commit (test + impl together, conventional-commit style, on the M1 feature branch).**

  ```bash
  git add sdks/typescript/src/types.ts sdks/typescript/tests/types.test.ts
  git commit -m "$(cat <<'EOF'
  feat(ts): structured type system (content blocks, stream taxonomy, usage cache tokens)

  Replace the flat string-only types.ts with the full structured type system
  mirroring the Rust SDK: discriminated-union ContentBlock (text|image|document)
  with Image/DocumentSource, expanded StreamEvent + StreamEventType taxonomy,
  Usage with optional cache tokens, Message.contentBlocks, ChatRequest/Response
  with system blocks / thinking / stop sequences, Tool, ToolChoice placeholder.
  Optional fields are omitted (not undefined). camelCase maps Rust snake_case.

  Foundation for M1 (0.4.0). MessageFactory's helpers move to message.ts (Task 3) as standalone functions.

  Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
  EOF
  )"
  ```

**Done criteria:** `npx vitest run tests/types.test.ts` green; `npm run build` exits 0 under strict mode; `types.ts` defines all of `Role`, `ContentBlock`/`ImageSource`/`DocumentSource`, `ToolCall`, `Tool`, `ToolChoice`, `ThinkingConfig`, `SystemBlock`, `Usage` (with optional cache tokens), `StopReason`, `StreamEventType`, `StreamEvent`, `Message` (with `contentBlocks`), `ChatRequest`, `ChatResponse`; no flat-string-only assumption remains; commit landed on the feature branch.

---

### Task 2: error.ts extensions + classification utils

**Files:**
- **Modify:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/error.ts` (extend existing error classes + add new ones + add utils)
- **Create:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/error.test.ts` (new test file)

---

- [ ] **Step 1: Write test file (TDD — expect FAIL)**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/error.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import {
  StreamReadTimeoutError,
  UnsupportedFeatureError,
  isRetryableStatus,
  isRetryableNetworkError,
  parseRetryAfter,
  extractErrorMessage,
} from '../src/error.js'

describe('StreamReadTimeoutError', () => {
  it('carries timeoutSecs property', () => {
    const error = new StreamReadTimeoutError(30)
    expect(error.timeoutSecs).toBe(30)
    expect(error.message).toContain('30')
  })

  it('has correct message format', () => {
    const error = new StreamReadTimeoutError(5)
    expect(error.message).toMatch(/stream.*timeout|timeout.*stream/i)
  })
})

describe('UnsupportedFeatureError', () => {
  it('extends MotosanError', () => {
    const error = new UnsupportedFeatureError('document input not supported')
    expect(error.message).toBe('document input not supported')
  })
})

describe('isRetryableStatus', () => {
  it('returns true for 429 (rate limit)', () => {
    expect(isRetryableStatus(429)).toBe(true)
  })

  it('returns true for status >= 500', () => {
    expect(isRetryableStatus(500)).toBe(true)
    expect(isRetryableStatus(502)).toBe(true)
    expect(isRetryableStatus(503)).toBe(true)
    expect(isRetryableStatus(599)).toBe(true)
  })

  it('returns false for 401, 400, 404, 4xx (except 429)', () => {
    expect(isRetryableStatus(401)).toBe(false)
    expect(isRetryableStatus(400)).toBe(false)
    expect(isRetryableStatus(404)).toBe(false)
    expect(isRetryableStatus(499)).toBe(false)
  })

  it('returns false for 2xx and 3xx', () => {
    expect(isRetryableStatus(200)).toBe(false)
    expect(isRetryableStatus(301)).toBe(false)
  })
})

describe('isRetryableNetworkError', () => {
  it('returns true for AbortError', () => {
    const error = new AbortError('cancelled')
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for TypeError (fetch network failure)', () => {
    const error = new TypeError('fetch failed')
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ECONNREFUSED (connection refused)', () => {
    const error = new Error('Connection refused')
    ;(error as any).code = 'ECONNREFUSED'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ENOTFOUND (DNS resolution failure)', () => {
    const error = new Error('getaddrinfo ENOTFOUND example.com')
    ;(error as any).code = 'ENOTFOUND'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns true for ETIMEDOUT (connection timeout)', () => {
    const error = new Error('Connection timeout')
    ;(error as any).code = 'ETIMEDOUT'
    expect(isRetryableNetworkError(error)).toBe(true)
  })

  it('returns false for unrelated errors', () => {
    const error = new Error('some other error')
    expect(isRetryableNetworkError(error)).toBe(false)
  })

  it('returns false for non-Error objects', () => {
    expect(isRetryableNetworkError('not an error')).toBe(false)
    expect(isRetryableNetworkError(null)).toBe(false)
    expect(isRetryableNetworkError(undefined)).toBe(false)
  })
})

describe('parseRetryAfter', () => {
  it('parses integer seconds from header value', () => {
    const result = parseRetryAfter('30')
    expect(result).toBe(30000) // 30 seconds in milliseconds
  })

  it('parses with leading/trailing whitespace', () => {
    const result = parseRetryAfter('  60  ')
    expect(result).toBe(60000)
  })

  it('returns undefined for non-integer value', () => {
    expect(parseRetryAfter('invalid')).toBeUndefined()
    expect(parseRetryAfter('30.5')).toBeUndefined()
    expect(parseRetryAfter('abc')).toBeUndefined()
  })

  it('returns undefined for null/empty string', () => {
    expect(parseRetryAfter(null)).toBeUndefined()
    expect(parseRetryAfter('')).toBeUndefined()
  })

  it('returns undefined for negative numbers', () => {
    expect(parseRetryAfter('-5')).toBeUndefined()
  })

  it('handles zero seconds', () => {
    const result = parseRetryAfter('0')
    expect(result).toBe(0)
  })

  it('handles large numbers', () => {
    const result = parseRetryAfter('3600')
    expect(result).toBe(3600000) // 1 hour in milliseconds
  })
})

describe('extractErrorMessage', () => {
  it('extracts message from {error:{message}} (Anthropic/OpenAI format)', () => {
    const body = {
      error: {
        message: 'API key is invalid',
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('API key is invalid')
  })

  it('uses fallback when error.message is missing', () => {
    const body = {
      error: {
        type: 'auth_error',
      },
    }
    expect(extractErrorMessage(body, 'authentication failed')).toBe('authentication failed')
  })

  it('uses fallback when error object is missing', () => {
    const body = {
      status: 401,
    }
    expect(extractErrorMessage(body, 'request failed')).toBe('request failed')
  })

  it('uses fallback for null body', () => {
    expect(extractErrorMessage(null, 'unknown error')).toBe('unknown error')
  })

  it('uses fallback for undefined body', () => {
    expect(extractErrorMessage(undefined, 'unknown error')).toBe('unknown error')
  })

  it('uses fallback for empty object', () => {
    expect(extractErrorMessage({}, 'fallback')).toBe('fallback')
  })

  it('uses fallback when error.message is not a string', () => {
    const body = {
      error: {
        message: 123,
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('default')
  })

  it('handles nested error structures', () => {
    const body = {
      error: {
        message: 'Rate limit exceeded: 100 requests per minute',
      },
    }
    expect(extractErrorMessage(body, 'default')).toBe('Rate limit exceeded: 100 requests per minute')
  })
})
```

Run test (expect FAIL):
```bash
npx vitest run tests/error.test.ts
```

Expected output:
```
FAIL  tests/error.test.ts (X tests)
✓ StreamReadTimeoutError (1)
✗ carries timeoutSecs property
  ReferenceError: StreamReadTimeoutError is not exported
...
```

---

- [ ] **Step 2: Implement error.ts extensions**

Modify `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/error.ts`:

```typescript
export class MotosanError extends Error {}
export class AuthError extends MotosanError {}
export class RateLimitError extends MotosanError {}
export class InvalidRequestError extends MotosanError {}
export class ConfigError extends MotosanError {}
export class ProviderError extends MotosanError {}
export class NetworkError extends MotosanError {}
export class StreamError extends MotosanError {}

/**
 * Error thrown when a stream read operation times out.
 * Carries the timeout duration in seconds.
 */
export class StreamReadTimeoutError extends MotosanError {
  readonly timeoutSecs: number

  constructor(timeoutSecs: number) {
    super(`stream read timeout: no data received within ${timeoutSecs} seconds`)
    this.timeoutSecs = timeoutSecs
    this.name = 'StreamReadTimeoutError'
  }
}

/**
 * Error thrown when a provider does not support a requested feature
 * (e.g., document input on a provider that only supports text and images).
 */
export class UnsupportedFeatureError extends MotosanError {
  constructor(message: string) {
    super(`unsupported feature: ${message}`)
    this.name = 'UnsupportedFeatureError'
  }
}

export function mapHttpError(status: number, message: string): MotosanError {
  if (status === 401) return new AuthError(message)
  if (status === 429) return new RateLimitError(message)
  if (status === 400) return new InvalidRequestError(message)
  return new ProviderError(message)
}

/**
 * Determine if an HTTP status code is retryable.
 * Retryable statuses: 429 (rate limit) or >= 500 (server error).
 *
 * Mirrors Rust `is_retryable_status`.
 */
export function isRetryableStatus(status: number): boolean {
  return status === 429 || status >= 500
}

/**
 * Determine if a network error is retryable.
 * Retryable errors:
 * - AbortError (request cancelled)
 * - TypeError (fetch network failure)
 * - Error.code === 'ECONNREFUSED' (connection refused)
 * - Error.code === 'ENOTFOUND' (DNS resolution failure)
 * - Error.code === 'ETIMEDOUT' (connection timeout)
 *
 * Mirrors Rust `is_retryable_network_error` mapped to fetch/Node error shapes.
 */
export function isRetryableNetworkError(error: unknown): boolean {
  if (!(error instanceof Error)) {
    return false
  }

  // AbortError (fetch cancelled/timed out at fetch level)
  if (error.name === 'AbortError') {
    return true
  }

  // TypeError (fetch network failure — e.g., "Failed to fetch")
  if (error instanceof TypeError) {
    return true
  }

  // Node.js error codes (socket/connection failures)
  const code = (error as any).code
  if (code === 'ECONNREFUSED' || code === 'ENOTFOUND' || code === 'ETIMEDOUT') {
    return true
  }

  return false
}

/**
 * Parse the Retry-After header value (integer seconds) into milliseconds.
 *
 * Returns undefined if the header is null, empty, or contains a non-integer value.
 * Mirrors Rust `parse_retry_after`: trim, parse as u64 seconds, convert to ms.
 */
export function parseRetryAfter(headerValue: string | null): number | undefined {
  if (headerValue === null || headerValue === '') {
    return undefined
  }

  const trimmed = headerValue.trim()
  if (trimmed === '') {
    return undefined
  }

  const seconds = parseInt(trimmed, 10)
  if (isNaN(seconds) || seconds < 0) {
    return undefined
  }

  return seconds * 1000
}

/**
 * Extract an error message from a response body.
 *
 * Attempts to extract `body.error.message` (Anthropic/OpenAI wire format).
 * Falls back to the provided fallback string if extraction fails.
 *
 * Mirrors Rust `extract_error_message`.
 */
export function extractErrorMessage(body: unknown, fallback: string): string {
  if (body === null || body === undefined) {
    return fallback
  }

  if (typeof body !== 'object') {
    return fallback
  }

  const error = (body as any).error
  if (error === null || error === undefined || typeof error !== 'object') {
    return fallback
  }

  const message = error.message
  if (typeof message === 'string') {
    return message
  }

  return fallback
}
```

---

- [ ] **Step 3: Run test (expect PASS)**

```bash
npx vitest run tests/error.test.ts
```

Expected output:
```
✓ tests/error.test.ts (22 tests) 2000ms
  ✓ StreamReadTimeoutError
    ✓ carries timeoutSecs property
    ✓ has correct message format
  ✓ UnsupportedFeatureError
    ✓ extends MotosanError
  ✓ isRetryableStatus (5 tests)
    ✓ returns true for 429
    ✓ returns true for status >= 500
    ✓ returns false for 4xx (except 429)
    ✓ returns false for 2xx and 3xx
  ✓ isRetryableNetworkError (7 tests)
    ✓ returns true for AbortError
    ✓ returns true for TypeError
    ✓ returns true for ECONNREFUSED
    ✓ returns true for ENOTFOUND
    ✓ returns true for ETIMEDOUT
    ✓ returns false for unrelated errors
    ✓ returns false for non-Error objects
  ✓ parseRetryAfter (7 tests)
    ✓ parses integer seconds
    ✓ parses with whitespace
    ✓ returns undefined for non-integer
    ✓ returns undefined for null/empty
    ✓ returns undefined for negative
    ✓ handles zero
    ✓ handles large numbers
  ✓ extractErrorMessage (8 tests)
    ✓ extracts message from {error:{message}}
    ✓ uses fallback when message missing
    ✓ uses fallback when error missing
    ✓ uses fallback for null body
    ✓ uses fallback for undefined body
    ✓ uses fallback for empty object
    ✓ uses fallback when message not string
    ✓ handles nested error structures
```

---

- [ ] **Step 4: Build (type-check under strict mode)**

```bash
npm run build
```

Expected output:
```
tsc --strict
(no errors — all types resolve, strict mode passes)
```

---

- [ ] **Step 5: Commit**

```bash
git add src/error.ts tests/error.test.ts && git commit -m "feat(error): add StreamReadTimeoutError, UnsupportedFeatureError, and classification utils

- Add StreamReadTimeoutError carrying timeoutSecs property
- Add UnsupportedFeatureError for unsupported provider features
- Implement isRetryableStatus (429 || >=500)
- Implement isRetryableNetworkError (AbortError/TypeError/ECONNREFUSED/ENOTFOUND/ETIMEDOUT)
- Implement parseRetryAfter (integer seconds -> milliseconds)
- Implement extractErrorMessage with fallback (Anthropic/OpenAI {error:{message}} format)
- Comprehensive test matrix covering all utils

Mirrors Rust error.rs + providers/mod.rs semantics.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

Expected output:
```
[feature-branch xxxxxxx] feat(error): add StreamReadTimeoutError, UnsupportedFeatureError, and classification utils
 2 files changed, 180 insertions(+), 15 deletions(-)
 create mode 100644 tests/error.test.ts
```

---

### Task 3: message.ts factory (multimodal + cache helpers)

**Files:**
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/message.ts`
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/message.test.ts`

> Prerequisite: `types.ts` is already complete from Task 1 — this task imports from it and does not modify it. `index.ts` re-exports happen in Task 9 (wire-up).

---

- [ ] **Step 1: Verify the type prerequisites from Task 1 exist (do NOT re-declare)**

`message.ts` builds on `ContentBlock`, `ImageSource`, `DocumentSource`, `ToolCall`, `Role`, and the `Message` interface (with `contentBlocks?` and `cache?`) — **all created in Task 1's `types.ts`**. Re-declaring them here would cause duplicate-identifier build errors. Confirm they are present; do not modify `types.ts` in this task.

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
grep -E "export type (ContentBlock|ImageSource|DocumentSource)" src/types.ts && grep -E "contentBlocks\?|cache\?" src/types.ts
```

Expected output: matching lines for `ContentBlock`, `ImageSource`, `DocumentSource`, and the `Message.contentBlocks?` / `cache?` fields. If any are missing, finish Task 1 first.

---

- [ ] **Step 2: Create message.test.ts with test cases**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/message.test.ts`:

```ts
import { describe, it, expect } from 'vitest'
import type { ContentBlock, Message } from '../src/types.js'
import {
  user,
  userWithCache,
  assistant,
  assistantWithToolCalls,
  system,
  tool,
  toolResult,
  userWithImage,
  userWithBlocks,
  userWithPdfBase64,
  userWithPdfUrl,
  userWithPdfBytes,
  withCache,
} from '../src/message.js'

describe('message factories', () => {
  describe('user', () => {
    it('creates a user message with content', () => {
      const msg = user('hello')
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('hello')
      expect('cache' in msg).toBe(false)
      expect('contentBlocks' in msg).toBe(false)
    })
  })

  describe('userWithCache', () => {
    it('creates a user message marked for caching', () => {
      const msg = userWithCache('hello')
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('hello')
      expect(msg.cache).toBe(true)
      expect('contentBlocks' in msg).toBe(false)
    })
  })

  describe('assistant', () => {
    it('creates an assistant message', () => {
      const msg = assistant('response')
      expect(msg.role).toBe('assistant')
      expect(msg.content).toBe('response')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('assistantWithToolCalls', () => {
    it('creates an assistant message with tool calls', () => {
      const toolCalls = [
        { id: 'call_1', name: 'get_weather', input: { city: 'NYC' } },
      ]
      const msg = assistantWithToolCalls('calling tool', toolCalls)
      expect(msg.role).toBe('assistant')
      expect(msg.content).toBe('calling tool')
      expect(msg.toolCalls).toEqual(toolCalls)
      expect('cache' in msg).toBe(false)
    })
  })

  describe('system', () => {
    it('creates a system message', () => {
      const msg = system('be helpful')
      expect(msg.role).toBe('system')
      expect(msg.content).toBe('be helpful')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('tool', () => {
    it('creates a tool message (alias of toolResult)', () => {
      const msg = tool('result text', 'call_123')
      expect(msg.role).toBe('tool')
      expect(msg.content).toBe('result text')
      expect(msg.toolCallId).toBe('call_123')
      expect('cache' in msg).toBe(false)
    })
  })

  describe('toolResult', () => {
    it('creates a tool message with tool call id', () => {
      const msg = toolResult('call_456', 'some result')
      expect(msg.role).toBe('tool')
      expect(msg.content).toBe('some result')
      expect(msg.toolCallId).toBe('call_456')
      expect('toolCalls' in msg).toBe(false)
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithImage', () => {
    it('creates a user message with text and image blocks', () => {
      const base64Data = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg=='
      const msg = userWithImage('look at this', base64Data, 'image/png')

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('look at this')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'look at this' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'image',
        source: { type: 'base64', mediaType: 'image/png', data: base64Data },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithBlocks', () => {
    it('extracts first text block as content and stores all blocks', () => {
      const blocks: ContentBlock[] = [
        { type: 'text', text: 'first text' },
        { type: 'text', text: 'second text' },
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
      ]
      const msg = userWithBlocks(blocks)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('first text')
      expect(msg.contentBlocks).toEqual(blocks)
      expect('cache' in msg).toBe(false)
    })

    it('handles empty blocks array', () => {
      const msg = userWithBlocks([])
      expect(msg.role).toBe('user')
      expect(msg.content).toBe('')
      expect(msg.contentBlocks).toEqual([])
    })

    it('extracts text from first text block when not first block', () => {
      const blocks: ContentBlock[] = [
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
        { type: 'text', text: 'found text' },
      ]
      const msg = userWithBlocks(blocks)

      expect(msg.content).toBe('found text')
      expect(msg.contentBlocks).toEqual(blocks)
    })
  })

  describe('userWithPdfBase64', () => {
    it('creates a user message with text and PDF document blocks', () => {
      const pdfBase64 = 'JVBERi0xLjQK'
      const msg = userWithPdfBase64('here is a pdf', pdfBase64)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('here is a pdf')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'here is a pdf' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'document',
        source: { type: 'base64', mediaType: 'application/pdf', data: pdfBase64 },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithPdfUrl', () => {
    it('creates a user message with text and PDF document from URL', () => {
      const url = 'https://example.com/document.pdf'
      const msg = userWithPdfUrl('check this pdf', url)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('check this pdf')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'check this pdf' })
      expect(msg.contentBlocks![1]).toEqual({
        type: 'document',
        source: { type: 'url', url },
      })
      expect('cache' in msg).toBe(false)
    })
  })

  describe('userWithPdfBytes', () => {
    it('base64-encodes bytes and creates PDF document message', () => {
      const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46]) // "%PDF"
      const msg = userWithPdfBytes('pdf from bytes', bytes)

      expect(msg.role).toBe('user')
      expect(msg.content).toBe('pdf from bytes')
      expect(msg.contentBlocks).toHaveLength(2)
      expect(msg.contentBlocks![0]).toEqual({ type: 'text', text: 'pdf from bytes' })
      expect(msg.contentBlocks![1].type).toBe('document')
      if (msg.contentBlocks![1].type === 'document') {
        const docBlock = msg.contentBlocks![1]
        expect(docBlock.source.type).toBe('base64')
        expect(docBlock.source.mediaType).toBe('application/pdf')
        if (docBlock.source.type === 'base64') {
          expect(docBlock.source.data).toBe('JVBERg==') // base64 of "%PDF"
        }
      }
      expect('cache' in msg).toBe(false)
    })
  })

  describe('withCache', () => {
    it('returns a copy of the message with cache flag set to true', () => {
      const original = user('hello')
      const cached = withCache(original)

      expect(cached.role).toBe('user')
      expect(cached.content).toBe('hello')
      expect(cached.cache).toBe(true)
      expect('cache' in original).toBe(false)
    })

    it('preserves contentBlocks when setting cache', () => {
      const blocks: ContentBlock[] = [
        { type: 'text', text: 'text' },
        { type: 'image', source: { type: 'url', url: 'https://example.com/img.png' } },
      ]
      const original = userWithBlocks(blocks)
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.contentBlocks).toEqual(blocks)
    })

    it('preserves toolCalls when setting cache', () => {
      const toolCalls = [{ id: 'tc1', name: 'func', input: {} }]
      const original = assistantWithToolCalls('resp', toolCalls)
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.toolCalls).toEqual(toolCalls)
    })

    it('preserves toolCallId when setting cache', () => {
      const original = toolResult('call_1', 'result')
      const cached = withCache(original)

      expect(cached.cache).toBe(true)
      expect(cached.toolCallId).toBe('call_1')
      expect(cached.role).toBe('tool')
    })
  })
})
```

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
npx vitest run tests/message.test.ts
```

Expected output: FAIL — module not found (message.js).

```
Error: Cannot find module '../src/message.js' imported from '.../tests/message.test.ts'
```

---

- [ ] **Step 3: Implement message.ts factory functions**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/message.ts`:

```ts
import type { ContentBlock, Message, ToolCall } from './types.js'

/**
 * Create a user message with plain text content.
 */
export function user(content: string): Message {
  return {
    role: 'user',
    content,
  }
}

/**
 * Create a user message marked as cacheable (Anthropic prompt caching).
 * Non-Anthropic providers silently ignore the cache flag.
 */
export function userWithCache(content: string): Message {
  return {
    role: 'user',
    content,
    cache: true,
  }
}

/**
 * Create an assistant message with plain text content.
 */
export function assistant(content: string): Message {
  return {
    role: 'assistant',
    content,
  }
}

/**
 * Create an assistant message with tool calls.
 */
export function assistantWithToolCalls(content: string, toolCalls: ToolCall[]): Message {
  return {
    role: 'assistant',
    content,
    toolCalls,
  }
}

/**
 * Create a system message.
 */
export function system(content: string): Message {
  return {
    role: 'system',
    content,
  }
}

/**
 * Create a tool message. Alias for toolResult.
 */
export function tool(content: string, toolCallId: string): Message {
  return toolResult(toolCallId, content)
}

/**
 * Create a tool message for returning results to a tool call.
 */
export function toolResult(toolCallId: string, content: string): Message {
  return {
    role: 'tool',
    content,
    toolCallId,
  }
}

/**
 * Create a user message with text and an image.
 *
 * @param text - The text content accompanying the image.
 * @param base64Data - Base64-encoded image data.
 * @param mediaType - The MIME type of the image (e.g., 'image/png', 'image/jpeg').
 */
export function userWithImage(text: string, base64Data: string, mediaType: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'image',
        source: {
          type: 'base64',
          mediaType,
          data: base64Data,
        },
      },
    ],
  }
}

/**
 * Create a user message with multiple content blocks.
 * The content field (for backward compatibility) is extracted from the first text block.
 */
export function userWithBlocks(blocks: ContentBlock[]): Message {
  // Extract text from first text block for backward compat
  const content = blocks
    .find((b) => b.type === 'text')
    ?.type === 'text' ? blocks.find((b) => b.type === 'text')!.text : ''

  return {
    role: 'user',
    content,
    contentBlocks: blocks,
  }
}

/**
 * Create a user message with a PDF document from base64-encoded data.
 *
 * @param text - The text content accompanying the PDF.
 * @param base64Data - Base64-encoded PDF data.
 */
export function userWithPdfBase64(text: string, base64Data: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'document',
        source: {
          type: 'base64',
          mediaType: 'application/pdf',
          data: base64Data,
        },
      },
    ],
  }
}

/**
 * Create a user message with a PDF document from a URL.
 *
 * @param text - The text content accompanying the PDF.
 * @param url - The URL of the PDF document.
 */
export function userWithPdfUrl(text: string, url: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'document',
        source: {
          type: 'url',
          url,
        },
      },
    ],
  }
}

/**
 * Create a user message with a PDF document from raw bytes.
 * The bytes are automatically base64-encoded.
 *
 * @param text - The text content accompanying the PDF.
 * @param bytes - Raw PDF bytes.
 */
export function userWithPdfBytes(text: string, bytes: Uint8Array): Message {
  const base64Data = Buffer.from(bytes).toString('base64')
  return userWithPdfBase64(text, base64Data)
}

/**
 * Mark a message as cacheable by returning a copy with cache flag set to true.
 * Non-Anthropic providers silently ignore the cache flag.
 */
export function withCache(message: Message): Message {
  return {
    ...message,
    cache: true,
  }
}
```

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
npx vitest run tests/message.test.ts
```

Expected output: All tests pass.

```
✓ tests/message.test.ts (16)
  ✓ message factories (16)
    ✓ user (1)
    ✓ userWithCache (1)
    ✓ assistant (1)
    ✓ assistantWithToolCalls (1)
    ✓ system (1)
    ✓ tool (1)
    ✓ toolResult (1)
    ✓ userWithImage (1)
    ✓ userWithBlocks (3)
    ✓ userWithPdfBase64 (1)
    ✓ userWithPdfUrl (1)
    ✓ userWithPdfBytes (1)
    ✓ withCache (4)
```

---

- [ ] **Step 4: Build and type-check**

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
npm run build
```

Expected output: No TypeScript errors under strict mode.

```
Successfully compiled 2 files with tsc.
```

---

- [ ] **Step 5: Update index.ts to export new symbols**

Modify `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/index.ts` to add exports from message.ts:

```ts
// Add these exports alongside existing ones:
export {
  user,
  userWithCache,
  assistant,
  assistantWithToolCalls,
  system,
  tool,
  toolResult,
  userWithImage,
  userWithBlocks,
  userWithPdfBase64,
  userWithPdfUrl,
  userWithPdfBytes,
  withCache,
} from './message.js'

// Also export new types:
export type { ContentBlock, ImageSource, DocumentSource } from './types.js'
```

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
npm run build
```

Expected output: No TypeScript errors.

---

- [ ] **Step 6: Commit the changes**

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
git add src/message.ts src/types.ts src/index.ts tests/message.test.ts
git commit -m "feat: implement message.ts factory with multimodal and cache helpers

- Add ContentBlock, ImageSource, DocumentSource types for structured multimodal content
- Implement user, assistant, system, tool message factories
- Add userWithImage, userWithBlocks, userWithPdfBase64, userWithPdfUrl, userWithPdfBytes
- Add withCache helper to mark messages for caching
- content field populated from first text block for backward compat
- All tests passing under strict mode

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

Expected output: Commit successful with all tests passing.

```
[feature-branch abc1234] feat: implement message.ts factory with multimodal and cache helpers
 4 files changed, 350 insertions(+)
 create mode 100644 src/message.ts
 create mode 100644 tests/message.test.ts
```

---

### Task 4: SSE + NDJSON streaming parsers

**Files:**
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/sse.ts`
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/ndjson.ts`
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http.sse.test.ts`
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http.ndjson.test.ts`

**Spec requirements (§5 task 3):**
- SSE parser: async generator over `ReadableStream<Uint8Array>`; buffer across chunk boundaries; split on `\n\n` event boundaries; parse `event:` and `data:` lines; `JSON.parse(data)` (skip malformed silently); recognize `[DONE]` string but DO NOT terminate (completion is per-provider adapter decision, required for Gemini M6)
- NDJSON parser: async generator over `ReadableStream<Uint8Array>`; buffer across chunks; split on `\n` boundaries; `JSON.parse` each non-empty line; skip malformed silently (basic implementation; finalized M5)
- Tests: synthetic byte stream split mid-line and mid-JSON reassembles correctly; malformed data lines skipped; `[DONE]` recognized but not terminating (SSE only)

---

## Step 1: Write failing SSE parser test

- [ ] **Step 1: Create `tests/http.sse.test.ts` with test cases for SSE parsing**

Create test file at `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http.sse.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { parseSse } from '../src/http/sse.js'

describe('parseSse', () => {
  it('parses basic SSE events with event and data fields', async () => {
    const input = 'event: message\ndata: {"type":"text","content":"hello"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ type: 'text', content: 'hello' })
  })

  it('handles multiple events in stream', async () => {
    const input =
      'event: start\ndata: {"id":1}\n\nevent: delta\ndata: {"text":"hi"}\n\nevent: done\ndata: [DONE]\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[0].event).toBe('start')
    expect(events[0].data).toEqual({ id: 1 })
    expect(events[1].event).toBe('delta')
    expect(events[1].data).toEqual({ text: 'hi' })
    expect(events[2].event).toBe('done')
    expect(events[2].data).toBe('[DONE]')
  })

  it('buffers across chunk boundaries (split mid-line)', async () => {
    const full = 'event: message\ndata: {"content":"split"}\n\n'
    const chunk1 = full.substring(0, 15) // "event: message\nda"
    const chunk2 = full.substring(15) // 'ta: {"content":"split"}\n\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('message')
    expect(events[0].data).toEqual({ content: 'split' })
  })

  it('buffers across chunk boundaries (split mid-JSON)', async () => {
    const full = 'event: msg\ndata: {"x":123,"y":456}\n\n'
    const chunk1 = full.substring(0, 25) // "event: msg\ndata: {"x":123"
    const chunk2 = full.substring(25) // ',"y":456}\n\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('msg')
    expect(events[0].data).toEqual({ x: 123, y: 456 })
  })

  it('skips malformed JSON data silently', async () => {
    const input =
      'event: good\ndata: {"valid":true}\n\nevent: bad\ndata: {not json}\n\nevent: good2\ndata: {"valid":2}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(2)
    expect(events[0].event).toBe('good')
    expect(events[1].event).toBe('good2')
  })

  it('recognizes [DONE] string but does not terminate parsing', async () => {
    const input =
      'event: delta\ndata: {"text":"chunk1"}\n\nevent: done\ndata: [DONE]\n\nevent: final\ndata: {"text":"chunk2"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(3)
    expect(events[1].data).toBe('[DONE]')
    expect(events[2].event).toBe('final')
  })

  it('handles events without event field (data only)', async () => {
    const input = 'data: {"text":"no event"}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBeUndefined()
    expect(events[0].data).toEqual({ text: 'no event' })
  })

  it('ignores empty lines and non-field lines', async () => {
    const input = 'event: msg\n\ndata: {"ok":true}\n\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('msg')
    expect(events[0].data).toEqual({ ok: true })
  })

  it('handles stream ending without final double newline', async () => {
    const input = 'event: last\ndata: {"final":true}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const events: Array<{ event?: string; data: any }> = []
    for await (const event of parseSse(stream)) {
      events.push(event)
    }

    expect(events).toHaveLength(1)
    expect(events[0].event).toBe('last')
    expect(events[0].data).toEqual({ final: true })
  })
})
```

Run the test to confirm it fails:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/http.sse.test.ts
```

Expected output (test fails because `parseSse` doesn't exist yet):
```
FAIL  tests/http.sse.test.ts
Error: Cannot find module '../src/http/sse.js'
```

---

## Step 2: Write failing NDJSON parser test

- [ ] **Step 2: Create `tests/http.ndjson.test.ts` with test cases for NDJSON parsing**

Create test file at `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http.ndjson.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { parseNdjson } from '../src/http/ndjson.js'

describe('parseNdjson', () => {
  it('parses newline-delimited JSON objects', async () => {
    const input = '{"id":1,"text":"hello"}\n{"id":2,"text":"world"}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0]).toEqual({ id: 1, text: 'hello' })
    expect(objects[1]).toEqual({ id: 2, text: 'world' })
  })

  it('buffers across chunk boundaries (split mid-line)', async () => {
    const full = '{"x":100,"y":200}\n'
    const chunk1 = full.substring(0, 10) // '{"x":100,'
    const chunk2 = full.substring(10) // '"y":200}\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0]).toEqual({ x: 100, y: 200 })
  })

  it('buffers across multiple chunks', async () => {
    const full = '{"a":1}\n{"b":2}\n{"c":3}\n'
    const chunk1 = full.substring(0, 8) // '{"a":1}\n'
    const chunk2 = full.substring(8, 16) // '{"b":2}\n'
    const chunk3 = full.substring(16) // '{"c":3}\n'

    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(chunk1))
        controller.enqueue(new TextEncoder().encode(chunk2))
        controller.enqueue(new TextEncoder().encode(chunk3))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0]).toEqual({ a: 1 })
    expect(objects[1]).toEqual({ b: 2 })
    expect(objects[2]).toEqual({ c: 3 })
  })

  it('skips malformed JSON lines silently', async () => {
    const input = '{"good":1}\n{not json}\n{"good":2}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0]).toEqual({ good: 1 })
    expect(objects[1]).toEqual({ good: 2 })
  })

  it('ignores empty lines', async () => {
    const input = '{"a":1}\n\n{"b":2}\n\n\n{"c":3}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(3)
    expect(objects[0]).toEqual({ a: 1 })
    expect(objects[1]).toEqual({ b: 2 })
    expect(objects[2]).toEqual({ c: 3 })
  })

  it('handles stream ending without final newline', async () => {
    const input = '{"final":true}'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(1)
    expect(objects[0]).toEqual({ final: true })
  })

  it('handles mixed single and multi-line JSON objects (no nested newlines)', async () => {
    const input = '{"id":1,"name":"alice"}\n{"id":2,"text":"bob"}\n'
    const stream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(input))
        controller.close()
      }
    })

    const objects: any[] = []
    for await (const obj of parseNdjson(stream)) {
      objects.push(obj)
    }

    expect(objects).toHaveLength(2)
    expect(objects[0].name).toBe('alice')
    expect(objects[1].text).toBe('bob')
  })
})
```

Run the test to confirm it fails:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/http.ndjson.test.ts
```

Expected output (test fails because `parseNdjson` doesn't exist yet):
```
FAIL  tests/http.ndjson.test.ts
Error: Cannot find module '../src/http/ndjson.js'
```

---

## Step 3: Implement SSE parser

- [ ] **Step 3: Implement `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/sse.ts`**

```typescript
/**
 * Server-Sent Events (SSE) streaming parser.
 *
 * Parses a ReadableStream<Uint8Array> into an async generator of SSE events.
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete events, splits on \n\n boundaries, and parses event:/data: lines.
 * Malformed JSON in data fields is silently skipped. [DONE] is recognized as
 * data but does NOT terminate the stream (completion is adapter-specific).
 */

export interface SseEvent {
  event?: string
  data: any
}

/**
 * Parse a ReadableStream<Uint8Array> as Server-Sent Events.
 *
 * Yields SseEvent objects. Each event consists of optional 'event' field
 * and a 'data' field (JSON-parsed or string '[DONE]'). Malformed JSON is
 * skipped; [DONE] is recognized but does not terminate parsing.
 */
export async function* parseSse(
  body: ReadableStream<Uint8Array>
): AsyncGenerator<SseEvent> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    while (true) {
      const { done, value } = await reader.read()

      if (value) {
        buffer += decoder.decode(value, { stream: true })
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += decoder.decode()
        // Process final event if buffer is non-empty
        if (buffer.trim()) {
          const event = parseEventFromText(buffer)
          if (event) {
            yield event
          }
        }
        break
      }

      // Process complete events (split by \n\n)
      while (true) {
        const eventEndIndex = buffer.indexOf('\n\n')
        if (eventEndIndex === -1) {
          break // Wait for more data
        }

        const eventText = buffer.substring(0, eventEndIndex)
        buffer = buffer.substring(eventEndIndex + 2)

        const event = parseEventFromText(eventText)
        if (event) {
          yield event
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

/**
 * Parse a single event text block (lines before \n\n).
 * Extracts event: and data: fields, JSON-parses data.
 * Returns null if parsing fails (e.g., no data field, malformed JSON).
 */
function parseEventFromText(text: string): SseEvent | null {
  const lines = text.split('\n')
  let eventName: string | undefined
  let dataStr = ''

  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed) {
      continue
    }

    if (trimmed.startsWith('event:')) {
      eventName = trimmed.substring('event:'.length).trim()
    } else if (trimmed.startsWith('data:')) {
      dataStr = trimmed.substring('data:'.length).trim()
    }
  }

  // Must have at least a data field
  if (dataStr === '') {
    return null
  }

  // Special case: [DONE] is passed through as a string, not parsed as JSON
  if (dataStr === '[DONE]') {
    return {
      event: eventName,
      data: '[DONE]'
    }
  }

  // Try to parse data as JSON; skip malformed silently
  let parsedData: any
  try {
    parsedData = JSON.parse(dataStr)
  } catch {
    return null // Skip malformed JSON
  }

  return {
    event: eventName,
    data: parsedData
  }
}
```

Run the SSE tests:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/http.sse.test.ts
```

Expected output:
```
✓ tests/http.sse.test.ts (9 tests) 1234ms
  ✓ parses basic SSE events with event and data fields
  ✓ handles multiple events in stream
  ✓ buffers across chunk boundaries (split mid-line)
  ✓ buffers across chunk boundaries (split mid-JSON)
  ✓ skips malformed JSON data silently
  ✓ recognizes [DONE] string but does not terminate parsing
  ✓ handles events without event field (data only)
  ✓ ignores empty lines and non-field lines
  ✓ handles stream ending without final double newline

Test Files  1 passed (1)
     Tests  9 passed (9)
```

---

## Step 4: Implement NDJSON parser

- [ ] **Step 4: Implement `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/ndjson.ts`**

```typescript
/**
 * Newline-Delimited JSON (NDJSON) streaming parser.
 *
 * Parses a ReadableStream<Uint8Array> into an async generator of JSON objects.
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete lines, splits on \n boundaries, and JSON-parses each non-empty line.
 * Malformed JSON lines are silently skipped.
 *
 * Basic implementation for M1; finalized in M5.
 */

/**
 * Parse a ReadableStream<Uint8Array> as newline-delimited JSON.
 *
 * Yields parsed JSON objects (any type). Malformed JSON lines are silently
 * skipped. Empty lines are ignored.
 */
export async function* parseNdjson(
  body: ReadableStream<Uint8Array>
): AsyncGenerator<any> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    while (true) {
      const { done, value } = await reader.read()

      if (value) {
        buffer += decoder.decode(value, { stream: true })
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += decoder.decode()
        // Process final line if buffer is non-empty
        if (buffer.trim()) {
          const obj = parseJsonLine(buffer.trim())
          if (obj !== undefined) {
            yield obj
          }
        }
        break
      }

      // Process complete lines (split by \n)
      while (true) {
        const newlineIndex = buffer.indexOf('\n')
        if (newlineIndex === -1) {
          break // Wait for more data
        }

        const line = buffer.substring(0, newlineIndex)
        buffer = buffer.substring(newlineIndex + 1)

        const obj = parseJsonLine(line.trim())
        if (obj !== undefined) {
          yield obj
        }
      }
    }
  } finally {
    reader.releaseLock()
  }
}

/**
 * Parse a single line as JSON.
 * Returns undefined if line is empty or JSON parsing fails (malformed).
 */
function parseJsonLine(line: string): any {
  if (!line) {
    return undefined
  }

  try {
    return JSON.parse(line)
  } catch {
    return undefined // Skip malformed JSON
  }
}
```

Run the NDJSON tests:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/http.ndjson.test.ts
```

Expected output:
```
✓ tests/http.ndjson.test.ts (7 tests) 890ms
  ✓ parses newline-delimited JSON objects
  ✓ buffers across chunk boundaries (split mid-line)
  ✓ buffers across multiple chunks
  ✓ skips malformed JSON lines silently
  ✓ ignores empty lines
  ✓ handles stream ending without final newline
  ✓ handles mixed single and multi-line JSON objects (no nested newlines)

Test Files  1 passed (1)
     Tests  7 passed (7)
```

---

## Step 5: Run both parser tests together

- [ ] **Step 5: Run full test suite to verify both parsers pass**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/http.sse.test.ts tests/http.ndjson.test.ts
```

Expected output:
```
✓ tests/http.sse.test.ts (9 tests)
✓ tests/http.ndjson.test.ts (7 tests)

Test Files  2 passed (2)
     Tests  16 passed (16)
```

---

## Step 6: Type-check with strict TypeScript

- [ ] **Step 6: Verify strict mode compilation**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npm run build
```

Expected output:
```
[no errors]
```

---

## Step 7: Commit the SSE and NDJSON parsers

- [ ] **Step 7: Commit Task 4 implementation**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
git add src/http/sse.ts src/http/ndjson.ts tests/http.sse.test.ts tests/http.ndjson.test.ts
git commit -m "feat(http): add SSE and NDJSON streaming parsers

- Implement parseSse: ReadableStream<Uint8Array> → AsyncGenerator<SseEvent>
  - Buffer across chunk boundaries using TextDecoder
  - Split events on \\n\\n, parse event:/data: lines
  - JSON.parse data; skip malformed silently
  - Recognize [DONE] but do not terminate (adapter-specific)
- Implement parseNdjson: ReadableStream<Uint8Array> → AsyncGenerator<any>
  - Buffer across chunk boundaries; split on \\n
  - JSON.parse each non-empty line; skip malformed
- Tests: verify reassembly across mid-line/mid-JSON splits,
  malformed data handling, [DONE] non-termination (SSE)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

Expected git output:
```
[feature-branch XXXXXX] feat(http): add SSE and NDJSON streaming parsers
 4 files changed, 350 insertions(+)
 create mode 100644 src/http/sse.ts
 create mode 100644 src/http/ndjson.ts
 create mode 100644 tests/http.sse.test.ts
 create mode 100644 tests/http.ndjson.test.ts
```

---

## Contract Fulfillment

This task completes the locked contract for `http/sse.ts` and `http/ndjson.ts`:

**`SseEvent` interface**: `{ event?: string; data: any }`
**`parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseEvent>`**
- Buffers across chunk boundaries with TextDecoder
- Splits on `\n\n` boundaries per SSE spec
- Parses `event:` and `data:` lines (data is JSON-parsed or `[DONE]`)
- Skips malformed JSON silently
- Recognizes `[DONE]` but does NOT terminate (per adapter pattern)

**`parseNdjson(body: ReadableStream<Uint8Array>): AsyncGenerator<any>`**
- Buffers across chunk boundaries with TextDecoder
- Splits on `\n` boundaries
- JSON-parses each non-empty line; skips malformed silently
- Basic implementation (finalized in M5)

Both parsers follow the Rust SDK semantics (read Anthropic provider stream loop for SSE behavior; Ollama provider for NDJSON buffering). Tests validate reassembly, malformed handling, and [DONE] non-termination.

---

### Task 5: http/fetch.ts raw fetch wrapper

**Files:**
- **Create:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/fetch.ts`
- **Test:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http-fetch.test.ts`

> Depends on `src/error.ts`, which an earlier task already extended with `mapHttpError`, `extractErrorMessage`, `isRetryableStatus`, `isRetryableNetworkError`, and `parseRetryAfter`. This task imports those — it must NOT redefine them. It exports exactly three symbols: `FetchOptions`, `postJson`, `postStream`.

---

- [ ] **Step 1: Write the failing test for `postJson` and `postStream`**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/http-fetch.test.ts`:

```typescript
import { describe, it, expect, vi, afterEach } from 'vitest'
import { postJson, postStream } from '../src/http/fetch.js'
import { ProviderError, AuthError, RateLimitError, InvalidRequestError } from '../src/error.js'

describe('http/fetch', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  describe('postJson', () => {
    it('returns parsed JSON body on 200 response', async () => {
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      const result = await postJson('https://api.test.com/v1/messages', {}, { test: true })

      expect(result).toEqual({ result: 'success' })
      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({
          method: 'POST',
          headers: expect.any(Object),
          body: JSON.stringify({ test: true }),
        }),
      )
    })

    it('throws InvalidRequestError (mapped via extractErrorMessage) on 400', async () => {
      const mockResponse = {
        ok: false,
        status: 400,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad request' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(InvalidRequestError)
    })

    it('throws AuthError on 401 with the extracted message', async () => {
      const mockResponse = {
        ok: false,
        status: 401,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'unauthorized' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow('unauthorized')
    })

    it('throws RateLimitError on 429 response', async () => {
      const mockResponse = {
        ok: false,
        status: 429,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'rate limited' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(RateLimitError)
    })

    it('throws ProviderError on 500 response', async () => {
      const mockResponse = {
        ok: false,
        status: 500,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'server error' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow(ProviderError)
    })

    it('falls back to "HTTP <status>" when the body has no error message', async () => {
      const mockResponse = {
        ok: false,
        status: 500,
        text: vi.fn().mockResolvedValue('not json'),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postJson('https://api.test.com/v1/messages', {}, { test: true }),
      ).rejects.toThrow('HTTP 500')
    })

    it('respects AbortSignal in FetchOptions', async () => {
      const controller = new AbortController()
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await postJson(
        'https://api.test.com/v1/messages',
        {},
        { test: true },
        { signal: controller.signal },
      )

      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({ signal: controller.signal }),
      )
    })

    it('includes custom headers in request', async () => {
      const mockResponse = {
        ok: true,
        status: 200,
        json: vi.fn().mockResolvedValue({ result: 'success' }),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await postJson(
        'https://api.test.com/v1/messages',
        { 'x-api-key': 'test-key', 'content-type': 'application/json' },
        { test: true },
      )

      expect(fetch).toHaveBeenCalledWith(
        'https://api.test.com/v1/messages',
        expect.objectContaining({
          headers: expect.objectContaining({
            'x-api-key': 'test-key',
            'content-type': 'application/json',
          }),
        }),
      )
    })
  })

  describe('postStream', () => {
    it('returns the ReadableStream body on 200 response', async () => {
      const mockReadableStream = new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode('chunk1'))
          controller.close()
        },
      })
      const mockResponse = {
        ok: true,
        status: 200,
        body: mockReadableStream,
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      const result = await postStream('https://api.test.com/v1/stream', {}, { test: true })

      expect(result).toBe(mockReadableStream)
    })

    it('throws InvalidRequestError on 400 response', async () => {
      const mockResponse = {
        ok: false,
        status: 400,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad request' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postStream('https://api.test.com/v1/stream', {}, { test: true }),
      ).rejects.toThrow(InvalidRequestError)
    })

    it('throws ProviderError on 5xx response', async () => {
      const mockResponse = {
        ok: false,
        status: 502,
        text: vi.fn().mockResolvedValue(JSON.stringify({ error: { message: 'bad gateway' } })),
      }
      vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))

      await expect(
        postStream('https://api.test.com/v1/stream', {}, { test: true }),
      ).rejects.toThrow(ProviderError)
    })
  })
})
```

Run the test to confirm it fails:

```bash
npx vitest run tests/http-fetch.test.ts 2>&1 | tail -20
```

Expected output (module does not exist yet):

```
Error: Failed to load url ../src/http/fetch.js (resolved id: .../src/http/fetch.ts)
```

---

- [ ] **Step 2: Create `src/http/fetch.ts`**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/http/fetch.ts`. It imports the error utilities from `../error.js` and must NOT redefine any of them:

```typescript
import { extractErrorMessage, mapHttpError } from '../error.js'

export interface FetchOptions {
  signal?: AbortSignal
}

async function throwMappedError(response: Response): Promise<never> {
  const text = await response.text()
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    payload = text
  }
  const message = extractErrorMessage(payload, `HTTP ${response.status}`)
  throw mapHttpError(response.status, message)
}

export async function postJson<T = unknown>(
  url: string,
  headers: Record<string, string>,
  body: unknown,
  options?: FetchOptions,
): Promise<T> {
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }
  if (options?.signal) {
    fetchOptions.signal = options.signal
  }

  const response = await fetch(url, fetchOptions)

  if (!response.ok) {
    await throwMappedError(response)
  }

  return response.json() as Promise<T>
}

export async function postStream(
  url: string,
  headers: Record<string, string>,
  body: unknown,
  options?: FetchOptions,
): Promise<ReadableStream<Uint8Array>> {
  const fetchOptions: RequestInit = {
    method: 'POST',
    headers: { 'content-type': 'application/json', ...headers },
    body: JSON.stringify(body),
  }
  if (options?.signal) {
    fetchOptions.signal = options.signal
  }

  const response = await fetch(url, fetchOptions)

  if (!response.ok) {
    await throwMappedError(response)
  }

  if (!response.body) {
    throw new ProviderError('response body is null')
  }

  return response.body
}
```

The `ProviderError` reference in `postStream` requires an import. Update the import line at the top of the file to:

```typescript
import { extractErrorMessage, mapHttpError, ProviderError } from '../error.js'
```

Run the test again — it must now PASS:

```bash
npx vitest run tests/http-fetch.test.ts 2>&1 | tail -20
```

Expected output:

```
 ✓ tests/http-fetch.test.ts (12)

 Test Files  1 passed (1)
      Tests  12 passed (12)
```

---

- [ ] **Step 3: Build and type-check**

```bash
npm run build 2>&1 | tail -20
```

Expected output (no errors; clean exit):

```
> @motosan-ai/sdk@0.3.0 build
> tsc -p tsconfig.json
```

---

- [ ] **Step 4: Commit**

```bash
git add src/http/fetch.ts tests/http-fetch.test.ts && git commit -m "$(cat <<'EOF'
feat(http): add raw fetch wrapper (postJson, postStream)

Implement http/fetch.ts with low-level HTTP POST helpers (no retry; retry
is M3). Both helpers import extractErrorMessage + mapHttpError from error.ts
and throw the mapped MotosanError subclass on !ok responses.

- FetchOptions.signal threads an AbortSignal through to fetch
- postJson<T>(): POST JSON, parse response, map HTTP errors
- postStream(): POST JSON, return response.body ReadableStream

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected output:

```
[<branch> <hash>] feat(http): add raw fetch wrapper (postJson, postStream)
 2 files changed, ...
 create mode 100644 src/http/fetch.ts
 create mode 100644 tests/http-fetch.test.ts
```

---

---

### Task 6: stream.ts: BoxStream + StreamEvent constructors + collectStream

**Files:**
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/stream.ts`
- Create: `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/stream.test.ts`

> Prerequisite: `StreamEvent`, `StreamEventType`, `Usage`, `StopReason`, `ChatResponse`, `ToolCall` are already defined in Task 1's `types.ts` — import them; do not modify `types.ts`.

**Specification:**
- `BoxStream = AsyncIterable<StreamEvent>` (mirror Rust's Pin<Box<dyn Stream>>)
- Constructor helpers: `textEvent`, `doneEvent`, `doneWithStopReason`, `usageEvent`, `toolCallStart`, `toolCallArgs`, `toolCallArgsWithId`, `toolCallEnd`, `toolCallEndWithId`, `thinkingDelta`, `thinkingDone` — each sets `eventType` and `done` correctly
- `collectStream(stream)` logic mirrors Rust lines 29–137:
  - Accumulate `text` events into `content`
  - `toolCallStart`: buffer id/name, clear args
  - `toolCallArgs`: append delta (with or without id)
  - `toolCallEnd`: parse args as JSON (fallback to `{}`), push `{id, name, input}` to toolCalls
  - `usage` events: sum `inputTokens`, `outputTokens`, optional cache tokens (initialized via `get_or_insert(0)`)
  - **Thinking 3-way logic**: prefer non-empty `thinkingDone` text; empty `thinkingDone` → `thinking: undefined`; fallback to delta buffer only if `thinkingDone` never fired
  - stopReason: explicit (from terminal event) > heuristic (`toolCalls.length > 0 ? 'tool_use' : 'end_turn'`)
  - `model` set to `''` (empty string)
  - Break on first `done: true` event (capturing its `stopReason`)

---

- [ ] **Step 1: Verify the type prerequisites from Task 1 exist (do NOT re-declare)**

`stream.ts` builds on `StreamEvent`, `StreamEventType`, `Usage`, `StopReason`, `ChatResponse`, and `ToolCall` — **all created in Task 1's `types.ts`**. Re-declaring them here would cause duplicate-identifier build errors. Confirm they exist; do not modify `types.ts` in this task.

Run from `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript`:

```bash
grep -E "export (type|interface) (StreamEvent|StreamEventType|Usage|StopReason)" src/types.ts
```

Expected output: matching lines for `StreamEvent`, `StreamEventType`, `Usage`, and `StopReason`. If any are missing, finish Task 1 first.

---

- [ ] **Step 2: Write failing test for collectStream (no thinking case)**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/stream.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import {
  textEvent,
  doneEvent,
  doneWithStopReason,
  usageEvent,
  toolCallStart,
  toolCallArgs,
  toolCallEnd,
  collectStream,
  type BoxStream,
} from '../src/stream.js'
import type { StreamEvent } from '../src/types.js'

describe('stream.ts', () => {
  describe('collectStream', () => {
    it('accumulates text and usage for simple response (no thinking)', async () => {
      const events: StreamEvent[] = [
        textEvent('Hello '),
        textEvent('world'),
        usageEvent({ inputTokens: 10, outputTokens: 5 }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.content).toBe('Hello world')
      expect(response.usage.inputTokens).toBe(10)
      expect(response.usage.outputTokens).toBe(5)
      expect(response.model).toBe('')
      expect(response.stopReason).toBe('end_turn')
      expect(response.thinking).toBeUndefined()
      expect(response.toolCalls).toEqual([])
    })
  })
})
```

Run the test to see it fail:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (FAIL):
```
✓ tests/stream.test.ts (FAIL)
  ✗ stream.ts > collectStream > accumulates text and usage for simple response (no thinking)
    [error] Cannot find module '../src/stream.js'
```

---

- [ ] **Step 3: Create stream.ts skeleton with constructor helpers**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/stream.ts`:

```typescript
import type { ChatResponse, StopReason, StreamEvent, Usage } from './types.js'

export type BoxStream = AsyncIterable<StreamEvent>

export function textEvent(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'text',
  }
}

export function doneEvent(): StreamEvent {
  return {
    content: '',
    done: true,
    eventType: 'text',
  }
}

export function doneWithStopReason(stopReason: StopReason): StreamEvent {
  return {
    content: '',
    done: true,
    eventType: 'text',
    stopReason,
  }
}

export function usageEvent(usage: Usage): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'usage',
    usage,
  }
}

export function toolCallStart(id: string, name: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_start',
    toolCallId: id,
    toolCallName: name,
  }
}

export function toolCallArgs(delta: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_args',
    toolCallArgsDelta: delta,
  }
}

export function toolCallArgsWithId(id: string, delta: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_args',
    toolCallId: id,
    toolCallArgsDelta: delta,
  }
}

export function toolCallEnd(): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_end',
  }
}

export function toolCallEndWithId(id: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_end',
    toolCallId: id,
  }
}

export function thinkingDelta(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'thinking_delta',
  }
}

export function thinkingDone(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'thinking_done',
  }
}

export async function collectStream(stream: BoxStream): Promise<ChatResponse> {
  let content = ''
  const toolCalls: any[] = []
  let currentToolId = ''
  let currentToolName = ''
  let currentToolArgs = ''
  let inputTokens = 0
  let outputTokens = 0
  let cacheCreationInputTokens: number | undefined
  let cacheReadInputTokens: number | undefined
  let explicitStopReason: StopReason | undefined
  let thinkingDeltaBuf = ''
  let thinkingDoneBuf: string | undefined

  for await (const event of stream) {
    if (event.done) {
      if (event.stopReason) {
        explicitStopReason = event.stopReason
      }
      break
    }

    switch (event.eventType) {
      case 'text':
        content += event.content
        break

      case 'usage':
        if (event.usage) {
          inputTokens += event.usage.inputTokens
          outputTokens += event.usage.outputTokens
          if (event.usage.cacheCreationInputTokens !== undefined) {
            cacheCreationInputTokens =
              (cacheCreationInputTokens ?? 0) + event.usage.cacheCreationInputTokens
          }
          if (event.usage.cacheReadInputTokens !== undefined) {
            cacheReadInputTokens =
              (cacheReadInputTokens ?? 0) + event.usage.cacheReadInputTokens
          }
        }
        break

      case 'tool_call_start':
        currentToolId = event.toolCallId ?? ''
        currentToolName = event.toolCallName ?? ''
        currentToolArgs = ''
        break

      case 'tool_call_args':
        if (event.toolCallArgsDelta) {
          currentToolArgs += event.toolCallArgsDelta
        }
        break

      case 'tool_call_end': {
        let input: Record<string, unknown> = {}
        try {
          input = JSON.parse(currentToolArgs)
        } catch {
          // Fallback to empty object on parse failure
        }
        toolCalls.push({
          id: currentToolId,
          name: currentToolName,
          input,
        })
        currentToolArgs = ''
        break
      }

      case 'thinking_delta':
        thinkingDeltaBuf += event.content
        break

      case 'thinking_done':
        thinkingDoneBuf = event.content
        thinkingDeltaBuf = ''
        break
    }
  }

  const stopReason: StopReason = explicitStopReason
    ? explicitStopReason
    : toolCalls.length > 0
      ? 'tool_use'
      : 'end_turn'

  let thinking: string | undefined
  if (thinkingDoneBuf !== undefined) {
    thinking = thinkingDoneBuf.length > 0 ? thinkingDoneBuf : undefined
  } else if (thinkingDeltaBuf.length > 0) {
    thinking = thinkingDeltaBuf
  }

  const usage: Usage = {
    inputTokens,
    outputTokens,
  }
  if (cacheCreationInputTokens !== undefined) {
    usage.cacheCreationInputTokens = cacheCreationInputTokens
  }
  if (cacheReadInputTokens !== undefined) {
    usage.cacheReadInputTokens = cacheReadInputTokens
  }

  return {
    content,
    thinking,
    toolCalls,
    model: '',
    usage,
    stopReason,
  }
}
```

Run the test again:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (1 passed)
  ✓ stream.ts > collectStream > accumulates text and usage for simple response (no thinking)
```

---

- [ ] **Step 4: Add test for empty thinking block → undefined**

Add to the `describe('collectStream', () => {...})` block in `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/stream.test.ts`:

```typescript
    it('converts empty thinkingDone block to undefined', async () => {
      const events: StreamEvent[] = [
        thinkingDone(''),
        textEvent('Answer: 42'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.content).toBe('Answer: 42')
      expect(response.thinking).toBeUndefined()
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (1 passed)
  ✓ stream.ts > collectStream > converts empty thinkingDone block to undefined
```

---

- [ ] **Step 5: Add test for delta-only thinking (no thinkingDone)**

Add to the `describe('collectStream', () => {...})` block:

```typescript
    it('falls back to accumulated thinkingDeltas if no thinkingDone fired', async () => {
      const events: StreamEvent[] = [
        thinkingDelta('A '),
        thinkingDelta('B '),
        thinkingDelta('C'),
        textEvent('answer'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.thinking).toBe('A B C')
      expect(response.content).toBe('answer')
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 3 passed)
  ✓ stream.ts > collectStream > falls back to accumulated thinkingDeltas if no thinkingDone fired
```

---

- [ ] **Step 6: Add test for thinking preference (thinkingDone over deltas)**

Add to the `describe('collectStream', () => {...})` block:

```typescript
    it('prefers thinkingDone over accumulated deltas', async () => {
      const events: StreamEvent[] = [
        thinkingDelta('wrong '),
        thinkingDelta('data'),
        thinkingDone('Correct thinking text'),
        textEvent('Final answer'),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.thinking).toBe('Correct thinking text')
      expect(response.content).toBe('Final answer')
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 4 passed)
  ✓ stream.ts > collectStream > prefers thinkingDone over accumulated deltas
```

---

- [ ] **Step 7: Add test for tool call accumulation with JSON parsing**

Add to the `describe('collectStream', () => {...})` block:

```typescript
    it('accumulates tool calls with correct input parsing', async () => {
      const events: StreamEvent[] = [
        toolCallStart('call_1', 'get_weather'),
        toolCallArgs('{"city":"'),
        toolCallArgs('Tokyo",'),
        toolCallArgs('"units":"celsius"}'),
        toolCallEnd(),
        textEvent('Got weather'),
        usageEvent({ inputTokens: 20, outputTokens: 15 }),
        doneWithStopReason('tool_use'),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.toolCalls).toHaveLength(1)
      expect(response.toolCalls[0]).toEqual({
        id: 'call_1',
        name: 'get_weather',
        input: { city: 'Tokyo', units: 'celsius' },
      })
      expect(response.content).toBe('Got weather')
      expect(response.stopReason).toBe('tool_use')
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 5 passed)
  ✓ stream.ts > collectStream > accumulates tool calls with correct input parsing
```

---

- [ ] **Step 8: Add test for stopReason heuristic (tool_use vs end_turn)**

Add to the `describe('collectStream', () => {...})` block:

```typescript
    it('uses stopReason heuristic (tool_use when toolCalls present, end_turn otherwise)', async () => {
      const noToolEvents: StreamEvent[] = [textEvent('hello'), doneEvent()]
      const noToolStream = (async function* () {
        for (const ev of noToolEvents) yield ev
      })() as BoxStream
      const noToolResponse = await collectStream(noToolStream)
      expect(noToolResponse.stopReason).toBe('end_turn')

      const toolEvents: StreamEvent[] = [
        toolCallStart('call_1', 'func'),
        toolCallArgs('{}'),
        toolCallEnd(),
        doneEvent(),
      ]
      const toolStream = (async function* () {
        for (const ev of toolEvents) yield ev
      })() as BoxStream
      const toolResponse = await collectStream(toolStream)
      expect(toolResponse.stopReason).toBe('tool_use')
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 6 passed)
  ✓ stream.ts > collectStream > uses stopReason heuristic (tool_use when toolCalls present, end_turn otherwise)
```

---

- [ ] **Step 9: Add test for cache token summing**

Add to the `describe('collectStream', () => {...})` block:

```typescript
    it('sums cache tokens with lazy initialization', async () => {
      const events: StreamEvent[] = [
        usageEvent({
          inputTokens: 10,
          outputTokens: 5,
          cacheCreationInputTokens: 100,
        }),
        usageEvent({
          inputTokens: 5,
          outputTokens: 3,
          cacheReadInputTokens: 50,
        }),
        usageEvent({
          inputTokens: 2,
          outputTokens: 1,
          cacheCreationInputTokens: 20,
        }),
        doneEvent(),
      ]
      const stream = (async function* () {
        for (const ev of events) yield ev
      })() as BoxStream

      const response = await collectStream(stream)

      expect(response.usage.inputTokens).toBe(17)
      expect(response.usage.outputTokens).toBe(9)
      expect(response.usage.cacheCreationInputTokens).toBe(120)
      expect(response.usage.cacheReadInputTokens).toBe(50)
    })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 7 passed)
  ✓ stream.ts > collectStream > sums cache tokens with lazy initialization
```

---

- [ ] **Step 10: Add test for toolCallArgsWithId helper**

Add to the `describe('stream.ts', () => {...})` block (but outside the `collectStream` sub-describe), a new sub-describe for constructor helpers:

```typescript
  describe('constructor helpers', () => {
    it('toolCallArgsWithId includes id in event', () => {
      const event = toolCallArgsWithId('call_1', '{"key":"value"')
      expect(event).toEqual({
        content: '',
        done: false,
        eventType: 'tool_call_args',
        toolCallId: 'call_1',
        toolCallArgsDelta: '{"key":"value"',
      })
    })

    it('toolCallEnd sets eventType but no id', () => {
      const event = toolCallEnd()
      expect(event.eventType).toBe('tool_call_end')
      expect(event.toolCallId).toBeUndefined()
    })

    it('toolCallEndWithId includes id', () => {
      const event = toolCallEndWithId('call_2')
      expect(event.eventType).toBe('tool_call_end')
      expect(event.toolCallId).toBe('call_2')
    })

    it('thinkingDelta and thinkingDone set correct eventTypes', () => {
      const delta = thinkingDelta('chunk')
      const done = thinkingDone('full text')
      expect(delta.eventType).toBe('thinking_delta')
      expect(delta.content).toBe('chunk')
      expect(done.eventType).toBe('thinking_done')
      expect(done.content).toBe('full text')
    })
  })
```

Run the test:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npx vitest run tests/stream.test.ts
```

Expected output (PASS):
```
✓ tests/stream.test.ts (all 11 passed)
  ✓ stream.ts > constructor helpers > ...
```

---

- [ ] **Step 11: Build and verify no type errors**

Run the TypeScript type checker:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npm run build
```

Expected output:
```
$ tsc -p tsconfig.json
(no errors)
```

---

- [ ] **Step 12: Run full test suite to verify integration**

Run all tests:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && npm run test
```

Expected output:
```
✓ tests/stream.test.ts (all 11 passed)
✓ tests/types.test.ts (passed)
... (other test files)
Test Files: X passed
Tests: X passed
```

---

- [ ] **Step 13: Commit**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript && git add -A && git commit -m "$(cat <<'EOF'
feat: stream.ts BoxStream + StreamEvent constructors + collectStream

Implement BoxStream type (AsyncIterable<StreamEvent>) with constructor
helpers mirroring Rust ctors (textEvent, doneEvent, toolCallStart, etc.).

Implement collectStream with three-way thinking logic:
- Prefer non-empty thinkingDone text over accumulated deltas
- Empty thinkingDone → thinking:undefined (not '')
- Fall back to delta buffer only if thinkingDone never fired

stopReason handling: explicit from terminal event > heuristic
(toolCalls.length>0 ? 'tool_use' : 'end_turn'). Usage tokens summed
including optional cache fields with lazy initialization.

Imports StreamEvent/StreamEventType/Usage/StopReason/ChatResponse/ToolCall
from types.ts (created in Task 1); this task adds no new types.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected output:
```
[feature-branch abcdef1] feat: stream.ts BoxStream + StreamEvent constructors + collectStream
 2 files changed, X insertions(+), Y deletions(-)
 create mode 100644 src/stream.ts
 create mode 100644 tests/stream.test.ts
```

---

### Task 7: serialize/anthropic.ts request serializer

**Files:**
- Create: /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/serialize/anthropic.ts
- Create: /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/serialize.anthropic.test.ts

---

## Step 1: Write the test file with failing tests

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/serialize.anthropic.test.ts`:

```typescript
import { describe, it, expect } from 'vitest'
import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
import type { ContentBlock, Message, SystemBlock } from '../src/types.js'

describe('serializeAnthropicRequest', () => {
  describe('basic structure', () => {
    it('serializes model, max_tokens, and messages', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        maxTokens: 2048,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.model).toBe('claude-opus-4')
      expect(result.max_tokens).toBe(2048)
      expect(Array.isArray(result.messages)).toBe(true)
      expect(result.messages.length).toBe(1)
      expect(result.messages[0].role).toBe('user')
      expect(result.messages[0].content).toBe('hello')
    })

    it('includes optional fields only when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        model: 'claude-sonnet',
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('system' in result).toBe(false)
      expect('tools' in result).toBe(false)
      expect('thinking' in result).toBe(false)
      expect('stop_sequences' in result).toBe(false)
      expect('temperature' in result).toBe(false)
    })
  })

  describe('messages: contentBlocks', () => {
    it('converts user contentBlocks to structured content array', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'Look at this:' },
        { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'Look at this:', contentBlocks },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content.length).toBe(2)
      expect(msg.content[0]).toEqual({ type: 'text', text: 'Look at this:' })
      expect(msg.content[1].type).toBe('image')
      expect(msg.content[1].source.type).toBe('base64')
      expect(msg.content[1].source.media_type).toBe('image/png')
    })

    it('converts document source to snake_case media_type', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'PDF attached' },
        { type: 'document', source: { type: 'base64', mediaType: 'application/pdf', data: 'pdf_data_here' } },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'PDF attached', contentBlocks },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[1]).toEqual({
        type: 'document',
        source: {
          type: 'base64',
          media_type: 'application/pdf',
          data: 'pdf_data_here',
        },
      })
    })

    it('applies cache_control to last block when cache=true', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'First' },
        { type: 'text', text: 'Second' },
      ]
      const req = {
        messages: [
          { role: 'user' as const, content: 'First\nSecond', contentBlocks, cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[0].cache_control).toBeUndefined()
      expect(msg.content[1].cache_control).toEqual({ type: 'ephemeral' })
    })
  })

  describe('messages: plain text with cache', () => {
    it('wraps cached plain-text user message in content block array', () => {
      const req = {
        messages: [
          { role: 'user' as const, content: 'hello', cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content.length).toBe(1)
      expect(msg.content[0]).toEqual({
        type: 'text',
        text: 'hello',
        cache_control: { type: 'ephemeral' },
      })
    })

    it('keeps plain-text user message as string when cache=false', () => {
      const req = {
        messages: [
          { role: 'user' as const, content: 'hello' },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.messages[0].content).toBe('hello')
    })
  })

  describe('messages: assistant toolCalls', () => {
    it('converts assistant toolCalls to tool_use blocks', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: 'Let me check the weather',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
            ],
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0]).toEqual({ type: 'text', text: 'Let me check the weather' })
      expect(msg.content[1]).toEqual({
        type: 'tool_use',
        id: 'toolu_1',
        name: 'get_weather',
        input: { city: 'Tokyo' },
      })
    })

    it('applies cache_control to last tool_use block when cache=true', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: 'checking',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
              { id: 'toolu_2', name: 'get_time', input: {} },
            ],
            cache: true,
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.content[0].cache_control).toBeUndefined()
      expect(msg.content[1].cache_control).toBeUndefined()
      expect(msg.content[2].cache_control).toEqual({ type: 'ephemeral' })
    })

    it('omits text block if assistant content is empty', () => {
      const req = {
        messages: [
          {
            role: 'assistant' as const,
            content: '',
            toolCalls: [
              { id: 'toolu_1', name: 'get_weather', input: { city: 'Tokyo' } },
            ],
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0].type).toBe('tool_use')
    })
  })

  describe('messages: tool role (tool_result)', () => {
    it('converts tool role message to user message with tool_result block', () => {
      const req = {
        messages: [
          { role: 'tool' as const, content: '25C', toolCallId: 'toolu_1' },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      const msg = result.messages[0]
      expect(msg.role).toBe('user')
      expect(Array.isArray(msg.content)).toBe(true)
      expect(msg.content[0]).toEqual({
        type: 'tool_result',
        tool_use_id: 'toolu_1',
        content: '25C',
      })
    })
  })

  describe('system prompt handling', () => {
    it('serializes plain system string as string when no systemBlocks or systemCache', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'You are helpful',
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.system).toBe('You are helpful')
    })

    it('wraps system string in array with cache_control when systemCache=true', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'You are helpful',
        systemCache: true,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(Array.isArray(result.system)).toBe(true)
      expect(result.system).toEqual([
        { type: 'text', text: 'You are helpful', cache_control: { type: 'ephemeral' } },
      ])
    })

    it('serializes systemBlocks as array with per-block cache_control', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'You are a helpful assistant' },
        { text: 'Always be polite', cacheControl: true },
      ]
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        systemBlocks,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(Array.isArray(result.system)).toBe(true)
      expect(result.system).toEqual([
        { type: 'text', text: 'You are a helpful assistant' },
        {
          type: 'text',
          text: 'Always be polite',
          cache_control: { type: 'ephemeral' },
        },
      ])
    })

    it('prioritizes systemBlocks over plain system string', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'Block system' },
      ]
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        system: 'String system',
        systemBlocks,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.system).toEqual([{ type: 'text', text: 'Block system' }])
    })

    it('omits system field when not provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('system' in result).toBe(false)
    })
  })

  describe('tools', () => {
    it('flattens tools to {name, description, input_schema}', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        tools: [
          {
            name: 'get_weather',
            description: 'Get current weather',
            inputSchema: {
              type: 'object',
              properties: { city: { type: 'string' } },
            },
          },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.tools).toEqual([
        {
          name: 'get_weather',
          description: 'Get current weather',
          input_schema: {
            type: 'object',
            properties: { city: { type: 'string' } },
          },
        },
      ])
    })

    it('includes cache_control on last tool when there are multiple tools', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        tools: [
          { name: 'tool1', description: 'First', inputSchema: {} },
          { name: 'tool2', description: 'Second', inputSchema: {}, cache: true },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.tools[0].cache_control).toBeUndefined()
      expect(result.tools[1].cache_control).toEqual({ type: 'ephemeral' })
    })

    it('omits tools field when no tools provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect('tools' in result).toBe(false)
    })
  })

  describe('optional fields', () => {
    it('includes thinking when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        thinking: { budgetTokens: 5000 },
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.thinking).toBeDefined()
    })

    it('includes stop_sequences when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        stopSequences: ['STOP', 'END'],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.stop_sequences).toEqual(['STOP', 'END'])
    })

    it('includes temperature when present', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        temperature: 0.8,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.temperature).toBe(0.8)
    })
  })

  describe('providerOptions', () => {
    it('merges providerOptions into root of result', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
        providerOptions: {
          custom_field: 'value',
          another: 123,
        },
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.custom_field).toBe('value')
      expect(result.another).toBe(123)
    })
  })

  describe('default maxTokens', () => {
    it('uses 8192 as default max_tokens when not provided', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hello' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4')

      expect(result.max_tokens).toBe(8192)
    })
  })
})
```

Run the test to confirm it fails:

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/serialize.anthropic.test.ts
```

Expected output:
```
FAIL  tests/serialize.anthropic.test.ts
Error: Cannot find module '../src/serialize/anthropic.js'
```

---

## Step 2: Create the serialize/anthropic.ts file with the implementation

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/serialize/anthropic.ts`:

```typescript
import type { ChatRequest, ContentBlock, Message } from '../types.js'

const DEFAULT_MAX_TOKENS = 8192

/**
 * Serialize a ContentBlock to Anthropic JSON format.
 * - image/document sources use snake_case `media_type` on the wire
 * - text blocks are plain objects
 */
function serializeContentBlock(block: ContentBlock): Record<string, unknown> {
  if (block.type === 'text') {
    return { type: 'text', text: block.text }
  }

  if (block.type === 'image') {
    const source = block.source
    if (source.type === 'base64') {
      return {
        type: 'image',
        source: {
          type: 'base64',
          media_type: source.mediaType,
          data: source.data,
        },
      }
    }
    return {
      type: 'image',
      source: {
        type: 'url',
        url: source.url,
      },
    }
  }

  if (block.type === 'document') {
    const source = block.source
    if (source.type === 'base64') {
      return {
        type: 'document',
        source: {
          type: 'base64',
          media_type: source.mediaType,
          data: source.data,
        },
      }
    }
    return {
      type: 'document',
      source: {
        type: 'url',
        url: source.url,
      },
    }
  }

  const _exhaustive: never = block
  throw new Error(`Unknown content block type: ${_exhaustive}`)
}

/**
 * Serialize system blocks to Anthropic array format.
 * Each block includes optional cache_control based on its cacheControl flag.
 */
function serializeSystemBlocks(
  blocks: Array<{ text: string; cacheControl?: boolean }>,
): Array<Record<string, unknown>> {
  return blocks.map((b) => {
    const obj: Record<string, unknown> = {
      type: 'text',
      text: b.text,
    }
    if (b.cacheControl) {
      obj.cache_control = { type: 'ephemeral' }
    }
    return obj
  })
}

/**
 * Serialize a ChatRequest to Anthropic API format.
 *
 * Conversion rules:
 * - messages: user/assistant/system/tool roles
 *   - contentBlocks -> structured content array (text/image/document)
 *   - cache=true -> cache_control on LAST block
 *   - assistant toolCalls -> tool_use blocks array
 *   - tool role -> user message with tool_result block
 * - system:
 *   - systemBlocks -> array of {type,text,cache_control?} (takes priority)
 *   - systemCache=true -> wrap plain system string in single-block array
 *   - else -> plain string (omit if absent)
 * - tools: flatten to {name, description, input_schema}
 *   - cache on last tool
 * - optional fields: thinking, stop_sequences, temperature, providerOptions
 */
export function serializeAnthropicRequest(
  req: ChatRequest,
  model: string,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    model,
    max_tokens: req.maxTokens ?? DEFAULT_MAX_TOKENS,
    messages: [],
  }

  // Serialize messages
  const messages: Array<Record<string, unknown>> = []

  for (const message of req.messages) {
    if (message.role === 'system') {
      // System messages are not sent as message blocks; they are extracted
      // and handled via the system field. This path should not normally occur
      // in well-formed requests, but we skip it here.
      continue
    }

    if (message.role === 'user') {
      if (message.contentBlocks && message.contentBlocks.length > 0) {
        // Structured content blocks (vision/multimodal/document)
        const blocks = message.contentBlocks.map(serializeContentBlock)
        // Apply cache_control to last block if cache=true
        if (message.cache && blocks.length > 0) {
          const last = blocks[blocks.length - 1]
          last.cache_control = { type: 'ephemeral' }
        }
        messages.push({
          role: 'user',
          content: blocks,
        })
      } else if (message.cache) {
        // Cached plain-text: wrap in block array with cache_control
        messages.push({
          role: 'user',
          content: [
            {
              type: 'text',
              text: message.content,
              cache_control: { type: 'ephemeral' },
            },
          ],
        })
      } else {
        // Plain string content
        messages.push({
          role: 'user',
          content: message.content,
        })
      }
    } else if (message.role === 'assistant') {
      if (message.toolCalls && message.toolCalls.length > 0) {
        // Assistant with tool calls: build block array
        const blocks: Array<Record<string, unknown>> = []

        // Add text block if content is non-empty
        if (message.content.trim().length > 0) {
          blocks.push({
            type: 'text',
            text: message.content,
          })
        }

        // Add tool_use blocks
        for (const tc of message.toolCalls) {
          blocks.push({
            type: 'tool_use',
            id: tc.id,
            name: tc.name,
            input: tc.input,
          })
        }

        // Apply cache_control to last block if cache=true
        if (message.cache && blocks.length > 0) {
          const last = blocks[blocks.length - 1]
          last.cache_control = { type: 'ephemeral' }
        }

        messages.push({
          role: 'assistant',
          content: blocks,
        })
      } else {
        // Assistant without tool calls
        if (message.cache) {
          // Wrap in block array with cache_control
          messages.push({
            role: 'assistant',
            content: [
              {
                type: 'text',
                text: message.content,
                cache_control: { type: 'ephemeral' },
              },
            ],
          })
        } else {
          // Plain string content
          messages.push({
            role: 'assistant',
            content: message.content,
          })
        }
      }
    } else if (message.role === 'tool') {
      // Tool response: becomes user message with tool_result block
      if (message.toolCallId) {
        messages.push({
          role: 'user',
          content: [
            {
              type: 'tool_result',
              tool_use_id: message.toolCallId,
              content: message.content,
            },
          ],
        })
      }
    }
  }

  result.messages = messages

  // System prompt
  if (req.systemBlocks && req.systemBlocks.length > 0) {
    // Priority: systemBlocks takes precedence
    result.system = serializeSystemBlocks(req.systemBlocks)
  } else if (req.system) {
    if (req.systemCache) {
      // Wrap in single-block array with cache_control
      result.system = [
        {
          type: 'text',
          text: req.system,
          cache_control: { type: 'ephemeral' },
        },
      ]
    } else {
      // Plain string
      result.system = req.system
    }
  }

  // Tools
  if (req.tools && req.tools.length > 0) {
    result.tools = req.tools.map((tool, index) => {
      const obj: Record<string, unknown> = {
        name: tool.name,
        description: tool.description,
        input_schema: tool.inputSchema,
      }
      // Apply cache_control to last tool if it has cache=true
      if (tool.cache) {
        obj.cache_control = { type: 'ephemeral' }
      }
      return obj
    })
  }

  // Optional fields
  if (req.thinking) {
    result.thinking = req.thinking
  }

  if (req.stopSequences && req.stopSequences.length > 0) {
    result.stop_sequences = req.stopSequences
  }

  if (req.temperature !== undefined) {
    result.temperature = req.temperature
  }

  // Merge providerOptions into root
  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(result, req.providerOptions)
  }

  return result
}
```

---

## Step 3: Run the tests to check for failures

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npx vitest run tests/serialize.anthropic.test.ts
```

Expected output: All tests pass (or identify any specific failures to fix).

---

## Step 4: Update types.ts to include new types

The spec requires that `types.ts` includes the full contract from the locked contract. Verify that the types include:
- `ContentBlock` with variants: text, image, document
- `ImageSource` and `DocumentSource`
- `SystemBlock` with optional `cacheControl`
- `ThinkingConfig`
- All other required types

If not already present, they should be added as part of the M1 types setup (assumed to be Task 1).

---

## Step 5: Build and verify type safety

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npm run build
```

Expected output:
```
✓ No TypeScript errors
```

---

## Step 6: Run the full test suite to ensure no regressions

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
npm run test
```

Expected output:
```
✓ All tests pass
✓ serialize.anthropic.test.ts passes
```

---

## Step 7: Commit the changes

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript
git add src/serialize/anthropic.ts tests/serialize.anthropic.test.ts
git commit -m "feat(serialize): implement serializeAnthropicRequest with message/system/tools serialization

- Serialize contentBlocks to structured format with snake_case media_type
- Apply cache_control to last block/tool when cache=true
- Handle assistant toolCalls as tool_use blocks
- Support tool role as user message with tool_result
- System: systemBlocks array, systemCache boolean, or plain string (priority order)
- Tools: flatten to {name, description, input_schema} with per-tool cache support
- Include optional fields: thinking, stop_sequences, temperature
- Merge providerOptions into root

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

Expected output:
```
✓ 2 files changed, X insertions(+)
✓ create mode 100644 src/serialize/anthropic.ts
✓ create mode 100644 tests/serialize.anthropic.test.ts
```

---

### Task 8: providers/anthropic.ts rewrite (self-implemented chat + stream)

**Files:**
- **Modify:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/providers/anthropic.ts`
- **Test:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/providers-anthropic.test.ts`

> Depends on Task 5 (`http/fetch.ts`), Task 3 (`http/sse.ts`), Task 6 (`stream.ts` helpers), and Task 7 (`serialize/anthropic.ts`). This task wires those together — it does NOT define its own SSE parser, its own `streamEvent()` factory, or its own request serializer. It imports `postJson`/`postStream` from `../http/fetch.js`, `parseSse` from `../http/sse.js`, `serializeAnthropicRequest` from `../serialize/anthropic.js`, and the `StreamEvent` constructor helpers from `../stream.js`. Note: `parseSse` yields `SseEvent { event?: string; data: any }` — read `evt.event` (the SSE event name) and `evt.data` (the already-parsed JSON), NOT `{ eventType, payload }`.

---

- [ ] **Step 1: Write the failing test for chat request body + response parsing**

Create `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/providers-anthropic.test.ts`:

```typescript
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import type { ChatRequest, StreamEvent } from '../src/types.js'

describe('AnthropicProvider chat', () => {
  let capturedRequest: { url: string; headers: Record<string, string>; body: any } | null = null

  beforeEach(() => {
    capturedRequest = null
    const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
      capturedRequest = {
        url,
        headers: (options?.headers as Record<string, string>) ?? {},
        body: options?.body ? JSON.parse(String(options.body)) : null,
      }
      return new Response(
        JSON.stringify({
          id: 'msg_1',
          type: 'message',
          role: 'assistant',
          content: [{ type: 'text', text: 'Hello, world!' }],
          model: 'claude-3-5-sonnet-20241022',
          stop_reason: 'end_turn',
          stop_sequence: null,
          usage: {
            input_tokens: 10,
            output_tokens: 5,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
          },
        }),
        { status: 200, headers: { 'content-type': 'application/json' } },
      )
    })
    vi.stubGlobal('fetch', mockFetch)
  })

  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('sends correct URL + headers and parses a text response', async () => {
    const provider = new AnthropicProvider(
      'test-api-key',
      'claude-3-5-sonnet-20241022',
      'https://api.anthropic.com',
    )
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'Hello' }],
      maxTokens: 100,
    }

    const response = await provider.chat(request)

    expect(capturedRequest?.url).toBe('https://api.anthropic.com/v1/messages')
    expect(capturedRequest?.headers['x-api-key']).toBe('test-api-key')
    expect(capturedRequest?.headers['anthropic-version']).toBe('2023-06-01')
    expect(capturedRequest?.headers['content-type']).toBe('application/json')
    expect(capturedRequest?.body.model).toBe('claude-3-5-sonnet-20241022')
    expect(capturedRequest?.body.max_tokens).toBe(100)
    expect(capturedRequest?.body.stream).toBeUndefined()

    expect(response.content).toBe('Hello, world!')
    expect(response.model).toBe('claude-3-5-sonnet-20241022')
    expect(response.toolCalls).toEqual([])
    expect(response.usage.inputTokens).toBe(10)
    expect(response.usage.outputTokens).toBe(5)
    expect(response.stopReason).toBe('end_turn')
  })

  it('parses cache tokens from a non-streaming usage block', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'cached' }],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: {
              input_tokens: 10,
              output_tokens: 5,
              cache_creation_input_tokens: 100,
              cache_read_input_tokens: 200,
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.usage.cacheCreationInputTokens).toBe(100)
    expect(response.usage.cacheReadInputTokens).toBe(200)
  })

  it('parses tool_use blocks into toolCalls', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [
              { type: 'text', text: 'Let me check' },
              { type: 'tool_use', id: 'tool_1', name: 'get_weather', input: { city: 'NYC' } },
            ],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'tool_use',
            usage: { input_tokens: 10, output_tokens: 5 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'weather' }] })

    expect(response.content).toBe('Let me check')
    expect(response.toolCalls).toHaveLength(1)
    expect(response.toolCalls[0]).toEqual({
      id: 'tool_1',
      name: 'get_weather',
      input: { city: 'NYC' },
    })
    expect(response.stopReason).toBe('tool_use')
  })

  it('extracts thinking content when present', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [
              { type: 'thinking', thinking: 'Let me think about this...' },
              { type: 'text', text: 'The answer is 42' },
            ],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: { input_tokens: 10, output_tokens: 5 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        ),
      ),
    )

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const response = await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

    expect(response.thinking).toBe('Let me think about this...')
    expect(response.content).toBe('The answer is 42')
  })

  it('capabilities() reports image + document support', () => {
    const provider = new AnthropicProvider('key')
    expect(provider.capabilities()).toEqual({ supportsImage: true, supportsDocument: true })
  })
})
```

Run the test to confirm it fails:

```bash
npx vitest run tests/providers-anthropic.test.ts 2>&1 | tail -30
```

Expected output (the current `anthropic.ts` still imports `@anthropic-ai/sdk` / has the old shape, so chat assertions fail or the module errors). The key signal is that the chat tests are RED:

```
 FAIL  tests/providers-anthropic.test.ts > AnthropicProvider chat > sends correct URL + headers and parses a text response
```

---

- [ ] **Step 2: Add the failing stream test (hand-authored SSE transcript)**

Append the streaming `describe` block to `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/providers-anthropic.test.ts`. The transcript style mirrors `sdks/rust/tests/anthropic_stream.rs` (per-event `event:`/`data:` lines separated by blank lines):

```typescript
describe('AnthropicProvider stream', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  function streamFromTranscript(sse: string): void {
    const bytes = new TextEncoder().encode(sse)
    const mockStream = new ReadableStream<Uint8Array>({
      start(controller) {
        controller.enqueue(bytes)
        controller.close()
      },
    })
    vi.stubGlobal(
      'fetch',
      vi.fn(async () =>
        new Response(mockStream, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        }),
      ),
    )
  }

  it('maps a full thinking + text + tool_use + usage transcript to StreamEvents', async () => {
    const sse =
      'event: message_start\n' +
      'data: {"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"model":"claude-3-5-sonnet-20241022","stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":100,"output_tokens":0,"cache_creation_input_tokens":50,"cache_read_input_tokens":0}}}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think..."}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"sig..."}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":0}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"The "}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":1}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":2,"content_block":{"type":"tool_use","id":"tool_xyz","name":"calculator","input":{}}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"{"}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"\\"x\\": 2"}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":2,"delta":{"type":"input_json_delta","partial_json":"}"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":2}\n\n' +
      'event: message_delta\n' +
      'data: {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"input_tokens":100,"output_tokens":20}}\n\n' +
      'event: message_stop\n' +
      'data: {"type":"message_stop"}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
      events.push(event)
    }

    // Exact ordered sequence the adapter must emit.
    const labels = events.map((e) => `${e.eventType}${e.done ? ':done' : ''}`)
    expect(labels).toEqual([
      'usage',
      'thinking_delta',
      'thinking_delta',
      'thinking_done',
      'text',
      'text',
      'tool_call_start',
      'tool_call_args',
      'tool_call_args',
      'tool_call_args',
      'tool_call_end',
      'usage',
      'text:done', // doneWithStopReason yields a default-eventType ('text') event with done=true
    ])

    const usageEvents = events.filter((e) => e.eventType === 'usage')
    expect(usageEvents).toHaveLength(2)
    // message_start usage carries cache tokens
    expect(usageEvents[0].usage?.inputTokens).toBe(100)
    expect(usageEvents[0].usage?.cacheCreationInputTokens).toBe(50)
    // message_delta usage carries NO cache tokens
    expect(usageEvents[1].usage?.inputTokens).toBe(100)
    expect(usageEvents[1].usage?.outputTokens).toBe(20)
    expect(usageEvents[1].usage?.cacheCreationInputTokens).toBeUndefined()
    expect(usageEvents[1].usage?.cacheReadInputTokens).toBeUndefined()

    const thinkingDeltas = events.filter((e) => e.eventType === 'thinking_delta')
    expect(thinkingDeltas.map((e) => e.content)).toEqual(['Let me ', 'think...'])

    const thinkingDone = events.filter((e) => e.eventType === 'thinking_done')
    expect(thinkingDone).toHaveLength(1)
    expect(thinkingDone[0].content).toBe('Let me think...')

    const textEvents = events.filter((e) => e.eventType === 'text' && !e.done)
    expect(textEvents.map((e) => e.content)).toEqual(['The ', 'answer'])

    const toolStart = events.filter((e) => e.eventType === 'tool_call_start')
    expect(toolStart).toHaveLength(1)
    expect(toolStart[0].toolCallId).toBe('tool_xyz')
    expect(toolStart[0].toolCallName).toBe('calculator')

    const toolArgs = events.filter((e) => e.eventType === 'tool_call_args')
    expect(toolArgs).toHaveLength(3)
    expect(toolArgs.every((e) => e.toolCallId === 'tool_xyz')).toBe(true)
    expect(toolArgs.map((e) => e.toolCallArgsDelta).join('')).toBe('{"x": 2}')

    const toolEnd = events.filter((e) => e.eventType === 'tool_call_end')
    expect(toolEnd).toHaveLength(1)
    expect(toolEnd[0].toolCallId).toBe('tool_xyz')

    const doneEvents = events.filter((e) => e.done)
    expect(doneEvents).toHaveLength(1)
    expect(doneEvents[0].stopReason).toBe('tool_use')
  })

  it('ignores redacted_thinking and signature_delta, emits only text + done', async () => {
    const sse =
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":0,"content_block":{"type":"redacted_thinking","data":"blob"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":0}\n\n' +
      'event: content_block_start\n' +
      'data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}\n\n' +
      'event: content_block_delta\n' +
      'data: {"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"answer"}}\n\n' +
      'event: content_block_stop\n' +
      'data: {"type":"content_block_stop","index":1}\n\n' +
      'event: message_stop\n' +
      'data: {"type":"message_stop"}\n\n'

    streamFromTranscript(sse)

    const provider = new AnthropicProvider('key', 'claude-3-5-sonnet-20241022')
    const events: StreamEvent[] = []
    for await (const event of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
      events.push(event)
    }

    expect(events.some((e) => e.eventType === 'thinking_delta')).toBe(false)
    expect(events.some((e) => e.eventType === 'thinking_done')).toBe(false)
    expect(events.filter((e) => e.eventType === 'text' && !e.done).map((e) => e.content)).toEqual([
      'answer',
    ])
    const doneEvents = events.filter((e) => e.done)
    expect(doneEvents).toHaveLength(1)
    expect(doneEvents[0].stopReason).toBeUndefined()
  })
})

describe('AnthropicProvider live', () => {
  it.skipIf(!process.env.ANTHROPIC_API_KEY)('streams from the real API', async () => {
    const provider = new AnthropicProvider(process.env.ANTHROPIC_API_KEY as string)
    const chunks: string[] = []
    let sawDone = false
    for await (const event of provider.stream({
      messages: [{ role: 'user', content: 'reply with OK' }],
      maxTokens: 32,
    })) {
      if (event.done) sawDone = true
      else if (event.eventType === 'text') chunks.push(event.content)
    }
    expect(sawDone).toBe(true)
    expect(chunks.join('').length).toBeGreaterThan(0)
  }, 60_000)
})
```

Run the streaming + chat tests; they must be RED (old implementation):

```bash
npx vitest run tests/providers-anthropic.test.ts 2>&1 | tail -30
```

Expected output (chat + stream assertions fail against the old provider):

```
 FAIL  tests/providers-anthropic.test.ts > AnthropicProvider stream > maps a full thinking + text + tool_use + usage transcript to StreamEvents
```

---

- [ ] **Step 3: Rewrite `src/providers/anthropic.ts`**

Replace the entire contents of `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/providers/anthropic.ts`. It imports the HTTP layer, the SSE parser, the request serializer, and the stream-event constructor helpers — it defines NO local `parseSse`, NO local `streamEvent`, and NO local `serializeMessages`:

```typescript
import { ProviderError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { serializeAnthropicRequest } from '../serialize/anthropic.js'
import {
  doneEvent,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  thinkingDone,
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
  ToolCall,
  Usage,
} from '../types.js'

const ANTHROPIC_VERSION = '2023-06-01'

function parseStopReason(reason: unknown): StopReason {
  switch (reason) {
    case 'end_turn':
      return 'end_turn'
    case 'max_tokens':
      return 'max_tokens'
    case 'tool_use':
      return 'tool_use'
    case 'stop_sequence':
      return 'stop_sequence'
    case 'stop':
      return 'stop'
    default:
      return 'other'
  }
}

/** Build a Usage object, omitting cache fields unless explicitly provided. */
function toUsage(raw: any, includeCache: boolean): Usage {
  const usage: Usage = {
    inputTokens: Number(raw?.input_tokens ?? 0),
    outputTokens: Number(raw?.output_tokens ?? 0),
  }
  if (includeCache) {
    if (raw?.cache_creation_input_tokens != null) {
      usage.cacheCreationInputTokens = Number(raw.cache_creation_input_tokens)
    }
    if (raw?.cache_read_input_tokens != null) {
      usage.cacheReadInputTokens = Number(raw.cache_read_input_tokens)
    }
  }
  return usage
}

interface StreamState {
  currentToolId?: string
  currentThinkingBuf?: string
  stopReason?: StopReason
}

export class AnthropicProvider {
  private readonly model: string
  private readonly baseUrl: string

  constructor(
    private readonly apiKey: string,
    model?: string,
    baseUrl = 'https://api.anthropic.com',
  ) {
    this.model = model ?? 'claude-3-5-sonnet-20241022'
    this.baseUrl = baseUrl
  }

  private headers(): Record<string, string> {
    return {
      'x-api-key': this.apiKey,
      'anthropic-version': ANTHROPIC_VERSION,
      'content-type': 'application/json',
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const body = serializeAnthropicRequest(request, request.model ?? this.model)
    const payload = await postJson<any>(`${this.baseUrl}/v1/messages`, this.headers(), body)

    const blocks: any[] = Array.isArray(payload?.content) ? payload.content : []

    const content = blocks
      .filter((b) => b?.type === 'text')
      .map((b) => String(b?.text ?? ''))
      .join('')

    const thinking =
      blocks
        .filter((b) => b?.type === 'thinking')
        .map((b) => String(b?.thinking ?? ''))
        .join('') || undefined

    const toolCalls: ToolCall[] = blocks
      .filter((b) => b?.type === 'tool_use')
      .map((b) => ({
        id: String(b?.id ?? ''),
        name: String(b?.name ?? ''),
        input: (b?.input ?? {}) as Record<string, unknown>,
      }))
      .filter((tc) => tc.id && tc.name)

    return {
      content,
      ...(thinking ? { thinking } : {}),
      toolCalls,
      model: String(payload?.model ?? this.model),
      usage: toUsage(payload?.usage ?? {}, true),
      stopReason: parseStopReason(payload?.stop_reason),
    }
  }

  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  private async *streamImpl(request: ChatRequest) {
    const body = {
      ...serializeAnthropicRequest(request, request.model ?? this.model),
      stream: true,
    }
    const responseBody = await postStream(`${this.baseUrl}/v1/messages`, this.headers(), body)

    const state: StreamState = {}

    for await (const evt of parseSse(responseBody)) {
      const data = evt.data
      if (!data) continue

      switch (evt.event) {
        case 'message_start': {
          const usage = data?.message?.usage
          if (usage) {
            yield usageEvent(toUsage(usage, true))
          }
          break
        }

        case 'content_block_start': {
          const block = data?.content_block
          if (block?.type === 'tool_use') {
            const id = String(block.id ?? '')
            const name = String(block.name ?? '')
            state.currentToolId = id
            yield toolCallStart(id, name)
          } else if (block?.type === 'thinking') {
            state.currentThinkingBuf = ''
          }
          // redacted_thinking and text blocks: nothing to emit on start.
          break
        }

        case 'content_block_delta': {
          const delta = data?.delta
          if (!delta) break

          if (delta.type === 'text_delta') {
            const text = String(delta.text ?? '')
            if (text) yield textEvent(text)
          } else if (delta.type === 'input_json_delta') {
            const partial = String(delta.partial_json ?? '')
            if (partial && state.currentToolId !== undefined) {
              yield toolCallArgsWithId(state.currentToolId, partial)
            }
          } else if (delta.type === 'thinking_delta') {
            const text = String(delta.thinking ?? '')
            if (text) {
              if (state.currentThinkingBuf !== undefined) {
                state.currentThinkingBuf += text
              }
              yield thinkingDelta(text)
            }
          }
          // signature_delta and any other delta types are ignored.
          break
        }

        case 'content_block_stop': {
          if (state.currentToolId !== undefined) {
            const id = state.currentToolId
            state.currentToolId = undefined
            yield toolCallEndWithId(id)
          }
          if (state.currentThinkingBuf !== undefined) {
            const buf = state.currentThinkingBuf
            state.currentThinkingBuf = undefined
            yield thinkingDone(buf)
          }
          break
        }

        case 'message_delta': {
          const reason = data?.delta?.stop_reason
          if (reason) state.stopReason = parseStopReason(reason)
          const usage = data?.usage
          if (usage) {
            // message_delta usage carries NO cache tokens.
            yield usageEvent(toUsage(usage, false))
          }
          break
        }

        case 'message_stop': {
          yield state.stopReason !== undefined
            ? doneWithStopReason(state.stopReason)
            : doneEvent()
          return
        }

        default:
          // ping / unknown events are ignored.
          break
      }
    }

    // Defensive: terminate even if message_stop never arrived.
    if (state.stopReason !== undefined) {
      yield doneWithStopReason(state.stopReason)
    } else {
      yield doneEvent()
    }
  }

  capabilities(): { supportsImage: boolean; supportsDocument: boolean } {
    return { supportsImage: true, supportsDocument: true }
  }
}
```

> Note on `ProviderError`: the import is retained because Task 5's `postStream` and Task 7's serializer surface domain errors; if your linter flags it as unused after wiring, drop it from the import line — the wire behavior does not depend on it being referenced here.

Run the chat tests:

```bash
npx vitest run tests/providers-anthropic.test.ts -t "AnthropicProvider chat" 2>&1 | tail -20
```

Expected output:

```
 ✓ tests/providers-anthropic.test.ts (5)
   ✓ AnthropicProvider chat (5)
```

---

- [ ] **Step 4: Run the full provider test file (chat + stream)**

```bash
npx vitest run tests/providers-anthropic.test.ts 2>&1 | tail -30
```

Expected output (the live test is skipped without an API key):

```
 ✓ tests/providers-anthropic.test.ts (8)
   ✓ AnthropicProvider chat (5)
   ✓ AnthropicProvider stream (2)
   ↓ AnthropicProvider live > streams from the real API (skipped)

 Test Files  1 passed (1)
      Tests  7 passed | 1 skipped (8)
```

---

- [ ] **Step 5: Build and type-check**

```bash
npm run build 2>&1 | tail -20
```

Expected output (clean compile):

```
> @motosan-ai/sdk@0.3.0 build
> tsc -p tsconfig.json
```

---

- [ ] **Step 6: Commit**

```bash
git add src/providers/anthropic.ts tests/providers-anthropic.test.ts && git commit -m "$(cat <<'EOF'
feat(anthropic): self-implement chat + stream without @anthropic-ai/sdk

Rewrite AnthropicProvider on the shared HTTP/SSE/serialize layer:
- chat() via postJson + serializeAnthropicRequest; parses text/thinking/
  tool_use blocks, cache tokens, and stop_reason
- stream() via postStream + parseSse, emitting StreamEvents through the
  stream.ts constructor helpers (no local streamEvent/parseSse)
- tracks currentToolId (Anthropic sends it only in content_block_start)
- message_start usage carries cache tokens; message_delta carries none
- ignores redacted_thinking and signature_delta

Hand-authored SSE transcript test asserts the full ordered StreamEvent
sequence; env-gated live test covers the real API.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected output:

```
[<branch> <hash>] feat(anthropic): self-implement chat + stream without @anthropic-ai/sdk
 2 files changed, ...
 create mode 100644 tests/providers-anthropic.test.ts
```

---

---

### Task 9: wire up index.ts / client.ts / package.json; drop @anthropic-ai/sdk

**Files:**
- **Modify:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/index.ts`
- **Modify:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/client.ts`
- **Modify:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/package.json`
- **Test:** `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/client.test.ts`

> MINIMAL integration task. Do NOT reimplement serialize/fetch/sse/provider — those land in Tasks 3-8. This task only: (1) exports the public surface from `index.ts`; (2) routes `provider:'anthropic'` through the self-hosted `AnthropicProvider`; (3) removes `@anthropic-ai/sdk` from `package.json`. `openai`/`minimax` routing is untouched (their rewrites are M2/M4).

---

- [ ] **Step 1: Write the failing test for self-hosted anthropic routing**

Append a new `describe` block to the existing `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/tests/client.test.ts` (keep the existing `client` describe block intact). The mocked-fetch test proves the `'anthropic'` string resolves to the self-hosted provider with no `@anthropic-ai/sdk` involvement:

```typescript
describe('client anthropic routing', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('routes provider:"anthropic" to the self-hosted AnthropicProvider', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'msg_1',
            type: 'message',
            role: 'assistant',
            content: [{ type: 'text', text: 'ok' }],
            model: 'claude-3-5-sonnet-20241022',
            stop_reason: 'end_turn',
            usage: { input_tokens: 1, output_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'anthropic', apiKey: 'test-key' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    // Self-hosted provider hits /v1/messages directly with x-api-key.
    expect(capturedUrl).toContain('/v1/messages')
    expect(capturedHeaders['x-api-key']).toBe('test-key')
    expect(capturedHeaders['anthropic-version']).toBe('2023-06-01')
    expect(response.content).toBe('ok')
  })
})
```

Update the import line at the top of `tests/client.test.ts` so `vi` and `afterEach` are available alongside the existing imports. Change:

```typescript
import { describe, expect, it } from 'vitest'
```

to:

```typescript
import { afterEach, describe, expect, it, vi } from 'vitest'
```

Run the test:

```bash
npx vitest run tests/client.test.ts 2>&1 | tail -20
```

Expected output (PASS already if Task 8 landed, since `client.ts` constructs the new `AnthropicProvider` with `(apiKey, model)`):

```
 ✓ tests/client.test.ts (2)
```

If the existing `client.ts` still constructs the old provider in a way that does not hit `/v1/messages`, this test is RED until Step 2 confirms the routing. Either way, proceed to Step 2.

---

- [ ] **Step 2: Confirm `src/client.ts` routes anthropic to the self-hosted provider**

`client.ts` already imports `AnthropicProvider` from `./providers/anthropic.js` and constructs it as `new AnthropicProvider(apiKey, options.model)`. The self-hosted provider's constructor signature is `(apiKey: string, model?: string, baseUrl?: string)` — identical for the first two args — so no change to the construction call is required. Keep `ProviderName`, `ProviderLike`, and `Client` exactly as they are. Verify the file reads as follows (no edits needed if it already matches):

```typescript
import { ConfigError } from './error.js'
import { AnthropicProvider } from './providers/anthropic.js'
import { MinimaxProvider } from './providers/minimax.js'
import { OpenAIProvider } from './providers/openai.js'
import type { ChatRequest, ChatResponse, StreamEvent } from './types.js'

export type ProviderName = 'anthropic' | 'openai' | 'minimax'

export interface ProviderLike {
  chat(request: ChatRequest): Promise<ChatResponse>
  stream(request: ChatRequest): AsyncGenerator<StreamEvent>
}

export class Client {
  private provider: ProviderLike

  constructor(options: {
    provider: ProviderName | ProviderLike
    apiKey?: string
    model?: string
    minimaxEndpoint?: string
  }) {
    if (typeof options.provider !== 'string') {
      this.provider = options.provider
      return
    }

    const provider = options.provider
    const envMap: Record<ProviderName, string> = {
      anthropic: 'ANTHROPIC_API_KEY',
      openai: 'OPENAI_API_KEY',
      minimax: 'MINIMAX_API_KEY'
    }

    const apiKey = options.apiKey ?? process.env[envMap[provider]]
    if (!apiKey) {
      throw new ConfigError(`Missing API key for provider ${provider}`)
    }

    if (provider === 'anthropic') {
      this.provider = new AnthropicProvider(apiKey, options.model)
    } else if (provider === 'openai') {
      this.provider = new OpenAIProvider(apiKey, options.model)
    } else {
      this.provider = new MinimaxProvider(apiKey, options.model, options.minimaxEndpoint)
    }
  }

  async chat(request: ChatRequest): Promise<ChatResponse> {
    return this.provider.chat(request)
  }

  stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
    return this.provider.stream(request)
  }
}
```

> The `ProviderLike.stream` return type is `AsyncGenerator<StreamEvent>`. The self-hosted `AnthropicProvider.stream` returns a `BoxStream` (`AsyncIterable<StreamEvent>`); an `async *` generator satisfies `AsyncIterable`, and the assignment is via structural typing of `chat`/`stream`. If `tsc` complains that `BoxStream` is not assignable to `AsyncGenerator`, leave `client.ts`'s `ProviderLike.stream` typed as `AsyncIterable<StreamEvent>` instead — but do NOT change `chat`/the rest of the shape. Run the build (Step 5) to confirm before editing.

Re-run the client test to confirm GREEN:

```bash
npx vitest run tests/client.test.ts 2>&1 | tail -20
```

Expected output:

```
 ✓ tests/client.test.ts (2)

 Test Files  1 passed (1)
      Tests  2 passed (2)
```

---

- [ ] **Step 3: Update `src/index.ts` to export only the public surface**

Overwrite `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/src/index.ts` so it re-exports the public modules and does NOT export internal `http/*` or `serialize/*`:

```typescript
export * from './types.js'
export * from './message.js'
export * from './stream.js'
export * from './error.js'
export * from './client.js'
export * from './providers/anthropic.js'
export * from './providers/openai.js'
export * from './providers/minimax.js'
```

---

- [ ] **Step 4: Remove `@anthropic-ai/sdk` from `package.json`**

Edit `/Users/daiwanwei/Projects/wade/motosan-ai/sdks/typescript/package.json`. Remove the `@anthropic-ai/sdk` entry from `peerDependencies`, from `peerDependenciesMeta`, and from `devDependencies`. KEEP `openai` in all three. The resulting dependency blocks must read:

```json
  "peerDependencies": {
    "openai": ">=4.0.0"
  },
  "peerDependenciesMeta": {
    "openai": {
      "optional": true
    }
  },
  "devDependencies": {
    "@types/node": "^22.13.10",
    "openai": "^6.4.0",
    "typescript": "^5.8.2",
    "vitest": "^3.0.8"
  }
```

Then refresh the lockfile so `npm` does not error on the removed dependency:

```bash
npm install 2>&1 | tail -5
```

Expected output (lockfile updated, no errors):

```
up to date, audited ... packages in ...s
```

---

- [ ] **Step 5: Verify no `@anthropic-ai/sdk` remains, then build + test**

Confirm the dependency is fully gone from source:

```bash
grep -rn '@anthropic-ai/sdk' src 2>&1; echo "exit=$?"
```

Expected output (no matches; grep exit code 1 means "not found", which is the success condition here):

```
exit=1
```

Run the full build and test suite:

```bash
npm run build 2>&1 | tail -10 && npm run test 2>&1 | tail -25
```

Expected output (clean build; all suites pass, live tests skipped without keys):

```
> @motosan-ai/sdk@0.3.0 build
> tsc -p tsconfig.json

> @motosan-ai/sdk@0.3.0 test
> vitest run

 ... (all test files passed) ...

 Test Files  N passed (N)
      Tests  M passed | K skipped (M+K)
```

---

- [ ] **Step 6: Commit**

```bash
git add src/index.ts src/client.ts package.json package-lock.json tests/client.test.ts && git commit -m "$(cat <<'EOF'
feat: drop @anthropic-ai/sdk; wire self-hosted provider into client/index

- index.ts re-exports the public surface (types, message, stream, error,
  client, providers/*); internal http/* and serialize/* stay private
- client.ts routes provider:'anthropic' through the self-hosted
  AnthropicProvider (openai/minimax unchanged, pending M2/M4)
- package.json removes @anthropic-ai/sdk from peer/peerMeta/dev deps;
  openai retained

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

Expected output:

```
[<branch> <hash>] feat: drop @anthropic-ai/sdk; wire self-hosted provider into client/index
 5 files changed, ...
```

---

## Cross-task implementation notes (read before executing)

- **`BoxStream` vs `AsyncGenerator<StreamEvent>` typing seam:** the new `AnthropicProvider.stream()` returns `BoxStream` (`AsyncIterable<StreamEvent>`), while the existing `ProviderLike.stream` in `client.ts` is typed `AsyncGenerator<StreamEvent>`. An async generator satisfies `AsyncIterable`, so this is usually fine; if `tsc` complains in Task 9, widen `ProviderLike.stream` to `AsyncIterable<StreamEvent>`.
- **The terminal `done` event has `eventType: 'text'`:** `doneEvent`/`doneWithStopReason` from `stream.ts` set `eventType` to `'text'` (there is no dedicated done type in `StreamEventType`). Identify the terminal event by `e.done === true`, NOT by `eventType`. Task 8's transcript test asserts on `e.done`.
- **`parseSse` yields `SseEvent { event?, data }`:** the Anthropic stream adapter reads `evt.event` (e.g. `'content_block_delta'`) and `evt.data` (the parsed JSON) — not a `{ eventType, payload }` shape.
- **Existing tests stay green:** `tests/integration.anthropic.test.ts` (env-gated) is untouched; Task 9 appends to `tests/client.test.ts` rather than rewriting it.

---

## Milestone Done Criteria (verify all before tagging v0.4.0)

- [ ] `grep -rn '@anthropic-ai/sdk' sdks/typescript/src` returns nothing; the dependency is removed from `package.json` (peerDependencies, peerDependenciesMeta, devDependencies).
- [ ] From `sdks/typescript/`: `npm run build` passes (tsc strict, NodeNext) and `npm run test` is fully green.
- [ ] Mocked-fetch unit tests prove the Anthropic provider `chat()` serializes content blocks, `tool_use`/`tool_result`, and system correctly, and parses usage (incl. cache tokens), thinking, tool_calls, and stopReason from a non-streaming response.
- [ ] A hand-authored SSE-transcript streaming test asserts the full `StreamEvent` sequence (Text, ToolCall Start/Args/End, ThinkingDelta/Done, Usage, done + stopReason).
- [ ] `collectStream()` reassembles a synthetic stream into a `ChatResponse` with correct content, thinking (three-way logic), JSON-parsed tool-call `input`, summed usage, and stopReason (explicit > heuristic).
- [ ] Env-gated live Anthropic test (`it.skipIf(!process.env.ANTHROPIC_API_KEY)`) passes for chat + stream + tool use.
- [ ] `Client({ provider: 'anthropic', ... })` returns the self-hosted provider; OpenAI/MiniMax still construct and pass their existing tests.

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet accompanies this plan):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks (superpowers:subagent-driven-development).
2. **Inline:** execute tasks in-session with checkpoints (superpowers:executing-plans).

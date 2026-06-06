# Milestone 2 — OpenAI + Serialization Split Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Self-implement the OpenAI `/v1/chat/completions` wire (chat + SSE), introduce a provider-specific serialization layer (`serialize/openai.ts`) alongside the M1 Anthropic serializer, wire `tool_choice` into both, drop the `openai` npm package, and keep MiniMax working via an interim rewire — shipping v0.5.0 with **zero official-SDK peer dependencies**.

**Architecture:** Builds directly on M1's self-implemented foundation (raw `fetch` + SSE, structured types, `stream.ts`, `error.ts`). M2 adds the OpenAI half of the per-provider serialization split — the boundary where the Anthropic-vs-OpenAI tool-call/system/schema divergence (CLAUDE.md's #1 bug source) is contained and tested. OpenAI and MiniMax stop depending on the `openai` npm package.

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, raw `fetch` (no official LLM SDKs). Reference: Rust SDK `sdks/rust/src/providers/openai.rs`.

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M2).

**Depends on:** M1 (PR #185). Branch M2 off `feat/typescript-m1-foundation` if M1 is unmerged, or off `main` once M1 lands.

---

## Conventions (apply to EVERY task — override anything ambiguous in a task body)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. SDK package: `sdks/typescript/`. **All commands run from `sdks/typescript/`**. Paths below are repo-relative.
- **Workflow:** feature branch, land via **PR + CI**. Commit after each task. (Pre-push hook can't run from a git worktree — the Rust workspace's relative path dep resolves outside it; if pushing from a worktree, verify `npm run build` + `npm run test` + the Python suite manually and use `git push --no-verify`; CI runs the full gate.)
- **Module system:** `strict` + `NodeNext`. Every relative import MUST end in `.js`.
- **Layout:** source in `src/`, tests in `tests/`. Tests are NOT type-checked by `tsc` (tsconfig `include` is `src/**` only) — they are run by vitest.
- **Per-task quality gate:** `npm run build` (tsc strict) passes AND the task's `npx vitest run tests/<file>` is green. No TS formatter yet (prettier is M7).
- **Live tests** are env-gated and skip cleanly: `it.skipIf(!process.env.OPENAI_API_KEY)(...)`.
- **Tool-call argument field is `input`** (parsed object) in TS; the OpenAI wire uses `function.arguments` as a JSON **string**.

## Built on M1 (import, never redefine)

`types.ts` already provides `ChatRequest` (with `toolChoice`/`systemBlocks`/`systemCache`/`stopSequences`), `ToolChoice`, `ContentBlock`/`ImageSource`/`DocumentSource`, `Tool` (with `cache?`), `Message`, `ToolCall`, `Usage`, `StopReason`, `StreamEvent`, `ChatResponse`. `error.ts` provides `mapHttpError`/`extractErrorMessage`/`ProviderError`/etc. `http/fetch.ts` provides `postJson(url, headers, body)` / `postStream(url, headers, body)`. `http/sse.ts` provides `parseSse` (yields `SseEvent {event?, data}`). `stream.ts` provides `BoxStream` + the event constructors (`textEvent`, `usageEvent`, `toolCallStart`, `toolCallArgs`, `toolCallArgsWithId`, `toolCallEnd`, `toolCallEndWithId`, `doneEvent`, `doneWithStopReason`) + `collectStream`. `serialize/anthropic.ts` provides `serializeAnthropicRequest` (already does content blocks, per-tool `cache_control`, system-as-array, `stop_sequences`; **does NOT yet do `tool_choice`** — Task 1 adds it).

## Canonical symbol homes (single source of truth — never re-declare outside its home)

| Symbol(s) | Home | Created/extended by |
|---|---|---|
| `tool_choice` serialization (Anthropic) | `src/serialize/anthropic.ts` | **Task 1** (extends existing `serializeAnthropicRequest`) |
| `serializeOpenAiRequest` | `src/serialize/openai.ts` | **Task 2** (canonical OpenAI serializer; imported by providers/openai.ts AND providers/minimax.ts) |
| `OpenAIProvider` (self-implemented) | `src/providers/openai.ts` | **Task 3** |
| `MinimaxProvider` (rewired onto `serialize/openai.ts`) | `src/providers/minimax.ts` | **Task 4** |
| collectStream OpenAI-style coverage | `tests/stream.test.ts` (+ `src/stream.ts` only if a fix is needed) | **Task 5** |
| drop `openai` dep / routing tests | `package.json`, `tests/client.test.ts` | **Task 6** |

**`tool_choice` mapping (the divergence to NOT mix up):** `{type:'auto'}` → Anthropic `{type:'auto'}` / OpenAI `'auto'`; `{type:'required'}` → Anthropic `{type:'any'}` / OpenAI `'required'`; `{type:'none'}` → Anthropic **removes the `tools` array** / OpenAI `'none'`; `{type:'tool',name}` → Anthropic `{type:'tool',name}` / OpenAI `{type:'function',function:{name}}`.

**Dependency order (top to bottom):** 1 anthropic tool_choice → 2 serialize/openai → 3 openai provider → 4 minimax rewire → 5 collectStream test → 6 drop openai + wire-up. Tasks 3 and 4 both depend on Task 2.

## Deferred to M3 (do NOT build in M2 — ruling from the Rust review)

- **OpenAI Responses-API 404 fallback** — a whole second serializer + response parser, opt-in, off by default, reachable only via a `ClientBuilder` toggle that does not exist until M3.
- **Multiple auth styles** (`x-api-key`/custom header) — OpenAI and MiniMax both use `Authorization: Bearer`; the alternates are only selectable via the M3 builder.
- M2 ships: single chat-completions endpoint, **Bearer auth**, with an **optional `baseUrl` constructor override** (cheap seam, used by MiniMax-style compat endpoints and M5 Ollama).

---

### Task 1: extend serialize/anthropic.ts with tool_choice

**Goal.** Add `tool_choice` serialization to the EXISTING `serializeAnthropicRequest` in `src/serialize/anthropic.ts`. Mirror `sdks/rust/src/providers/anthropic.rs:395-411` EXACTLY: `auto→{type:'auto'}`, `required→{type:'any'}` (note: `any`, not `required`), `none→` remove the `tools` key entirely (Anthropic has no native none), `tool→{type:'tool', name}`. M1 already serialized everything else; this is the only gap.

**Why first.** It is the smallest, lowest-risk change and proves the tool_choice half of the cross-provider divergence on the side that already has full test coverage. Task 2 then mirrors the OpenAI half.

TDD, bite-sized. All commands run from `sdks/typescript/`.

#### Step 1.1 — RED: add tool_choice tests to the existing suite

Append a new `describe('tool_choice', ...)` block to `tests/serialize.anthropic.test.ts` (the file already imports `serializeAnthropicRequest`). Critical: the `none` case must verify `tools` is REMOVED even though tools were supplied.

```ts
describe('tool_choice', () => {
  const toolsReq = {
    messages: [{ role: 'user' as const, content: 'hi' }],
    tools: [{ name: 'get_weather', description: 'w', inputSchema: { type: 'object' } }],
  }

  it("serializes auto as {type:'auto'}", () => {
    const result = serializeAnthropicRequest(
      { ...toolsReq, toolChoice: { type: 'auto' } },
      'claude-opus-4',
    )
    expect(result.tool_choice).toEqual({ type: 'auto' })
    expect(result.tools).toBeDefined()
  })

  it("serializes required as {type:'any'} (NOT 'required')", () => {
    const result = serializeAnthropicRequest(
      { ...toolsReq, toolChoice: { type: 'required' } },
      'claude-opus-4',
    )
    expect(result.tool_choice).toEqual({ type: 'any' })
  })

  it('serializes none by REMOVING the tools array and emitting no tool_choice', () => {
    const result = serializeAnthropicRequest(
      { ...toolsReq, toolChoice: { type: 'none' } },
      'claude-opus-4',
    )
    expect('tools' in result).toBe(false)
    expect('tool_choice' in result).toBe(false)
  })

  it("serializes a named tool as {type:'tool', name}", () => {
    const result = serializeAnthropicRequest(
      { ...toolsReq, toolChoice: { type: 'tool', name: 'get_weather' } },
      'claude-opus-4',
    )
    expect(result.tool_choice).toEqual({ type: 'tool', name: 'get_weather' })
  })

  it('omits tool_choice when not provided', () => {
    const result = serializeAnthropicRequest(toolsReq, 'claude-opus-4')
    expect('tool_choice' in result).toBe(false)
    expect(result.tools).toBeDefined()
  })
})
```

Run `npm run test -- serialize.anthropic` — the four asserting cases fail (the omit case already passes).

#### Step 1.2 — GREEN: implement tool_choice in serialize/anthropic.ts

In `src/serialize/anthropic.ts`, insert the `tool_choice` block AFTER the existing `if (req.tools && req.tools.length > 0) { ... }` block (so `result.tools` exists when `none` needs to delete it) and BEFORE the `if (req.thinking)` block:

```ts
  if (req.toolChoice) {
    switch (req.toolChoice.type) {
      case 'auto':
        result.tool_choice = { type: 'auto' }
        break
      case 'required':
        result.tool_choice = { type: 'any' }
        break
      case 'none':
        // Anthropic has no native "none"; removing tools prevents calls.
        delete result.tools
        break
      case 'tool':
        result.tool_choice = { type: 'tool', name: req.toolChoice.name }
        break
    }
  }
```

`result` is typed `Record<string, any>` in M1, so `delete result.tools` type-checks. Do not change the function signature or return type.

#### Step 1.3 — VERIFY

- `npm run test -- serialize.anthropic` → all green (existing + 5 new).
- `npm run build` → `tsc` clean.
- `npm run test` → full suite still green.

#### Step 1.4 — COMMIT

On the M2 feature branch (`feat/typescript-m2-openai`, branched off M1's `feat/typescript-m1-foundation` or off `main` once M1 merges):

```
feat(typescript): serialize anthropic tool_choice (auto/any/none/tool)

Mirror Rust providers/anthropic.rs: required->{type:any}, none removes
the tools array, tool->{type:tool,name}.
```

**Done criteria.** All `serialize.anthropic.test.ts` tests pass including the 5 new tool_choice cases; `none` provably removes `tools`; `required` provably maps to `any` (not `required`); `npm run build` and `npm run test` green.

---

### Task 2: serialize/openai.ts request serializer

**Goal.** Create `src/serialize/openai.ts` exporting `serializeOpenAiRequest(req: ChatRequest, model: string): Record<string, unknown>` — the CANONICAL OpenAI Chat Completions serializer. Mirror the structure/idioms of M1's `serialize/anthropic.ts` but project onto the OpenAI wire, proving the Anthropic-vs-OpenAI divergence (CLAUDE.md #1 bug source): system-as-message, flat function tools, stringified `tool_calls.arguments`, `image_url` content, `tool` role, OpenAI tool_choice strings, `stop`. The complete file content is given inline in Step 2.2 below. This task delivers ONLY the serializer; the raw-fetch provider rewrite + MiniMax rewire are separate M2 tasks that import this function.

TDD, bite-sized. All commands run from `sdks/typescript/`.

#### Step 2.1 — RED: create the divergence test file

Create `tests/serialize.openai.test.ts`. Each block deliberately contrasts the OpenAI shape against the Anthropic shape proven in `serialize.anthropic.test.ts`.

```ts
import { describe, it, expect } from 'vitest'
import { serializeOpenAiRequest } from '../src/serialize/openai.js'
import type { ContentBlock, SystemBlock } from '../src/types.js'

describe('serializeOpenAiRequest', () => {
  describe('basic structure', () => {
    it('emits model + messages; max_tokens only when set', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }] },
        'gpt-4o',
      )
      expect(r.model).toBe('gpt-4o')
      expect(Array.isArray(r.messages)).toBe(true)
      expect('max_tokens' in r).toBe(false) // no SDK-side default, unlike Anthropic
    })

    it('emits max_tokens, temperature, stop when present', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          maxTokens: 256,
          temperature: 0.7,
          stopSequences: ['STOP'],
        },
        'gpt-4o',
      )
      expect(r.max_tokens).toBe(256)
      expect(r.temperature).toBe(0.7)
      expect(r.stop).toEqual(['STOP']) // 'stop', not 'stop_sequences'
    })
  })

  describe('system prompt as a message (NOT top-level)', () => {
    it('prepends system string as a role:system message', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], system: 'be terse' },
        'gpt-4o',
      )
      expect('system' in r).toBe(false) // divergence from Anthropic
      const msgs = r.messages as any[]
      expect(msgs[0]).toEqual({ role: 'system', content: 'be terse' })
      expect(msgs[1]).toEqual({ role: 'user', content: 'hi' })
    })

    it('joins systemBlocks with newlines into one system message (no per-block cache)', () => {
      const systemBlocks: SystemBlock[] = [
        { text: 'line one' },
        { text: 'line two', cacheControl: true },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], systemBlocks },
        'gpt-4o',
      )
      const msgs = r.messages as any[]
      expect(msgs[0]).toEqual({ role: 'system', content: 'line one\nline two' })
      expect(JSON.stringify(msgs[0])).not.toContain('cache_control')
    })

    it('prioritizes systemBlocks over system string', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          system: 'ignored',
          systemBlocks: [{ text: 'used' }],
        },
        'gpt-4o',
      )
      expect((r.messages as any[])[0].content).toBe('used')
    })
  })

  describe('tools (flat function schema)', () => {
    it('wraps tools as {type:function, function:{name,description,parameters}}', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          tools: [
            { name: 'get_weather', description: 'w', inputSchema: { type: 'object', properties: { city: { type: 'string' } } } },
          ],
        },
        'gpt-4o',
      )
      expect(r.tools).toEqual([
        {
          type: 'function',
          function: {
            name: 'get_weather',
            description: 'w',
            parameters: { type: 'object', properties: { city: { type: 'string' } } },
          },
        },
      ])
      // Divergence: NOT Anthropic's {name, description, input_schema}.
      expect(JSON.stringify(r.tools)).not.toContain('input_schema')
    })

    it('defaults missing description/parameters', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'hi' }], tools: [{ name: 'noop' }] },
        'gpt-4o',
      )
      const fn = (r.tools as any[])[0].function
      expect(fn.description).toBe('')
      expect(fn.parameters).toEqual({ type: 'object', properties: {} })
    })

    it('omits tools when none provided', () => {
      const r = serializeOpenAiRequest({ messages: [{ role: 'user', content: 'hi' }] }, 'gpt-4o')
      expect('tools' in r).toBe(false)
    })
  })

  describe('tool_choice (OpenAI string/function forms)', () => {
    const base = {
      messages: [{ role: 'user' as const, content: 'hi' }],
      tools: [{ name: 'get_weather', description: 'w', inputSchema: { type: 'object' } }],
    }
    it('auto -> "auto" (string, not object)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'auto' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('auto')
    })
    it('required -> "required" (NOT Anthropic any)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'required' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('required')
    })
    it('none -> "none" string; tools UNTOUCHED (unlike Anthropic which removes them)', () => {
      const r = serializeOpenAiRequest({ ...base, toolChoice: { type: 'none' } }, 'gpt-4o')
      expect(r.tool_choice).toBe('none')
      expect(r.tools).toBeDefined()
    })
    it('tool -> {type:function, function:{name}}', () => {
      const r = serializeOpenAiRequest(
        { ...base, toolChoice: { type: 'tool', name: 'get_weather' } },
        'gpt-4o',
      )
      expect(r.tool_choice).toEqual({ type: 'function', function: { name: 'get_weather' } })
    })
  })

  describe('assistant tool_calls (stringified arguments)', () => {
    it('serializes tool_calls with function.arguments as a JSON STRING', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [
            {
              role: 'assistant',
              content: 'checking',
              toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Taipei' } }],
            },
          ],
        },
        'gpt-4o',
      )
      const a = (r.messages as any[])[0]
      expect(a.tool_calls[0]).toEqual({
        id: 'call_1',
        type: 'function',
        function: { name: 'get_weather', arguments: '{"city":"Taipei"}' },
      })
      expect(typeof a.tool_calls[0].function.arguments).toBe('string') // NOT an object
    })
  })

  describe('tool role -> role:tool message', () => {
    it('maps tool message to {role:tool, tool_call_id, content}', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'tool', content: '25C', toolCallId: 'call_1' }] },
        'gpt-4o',
      )
      expect((r.messages as any[])[0]).toEqual({
        role: 'tool',
        tool_call_id: 'call_1',
        content: '25C',
      })
    })

    it('drops a tool message lacking tool_call_id', () => {
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'tool', content: 'orphan' }] },
        'gpt-4o',
      )
      expect((r.messages as any[]).length).toBe(0)
    })
  })

  describe('user content blocks -> image_url', () => {
    it('serializes base64 image as data URL image_url', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'text', text: 'look' },
        { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc' } },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: 'look', contentBlocks }] },
        'gpt-4o',
      )
      const content = (r.messages as any[])[0].content
      expect(content[0]).toEqual({ type: 'text', text: 'look' })
      expect(content[1]).toEqual({
        type: 'image_url',
        image_url: { url: 'data:image/png;base64,abc' },
      })
    })

    it('serializes url image as image_url passthrough', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'image', source: { type: 'url', url: 'https://x/y.png' } },
      ]
      const r = serializeOpenAiRequest(
        { messages: [{ role: 'user', content: '', contentBlocks }] },
        'gpt-4o',
      )
      expect((r.messages as any[])[0].content[0]).toEqual({
        type: 'image_url',
        image_url: { url: 'https://x/y.png' },
      })
    })

    it('throws on a document content block (OpenAI-unsupported)', () => {
      const contentBlocks: ContentBlock[] = [
        { type: 'document', source: { type: 'base64', mediaType: 'application/pdf', data: 'd' } },
      ]
      expect(() =>
        serializeOpenAiRequest(
          { messages: [{ role: 'user', content: '', contentBlocks }] },
          'gpt-4o',
        ),
      ).toThrow()
    })
  })

  describe('providerOptions', () => {
    it('merges providerOptions into the request root', () => {
      const r = serializeOpenAiRequest(
        {
          messages: [{ role: 'user', content: 'hi' }],
          providerOptions: { stream_options: { include_usage: true } },
        },
        'gpt-4o',
      )
      expect(r.stream_options).toEqual({ include_usage: true })
    })
  })
})
```

Run `npm run test -- serialize.openai` — every test fails (module does not exist yet).

#### Step 2.2 — GREEN: create src/serialize/openai.ts

Create `src/serialize/openai.ts` with exactly this content:

```typescript
import type { ChatRequest, ContentBlock, Message } from '../types.js'

/**
 * OpenAI Chat Completions request serializer.
 *
 * Mirrors the structure/idioms of serialize/anthropic.ts but projects the
 * provider-agnostic ChatRequest onto the OpenAI `/v1/chat/completions` wire,
 * which diverges from Anthropic in four load-bearing ways (CLAUDE.md #1 bug
 * source):
 *
 *   1. System prompt is a `role: "system"` MESSAGE in the messages array,
 *      NOT a top-level `system` field. (system_blocks are JOINED into one
 *      system message — OpenAI has no per-block cache_control; cache flags
 *      are silently ignored here.)
 *   2. Tools are FLAT: `{type:"function", function:{name,description,parameters}}`,
 *      NOT Anthropic's nested `{name,description,input_schema}`.
 *   3. Assistant tool calls are `message.tool_calls[]` with
 *      `function.arguments` as a JSON STRING (not a parsed object), NOT
 *      Anthropic `tool_use` content blocks.
 *   4. tool_choice maps: auto→"auto", required→"required", none→"none"
 *      (string forms — OpenAI HAS a real "none"), tool→
 *      {type:"function", function:{name}}.
 *
 * Stop sequences serialize to `stop`; `max_tokens` is only emitted when the
 * caller set it (OpenAI has no SDK-side default). providerOptions are merged
 * into the request root last, mirroring the Anthropic serializer.
 *
 * This is the CANONICAL OpenAI serializer export. providers/openai.ts and
 * providers/minimax.ts both import serializeOpenAiRequest from HERE.
 */

type SerializedMessage = Record<string, unknown>

function serializeUserContentBlock(block: ContentBlock): Record<string, unknown> {
  if (block.type === 'text') {
    return { type: 'text', text: block.text }
  }

  if (block.type === 'image') {
    const source = block.source
    if (source.type === 'base64') {
      return {
        type: 'image_url',
        image_url: { url: `data:${source.mediaType};base64,${source.data}` },
      }
    }

    return {
      type: 'image_url',
      image_url: { url: source.url },
    }
  }

  // Document blocks are not supported by OpenAI chat completions; capability
  // validation rejects them before serialization (matches Rust's
  // `ContentBlock::Document { .. } => unreachable!()`). Defensive throw keeps
  // this total without an `any` cast.
  throw new Error('OpenAI does not support document content blocks')
}

function serializeMessage(message: Message): SerializedMessage | null {
  if (message.role === 'tool') {
    if (!message.toolCallId) {
      return null
    }
    return {
      role: 'tool',
      tool_call_id: message.toolCallId,
      content: message.content,
    }
  }

  if (message.role === 'system') {
    return { role: 'system', content: message.content }
  }

  if (message.role === 'assistant') {
    if (message.toolCalls && message.toolCalls.length > 0) {
      return {
        role: 'assistant',
        content: message.content,
        tool_calls: message.toolCalls.map((toolCall) => ({
          id: toolCall.id,
          type: 'function',
          function: {
            name: toolCall.name,
            // OpenAI requires arguments as a JSON STRING, not an object.
            arguments: JSON.stringify(toolCall.input),
          },
        })),
      }
    }
    return { role: 'assistant', content: message.content }
  }

  // role === 'user'
  if (message.contentBlocks && message.contentBlocks.length > 0) {
    return {
      role: 'user',
      content: message.contentBlocks.map(serializeUserContentBlock),
    }
  }

  return { role: 'user', content: message.content }
}

export function serializeOpenAiRequest(
  req: ChatRequest,
  model: string,
): Record<string, unknown> {
  const messages: SerializedMessage[] = []

  // System prompt becomes the FIRST message (role: system). Priority:
  // systemBlocks > system string (matches Rust OpenAIRequestBuilder). OpenAI
  // has no per-block cache_control, so blocks are joined with a single newline
  // (the chat-completions wire; the Responses API — deferred to M3 — uses \n\n).
  if (req.systemBlocks && req.systemBlocks.length > 0) {
    const joined = req.systemBlocks.map((block) => block.text).join('\n')
    if (joined.length > 0) {
      messages.push({ role: 'system', content: joined })
    }
  } else if (req.system) {
    messages.push({ role: 'system', content: req.system })
  }

  for (const message of req.messages) {
    const serialized = serializeMessage(message)
    if (serialized !== null) {
      messages.push(serialized)
    }
  }

  const result: Record<string, unknown> = {
    model,
    messages,
  }

  if (req.temperature !== undefined) {
    result.temperature = req.temperature
  }

  if (req.maxTokens !== undefined) {
    result.max_tokens = req.maxTokens
  }

  if (req.tools && req.tools.length > 0) {
    result.tools = req.tools.map((tool) => ({
      type: 'function',
      function: {
        name: tool.name,
        description: tool.description ?? '',
        parameters: tool.inputSchema ?? { type: 'object', properties: {} },
      },
    }))
  }

  if (req.toolChoice) {
    switch (req.toolChoice.type) {
      case 'auto':
        result.tool_choice = 'auto'
        break
      case 'required':
        result.tool_choice = 'required'
        break
      case 'none':
        result.tool_choice = 'none'
        break
      case 'tool':
        result.tool_choice = {
          type: 'function',
          function: { name: req.toolChoice.name },
        }
        break
    }
  }

  if (req.stopSequences && req.stopSequences.length > 0) {
    result.stop = req.stopSequences
  }

  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(result, req.providerOptions)
  }

  return result
}
```

Key points this satisfies (matches Rust `openai.rs:297-469`):
- Strict TS, NodeNext: the only import is `import type { ChatRequest, ContentBlock, Message } from '../types.js'`.
- System message prepended (systemBlocks joined with a single `\n`, priority over the bare `system` string which is emitted as-is), matching `openai.rs:325-337`.
- `max_tokens` emitted only when `req.maxTokens !== undefined` (no default — divergence from Anthropic's 8192).
- Tools flat; `tool_calls.function.arguments = JSON.stringify(toolCall.input)`.
- `tool` role → `{role:'tool', tool_call_id, content}`, dropped when no `toolCallId`.
- User `contentBlocks` → `image_url` (base64 → `data:<mt>;base64,<data>`); document throws.
- tool_choice strings + `{type:'function', function:{name}}`; `none` does NOT touch tools.
- `providerOptions` merged last via `Object.assign`, mirroring `serialize/anthropic.ts`.

Run `npm run test -- serialize.openai` → all green.

#### Step 2.3 — VERIFY

- `npm run build` → `tsc` clean (note: tests are not type-checked by tsc; the serializer file must compile under `src/**`).
- `npm run test` → full suite green (Task 1 + Task 2 + all M1 tests).

#### Step 2.4 — COMMIT

```
feat(typescript): add serialize/openai.ts canonical request serializer

Project ChatRequest onto OpenAI chat-completions wire: system-as-message,
flat function tools, stringified tool_calls arguments, image_url content,
tool role, OpenAI tool_choice forms, stop. Mirrors Rust openai.rs;
proves the Anthropic-vs-OpenAI divergence with contrast tests.
```

**Done criteria.** `tests/serialize.openai.test.ts` passes; tests provably contrast OpenAI vs Anthropic (system-as-message, flat `function.parameters` with no `input_schema`, stringified `arguments`, `image_url`, OpenAI tool_choice strings, `stop` not `stop_sequences`); document block throws; `npm run build` and `npm run test` green. The provider rewrite and MiniMax rewire (which `import { serializeOpenAiRequest } from '../serialize/openai.js'`) are subsequent M2 tasks gated by the MiniMax verification test in the M2 Done-when.

---

### Task 3: providers/openai.ts rewrite (self-implemented chat + stream)

Remove the `openai` npm import dependency and rewrite `src/providers/openai.ts` using raw fetch, mirroring the Rust `openai.rs` stream adapter. This task _depends on_ Task 1 (serialize/openai.ts already exist via the locked contract).

**Files:**
- `src/providers/openai.ts` — rewritten provider (no SDK import)
- `src/serialize/openai.ts` — imported from Task 1 contract; NOT defined here
- `tests/providers-openai.test.ts` — new tests: mocked chat request/response + hand-authored SSE stream with tool calls

#### TDD Steps

- [ ] **Step 1: Create the OpenAI provider constructor**

  Create `src/providers/openai.ts` with the class skeleton:

  ```typescript
  export class OpenAIProvider {
    private readonly model: string
    private readonly baseUrl: string

    constructor(
      private readonly apiKey: string,
      model?: string,
      baseUrl = 'https://api.openai.com/v1'
    ) {
      this.model = model ?? 'gpt-4o'
      this.baseUrl = baseUrl.replace(/\/$/, '') // trim trailing slash
    }

    private headers(): Record<string, string> {
      return {
        'authorization': `Bearer ${this.apiKey}`,
        'content-type': 'application/json',
      }
    }
  }
  ```

  Run `npm run build` from `sdks/typescript/`. Verify no errors.

- [ ] **Step 2: Implement the chat() method**

  Inside `OpenAIProvider`, add the chat method. Import `serializeOpenAiRequest` from `../serialize/openai.js`, and error utilities:

  ```typescript
  import { mapHttpError, extractErrorMessage } from '../error.js'
  import { postJson } from '../http/fetch.js'
  import { serializeOpenAiRequest } from '../serialize/openai.js'
  import type { ChatRequest, ChatResponse, ToolCall, StopReason } from '../types.js'

  async chat(request: ChatRequest): Promise<ChatResponse> {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)

    let payload: any
    try {
      payload = await postJson<any>(
        `${this.baseUrl}/chat/completions`,
        this.headers(),
        body
      )
    } catch (error) {
      throw error // postJson already maps via mapHttpError
    }

    // Extract content from choices[0].message.content
    const message = payload?.choices?.[0]?.message ?? {}
    const content = String(message?.content ?? '')

    // Parse tool_calls: function.arguments is a JSON STRING (not object)
    const toolCalls: ToolCall[] = (message?.tool_calls ?? []).map((tc: any) => {
      const args = String(tc?.function?.arguments ?? '{}')
      let input: Record<string, unknown> = {}
      try {
        input = JSON.parse(args)
      } catch {
        input = {}
      }
      return {
        id: String(tc?.id ?? ''),
        name: String(tc?.function?.name ?? ''),
        input,
      }
    })

    // Map finish_reason to StopReason
    const finishReasonMap: Record<string, StopReason> = {
      'stop': 'stop',
      'length': 'max_tokens',
      'tool_calls': 'tool_use',
    }
    const stopReason = finishReasonMap[String(payload?.choices?.[0]?.finish_reason ?? '')] ?? 'other'

    return {
      content,
      toolCalls,
      model: String(payload?.model ?? resolvedModel),
      usage: {
        inputTokens: Number(payload?.usage?.prompt_tokens ?? 0),
        outputTokens: Number(payload?.usage?.completion_tokens ?? 0),
      },
      stopReason,
    }
  }
  ```

  Run `npm run build`. Verify no errors.

- [ ] **Step 3: Implement the stream() method with tool-call buffering**

  Inside `OpenAIProvider`, add the stream method. This implements the per-index tool-call buffering strategy from the spec:

  ```typescript
  import { postStream } from '../http/fetch.js'
  import { parseSse } from '../http/sse.js'
  import {
    doneEvent,
    doneWithStopReason,
    textEvent,
    toolCallArgsWithId,
    toolCallEndWithId,
    toolCallStart,
    usageEvent,
    type BoxStream,
  } from '../stream.js'
  import type { StopReason, StreamEvent, Usage } from '../types.js'

  stream(request: ChatRequest): BoxStream {
    return this.streamImpl(request)
  }

  private async *streamImpl(request: ChatRequest) {
    const resolvedModel = request.model ?? this.model
    const body = serializeOpenAiRequest(request, resolvedModel)
    body.stream = true

    const responseBody = await postStream(
      `${this.baseUrl}/chat/completions`,
      this.headers(),
      body
    )

    // Per-index tool-call tracking (only one tool open at a time for collectStream)
    const toolBuffer: Map<number, { id: string; name: string }> = new Map()
    let openToolIndex: number | undefined
    let pendingStopReason: StopReason | undefined
    let doneEmitted = false

    for await (const evt of parseSse(responseBody)) {
      // parseSse returns {event?: string, data: any}
      // OpenAI SSE has NO event: line, so evt.event is undefined; read evt.data
      const data = evt.data

      // [DONE] sentinel
      if (data === '[DONE]') {
        if (!doneEmitted) {
          doneEmitted = true
          yield pendingStopReason !== undefined
            ? doneWithStopReason(pendingStopReason)
            : doneEvent()
        }
        break
      }

      if (!data || typeof data !== 'object') continue

      const choice = data?.choices?.[0]
      if (!choice) continue

      const delta = choice?.delta
      if (!delta) continue

      // Text content (fall back to reasoning_content)
      const text =
        (typeof delta.content === 'string' ? delta.content.trim() : '') ||
        (typeof delta.reasoning_content === 'string' ? delta.reasoning_content.trim() : '')
      if (text) {
        yield textEvent(text)
      }

      // Tool calls (indexed per spec)
      if (Array.isArray(delta.tool_calls)) {
        for (const tc of delta.tool_calls) {
          const tcIndex = tc?.index
          if (tcIndex === undefined) continue

          const tcId = tc?.id
          const tcName = tc?.function?.name
          const tcArgs = tc?.function?.arguments

          // First delta for this index: has id + name
          if (tcId && tcName) {
            // Close any open tool from a different index
            if (openToolIndex !== undefined && openToolIndex !== tcIndex) {
              const openId = toolBuffer.get(openToolIndex)?.id
              if (openId) {
                yield toolCallEndWithId(openId)
              }
            }

            // Open this tool
            toolBuffer.set(tcIndex, { id: tcId, name: tcName })
            openToolIndex = tcIndex
            yield toolCallStart(tcId, tcName)
          }

          // Arguments fragment
          if (tcArgs) {
            const bufferedTool = toolBuffer.get(tcIndex)
            if (bufferedTool) {
              yield toolCallArgsWithId(bufferedTool.id, tcArgs)
            }
          }
        }
      }

      // Stash finish_reason for the terminal done event
      if (choice?.finish_reason) {
        const finishReasonMap: Record<string, StopReason> = {
          'stop': 'stop',
          'length': 'max_tokens',
          'tool_calls': 'tool_use',
        }
        pendingStopReason = finishReasonMap[String(choice.finish_reason)] ?? 'other'

        // If finish_reason is tool_calls, close the open tool now
        if (choice.finish_reason === 'tool_calls' && openToolIndex !== undefined) {
          const openId = toolBuffer.get(openToolIndex)?.id
          if (openId) {
            yield toolCallEndWithId(openId)
            openToolIndex = undefined
          }
        }
      }

      // Usage event (if present in final chunk with stream_options)
      const usage = data?.usage
      if (usage) {
        yield usageEvent({
          inputTokens: Number(usage?.prompt_tokens ?? 0),
          outputTokens: Number(usage?.completion_tokens ?? 0),
        })
      }
    }

    // Defensive: EOF without [DONE] — emit terminal once
    if (!doneEmitted) {
      doneEmitted = true
      yield pendingStopReason !== undefined
        ? doneWithStopReason(pendingStopReason)
        : doneEvent()
    }
  }
  ```

  Run `npm run build`. Verify no errors.

- [ ] **Step 4: Create tests — mocked chat request/response**

  Create `tests/providers-openai.test.ts` with mocked-fetch tests:

  ```typescript
  import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
  import { OpenAIProvider } from '../src/providers/openai.js'
  import type { ChatRequest } from '../src/types.js'

  describe('OpenAIProvider chat', () => {
    let capturedRequest: { url: string; headers: Record<string, string>; body: any } | null = null

    beforeEach(() => {
      capturedRequest = null
    })

    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('sends correct URL + Bearer auth header and parses a text response', async () => {
      const mockFetch = vi.fn(async (url: string, options?: RequestInit) => {
        capturedRequest = {
          url,
          headers: (options?.headers as Record<string, string>) ?? {},
          body: options?.body ? JSON.parse(String(options.body)) : null,
        }
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_1',
            object: 'chat.completion',
            created: 1234567890,
            model: 'gpt-4o',
            choices: [
              {
                index: 0,
                message: {
                  role: 'assistant',
                  content: 'Hello, world!',
                },
                finish_reason: 'stop',
              },
            ],
            usage: {
              prompt_tokens: 10,
              completion_tokens: 5,
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test', 'gpt-4o', 'https://api.openai.com/v1')
      const request: ChatRequest = {
        messages: [{ role: 'user', content: 'Hello' }],
      }

      const response = await provider.chat(request)

      expect(capturedRequest?.url).toBe('https://api.openai.com/v1/chat/completions')
      expect(capturedRequest?.headers['authorization']).toBe('Bearer sk-test')
      expect(capturedRequest?.headers['content-type']).toBe('application/json')
      expect(capturedRequest?.body.model).toBe('gpt-4o')
      expect(Array.isArray(capturedRequest?.body.messages)).toBe(true)
      expect(response.content).toBe('Hello, world!')
      expect(response.model).toBe('gpt-4o')
      expect(response.toolCalls).toEqual([])
      expect(response.usage.inputTokens).toBe(10)
      expect(response.usage.outputTokens).toBe(5)
      expect(response.stopReason).toBe('stop')
    })

    it('parses tool_calls with function.arguments as stringified JSON', async () => {
      const mockFetch = vi.fn(async () => {
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_2',
            object: 'chat.completion',
            created: 1234567890,
            model: 'gpt-4o',
            choices: [
              {
                index: 0,
                message: {
                  role: 'assistant',
                  content: 'Calling a tool',
                  tool_calls: [
                    {
                      id: 'call_abc123',
                      type: 'function',
                      function: {
                        name: 'calculate',
                        arguments: '{"x": 2, "y": 3}',
                      },
                    },
                  ],
                },
                finish_reason: 'tool_calls',
              },
            ],
            usage: {
              prompt_tokens: 15,
              completion_tokens: 8,
            },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test', 'gpt-4o')
      const response = await provider.chat({
        messages: [{ role: 'user', content: 'Call calculate' }],
      })

      expect(response.toolCalls).toHaveLength(1)
      expect(response.toolCalls[0]).toEqual({
        id: 'call_abc123',
        name: 'calculate',
        input: { x: 2, y: 3 },
      })
      expect(response.stopReason).toBe('tool_use')
    })

    it('maps finish_reason: length to max_tokens', async () => {
      const mockFetch = vi.fn(async () => {
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_3',
            model: 'gpt-4o',
            choices: [
              {
                message: { content: 'truncated' },
                finish_reason: 'length',
              },
            ],
            usage: { prompt_tokens: 5, completion_tokens: 5 },
          }),
          { status: 200 }
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test')
      const response = await provider.chat({
        messages: [{ role: 'user', content: 'test' }],
      })

      expect(response.stopReason).toBe('max_tokens')
    })

    it('uses baseUrl constructor parameter for custom endpoints', async () => {
      const mockFetch = vi.fn(async (url: string) => {
        capturedRequest = { url, headers: {}, body: null }
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_4',
            model: 'gpt-4o',
            choices: [{ message: { content: 'ok' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200 }
        )
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test', 'gpt-4o', 'https://api.custom.com/v1/')
      await provider.chat({ messages: [{ role: 'user', content: 'test' }] })

      expect(capturedRequest?.url).toBe('https://api.custom.com/v1/chat/completions')
    })
  })
  ```

  Run `npm run test -- tests/providers-openai.test.ts`. All tests should pass.

- [ ] **Step 5: Create tests — streaming with tool calls**

  Add to `tests/providers-openai.test.ts`:

  ```typescript
  describe('OpenAIProvider stream', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('emits text events and terminal done event', async () => {
      const mockFetch = vi.fn(async () => {
        const sseData = [
          'data: {"choices":[{"index":0,"delta":{"content":"Hello"},"finish_reason":null}]}\n\n',
          'data: {"choices":[{"index":0,"delta":{"content":" world"},"finish_reason":null}]}\n\n',
          'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}\n\n',
          'data: [DONE]\n\n',
        ].join('')

        return new Response(sseData, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test')
      const events: StreamEvent[] = []

      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Hi' }] })) {
        events.push(evt)
      }

      expect(events.length).toBeGreaterThanOrEqual(2)
      expect(events[0].eventType).toBe('text')
      expect(events[0].content).toBe('Hello')
      expect(events[1].eventType).toBe('text')
      expect(events[1].content).toBe(' world')
      expect(events[events.length - 1].done).toBe(true)
      expect(events[events.length - 1].stopReason).toBe('stop')
    })

    it('handles indexed tool_calls in deltas with sequential flush', async () => {
      const mockFetch = vi.fn(async () => {
        const sseData = [
          // First tool call (index 0) starts with id + name
          'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"get_weather"}}]},"finish_reason":null}]}\n\n',
          // Arguments fragment for index 0
          'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\\\"city"}}]},"finish_reason":null}]}\n\n',
          // More arguments for index 0
          'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\\\":\\\"NYC\\\"}"}}]},"finish_reason":null}]}\n\n',
          // Finish with tool_calls reason
          'data: {"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}\n\n',
          'data: [DONE]\n\n',
        ].join('')

        return new Response(sseData, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test')
      const events: StreamEvent[] = []

      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'Weather?' }] })) {
        events.push(evt)
      }

      // Should emit: tool_call_start, tool_call_args (x2), tool_call_end, done with stop_reason=tool_use
      const toolEvents = events.filter((e) => e.eventType.startsWith('tool_call'))
      expect(toolEvents.length).toBeGreaterThanOrEqual(3) // start, args (x2), end
      expect(toolEvents[0].eventType).toBe('tool_call_start')
      expect(toolEvents[0].toolCallId).toBe('call_1')
      expect(toolEvents[0].toolCallName).toBe('get_weather')

      const toolArgEvents = toolEvents.filter((e) => e.eventType === 'tool_call_args')
      expect(toolArgEvents.length).toBe(2)
      expect(toolArgEvents[0].toolCallArgsDelta).toBe('{"city')
      expect(toolArgEvents[1].toolCallArgsDelta).toBe('":"NYC"}')

      const endEvent = toolEvents[toolEvents.length - 1]
      expect(endEvent.eventType).toBe('tool_call_end')
      expect(endEvent.toolCallId).toBe('call_1')

      const doneEvent = events[events.length - 1]
      expect(doneEvent.done).toBe(true)
      expect(doneEvent.stopReason).toBe('tool_use')
    })

    it('closes open tool and emits done when [DONE] arrives', async () => {
      const mockFetch = vi.fn(async () => {
        const sseData = [
          'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_x","function":{"name":"func_x"}}]},"finish_reason":null}]}\n\n',
          'data: {"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{}"}}]},"finish_reason":null}]}\n\n',
          'data: [DONE]\n\n',
        ].join('')

        return new Response(sseData, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test')
      const events: StreamEvent[] = []

      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
        events.push(evt)
      }

      // Must have tool_call_start, args, and end before done
      const eventTypes = events.map((e) => e.eventType)
      const toolStartIdx = eventTypes.indexOf('tool_call_start')
      const toolEndIdx = eventTypes.indexOf('tool_call_end')
      const doneIdx = eventTypes.indexOf('text') // done event is text type

      expect(toolStartIdx).toBeGreaterThanOrEqual(0)
      expect(toolEndIdx).toBeGreaterThan(toolStartIdx)
      expect(events[events.length - 1].done).toBe(true)
    })

    it('emits usage event if present in final chunk', async () => {
      const mockFetch = vi.fn(async () => {
        const sseData = [
          'data: {"choices":[{"index":0,"delta":{"content":"test"},"finish_reason":null}]}\n\n',
          'data: {"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":5,"completion_tokens":3}}\n\n',
          'data: [DONE]\n\n',
        ].join('')

        return new Response(sseData, {
          status: 200,
          headers: { 'content-type': 'text/event-stream' },
        })
      })
      vi.stubGlobal('fetch', mockFetch)

      const provider = new OpenAIProvider('sk-test')
      const events: StreamEvent[] = []

      for await (const evt of provider.stream({ messages: [{ role: 'user', content: 'test' }] })) {
        events.push(evt)
      }

      const usageEvent = events.find((e) => e.eventType === 'usage')
      expect(usageEvent).toBeDefined()
      expect(usageEvent?.usage?.inputTokens).toBe(5)
      expect(usageEvent?.usage?.outputTokens).toBe(3)
    })
  })
  ```

  Run `npm run test -- tests/providers-openai.test.ts`. All tests should pass.

- [ ] **Step 6: Create live integration test (env-gated)**

  Add to `tests/providers-openai.test.ts`:

  ```typescript
  describe('OpenAIProvider (live integration)', () => {
    it.skipIf(!process.env.OPENAI_API_KEY)(
      'performs a real chat.completions request',
      async () => {
        const provider = new OpenAIProvider(process.env.OPENAI_API_KEY!, 'gpt-4o')
        const response = await provider.chat({
          messages: [{ role: 'user', content: 'Say "live test ok"' }],
          maxTokens: 10,
        })

        expect(response.content).toBeTruthy()
        expect(response.model).toBe('gpt-4o')
        expect(response.usage.inputTokens).toBeGreaterThan(0)
        expect(response.usage.outputTokens).toBeGreaterThan(0)
      }
    )

    it.skipIf(!process.env.OPENAI_API_KEY)(
      'streams a real response',
      async () => {
        const provider = new OpenAIProvider(process.env.OPENAI_API_KEY!, 'gpt-4o')
        let textAccum = ''
        let gotStream = false

        for await (const evt of provider.stream({
          messages: [{ role: 'user', content: 'Say "streaming ok" in one short word' }],
          maxTokens: 5,
        })) {
          if (evt.eventType === 'text' && evt.content) {
            textAccum += evt.content
            gotStream = true
          }
        }

        expect(gotStream).toBe(true)
        expect(textAccum.length).toBeGreaterThan(0)
      }
    )
  })
  ```

  Run `npm run test -- tests/providers-openai.test.ts` without `OPENAI_API_KEY` set (tests should skip gracefully). Run with the key set if available.

- [ ] **Step 7: Leave the old static `OpenAIProvider.serializeMessages` in place (for now)**

  The rewritten `OpenAIProvider` (Steps 1–3) uses `serializeOpenAiRequest` and no longer needs the old static `serializeMessages`. **Do NOT delete it yet** — `src/providers/minimax.ts` still calls `OpenAIProvider.serializeMessages` (M1 code), so deleting it now would break the minimax build and leave Task 3 red.

  Keep the static method as-is so the build stays green; **Task 4 rewires minimax onto `serializeOpenAiRequest` and then deletes the now-unused static method** (single owner of minimax.ts changes). Do not edit minimax.ts in this task.

  Run `npm run build`. Expected: no errors (minimax still compiles against the retained static method; the rewritten OpenAI provider compiles against `serializeOpenAiRequest`).

- [ ] **Step 8: Run full test suite and build**

  ```bash
  cd sdks/typescript
  npm run test
  npm run build
  ```

  Verify:
  - All tests in `tests/providers-openai.test.ts` pass (6+ test cases)
  - No TypeScript errors
  - No unused imports
  - Integration tests skip gracefully when `OPENAI_API_KEY` is not set

- [ ] **Step 9: Commit with conventional message**

  ```bash
  cd sdks/typescript
  git add src/providers/openai.ts src/providers/minimax.ts tests/providers-openai.test.ts
  git commit -m "feat(openai): rewrite provider with self-implemented fetch + stream

Remove openai npm dependency. Implement chat() and stream() using raw
postJson/postStream + SSE parsing, mirroring openai.rs behavior. Stream
adapter buffers tool_calls per index and flushes sequentially for
collectStream. Integrate canonical serializeOpenAiRequest serializer.
Update minimax to use shared serializer.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
  ```

---

### Task 4: Rewire providers/minimax.ts onto serialize/openai.ts (interim)

**Spec:** MiniMax already speaks OpenAI-compatible wire format. After Task 2 delivers `src/serialize/openai.ts` with the canonical `serializeOpenAiRequest(req: ChatRequest, model: string)`, rewire `src/providers/minimax.ts` to call this shared serializer instead of `OpenAIProvider.serializeMessages()`. The goal: confirm minimax.ts compiles and produces identical request bodies after openai.ts drops its static serializer (M4 work). Keep MiniMax's own `endpoint` and `Bearer` auth unchanged.

**Files:**
- `src/serialize/openai.ts` (consumed, no changes)
- `src/providers/minimax.ts` (modified: swap serializer source onto `serializeOpenAiRequest`)
- `src/providers/openai.ts` (modified: delete the now-unused static `serializeMessages` — Step 4)
- `tests/providers-minimax.test.ts` (new: mocked-fetch test asserting serialization matches expected shape)

---

- [ ] **Step 1: Import serializeOpenAiRequest in minimax.ts**

Replace the OpenAIProvider import with an import of `serializeOpenAiRequest` from the serialize module.

```typescript
// src/providers/minimax.ts (top of file, replace line 2)

// OLD:
// import { OpenAIProvider } from './openai.js'

// NEW:
import { serializeOpenAiRequest } from '../serialize/openai.js'
```

---

- [ ] **Step 2: Rewire chat() body construction**

In `src/providers/minimax.ts`, replace the `chat()` method's body construction (currently lines ~19–36) so it calls `serializeOpenAiRequest` instead of the removed `OpenAIProvider.serializeMessages()`.

`serializeOpenAiRequest(request, resolvedModel)` returns the **complete** request body — `model`, `messages` (incl. system), `tools`, `tool_choice`, `temperature`, `max_tokens`, `stop`, and merged `providerOptions`. So MiniMax no longer hand-builds any of those; it just adds its own transport. Replace the body-construction block with:

```typescript
async chat(request: ChatRequest): Promise<ChatResponse> {
  const resolvedModel = request.model ?? this.model
  const body = serializeOpenAiRequest(request, resolvedModel)

  let response: Response
  try {
    response = await this.fetcher(this.endpoint, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${this.apiKey}`,
        'content-type': 'application/json'
      },
      body: JSON.stringify(body)
    })
  } catch (error: any) {
    throw new NetworkError(String(error))
  }

  // ... existing response parsing (text(), JSON.parse, !response.ok →
  //     mapHttpError, choices[0] extraction, tool_calls, usage, stopReason)
  //     stays UNCHANGED.
}
```

> Do not re-add the old `max_tokens`/`temperature`/`tools`/`providerOptions` lines — `serializeOpenAiRequest` already emits them. Re-adding would double-wrap the tools array.

---

- [ ] **Step 3: Rewire stream() body construction**

In `src/providers/minimax.ts`, replace the `stream()` method's lines 100–105 to call `serializeOpenAiRequest`.

```typescript
// src/providers/minimax.ts lines 99–115 (stream method)

async *stream(request: ChatRequest): AsyncGenerator<StreamEvent> {
  const resolvedModel = request.model ?? this.model
  const body = serializeOpenAiRequest(request, resolvedModel)
  body.stream = true
  
  let response: Response
  try {
    response = await this.fetcher(this.endpoint, {
      method: 'POST',
      headers: {
        authorization: `Bearer ${this.apiKey}`,
        'content-type': 'application/json'
      },
      body: JSON.stringify(body)
    })
  } catch (error: any) {
    throw new NetworkError(String(error))
  }
  
  // ... rest of streaming logic unchanged
}
```

---

- [ ] **Step 4: Delete the now-unused static `OpenAIProvider.serializeMessages`, then verify the build**

Now that minimax no longer calls it (and the rewritten OpenAI provider uses `serializeOpenAiRequest`), the old static method left in place by Task 3 Step 7 has zero callers. Delete the `static serializeMessages(...)` method from `src/providers/openai.ts`.

Confirm nothing still references it:

```bash
cd sdks/typescript
grep -rn 'serializeMessages' src
```

**Expected:** no output (the method is gone and nothing imports it). Then run the compiler:

```bash
npm run build
```

**Expected output:** No errors; `dist/providers/minimax.js` and `dist/providers/minimax.d.ts` are generated. (This is the build-green checkpoint that closes out the Task 3 → Task 4 OpenAI/MiniMax rewire.)

---

- [ ] **Step 5: Create mocked-fetch test for minimax serialization**

Create a new test file `tests/providers-minimax.test.ts` with a mocked `fetch` that captures the request body and asserts it matches the expected OpenAI-wire shape.

```typescript
// tests/providers-minimax.test.ts

import { afterEach, describe, expect, it, vi } from 'vitest'
import { MinimaxProvider } from '../src/providers/minimax.js'
import type { ChatRequest } from '../src/types.js'

describe('minimax provider serialization', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('serializes messages via serializeOpenAiRequest and produces correct body', async () => {
    let capturedBody: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
        return new Response(
          JSON.stringify({
            id: 'cmpl_123',
            model: 'MiniMax-Text-01',
            choices: [
              {
                message: { role: 'assistant', content: 'ok' },
                finish_reason: 'stop'
              }
            ],
            usage: { prompt_tokens: 10, completion_tokens: 5 }
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
    )

    const provider = new MinimaxProvider('test-key')
    const request: ChatRequest = {
      messages: [
        { role: 'user', content: 'What is 2+2?' },
        {
          role: 'assistant',
          content: '2+2=4',
          toolCalls: [{ id: 'call_1', name: 'calculator', input: { expr: '2+2' } }]
        },
        { role: 'tool', toolCallId: 'call_1', content: '4' }
      ],
      system: 'You are a helpful assistant.',
      temperature: 0.7,
      maxTokens: 256,
      tools: [
        {
          name: 'calculator',
          description: 'Evaluate math',
          inputSchema: { type: 'object', properties: { expr: { type: 'string' } } }
        }
      ]
    }

    await provider.chat(request)

    // Verify the captured body has OpenAI wire format
    expect(capturedBody.model).toBe('MiniMax-Text-01')
    
    // System message should be FIRST message in the array (OpenAI format)
    expect(capturedBody.messages[0]).toEqual({
      role: 'system',
      content: 'You are a helpful assistant.'
    })
    
    // User message comes next
    expect(capturedBody.messages[1]).toEqual({
      role: 'user',
      content: 'What is 2+2?'
    })
    
    // Assistant message with tool_calls
    const assistantMsg = capturedBody.messages[2]
    expect(assistantMsg.role).toBe('assistant')
    expect(assistantMsg.content).toBe('2+2=4')
    expect(Array.isArray(assistantMsg.tool_calls)).toBe(true)
    expect(assistantMsg.tool_calls[0]).toEqual({
      id: 'call_1',
      type: 'function',
      function: {
        name: 'calculator',
        arguments: JSON.stringify({ expr: '2+2' })
      }
    })
    
    // Tool result message
    expect(capturedBody.messages[3]).toEqual({
      role: 'tool',
      tool_call_id: 'call_1',
      content: '4'
    })
    
    // Tools array (flat OpenAI format)
    expect(Array.isArray(capturedBody.tools)).toBe(true)
    expect(capturedBody.tools[0]).toEqual({
      type: 'function',
      function: {
        name: 'calculator',
        description: 'Evaluate math',
        parameters: { type: 'object', properties: { expr: { type: 'string' } } }
      }
    })
    
    // Temperature and maxTokens
    expect(capturedBody.temperature).toBe(0.7)
    expect(capturedBody.max_tokens).toBe(256)
  })

  it('respects custom endpoint and Bearer auth', async () => {
    let capturedUrl: string = ''
    let capturedHeaders: HeadersInit = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = options?.headers ?? {}
        return new Response(
          JSON.stringify({
            id: 'cmpl_456',
            model: 'test-model',
            choices: [{ message: { role: 'assistant', content: 'test' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 }
          }),
          { status: 200, headers: { 'content-type': 'application/json' } }
        )
      })
    )

    const customEndpoint = 'https://custom.minimax.example/v1/api'
    const provider = new MinimaxProvider('custom-key', 'custom-model', customEndpoint)
    
    await provider.chat({
      messages: [{ role: 'user', content: 'test' }]
    })

    expect(capturedUrl).toBe(customEndpoint)
    expect((capturedHeaders as Record<string, string>).authorization).toBe('Bearer custom-key')
  })

  it('stream respects stream:true flag in body', async () => {
    let capturedBody: any
    vi.stubGlobal(
      'fetch',
      vi.fn(async (_url: string, options?: RequestInit) => {
        capturedBody = JSON.parse(String(options?.body ?? '{}'))
        return new Response('', { status: 200, headers: { 'content-type': 'text/event-stream' } })
      })
    )

    const provider = new MinimaxProvider('test-key')
    const request: ChatRequest = {
      messages: [{ role: 'user', content: 'test' }]
    }

    // Consume the async generator without caring about the events
    try {
      for await (const _ of provider.stream(request)) {
        break
      }
    } catch {
      // Ignore stream parsing errors; we only care about the body
    }

    expect(capturedBody.stream).toBe(true)
  })
})
```

**Rationale:** The test uses `vi.stubGlobal('fetch', ...)` to mock the HTTP layer, captures the request body sent to the endpoint, and asserts:
1. Messages are serialized in OpenAI format (system as first message in array, not separate field).
2. Tool calls use `tool_calls[]` with `function.arguments` as JSON string (not parsed object).
3. Tools array is flat `{type:'function', function:{...}}` format (not Anthropic nested).
4. Endpoint and Bearer auth are unchanged (MiniMax-specific wiring).
5. `stream:true` is set in the body for streaming calls.

This confirms minimax.ts correctly uses `serializeOpenAiRequest` after openai.ts removes its static method.

---

- [ ] **Step 6: Run tests to confirm all pass**

```bash
cd sdks/typescript
npm test
```

**Expected output:**
```
✓ providers-minimax.test.ts (3 tests)
  ✓ serializes messages via serializeOpenAiRequest and produces correct body
  ✓ respects custom endpoint and Bearer auth
  ✓ stream respects stream:true flag in body

✓ providers.serialization.test.ts (existing tests)
✓ ... (all other tests continue to pass)

Test Files  10 passed (10)
     Tests  42 passed (42)
```

---

- [ ] **Step 7: Build and commit**

Build the project to ensure everything typechecks and outputs valid JavaScript:

```bash
cd sdks/typescript
npm run build
```

**Expected output:**
```
$ tsc -p tsconfig.json
# No errors; dist/ is updated with minimax.js and minimax.d.ts
```

Stage the changes and create a conventional commit:

```bash
cd sdks/typescript
git add src/providers/minimax.ts src/providers/openai.ts tests/providers-minimax.test.ts
git commit -m "$(cat <<'EOF'
refactor(minimax): use shared serializeOpenAiRequest from serialize/openai

Rewire MinimaxProvider to import and call serializeOpenAiRequest instead
of OpenAIProvider.serializeMessages, preparing for openai.ts to drop its
static serializer in M4. MiniMax continues to use its own endpoint and
Bearer auth; only message/tool serialization source moves to the shared
module. Adds comprehensive mocked-fetch test asserting body shape matches
OpenAI wire format (system as message, tool_calls array, flat tools).

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

**Expected output:**
```
[feat/typescript-m2 0a7b2c3] refactor(minimax): use shared serializeOpenAiRequest from serialize/openai
 2 files changed, 68 insertions(+), 15 deletions(-)
 create mode 100644 tests/providers-minimax.test.ts
```

Verify the commit was created:

```bash
cd sdks/typescript
git log --oneline -1
```

**Expected output:**
```
0a7b2c3 refactor(minimax): use shared serializeOpenAiRequest from serialize/openai
```

---

---

### Task 5: collectStream OpenAI-style accumulation test (+ multi-tool guard)

**Files:**
- `sdks/typescript/tests/stream.test.ts` (extend with new test case)
- `sdks/typescript/src/stream.ts` (READ ONLY — no changes needed; collectStream already handles two sequential tools correctly)

**Context:**

The OpenAI provider will emit tool calls as a sequence of `tool_call_start(id, name) → tool_call_args* → tool_call_end(id) → [next tool]` events. M1's `collectStream` tracks a **single current tool** via `currentToolId`, `currentToolName`, and `currentToolArgs`, which are **reset on each new `tool_call_start`** (line 146 of `stream.ts`). This reset pattern enables correct accumulation of multiple sequential tool calls: each `tool_call_start` wipes the previous tool's buffer, guaranteeing no cross-tool contamination.

This task verifies that the OpenAI adapter's synthetic stream (two back-to-back tool calls with JSON-parsed inputs and usage summation) correctly round-trips through `collectStream`, confirming the invariant that the event sequence for each tool is contiguous (`tool_call_start → tool_call_args* → tool_call_end`) and never interleaved.

**Steps:**

- [ ] **Step 1: Add the multi-tool accumulation test to stream.test.ts.**
  
  After the existing `'accumulates tool calls with correct input parsing'` test (which covers a single tool), add a new test case that feeds a stream mimicking the OpenAI provider's output: text + two sequential tool calls with parsed JSON inputs + usage + `doneWithStopReason('tool_use')`.
  
  Create file `sdks/typescript/tests/stream.test.ts` (extend):
  
  ```typescript
  it('accumulates multiple sequential tool calls with correct JSON parsing and usage', async () => {
    const events: StreamEvent[] = [
      textEvent('Processing '),
      toolCallStart('call_1', 'get_weather'),
      toolCallArgsWithId('call_1', '{"city":"'),
      toolCallArgsWithId('call_1', 'Tokyo",'),
      toolCallArgsWithId('call_1', '"units":"celsius"}'),
      toolCallEndWithId('call_1'),
      textEvent('and '),
      toolCallStart('call_2', 'translate_text'),
      toolCallArgsWithId('call_2', '{"text":"hello",'),
      toolCallArgsWithId('call_2', '"target_language":"fr"}'),
      toolCallEndWithId('call_2'),
      usageEvent({ inputTokens: 50, outputTokens: 25 }),
      doneWithStopReason('tool_use'),
    ]
    const stream = (async function* () {
      for (const ev of events) yield ev
    })() as BoxStream

    const response = await collectStream(stream)

    expect(response.toolCalls).toHaveLength(2)
    expect(response.toolCalls[0]).toEqual({
      id: 'call_1',
      name: 'get_weather',
      input: { city: 'Tokyo', units: 'celsius' },
    })
    expect(response.toolCalls[1]).toEqual({
      id: 'call_2',
      name: 'translate_text',
      input: { text: 'hello', target_language: 'fr' },
    })
    expect(response.content).toBe('Processing and ')
    expect(response.usage.inputTokens).toBe(50)
    expect(response.usage.outputTokens).toBe(25)
    expect(response.stopReason).toBe('tool_use')
  })
  ```
  
  Expected output on test run: PASS — confirms `collectStream` correctly resets `currentToolArgs` on each `tool_call_start`, isolating the two tool calls.

- [ ] **Step 2: Verify the test passes (confirms no stream.ts fix needed).**
  
  ```bash
  cd sdks/typescript
  npm test -- stream.test.ts
  ```
  
  Expected output:
  ```
  ✓ stream.ts > collectStream > accumulates multiple sequential tool calls with correct JSON parsing and usage
  
  Test Files  1 passed (1)
       Tests  1 passed (1)
  ```
  
  This confirms `collectStream`'s single-tool-at-a-time buffer strategy (with reset on `tool_call_start`) already handles two sequential tools correctly. **No fix to `src/stream.ts` is required.**

- [ ] **Step 3: Run full stream.test.ts suite to ensure no regressions.**
  
  ```bash
  cd sdks/typescript
  npm test -- stream.test.ts
  ```
  
  Expected output: all 8 tests pass (6 existing + 1 new multi-tool + 1 constructor helper).

- [ ] **Step 4: Build and verify TypeScript strict mode.**
  
  ```bash
  cd sdks/typescript
  npm run build
  ```
  
  Expected output: clean build, no type errors.

- [ ] **Step 5: Create a conventional commit.**
  
  ```bash
  cd sdks/typescript
  git add tests/stream.test.ts
  git commit -m "test(stream): verify collectStream handles two sequential tool calls

Add comprehensive test case mimicking OpenAI provider output: text + toolCallStart(A)/args/toolCallEnd(A) + toolCallStart(B)/args/toolCallEnd(B) + doneWithStopReason.

Confirms collectStream's single-tool-at-a-time buffer (reset on toolCallStart) correctly isolates and parses multiple sequential tool calls without cross-tool contamination.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
  ```
  
  Expected output: commit created, verified with `git log --oneline -1`.

---

**Verification:**

- `collectStream` correctly reassembles two tool calls with JSON-parsed `input` objects from the event stream.
- Usage tokens are summed correctly across events.
- `stopReason` is correctly set to `'tool_use'` when tool calls are present.
- No changes to `src/stream.ts` were needed — the existing reset-on-start pattern is sufficient.

**Notes:**

- The test uses `toolCallArgsWithId` (which includes the `toolCallId` field) to match the OpenAI provider's output strategy of tracking IDs across argument chunks.
- The test feeds events in the exact order an OpenAI SSE adapter would emit them (text mixed with tool calls, not batched).
- Both tool calls are completed before the `doneWithStopReason` event, matching the real streaming behavior.

---

### Task 6: Drop the `openai` npm package; finalize wire-up

**Goal:** With OpenAI and MiniMax now self-implemented (Tasks 3–4), remove the `openai` npm package so the SDK has **zero official-SDK peer dependencies**, and lock that in with a routing test. This task does NOT touch the serializer or providers — those are done in earlier tasks.

**Files:**
- Modify: `sdks/typescript/package.json` (remove `openai`)
- Modify: `sdks/typescript/tests/client.test.ts` (add no-npm-deps routing tests)

> Prerequisite: Tasks 2–4 are complete — `serialize/openai.ts` exists, `providers/openai.ts` and `providers/minimax.ts` no longer import the `openai` npm package. Do NOT create or modify the serializer/providers here.

- [ ] **Step 1: Verify no source still imports the `openai` npm package**

Run from `sdks/typescript/`:

```bash
grep -rn "from 'openai'" src ; grep -rn '@anthropic-ai/sdk' src
```

Expected: **no output** from either grep. If anything matches, the referencing file was missed in Tasks 3–4 — fix it there before continuing (this task only removes the dependency once nothing imports it).

- [ ] **Step 2: Remove `openai` from `package.json`**

Edit `sdks/typescript/package.json`: delete `"openai": ">=4.0.0"` from `peerDependencies`, delete the `peerDependenciesMeta.openai` entry, and delete `"openai": "^6.4.0"` from `devDependencies`. The dependency sections become:

```json
{
  "peerDependencies": {},
  "peerDependenciesMeta": {},
  "devDependencies": {
    "@types/node": "^22.13.10",
    "typescript": "^5.8.2",
    "vitest": "^3.0.8"
  }
}
```

- [ ] **Step 3: Reinstall to drop the package from `node_modules` / lockfile**

Run from `sdks/typescript/`:

```bash
npm install
```

Expected: completes without error; `package-lock.json` no longer references `openai`. Verify:

```bash
grep -c '"node_modules/openai"' package-lock.json
```

Expected: `0`.

- [ ] **Step 4: Write the failing routing tests (no npm deps)**

Append to `sdks/typescript/tests/client.test.ts`:

```typescript
describe('client openai/minimax routing (no npm deps)', () => {
  afterEach(() => {
    vi.unstubAllGlobals()
  })

  it('routes provider:"openai" to the self-hosted OpenAIProvider (no npm openai)', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            id: 'chatcmpl_1',
            object: 'chat.completion',
            created: 1234567890,
            model: 'gpt-4o',
            choices: [{ index: 0, message: { role: 'assistant', content: 'ok' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'openai', apiKey: 'sk-test' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(capturedUrl).toContain('api.openai.com/v1/chat/completions')
    expect(capturedHeaders['authorization']).toBe('Bearer sk-test')
    expect(response.content).toBe('ok')
  })

  it('routes provider:"minimax" to the self-hosted MinimaxProvider (no npm deps)', async () => {
    let capturedUrl = ''
    let capturedHeaders: Record<string, string> = {}
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string, options?: RequestInit) => {
        capturedUrl = url
        capturedHeaders = (options?.headers as Record<string, string>) ?? {}
        return new Response(
          JSON.stringify({
            model: 'MiniMax-Text-01',
            choices: [{ index: 0, message: { role: 'assistant', content: 'ok' }, finish_reason: 'stop' }],
            usage: { prompt_tokens: 1, completion_tokens: 1 },
          }),
          { status: 200, headers: { 'content-type': 'application/json' } },
        )
      }),
    )

    const client = new Client({ provider: 'minimax', apiKey: 'mk-test' })
    const response = await client.chat({ messages: [{ role: 'user', content: 'hi' }] })

    expect(capturedUrl).toContain('api.minimax.chat/v1/text/chatcompletion_v2')
    expect(capturedHeaders['authorization']).toBe('Bearer mk-test')
    expect(response.content).toBe('ok')
  })
})
```

> Ensure the existing `client.test.ts` imports include `describe, it, expect, vi, afterEach` from `'vitest'` and `Client` from `'../src/client.js'` — add any missing ones to the existing import lines (do not duplicate imports).

- [ ] **Step 5: Run the new tests — they must pass against the self-hosted providers**

Run from `sdks/typescript/`:

```bash
npx vitest run tests/client.test.ts
```

Expected: PASS, including the two new routing tests (the providers reach their endpoints via raw `fetch`, no npm package).

- [ ] **Step 6: Full build + suite gate**

Run from `sdks/typescript/`:

```bash
npm run build && npm run test
```

Expected: `tsc` succeeds (no type errors); the full vitest suite passes; env-gated live tests skip cleanly when keys are absent.

- [ ] **Step 7: Commit**

```bash
git add sdks/typescript/package.json sdks/typescript/package-lock.json sdks/typescript/tests/client.test.ts
git commit -m "$(cat <<'EOF'
chore(ts): drop openai npm package — zero official-SDK peer deps

OpenAI and MiniMax are now self-implemented (Tasks 3-4), so the openai
npm package is removed from peer/peerMeta/dev dependencies. Routing tests
assert provider:'openai' and provider:'minimax' reach their endpoints via
raw fetch with Bearer auth, no npm SDK.

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Milestone Done Criteria (verify all before tagging v0.5.0)

- [ ] `grep -rn "from 'openai'" sdks/typescript/src` and `grep -rn '@anthropic-ai/sdk' sdks/typescript/src` are both empty; `package.json` has **zero** `peerDependencies` (no official LLM SDKs).
- [ ] From `sdks/typescript/`: `npm run build` passes (tsc strict) and `npm run test` is fully green.
- [ ] Serializer tests prove the Anthropic-vs-OpenAI divergence: Anthropic `tools:[{name,description,input_schema}]` + `tool_use` blocks vs OpenAI `tools:[{type:function,function:{name,description,parameters}}]` + `message.tool_calls[].function.arguments` as a JSON string; system as Anthropic top-level field vs OpenAI `role:system` message.
- [ ] `tool_choice` tests pass for BOTH providers (auto / required→any / none→tools-removed (Anthropic) | none-string (OpenAI) / tool→named).
- [ ] OpenAI streaming test reconstructs indexed `tool_calls` (id from the first chunk, args accumulated by index) and parses `finish_reason`→`stopReason`.
- [ ] `collectStream` reassembles the OpenAI provider's emitted sequence, including a TWO-tool-call response, into the correct `ChatResponse`.
- [ ] MiniMax still serializes/works via the relocated `serialize/openai.ts` (mocked-fetch test); env-gated live OpenAI test passes when `OPENAI_API_KEY` is set.

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet can accompany this plan, as for M1):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks (superpowers:subagent-driven-development).
2. **Inline:** execute tasks in-session with checkpoints (superpowers:executing-plans).

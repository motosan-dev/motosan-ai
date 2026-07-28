# Freeform Tool Parity — TypeScript Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Port Rust's native model / Freeform-custom-tool API to the TypeScript SDK, and add the spec-anchored Freeform conformance suite to both TypeScript and Rust, so `specs/types.md` § Native Model API is implemented by more than one SDK.

**Architecture:** A new `src/serialize/responses.ts` module owns the entire OpenAI-Responses codec — encoding `ModelChatRequest` into a Responses body, decoding a non-streaming Responses payload into `ModelChatResponse`, and adapting the Responses SSE frame stream into `ModelStreamDelta` values. `providers/chatgpt_codex.ts` (native by default) and `providers/openai.ts` (native only behind the `withResponsesApi(true)` opt-in) both call that one codec, so the two providers cannot drift. `Client.modelChat` / `modelStream` / `modelStreamCollect` dispatch through `provider.ts`, which validates against a new `supportsFreeformTools` capability before any network I/O and wraps native streams in a read-idle timeout.

**Tech Stack:** TypeScript 5.8 (ESM, NodeNext, `strict: true`), vitest 3, native `fetch` / `ReadableStream` (no provider SDKs); Rust 1.x with `mockito` for the conformance mirror.

## Global Constraints

- Baseline for every branch is `origin/main`. Branch, never push code straight to `main`.
- **Prerequisite:** task group **S** of the *Python* plan — the `specs/types.md` § Native Model API widening — must merge before T1 starts. This plan implements the widened contract; it does not edit `specs/`.
- Each task group ships as its own PR: **T1**, **T2**, **C-TS**, **C-RS**. T2 depends on T1; C-TS depends on T2; C-RS depends on nothing but merges with C-TS as the D9 gate.
- Commit subjects use a bare conventional type — `fix:`, `feat:`, `refactor:` — with **no scope parentheses**, and end with `(#270)`. Documented in `AGENTS.md` § Commits. Example: `feat: TypeScript native model types and Responses encoders (#270)`.
- Every commit carries a second `-m` with exactly: `Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>`.
- TypeScript gates, run from `sdks/typescript/`: `npm ci`, then `npm run build`, then `npm run typecheck`, then `npm run test`. **Build before test** — `tests/pack-smoke.test.ts` asserts `dist/index.js` and `dist/index.d.ts` exist.
- Rust gates, run from `sdks/rust/`: `cargo fmt --all -- --check`; `cargo clippy --all-features --all-targets -- -D warnings`; and the credential-stripped full suite (the suite contains env-gated live tests):
  ```bash
  env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY \
      -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY \
      -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST \
      cargo test --all-features
  ```
- Repo-wide gates, run from the repo root: `treefmt --fail-on-change` and `python3 scripts/check-versions.py`. (`treefmt.toml` formats `*.rs`, `*.py`, `*.toml`, `*.nix` — it does **not** touch TypeScript.)
- In any fresh worktree, run `uv sync --all-extras` in `sdks/python/` before pushing — the pre-push hook needs it, even for a TypeScript-only change.
- Verify every push landed by SHA before claiming success:
  ```bash
  test "$(git ls-remote origin refs/heads/<branch> | cut -f1)" = "$(git rev-parse HEAD)"
  ```
- **No version bumps anywhere in this plan.** `scripts/bump-version.py` owns the release (Python 0.20.0 / TypeScript 0.16.0, D10); C-RS adds a Rust test file only, so Rust needs no version change.
- TypeScript house rules, from the `src/types.ts` header: discriminated unions keyed on a tag; optional fields **omitted, never `undefined`**; camelCase field names; wire serialization lives in `serialize/*.ts` and **never** in `types.ts`.
- **D2 tag choice:** `ModelToolSpec`, `ModelToolCall`, `ModelToolOutput`, and `ModelContextItem` are tagged with **`kind`**, following the `McpToolConfig` precedent (`types.ts:86-89`), because the model shape and the wire shape disagree (`freeform` ↔ wire `"custom"`, `id` ↔ wire `call_id`). `ModelStreamDelta` and `FunctionCallOutputContentItem` are tagged with **`type`** because their tag *values* are exactly the wire values (Rust `#[serde(tag = "type", rename_all = "snake_case")]`).
- **D3:** `ModelChatRequest` gets **no** `thinking`, `mcpServers`, or `mcpToolConfigs` fields. Provider-specific reasoning controls go through `providerOptions`.
- **D5:** `ProviderCapabilities` grows exactly one field, `supportsFreeformTools`. `fullCaps()` keeps it **false** — matching Rust's `ProviderCapabilities::full()`. TypeScript ends with four capability fields because it keeps its TS-only `supportsMcp`.

### Traps this plan must not step on

| Trap | Rule |
|---|---|
| Wire keys ≠ field names | `ModelToolCall.id` → wire `call_id`; `maxTokens` → body `max_output_tokens`; `Tool.inputSchema` → wire `parameters`. Decoding accepts `call_id` **or** `id`. |
| System hoisting | System messages inside `context` move into `instructions` **and are removed from `input`**. |
| `providerOptions` | Shallow-merged into the body root **LAST**, so it can override anything the encoder produced. |
| Codex reasoning | Effort resolves per-request `providerOptions.reasoning_effort` **first**, provider default second, omitted if neither. When one resolves, the body gets `reasoning = { effort, summary: 'auto' }` **and the raw top-level `reasoning_effort` key is deleted** — the `providerOptions` merge will have injected it. |
| Codex body overrides | Codex hard-sets `store:false`, `include:["reasoning.encrypted_content"]`, `parallel_tool_calls:true`, and **`tool_choice:"auto"` regardless of what the caller passed** — applied *after* the `providerOptions` merge. |
| `ProviderImpl.modelChat` / `modelStream` | **OPTIONAL** members. `ProviderImpl` is a structural contract third parties implement; making them required is a breaking change. |
| `asDispatchProvider` | `client.ts:70-80` rebuilds a plain object exposing only `capabilities`/`chat`/`stream`. Unless the shim forwards `modelChat`/`modelStream`, a `ProviderLike` without `capabilities` silently loses the native surface. |
| Provider string in `IncompleteStreamError` | Native path uses **`chatgpt-codex`** (hyphen) and **`openai`**, per D8 and `specs/types.md`. The legacy TS codex path uses `chatgpt_codex` (underscore) and is **not** changed. |
| `Usage` in `collectModelStream` | **Replaces**, never merges. This differs from `collectStream`'s replace-with-fallback rule. |
| `ToolCallDone` | Authoritative. Accumulated `function_arguments` / `freeform_input` deltas are bookkeeping and must never be lowered into the returned call. Freeform `input` is never parsed as JSON. |

---

## File Structure

| File | Status | Responsibility |
|---|---|---|
| `sdks/typescript/src/types.ts` | Modify | Adds the native model type family: `FreeformToolFormat`, `FreeformTool`, `ModelToolSpec`, `ImageDetail`, `FunctionCallOutputContentItem`, `FunctionCallOutputPayload`, `ModelToolCall`, `ModelToolOutput`, `ModelContextItem`, `ModelChatRequest`, `ModelChatResponse`, `ModelStreamDelta`. Types only — zero runtime code. |
| `sdks/typescript/src/serialize/responses.ts` | Create | The whole Responses codec: encoders (`encodeToolSpec`/`encodeTools`/`encodeToolCall`/`encodeToolOutput`/`encodeContextItem`/`encodeInput`/`buildModelRequestBody`), decoders (`decodeToolCall`/`decodeToolOutput`/`decodeOutputText`/`decodeUsage`/`stopReasonFromStatus`/`modelChatResponseFromOutput`), and the SSE adapter `modelStreamAdapter`. |
| `sdks/typescript/src/stream.ts` | Modify | Adds `BoxModelStream` and `collectModelStream`. |
| `sdks/typescript/src/provider.ts` | Modify | Adds `supportsFreeformTools` to `ProviderCapabilities`; `withFreeformTools()` / `withImageAndFreeformTools()`; `validateModelRequest`; optional `modelChat`/`modelStream` on `ProviderImpl`; `dispatchModelChat` / `dispatchModelStream`; `readTimeoutModelStream`. |
| `sdks/typescript/src/providers/chatgpt_codex.ts` | Modify | `capabilities()` → `withFreeformTools()`; adds `buildModelResponsesBody`, `modelChat`, `modelStream`. |
| `sdks/typescript/src/providers/openai.ts` | Modify | Adds `withResponsesApi(boolean)` (distinct from `withResponsesFallback`); capability switch; `modelChat` (genuine non-streaming POST) and `modelStream`. |
| `sdks/typescript/src/client.ts` | Modify | `ProviderLike` + `asDispatchProvider` forward the model methods; `ClientBuilder.openaiResponsesApi(boolean)`; `Client.modelChat` / `modelStream` / `modelStreamCollect`. |
| `sdks/typescript/src/index.ts` | Modify | Exports `withFreeformTools`, `withImageAndFreeformTools`, `validateModelRequest`; lists the native types for discoverability. |
| `sdks/typescript/tests/serialize.responses.test.ts` | Create | Unit tests for the codec (T1). |
| `sdks/typescript/tests/providers-native-model.test.ts` | Create | Provider/client/dispatch tests for the native surface (T2). |
| `sdks/typescript/tests/capabilities.test.ts` | Modify | Expected capability-shape flips (D5) plus the two new factories. |
| `sdks/typescript/tests/index.test.ts` | Modify | Pins the widened public export surface. |
| `sdks/typescript/tests/freeform-conformance.test.ts` | Create | The TypeScript half of the D9 conformance suite, anchored to `specs/types.md` § Native Model API. |
| `sdks/rust/tests/freeform_conformance.rs` | Create | The Rust half of the same suite. **Test file only — no Rust source changes, no version bump.** |

---

### Task 1: Native model types and Responses tool encoders

**Files:**
- Modify: `sdks/typescript/src/types.ts:172-181` (append after `ChatResponse`)
- Create: `sdks/typescript/src/serialize/responses.ts`
- Test: `sdks/typescript/tests/serialize.responses.test.ts`

**Interfaces:**
- Consumes: existing `types.ts` exports `Tool`, `Message`, `SystemBlock`, `ToolChoice`, `Usage`, `StopReason`, `ContentBlock`.
- Produces: types `FreeformToolFormat`, `FreeformTool`, `ModelToolSpec`, `ImageDetail`, `FunctionCallOutputContentItem`, `FunctionCallOutputPayload`, `ModelToolCall`, `ModelToolOutput`, `ModelContextItem`, `ModelChatRequest`, `ModelChatResponse`, `ModelStreamDelta`; functions `encodeToolSpec(spec: ModelToolSpec): Record<string, unknown>`, `encodeTools(specs: ModelToolSpec[]): Record<string, unknown>[]`, `encodeToolCall(call: ModelToolCall): Record<string, unknown>`, `encodeToolOutput(output: ModelToolOutput): Record<string, unknown>`, `encodeContextItem(item: ModelContextItem): Record<string, unknown>[]`, `encodeInput(context: ModelContextItem[]): Record<string, unknown>[]`.

- [ ] **Step 1: Write the failing test**

Create `sdks/typescript/tests/serialize.responses.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import {
  encodeContextItem,
  encodeInput,
  encodeToolCall,
  encodeToolOutput,
  encodeTools,
  encodeToolSpec,
} from '../src/serialize/responses.js'
import type { FreeformTool, ModelToolSpec } from '../src/types.js'

const GRAMMAR_TOOL: FreeformTool = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
}

describe('encodeToolSpec', () => {
  it('encodes a freeform tool with the mandatory exact format object', () => {
    expect(encodeToolSpec({ kind: 'freeform', tool: GRAMMAR_TOOL })).toEqual({
      type: 'custom',
      name: 'exec',
      description: 'Run JavaScript',
      format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
    })
  })

  it('encodes a function tool with inputSchema under the wire key `parameters`', () => {
    const spec: ModelToolSpec = {
      kind: 'function',
      tool: {
        name: 'get_weather',
        description: 'Fetch the weather',
        inputSchema: { type: 'object', properties: { city: { type: 'string' } } },
      },
    }
    expect(encodeToolSpec(spec)).toEqual({
      type: 'function',
      name: 'get_weather',
      description: 'Fetch the weather',
      parameters: { type: 'object', properties: { city: { type: 'string' } } },
    })
  })

  it("defaults TypeScript's optional description/inputSchema to '' and {}", () => {
    expect(encodeToolSpec({ kind: 'function', tool: { name: 'noop' } })).toEqual({
      type: 'function',
      name: 'noop',
      description: '',
      parameters: {},
    })
  })

  it('encodeTools maps every spec in order', () => {
    expect(
      encodeTools([
        { kind: 'function', tool: { name: 'a', description: 'A', inputSchema: {} } },
        { kind: 'freeform', tool: GRAMMAR_TOOL },
      ]).map((t) => t.type),
    ).toEqual(['function', 'custom'])
  })
})

describe('encodeToolCall', () => {
  it('encodes a freeform call as custom_tool_call with id under call_id', () => {
    expect(
      encodeToolCall({ kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' }),
    ).toEqual({
      type: 'custom_tool_call',
      call_id: 'call_js',
      name: 'exec',
      input: 'console.log(1);',
    })
  })

  it('encodes a function call as function_call with a string arguments field', () => {
    expect(
      encodeToolCall({ kind: 'function', id: 'call_1', name: 'get_weather', arguments: '{"city":"Paris"}' }),
    ).toEqual({
      type: 'function_call',
      call_id: 'call_1',
      name: 'get_weather',
      arguments: '{"city":"Paris"}',
    })
  })

  it('preserves freeform input byte-for-byte and never parses it as JSON', () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    const encoded = encodeToolCall({ kind: 'freeform', id: 'call_js', name: 'exec', input: raw })
    expect(encoded.input).toBe(raw)
    expect(typeof encoded.input).toBe('string')
  })
})

describe('encodeToolOutput', () => {
  it('encodes a function output', () => {
    expect(encodeToolOutput({ kind: 'function', callId: 'call_1', output: 'sunny, 21C' })).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: 'sunny, 21C',
    })
  })

  it('encodes a custom output and drops `name` (mirrors Rust encode_tool_output)', () => {
    expect(
      encodeToolOutput({ kind: 'custom', callId: 'call_js', name: 'exec', output: 'stdout: 42' }),
    ).toEqual({
      type: 'custom_tool_call_output',
      call_id: 'call_js',
      output: 'stdout: 42',
    })
  })

  it('encodes a content-item payload with snake_case wire keys', () => {
    expect(
      encodeToolOutput({
        kind: 'function',
        callId: 'call_1',
        output: [
          { type: 'input_text', text: 'hi' },
          { type: 'input_image', imageUrl: 'https://e.example/a.png', detail: 'high' },
          { type: 'encrypted_content', encryptedContent: 'zzz' },
        ],
      }),
    ).toEqual({
      type: 'function_call_output',
      call_id: 'call_1',
      output: [
        { type: 'input_text', text: 'hi' },
        { type: 'input_image', image_url: 'https://e.example/a.png', detail: 'high' },
        { type: 'encrypted_content', encrypted_content: 'zzz' },
      ],
    })
  })
})

describe('encodeContextItem / encodeInput', () => {
  it('drops system messages from input', () => {
    expect(
      encodeContextItem({ kind: 'message', message: { role: 'system', content: 'be terse' } }),
    ).toEqual([])
  })

  it('encodes a user message as a message item with input_text', () => {
    expect(
      encodeContextItem({ kind: 'message', message: { role: 'user', content: 'run js' } }),
    ).toEqual([
      { type: 'message', role: 'user', content: [{ type: 'input_text', text: 'run js' }] },
    ])
  })

  it('encodes user image blocks as input_image data URLs', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: {
          role: 'user',
          content: 'inspect',
          contentBlocks: [
            { type: 'text', text: 'inspect' },
            { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: 'abc123' } },
          ],
        },
      }),
    ).toEqual([
      {
        type: 'message',
        role: 'user',
        content: [
          { type: 'input_text', text: 'inspect' },
          { type: 'input_image', image_url: 'data:image/png;base64,abc123' },
        ],
      },
    ])
  })

  it('splits an assistant message with tool calls into text + function_call items', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: {
          role: 'assistant',
          content: 'let me look',
          toolCalls: [{ id: 'call_1', name: 'get_weather', input: { city: 'Paris' } }],
        },
      }),
    ).toEqual([
      {
        type: 'message',
        role: 'assistant',
        content: [{ type: 'output_text', text: 'let me look' }],
      },
      {
        type: 'function_call',
        call_id: 'call_1',
        name: 'get_weather',
        arguments: '{"city":"Paris"}',
      },
    ])
  })

  it('encodes a tool-role message as a function_call_output', () => {
    expect(
      encodeContextItem({
        kind: 'message',
        message: { role: 'tool', content: 'sunny', toolCallId: 'call_1' },
      }),
    ).toEqual([{ type: 'function_call_output', call_id: 'call_1', output: 'sunny' }])
  })

  it('preserves mixed message / toolCall / toolOutput order', () => {
    const items = encodeInput([
      { kind: 'message', message: { role: 'user', content: 'run it' } },
      {
        kind: 'toolCall',
        call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
      },
      {
        kind: 'toolOutput',
        output: { kind: 'custom', callId: 'call_js', name: 'exec', output: '1\n' },
      },
    ])
    expect(items.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/serialize.responses.test.ts`

Expected: FAIL with `Failed to load url ../src/serialize/responses.js` (the module does not exist yet).

- [ ] **Step 3: Implement**

Append to `sdks/typescript/src/types.ts` (after `ChatResponse`, at the end of the file):

```ts
// ---------------------------------------------------------------------------
// Native model API (specs/types.md § Native Model API).
//
// A surface PARALLEL to ChatRequest/Tool/ToolCall/ChatResponse/StreamEvent,
// for providers that expose OpenAI Responses-style ordered input items and
// custom (freeform) tool calls. The legacy surface above stays
// function-tool-only; nothing here widens it.
//
// Tag choice (milestone D2): ModelToolSpec / ModelToolCall / ModelToolOutput /
// ModelContextItem are tagged on `kind` because the model shape and the wire
// shape disagree (freeform <-> wire "custom", id <-> wire call_id) — the same
// reason McpToolConfig above uses `kind`. ModelStreamDelta and
// FunctionCallOutputContentItem are tagged on `type` because their tag VALUES
// are exactly the wire values.
//
// Wire encoding lives in serialize/responses.ts, never here.
// ---------------------------------------------------------------------------

/** Grammar/format descriptor for a freeform tool. All three fields are mandatory. */
export interface FreeformToolFormat {
  type: string
  syntax: string
  definition: string
}

/** A freeform ("custom") tool definition. Serializes with a wire `type: "custom"`. */
export interface FreeformTool {
  name: string
  description: string
  format: FreeformToolFormat
}

/**
 * A tool exposed to the model on the native surface. `function` wraps the
 * existing `Tool` (wire `{type:"function", name, description, parameters}`);
 * `freeform` wraps a `FreeformTool` (wire `{type:"custom", name, description,
 * format}`).
 */
export type ModelToolSpec =
  | { kind: 'function'; tool: Tool }
  | { kind: 'freeform'; tool: FreeformTool }

/** Image fidelity hint on a Responses `input_image` content item. */
export type ImageDetail = 'auto' | 'low' | 'high' | 'original'

/** One Responses-style content item inside a tool output payload. */
export type FunctionCallOutputContentItem =
  | { type: 'input_text'; text: string }
  | { type: 'input_image'; imageUrl: string; detail?: ImageDetail }
  | { type: 'encrypted_content'; encryptedContent: string }

/** A tool output payload: plain text, or Responses-style content items. */
export type FunctionCallOutputPayload = string | FunctionCallOutputContentItem[]

/**
 * A tool call the model produced. `id` is the caller-facing identity; it is
 * written to (and read from) the wire key `call_id`. Freeform `input` is raw
 * model text — preserved byte-for-byte, never parsed as JSON, never lowered
 * into a function call's `arguments`.
 */
export type ModelToolCall =
  | { kind: 'function'; id: string; name: string; arguments: string }
  | { kind: 'freeform'; id: string; name: string; input: string }

/** A tool result the caller returns to the model. */
export type ModelToolOutput =
  | { kind: 'function'; callId: string; output: FunctionCallOutputPayload }
  | { kind: 'custom'; callId: string; name?: string; output: FunctionCallOutputPayload }

/**
 * One ordered history entry. Preserving message / tool-call / tool-output
 * ORDER is what makes byte-exact replay of freeform inputs possible in
 * multi-turn histories.
 */
export type ModelContextItem =
  | { kind: 'message'; message: Message }
  | { kind: 'toolCall'; call: ModelToolCall }
  | { kind: 'toolOutput'; output: ModelToolOutput }

/**
 * A native model request. Deliberately carries NO thinking and NO MCP config
 * (milestone D3): native requests reach provider-specific reasoning controls
 * through `providerOptions`.
 */
export interface ModelChatRequest {
  context: ModelContextItem[]
  toolSpecs?: ModelToolSpec[]
  model?: string
  system?: string
  systemBlocks?: SystemBlock[]
  systemCache?: boolean
  temperature?: number
  /** Serialized to the Responses body key `max_output_tokens`. */
  maxTokens?: number
  toolChoice?: ToolChoice
  stopSequences?: string[]
  /** Shallow-merged into the request body root LAST — it overrides everything. */
  providerOptions?: Record<string, unknown>
}

/** A native, non-streaming model response. */
export interface ModelChatResponse {
  content: string
  thinking?: string
  toolCalls: ModelToolCall[]
  model: string
  usage: Usage
  stopReason: StopReason
  sessionId?: string
}

/**
 * One native stream delta. `tool_call_done` is AUTHORITATIVE for a completed
 * call; accumulated `function_arguments` / `freeform_input` deltas are display
 * bookkeeping only. Exactly one `done` per successfully completed stream.
 */
export type ModelStreamDelta =
  | { type: 'text'; delta: string }
  | { type: 'thinking_delta'; delta: string }
  | { type: 'thinking_done'; thinking: string }
  | { type: 'function_arguments'; callId: string; delta: string }
  | { type: 'freeform_input'; callId: string; delta: string }
  | { type: 'tool_call_done'; call: ModelToolCall }
  | { type: 'usage'; usage: Usage }
  | { type: 'done'; stopReason: StopReason }
```

Create `sdks/typescript/src/serialize/responses.ts`:

```ts
/**
 * OpenAI Responses API codec — the ONE place the native model surface is
 * translated to and from the wire.
 *
 * Mirrors Rust `providers/responses.rs`. Both native providers
 * (providers/chatgpt_codex.ts and providers/openai.ts) call into here, so the
 * two cannot drift.
 *
 * Wire divergences that matter (all pinned by tests):
 *   - ModelToolCall.id            -> wire key `call_id` (decoding accepts `call_id` OR `id`)
 *   - ModelChatRequest.maxTokens  -> body key `max_output_tokens`
 *   - Tool.inputSchema            -> wire key `parameters`
 *   - freeform tools              -> wire `type: "custom"` (never "freeform")
 *   - freeform `input`            -> raw text, preserved byte-for-byte, NEVER JSON-parsed
 *
 * Unlike serialize/{anthropic,gemini,openai}.ts this module owns decoding and
 * the SSE adapter too, because the Responses codec is symmetric and shared.
 */

import type {
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  Message,
  ModelContextItem,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
} from '../types.js'

/** Encode one tool definition into its Responses wire object. */
export function encodeToolSpec(spec: ModelToolSpec): Record<string, unknown> {
  if (spec.kind === 'freeform') {
    return {
      type: 'custom',
      name: spec.tool.name,
      description: spec.tool.description,
      format: {
        type: spec.tool.format.type,
        syntax: spec.tool.format.syntax,
        definition: spec.tool.format.definition,
      },
    }
  }
  // Rust's ToolSchema requires description/input_schema; TypeScript's Tool
  // makes them optional, so supply the empty forms rather than emitting null.
  return {
    type: 'function',
    name: spec.tool.name,
    description: spec.tool.description ?? '',
    parameters: spec.tool.inputSchema ?? {},
  }
}

/** Encode the whole tool list, order preserved. */
export function encodeTools(specs: ModelToolSpec[]): Record<string, unknown>[] {
  return specs.map(encodeToolSpec)
}

/** Encode a model tool call. `id` goes out under the wire key `call_id`. */
export function encodeToolCall(call: ModelToolCall): Record<string, unknown> {
  if (call.kind === 'freeform') {
    return {
      type: 'custom_tool_call',
      call_id: call.id,
      name: call.name,
      input: call.input,
    }
  }
  return {
    type: 'function_call',
    call_id: call.id,
    name: call.name,
    arguments: call.arguments,
  }
}

function encodeOutputContentItem(item: FunctionCallOutputContentItem): Record<string, unknown> {
  if (item.type === 'input_text') {
    return { type: 'input_text', text: item.text }
  }
  if (item.type === 'input_image') {
    const encoded: Record<string, unknown> = { type: 'input_image', image_url: item.imageUrl }
    if (item.detail !== undefined) encoded.detail = item.detail
    return encoded
  }
  return { type: 'encrypted_content', encrypted_content: item.encryptedContent }
}

function encodeOutputPayload(payload: FunctionCallOutputPayload): unknown {
  if (typeof payload === 'string') return payload
  return payload.map(encodeOutputContentItem)
}

/**
 * Encode a tool output. NOTE: the custom arm deliberately drops `name` — the
 * Responses body carries identity in `call_id` only, matching Rust
 * `encode_tool_output` (providers/responses.rs:34-49).
 */
export function encodeToolOutput(output: ModelToolOutput): Record<string, unknown> {
  if (output.kind === 'custom') {
    return {
      type: 'custom_tool_call_output',
      call_id: output.callId,
      output: encodeOutputPayload(output.output),
    }
  }
  return {
    type: 'function_call_output',
    call_id: output.callId,
    output: encodeOutputPayload(output.output),
  }
}

function encodeUserContent(message: Message): Record<string, unknown>[] {
  const blocks = message.contentBlocks ?? []
  if (blocks.length === 0) {
    return [{ type: 'input_text', text: message.content }]
  }

  const content: Record<string, unknown>[] = []
  for (const block of blocks) {
    if (block.type === 'text') {
      content.push({ type: 'input_text', text: block.text })
    } else if (block.type === 'image') {
      const source = block.source
      const imageUrl =
        source.type === 'base64'
          ? `data:${source.mediaType};base64,${source.data}`
          : source.url
      content.push({ type: 'input_image', image_url: imageUrl })
    }
    // Document blocks produce nothing (mirrors Rust ContentBlock::Document => {}).
  }

  if (content.length === 0) {
    content.push({ type: 'input_text', text: message.content })
  }
  return content
}

function encodeMessage(message: Message): Record<string, unknown>[] {
  switch (message.role) {
    case 'system':
      // System text belongs in `instructions`, never in `input`.
      return []
    case 'user':
      return [{ type: 'message', role: 'user', content: encodeUserContent(message) }]
    case 'assistant': {
      const items: Record<string, unknown>[] = []
      if (message.content) {
        items.push({
          type: 'message',
          role: 'assistant',
          content: [{ type: 'output_text', text: message.content }],
        })
      }
      for (const call of message.toolCalls ?? []) {
        items.push(
          encodeToolCall({
            kind: 'function',
            id: call.id,
            name: call.name,
            arguments: JSON.stringify(call.input) ?? '{}',
          }),
        )
      }
      return items
    }
    case 'tool':
      if (message.toolCallId === undefined) return []
      return [
        encodeToolOutput({
          kind: 'function',
          callId: message.toolCallId,
          output: message.content,
        }),
      ]
  }
}

/** Encode one ordered history entry into zero or more Responses input items. */
export function encodeContextItem(item: ModelContextItem): Record<string, unknown>[] {
  switch (item.kind) {
    case 'message':
      return encodeMessage(item.message)
    case 'toolCall':
      return [encodeToolCall(item.call)]
    case 'toolOutput':
      return [encodeToolOutput(item.output)]
  }
}

/** Encode the whole ordered context into the Responses `input` array. */
export function encodeInput(context: ModelContextItem[]): Record<string, unknown>[] {
  return context.flatMap(encodeContextItem)
}
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/ts-native-model-types origin/main
git add sdks/typescript/src/types.ts sdks/typescript/src/serialize/responses.ts sdks/typescript/tests/serialize.responses.test.ts
git commit -m "feat: TypeScript native model types and Responses tool encoders (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: `buildModelRequestBody` — system hoisting, wire keys, providerOptions-last

**Files:**
- Modify: `sdks/typescript/src/serialize/responses.ts` (append after `encodeInput`)
- Test: `sdks/typescript/tests/serialize.responses.test.ts` (append)

**Interfaces:**
- Consumes: `encodeInput(context: ModelContextItem[]): Record<string, unknown>[]`, `encodeTools(specs: ModelToolSpec[]): Record<string, unknown>[]` from Task 1.
- Produces: `buildModelRequestBody(req: ModelChatRequest, defaultModel: string, stream: boolean, defaultInstructions?: string): Record<string, unknown>`.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/serialize.responses.test.ts`:

```ts
import { buildModelRequestBody } from '../src/serialize/responses.js'
import type { ModelChatRequest } from '../src/types.js'

describe('buildModelRequestBody', () => {
  it('uses the request model, falling back to the provider default', () => {
    expect(
      buildModelRequestBody({ context: [], model: 'gpt-5.5-codex' }, 'gpt-5.5', false).model,
    ).toBe('gpt-5.5-codex')
    expect(buildModelRequestBody({ context: [] }, 'gpt-5.5', false).model).toBe('gpt-5.5')
  })

  it('sets stream only when asked', () => {
    expect(buildModelRequestBody({ context: [] }, 'm', false).stream).toBeUndefined()
    expect(buildModelRequestBody({ context: [] }, 'm', true).stream).toBe(true)
  })

  it('hoists a system message into instructions AND removes it from input', () => {
    const body = buildModelRequestBody(
      {
        context: [
          { kind: 'message', message: { role: 'system', content: 'be terse' } },
          { kind: 'message', message: { role: 'user', content: 'hi' } },
        ],
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect((body.input as Record<string, unknown>[])[0].role).toBe('user')
  })

  it('prefers systemBlocks over system and joins with a blank line', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        system: 'ignored',
        systemBlocks: [{ text: 'a' }, { text: '  ' }, { text: 'b' }],
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('a\n\nb')
  })

  it('appends hoisted system messages after system/systemBlocks', () => {
    const body = buildModelRequestBody(
      {
        context: [{ kind: 'message', message: { role: 'system', content: 'second' } }],
        system: 'first',
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('first\n\nsecond')
  })

  it('falls back to defaultInstructions only when nothing was supplied', () => {
    expect(
      buildModelRequestBody({ context: [] }, 'm', false, 'You are a helpful assistant.')
        .instructions,
    ).toBe('You are a helpful assistant.')
    expect(
      buildModelRequestBody({ context: [], system: 'given' }, 'm', false, 'default').instructions,
    ).toBe('given')
    expect(buildModelRequestBody({ context: [] }, 'm', false).instructions).toBeUndefined()
  })

  it('maps maxTokens to max_output_tokens and never emits max_tokens', () => {
    const body = buildModelRequestBody({ context: [], maxTokens: 512 }, 'm', false)
    expect(body.max_output_tokens).toBe(512)
    expect(body.max_tokens).toBeUndefined()
  })

  it('emits temperature, tool_choice and stop when set', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        temperature: 0.3,
        toolChoice: { type: 'tool', name: 'exec' },
        stopSequences: ['STOP'],
      },
      'm',
      false,
    )
    expect(body.temperature).toBe(0.3)
    expect(body.tool_choice).toEqual({ type: 'function', name: 'exec' })
    expect(body.stop).toEqual(['STOP'])
  })

  it('maps the string tool choices', () => {
    expect(buildModelRequestBody({ context: [], toolChoice: { type: 'auto' } }, 'm', false).tool_choice).toBe('auto')
    expect(buildModelRequestBody({ context: [], toolChoice: { type: 'required' } }, 'm', false).tool_choice).toBe('required')
    expect(buildModelRequestBody({ context: [], toolChoice: { type: 'none' } }, 'm', false).tool_choice).toBe('none')
  })

  it('omits stop for an empty stopSequences array', () => {
    expect(buildModelRequestBody({ context: [], stopSequences: [] }, 'm', false).stop).toBeUndefined()
  })

  it('omits tools when there are no tool specs', () => {
    expect(buildModelRequestBody({ context: [] }, 'm', false).tools).toBeUndefined()
    expect(buildModelRequestBody({ context: [], toolSpecs: [] }, 'm', false).tools).toBeUndefined()
  })

  it('shallow-merges providerOptions LAST so it overrides encoder output', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        temperature: 0.1,
        providerOptions: { temperature: 0.9, reasoning_effort: 'high', custom: 1 },
      },
      'm',
      false,
    )
    expect(body.temperature).toBe(0.9)
    expect(body.reasoning_effort).toBe('high')
    expect(body.custom).toBe(1)
  })

  it('replays a symmetric freeform history byte-exact and in order', () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    const req: ModelChatRequest = {
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        { kind: 'toolCall', call: { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [
        {
          kind: 'freeform',
          tool: {
            name: 'exec',
            description: 'Run JavaScript',
            format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
          },
        },
      ],
    }
    const body = buildModelRequestBody(req, 'gpt-5.5-codex', false)
    const input = body.input as Record<string, unknown>[]
    expect(input.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    expect(input[1].input).toBe(raw)
    expect(input[1].call_id).toBe('call_js')
    expect((body.tools as Record<string, unknown>[])[0].type).toBe('custom')
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/serialize.responses.test.ts`

Expected: FAIL with `SyntaxError: The requested module '../src/serialize/responses.js' does not provide an export named 'buildModelRequestBody'`

- [ ] **Step 3: Implement**

Append to `sdks/typescript/src/serialize/responses.ts` (and widen its `types.js` import to add `ModelChatRequest` and `ToolChoice`):

```ts
function encodeToolChoice(choice: ToolChoice): unknown {
  switch (choice.type) {
    case 'auto':
      return 'auto'
    case 'required':
      return 'required'
    case 'none':
      return 'none'
    case 'tool':
      return { type: 'function', name: choice.name }
  }
}

/**
 * Build a Responses request body from a native model request.
 *
 * Two non-obvious rules, both load-bearing (Rust build_model_request_body,
 * providers/responses.rs:59-140):
 *
 *   1. System text is HOISTED into `instructions` and REMOVED from `input`.
 *      Precedence: systemBlocks (joined) > system > any Role::System message
 *      inside `context`; hoisted context messages are appended after whichever
 *      of the first two applied. `defaultInstructions` fills in only when
 *      nothing at all was supplied.
 *   2. `providerOptions` is shallow-merged into the body root LAST, so a
 *      caller can override anything this function produced.
 */
export function buildModelRequestBody(
  req: ModelChatRequest,
  defaultModel: string,
  stream: boolean,
  defaultInstructions?: string,
): Record<string, unknown> {
  const model = req.model ?? defaultModel
  const inputContext: ModelContextItem[] = []
  const instructionsParts: string[] = []

  if (req.systemBlocks !== undefined) {
    for (const block of req.systemBlocks) {
      const trimmed = block.text.trim()
      if (trimmed) instructionsParts.push(trimmed)
    }
  } else if (req.system !== undefined) {
    const trimmed = req.system.trim()
    if (trimmed) instructionsParts.push(trimmed)
  }

  for (const item of req.context) {
    if (item.kind === 'message' && item.message.role === 'system') {
      const trimmed = item.message.content.trim()
      if (trimmed) instructionsParts.push(trimmed)
      continue
    }
    inputContext.push(item)
  }

  const body: Record<string, unknown> = {
    model,
    input: encodeInput(inputContext),
  }

  if (stream) body.stream = true

  const toolSpecs = req.toolSpecs ?? []
  if (toolSpecs.length > 0) body.tools = encodeTools(toolSpecs)

  const instructions =
    instructionsParts.length > 0 ? instructionsParts.join('\n\n') : defaultInstructions
  if (instructions !== undefined) body.instructions = instructions

  if (req.temperature !== undefined) body.temperature = req.temperature
  // Wire key divergence: maxTokens -> max_output_tokens.
  if (req.maxTokens !== undefined) body.max_output_tokens = req.maxTokens
  if (req.toolChoice !== undefined) body.tool_choice = encodeToolChoice(req.toolChoice)
  if (req.stopSequences !== undefined && req.stopSequences.length > 0) {
    body.stop = req.stopSequences
  }

  // LAST — providerOptions wins over everything above.
  if (req.providerOptions !== undefined) {
    for (const [key, value] of Object.entries(req.providerOptions)) {
      body[key] = value
    }
  }

  return body
}
```

The import at the top of the file becomes:

```ts
import type {
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  Message,
  ModelChatRequest,
  ModelContextItem,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
  ToolChoice,
} from '../types.js'
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/serialize/responses.ts sdks/typescript/tests/serialize.responses.test.ts
git commit -m "feat: build native Responses request bodies with system hoisting (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: Responses decoders and `modelChatResponseFromOutput`

**Files:**
- Modify: `sdks/typescript/src/serialize/responses.ts` (append after `buildModelRequestBody`)
- Test: `sdks/typescript/tests/serialize.responses.test.ts` (append)

**Interfaces:**
- Consumes: nothing from Tasks 1–2 at runtime; shares the module's `types.js` imports.
- Produces: `decodeToolCall(item: unknown): ModelToolCall | undefined`, `decodeToolOutput(item: unknown): ModelToolOutput | undefined`, `decodeOutputText(item: unknown): string | undefined`, `decodeUsage(value: unknown): Usage`, `stopReasonFromStatus(status: string | undefined, hasToolCalls: boolean): StopReason`, `modelChatResponseFromOutput(payload: unknown, defaultModel: string): ModelChatResponse`.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/serialize.responses.test.ts`:

```ts
import {
  decodeOutputText,
  decodeToolCall,
  decodeToolOutput,
  decodeUsage,
  modelChatResponseFromOutput,
  stopReasonFromStatus,
} from '../src/serialize/responses.js'

describe('decodeToolCall', () => {
  it('decodes a custom_tool_call and keeps input byte-exact', () => {
    const raw = 'const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n'
    expect(
      decodeToolCall({ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }),
    ).toEqual({ kind: 'freeform', id: 'call_js', name: 'exec', input: raw })
  })

  it('decodes a function_call', () => {
    expect(
      decodeToolCall({
        type: 'function_call',
        call_id: 'call_1',
        name: 'get_weather',
        arguments: '{"city":"Paris"}',
      }),
    ).toEqual({ kind: 'function', id: 'call_1', name: 'get_weather', arguments: '{"city":"Paris"}' })
  })

  it('accepts `id` when `call_id` is absent', () => {
    expect(decodeToolCall({ type: 'function_call', id: 'call_2', name: 'f' })).toEqual({
      kind: 'function',
      id: 'call_2',
      name: 'f',
      arguments: '',
    })
  })

  it('returns undefined for non-call items', () => {
    expect(decodeToolCall({ type: 'message', role: 'assistant' })).toBeUndefined()
    expect(decodeToolCall({ type: 'function_call' })).toBeUndefined()
    expect(decodeToolCall('nope')).toBeUndefined()
  })

  it('round-trips a freeform call through encode then decode', () => {
    const raw = 'const x = {a: 1};\nconsole.log(`raw ${x.a}`);\n'
    const call = { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } as const
    expect(decodeToolCall(JSON.parse(JSON.stringify(encodeToolCall(call))))).toEqual(call)
  })
})

describe('decodeToolOutput', () => {
  it('decodes a custom output including its optional name', () => {
    expect(
      decodeToolOutput({
        type: 'custom_tool_call_output',
        call_id: 'call_js',
        name: 'exec',
        output: 'stdout: 42',
      }),
    ).toEqual({ kind: 'custom', callId: 'call_js', name: 'exec', output: 'stdout: 42' })
  })

  it('omits name when the wire has none', () => {
    expect(
      decodeToolOutput({ type: 'custom_tool_call_output', call_id: 'call_js', output: 'x' }),
    ).toEqual({ kind: 'custom', callId: 'call_js', output: 'x' })
  })

  it('decodes a function output with content items', () => {
    expect(
      decodeToolOutput({
        type: 'function_call_output',
        call_id: 'call_1',
        output: [{ type: 'input_text', text: 'hi' }],
      }),
    ).toEqual({ kind: 'function', callId: 'call_1', output: [{ type: 'input_text', text: 'hi' }] })
  })

  it('returns undefined without a payload or a known type', () => {
    expect(decodeToolOutput({ type: 'function_call_output', call_id: 'c' })).toBeUndefined()
    expect(decodeToolOutput({ type: 'other', call_id: 'c', output: 'x' })).toBeUndefined()
  })
})

describe('decodeOutputText', () => {
  it('concatenates output_text parts of a message item', () => {
    expect(
      decodeOutputText({
        type: 'message',
        content: [
          { type: 'output_text', text: 'Hi ' },
          { type: 'refusal', text: 'IGNORED' },
          { type: 'output_text', text: 'there' },
        ],
      }),
    ).toBe('Hi there')
  })

  it('returns undefined for non-message items and empty text', () => {
    expect(decodeOutputText({ type: 'reasoning' })).toBeUndefined()
    expect(decodeOutputText({ type: 'message', content: [] })).toBeUndefined()
  })
})

describe('decodeUsage', () => {
  it('reads Responses keys', () => {
    expect(decodeUsage({ input_tokens: 9, output_tokens: 7 })).toEqual({
      inputTokens: 9,
      outputTokens: 7,
    })
  })

  it('falls back to chat-completions key names', () => {
    expect(decodeUsage({ prompt_tokens: 4, completion_tokens: 5 })).toEqual({
      inputTokens: 4,
      outputTokens: 5,
    })
  })

  it('maps cached_tokens only when positive', () => {
    expect(
      decodeUsage({ input_tokens: 1, output_tokens: 1, input_tokens_details: { cached_tokens: 3 } }),
    ).toEqual({ inputTokens: 1, outputTokens: 1, cacheReadInputTokens: 3 })
    expect(
      decodeUsage({ input_tokens: 1, output_tokens: 1, input_tokens_details: { cached_tokens: 0 } }),
    ).toEqual({ inputTokens: 1, outputTokens: 1 })
  })

  it('returns zeros for missing usage', () => {
    expect(decodeUsage(undefined)).toEqual({ inputTokens: 0, outputTokens: 0 })
  })
})

describe('stopReasonFromStatus', () => {
  it('prefers tool_use whenever there are tool calls', () => {
    expect(stopReasonFromStatus('incomplete', true)).toBe('tool_use')
  })

  it('maps statuses', () => {
    expect(stopReasonFromStatus('completed', false)).toBe('end_turn')
    expect(stopReasonFromStatus(undefined, false)).toBe('end_turn')
    expect(stopReasonFromStatus('incomplete', false)).toBe('max_tokens')
    expect(stopReasonFromStatus('failed', false)).toBe('other')
    expect(stopReasonFromStatus('weird', false)).toBe('other')
  })
})

describe('modelChatResponseFromOutput', () => {
  it('decodes a freeform tool call with tool_use and usage', () => {
    const raw = 'const x = {a: 1};\nconsole.log(x.a);\n'
    const response = modelChatResponseFromOutput(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }],
        usage: { input_tokens: 9, output_tokens: 7 },
      },
      'fallback-model',
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: raw },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.inputTokens).toBe(9)
    expect(response.model).toBe('gpt-5.5-codex')
    expect(response.content).toBe('')
  })

  it('concatenates output_text items into content', () => {
    const response = modelChatResponseFromOutput(
      {
        status: 'completed',
        output: [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: 'ok' }] }],
        usage: { input_tokens: 1, output_tokens: 1 },
      },
      'gpt-5.5-codex',
    )
    expect(response.content).toBe('ok')
    expect(response.model).toBe('gpt-5.5-codex')
    expect(response.stopReason).toBe('end_turn')
  })

  it('keeps reasoning summary text in thinking, separate from content', () => {
    const response = modelChatResponseFromOutput(
      {
        status: 'completed',
        output: [
          { type: 'reasoning', summary: [{ text: 'private reasoning' }] },
          { type: 'message', content: [{ type: 'output_text', text: 'answer' }] },
        ],
      },
      'm',
    )
    expect(response.content).toBe('answer')
    expect(response.thinking).toBe('private reasoning')
  })

  it('omits thinking entirely when there is no reasoning summary', () => {
    const response = modelChatResponseFromOutput({ status: 'completed' }, 'm')
    expect('thinking' in response).toBe(false)
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/serialize.responses.test.ts`

Expected: FAIL with `SyntaxError: The requested module '../src/serialize/responses.js' does not provide an export named 'decodeToolCall'`

- [ ] **Step 3: Implement**

Append to `sdks/typescript/src/serialize/responses.ts` (and add `ModelChatResponse`, `StopReason`, `Usage` to the `types.js` type import):

```ts
function asRecord(value: unknown): Record<string, any> | undefined {
  return value !== null && typeof value === 'object' ? (value as Record<string, any>) : undefined
}

function numberOrZero(value: unknown): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : 0
}

/**
 * Decode a Responses output item into a model tool call.
 *
 * Accepts `call_id` OR `id` for the identity, mirroring Rust's
 * `ModelToolCall` Deserialize. Returns undefined for anything that is not a
 * `function_call` / `custom_tool_call`, which is how the output-item loops use
 * it as a filter.
 */
export function decodeToolCall(item: unknown): ModelToolCall | undefined {
  const obj = asRecord(item)
  if (obj === undefined) return undefined

  const id =
    typeof obj.call_id === 'string' ? obj.call_id : typeof obj.id === 'string' ? obj.id : undefined
  const name = typeof obj.name === 'string' ? obj.name : undefined
  if (id === undefined || name === undefined) return undefined

  if (obj.type === 'function_call') {
    return {
      kind: 'function',
      id,
      name,
      arguments: typeof obj.arguments === 'string' ? obj.arguments : '',
    }
  }
  if (obj.type === 'custom_tool_call') {
    // Raw model text. Never JSON.parse it, never move it into `arguments`.
    return { kind: 'freeform', id, name, input: typeof obj.input === 'string' ? obj.input : '' }
  }
  return undefined
}

function decodeOutputContentItem(value: unknown): FunctionCallOutputContentItem | undefined {
  const obj = asRecord(value)
  if (obj === undefined) return undefined
  if (obj.type === 'input_text' && typeof obj.text === 'string') {
    return { type: 'input_text', text: obj.text }
  }
  if (obj.type === 'input_image' && typeof obj.image_url === 'string') {
    const item: FunctionCallOutputContentItem = { type: 'input_image', imageUrl: obj.image_url }
    if (typeof obj.detail === 'string') {
      item.detail = obj.detail as FunctionCallOutputContentItem extends { detail?: infer D }
        ? NonNullable<D>
        : never
    }
    return item
  }
  if (obj.type === 'encrypted_content' && typeof obj.encrypted_content === 'string') {
    return { type: 'encrypted_content', encryptedContent: obj.encrypted_content }
  }
  return undefined
}

function decodeOutputPayload(value: unknown): FunctionCallOutputPayload | undefined {
  if (typeof value === 'string') return value
  if (!Array.isArray(value)) return undefined
  const items: FunctionCallOutputContentItem[] = []
  for (const entry of value) {
    const item = decodeOutputContentItem(entry)
    if (item === undefined) return undefined
    items.push(item)
  }
  return items
}

/** Decode a Responses output item into a caller tool output. */
export function decodeToolOutput(item: unknown): ModelToolOutput | undefined {
  const obj = asRecord(item)
  if (obj === undefined) return undefined

  const payload = decodeOutputPayload(obj.output)
  if (payload === undefined) return undefined
  const callId = typeof obj.call_id === 'string' ? obj.call_id : undefined
  if (callId === undefined) return undefined

  if (obj.type === 'function_call_output') {
    return { kind: 'function', callId, output: payload }
  }
  if (obj.type === 'custom_tool_call_output') {
    const output: ModelToolOutput = { kind: 'custom', callId, output: payload }
    if (typeof obj.name === 'string') output.name = obj.name
    return output
  }
  return undefined
}

/** Concatenate the `output_text` parts of a Responses `message` output item. */
export function decodeOutputText(item: unknown): string | undefined {
  const obj = asRecord(item)
  if (obj === undefined || obj.type !== 'message') return undefined
  if (!Array.isArray(obj.content)) return undefined

  let text = ''
  for (const part of obj.content) {
    const partObj = asRecord(part)
    if (partObj !== undefined && partObj.type === 'output_text' && typeof partObj.text === 'string') {
      text += partObj.text
    }
  }
  return text === '' ? undefined : text
}

/**
 * Decode a Responses usage object. Accepts the Responses key names and the
 * chat-completions ones; `cacheReadInputTokens` is emitted only when
 * `input_tokens_details.cached_tokens` is greater than zero.
 */
export function decodeUsage(value: unknown): Usage {
  const usage = asRecord(value)
  const inputTokens = numberOrZero(usage?.input_tokens ?? usage?.prompt_tokens)
  const outputTokens = numberOrZero(usage?.output_tokens ?? usage?.completion_tokens)
  const cached = numberOrZero(asRecord(usage?.input_tokens_details)?.cached_tokens)

  const result: Usage = { inputTokens, outputTokens }
  if (cached > 0) result.cacheReadInputTokens = cached
  return result
}

/** Map a Responses `status` to a StopReason. Tool calls always win. */
export function stopReasonFromStatus(
  status: string | undefined,
  hasToolCalls: boolean,
): StopReason {
  if (hasToolCalls) return 'tool_use'
  if (status === 'incomplete') return 'max_tokens'
  if (status === undefined || status === 'completed') return 'end_turn'
  return 'other'
}

/**
 * Decode a NON-STREAMING Responses payload into a ModelChatResponse. This is
 * the genuine blocking decode path (OpenAI); ChatGPT Codex has no
 * non-streaming endpoint and reaches the same shape via collectModelStream.
 */
export function modelChatResponseFromOutput(
  payload: unknown,
  defaultModel: string,
): ModelChatResponse {
  const obj = asRecord(payload) ?? {}
  let content = ''
  let thinking: string | undefined
  const toolCalls: ModelToolCall[] = []

  if (typeof obj.output_text === 'string') content += obj.output_text

  if (Array.isArray(obj.output)) {
    for (const item of obj.output) {
      const text = decodeOutputText(item)
      if (text !== undefined) content += text

      const itemObj = asRecord(item)
      if (itemObj !== undefined && itemObj.type === 'reasoning' && Array.isArray(itemObj.summary)) {
        let summary = ''
        for (const part of itemObj.summary) {
          const partObj = asRecord(part)
          if (partObj === undefined) continue
          const value =
            typeof partObj.text === 'string'
              ? partObj.text
              : typeof partObj.content === 'string'
                ? partObj.content
                : undefined
          if (value !== undefined) summary += value
        }
        if (summary !== '') thinking = summary
      }

      const call = decodeToolCall(item)
      if (call !== undefined) toolCalls.push(call)
    }
  }

  const response: ModelChatResponse = {
    content,
    toolCalls,
    model: typeof obj.model === 'string' ? obj.model : defaultModel,
    usage: decodeUsage(obj.usage),
    stopReason: stopReasonFromStatus(
      typeof obj.status === 'string' ? obj.status : undefined,
      toolCalls.length > 0,
    ),
  }
  if (thinking !== undefined) response.thinking = thinking
  return response
}
```

The `types.js` type import at the top of the file becomes:

```ts
import type {
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  Message,
  ModelChatRequest,
  ModelChatResponse,
  ModelContextItem,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
  StopReason,
  ToolChoice,
  Usage,
} from '../types.js'
```

> If the conditional-type cast inside `decodeOutputContentItem` reads awkwardly under review, replace it with the plain narrowing form
> `if (obj.detail === 'auto' || obj.detail === 'low' || obj.detail === 'high' || obj.detail === 'original') item.detail = obj.detail`
> — same behaviour, no cast. Either is acceptable; the tests do not distinguish them.

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/serialize/responses.ts sdks/typescript/tests/serialize.responses.test.ts
git commit -m "feat: decode native Responses payloads into ModelChatResponse (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: `modelStreamAdapter` — Responses SSE to `ModelStreamDelta` (closes PR T1)

**Files:**
- Modify: `sdks/typescript/src/serialize/responses.ts` (append after `modelChatResponseFromOutput`)
- Test: `sdks/typescript/tests/serialize.responses.test.ts` (append)

**Interfaces:**
- Consumes: `decodeToolCall`, `decodeUsage`, `stopReasonFromStatus` from Task 3; `parseSse(body: ReadableStream<Uint8Array>): AsyncGenerator<SseEvent>` from `../http/sse.js`; `StreamError`, `IncompleteStreamError` from `../error.js`.
- Produces: `modelStreamAdapter(body: ReadableStream<Uint8Array>, provider: string): AsyncGenerator<ModelStreamDelta>`.

The four frame families the legacy adapters do **not** handle and that live only here: `response.custom_tool_call_input.delta`, `custom_tool_call` output items, `response.reasoning_text.done` / `response.reasoning_summary_text.done`, and `response.incomplete`.

`call_id` resolution order, exactly: event `call_id` → `item_id` looked up in the item→call map → raw `item_id`.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/serialize.responses.test.ts`:

```ts
import { modelStreamAdapter } from '../src/serialize/responses.js'
import { IncompleteStreamError, StreamError } from '../src/error.js'
import type { ModelStreamDelta } from '../src/types.js'

function sseBody(text: string): ReadableStream<Uint8Array> {
  return new ReadableStream({
    start(controller) {
      controller.enqueue(new TextEncoder().encode(text))
      controller.close()
    },
  })
}

async function drainDeltas(sse: string, provider = 'openai'): Promise<ModelStreamDelta[]> {
  const deltas: ModelStreamDelta[] = []
  for await (const delta of modelStreamAdapter(sseBody(sse), provider)) deltas.push(delta)
  return deltas
}

describe('modelStreamAdapter', () => {
  it('decodes custom tool input deltas plus an authoritative tool_call_done', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);\\n"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'freeform_input', callId: 'call_js', delta: 'console.' },
      { type: 'freeform_input', callId: 'call_js', delta: 'log(1);\n' },
      {
        type: 'tool_call_done',
        call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
      },
      { type: 'usage', usage: { inputTokens: 2, outputTokens: 3 } },
      { type: 'done', stopReason: 'tool_use' },
    ])
  })

  it('emits text deltas and skips empty ones', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":""}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"lo"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'text', delta: 'hel' },
      { type: 'text', delta: 'lo' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('emits thinking deltas and a thinking_done from both reasoning families', async () => {
    const sse =
      'data: {"type":"response.reasoning_text.delta","delta":"think "}\n\n' +
      'data: {"type":"response.reasoning_summary_text.delta","delta":"hard"}\n\n' +
      'data: {"type":"response.reasoning_summary_text.done","text":"think hard"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'thinking_delta', delta: 'think ' },
      { type: 'thinking_delta', delta: 'hard' },
      { type: 'thinking_done', thinking: 'think hard' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('resolves call_id through the item map when frames are keyed by item_id', async () => {
    const sse =
      'data: {"type":"response.output_item.added","item":{"type":"function_call","id":"fc_1","call_id":"call_1","name":"f"}}\n\n' +
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_1","delta":"{\\"a\\":1}"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'function_arguments', callId: 'call_1', delta: '{"a":1}' },
      { type: 'done', stopReason: 'tool_use' },
    ])
  })

  it('falls back to the raw item_id when the map has no entry', async () => {
    const sse =
      'data: {"type":"response.function_call_arguments.delta","item_id":"fc_orphan","delta":"x"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'function_arguments', callId: 'fc_orphan', delta: 'x' },
      { type: 'done', stopReason: 'end_turn' },
    ])
  })

  it('maps response.incomplete to a max_tokens done', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
      'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n'

    expect(await drainDeltas(sse)).toEqual([
      { type: 'text', delta: 'partial' },
      { type: 'usage', usage: { inputTokens: 6, outputTokens: 7 } },
      { type: 'done', stopReason: 'max_tokens' },
    ])
  })

  it('omits the usage delta when the terminal frame carries no usage', async () => {
    const sse = 'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    expect(await drainDeltas(sse)).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('ignores empty, [DONE], malformed and unknown frames', async () => {
    const sse =
      'data: \n\n' +
      'data: [DONE]\n\n' +
      'data: {not json}\n\n' +
      'data: {"type":"response.unknown"}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n'
    expect(await drainDeltas(sse)).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('drains pending deltas before surfacing a stream error frame', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"before"}\n\n' +
      'data: {"type":"error","message":"upstream exploded"}\n\n'

    const deltas: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of modelStreamAdapter(sseBody(sse), 'openai')) deltas.push(delta)
    } catch (error) {
      caught = error
    }
    expect(deltas).toEqual([{ type: 'text', delta: 'before' }])
    expect(caught).toBeInstanceOf(StreamError)
    expect((caught as Error).message).toBe('upstream exploded')
  })

  it('uses the nested response.error.message, then error.message, then a fallback', async () => {
    const nested =
      'data: {"type":"response.failed","response":{"error":{"message":"nested boom"}}}\n\n'
    await expect(drainDeltas(nested)).rejects.toThrow('nested boom')

    const top = 'data: {"type":"error","error":{"message":"top boom"}}\n\n'
    await expect(drainDeltas(top)).rejects.toThrow('top boom')

    const bare = 'data: {"type":"error"}\n\n'
    await expect(drainDeltas(bare)).rejects.toThrow('responses stream error')
  })

  it('throws IncompleteStreamError with the openai payload on EOF without a terminal', async () => {
    const sse =
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"lo"}\n\n'

    const deltas: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of modelStreamAdapter(sseBody(sse), 'openai')) deltas.push(delta)
    } catch (error) {
      caught = error
    }
    expect(deltas.some((d) => d.type === 'done')).toBe(false)
    expect(caught).toBeInstanceOf(IncompleteStreamError)
    expect(caught).toBeInstanceOf(StreamError)
    expect((caught as Error).message).toBe(
      'incomplete stream: openai ended without a terminal event',
    )
  })

  it('uses the hyphenated chatgpt-codex provider token on the native path', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n'
    await expect(drainDeltas(sse, 'chatgpt-codex')).rejects.toThrow(
      'incomplete stream: chatgpt-codex ended without a terminal event',
    )
  })

  it('emits exactly one done even when frames follow the terminal', async () => {
    const sse =
      'data: {"type":"response.completed","response":{"status":"completed"}}\n\n' +
      'data: {"type":"response.output_text.delta","delta":"trailing"}\n\n'
    const deltas = await drainDeltas(sse)
    expect(deltas.filter((d) => d.type === 'done')).toHaveLength(1)
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/serialize.responses.test.ts`

Expected: FAIL with `SyntaxError: The requested module '../src/serialize/responses.js' does not provide an export named 'modelStreamAdapter'`

- [ ] **Step 3: Implement**

Add the two runtime imports at the top of `sdks/typescript/src/serialize/responses.ts`, above the `import type` block:

```ts
import { IncompleteStreamError, StreamError } from '../error.js'
import { parseSse } from '../http/sse.js'
```

Add `ModelStreamDelta` to the `types.js` type import, then append:

```ts
/**
 * First non-empty wins: top-level `message` -> `response.error.message` ->
 * `error.message` -> fallback. Mirrors Rust's error arm in
 * ResponsesModelStreamAdapter::handle_event.
 */
function responsesStreamErrorMessage(data: Record<string, any>): string {
  if (typeof data.message === 'string' && data.message) return data.message
  const nested = asRecord(asRecord(data.response)?.error)?.message
  if (typeof nested === 'string' && nested) return nested
  const top = asRecord(data.error)?.message
  if (typeof top === 'string' && top) return top
  return 'responses stream error'
}

/**
 * Adapt a Responses SSE byte stream into ModelStreamDelta values.
 *
 * Contract (specs/types.md § Stream termination (native), milestone D8):
 *   - Exactly ONE terminal `done` per successfully completed stream, emitted
 *     on `response.completed` or `response.incomplete`. Frames after the
 *     terminal are consumed but produce no second done.
 *   - EOF without either terminal throws IncompleteStreamError carrying
 *     `<provider> ended without a terminal event`. Callers pass `openai` or
 *     `chatgpt-codex` (HYPHEN — the legacy chat adapter's `chatgpt_codex`
 *     token is a different, unchanged string).
 *   - Pending deltas from a frame are yielded before an error frame throws.
 *   - `tool_call_done` is authoritative; the accumulated input/argument
 *     deltas are display bookkeeping only.
 */
export async function* modelStreamAdapter(
  body: ReadableStream<Uint8Array>,
  provider: string,
): AsyncGenerator<ModelStreamDelta> {
  const itemToCallId = new Map<string, string>()
  let sawToolCall = false
  let sawTerminal = false

  const rememberOutputItem = (item: Record<string, any>): void => {
    if (item.type !== 'function_call' && item.type !== 'custom_tool_call') return
    if (typeof item.call_id !== 'string') return
    sawToolCall = true
    if (typeof item.id === 'string' && item.id !== '') itemToCallId.set(item.id, item.call_id)
  }

  const callIdFromEvent = (data: Record<string, any>): string | undefined => {
    if (typeof data.call_id === 'string') return data.call_id
    if (typeof data.item_id === 'string') return itemToCallId.get(data.item_id) ?? data.item_id
    return undefined
  }

  for await (const evt of parseSse(body)) {
    const data = evt.data
    if (!data || data === '[DONE]' || typeof data !== 'object') continue

    switch (data.type) {
      case 'response.output_text.delta': {
        if (typeof data.delta === 'string' && data.delta !== '') {
          yield { type: 'text', delta: data.delta }
        }
        break
      }
      case 'response.reasoning_text.delta':
      case 'response.reasoning_summary_text.delta': {
        if (typeof data.delta === 'string' && data.delta !== '') {
          yield { type: 'thinking_delta', delta: data.delta }
        }
        break
      }
      case 'response.reasoning_text.done':
      case 'response.reasoning_summary_text.done': {
        const thinking =
          typeof data.text === 'string'
            ? data.text
            : typeof data.delta === 'string'
              ? data.delta
              : undefined
        if (thinking !== undefined) yield { type: 'thinking_done', thinking }
        break
      }
      case 'response.output_item.added': {
        const item = asRecord(data.item)
        if (item !== undefined) rememberOutputItem(item)
        break
      }
      case 'response.function_call_arguments.delta': {
        const callId = callIdFromEvent(data)
        if (callId !== undefined && typeof data.delta === 'string') {
          yield { type: 'function_arguments', callId, delta: data.delta }
        }
        break
      }
      case 'response.custom_tool_call_input.delta': {
        const callId = callIdFromEvent(data)
        if (callId !== undefined && typeof data.delta === 'string') {
          yield { type: 'freeform_input', callId, delta: data.delta }
        }
        break
      }
      case 'response.output_item.done': {
        const item = asRecord(data.item)
        if (item !== undefined) {
          rememberOutputItem(item)
          const call = decodeToolCall(item)
          if (call !== undefined) {
            sawToolCall = true
            yield { type: 'tool_call_done', call }
          }
        }
        break
      }
      case 'response.completed':
      case 'response.incomplete': {
        const response = asRecord(data.response)
        const usage = decodeUsage(response?.usage)
        if (
          usage.inputTokens !== 0 ||
          usage.outputTokens !== 0 ||
          usage.cacheCreationInputTokens !== undefined ||
          usage.cacheReadInputTokens !== undefined
        ) {
          yield { type: 'usage', usage }
        }
        const status = typeof response?.status === 'string' ? response.status : undefined
        yield { type: 'done', stopReason: stopReasonFromStatus(status, sawToolCall) }
        sawTerminal = true
        break
      }
      case 'error':
      case 'response.failed': {
        sawTerminal = true
        throw new StreamError(responsesStreamErrorMessage(data))
      }
      default:
        break
    }
  }

  if (!sawTerminal) {
    throw new IncompleteStreamError(`incomplete stream: ${provider} ended without a terminal event`)
  }
}
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit, push, verify, open PR T1**

```bash
git add sdks/typescript/src/serialize/responses.ts sdks/typescript/tests/serialize.responses.test.ts
git commit -m "feat: adapt Responses SSE frames into native model stream deltas (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

# fresh-worktree prerequisite for the pre-push hook
(cd sdks/python && uv sync --all-extras)
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/ts-native-model-types
test "$(git ls-remote origin refs/heads/feat/ts-native-model-types | cut -f1)" = "$(git rev-parse HEAD)"

gh pr create --base main --head feat/ts-native-model-types \
  --title "feat: TypeScript native model types and Responses codec (#270)" \
  --body "Task group T1 of the Freeform parity milestone (#270). Adds the native model type family to \`types.ts\` and the shared \`serialize/responses.ts\` codec (encoders, decoders, SSE adapter). No provider wiring — that is T2.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

---

### Task 5: `collectModelStream` and `BoxModelStream`

> **PR T2 starts here.** Branch off `origin/main` *after* T1 has merged.

**Files:**
- Modify: `sdks/typescript/src/stream.ts:1` (import line) and `:217` (append after `collectStream`)
- Test: `sdks/typescript/tests/providers-native-model.test.ts` (create)

**Interfaces:**
- Consumes: types `ModelChatResponse`, `ModelStreamDelta`, `ModelToolCall`, `StopReason`, `Usage` from `./types.js`.
- Produces: `export type BoxModelStream = AsyncIterable<ModelStreamDelta>`; `collectModelStream(stream: BoxModelStream): Promise<ModelChatResponse>`.

- [ ] **Step 1: Write the failing test**

Create `sdks/typescript/tests/providers-native-model.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { collectModelStream } from '../src/stream.js'
import type { ModelStreamDelta } from '../src/types.js'

async function* deltas(items: ModelStreamDelta[]): AsyncGenerator<ModelStreamDelta> {
  for (const item of items) yield item
}

describe('collectModelStream', () => {
  it('preserves the completed freeform call and never lowers accumulated deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'freeform_input', callId: 'call_js', delta: 'console.' },
        { type: 'freeform_input', callId: 'call_js', delta: 'log(1);' },
        {
          type: 'tool_call_done',
          call: { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
        },
        { type: 'usage', usage: { inputTokens: 2, outputTokens: 3 } },
        { type: 'done', stopReason: 'tool_use' },
      ]),
    )

    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
    expect(response.model).toBe('')
  })

  it('prefers thinking_done over accumulated thinking deltas', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'think ' },
        { type: 'thinking_delta', delta: 'hard' },
        { type: 'thinking_done', thinking: 'think hard' },
        { type: 'text', delta: 'answer' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.content).toBe('answer')
    expect(response.thinking).toBe('think hard')
  })

  it('falls back to concatenated thinking deltas when no thinking_done arrives', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'partial ' },
        { type: 'thinking_delta', delta: 'reasoning' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBe('partial reasoning')
  })

  it('drops an empty thinking_done payload entirely', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'thinking_delta', delta: 'ignored' },
        { type: 'thinking_done', thinking: '' },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.thinking).toBeUndefined()
  })

  it('REPLACES usage rather than merging it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'usage', usage: { inputTokens: 100, outputTokens: 100, cacheReadInputTokens: 9 } },
        { type: 'usage', usage: { inputTokens: 1, outputTokens: 2 } },
        { type: 'done', stopReason: 'end_turn' },
      ]),
    )
    expect(response.usage).toEqual({ inputTokens: 1, outputTokens: 2 })
  })

  it('stops at the terminal done and ignores anything after it', async () => {
    const response = await collectModelStream(
      deltas([
        { type: 'text', delta: 'kept' },
        { type: 'done', stopReason: 'end_turn' },
        { type: 'text', delta: 'dropped' },
      ]),
    )
    expect(response.content).toBe('kept')
  })

  it('infers tool_use when no done delta carried a stop reason', async () => {
    const response = await collectModelStream(
      deltas([
        {
          type: 'tool_call_done',
          call: { kind: 'function', id: 'call_1', name: 'f', arguments: '{}' },
        },
      ]),
    )
    expect(response.stopReason).toBe('tool_use')
  })

  it('infers end_turn for a bare text stream with no done delta', async () => {
    const response = await collectModelStream(deltas([{ type: 'text', delta: 'hi' }]))
    expect(response.stopReason).toBe('end_turn')
  })

  it('propagates a mid-stream error instead of returning a partial response', async () => {
    async function* failing(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'partial' }
      throw new Error('boom')
    }
    await expect(collectModelStream(failing())).rejects.toThrow('boom')
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/providers-native-model.test.ts`

Expected: FAIL with `SyntaxError: The requested module '../src/stream.js' does not provide an export named 'collectModelStream'`

- [ ] **Step 3: Implement**

Change line 1 of `sdks/typescript/src/stream.ts` to:

```ts
import type {
  ChatResponse,
  ModelChatResponse,
  ModelStreamDelta,
  ModelToolCall,
  StopReason,
  StreamEvent,
  ToolCall,
  Usage,
} from './types.js'
```

Append to `sdks/typescript/src/stream.ts`:

```ts
/** An async iterable of native model stream deltas. Mirrors Rust `BoxModelStream`. */
export type BoxModelStream = AsyncIterable<ModelStreamDelta>

/**
 * Reassemble a native model stream into a ModelChatResponse.
 *
 * Mirrors Rust `collect_model_stream` (stream.rs:155-239). Three rules differ
 * from `collectStream` above and are contract, not implementation detail
 * (milestone D8):
 *
 *   1. `tool_call_done` is AUTHORITATIVE. The accumulated `function_arguments`
 *      / `freeform_input` buffers exist so a caller can render progress; they
 *      are discarded for the completed call and NEVER lowered into it. In
 *      particular a freeform `input` is never JSON-parsed.
 *   2. `usage` REPLACES the running value. (collectStream uses
 *      replace-with-fallback because Anthropic's message_delta usage is
 *      cumulative; the Responses terminal frame is not.)
 *   3. `thinking_done` wins over concatenated `thinking_delta` content, and an
 *      empty `thinking_done` payload means "no thinking", not "empty string".
 *
 * `model` is returned EMPTY — the caller (provider or Client) backfills it
 * from the request/provider default.
 */
export async function collectModelStream(stream: BoxModelStream): Promise<ModelChatResponse> {
  let content = ''
  let thinkingDeltaBuf = ''
  let thinkingDoneBuf: string | undefined
  const functionArguments = new Map<string, string>()
  const freeformInputs = new Map<string, string>()
  const toolCalls: ModelToolCall[] = []
  let usage: Usage = { inputTokens: 0, outputTokens: 0 }
  let explicitStopReason: StopReason | undefined

  for await (const delta of stream) {
    if (delta.type === 'done') {
      explicitStopReason = delta.stopReason
      break
    }

    switch (delta.type) {
      case 'text':
        content += delta.delta
        break

      case 'thinking_delta':
        thinkingDeltaBuf += delta.delta
        break

      case 'thinking_done':
        thinkingDoneBuf = delta.thinking
        thinkingDeltaBuf = ''
        break

      case 'function_arguments':
        functionArguments.set(
          delta.callId,
          (functionArguments.get(delta.callId) ?? '') + delta.delta,
        )
        break

      case 'freeform_input':
        freeformInputs.set(delta.callId, (freeformInputs.get(delta.callId) ?? '') + delta.delta)
        break

      case 'tool_call_done':
        // Discard the bookkeeping buffer; the completed call is the truth.
        if (delta.call.kind === 'function') {
          functionArguments.delete(delta.call.id)
        } else {
          freeformInputs.delete(delta.call.id)
        }
        toolCalls.push(delta.call)
        break

      case 'usage':
        usage = delta.usage
        break
    }
  }

  let thinking: string | undefined
  if (thinkingDoneBuf !== undefined) {
    thinking = thinkingDoneBuf.length > 0 ? thinkingDoneBuf : undefined
  } else if (thinkingDeltaBuf.length > 0) {
    thinking = thinkingDeltaBuf
  }

  const stopReason: StopReason =
    explicitStopReason ?? (toolCalls.length > 0 ? 'tool_use' : 'end_turn')

  const response: ModelChatResponse = {
    content,
    toolCalls,
    model: '',
    usage,
    stopReason,
  }
  if (thinking !== undefined) response.thinking = thinking
  return response
}
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git checkout -b feat/ts-native-model-providers origin/main
git add sdks/typescript/src/stream.ts sdks/typescript/tests/providers-native-model.test.ts
git commit -m "feat: collect native model streams into a ModelChatResponse (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: Freeform capability, native validation, native dispatch, native read-idle timeout

**Files:**
- Modify: `sdks/typescript/src/provider.ts:8-10` (imports), `:20-61` (capabilities), `:127-131` (`ProviderImpl`), `:159` (append dispatch), `:218` (append `readTimeoutModelStream`)
- Modify: `sdks/typescript/tests/capabilities.test.ts:14-46` (expected capability-shape flips)
- Test: `sdks/typescript/tests/providers-native-model.test.ts` (append)

**Interfaces:**
- Consumes: `BoxModelStream` from Task 5; `UnsupportedFeatureError`, `StreamReadTimeoutError` from `./error.js`.
- Produces: `ProviderCapabilities.supportsFreeformTools: boolean`; `withFreeformTools(): ProviderCapabilities`; `withImageAndFreeformTools(): ProviderCapabilities`; `validateModelRequest(req: ModelChatRequest, caps: ProviderCapabilities): void`; optional `ProviderImpl.modelChat?(req, opts?): Promise<ModelChatResponse>` and `ProviderImpl.modelStream?(req, opts?): BoxModelStream`; `dispatchModelChat(provider, req, opts?): Promise<ModelChatResponse>`; `dispatchModelStream(provider, req, opts?): BoxModelStream`; `readTimeoutModelStream(inner: BoxModelStream, timeoutSecs: number): BoxModelStream`.

**D5 flip list (expected, not failures):** every existing assertion of an exact `ProviderCapabilities` object gains `supportsFreeformTools: false` — `tests/capabilities.test.ts:14-46` and `tests/index.test.ts:20-35`. `index.test.ts` is updated in Task 10.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/providers-native-model.test.ts`:

```ts
import {
  dispatchModelChat,
  dispatchModelStream,
  readTimeoutModelStream,
  textOnly,
  validateModelRequest,
  withFreeformTools,
  withImage,
  withImageAndFreeformTools,
  type ProviderImpl,
} from '../src/provider.js'
import { StreamReadTimeoutError, UnsupportedFeatureError } from '../src/error.js'
import type { ModelChatRequest } from '../src/types.js'

const FREEFORM_SPEC_REQUEST: ModelChatRequest = {
  context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
  toolSpecs: [
    {
      kind: 'freeform',
      tool: {
        name: 'exec',
        description: 'Run JavaScript',
        format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
      },
    },
  ],
}

describe('freeform capability factories (D5)', () => {
  it('withFreeformTools() is text-only plus freeform', () => {
    expect(withFreeformTools()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('withImageAndFreeformTools() adds images', () => {
    expect(withImageAndFreeformTools()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('the pre-existing factories keep freeform false', () => {
    expect(textOnly().supportsFreeformTools).toBe(false)
    expect(withImage().supportsFreeformTools).toBe(false)
  })
})

describe('validateModelRequest', () => {
  it('rejects a freeform tool spec on a non-freeform provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      UnsupportedFeatureError,
    )
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withImage())).toThrow(
      'provider does not support native freeform tools',
    )
  })

  it('rejects freeform history even without a freeform spec', () => {
    const withCall: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'freeform', id: 'c', name: 'exec', input: 'x' } },
      ],
    }
    const withOutput: ModelChatRequest = {
      context: [{ kind: 'toolOutput', output: { kind: 'custom', callId: 'c', output: 'x' } }],
    }
    expect(() => validateModelRequest(withCall, withImage())).toThrow(UnsupportedFeatureError)
    expect(() => validateModelRequest(withOutput, withImage())).toThrow(UnsupportedFeatureError)
  })

  it('accepts freeform on a freeform-capable provider', () => {
    expect(() => validateModelRequest(FREEFORM_SPEC_REQUEST, withFreeformTools())).not.toThrow()
  })

  it('accepts function specs and function history everywhere', () => {
    const req: ModelChatRequest = {
      context: [
        { kind: 'toolCall', call: { kind: 'function', id: 'c', name: 'f', arguments: '{}' } },
        { kind: 'toolOutput', output: { kind: 'function', callId: 'c', output: 'ok' } },
      ],
      toolSpecs: [{ kind: 'function', tool: { name: 'f', description: 'F', inputSchema: {} } }],
    }
    expect(() => validateModelRequest(req, textOnly())).not.toThrow()
  })

  it('rejects image blocks in context on a text-only provider', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'image', source: { type: 'url', url: 'https://e.example/a.png' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withFreeformTools())).toThrow(
      'provider does not support image input',
    )
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).not.toThrow()
  })

  it('rejects document blocks in context', () => {
    const req: ModelChatRequest = {
      context: [
        {
          kind: 'message',
          message: {
            role: 'user',
            content: '',
            contentBlocks: [
              { type: 'document', source: { type: 'url', url: 'https://e.example/a.pdf' } },
            ],
          },
        },
      ],
    }
    expect(() => validateModelRequest(req, withImageAndFreeformTools())).toThrow(
      'provider does not support document input',
    )
  })
})

describe('native dispatch', () => {
  const bareProvider: ProviderImpl = {
    capabilities: withFreeformTools,
    chat: async () => {
      throw new Error('unused')
    },
    stream: () => {
      throw new Error('unused')
    },
  }

  it('rejects modelChat on a provider that implements no native surface', async () => {
    await expect(dispatchModelChat(bareProvider, { context: [] })).rejects.toThrow(
      'provider does not support native model requests',
    )
  })

  it('rejects modelStream on a provider that implements no native surface', () => {
    expect(() => dispatchModelStream(bareProvider, { context: [] })).toThrow(
      'provider does not support native model streams',
    )
  })

  it('validates BEFORE consulting the native surface', async () => {
    const imageOnly: ProviderImpl = { ...bareProvider, capabilities: withImage }
    await expect(dispatchModelChat(imageOnly, FREEFORM_SPEC_REQUEST)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
  })
})

describe('readTimeoutModelStream', () => {
  it('passes deltas through untouched', async () => {
    const out: ModelStreamDelta[] = []
    for await (const delta of readTimeoutModelStream(
      deltas([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }]),
      5,
    )) {
      out.push(delta)
    }
    expect(out).toEqual([{ type: 'text', delta: 'a' }, { type: 'done', stopReason: 'end_turn' }])
  })

  it('throws StreamReadTimeoutError on an idle gap and fabricates no done', async () => {
    async function* stalls(): AsyncGenerator<ModelStreamDelta> {
      yield { type: 'text', delta: 'tick' }
      await new Promise((resolve) => setTimeout(resolve, 500))
      yield { type: 'done', stopReason: 'end_turn' }
    }

    const out: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of readTimeoutModelStream(stalls(), 0.05)) out.push(delta)
    } catch (error) {
      caught = error
    }
    expect(caught).toBeInstanceOf(StreamReadTimeoutError)
    expect(out).toEqual([{ type: 'text', delta: 'tick' }])
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/providers-native-model.test.ts`

Expected: FAIL with `SyntaxError: The requested module '../src/provider.js' does not provide an export named 'withFreeformTools'`

- [ ] **Step 3: Implement**

Replace the import block at `sdks/typescript/src/provider.ts:8-10`:

```ts
import { StreamReadTimeoutError, UnsupportedFeatureError } from './error.js'
import type { BoxModelStream, BoxStream } from './stream.js'
import type {
  ChatRequest,
  ChatResponse,
  ContentBlock,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StreamEvent,
} from './types.js'
```

Replace `ProviderCapabilities` and the four factories (`provider.ts:20-61`):

```ts
/**
 * Describes what features a provider supports.
 *
 * Mirrors Rust `ProviderCapabilities` (types.rs:1518-1523) PLUS a TS-only
 * `supportsMcp` flag — Rust has no MCP capability and achieves "no MCP on
 * OpenAI" by serializer omission, while TS lets validateRequest reject it.
 * `supportsFreeformTools` gates the NATIVE model surface only; it says nothing
 * about the legacy function-tool surface.
 */
export interface ProviderCapabilities {
  supportsImage: boolean
  supportsDocument: boolean
  /** TS-only divergence from Rust: whether the provider accepts MCP server/tool config. */
  supportsMcp: boolean
  /** Whether the provider speaks native Freeform (custom) tools. */
  supportsFreeformTools: boolean
}

/** Provider with text only — no images, no documents, no freeform tools. */
export function textOnly(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: false,
  }
}

/** Provider with image support — images but no documents, no freeform tools. */
export function withImage(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: false,
  }
}

/**
 * Provider with full legacy support — images and documents.
 *
 * `supportsFreeformTools` stays FALSE on purpose, matching Rust
 * `ProviderCapabilities::full()` (types.rs:1558-1564). A provider that flipped
 * it here would silently claim native support it does not have.
 */
export function fullCaps(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: true,
    supportsMcp: true,
    supportsFreeformTools: false,
  }
}

/**
 * MiniMax capabilities: text-only (no images/documents) but MCP-capable,
 * because MiniMax routes through the Anthropic-compatible wire (contract §5/§6).
 */
export function minimaxCaps(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: true,
    supportsFreeformTools: false,
  }
}

/**
 * Text-only provider that speaks native Freeform tools (ChatGPT Codex).
 * Mirrors Rust `ProviderCapabilities::with_freeform_tools()`.
 */
export function withFreeformTools(): ProviderCapabilities {
  return {
    supportsImage: false,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: true,
  }
}

/**
 * Image-capable provider that speaks native Freeform tools (OpenAI with the
 * Responses opt-in on). Mirrors Rust
 * `ProviderCapabilities::with_image_and_freeform_tools()`.
 */
export function withImageAndFreeformTools(): ProviderCapabilities {
  return {
    supportsImage: true,
    supportsDocument: false,
    supportsMcp: false,
    supportsFreeformTools: true,
  }
}
```

Replace `ProviderImpl` (`provider.ts:127-131`):

```ts
/**
 * Minimal shape a provider must expose to be dispatched.
 *
 * `modelChat` / `modelStream` are OPTIONAL: this is a structural contract that
 * third parties implement, so requiring the native surface would be a breaking
 * change. Providers that omit them are rejected by dispatchModelChat /
 * dispatchModelStream with UnsupportedFeatureError.
 */
export interface ProviderImpl {
  capabilities(): ProviderCapabilities
  chat(req: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse>
  stream(req: ChatRequest, opts?: ProviderRequestOptions): BoxStream
  modelChat?(req: ModelChatRequest, opts?: ProviderRequestOptions): Promise<ModelChatResponse>
  modelStream?(req: ModelChatRequest, opts?: ProviderRequestOptions): BoxModelStream
}
```

Append after `dispatchStream` (`provider.ts:159`):

```ts
/**
 * Validate a native model request against provider capabilities.
 *
 * Mirrors Rust `ProviderImpl::validate_model_request` (providers/mod.rs:82-140)
 * MINUS the thinking and MCP arms: milestone D3 omits those request fields
 * entirely, so a caller reaching for them gets a type error rather than a
 * runtime UnsupportedFeatureError. Throws BEFORE any HTTP call.
 */
export function validateModelRequest(req: ModelChatRequest, caps: ProviderCapabilities): void {
  const hasFreeformSpec = (req.toolSpecs ?? []).some((spec) => spec.kind === 'freeform')
  const hasFreeformHistory = req.context.some(
    (item) =>
      (item.kind === 'toolCall' && item.call.kind === 'freeform') ||
      (item.kind === 'toolOutput' && item.output.kind === 'custom'),
  )
  if ((hasFreeformSpec || hasFreeformHistory) && !caps.supportsFreeformTools) {
    throw new UnsupportedFeatureError('provider does not support native freeform tools')
  }

  for (const item of req.context) {
    if (item.kind !== 'message') continue
    const blocks: ContentBlock[] = item.message.contentBlocks ?? []
    for (const block of blocks) {
      if (block.type === 'image' && !caps.supportsImage) {
        throw new UnsupportedFeatureError('provider does not support image input')
      }
      if (block.type === 'document' && !caps.supportsDocument) {
        throw new UnsupportedFeatureError('provider does not support document input')
      }
    }
  }
}

/**
 * Dispatch a native model request: validate BEFORE any HTTP call, then
 * provider.modelChat (which owns its retry loop). Mirrors Rust
 * client.rs dispatch_model_chat.
 */
export async function dispatchModelChat(
  provider: ProviderImpl,
  req: ModelChatRequest,
  opts?: ProviderRequestOptions,
): Promise<ModelChatResponse> {
  validateModelRequest(req, provider.capabilities())
  if (!provider.modelChat) {
    throw new UnsupportedFeatureError('provider does not support native model requests')
  }
  return provider.modelChat(req, opts)
}

/**
 * Dispatch a native model stream: validate BEFORE connecting, then
 * provider.modelStream. Sync return, so a rejected request throws
 * synchronously rather than on first iteration.
 */
export function dispatchModelStream(
  provider: ProviderImpl,
  req: ModelChatRequest,
  opts?: ProviderRequestOptions,
): BoxModelStream {
  validateModelRequest(req, provider.capabilities())
  if (!provider.modelStream) {
    throw new UnsupportedFeatureError('provider does not support native model streams')
  }
  return provider.modelStream(req, opts)
}
```

Append after `readTimeoutStream` (`provider.ts:218`):

```ts
/**
 * Native-stream analogue of readTimeoutStream. THROWS StreamReadTimeoutError
 * when no delta arrives within the deadline; the deadline resets on each
 * yielded delta and the inner iterator is cancelled before throwing. Mirrors
 * Rust `ReadTimeoutModelStream` (client.rs:1239-1259).
 */
export async function* readTimeoutModelStream(
  inner: BoxModelStream,
  timeoutSecs: number,
): BoxModelStream {
  const timeoutMs = timeoutSecs * 1000
  const iterator = inner[Symbol.asyncIterator]()
  let closed = false

  async function closeInner(): Promise<void> {
    if (!closed) {
      closed = true
      await iterator.return?.()
    }
  }

  function cancelInnerWithoutWaiting(): void {
    if (!closed) {
      closed = true
      void iterator.return?.().catch(() => {})
    }
  }

  try {
    while (true) {
      let timer: ReturnType<typeof setTimeout> | undefined
      try {
        const timeout = new Promise<'__timeout__'>((resolve) => {
          timer = setTimeout(() => resolve('__timeout__'), timeoutMs)
        })
        const next = iterator.next()
        const raced = await Promise.race([next, timeout])
        if (timer !== undefined) {
          clearTimeout(timer)
          timer = undefined
        }

        if (raced === '__timeout__') {
          cancelInnerWithoutWaiting()
          throw new StreamReadTimeoutError(timeoutSecs)
        }

        const result = raced as IteratorResult<ModelStreamDelta>
        if (result.done) {
          closed = true
          return
        }
        yield result.value
      } finally {
        if (timer !== undefined) clearTimeout(timer)
      }
    }
  } finally {
    await closeInner()
  }
}
```

Update the four exact-shape assertions in `sdks/typescript/tests/capabilities.test.ts:14-46` to include the new field, and add the two new factories:

```ts
describe('ProviderCapabilities factories', () => {
  it('textOnly() returns all-false', () => {
    expect(textOnly()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
  })

  it('withImage() adds images only', () => {
    expect(withImage()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
  })

  it('fullCaps() keeps supportsFreeformTools FALSE (mirrors Rust full())', () => {
    expect(fullCaps()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
      supportsFreeformTools: false,
    })
  })

  it('minimaxCaps() returns text-only but MCP-capable', () => {
    expect(minimaxCaps()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: true,
      supportsFreeformTools: false,
    })
  })

  it('withFreeformTools() is text-only plus native freeform', () => {
    expect(withFreeformTools()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('withImageAndFreeformTools() adds images to native freeform', () => {
    expect(withImageAndFreeformTools()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })
})
```

and extend that file's import at `tests/capabilities.test.ts:1-12` with `withFreeformTools` and `withImageAndFreeformTools`.

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS. `tests/index.test.ts` still asserts the pre-flip capability shapes and will fail here — that is the D5 flip list; fix it in Task 10 or, if you prefer a green tree at every commit, apply the `index.test.ts` edit from Task 10 now.

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/provider.ts sdks/typescript/tests/capabilities.test.ts sdks/typescript/tests/providers-native-model.test.ts
git commit -m "feat: add freeform capability, native validation and native dispatch (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: ChatGPT Codex native surface

**Files:**
- Modify: `sdks/typescript/src/providers/chatgpt_codex.ts:11-28` (imports), `:105-107` (`capabilities`), `:219` (append `buildModelResponsesBody`), `:345` (append `modelChat` / `modelStream` / `modelStreamImpl`)
- Test: `sdks/typescript/tests/providers-native-model.test.ts` (append)

**Interfaces:**
- Consumes: `buildModelRequestBody`, `modelStreamAdapter` from `../serialize/responses.js`; `collectModelStream`, `BoxModelStream` from `../stream.js`; `withFreeformTools`, `validateModelRequest` from `../provider.js`.
- Produces: `ChatGptCodexProvider.buildModelResponsesBody(request: ModelChatRequest): Record<string, unknown>`; `ChatGptCodexProvider.modelChat(request, opts?): Promise<ModelChatResponse>`; `ChatGptCodexProvider.modelStream(request, opts?): BoxModelStream`.

Codex has no non-streaming endpoint: `modelChat` is `modelStream` + `collectModelStream`, then a model backfill. Its body hard-sets four fields **after** the `providerOptions` merge, so they beat the caller — including `tool_choice: 'auto'` regardless of `request.toolChoice`.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/providers-native-model.test.ts`:

```ts
import { afterEach, vi } from 'vitest'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { IncompleteStreamError } from '../src/error.js'
import { collectModelStream } from '../src/stream.js'

function sseFetch(sse: string, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(sse))
          controller.close()
        },
      }),
      { status: 200, headers: { 'content-type': 'text/event-stream' } },
    )
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

const FREEFORM_TOOL = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
} as const

describe('ChatGptCodexProvider native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('declares freeform yes / image no / document no', () => {
    expect(new ChatGptCodexProvider('tok', 'acct').capabilities()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
  })

  it('hard-sets the codex body fields, overriding the caller tool choice', () => {
    const body = new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      toolChoice: { type: 'required' },
    })
    expect(body.store).toBe(false)
    expect(body.stream).toBe(true)
    expect(body.include).toEqual(['reasoning.encrypted_content'])
    expect(body.parallel_tool_calls).toBe(true)
    expect(body.tool_choice).toBe('auto')
    expect(body.instructions).toBe('You are a helpful assistant.')
    expect(body.model).toBe('gpt-5.5')
  })

  it('normalizes a per-request reasoning effort and deletes the raw key', () => {
    const body = new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({
      context: [],
      providerOptions: { reasoning_effort: 'high' },
    })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect(body.reasoning_effort).toBeUndefined()
    expect('reasoning_effort' in body).toBe(false)
  })

  it('lets the per-request effort beat the provider default', () => {
    const body = new ChatGptCodexProvider('tok', 'acct')
      .reasoningEffort('low')
      .buildModelResponsesBody({ context: [], providerOptions: { reasoning_effort: 'high' } })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('uses the provider default when the request supplies none', () => {
    const body = new ChatGptCodexProvider('tok', 'acct')
      .reasoningEffort('high')
      .buildModelResponsesBody({ context: [] })
    expect(body.reasoning).toEqual({ effort: 'high', summary: 'auto' })
  })

  it('omits reasoning entirely when neither is set', () => {
    expect(
      new ChatGptCodexProvider('tok', 'acct').buildModelResponsesBody({ context: [] }).reasoning,
    ).toBeUndefined()
  })

  it('streams a freeform call and collects it', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'
    sseFetch(sse)

    const response = await collectModelStream(
      new ChatGptCodexProvider('tok', 'acct').modelStream({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
  })

  it('modelChat collects the native stream and backfills the model id', async () => {
    const sse =
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"text(\'captured\');"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":4,"output_tokens":5}}}\n\n'
    sseFetch(sse)

    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    })
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: "text('captured');" },
    ])
    expect(response.model).toBe('gpt-5.5')
  })

  it('maps response.incomplete to max_tokens', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n',
    )
    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'short' } }],
    })
    expect(response.content).toBe('partial')
    expect(response.stopReason).toBe('max_tokens')
    expect(response.usage.outputTokens).toBe(7)
  })

  it('sends the custom tool and the symmetric history byte-exact', async () => {
    const raw = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'
    let sent: Record<string, any> = {}
    sseFetch(
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":1,"output_tokens":1}}}\n\n',
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        { kind: 'toolCall', call: { kind: 'freeform', id: 'call_js', name: 'exec', input: raw } },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    })

    expect(sent.tools[0].type).toBe('custom')
    expect(sent.input.map((item: Record<string, unknown>) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    expect(sent.input[1].input).toBe(raw)
    expect(response.stopReason).toBe('end_turn')
  })

  it('throws IncompleteStreamError with the hyphenated provider token on truncation', async () => {
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n',
    )
    await expect(
      collectModelStream(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] })),
    ).rejects.toThrow('incomplete stream: chatgpt-codex ended without a terminal event')
    await expect(
      collectModelStream(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] })),
    ).rejects.toBeInstanceOf(IncompleteStreamError)
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/providers-native-model.test.ts`

Expected: FAIL with `TypeError: (intermediate value).buildModelResponsesBody is not a function`

- [ ] **Step 3: Implement**

Extend the imports at `sdks/typescript/src/providers/chatgpt_codex.ts:11-28`:

```ts
import { IncompleteStreamError, StreamError } from '../error.js'
import { postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_CHATGPT_CODEX_MODEL } from '../models.js'
import {
  validateModelRequest,
  withFreeformTools,
  type ProviderCapabilities,
  type ProviderRequestOptions,
} from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import { buildModelRequestBody, modelStreamAdapter } from '../serialize/responses.js'
import {
  collectModelStream,
  collectStream,
  doneWithStopReason,
  textEvent,
  thinkingDelta,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxModelStream,
  type BoxStream,
} from '../stream.js'
import type {
  ChatRequest,
  ChatResponse,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StopReason,
  StreamEvent,
  Usage,
} from '../types.js'
```

Replace `capabilities()` (`chatgpt_codex.ts:105-107`):

```ts
  /**
   * Text-only, but native Freeform-capable by default — Codex speaks the
   * Responses transport natively (milestone D5: freeform yes / image no /
   * document no).
   */
  capabilities(): ProviderCapabilities {
    return withFreeformTools()
  }
```

Append after `buildResponsesBody` (`chatgpt_codex.ts:219`):

```ts
  /**
   * Build the Responses body for a NATIVE model request. Public as a test seam,
   * matching buildResponsesBody above.
   *
   * The shared codec produces the body; Codex then hard-sets four fields
   * AFTER the codec's providerOptions merge, so they override the caller —
   * including `tool_choice: 'auto'` regardless of `request.toolChoice`.
   *
   * Reasoning effort resolves per-request providerOptions.reasoning_effort
   * FIRST, provider default second, omitted if neither. When one resolves the
   * body gets `reasoning = { effort, summary: 'auto' }` and the raw
   * `reasoning_effort` key is DELETED — the providerOptions shallow merge will
   * have injected it onto the body root, and it must never reach the wire.
   */
  buildModelResponsesBody(request: ModelChatRequest): Record<string, unknown> {
    const body = buildModelRequestBody(request, this.model, true, 'You are a helpful assistant.')
    body.store = false
    body.include = ['reasoning.encrypted_content']
    body.tool_choice = 'auto'
    body.parallel_tool_calls = true

    let effort: string | undefined
    const candidate = request.providerOptions?.reasoning_effort
    if (typeof candidate === 'string') effort = candidate
    if (effort === undefined) effort = this._reasoningEffort
    if (effort !== undefined) {
      body.reasoning = { effort, summary: 'auto' }
      delete body.reasoning_effort
    }

    return body
  }
```

Append after `streamImpl` (end of the class, `chatgpt_codex.ts:345`):

```ts
  /**
   * Native model chat. Codex has NO non-streaming endpoint, so this is
   * modelStream + collect, with the model id backfilled (mirrors Rust
   * ChatGptCodexProvider::model_chat).
   */
  async modelChat(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): Promise<ModelChatResponse> {
    const model = request.model ?? this.model
    const response = await collectModelStream(this.modelStream(request, opts))
    if (!response.model) response.model = model
    return response
  }

  /**
   * Native model stream. Validation runs SYNCHRONOUSLY here, before the
   * generator is created, so an unsupported request rejects before any network
   * I/O even if the caller never iterates.
   */
  modelStream(request: ModelChatRequest, opts?: ProviderRequestOptions): BoxModelStream {
    validateModelRequest(request, this.capabilities())
    return this.modelStreamImpl(request, opts)
  }

  private async *modelStreamImpl(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): AsyncGenerator<ModelStreamDelta> {
    const body = this.buildModelResponsesBody(request)

    // Retry ONLY the initial fetch; the token is re-resolved per attempt.
    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, async () =>
          postStream(this.baseUrl, this.headers(await this.resolveToken()), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    // Provider token is the HYPHENATED `chatgpt-codex` on the native path
    // (specs/types.md § Stream termination (native)). The legacy chat adapter
    // above keeps its `chatgpt_codex` token unchanged.
    yield* modelStreamAdapter(responseBody, 'chatgpt-codex')
  }
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/providers/chatgpt_codex.ts sdks/typescript/tests/providers-native-model.test.ts
git commit -m "feat: add the native model surface to the ChatGPT Codex provider (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: OpenAI Responses opt-in and native surface

**Files:**
- Modify: `sdks/typescript/src/providers/openai.ts:1-18` (imports), `:45` (add `responsesApi` field), `:79` (add `withResponsesApi`), `:321-323` (`capabilities`), `:471` (append `modelChat` / `modelStream` / `modelStreamImpl`)
- Test: `sdks/typescript/tests/providers-native-model.test.ts` (append)

**Interfaces:**
- Consumes: `buildModelRequestBody`, `modelChatResponseFromOutput`, `modelStreamAdapter` from `../serialize/responses.js`; `withImageAndFreeformTools`, `validateModelRequest` from `../provider.js`.
- Produces: `OpenAIProvider.withResponsesApi(enabled: boolean): this`; `OpenAIProvider.modelChat(request, opts?): Promise<ModelChatResponse>`; `OpenAIProvider.modelStream(request, opts?): BoxModelStream`.

`withResponsesApi` and `withResponsesFallback` are **semantically different and must stay distinguishable**: `withResponsesFallback` is a 404-recovery path for the legacy `chat()` surface; `withResponsesApi` is the native opt-in that unlocks `modelChat`/`modelStream` and flips `supportsFreeformTools`. Neither implies the other.

Order inside `modelChat`/`modelStream`: `validateModelRequest` **first**, the `responsesApi` gate second — so a freeform request against a non-opted-in provider fails with `provider does not support native freeform tools` (which is what the Rust tests assert on), not with the endpoint message.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/providers-native-model.test.ts`:

```ts
import { DEFAULT_OPENAI_RESPONSES_URL, OpenAIProvider } from '../src/providers/openai.js'

function jsonFetch(body: unknown, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

describe('OpenAIProvider native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('keeps withResponsesApi and withResponsesFallback independent', () => {
    const provider = new OpenAIProvider('k', 'gpt-5.5-codex')
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
    provider.withResponsesFallback(true)
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
    provider.withResponsesApi(true)
    expect(provider.capabilities()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
    provider.withResponsesApi(false)
    expect(provider.capabilities().supportsFreeformTools).toBe(false)
  })

  it('POSTs the Responses endpoint and decodes a freeform call (non-streaming)', async () => {
    const raw = 'const x = {a: 1};\nconsole.log(x.a);\n'
    let sent: Record<string, any> = {}
    const mockFetch = jsonFetch(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'custom_tool_call', call_id: 'call_js', name: 'exec', input: raw }],
        usage: { input_tokens: 9, output_tokens: 7 },
      },
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new OpenAIProvider('test-key', 'gpt-5.5-codex')
      .withResponsesApi(true)
      .modelChat({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      })

    expect(String(mockFetch.mock.calls[0][0])).toBe(DEFAULT_OPENAI_RESPONSES_URL)
    expect(sent.stream).toBeUndefined()
    expect(sent.tools[0]).toEqual({
      type: 'custom',
      name: 'exec',
      description: 'Run JavaScript',
      format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
    })
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: raw },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.inputTokens).toBe(9)
  })

  it('encodes image content blocks as input_image data URLs', async () => {
    let sent: Record<string, any> = {}
    jsonFetch(
      {
        model: 'gpt-5.5-codex',
        status: 'completed',
        output: [{ type: 'message', content: [{ type: 'output_text', text: 'ok' }] }],
        usage: { input_tokens: 1, output_tokens: 1 },
      },
      (_url, init) => {
        sent = JSON.parse(String(init?.body))
      },
    )

    const response = await new OpenAIProvider('test-key', 'gpt-5.5-codex')
      .withResponsesApi(true)
      .modelChat({
        context: [
          {
            kind: 'message',
            message: {
              role: 'user',
              content: 'inspect',
              contentBlocks: [
                { type: 'text', text: 'inspect' },
                {
                  type: 'image',
                  source: { type: 'base64', mediaType: 'image/png', data: 'abc123' },
                },
              ],
            },
          },
        ],
      })

    expect(sent.input[0].content).toEqual([
      { type: 'input_text', text: 'inspect' },
      { type: 'input_image', image_url: 'data:image/png;base64,abc123' },
    ])
    expect(response.content).toBe('ok')
  })

  it('rejects native freeform BEFORE any HTTP call when the opt-in is off', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    const request: ModelChatRequest = {
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
    }

    await expect(provider.modelChat(request)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
    expect(() => provider.modelStream(request)).toThrow(
      'provider does not support native freeform tools',
    )
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('reports the endpoint message for a non-freeform request with the opt-in off', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    await expect(provider.modelChat({ context: [] })).rejects.toThrow(
      'OpenAI Chat Completions does not support native model requests; enable OpenAI Responses API',
    )
    expect(() => provider.modelStream({ context: [] })).toThrow(
      'OpenAI Chat Completions does not support native model streams; enable OpenAI Responses API',
    )
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('streams native custom deltas and collects them', async () => {
    const sse =
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);\\n"}\n\n' +
      'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"console.log(1);\\n"}}\n\n' +
      'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n'
    sseFetch(sse)

    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'gpt-5.5-codex').withResponsesApi(true).modelStream({
        context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'console.log(1);\n' },
    ])
    expect(response.usage.outputTokens).toBe(3)
  })

  it('throws IncompleteStreamError with the openai payload on truncation', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"lo"}\n\n',
    )
    await expect(
      collectModelStream(
        new OpenAIProvider('test-key', 'gpt-5.5-codex')
          .withResponsesApi(true)
          .modelStream({ context: [] }),
      ),
    ).rejects.toThrow('incomplete stream: openai ended without a terminal event')
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/providers-native-model.test.ts`

Expected: FAIL with `TypeError: (intermediate value).withResponsesApi is not a function`

- [ ] **Step 3: Implement**

Extend the imports at `sdks/typescript/src/providers/openai.ts:1-18`:

```ts
import { IncompleteStreamError, UnsupportedFeatureError } from '../error.js'
import { postJson, postStream } from '../http/fetch.js'
import { parseSse } from '../http/sse.js'
import { DEFAULT_OPENAI_MODEL } from '../models.js'
import {
  validateModelRequest,
  withImage,
  withImageAndFreeformTools,
  type ProviderCapabilities,
  type ProviderRequestOptions,
} from '../provider.js'
import { attemptWithCancellation, classifyForRetry, RetryPolicy, withRetry } from '../retry.js'
import { serializeOpenAiRequest } from '../serialize/openai.js'
import {
  buildModelRequestBody,
  modelChatResponseFromOutput,
  modelStreamAdapter,
} from '../serialize/responses.js'
import {
  doneEvent,
  doneWithStopReason,
  textEvent,
  toolCallArgsWithId,
  toolCallEndWithId,
  toolCallStart,
  usageEvent,
  type BoxModelStream,
  type BoxStream,
} from '../stream.js'
import type {
  ChatRequest,
  ChatResponse,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StopReason,
  ToolCall,
} from '../types.js'
```

Add the field beside `responsesFallback` (`openai.ts:45`):

```ts
  private responsesFallback = false
  private responsesApi = false
```

Add the setter beside `withResponsesFallback` (`openai.ts:79`):

```ts
  /**
   * 404-recovery for the LEGACY chat surface: when /v1/chat/completions
   * answers 404, retry the request against the Responses endpoint. Unrelated
   * to withResponsesApi below.
   */
  withResponsesFallback(enabled: boolean): this {
    this.responsesFallback = enabled
    return this
  }

  /**
   * Opt into the OpenAI Responses API for the NATIVE model surface
   * (modelChat / modelStream), and flip supportsFreeformTools on.
   *
   * Deliberately distinct from withResponsesFallback: that is a 404 recovery
   * path for chat(), this is the native opt-in. Neither implies the other.
   * Mirrors Rust `OpenAIProvider::with_responses_api`.
   */
  withResponsesApi(enabled: boolean): this {
    this.responsesApi = enabled
    return this
  }
```

Replace `capabilities()` (`openai.ts:321-323`):

```ts
  capabilities(): ProviderCapabilities {
    return this.responsesApi ? withImageAndFreeformTools() : withImage()
  }
```

Append at the end of the class (`openai.ts:471`):

```ts
  /**
   * Native, genuinely NON-STREAMING model request: one POST to the Responses
   * endpoint, decoded by the shared codec. (ChatGPT Codex has no such
   * endpoint and reaches the same shape via stream + collect.)
   *
   * Validation runs FIRST so a freeform request against a non-opted-in
   * provider reports the freeform capability failure, not the endpoint one.
   */
  async modelChat(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): Promise<ModelChatResponse> {
    validateModelRequest(request, this.capabilities())
    if (!this.responsesApi) {
      throw new UnsupportedFeatureError(
        'OpenAI Chat Completions does not support native model requests; enable OpenAI Responses API',
      )
    }

    const body = buildModelRequestBody(request, this.model, false)
    const payload = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postJson<unknown>(this.responsesUrl, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    return modelChatResponseFromOutput(payload, this.model)
  }

  /**
   * Native model stream. Validation and the opt-in gate run SYNCHRONOUSLY,
   * before the generator is created, so an unsupported request rejects before
   * any network I/O even if the caller never iterates.
   */
  modelStream(request: ModelChatRequest, opts?: ProviderRequestOptions): BoxModelStream {
    validateModelRequest(request, this.capabilities())
    if (!this.responsesApi) {
      throw new UnsupportedFeatureError(
        'OpenAI Chat Completions does not support native model streams; enable OpenAI Responses API',
      )
    }
    return this.modelStreamImpl(request, opts)
  }

  private async *modelStreamImpl(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): AsyncGenerator<ModelStreamDelta> {
    const body = buildModelRequestBody(request, this.model, true)

    const responseBody = await withRetry(
      this.retryPolicy,
      async () =>
        attemptWithCancellation(opts?.callerSignal, () =>
          postStream(this.responsesUrl, this.headers(), body, {
            signal: opts?.signal,
            preHeadersTimeoutMs: opts?.preHeadersTimeoutMs,
          }),
        ),
      classifyForRetry,
    )

    yield* modelStreamAdapter(responseBody, 'openai')
  }
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/providers/openai.ts sdks/typescript/tests/providers-native-model.test.ts
git commit -m "feat: add the OpenAI Responses opt-in and native model surface (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: Client, ClientBuilder, and the `asDispatchProvider` shim

**Files:**
- Modify: `sdks/typescript/src/client.ts:1-22` (imports), `:39-43` (`ProviderLike`), `:70-80` (`asDispatchProvider`), `:97` (builder field), `:179` (builder setter), `:245-259` (openai build arm), `:491` (append `Client` methods)
- Test: `sdks/typescript/tests/providers-native-model.test.ts` (append)

**Interfaces:**
- Consumes: `dispatchModelChat`, `dispatchModelStream`, `readTimeoutModelStream` from `./provider.js`; `collectModelStream`, `BoxModelStream` from `./stream.js`.
- Produces: `ProviderLike.modelChat?` / `.modelStream?`; `ClientBuilder.openaiResponsesApi(enabled: boolean): this`; `Client.modelChat(request, opts?): Promise<ModelChatResponse>`; `Client.modelStream(request, opts?): BoxModelStream`; `Client.modelStreamCollect(request, opts?): Promise<ModelChatResponse>`.

**The `asDispatchProvider` trap.** `client.ts:70-80` has two branches. When `provider.capabilities` exists the object is cast through and the native methods survive. When it does **not**, the function rebuilds a plain object from three properties — and `modelChat`/`modelStream` are silently dropped. Both branches need a test.

`Client.modelStream` applies `readTimeoutModelStream` but **not** `stripThink`: native thinking arrives as typed `thinking_delta`/`thinking_done` deltas, never as inline `<think>` text. This mirrors Rust `dispatch_model_stream`, which wraps only `ReadTimeoutModelStream`.

- [ ] **Step 1: Write the failing test**

Append to `sdks/typescript/tests/providers-native-model.test.ts`:

```ts
import { Client, ClientBuilder } from '../src/client.js'
import type { ProviderLike } from '../src/client.js'
import type { ModelChatResponse } from '../src/types.js'

describe('Client native surface', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('modelChat dispatches through the built provider', async () => {
    jsonFetch({
      model: 'gpt-5.5-codex',
      status: 'completed',
      output: [{ type: 'message', content: [{ type: 'output_text', text: 'ok' }] }],
      usage: { input_tokens: 1, output_tokens: 1 },
    })

    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .model('gpt-5.5-codex')
      .openaiResponsesApi(true)
      .build()

    const response = await client.modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
    })
    expect(response.content).toBe('ok')
  })

  it('openaiResponsesApi(true) flips the built provider capabilities', () => {
    const provider = new ClientBuilder()
      .provider('openai')
      .apiKey('k')
      .openaiResponsesApi(true)
      .buildProviderForTest()
    expect(provider.capabilities().supportsFreeformTools).toBe(true)
  })

  it('defaults the responses opt-in to off, independent of the fallback flag', () => {
    const plain = new ClientBuilder().provider('openai').apiKey('k').buildProviderForTest()
    expect(plain.capabilities().supportsFreeformTools).toBe(false)

    const fallbackOnly = new ClientBuilder()
      .provider('openai')
      .apiKey('k')
      .openaiResponsesFallback(true)
      .buildProviderForTest()
    expect(fallbackOnly.capabilities().supportsFreeformTools).toBe(false)
  })

  it('modelStream rejects synchronously before any HTTP when unsupported', () => {
    const mockFetch = jsonFetch({})
    const client = Client.builder().provider('openai').apiKey('test-key').build()
    expect(() =>
      client.modelStream({
        context: [],
        toolSpecs: [{ kind: 'freeform', tool: FREEFORM_TOOL }],
      }),
    ).toThrow('provider does not support native freeform tools')
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('modelStreamCollect backfills the model from the request', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hi"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .openaiResponsesApi(true)
      .build()

    const response = await client.modelStreamCollect({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      model: 'gpt-5.5-codex',
    })
    expect(response.content).toBe('hi')
    expect(response.model).toBe('gpt-5.5-codex')
  })

  it('applies the read-idle timeout to native streams', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(
        async () =>
          new Response(
            new ReadableStream<Uint8Array>({
              start(controller) {
                controller.enqueue(
                  new TextEncoder().encode(
                    'data: {"type":"response.output_text.delta","delta":"tick"}\n\n',
                  ),
                )
                // never closed — the stream stalls
              },
            }),
            { status: 200, headers: { 'content-type': 'text/event-stream' } },
          ),
      ),
    )

    const client = Client.builder()
      .provider('openai')
      .apiKey('test-key')
      .openaiResponsesApi(true)
      .timeouts({ readIdleMs: 50 })
      .build()

    const seen: ModelStreamDelta[] = []
    let caught: unknown
    try {
      for await (const delta of client.modelStream({ context: [] })) seen.push(delta)
    } catch (error) {
      caught = error
    }
    expect(caught).toBeInstanceOf(StreamReadTimeoutError)
    expect(seen).toEqual([{ type: 'text', delta: 'tick' }])
  })
})

describe('asDispatchProvider forwards the native surface', () => {
  const nativeResponse: ModelChatResponse = {
    content: 'shimmed',
    toolCalls: [],
    model: 'm',
    usage: { inputTokens: 0, outputTokens: 0 },
    stopReason: 'end_turn',
  }

  it('keeps modelChat/modelStream when the provider has no capabilities()', async () => {
    // No capabilities() -> asDispatchProvider takes the REBUILD branch, which
    // historically dropped every property it did not name.
    const bare: ProviderLike = {
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
      modelChat: async () => nativeResponse,
      modelStream: () => deltas([{ type: 'done', stopReason: 'end_turn' }]),
    }

    const client = new Client(bare)
    expect((await client.modelChat({ context: [] })).content).toBe('shimmed')

    const seen: ModelStreamDelta[] = []
    for await (const delta of client.modelStream({ context: [] })) seen.push(delta)
    expect(seen).toEqual([{ type: 'done', stopReason: 'end_turn' }])
  })

  it('still rejects when a shimmed provider omits the native surface', async () => {
    const bare: ProviderLike = {
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
    }
    const client = new Client(bare)
    await expect(client.modelChat({ context: [] })).rejects.toThrow(
      'provider does not support native model requests',
    )
    expect(() => client.modelStream({ context: [] })).toThrow(
      'provider does not support native model streams',
    )
  })

  it('keeps the native surface on a provider that DOES expose capabilities()', async () => {
    const full: ProviderLike = {
      capabilities: () => withFreeformTools(),
      chat: async () => {
        throw new Error('unused')
      },
      stream: () => {
        throw new Error('unused')
      },
      modelChat: async () => nativeResponse,
    }
    const client = new Client(full)
    expect((await client.modelChat({ context: [] })).content).toBe('shimmed')
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/providers-native-model.test.ts`

Expected: FAIL with `TypeError: Client.builder(...).provider(...).apiKey(...).model(...).openaiResponsesApi is not a function`

- [ ] **Step 3: Implement**

Extend the imports at `sdks/typescript/src/client.ts:1-22`:

```ts
import { CancelledError, ConfigError } from './error.js'
import {
  dispatchChat,
  dispatchModelChat,
  dispatchModelStream,
  dispatchStream,
  readTimeoutModelStream,
  readTimeoutStream,
  textOnly,
  type Provider,
  type ProviderImpl as DispatchProvider,
  type ProviderRequestOptions,
  type RequestOptions,
} from './provider.js'
import { RetryPolicy } from './retry.js'
import { collectModelStream, collectStream, type BoxModelStream, type BoxStream } from './stream.js'
import { stripThink } from './think_stripper.js'
import { AnthropicProvider } from './providers/anthropic.js'
import { MinimaxProvider } from './providers/minimax.js'
import { OllamaProvider } from './providers/ollama.js'
import { OpenAIProvider, type OpenAIAuthStyle } from './providers/openai.js'
import { GeminiProvider } from './providers/gemini.js'
import { ChatGptCodexProvider, type TokenSource } from './providers/chatgpt_codex.js'
import { DEFAULT_OLLAMA_MODEL } from './models.js'
import type {
  ChatRequest,
  ChatResponse,
  ModelChatRequest,
  ModelChatResponse,
  ModelStreamDelta,
  StreamEvent,
} from './types.js'
```

Replace `ProviderLike` (`client.ts:39-43`):

```ts
export interface ProviderLike {
  capabilities?(): ReturnType<DispatchProvider['capabilities']>
  chat(request: ChatRequest, opts?: ProviderRequestOptions): Promise<ChatResponse>
  stream(request: ChatRequest, opts?: ProviderRequestOptions): AsyncIterable<StreamEvent>
  /** Optional native surface. Forwarded by asDispatchProvider — see the note there. */
  modelChat?(request: ModelChatRequest, opts?: ProviderRequestOptions): Promise<ModelChatResponse>
  modelStream?(
    request: ModelChatRequest,
    opts?: ProviderRequestOptions,
  ): AsyncIterable<ModelStreamDelta>
}
```

Replace `asDispatchProvider` (`client.ts:70-80`):

```ts
/**
 * Adapt a caller-supplied ProviderLike to the dispatch contract.
 *
 * The rebuild branch below names every property it forwards, so ANY new member
 * of ProviderImpl must be added here or it is silently dropped for providers
 * that do not implement capabilities(). The native model methods are forwarded
 * only when present, keeping them optional end to end.
 */
function asDispatchProvider(provider: ProviderLike): DispatchProvider {
  if (provider.capabilities) {
    return provider as DispatchProvider
  }

  const shim: DispatchProvider = {
    capabilities: textOnly,
    chat: (request: ChatRequest, opts?: ProviderRequestOptions) => provider.chat(request, opts),
    stream: (request: ChatRequest, opts?: ProviderRequestOptions) => provider.stream(request, opts),
  }

  const { modelChat, modelStream } = provider
  if (modelChat) {
    shim.modelChat = (request: ModelChatRequest, opts?: ProviderRequestOptions) =>
      modelChat.call(provider, request, opts)
  }
  if (modelStream) {
    shim.modelStream = (request: ModelChatRequest, opts?: ProviderRequestOptions) =>
      modelStream.call(provider, request, opts)
  }
  return shim
}
```

Add the builder field beside `_openaiResponsesFallback` (`client.ts:97`):

```ts
  protected _openaiResponsesFallback = false
  protected _openaiResponsesApi = false
```

Add the builder setter beside `openaiResponsesFallback` (`client.ts:179`):

```ts
  /** 404-recovery for the legacy chat surface. NOT the native opt-in. */
  openaiResponsesFallback(enabled: boolean): this {
    this._openaiResponsesFallback = enabled
    return this
  }

  /**
   * Opt into the OpenAI Responses API for the NATIVE model surface
   * (Client.modelChat / modelStream / modelStreamCollect), flipping
   * supportsFreeformTools on. Mirrors Rust
   * `ClientBuilder::openai_responses_api`. Independent of
   * openaiResponsesFallback above.
   */
  openaiResponsesApi(enabled: boolean): this {
    this._openaiResponsesApi = enabled
    return this
  }
```

Replace the openai arm of `buildProvider` (`client.ts:245-259`):

```ts
    if (provider === 'openai') {
      let openai = new OpenAIProvider(apiKey, this._model)
        .withRetryPolicy(this._retryPolicy)
        .withAuthStyle(this._openaiAuthStyle)
        .withResponsesApi(this._openaiResponsesApi)
      if (this._openaiChatUrl) {
        openai = openai.withChatUrl(this._openaiChatUrl)
      }
      if (this._openaiResponsesUrl) {
        openai = openai.withResponsesUrl(this._openaiResponsesUrl)
      }
      if (this._openaiResponsesFallback) {
        openai = openai.withResponsesFallback(this._openaiResponsesFallback)
      }
      return openai
    }
```

Append the three native methods to `Client` (`client.ts:491`, after `streamCollectWith`):

```ts
  /**
   * Send a NATIVE model request; validates capabilities BEFORE any HTTP call.
   * Signal composition matches chat(): the caller signal plus the opt-in
   * totalMs budget; the connect budget rides preHeadersTimeoutMs.
   */
  async modelChat(request: ModelChatRequest, opts?: RequestOptions): Promise<ModelChatResponse> {
    let signal = opts?.signal
    if (this.totalMs !== undefined) {
      const total = AbortSignal.timeout(this.totalMs)
      signal = signal ? AbortSignal.any([signal, total]) : total
    }
    try {
      return await dispatchModelChat(this.provider, request, {
        signal,
        callerSignal: opts?.signal,
        preHeadersTimeoutMs: this.connectMs + this.readIdleMs,
      })
    } catch (error) {
      if (opts?.signal?.aborted && !(error instanceof CancelledError)) {
        throw new CancelledError()
      }
      throw error
    }
  }

  /**
   * Stream a NATIVE model request: dispatch (validate -> provider.modelStream)
   * -> readTimeoutModelStream -> caller-abort translation.
   *
   * NO stripThink: native thinking arrives as typed thinking_delta /
   * thinking_done deltas, never as inline <think> text. Streams get ONLY the
   * caller signal — totalMs never applies to stream body consumption.
   */
  modelStream(request: ModelChatRequest, opts?: RequestOptions): BoxModelStream {
    let stream: BoxModelStream = dispatchModelStream(this.provider, request, {
      signal: opts?.signal,
      callerSignal: opts?.signal,
      preHeadersTimeoutMs: this.connectMs + this.readIdleMs,
    })
    stream = readTimeoutModelStream(stream, this.readIdleMs / 1000)
    return this.translateModelCallerAbort(stream, opts?.signal)
  }

  /** Mid-stream caller abort surfaces as CancelledError on the native path too. */
  private async *translateModelCallerAbort(
    inner: BoxModelStream,
    callerSignal: AbortSignal | undefined,
  ): BoxModelStream {
    try {
      for await (const delta of inner) yield delta
    } catch (error) {
      if (callerSignal?.aborted && !(error instanceof CancelledError)) {
        throw new CancelledError()
      }
      throw error
    }
  }

  /**
   * Stream a native request and collect it, preferring the request's model
   * override in the result (collectModelStream returns an empty model).
   */
  async modelStreamCollect(
    request: ModelChatRequest,
    opts?: RequestOptions,
  ): Promise<ModelChatResponse> {
    const modelHint = request.model ?? ''
    const response = await collectModelStream(this.modelStream(request, opts))
    if (!response.model) response.model = modelHint
    return response
  }
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS (except `tests/index.test.ts`, still on the pre-flip capability shapes — Task 10 fixes it).

- [ ] **Step 5: Commit**

```bash
git add sdks/typescript/src/client.ts sdks/typescript/tests/providers-native-model.test.ts
git commit -m "feat: expose the native model surface on Client and ClientBuilder (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: Public export surface (closes PR T2)

**Files:**
- Modify: `sdks/typescript/src/index.ts:33-38`
- Test: `sdks/typescript/tests/index.test.ts:11-50` (capability flips + new symbols)

**Interfaces:**
- Consumes: everything produced by Tasks 1–9.
- Produces: root exports `withFreeformTools`, `withImageAndFreeformTools`, `validateModelRequest` (values); `collectModelStream` and `BoxModelStream` arrive for free through the existing `export * from './stream.js'`; the native types arrive through the existing `export * from './types.js'`.

- [ ] **Step 1: Write the failing test**

Replace the first `it` block of `sdks/typescript/tests/index.test.ts:12-50` and append a native block:

```ts
  it('re-exports M3 public symbols from the package entrypoint', async () => {
    const mod = await import('../src/index.js')

    expect(typeof mod.RetryPolicy).toBe('function')

    expect(typeof mod.textOnly).toBe('function')
    expect(typeof mod.withImage).toBe('function')
    expect(typeof mod.fullCaps).toBe('function')
    const caps: ProviderCapabilities = mod.textOnly()
    expect(caps).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
    expect(mod.withImage()).toEqual({
      supportsImage: true,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: false,
    })
    expect(mod.fullCaps()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
      supportsFreeformTools: false,
    })

    expect(typeof mod.DEFAULT_ANTHROPIC_MODEL).toBe('string')
    expect(typeof mod.DEFAULT_OPENAI_MODEL).toBe('string')
    expect(typeof mod.DEFAULT_MINIMAX_MODEL).toBe('string')
    expect(Array.isArray(mod.ANTHROPIC_MODELS)).toBe(true)
    expect(Array.isArray(mod.OPENAI_MODELS)).toBe(true)
    expect(Array.isArray(mod.MINIMAX_MODELS)).toBe(true)

    expect(typeof mod.ThinkStripper).toBe('function')
    expect(typeof mod.stripThink).toBe('function')
    expect(typeof mod.ClientBuilder).toBe('function')

    const provider: Provider = 'anthropic'
    expect(provider).toBe('anthropic')
  })
```

and append, at the end of `tests/index.test.ts`:

```ts
describe('native model API public surface', () => {
  it('re-exports the native capability factories and validator', async () => {
    const mod = await import('../src/index.js')
    expect(typeof mod.withFreeformTools).toBe('function')
    expect(typeof mod.withImageAndFreeformTools).toBe('function')
    expect(typeof mod.validateModelRequest).toBe('function')
    expect(mod.withFreeformTools()).toEqual({
      supportsImage: false,
      supportsDocument: false,
      supportsMcp: false,
      supportsFreeformTools: true,
    })
    expect(mod.withImageAndFreeformTools().supportsImage).toBe(true)
  })

  it('re-exports collectModelStream from the root', async () => {
    const mod = await import('../src/index.js')
    expect(typeof mod.collectModelStream).toBe('function')
  })

  it('accepts the native types from the root entrypoint', async () => {
    const mod = await import('../src/index.js')
    const request: import('../src/index.js').ModelChatRequest = {
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      toolSpecs: [
        {
          kind: 'freeform',
          tool: {
            name: 'exec',
            description: 'Run JavaScript',
            format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
          },
        },
      ],
    }
    expect(() => mod.validateModelRequest(request, mod.withFreeformTools())).not.toThrow()
    expect(() => mod.validateModelRequest(request, mod.withImage())).toThrow(
      'provider does not support native freeform tools',
    )
  })

  it('exposes the native methods on Client and ClientBuilder', async () => {
    const mod = await import('../src/index.js')
    expect(typeof mod.Client.prototype.modelChat).toBe('function')
    expect(typeof mod.Client.prototype.modelStream).toBe('function')
    expect(typeof mod.Client.prototype.modelStreamCollect).toBe('function')
    expect(typeof mod.ClientBuilder.prototype.openaiResponsesApi).toBe('function')
  })
})
```

- [ ] **Step 2: Run it to verify it fails**

Run: `cd sdks/typescript && npm run test -- tests/index.test.ts`

Expected: FAIL with `expected undefined to be 'function'` for `mod.withFreeformTools` (`provider.ts` is not star-exported from `index.ts`, so new value exports must be listed by hand).

- [ ] **Step 3: Implement**

Replace `sdks/typescript/src/index.ts:33-38`:

```ts
export type { ProviderCapabilities, Provider, RequestOptions, ProviderRequestOptions } from './provider.js'
export {
  textOnly,
  withImage,
  fullCaps,
  minimaxCaps,
  withFreeformTools,
  withImageAndFreeformTools,
  validateRequest,
  validateModelRequest,
} from './provider.js'

// M4: server-side MCP types (also covered by `export * from './types.js'`; listed
// explicitly for discoverability). No internal http/serialize symbols are exported.
export type { McpServerType, McpServerConfig, McpToolConfig } from './types.js'

// Native model API (specs/types.md § Native Model API). Also covered by
// `export * from './types.js'`; listed explicitly for discoverability.
// `collectModelStream` / `BoxModelStream` ride `export * from './stream.js'`.
// The Responses codec itself stays internal, exactly like serialize/openai.ts
// and serialize/anthropic.ts — `buildModelRequestBody` is module-public in
// src/serialize/responses.ts as a test seam only.
export type {
  FreeformTool,
  FreeformToolFormat,
  FunctionCallOutputContentItem,
  FunctionCallOutputPayload,
  ImageDetail,
  ModelChatRequest,
  ModelChatResponse,
  ModelContextItem,
  ModelStreamDelta,
  ModelToolCall,
  ModelToolOutput,
  ModelToolSpec,
} from './types.js'
```

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS (whole suite green — the D5 capability flips in `capabilities.test.ts` and `index.test.ts` are both applied by now).

- [ ] **Step 5: Commit, push, verify, open PR T2**

```bash
git add sdks/typescript/src/index.ts sdks/typescript/tests/index.test.ts
git commit -m "feat: export the native model API from the package root (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

(cd sdks/python && uv sync --all-extras)
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/ts-native-model-providers
test "$(git ls-remote origin refs/heads/feat/ts-native-model-providers | cut -f1)" = "$(git rev-parse HEAD)"

gh pr create --base main --head feat/ts-native-model-providers \
  --title "feat: TypeScript native model providers, dispatch and client surface (#270)" \
  --body "Task group T2 of the Freeform parity milestone (#270). Wires the T1 codec into ChatGPT Codex (native by default) and OpenAI (behind \`withResponsesApi\` / \`openaiResponsesApi\`), adds \`supportsFreeformTools\`, native validation and dispatch, \`collectModelStream\`, the \`readTimeoutModelStream\` wrapper, \`Client.modelChat\`/\`modelStream\`/\`modelStreamCollect\`, and the \`asDispatchProvider\` forwarding fix.

Capability-shape assertions in \`tests/capabilities.test.ts\` and \`tests/index.test.ts\` flip to include \`supportsFreeformTools\` — expected per milestone D5, not regressions.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

---

### Task 11: TypeScript Freeform conformance suite (PR C-TS)

> **Format deviation, deliberate.** This suite gates behaviour that T1/T2 already shipped, so it passes the moment it is written. Steps 2 and 3 replace "write a failing test" with "run it, then prove it is not vacuous by mutating the source and watching it fail". Everything else is unchanged.

**Files:**
- Create: `sdks/typescript/tests/freeform-conformance.test.ts`
- Test: itself

**Interfaces:**
- Consumes: `Client`, `ClientBuilder` from `../src/client.js`; `ChatGptCodexProvider`; `OpenAIProvider`; `collectModelStream` from `../src/stream.js`; `buildModelRequestBody` from `../src/serialize/responses.js`; `IncompleteStreamError`, `StreamError`, `UnsupportedFeatureError` from `../src/error.js`.
- Produces: nothing importable — a gate.

- [ ] **Step 1: Write the conformance suite**

Create `sdks/typescript/tests/freeform-conformance.test.ts`:

```ts
import { afterEach, describe, expect, it, vi } from 'vitest'
import { Client } from '../src/client.js'
import { IncompleteStreamError, StreamError, UnsupportedFeatureError } from '../src/error.js'
import { ChatGptCodexProvider } from '../src/providers/chatgpt_codex.js'
import { OpenAIProvider } from '../src/providers/openai.js'
import { buildModelRequestBody } from '../src/serialize/responses.js'
import { collectModelStream } from '../src/stream.js'
import type { FreeformTool, ModelChatRequest, ModelStreamDelta } from '../src/types.js'

// Freeform parity conformance gates (specs/types.md § Native Model API).
// Cross-SDK mirrors:
// - sdks/rust/tests/freeform_conformance.rs
// - sdks/python/tests/test_freeform_conformance.py
//
// Expected values are taken from the Rust suite that already pins this
// behaviour (tests/core_types.rs, tests/openai_provider.rs,
// tests/chatgpt_codex.rs, tests/native_collect_stream.rs). Do not invent new
// fixtures where one exists.

const EXEC_TOOL: FreeformTool = {
  name: 'exec',
  description: 'Run JavaScript',
  format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
}

/** The Rust fixture for "looks like JSON but is JavaScript". */
const JS_THAT_LOOKS_LIKE_JSON = '{"this":"looks like json"}\nconsole.log(\'but is JS\');'

function sseFetch(sse: string, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(
      new ReadableStream<Uint8Array>({
        start(controller) {
          controller.enqueue(new TextEncoder().encode(sse))
          controller.close()
        },
      }),
      { status: 200, headers: { 'content-type': 'text/event-stream' } },
    )
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

function jsonFetch(body: unknown, onRequest?: (url: string, init?: RequestInit) => void) {
  const mockFetch = vi.fn(async (url: string, init?: RequestInit) => {
    onRequest?.(url, init)
    return new Response(JSON.stringify(body), {
      status: 200,
      headers: { 'content-type': 'application/json' },
    })
  })
  vi.stubGlobal('fetch', mockFetch)
  return mockFetch
}

async function drain(stream: AsyncIterable<ModelStreamDelta>) {
  const deltas: ModelStreamDelta[] = []
  let error: unknown
  try {
    for await (const delta of stream) deltas.push(delta)
  } catch (caught) {
    error = caught
  }
  return { deltas, error }
}

describe('Freeform conformance — tool definitions', () => {
  it('a freeform tool serializes with a mandatory, exact format object', () => {
    const body = buildModelRequestBody(
      { context: [], toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }] },
      'm',
      false,
    )
    expect(body.tools).toEqual([
      {
        type: 'custom',
        name: 'exec',
        description: 'Run JavaScript',
        format: { type: 'grammar', syntax: 'lark', definition: 'start: source' },
      },
    ])
  })

  it('a function tool serializes inputSchema under the wire key `parameters`', () => {
    const body = buildModelRequestBody(
      {
        context: [],
        toolSpecs: [
          {
            kind: 'function',
            tool: {
              name: 'get_weather',
              description: 'Fetch the weather',
              inputSchema: { type: 'object' },
            },
          },
        ],
      },
      'm',
      false,
    )
    expect(body.tools).toEqual([
      {
        type: 'function',
        name: 'get_weather',
        description: 'Fetch the weather',
        parameters: { type: 'object' },
      },
    ])
  })
})

describe('Freeform conformance — ordered history replay', () => {
  it('preserves message / tool-call / tool-output order and byte-exact input', () => {
    const request: ModelChatRequest = {
      context: [
        { kind: 'message', message: { role: 'user', content: 'run js' } },
        {
          kind: 'toolCall',
          call: { kind: 'freeform', id: 'call_js', name: 'exec', input: JS_THAT_LOOKS_LIKE_JSON },
        },
        {
          kind: 'toolOutput',
          output: { kind: 'custom', callId: 'call_js', name: 'exec', output: 'done' },
        },
      ],
      toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }],
    }
    const input = buildModelRequestBody(request, 'gpt-5.5-codex', false).input as Record<
      string,
      unknown
    >[]

    expect(input.map((item) => item.type)).toEqual([
      'message',
      'custom_tool_call',
      'custom_tool_call_output',
    ])
    // Byte-for-byte: never parsed as JSON, never lowered into `arguments`.
    expect(input[1].input).toBe(JS_THAT_LOOKS_LIKE_JSON)
    expect(input[1].arguments).toBeUndefined()
    // Identity travels under `call_id`, not `id`.
    expect(input[1].call_id).toBe('call_js')
    expect(input[1].id).toBeUndefined()
    expect(input[2].call_id).toBe('call_js')
  })

  it('hoists system messages into instructions and removes them from input', () => {
    const body = buildModelRequestBody(
      {
        context: [
          { kind: 'message', message: { role: 'system', content: 'be terse' } },
          { kind: 'message', message: { role: 'user', content: 'hi' } },
        ],
      },
      'm',
      false,
    )
    expect(body.instructions).toBe('be terse')
    expect(body.input).toHaveLength(1)
    expect((body.input as Record<string, unknown>[])[0].role).toBe('user')
  })

  it('maps maxTokens to max_output_tokens and merges providerOptions LAST', () => {
    const body = buildModelRequestBody(
      { context: [], maxTokens: 512, temperature: 0.1, providerOptions: { temperature: 0.9 } },
      'm',
      false,
    )
    expect(body.max_output_tokens).toBe(512)
    expect(body.max_tokens).toBeUndefined()
    expect(body.temperature).toBe(0.9)
  })
})

describe('Freeform conformance — pre-network rejection', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('OpenAI without the Responses opt-in rejects freeform before any HTTP call', async () => {
    const mockFetch = jsonFetch({})
    const provider = new OpenAIProvider('test-key', 'gpt-5.5-codex')
    const request: ModelChatRequest = {
      context: [{ kind: 'message', message: { role: 'user', content: 'run js' } }],
      toolSpecs: [{ kind: 'freeform', tool: EXEC_TOOL }],
    }

    await expect(provider.modelChat(request)).rejects.toBeInstanceOf(UnsupportedFeatureError)
    await expect(provider.modelChat(request)).rejects.toThrow(
      'provider does not support native freeform tools',
    )
    expect(() => provider.modelStream(request)).toThrow(UnsupportedFeatureError)
    expect(mockFetch).not.toHaveBeenCalled()
  })

  it('a Client over a non-freeform provider rejects freeform history before any HTTP call', () => {
    const mockFetch = jsonFetch({})
    const client = Client.builder().provider('openai').apiKey('test-key').build()
    expect(() =>
      client.modelStream({
        context: [
          { kind: 'toolOutput', output: { kind: 'custom', callId: 'call_js', output: 'x' } },
        ],
      }),
    ).toThrow('provider does not support native freeform tools')
    expect(mockFetch).not.toHaveBeenCalled()
  })
})

describe('Freeform conformance — stream termination', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('emits exactly one terminal done per successfully completed stream', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hi"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"trailing"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(error).toBeUndefined()
    expect(deltas.filter((d) => d.type === 'done')).toHaveLength(1)
  })

  it('openai EOF without a terminal yields IncompleteStreamError with the exact payload', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"hel"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"lo"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(deltas.some((d) => d.type === 'done')).toBe(false)
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe(
      'incomplete stream: openai ended without a terminal event',
    )
  })

  it('chatgpt-codex EOF without a terminal yields the hyphenated payload', async () => {
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n',
    )
    const { error } = await drain(new ChatGptCodexProvider('tok', 'acct').modelStream({ context: [] }))
    expect(error).toBeInstanceOf(IncompleteStreamError)
    expect((error as Error).message).toBe(
      'incomplete stream: chatgpt-codex ended without a terminal event',
    )
  })

  it('collectModelStream propagates the incomplete error rather than guessing a stop reason', async () => {
    sseFetch('data: {"type":"response.output_text.delta","delta":"partial"}\n\n')
    await expect(
      collectModelStream(
        new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
      ),
    ).rejects.toBeInstanceOf(IncompleteStreamError)
  })

  it('response.incomplete is a terminal that maps to max_tokens', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"partial"}\n\n' +
        'data: {"type":"response.incomplete","response":{"status":"incomplete","usage":{"input_tokens":6,"output_tokens":7}}}\n\n',
    )
    const response = await new ChatGptCodexProvider('tok', 'acct').modelChat({ context: [] })
    expect(response.content).toBe('partial')
    expect(response.stopReason).toBe('max_tokens')
    expect(response.usage.outputTokens).toBe(7)
  })
})

describe('Freeform conformance — collector rules', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('ToolCallDone is authoritative over accumulated freeform deltas', async () => {
    // The deltas spell "console." + "log(1);" but the done frame is the truth.
    sseFetch(
      'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"console."}\n\n' +
        'data: {"type":"response.custom_tool_call_input.delta","call_id":"call_js","delta":"log(1);"}\n\n' +
        'data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":"AUTHORITATIVE"}}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":2,"output_tokens":3}}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.toolCalls).toEqual([
      { kind: 'freeform', id: 'call_js', name: 'exec', input: 'AUTHORITATIVE' },
    ])
    expect(response.stopReason).toBe('tool_use')
    expect(response.usage.outputTokens).toBe(3)
  })

  it('freeform input survives the whole stream byte-for-byte', async () => {
    const encoded = JSON.stringify(JS_THAT_LOOKS_LIKE_JSON)
    sseFetch(
      `data: {"type":"response.output_item.done","item":{"type":"custom_tool_call","call_id":"call_js","name":"exec","input":${encoded}}}\n\n` +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.toolCalls[0]).toEqual({
      kind: 'freeform',
      id: 'call_js',
      name: 'exec',
      input: JS_THAT_LOOKS_LIKE_JSON,
    })
  })

  it('usage REPLACES rather than merges', async () => {
    const response = await collectModelStream(
      (async function* (): AsyncGenerator<ModelStreamDelta> {
        yield { type: 'usage', usage: { inputTokens: 100, outputTokens: 100 } }
        yield { type: 'usage', usage: { inputTokens: 1, outputTokens: 2 } }
        yield { type: 'done', stopReason: 'end_turn' }
      })(),
    )
    expect(response.usage).toEqual({ inputTokens: 1, outputTokens: 2 })
  })

  it('ThinkingDone wins over accumulated thinking deltas', async () => {
    sseFetch(
      'data: {"type":"response.reasoning_text.delta","delta":"think "}\n\n' +
        'data: {"type":"response.reasoning_text.delta","delta":"hard"}\n\n' +
        'data: {"type":"response.reasoning_text.done","text":"AUTHORITATIVE"}\n\n' +
        'data: {"type":"response.output_text.delta","delta":"answer"}\n\n' +
        'data: {"type":"response.completed","response":{"status":"completed"}}\n\n',
    )
    const response = await collectModelStream(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(response.thinking).toBe('AUTHORITATIVE')
    expect(response.content).toBe('answer')
  })

  it('pending deltas drain before a stored stream error surfaces', async () => {
    sseFetch(
      'data: {"type":"response.output_text.delta","delta":"before"}\n\n' +
        'data: {"type":"error","message":"upstream exploded"}\n\n',
    )
    const { deltas, error } = await drain(
      new OpenAIProvider('test-key', 'm').withResponsesApi(true).modelStream({ context: [] }),
    )
    expect(deltas).toEqual([{ type: 'text', delta: 'before' }])
    expect(error).toBeInstanceOf(StreamError)
    expect((error as Error).message).toBe('upstream exploded')
  })
})

describe('Freeform conformance — Codex body normalization', () => {
  afterEach(() => vi.unstubAllGlobals())

  it('reasoning_effort never reaches the wire and the per-request value wins', async () => {
    let sent: Record<string, any> = {}
    sseFetch('data: {"type":"response.completed","response":{"status":"completed"}}\n\n', (_u, init) => {
      sent = JSON.parse(String(init?.body))
    })

    await new ChatGptCodexProvider('tok', 'acct').reasoningEffort('low').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      providerOptions: { reasoning_effort: 'high' },
    })

    expect(sent.reasoning).toEqual({ effort: 'high', summary: 'auto' })
    expect('reasoning_effort' in sent).toBe(false)
  })

  it('codex hard-sets store/include/parallel_tool_calls and tool_choice auto', async () => {
    let sent: Record<string, any> = {}
    sseFetch('data: {"type":"response.completed","response":{"status":"completed"}}\n\n', (_u, init) => {
      sent = JSON.parse(String(init?.body))
    })

    await new ChatGptCodexProvider('tok', 'acct').modelChat({
      context: [{ kind: 'message', message: { role: 'user', content: 'hi' } }],
      toolChoice: { type: 'required' },
    })

    expect(sent.store).toBe(false)
    expect(sent.include).toEqual(['reasoning.encrypted_content'])
    expect(sent.parallel_tool_calls).toBe(true)
    expect(sent.tool_choice).toBe('auto')
  })
})
```

- [ ] **Step 2: Run it**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test -- tests/freeform-conformance.test.ts`

Expected: PASS. The behaviour shipped in T1/T2; this file is the D9 cross-SDK gate, not a driver of new code.

- [ ] **Step 3: Prove the suite is not vacuous**

Make each of these three one-line mutations in turn, run the suite, confirm it FAILS, then revert:

1. In `src/serialize/responses.ts`, change `` `incomplete stream: ${provider} ended without a terminal event` `` to `` `incomplete stream: ${provider} ended` ``.
   Expected: FAIL — `expected 'incomplete stream: openai ended' to be 'incomplete stream: openai ended without a terminal event'`.
2. In `src/stream.ts` `collectModelStream`, replace the `tool_call_done` arm's `toolCalls.push(delta.call)` with a lowering of the accumulated buffer.
   Expected: FAIL — `ToolCallDone is authoritative over accumulated freeform deltas`.
3. In `src/providers/chatgpt_codex.ts` `buildModelResponsesBody`, delete the `delete body.reasoning_effort` line.
   Expected: FAIL — `expected true to be false` on `'reasoning_effort' in sent`.

Revert all three (`git checkout -- src/`) before continuing.

- [ ] **Step 4: Run tests**

Run: `cd sdks/typescript && npm run build && npm run typecheck && npm run test`

Expected: PASS

- [ ] **Step 5: Commit, push, verify, open PR C-TS**

```bash
git checkout -b feat/ts-freeform-conformance origin/main
git add sdks/typescript/tests/freeform-conformance.test.ts
git commit -m "feat: add the TypeScript Freeform conformance suite (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

(cd sdks/python && uv sync --all-extras)
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/ts-freeform-conformance
test "$(git ls-remote origin refs/heads/feat/ts-freeform-conformance | cut -f1)" = "$(git rev-parse HEAD)"

gh pr create --base main --head feat/ts-freeform-conformance \
  --title "feat: TypeScript Freeform conformance suite (#270)" \
  --body "Task group C-TS of the Freeform parity milestone (#270). Spec-anchored gates for \`specs/types.md\` § Native Model API: tool-definition wire shapes, ordered history replay, byte-exact freeform input, pre-network rejection, stream termination, and the collector rules from D8. Mirrors \`sdks/rust/tests/freeform_conformance.rs\`.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

---

### Task 12: Rust Freeform conformance suite (PR C-RS)

> **Format deviation, deliberate** — same reason as Task 11, and stronger here: D9 requires the Rust file *because a cross-SDK gate that skips one SDK is not a gate*, and the Rust behaviour already ships. **No Rust source changes. No version bump.** Everything this file touches is already `pub`.

**Files:**
- Create: `sdks/rust/tests/freeform_conformance.rs`
- Test: itself

**Interfaces:**
- Consumes (all already public): `motosan_ai::providers::responses::build_model_request_body`; `motosan_ai::providers::openai::OpenAIProvider`; `motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider`; `motosan_ai::providers::ProviderImpl`; root re-exports `collect_model_stream`, `FreeformTool`, `FreeformToolFormat`, `FunctionCallOutputPayload`, `Message`, `ModelChatRequest`, `ModelContextItem`, `ModelStreamDelta`, `ModelToolCall`, `ModelToolOutput`, `ModelToolSpec`, `MotosanError`, `StopReason`, `Tool`, `ToolChoice`, `ToolSchema`, `Usage`.
- Produces: nothing importable — a gate.

- [ ] **Step 1: Write the conformance suite**

Create `sdks/rust/tests/freeform_conformance.rs`:

```rust
#![cfg(all(feature = "openai", feature = "chatgpt-codex"))]

//! Freeform parity conformance gates (specs/types.md § Native Model API).
//!
//! Cross-SDK mirrors:
//! - sdks/python/tests/test_freeform_conformance.py
//! - sdks/typescript/tests/freeform-conformance.test.ts
//!
//! Rust already implements every rule asserted here; the file exists because a
//! cross-SDK gate that skips one SDK is not a gate (milestone D9). It adds no
//! source changes and no version bump.

use mockito::Matcher;
use motosan_ai::providers::chatgpt_codex::ChatGptCodexProvider;
use motosan_ai::providers::openai::OpenAIProvider;
use motosan_ai::providers::responses::build_model_request_body;
use motosan_ai::providers::ProviderImpl;
use motosan_ai::{
    collect_model_stream, FreeformTool, FreeformToolFormat, FunctionCallOutputPayload, Message,
    ModelChatRequest, ModelContextItem, ModelStreamDelta, ModelToolCall, ModelToolOutput,
    ModelToolSpec, MotosanError, StopReason, Tool, ToolChoice, ToolSchema, Usage,
};
use serde_json::json;
use tokio_stream::{iter, StreamExt};

/// The Rust fixture for "looks like JSON but is JavaScript".
const JS_THAT_LOOKS_LIKE_JSON: &str = "{\"this\":\"looks like json\"}\nconsole.log('but is JS');";

fn exec_tool() -> FreeformTool {
    FreeformTool {
        name: "exec".to_string(),
        description: "Run JavaScript".to_string(),
        format: FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: "start: source".to_string(),
        },
    }
}

fn freeform_request() -> ModelChatRequest {
    ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build()
}

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

#[test]
fn freeform_tool_serializes_with_a_mandatory_exact_format_object() {
    let req = ModelChatRequest::builder()
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(
        body["tools"],
        json!([{
            "type": "custom",
            "name": "exec",
            "description": "Run JavaScript",
            "format": {"type": "grammar", "syntax": "lark", "definition": "start: source"}
        }])
    );
}

#[test]
fn function_tool_serializes_input_schema_under_parameters() {
    let req = ModelChatRequest::builder()
        .tool_spec(ModelToolSpec::Function(Tool::from(ToolSchema::new(
            "get_weather",
            "Fetch the weather",
            json!({"type": "object"}),
        ))))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(
        body["tools"],
        json!([{
            "type": "function",
            "name": "get_weather",
            "description": "Fetch the weather",
            "parameters": {"type": "object"}
        }])
    );
}

// ---------------------------------------------------------------------------
// Ordered history replay
// ---------------------------------------------------------------------------

#[test]
fn ordered_history_replays_byte_exact_and_in_order() {
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("run js")))
        .context_item(ModelContextItem::ToolCall(ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: JS_THAT_LOOKS_LIKE_JSON.to_string(),
        }))
        .context_item(ModelContextItem::ToolOutput(ModelToolOutput::Custom {
            call_id: "call_js".to_string(),
            name: Some("exec".to_string()),
            output: FunctionCallOutputPayload::Text("done".to_string()),
        }))
        .tool_spec(ModelToolSpec::Freeform(exec_tool()))
        .build();
    let body = build_model_request_body(&req, "gpt-5.5-codex", false, None);
    let input = body["input"].as_array().expect("input is an array");

    let types: Vec<&str> = input
        .iter()
        .map(|item| item["type"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        types,
        vec!["message", "custom_tool_call", "custom_tool_call_output"]
    );

    // Byte-for-byte: never parsed as JSON, never lowered into `arguments`.
    assert_eq!(input[1]["input"], json!(JS_THAT_LOOKS_LIKE_JSON));
    assert!(input[1].get("arguments").is_none());
    // Identity travels under `call_id`, not `id`.
    assert_eq!(input[1]["call_id"], json!("call_js"));
    assert!(input[1].get("id").is_none());
    assert_eq!(input[2]["call_id"], json!("call_js"));
}

#[test]
fn system_messages_are_hoisted_into_instructions_and_removed_from_input() {
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::system("be terse")))
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(body["instructions"], json!("be terse"));
    let input = body["input"].as_array().expect("input is an array");
    assert_eq!(input.len(), 1);
    assert_eq!(input[0]["role"], json!("user"));
}

#[test]
fn max_tokens_maps_to_max_output_tokens_and_provider_options_merge_last() {
    let req = ModelChatRequest::builder()
        .max_tokens(512)
        .temperature(0.1)
        .tool_choice(ToolChoice::Required)
        .provider_options(json!({"temperature": 0.9}))
        .build();
    let body = build_model_request_body(&req, "m", false, None);

    assert_eq!(body["max_output_tokens"], json!(512));
    assert!(body.get("max_tokens").is_none());
    assert_eq!(body["tool_choice"], json!("required"));
    assert_eq!(body["temperature"], json!(0.9));
}

// ---------------------------------------------------------------------------
// Pre-network rejection
// ---------------------------------------------------------------------------

#[tokio::test]
async fn openai_without_the_responses_opt_in_rejects_freeform_before_http() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", Matcher::Any)
        .expect(0)
        .with_status(500)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_chat_url(format!("{}/v1/chat/completions", server.url()))
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let chat_err = provider
        .model_chat(freeform_request())
        .await
        .expect_err("native freeform must be rejected");
    assert!(matches!(chat_err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform")));

    let stream_err = match provider.model_stream(freeform_request()).await {
        Ok(_) => panic!("native freeform streams must be rejected"),
        Err(err) => err,
    };
    assert!(matches!(stream_err, MotosanError::UnsupportedFeature(msg) if msg.contains("freeform")));

    mock.assert_async().await;
}

// ---------------------------------------------------------------------------
// Stream termination
// ---------------------------------------------------------------------------

#[tokio::test]
async fn exactly_one_done_per_successfully_completed_stream() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hi\"}\n\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\"}}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"trailing\"}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let mut stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let mut dones = 0;
    while let Some(item) = stream.next().await {
        if matches!(item.expect("no stream error"), ModelStreamDelta::Done { .. }) {
            dones += 1;
        }
    }
    assert_eq!(dones, 1, "exactly one terminal Done per completed stream");
}

#[tokio::test]
async fn openai_eof_without_terminal_yields_the_exact_incomplete_payload() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"hel\"}\n\n",
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"lo\"}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));

    let mut stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let mut saw_done = false;
    let mut last_err = None;
    while let Some(item) = stream.next().await {
        match item {
            Ok(delta) => saw_done |= matches!(delta, ModelStreamDelta::Done { .. }),
            Err(err) => {
                last_err = Some(err);
                break;
            }
        }
    }

    assert!(!saw_done, "no Done may be fabricated on truncation");
    match last_err.expect("EOF without a terminal must yield an error") {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "openai ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

#[tokio::test]
async fn chatgpt_codex_eof_without_terminal_yields_the_exact_incomplete_payload() {
    let mut server = mockito::Server::new_async().await;
    let sse = "data: {\"type\":\"response.custom_tool_call_input.delta\",\"call_id\":\"call_js\",\"delta\":\"console.\"}\n\n";
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");

    match collect_model_stream(stream)
        .await
        .expect_err("EOF without a terminal must yield IncompleteStream")
    {
        MotosanError::IncompleteStream(msg) => {
            assert_eq!(msg, "chatgpt-codex ended without a terminal event")
        }
        other => panic!("expected IncompleteStream, got {other:?}"),
    }
}

#[tokio::test]
async fn response_incomplete_is_a_terminal_that_maps_to_max_tokens() {
    let mut server = mockito::Server::new_async().await;
    let sse = concat!(
        "data: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n",
        "data: {\"type\":\"response.incomplete\",\"response\":{\"status\":\"incomplete\",\"usage\":{\"input_tokens\":6,\"output_tokens\":7}}}\n\n"
    );
    server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider =
        ChatGptCodexProvider::new("oauth-token", "acct-123", "gpt-5.5", Some(server.url()));
    let response = provider
        .model_chat(
            ModelChatRequest::builder()
                .context_item(ModelContextItem::Message(Message::user("short")))
                .build(),
        )
        .await
        .expect("native chat");

    assert_eq!(response.content, "partial");
    assert_eq!(response.stop_reason, StopReason::MaxTokens);
    assert_eq!(response.usage.output_tokens, 7);
}

// ---------------------------------------------------------------------------
// Collector rules
// ---------------------------------------------------------------------------

#[tokio::test]
async fn tool_call_done_is_authoritative_over_accumulated_freeform_deltas() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::FreeformInput {
                call_id: "call_js".to_string(),
                delta: "console.".to_string(),
            },
            ModelStreamDelta::FreeformInput {
                call_id: "call_js".to_string(),
                delta: "log(1);".to_string(),
            },
            ModelStreamDelta::ToolCallDone {
                call: ModelToolCall::Freeform {
                    id: "call_js".to_string(),
                    name: "exec".to_string(),
                    input: "AUTHORITATIVE".to_string(),
                },
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::ToolUse,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: "AUTHORITATIVE".to_string(),
        }]
    );
    assert_eq!(response.stop_reason, StopReason::ToolUse);
}

#[tokio::test]
async fn usage_replaces_rather_than_merges() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::Usage {
                usage: Usage {
                    input_tokens: 100,
                    output_tokens: 100,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
            ModelStreamDelta::Usage {
                usage: Usage {
                    input_tokens: 1,
                    output_tokens: 2,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                },
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(response.usage.input_tokens, 1);
    assert_eq!(response.usage.output_tokens, 2);
}

#[tokio::test]
async fn thinking_done_wins_over_accumulated_thinking_deltas() {
    let stream = Box::pin(iter(
        vec![
            ModelStreamDelta::ThinkingDelta {
                delta: "think ".to_string(),
            },
            ModelStreamDelta::ThinkingDelta {
                delta: "hard".to_string(),
            },
            ModelStreamDelta::ThinkingDone {
                thinking: "AUTHORITATIVE".to_string(),
            },
            ModelStreamDelta::Text {
                delta: "answer".to_string(),
            },
            ModelStreamDelta::Done {
                stop_reason: StopReason::EndTurn,
            },
        ]
        .into_iter()
        .map(Ok),
    ));

    let response = collect_model_stream(stream).await.expect("collect");
    assert_eq!(response.thinking.as_deref(), Some("AUTHORITATIVE"));
    assert_eq!(response.content, "answer");
}

#[tokio::test]
async fn freeform_input_survives_the_whole_stream_byte_for_byte() {
    let mut server = mockito::Server::new_async().await;
    let item = json!({
        "type": "response.output_item.done",
        "item": {
            "type": "custom_tool_call",
            "call_id": "call_js",
            "name": "exec",
            "input": JS_THAT_LOOKS_LIKE_JSON
        }
    });
    let sse = format!(
        "data: {item}\n\ndata: {{\"type\":\"response.completed\",\"response\":{{\"status\":\"completed\"}}}}\n\n"
    );
    server
        .mock("POST", "/v1/responses")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse)
        .create_async()
        .await;

    let provider = OpenAIProvider::new("test-key", None)
        .with_responses_api(true)
        .with_responses_url(format!("{}/v1/responses", server.url()));
    let stream = provider
        .model_stream(freeform_request())
        .await
        .expect("native stream");
    let response = collect_model_stream(stream).await.expect("collect");

    assert_eq!(
        response.tool_calls,
        vec![ModelToolCall::Freeform {
            id: "call_js".to_string(),
            name: "exec".to_string(),
            input: JS_THAT_LOOKS_LIKE_JSON.to_string(),
        }]
    );
}

// ---------------------------------------------------------------------------
// Codex body normalization
// ---------------------------------------------------------------------------

#[test]
fn codex_reasoning_effort_never_reaches_the_wire_and_per_request_wins() {
    let provider = ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None)
        .with_reasoning_effort(Some("low".to_string()));
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .provider_options(json!({"reasoning_effort": "high"}))
        .build();
    let body = provider.build_model_responses_body(&req);

    assert_eq!(body["reasoning"], json!({"effort": "high", "summary": "auto"}));
    assert!(
        body.get("reasoning_effort").is_none(),
        "the raw reasoning_effort key must never reach the wire"
    );
}

#[test]
fn codex_hard_sets_its_body_fields_over_the_caller() {
    let provider = ChatGptCodexProvider::new("test-token", "acct-123", "gpt-5.5", None);
    let req = ModelChatRequest::builder()
        .context_item(ModelContextItem::Message(Message::user("hi")))
        .tool_choice(ToolChoice::Required)
        .build();
    let body = provider.build_model_responses_body(&req);

    assert_eq!(body["store"], json!(false));
    assert_eq!(body["stream"], json!(true));
    assert_eq!(body["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(body["parallel_tool_calls"], json!(true));
    assert_eq!(body["tool_choice"], json!("auto"));
    assert_eq!(body["instructions"], json!("You are a helpful assistant."));
}
```

- [ ] **Step 2: Run it**

Run:

```bash
cd sdks/rust && env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY \
  -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY \
  -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST \
  cargo test --all-features --test freeform_conformance
```

Expected: PASS — 14 tests, `test result: ok`. Rust already implements every rule; this is the D9 gate-parity file.

- [ ] **Step 3: Prove the suite is not vacuous**

Make each mutation in turn, run the command from Step 2, confirm FAIL, then `git checkout -- src/`:

1. In `src/providers/responses.rs`, change `"{} ended without a terminal event"` to `"{} ended"`.
   Expected: FAIL — `assertion \`left == right\` failed` on `openai ended without a terminal event`.
2. In `src/providers/responses.rs` `build_model_request_body`, move the `provider_options` merge loop above the `temperature` assignment.
   Expected: FAIL — `max_tokens_maps_to_max_output_tokens_and_provider_options_merge_last`.
3. In `src/providers/chatgpt_codex.rs` `build_model_responses_body`, delete the `remove("reasoning_effort")` block.
   Expected: FAIL — `the raw reasoning_effort key must never reach the wire`.

- [ ] **Step 4: Run the full gates**

```bash
cd sdks/rust
cargo fmt --all -- --check
cargo clippy --all-features --all-targets -- -D warnings
env -u ANTHROPIC_API_KEY -u OPENAI_API_KEY -u GEMINI_API_KEY -u GOOGLE_API_KEY \
    -u GEMINI_OAUTH_TOKEN -u GEMINI_PROJECT_ID -u MINIMAX_API_KEY \
    -u OLLAMA_API_KEY -u OLLAMA_BASE_URL -u OLLAMA_HOST \
    cargo test --all-features
```

Expected: PASS. If `cargo fmt --all -- --check` reports a diff in the new file, run `cargo fmt --all` and re-run; `treefmt` formats `*.rs` too and the pre-push hook rejects an unformatted tree.

- [ ] **Step 5: Commit, push, verify, open PR C-RS**

```bash
git checkout -b feat/rust-freeform-conformance origin/main
cargo fmt --all
git add sdks/rust/tests/freeform_conformance.rs
git commit -m "feat: add the Rust Freeform conformance suite (#270)" -m "Co-Authored-By: Claude Opus 5 (1M context) <noreply@anthropic.com>"

(cd sdks/python && uv sync --all-extras)
treefmt --fail-on-change
python3 scripts/check-versions.py

git push -u origin feat/rust-freeform-conformance
test "$(git ls-remote origin refs/heads/feat/rust-freeform-conformance | cut -f1)" = "$(git rev-parse HEAD)"

gh pr create --base main --head feat/rust-freeform-conformance \
  --title "feat: Rust Freeform conformance suite (#270)" \
  --body "Task group C-RS of the Freeform parity milestone (#270). Adds \`sdks/rust/tests/freeform_conformance.rs\`, the Rust third of the spec-anchored suite required by milestone decision D9 — a cross-SDK gate that skips one SDK is not a gate.

**Test file only.** No \`src/\` changes, no version bump: every symbol it uses is already public and every rule it asserts already ships in Rust 0.26.0+.

🤖 Generated with [Claude Code](https://claude.com/claude-code)"
```

---

## Done criteria

- [ ] `specs/types.md` § Native Model API (widened by the Python plan's task group S) is merged before T1 opens.
- [ ] Four PRs merged in order: **T1** → **T2** → **C-TS**, with **C-RS** merged alongside C-TS.
- [ ] `sdks/typescript`: `npm ci && npm run build && npm run typecheck && npm run test` green on `main`.
- [ ] `sdks/rust`: `cargo fmt --all -- --check`, `cargo clippy --all-features --all-targets -- -D warnings`, and the credential-stripped `cargo test --all-features` green on `main`.
- [ ] Root: `treefmt --fail-on-change` and `python3 scripts/check-versions.py` clean.
- [ ] Every push verified by SHA against `git ls-remote`.
- [ ] `Client.modelChat` / `modelStream` / `modelStreamCollect` reachable from `@motosan-ai/sdk`'s root entrypoint, and `ClientBuilder.openaiResponsesApi(true)` flips `supportsFreeformTools`.
- [ ] `asDispatchProvider` forwards `modelChat`/`modelStream` in **both** branches, pinned by tests.
- [ ] IncompleteStream payloads on the native path are exactly `openai ended without a terminal event` and `chatgpt-codex ended without a terminal event` (TypeScript renders them behind the SDK's standard `incomplete stream: ` prefix); the legacy `chatgpt_codex` token is unchanged.
- [ ] No version bumps in any of the four PRs — the release is `scripts/bump-version.py`'s job (D10: Python 0.20.0 / TypeScript 0.16.0; Rust unchanged).

## Not in scope

- **Python** (types, `providers/responses.py`, `UnsupportedFeatureError`, provider wiring, `tests/test_public_exports.py`, and the Python third of the conformance suite) — a separate plan, task groups P1/P2.
- **The spec PR** — task group **S** of the Python plan, and a **prerequisite** of this one. It widens § Native Model API to a cross-SDK normative contract and records D3's omission and D4's Python error type. This plan edits nothing under `specs/`.
- **The release** (task group REL): version bumps, CHANGELOG headings, doc banners, and the spec's implementation-status line all belong to `scripts/bump-version.py` plus the manual REL edits.
- Porting `GeminiCodeAssist` or the CLI backends to TypeScript.
- Native Freeform support on any provider beyond OpenAI-in-Responses-mode and ChatGPT Codex.
- Any change to the legacy `ChatRequest` / `Tool` / `ToolCall` / `ChatResponse` / `StreamEvent` APIs, including the legacy `chatgpt_codex` IncompleteStream token.


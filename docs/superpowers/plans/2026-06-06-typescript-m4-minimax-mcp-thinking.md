# Milestone 4 — MiniMax Anthropic Wire + MCP + Extended-Thinking Request Config Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Re-route MiniMax from its interim OpenAI-compat wire to the Anthropic-compatible `/v1/messages` wire, add MCP server/tool config (types + serialization + beta headers), and replace the naive thinking passthrough with proper extended-thinking request serialization (adaptive vs enabled, forced temperature) — shipping v0.7.0.

**Architecture:** Builds on merged M1+M2+M3 (on `main`). MCP types come first; the Anthropic serializer gains real thinking-config + MCP serialization; the Anthropic provider gains beta headers; MiniMax is rewritten onto the Anthropic wire; non-Anthropic providers reject MCP via a new capability flag.

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, raw `fetch`. Reference: Rust `sdks/rust/src/providers/{anthropic,mod}.rs` + `types.rs`.

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M4). **Depends on:** M1 (#185) + M2 (#186) + M3 (#187), all merged. Branch M4 off `main`.

---

## Conventions (apply to EVERY task — override anything ambiguous in a task body)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. Package: `sdks/typescript/`. **Commands run from `sdks/typescript/`**. Paths repo-relative.
- **Workflow:** feature branch, land via **PR + CI**. Commit after each task. (From a git worktree the pre-push hook can't run Rust — verify `npm run build` + `npm run test` locally and `git push --no-verify`; CI runs the full gate.)
- **Module system:** strict + NodeNext. Every relative import ends in `.js`.
- **Layout:** source in `src/`, tests in `tests/` (NOT tsc-checked — run by vitest). Mock fetch via `vi.stubGlobal`. Live tests env-gated. **No `npm run format` script** (gate = `npm run build` + `npm run test`).

## Built on M1+M2+M3 (import, never redefine)

`types.ts` (ChatRequest with `thinking?:ThinkingConfig{budgetTokens}`, systemBlocks, toolChoice, stopSequences; ContentBlock incl document); `error.ts` (mapHttpError WITH `.status`, UnsupportedFeatureError, isRetryableStatus/Network, parseRetryAfter); `http/fetch.ts`, `http/sse.ts`, `stream.ts`; `serialize/anthropic.ts` (already serializes content blocks incl document, system-as-array, tools+cache, tool_choice, stop_sequences — and a **naive `result.thinking = req.thinking` passthrough that Task 2 REPLACES**); `provider.ts` (ProviderCapabilities + validateRequest + dispatch); `providers/anthropic.ts` (parses thinking in chat+stream, retry via `withRetryPolicy` setter); `providers/minimax.ts` (**currently OpenAI-compat — Task 4 rewrites it**); `client.ts` (Client + ClientBuilder).

## Canonical homes & cross-task contract

| Symbol(s) | Home | Owner |
|---|---|---|
| `McpServerType`, `McpServerConfig`, `McpToolConfig` (+ `ChatRequest.mcpServers?/mcpToolConfigs?`) | `src/types.ts` | **T1** |
| `ADAPTIVE_THINKING_MODELS`, `modelUsesAdaptiveThinking` (EXPORTED), `applyThinkingConfig`, `serializeMcpToolConfig` + the mcp_servers/combined-tools assembly + thinking/temperature control flow | `src/serialize/anthropic.ts` | **T2** |
| `buildBetaHeader` + beta-header wiring into chat()/stream() | `src/providers/anthropic.ts` | **T3** |
| MiniMax Anthropic-compat rewrite + `ClientBuilder.minimaxBaseUrl` | `src/providers/minimax.ts` + `src/client.ts` | **T4** |
| `supportsMcp` on `ProviderCapabilities` + `validateRequest` MCP gate + per-provider caps | `src/provider.ts` + providers | **T5** |
| MCP type exports + done-criteria smoke test | `src/index.ts` | **T6** |

**Binding rules:**
- **Thinking serialization** (T2, replaces the naive passthrough): adaptive models (`claude-opus-4-8`/`4-7`/`4-6`) → `thinking:{type:adaptive,display:summarized}` + `output_config:{effort:high}`, NO budget_tokens, NO temperature override; all other models → `thinking:{type:enabled,budget_tokens,display:summarized}` AND **force `temperature=1.0`** (overriding any user temperature). User `temperature` is applied ONLY when `thinking` is absent. Adaptive detection uses the resolved model (`request.model ?? this.model`).
- **MCP serialization** (T2): `mcp_servers` body key (only if non-empty); `mcp_toolset` items appended to the SAME `tools` array as regular tools (`[...regularTools, ...mcpToolsetItems]`, set `result.tools` if either non-empty); `tool_choice:'none'` still deletes `result.tools`. Serialize `mcpToolConfigs` as-given (no serializer-side auto-all).
- **Beta headers** (T3): `anthropic-beta` = mcp-client beta when `mcpServers` set + interleaved-thinking beta when `thinking` set AND not adaptive (comma-joined; header omitted when empty). `buildBetaHeader` imports `modelUsesAdaptiveThinking` from `serialize/anthropic.js`.
- **MCP is Anthropic-wire only** (T5): non-Anthropic providers reject `mcpServers`/`mcpToolConfigs` via `UnsupportedFeatureError` before any HTTP call (`supportsMcp` capability + `validateRequest`). MiniMax is Anthropic-wire after T4 → `supportsMcp:true`; OpenAI → false.

**Dependency order:** 1 types → 2 serialize → 3 beta headers → 4 minimax → 5 mcp-reject → 6 wireup. (T3 imports from T2; T4 uses T2's serializer; T5 adds the capability flag; T6 exports.)

---

### Task 1: MCP types in `types.ts`

Add the three MCP types (`McpServerType`, `McpServerConfig`, `McpToolConfig`) and extend
`ChatRequest` with the two optional MCP fields. Pure type-level work — no runtime code.
These types are imported (never re-declared) by Tasks 2, 5, and 6.

Source of truth: contract §1; Rust `sdks/rust/src/types.rs:300-340` (MCP enums/struct) and
`types.rs:364-372` (the two `ChatRequest` fields). TS uses camelCase field names; the wire
shape (snake_case) is produced by the serializer in Task 2, NOT here.

**Files:**
- `sdks/typescript/src/types.ts` (modify — add types + extend `ChatRequest`)
- `sdks/typescript/tests/types.test.ts` (modify — add roundtrip/omission tests)

**Steps:**

- [ ] **Step 1: Write the failing test first (TDD).**
  Add a new `describe` block to the END of `sdks/typescript/tests/types.test.ts` (after the
  last `describe`, before EOF). It imports the new types and asserts their shapes roundtrip and
  that `ChatRequest` carries the optional fields. The import line at the top of the file
  (`import type { ... } from '../src/types.js'`) must also be extended to pull in the three new
  type names so the test references them.

  First extend the existing top-of-file import block. Replace:
  ```ts
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
  ```
  with:
  ```ts
  import type {
    ChatRequest,
    ChatResponse,
    ContentBlock,
    DocumentSource,
    ImageSource,
    McpServerConfig,
    McpServerType,
    McpToolConfig,
    Message,
    StopReason,
    StreamEvent,
    StreamEventType,
    Tool,
    ToolCall,
    ToolChoice,
    Usage,
  } from '../src/types.js'
  ```

  Then append this `describe` block at the end of the file:
  ```ts
  describe('MCP types', () => {
    it('McpServerType is the literal "url"', () => {
      const t: McpServerType = 'url'
      expect(t).toBe('url')
    })

    it('McpServerConfig roundtrips with and without authorizationToken', () => {
      const withToken: McpServerConfig = {
        type: 'url',
        url: 'https://mcp.example.com/sse',
        name: 'example',
        authorizationToken: 'tok_123',
      }
      const withoutToken: McpServerConfig = {
        type: 'url',
        url: 'https://mcp.example.com/sse',
        name: 'example',
      }
      expect(roundtrip(withToken)).toEqual(withToken)
      expect(roundtrip(withoutToken)).toEqual(withoutToken)
      const json = JSON.parse(JSON.stringify(withoutToken))
      expect('authorizationToken' in json).toBe(false)
    })

    it('McpToolConfig discriminated union: all/allowed/denied roundtrip', () => {
      const all: McpToolConfig = { kind: 'all', mcpServerName: 'srv' }
      const allowed: McpToolConfig = {
        kind: 'allowed',
        mcpServerName: 'srv',
        allowedTools: ['read', 'list'],
      }
      const denied: McpToolConfig = {
        kind: 'denied',
        mcpServerName: 'srv',
        deniedTools: ['delete'],
      }
      expect(roundtrip(all)).toEqual(all)
      expect(roundtrip(allowed)).toEqual(allowed)
      expect(roundtrip(denied)).toEqual(denied)
    })

    it('ChatRequest carries optional mcpServers and mcpToolConfigs', () => {
      const req: ChatRequest = {
        messages: [{ role: 'user', content: 'hi' }],
        mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
        mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
      }
      const json = roundtrip(req)
      expect(json.mcpServers).toHaveLength(1)
      expect(json.mcpToolConfigs).toHaveLength(1)
      expect(json.mcpServers?.[0].name).toBe('m')
      expect(json.mcpToolConfigs?.[0].kind).toBe('all')
    })

    it('ChatRequest omits the MCP fields when unset', () => {
      const req: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      const json = JSON.parse(JSON.stringify(req))
      expect('mcpServers' in json).toBe(false)
      expect('mcpToolConfigs' in json).toBe(false)
    })
  })
  ```

  Run it — it MUST fail to compile/run because the types don't exist yet:
  ```bash
  npx vitest run tests/types.test.ts
  ```
  Expected: errors referencing `McpServerType`, `McpServerConfig`, `McpToolConfig`, or
  `mcpServers`/`mcpToolConfigs` not existing on the imported module / `ChatRequest`.

- [ ] **Step 2: Add the MCP types to `src/types.ts`.**
  Insert the three types immediately AFTER the `ThinkingConfig` interface (ends at line 61,
  `}`) and BEFORE the `SystemBlock` comment (line 63). Paste exactly:
  ```ts
  /** Server-side MCP transport type. Mirrors Rust `McpServerType` (types.rs:301-305, lowercase). */
  export type McpServerType = 'url'

  /**
   * Config for a server-side MCP server. Mirrors Rust `McpServerConfig`
   * (types.rs:312-320). The provider connects to the MCP server server-side;
   * the client never manages the connection. Anthropic wire only.
   */
  export interface McpServerConfig {
    /** Rust `kind`, serde-renamed to wire key "type" (types.rs:314). */
    type: McpServerType
    url: string
    name: string
    /** Rust `authorization_token`; omitted on the wire when absent (types.rs:318-319). */
    authorizationToken?: string
  }

  /**
   * Per-server MCP tool filtering. Mirrors Rust `McpToolConfig` enum
   * (types.rs:326-340). Discriminated union on `kind` — a TS contract choice
   * (the Rust enum is untagged; the wire form is hand-built by the serializer).
   */
  export type McpToolConfig =
    | { kind: 'all'; mcpServerName: string }
    | { kind: 'allowed'; mcpServerName: string; allowedTools: string[] }
    | { kind: 'denied'; mcpServerName: string; deniedTools: string[] }
  ```

- [ ] **Step 3: Extend `ChatRequest` with the two optional MCP fields.**
  In `src/types.ts`, in the `ChatRequest` interface, replace:
  ```ts
    temperature?: number
    providerOptions?: Record<string, unknown>
  }
  ```
  with:
  ```ts
    temperature?: number
    providerOptions?: Record<string, unknown>
    /** Server-side MCP servers (Anthropic wire only). Mirrors Rust `mcp_servers` (types.rs:364-372). */
    mcpServers?: McpServerConfig[]
    /** Per-server MCP tool filtering. Mirrors Rust `mcp_tool_configs` (types.rs:364-372). */
    mcpToolConfigs?: McpToolConfig[]
  }
  ```

- [ ] **Step 4: Verify the test passes.**
  ```bash
  npx vitest run tests/types.test.ts
  ```
  Expected output: the `MCP types` describe block passes (5 new `it`s green), and all
  pre-existing tests in that file still pass. No failures.

- [ ] **Step 5: Type-check the whole package (catch any strict-mode fallout).**
  ```bash
  npm run build
  ```
  Expected: `tsc` exits 0 with no output. (Only types were added; no other file references
  them yet, so the build is unaffected beyond compiling the new declarations.)

- [ ] **Step 6: Commit.**
  ```bash
  git add sdks/typescript/src/types.ts sdks/typescript/tests/types.test.ts
  git commit -m "feat(ts): add MCP server/tool config types to ChatRequest"
  ```

---

### Task 2: thinking-config + MCP serialization in `serialize/anthropic.ts`

Replace the broken naive thinking passthrough and the independent temperature block with the
contract's control flow, and add MCP serialization (combined tools array + `mcp_servers` body
key). This task OWNS the serializer; it imports MCP types from `../types.js` (added by Task 1)
and EXPORTS `modelUsesAdaptiveThinking` (imported by Task 3).

Source of truth: contract §2 + §3; Rust `anthropic.rs:195-220` (adaptive detection +
`apply_thinking_config`), `anthropic.rs:352-367` (temperature/thinking control flow),
`anthropic.rs:170-193` (`serialize_mcp_tool_config`), `anthropic.rs:386-435` (combined tools +
`mcp_servers`).

**Files:**
- `sdks/typescript/src/serialize/anthropic.ts` (modify)
- `sdks/typescript/tests/serialize.anthropic.test.ts` (modify — add thinking + MCP tests)

**Steps:**

- [ ] **Step 1: Write failing tests for thinking + temperature.**
  Append this `describe` block at the END of `sdks/typescript/tests/serialize.anthropic.test.ts`
  (after the final closing `})` of the top-level `describe('serializeAnthropicRequest', ...)`):
  ```ts
  describe('serializeAnthropicRequest thinking config (M4)', () => {
    it('adaptive model emits thinking{type:adaptive,display:summarized} + output_config, no budget, no forced temperature', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        thinking: { budgetTokens: 4096 },
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4-8')
      expect(result.thinking).toEqual({ type: 'adaptive', display: 'summarized' })
      expect(result.output_config).toEqual({ effort: 'high' })
      expect('budget_tokens' in result.thinking).toBe(false)
      expect('temperature' in result).toBe(false)
    })

    it('all three adaptive literals are adaptive', () => {
      for (const m of ['claude-opus-4-8', 'claude-opus-4-7', 'claude-opus-4-6']) {
        const result = serializeAnthropicRequest(
          { messages: [{ role: 'user' as const, content: 'hi' }], thinking: { budgetTokens: 1 } },
          m,
        )
        expect(result.thinking.type).toBe('adaptive')
      }
    })

    it('enabled (non-adaptive) model emits thinking{type:enabled,budget_tokens,display:summarized} + forces temperature=1.0', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        thinking: { budgetTokens: 4096 },
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.thinking).toEqual({
        type: 'enabled',
        budget_tokens: 4096,
        display: 'summarized',
      })
      expect('output_config' in result).toBe(false)
      expect(result.temperature).toBe(1.0)
    })

    it('temperature collision: user temperature overridden to 1.0 by non-adaptive thinking', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        thinking: { budgetTokens: 2048 },
        temperature: 0.2,
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.temperature).toBe(1.0)
    })

    it('adaptive thinking does NOT override a user temperature (it is simply not applied while thinking is set)', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        thinking: { budgetTokens: 2048 },
        temperature: 0.2,
      }
      const result = serializeAnthropicRequest(req, 'claude-opus-4-8')
      // thinking branch is taken; adaptive does not force temperature, and the user temp
      // lives in the else-if that thinking skips, so temperature is absent.
      expect('temperature' in result).toBe(false)
    })

    it('thinking absent: user temperature preserved', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        temperature: 0.3,
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.temperature).toBe(0.3)
      expect('thinking' in result).toBe(false)
    })
  })
  ```
  Run:
  ```bash
  npx vitest run tests/serialize.anthropic.test.ts
  ```
  Expected: the new block FAILS (current code emits `result.thinking = {budgetTokens:4096}` and
  applies temperature in an independent `if`, so `thinking.type` is undefined and the override
  cases fail).

- [ ] **Step 2: Write failing tests for MCP serialization.**
  Append a second `describe` block at the END of the same test file:
  ```ts
  describe('serializeAnthropicRequest MCP (M4)', () => {
    it('mcp_servers body array: type/url/name with and without authorizationToken', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        mcpServers: [
          { type: 'url' as const, url: 'https://a.example/sse', name: 'a', authorizationToken: 'tok' },
          { type: 'url' as const, url: 'https://b.example/sse', name: 'b' },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.mcp_servers).toEqual([
        { type: 'url', url: 'https://a.example/sse', name: 'a', authorization_token: 'tok' },
        { type: 'url', url: 'https://b.example/sse', name: 'b' },
      ])
    })

    it('mcp_servers absent when no servers given', () => {
      const result = serializeAnthropicRequest(
        { messages: [{ role: 'user' as const, content: 'hi' }] },
        'claude-sonnet-4-6',
      )
      expect('mcp_servers' in result).toBe(false)
    })

    it('mcp_toolset items appended to tools after regular tools (all/allowed/denied snake_case)', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        tools: [{ name: 'calc', description: 'd', inputSchema: { type: 'object' } }],
        mcpToolConfigs: [
          { kind: 'all' as const, mcpServerName: 's1' },
          { kind: 'allowed' as const, mcpServerName: 's2', allowedTools: ['read', 'list'] },
          { kind: 'denied' as const, mcpServerName: 's3', deniedTools: ['rm'] },
        ],
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.tools).toHaveLength(4)
      expect(result.tools[0].name).toBe('calc')
      expect(result.tools[1]).toEqual({ type: 'mcp_toolset', mcp_server_name: 's1' })
      expect(result.tools[2]).toEqual({
        type: 'mcp_toolset',
        mcp_server_name: 's2',
        allowed_tools: ['read', 'list'],
      })
      expect(result.tools[3]).toEqual({
        type: 'mcp_toolset',
        mcp_server_name: 's3',
        denied_tools: ['rm'],
      })
    })

    it('tools is set when ONLY mcpToolConfigs present (no regular tools)', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        mcpToolConfigs: [{ kind: 'all' as const, mcpServerName: 's1' }],
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect(result.tools).toEqual([{ type: 'mcp_toolset', mcp_server_name: 's1' }])
    })

    it('tools absent when neither regular tools nor mcpToolConfigs present', () => {
      const result = serializeAnthropicRequest(
        { messages: [{ role: 'user' as const, content: 'hi' }] },
        'claude-sonnet-4-6',
      )
      expect('tools' in result).toBe(false)
    })

    it('tool_choice none deletes tools INCLUDING mcp_toolset items', () => {
      const req = {
        messages: [{ role: 'user' as const, content: 'hi' }],
        tools: [{ name: 'calc', description: 'd', inputSchema: { type: 'object' } }],
        mcpToolConfigs: [{ kind: 'all' as const, mcpServerName: 's1' }],
        toolChoice: { type: 'none' as const },
      }
      const result = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      expect('tools' in result).toBe(false)
    })
  })
  ```
  Run:
  ```bash
  npx vitest run tests/serialize.anthropic.test.ts
  ```
  Expected: the new MCP block FAILS (no `mcp_servers`, no `mcp_toolset`, and `tools` not set
  when only MCP configs are present).

- [ ] **Step 3: Add the MCP type import.**
  In `src/serialize/anthropic.ts`, replace the first line:
  ```ts
  import type { ChatRequest, ContentBlock } from '../types.js'
  ```
  with:
  ```ts
  import type { ChatRequest, ContentBlock, McpToolConfig } from '../types.js'
  ```

- [ ] **Step 4: Add the adaptive-thinking helpers + serializers at module scope.**
  In `src/serialize/anthropic.ts`, insert this block immediately AFTER the
  `const CACHE_CONTROL = { type: 'ephemeral' } as const` line (line 4) and before
  `type SerializedBlock = ...` (line 6):
  ```ts
  /**
   * Models that use ADAPTIVE thinking (Anthropic chooses the budget; the older
   * budget-token shape is rejected). Mirrors Rust `model_uses_adaptive_thinking`
   * (anthropic.rs:195-200). Opus 4.x is adaptive; 4-7 is kept though absent from
   * ANTHROPIC_MODELS, matching the Rust literal set.
   */
  const ADAPTIVE_THINKING_MODELS = new Set(['claude-opus-4-8', 'claude-opus-4-7', 'claude-opus-4-6'])

  /** Whether `model` uses adaptive thinking. Exported for the beta-header builder (Task 3). */
  export function modelUsesAdaptiveThinking(model: string): boolean {
    return ADAPTIVE_THINKING_MODELS.has(model)
  }

  /**
   * Apply thinking config onto the result body. Mirrors Rust `apply_thinking_config`
   * (anthropic.rs:202-220). Adaptive → thinking{type:adaptive,display:summarized} +
   * output_config{effort:high}, no budget_tokens. Otherwise → thinking{type:enabled,
   * budget_tokens,display:summarized}. `display:"summarized"` is unconditional.
   */
  function applyThinkingConfig(
    result: Record<string, any>,
    model: string,
    thinking: { budgetTokens: number },
  ): void {
    if (modelUsesAdaptiveThinking(model)) {
      result.thinking = { type: 'adaptive', display: 'summarized' }
      result.output_config = { effort: 'high' }
    } else {
      result.thinking = {
        type: 'enabled',
        budget_tokens: thinking.budgetTokens,
        display: 'summarized',
      }
    }
  }

  /**
   * Serialize one McpToolConfig to an Anthropic `mcp_toolset` tools-array entry.
   * Mirrors Rust `serialize_mcp_tool_config` (anthropic.rs:170-193). Wire keys are
   * snake_case.
   */
  function serializeMcpToolConfig(config: McpToolConfig): SerializedBlock {
    if (config.kind === 'all') {
      return { type: 'mcp_toolset', mcp_server_name: config.mcpServerName }
    }
    if (config.kind === 'allowed') {
      return {
        type: 'mcp_toolset',
        mcp_server_name: config.mcpServerName,
        allowed_tools: config.allowedTools,
      }
    }
    return {
      type: 'mcp_toolset',
      mcp_server_name: config.mcpServerName,
      denied_tools: config.deniedTools,
    }
  }
  ```
  Note: `SerializedBlock` is declared on the next line (line 6 in the original); function
  hoisting means `serializeMcpToolConfig`'s reference to the `SerializedBlock` TYPE resolves
  fine regardless of textual order, and the `type` alias is in scope for the whole module.

- [ ] **Step 5: Replace the tools-array assembly to combine regular tools + mcp_toolset items.**
  In `src/serialize/anthropic.ts`, replace this exact block:
  ```ts
    if (req.tools && req.tools.length > 0) {
      result.tools = req.tools.map((tool) => {
        const serialized: SerializedBlock = {
          name: tool.name,
          description: tool.description,
          input_schema: tool.inputSchema,
        }

        // Per-tool cache flag, position-independent — matches Rust
        // providers/anthropic.rs (`if tool.cache { cache_control = ... }`).
        if (tool.cache) {
          serialized.cache_control = CACHE_CONTROL
        }

        return serialized
      })
    }
  ```
  with:
  ```ts
    // Combined tools array: regular tools first, then mcp_toolset items.
    // Mirrors Rust `all_tools` assembly (anthropic.rs:386-392); body.tools is set
    // iff the combined array is non-empty (`!all_tools.is_empty()`).
    const allTools: SerializedBlock[] = []
    if (req.tools && req.tools.length > 0) {
      for (const tool of req.tools) {
        const serialized: SerializedBlock = {
          name: tool.name,
          description: tool.description,
          input_schema: tool.inputSchema,
        }
        // Per-tool cache flag, position-independent — matches Rust
        // providers/anthropic.rs (`if tool.cache { cache_control = ... }`).
        if (tool.cache) {
          serialized.cache_control = CACHE_CONTROL
        }
        allTools.push(serialized)
      }
    }
    if (req.mcpToolConfigs && req.mcpToolConfigs.length > 0) {
      for (const config of req.mcpToolConfigs) {
        allTools.push(serializeMcpToolConfig(config))
      }
    }
    if (allTools.length > 0) {
      result.tools = allTools
    }
  ```

- [ ] **Step 6: Replace the naive thinking passthrough + temperature block with the contract control flow.**
  In `src/serialize/anthropic.ts`, replace this exact block (currently the two separate `if`s):
  ```ts
    if (req.thinking) {
      result.thinking = req.thinking
    }

    if (req.stopSequences && req.stopSequences.length > 0) {
      result.stop_sequences = req.stopSequences
    }

    if (req.temperature !== undefined) {
      result.temperature = req.temperature
    }
  ```
  with:
  ```ts
    // Thinking/temperature collision. Mirrors Rust anthropic.rs:352-367:
    // when thinking is set, non-adaptive forces temperature=1.0 and the user
    // temperature is NOT applied (it lives only in the else-if branch).
    if (req.thinking) {
      if (!modelUsesAdaptiveThinking(model)) {
        result.temperature = 1.0
      }
      applyThinkingConfig(result, model, req.thinking)
    } else if (req.temperature !== undefined) {
      result.temperature = req.temperature
    }

    if (req.stopSequences && req.stopSequences.length > 0) {
      result.stop_sequences = req.stopSequences
    }
  ```
  (Note: the `model` parameter of `serializeAnthropicRequest` is the already-resolved model —
  callers pass `request.model ?? this.model`, anthropic.ts:118/163 — so adaptive detection uses
  the correct model.)

- [ ] **Step 7: Add the `mcp_servers` body key.**
  In `src/serialize/anthropic.ts`, find the block that handles `providerOptions` near the end:
  ```ts
    if (req.providerOptions && typeof req.providerOptions === 'object') {
      Object.assign(result, req.providerOptions)
    }

    return result
  ```
  Replace it with (insert the `mcp_servers` block BEFORE `providerOptions`, so provider options
  retain last-write-wins precedence, matching Rust order where provider_options is applied
  last):
  ```ts
    // mcp_servers body key. Mirrors Rust anthropic.rs:417-435. Set only when non-empty.
    if (req.mcpServers && req.mcpServers.length > 0) {
      result.mcp_servers = req.mcpServers.map((s) => {
        const obj: SerializedBlock = { type: s.type, url: s.url, name: s.name }
        if (s.authorizationToken !== undefined) {
          obj.authorization_token = s.authorizationToken
        }
        return obj
      })
    }

    if (req.providerOptions && typeof req.providerOptions === 'object') {
      Object.assign(result, req.providerOptions)
    }

    return result
  ```

  Per contract §1/§3 auto-all note: we serialize `mcpToolConfigs` EXACTLY as given (no
  serializer-side auto-`all` synthesis). A caller that sets `mcpServers` without
  `mcpToolConfigs` gets `mcp_servers` on the body but no `mcp_toolset` entries — this is the
  documented minimal-correct behavior; no extra test required.

- [ ] **Step 8: Verify all serializer tests pass.**
  ```bash
  npx vitest run tests/serialize.anthropic.test.ts
  ```
  Expected: both new `describe` blocks pass and ALL pre-existing serializer tests still pass.
  In particular the pre-existing `includes optional fields only when present` test (asserts
  `'tools' in result` false, `'thinking' in result` false, `'temperature' in result` false for
  a request with none of them) must still pass — the combined-tools path only sets `result.tools`
  when `allTools.length > 0`.

- [ ] **Step 9: Build (strict type-check).**
  ```bash
  npm run build
  ```
  Expected: `tsc` exits 0.

- [ ] **Step 10: Commit.**
  ```bash
  git add sdks/typescript/src/serialize/anthropic.ts sdks/typescript/tests/serialize.anthropic.test.ts
  git commit -m "feat(ts): adaptive thinking config + MCP serialization for anthropic"
  ```

---

### Task 3: beta headers in `providers/anthropic.ts`

Add a `buildBetaHeader` function mirroring all Rust branches (so the future OAuth phase plugs
in), and wire it into `chat()` and `streamImpl()` request headers. In M4 the x-api-key-only
path emits the `anthropic-beta` header ONLY when the request carries MCP config; otherwise no
header is sent. Imports `modelUsesAdaptiveThinking` from `../serialize/anthropic.js` (exported
by Task 2).

Source of truth: contract §4; Rust `anthropic.rs:78-108` (`build_beta_header` +
`apply_beta_header`), `anthropic.rs:466-478` (chat: has_mcp + adaptive + apply order),
`anthropic.rs:800-810` (stream: same).

**Files:**
- `sdks/typescript/src/providers/anthropic.ts` (modify)
- `sdks/typescript/tests/providers-anthropic.test.ts` (modify — add beta-header tests)

**Steps:**

- [ ] **Step 1: Write failing tests for the beta header.**
  Append this `describe` block at the END of `sdks/typescript/tests/providers-anthropic.test.ts`.
  It reuses the file's mock-fetch pattern (capture URL/headers/body via `vi.stubGlobal`). The
  existing top-level `describe('AnthropicProvider chat', ...)` sets up its own `beforeEach`
  mock; this new top-level describe defines its own so it is self-contained:
  ```ts
  describe('AnthropicProvider beta headers (M4)', () => {
    let captured: { url: string; headers: Record<string, string>; body: any } | null = null

    beforeEach(() => {
      captured = null
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string, options?: RequestInit) => {
          captured = {
            url,
            headers: (options?.headers as Record<string, string>) ?? {},
            body: options?.body ? JSON.parse(String(options.body)) : null,
          }
          return new Response(
            JSON.stringify({
              id: 'msg_x',
              type: 'message',
              role: 'assistant',
              content: [{ type: 'text', text: 'ok' }],
              model: 'claude-sonnet-4-6',
              stop_reason: 'end_turn',
              usage: { input_tokens: 1, output_tokens: 1 },
            }),
            { status: 200, headers: { 'content-type': 'application/json' } },
          )
        }),
      )
    })

    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('adds anthropic-beta: mcp-client-2025-11-20 when mcpServers present', async () => {
      const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
      await provider.chat({
        messages: [{ role: 'user', content: 'hi' }],
        mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      })
      expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
    })

    it('adds anthropic-beta when only mcpToolConfigs present', async () => {
      const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
      await provider.chat({
        messages: [{ role: 'user', content: 'hi' }],
        mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
      })
      expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
    })

    it('omits anthropic-beta when no MCP config (even with thinking)', async () => {
      const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
      await provider.chat({
        messages: [{ role: 'user', content: 'hi' }],
        thinking: { budgetTokens: 1024 },
      })
      expect('anthropic-beta' in (captured?.headers ?? {})).toBe(false)
    })

    it('omits anthropic-beta for a plain request', async () => {
      const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
      await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect('anthropic-beta' in (captured?.headers ?? {})).toBe(false)
    })

    it('streamImpl also adds the MCP beta header', async () => {
      // Return an empty SSE body so the generator completes without events.
      vi.stubGlobal(
        'fetch',
        vi.fn(async (url: string, options?: RequestInit) => {
          captured = {
            url,
            headers: (options?.headers as Record<string, string>) ?? {},
            body: options?.body ? JSON.parse(String(options.body)) : null,
          }
          return new Response('', {
            status: 200,
            headers: { 'content-type': 'text/event-stream' },
          })
        }),
      )
      const provider = new AnthropicProvider('k', 'claude-sonnet-4-6')
      const events: StreamEvent[] = []
      for await (const e of provider.stream({
        messages: [{ role: 'user', content: 'hi' }],
        mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      })) {
        events.push(e)
      }
      expect(captured?.headers['anthropic-beta']).toBe('mcp-client-2025-11-20')
    })
  })
  ```
  Ensure the file's top import line includes `beforeEach`, `afterEach`, and `vi` (it already
  does — line 1: `import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'`) and
  `StreamEvent` (line 3 already imports `ChatRequest, StreamEvent`).

  Run:
  ```bash
  npx vitest run tests/providers-anthropic.test.ts
  ```
  Expected: the new block FAILS — current `headers()` never emits `anthropic-beta`, so the
  "present" assertions fail.

- [ ] **Step 2: Import the adaptive-thinking helper.**
  In `src/providers/anthropic.ts`, replace:
  ```ts
  import { serializeAnthropicRequest } from '../serialize/anthropic.js'
  ```
  with:
  ```ts
  import { modelUsesAdaptiveThinking, serializeAnthropicRequest } from '../serialize/anthropic.js'
  ```

- [ ] **Step 3: Add `buildBetaHeader` (module-scope, exported for testability/reuse).**
  In `src/providers/anthropic.ts`, insert this function immediately AFTER the
  `const ANTHROPIC_VERSION = '2023-06-01'` line (line 32):
  ```ts
  /**
   * Build the `anthropic-beta` header value. Mirrors Rust `build_beta_header`
   * (anthropic.rs:78-99): comma-joined (no spaces), `undefined` when empty.
   * The OAuth betas are wired for the future setup-token path; in M4 callers
   * pass `isOauth=false`, so only the MCP beta can appear on the x-api-key path.
   */
  export function buildBetaHeader(
    hasMcp: boolean,
    isOauth: boolean,
    adaptiveThinking: boolean,
  ): string | undefined {
    const betas: string[] = []
    if (isOauth) {
      betas.push('claude-code-20250219')
      betas.push('oauth-2025-04-20')
      betas.push('fine-grained-tool-streaming-2025-05-14')
      if (!adaptiveThinking) {
        betas.push('interleaved-thinking-2025-05-14')
      }
    }
    if (hasMcp) {
      betas.push('mcp-client-2025-11-20')
    }
    return betas.length === 0 ? undefined : betas.join(',')
  }

  /** Whether a request carries any MCP config. Mirrors Rust anthropic.rs:466-467. */
  function requestHasMcp(req: ChatRequest): boolean {
    return (req.mcpServers?.length ?? 0) > 0 || (req.mcpToolConfigs?.length ?? 0) > 0
  }
  ```

- [ ] **Step 4: Make `headers()` accept optional per-request extras.**
  In `src/providers/anthropic.ts`, replace the `headers()` method:
  ```ts
    private headers(): Record<string, string> {
      return {
        'x-api-key': this.apiKey,
        'anthropic-version': ANTHROPIC_VERSION,
        'content-type': 'application/json',
      }
    }
  ```
  with:
  ```ts
    private headers(extra?: Record<string, string>): Record<string, string> {
      return {
        'x-api-key': this.apiKey,
        'anthropic-version': ANTHROPIC_VERSION,
        'content-type': 'application/json',
        ...(extra ?? {}),
      }
    }

    /**
     * Build per-request headers including the beta header when applicable.
     * isOauth is always false in M4 (no setup-token path in TS).
     * adaptiveThinking is read off the serialized body, matching Rust
     * (anthropic.rs:469/802) — it only affects the OAuth-only interleaved beta.
     */
    private requestHeaders(req: ChatRequest, body: Record<string, any>): Record<string, string> {
      const hasMcp = requestHasMcp(req)
      const adaptiveThinking = body?.thinking?.type === 'adaptive'
      const beta = buildBetaHeader(hasMcp, false, adaptiveThinking)
      return this.headers(beta ? { 'anthropic-beta': beta } : {})
    }
  ```

- [ ] **Step 5: Wire `requestHeaders` into `chat()`.**
  In `src/providers/anthropic.ts`, replace the start of `chat()`:
  ```ts
    async chat(request: ChatRequest): Promise<ChatResponse> {
      const body = serializeAnthropicRequest(request, request.model ?? this.model)
      const payload = await withRetry(
        this.retryPolicy,
        async () => postJson<any>(`${this.baseUrl}/v1/messages`, this.headers(), body),
        classifyHttpError,
      )
  ```
  with:
  ```ts
    async chat(request: ChatRequest): Promise<ChatResponse> {
      const body = serializeAnthropicRequest(request, request.model ?? this.model)
      const headers = this.requestHeaders(request, body)
      const payload = await withRetry(
        this.retryPolicy,
        async () => postJson<any>(`${this.baseUrl}/v1/messages`, headers, body),
        classifyHttpError,
      )
  ```

- [ ] **Step 6: Wire `requestHeaders` into `streamImpl()`.**
  In `src/providers/anthropic.ts`, replace the start of `streamImpl()` through the `postStream`
  call. Replace:
  ```ts
    private async *streamImpl(request: ChatRequest) {
      const body = {
        ...serializeAnthropicRequest(request, request.model ?? this.model),
        stream: true,
      }
      let attempt = 0
      let responseBody: ReadableStream<Uint8Array>
      while (true) {
        try {
          responseBody = await postStream(`${this.baseUrl}/v1/messages`, this.headers(), body)
          break
  ```
  with:
  ```ts
    private async *streamImpl(request: ChatRequest) {
      const body = {
        ...serializeAnthropicRequest(request, request.model ?? this.model),
        stream: true,
      }
      const headers = this.requestHeaders(request, body)
      let attempt = 0
      let responseBody: ReadableStream<Uint8Array>
      while (true) {
        try {
          responseBody = await postStream(`${this.baseUrl}/v1/messages`, headers, body)
          break
  ```

- [ ] **Step 7: Verify the anthropic provider tests pass.**
  ```bash
  npx vitest run tests/providers-anthropic.test.ts
  ```
  Expected: the new `beta headers (M4)` block passes (5 new `it`s green) AND all pre-existing
  anthropic provider tests still pass (the existing header assertions check `x-api-key`,
  `anthropic-version`, `content-type` — all preserved by `headers(extra)`; no `anthropic-beta`
  is added for plain requests).

- [ ] **Step 8: Build (strict type-check).**
  ```bash
  npm run build
  ```
  Expected: `tsc` exits 0. (Confirms `modelUsesAdaptiveThinking` is importable from Task 2's
  export — even though M4's body-derived adaptive read makes it unused HERE, the import is
  contract-mandated; if strict `noUnusedLocals` flags it, prefer reading adaptive off the body
  as written and drop the import. Verify which by building.)

  IMPLEMENTATION NOTE: the contract (§4) says to import `modelUsesAdaptiveThinking`, but the
  faithful Rust port reads `adaptive_thinking` OFF THE BUILT BODY (`body["thinking"]["type"]`),
  which is what Step 4 does (`body?.thinking?.type === 'adaptive'`) — no call to the imported
  helper is needed at the header site. If `npm run build` errors with `'modelUsesAdaptiveThinking'
  is declared but its value is never read`, DELETE the import added in Step 2 (revert to
  `import { serializeAnthropicRequest } from '../serialize/anthropic.js'`) and rebuild. The
  body-derived read is the authoritative behavior and matches Rust exactly.

- [ ] **Step 9: Commit.**
  ```bash
  git add sdks/typescript/src/providers/anthropic.ts sdks/typescript/tests/providers-anthropic.test.ts
  git commit -m "feat(ts): anthropic-beta header for MCP requests"
  ```

---

### Task 4: MiniMax Anthropic-compat re-route + `minimaxBaseUrl` builder

Rewrite `MinimaxProvider` to the Anthropic wire and rewire the client to feed it a BASE URL
(not a legacy full endpoint). Per contract §5 we use the thin-delegation shape (option b): a
`MinimaxProvider` that holds an internal `AnthropicProvider` and overrides only `capabilities()`
to `textOnly()`. This inherits the full Anthropic wire — `serializeAnthropicRequest`, POST
`{base}/v1/messages`, `x-api-key` auth, `anthropic-version`, Anthropic chat/SSE parsing, retry,
and the beta headers from Task 3 — for free, exactly mirroring Rust's
`build_minimax_provider` (`client.rs:567-586`), which constructs an `AnthropicProvider` with
text-only caps.

Source of truth: contract §5; Rust `client.rs:567-586` (`build_minimax_provider`),
`anthropic.rs:59-76` (endpoint + x-api-key auth — MiniMax key is NOT a setup token).

**Files:**
- `sdks/typescript/src/providers/minimax.ts` (full rewrite)
- `sdks/typescript/src/client.ts` (builder method + buildProvider minimax arm + legacy
  constructor minimax arm)
- `sdks/typescript/tests/providers-minimax.test.ts` (full rewrite — Anthropic-wire assertions)

**Steps:**

- [ ] **Step 1: Rewrite the test to assert the Anthropic wire.**
  Replace the ENTIRE contents of `sdks/typescript/tests/providers-minimax.test.ts` with:
  ```ts
  import { afterEach, describe, expect, it, vi } from 'vitest'
  import { DEFAULT_MINIMAX_MODEL } from '../src/models.js'
  import { MinimaxProvider } from '../src/providers/minimax.js'
  import { textOnly } from '../src/provider.js'
  import type { ChatRequest } from '../src/types.js'

  function anthropicResponse(model = DEFAULT_MINIMAX_MODEL) {
    return new Response(
      JSON.stringify({
        id: 'msg_1',
        type: 'message',
        role: 'assistant',
        content: [{ type: 'text', text: 'ok' }],
        model,
        stop_reason: 'end_turn',
        usage: { input_tokens: 10, output_tokens: 5 },
      }),
      { status: 200, headers: { 'content-type': 'application/json' } },
    )
  }

  describe('MinimaxProvider (Anthropic-compat wire)', () => {
    afterEach(() => {
      vi.unstubAllGlobals()
    })

    it('posts an Anthropic-wire body to the default {base}/v1/messages', async () => {
      let url = ''
      let headers: Record<string, string> = {}
      let body: any
      vi.stubGlobal(
        'fetch',
        vi.fn(async (u: string, options?: RequestInit) => {
          url = u
          headers = (options?.headers as Record<string, string>) ?? {}
          body = JSON.parse(String(options?.body ?? '{}'))
          return anthropicResponse()
        }),
      )

      const provider = new MinimaxProvider('mm-key')
      const request: ChatRequest = {
        messages: [{ role: 'user', content: 'What is 2+2?' }],
        system: 'You are helpful.',
        maxTokens: 256,
      }
      const response = await provider.chat(request)

      // Default URL is the Anthropic-compat base + /v1/messages, NOT the legacy endpoint.
      expect(url).toBe('https://api.minimax.io/anthropic/v1/messages')
      expect(url).not.toContain('chatcompletion_v2')

      // x-api-key auth (NOT Authorization: Bearer), plus anthropic-version.
      expect(headers['x-api-key']).toBe('mm-key')
      expect('authorization' in headers).toBe(false)
      expect(headers['anthropic-version']).toBe('2023-06-01')

      // Anthropic wire body: top-level `system` string, default model, messages array.
      expect(body.model).toBe(DEFAULT_MINIMAX_MODEL)
      expect(body.system).toBe('You are helpful.')
      expect(body.max_tokens).toBe(256)
      expect(body.messages[0]).toEqual({ role: 'user', content: 'What is 2+2?' })

      // Anthropic-style response parse.
      expect(response.content).toBe('ok')
      expect(response.stopReason).toBe('end_turn')
      expect(response.usage.inputTokens).toBe(10)
      expect(response.usage.outputTokens).toBe(5)
    })

    it('default model is MiniMax-M2.7', async () => {
      let body: any
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_u: string, options?: RequestInit) => {
          body = JSON.parse(String(options?.body ?? '{}'))
          return anthropicResponse()
        }),
      )
      await new MinimaxProvider('mm-key').chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(body.model).toBe('MiniMax-M2.7')
      expect(DEFAULT_MINIMAX_MODEL).toBe('MiniMax-M2.7')
    })

    it('respects a custom base URL → {custom}/v1/messages', async () => {
      let url = ''
      vi.stubGlobal(
        'fetch',
        vi.fn(async (u: string) => {
          url = u
          return anthropicResponse()
        }),
      )
      const provider = new MinimaxProvider('mm-key', 'MiniMax-M2.7', 'https://proxy.example/anthropic')
      await provider.chat({ messages: [{ role: 'user', content: 'hi' }] })
      expect(url).toBe('https://proxy.example/anthropic/v1/messages')
    })

    it('reports text-only capabilities', () => {
      const provider = new MinimaxProvider('mm-key')
      expect(provider.capabilities()).toEqual(textOnly())
    })

    it('streams via the Anthropic SSE adapter and posts stream:true', async () => {
      let body: any
      const sse =
        'event: message_start\ndata: {"message":{"usage":{"input_tokens":1,"output_tokens":0}}}\n\n' +
        'event: content_block_delta\ndata: {"delta":{"type":"text_delta","text":"hello"}}\n\n' +
        'event: message_stop\ndata: {}\n\n'
      vi.stubGlobal(
        'fetch',
        vi.fn(async (_u: string, options?: RequestInit) => {
          body = JSON.parse(String(options?.body ?? '{}'))
          return new Response(sse, {
            status: 200,
            headers: { 'content-type': 'text/event-stream' },
          })
        }),
      )
      const provider = new MinimaxProvider('mm-key')
      const texts: string[] = []
      for await (const e of provider.stream({ messages: [{ role: 'user', content: 'hi' }] })) {
        if (e.eventType === 'text' && e.content) texts.push(e.content)
      }
      expect(body.stream).toBe(true)
      expect(texts.join('')).toBe('hello')
    })
  })

  // Env-gated live test — skipped unless MINIMAX_API_KEY is set.
  const liveKey = process.env.MINIMAX_API_KEY
  const liveDescribe = liveKey ? describe : describe.skip
  liveDescribe('MinimaxProvider live', () => {
    it('chats against the real MiniMax Anthropic-compat endpoint', async () => {
      const provider = new MinimaxProvider(liveKey as string)
      const response = await provider.chat({
        messages: [{ role: 'user', content: 'Reply with the single word: pong' }],
        maxTokens: 16,
      })
      expect(typeof response.content).toBe('string')
      expect(response.content.length).toBeGreaterThan(0)
    })
  })
  ```
  Run (will fail to compile/run — current MinimaxProvider uses the OpenAI wire and legacy
  endpoint):
  ```bash
  npx vitest run tests/providers-minimax.test.ts
  ```
  Expected: FAIL (legacy URL, Bearer auth, OpenAI body shape).

- [ ] **Step 2: Rewrite `src/providers/minimax.ts` as a thin Anthropic delegate.**
  Replace the ENTIRE contents of `sdks/typescript/src/providers/minimax.ts` with:
  ```ts
  /**
   * MiniMax provider. MiniMax exposes an Anthropic-compatible endpoint, so this
   * is a thin delegate over an internal AnthropicProvider with text-only caps —
   * mirroring Rust `build_minimax_provider` (client.rs:567-586), which builds an
   * AnthropicProvider with `ProviderCapabilities::text_only()`.
   *
   * Wire: serializeAnthropicRequest → POST {base}/v1/messages, x-api-key auth,
   * anthropic-version, Anthropic chat/SSE parsing — all inherited from
   * AnthropicProvider. The constructor's `baseUrl` is the BASE (the final URL is
   * `{base}/v1/messages`); default base `https://api.minimax.io/anthropic`
   * (Rust client.rs:577).
   */
  import { DEFAULT_MINIMAX_MODEL } from '../models.js'
  import { textOnly, type ProviderCapabilities } from '../provider.js'
  import { RetryPolicy } from '../retry.js'
  import type { BoxStream } from '../stream.js'
  import type { ChatRequest, ChatResponse } from '../types.js'
  import { AnthropicProvider } from './anthropic.js'

  /** Default MiniMax Anthropic-compatible base URL (Rust client.rs:577). */
  export const DEFAULT_MINIMAX_BASE_URL = 'https://api.minimax.io/anthropic'

  export class MinimaxProvider {
    private readonly inner: AnthropicProvider

    constructor(apiKey: string, model?: string, baseUrl?: string) {
      this.inner = new AnthropicProvider(
        apiKey,
        model ?? DEFAULT_MINIMAX_MODEL,
        baseUrl ?? DEFAULT_MINIMAX_BASE_URL,
      )
    }

    withRetryPolicy(policy: RetryPolicy): this {
      this.inner.withRetryPolicy(policy)
      return this
    }

    chat(request: ChatRequest): Promise<ChatResponse> {
      return this.inner.chat(request)
    }

    stream(request: ChatRequest): BoxStream {
      return this.inner.stream(request)
    }

    /** Text-only — images/documents rejected by validateRequest before any HTTP call. */
    capabilities(): ProviderCapabilities {
      return textOnly()
    }
  }
  ```
  Notes:
  - `AnthropicProvider.withRetryPolicy` returns `this` (the inner instance) and mutates in
    place (anthropic.ts:104-107); we ignore its return and return the MinimaxProvider `this` so
    chaining stays on the MiniMax type.
  - Default model is `MiniMax-M2.7` via `DEFAULT_MINIMAX_MODEL` (models.ts:28).
  - x-api-key auth + `anthropic-version` come from AnthropicProvider.headers(); MiniMax keys are
    not setup tokens, so the x-api-key path is correct (contract §5; Rust anthropic.rs:63-76).
    The legacy `Authorization: Bearer` and `chatcompletion_v2` endpoint are GONE.

- [ ] **Step 3: Verify the MiniMax provider tests pass.**
  ```bash
  npx vitest run tests/providers-minimax.test.ts
  ```
  Expected: the mocked block passes (5 `it`s green); the live block is SKIPPED (no
  `MINIMAX_API_KEY`).

- [ ] **Step 4: Rewire the client builder field + arm to a BASE URL.**
  In `sdks/typescript/src/client.ts`, rename the builder field and method from the legacy
  `minimaxEndpoint` to `minimaxBaseUrl` (the value is the Anthropic-compat BASE; `{base}/v1/
  messages` is the final URL, matching Rust `minimax_base_url`).

  4a. Replace the protected field declaration:
  ```ts
    protected _minimaxEndpoint?: string
  ```
  with:
  ```ts
    protected _minimaxBaseUrl?: string
  ```

  4b. Replace the builder method:
  ```ts
    minimaxEndpoint(u: string): this {
      this._minimaxEndpoint = u
      return this
    }
  ```
  with:
  ```ts
    /** MiniMax Anthropic-compat BASE URL; the final URL is `{base}/v1/messages`. */
    minimaxBaseUrl(u: string): this {
      this._minimaxBaseUrl = u
      return this
    }
  ```

  4c. Replace the `buildProvider` minimax arm (the `return new MinimaxProvider(...)` near the
  end of `buildProvider`):
  ```ts
      return new MinimaxProvider(apiKey, this._model, this._minimaxEndpoint).withRetryPolicy(
        this._retryPolicy,
      )
  ```
  with:
  ```ts
      return new MinimaxProvider(apiKey, this._model, this._minimaxBaseUrl).withRetryPolicy(
        this._retryPolicy,
      )
  ```

- [ ] **Step 5: Rewire the legacy options-object constructor minimax arm.**
  In `sdks/typescript/src/client.ts`, the legacy constructor accepts `opts.minimaxEndpoint`.
  Rename that option to `minimaxBaseUrl` for consistency.

  5a. Replace BOTH occurrences of the inline options type (there are two: the constructor
  parameter type and the `opts` cast). Each looks like:
  ```ts
        {
          provider: ProviderName | ProviderLike
          apiKey?: string
          model?: string
          minimaxEndpoint?: string
        }
  ```
  Replace each occurrence's `minimaxEndpoint?: string` line with `minimaxBaseUrl?: string`.
  (Use `replace_all` on the exact line `          minimaxEndpoint?: string` → `          minimaxBaseUrl?: string`; both the constructor signature and the `opts` cast share that
  indentation.)

  5b. Replace the legacy minimax construction:
  ```ts
      } else {
        this.provider = new MinimaxProvider(apiKey, opts.model, opts.minimaxEndpoint)
      }
  ```
  with:
  ```ts
      } else {
        this.provider = new MinimaxProvider(apiKey, opts.model, opts.minimaxBaseUrl)
      }
  ```

- [ ] **Step 6: Update any references to the renamed field/option in client tests.**
  ```bash
  grep -rn "minimaxEndpoint" sdks/typescript/tests sdks/typescript/src
  ```
  Expected after the rewrite: zero matches in `src/`. If `tests/client-builder.test.ts` (or
  another test) references `.minimaxEndpoint(` or `minimaxEndpoint:`, update those call sites to
  `.minimaxBaseUrl(` / `minimaxBaseUrl:` AND, if the test asserted the legacy
  `chatcompletion_v2` URL or `Authorization: Bearer`, update the expectation to
  `{base}/v1/messages` and `x-api-key`. (Only touch the minimax-specific assertions; leave
  unrelated builder tests alone. These test files are not owned by another M4 task.)

- [ ] **Step 7: Run the full test suite (catches client/builder fallout).**
  ```bash
  npx vitest run
  ```
  Expected: all green. Pay attention to `tests/client-builder.test.ts`,
  `tests/client.test.ts`, and `tests/integration.openai-minimax.test.ts` — fix any
  minimax-endpoint references there per Step 6.

- [ ] **Step 8: Build (strict type-check).**
  ```bash
  npm run build
  ```
  Expected: `tsc` exits 0.

- [ ] **Step 9: Commit.**
  ```bash
  git add sdks/typescript/src/providers/minimax.ts sdks/typescript/src/client.ts sdks/typescript/tests/providers-minimax.test.ts
  # plus any client test files touched in Step 6
  git commit -m "feat(ts)!: route minimax through anthropic-compat wire with minimaxBaseUrl"
  ```

---

### Task 5: MCP rejection on non-Anthropic providers

Add a `supportsMcp` flag to `ProviderCapabilities` and have `validateRequest` throw
`UnsupportedFeatureError` when a request carries MCP config but the provider does not support
MCP. Set capabilities per provider: Anthropic and MiniMax (Anthropic-compat wire) support MCP;
OpenAI does not. This DIVERGES from Rust (Rust has no MCP capability flag and OpenAI silently
ignores MCP via serializer omission) — a deliberate TS-specific design to satisfy the M4
Done-when requirement.

Source of truth: contract §6; existing `provider.ts:17-71` (caps + validateRequest);
`error.ts:31-35` (`UnsupportedFeatureError`).

**Files:**
- `sdks/typescript/src/provider.ts` (modify — caps interface, factories, validateRequest)
- `sdks/typescript/src/providers/anthropic.ts` (modify — `capabilities()` keeps fullCaps which
  now includes `supportsMcp:true`; no change needed beyond confirming fullCaps. See Step 3.)
- `sdks/typescript/src/providers/minimax.ts` (modify — `capabilities()` must return MCP-capable
  text-only caps)
- `sdks/typescript/src/providers/openai.ts` (no change — uses `withImage()`, which sets
  `supportsMcp:false`)
- `sdks/typescript/tests/capabilities.test.ts` (modify — add MCP rejection/pass tests)

DEPENDENCY NOTE: this task reads `req.mcpServers` / `req.mcpToolConfigs` (added by Task 1) and,
for MiniMax, coordinates with Task 4's `MinimaxProvider`. If executed before Task 4 lands,
MiniMax still has its own `capabilities()` method to edit — but Task 4 rewrites that file. To
avoid a collision, this task touches ONLY the `capabilities()` return inside `minimax.ts`. If
Task 4 already shipped the thin-delegate MiniMax (whose `capabilities()` returns `textOnly()`),
change that one return to `minimaxCaps()` per Step 4.

**Steps:**

- [ ] **Step 1: Write failing tests.**
  In `sdks/typescript/tests/capabilities.test.ts`, extend the factory tests and add an MCP
  validation block.

  1a. Replace the existing `ProviderCapabilities factories` describe body to assert the new
  flag. Replace:
  ```ts
  describe('ProviderCapabilities factories', () => {
    it('textOnly() returns {false, false}', () => {
      const caps = textOnly()
      expect(caps.supportsImage).toBe(false)
      expect(caps.supportsDocument).toBe(false)
    })

    it('withImage() returns {true, false}', () => {
      const caps = withImage()
      expect(caps.supportsImage).toBe(true)
      expect(caps.supportsDocument).toBe(false)
    })

    it('fullCaps() returns {true, true}', () => {
      const caps = fullCaps()
      expect(caps.supportsImage).toBe(true)
      expect(caps.supportsDocument).toBe(true)
    })
  })
  ```
  with:
  ```ts
  describe('ProviderCapabilities factories', () => {
    it('textOnly() returns {false, false, supportsMcp:false}', () => {
      expect(textOnly()).toEqual({
        supportsImage: false,
        supportsDocument: false,
        supportsMcp: false,
      })
    })

    it('withImage() returns {true, false, supportsMcp:false}', () => {
      expect(withImage()).toEqual({
        supportsImage: true,
        supportsDocument: false,
        supportsMcp: false,
      })
    })

    it('fullCaps() returns {true, true, supportsMcp:true}', () => {
      expect(fullCaps()).toEqual({
        supportsImage: true,
        supportsDocument: true,
        supportsMcp: true,
      })
    })

    it('minimaxCaps() returns text-only but MCP-capable', () => {
      expect(minimaxCaps()).toEqual({
        supportsImage: false,
        supportsDocument: false,
        supportsMcp: true,
      })
    })
  })
  ```

  1b. Update the top import to pull in `minimaxCaps`. Replace:
  ```ts
  import {
    textOnly,
    withImage,
    fullCaps,
    validateRequest,
    type ProviderCapabilities,
  } from '../src/provider.js'
  ```
  with:
  ```ts
  import {
    textOnly,
    withImage,
    fullCaps,
    minimaxCaps,
    validateRequest,
    type ProviderCapabilities,
  } from '../src/provider.js'
  ```

  1c. Append a new describe block at the END of the file:
  ```ts
  describe('validateRequest MCP gating (M4)', () => {
    const mcpReq: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
    }
    const mcpToolReq: ChatRequest = {
      messages: [{ role: 'user', content: 'hi' }],
      mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
    }

    it('throws UnsupportedFeatureError when mcpServers set and caps lack MCP (openai/withImage)', () => {
      expect(() => validateRequest(mcpReq, withImage())).toThrow(UnsupportedFeatureError)
      expect(() => validateRequest(mcpReq, withImage())).toThrow(
        'provider does not support MCP server config',
      )
    })

    it('throws when mcpToolConfigs set and caps lack MCP (textOnly)', () => {
      expect(() => validateRequest(mcpToolReq, textOnly())).toThrow(UnsupportedFeatureError)
    })

    it('passes MCP config for MCP-capable caps (fullCaps / anthropic)', () => {
      expect(() => validateRequest(mcpReq, fullCaps())).not.toThrow()
      expect(() => validateRequest(mcpToolReq, fullCaps())).not.toThrow()
    })

    it('passes MCP config for minimaxCaps (text-only but MCP-capable)', () => {
      expect(() => validateRequest(mcpReq, minimaxCaps())).not.toThrow()
    })

    it('does NOT throw when no MCP config is present (textOnly)', () => {
      const plain: ChatRequest = { messages: [{ role: 'user', content: 'hi' }] }
      expect(() => validateRequest(plain, textOnly())).not.toThrow()
    })
  })
  ```
  Run:
  ```bash
  npx vitest run tests/capabilities.test.ts
  ```
  Expected: FAIL — `minimaxCaps` is not exported, the factories don't include `supportsMcp`, and
  `validateRequest` doesn't reject MCP.

- [ ] **Step 2: Add `supportsMcp` to the caps interface + extend factories + add `minimaxCaps`.**
  In `sdks/typescript/src/provider.ts`, replace the interface:
  ```ts
  export interface ProviderCapabilities {
    supportsImage: boolean
    supportsDocument: boolean
  }
  ```
  with:
  ```ts
  /**
   * Describes what features a provider supports.
   *
   * Mirrors Rust `ProviderCapabilities` (types.rs:903-907) PLUS a TS-only
   * `supportsMcp` flag. Rust has no MCP capability — it achieves "no MCP on
   * OpenAI" by serializer omission. TS adds the flag so validateRequest can
   * reject MCP on non-Anthropic providers per the M4 Done-when requirement.
   */
  export interface ProviderCapabilities {
    supportsImage: boolean
    supportsDocument: boolean
    /** TS-only divergence from Rust: whether the provider accepts MCP server/tool config. */
    supportsMcp: boolean
  }
  ```

  Then replace the three factory functions:
  ```ts
  export function textOnly(): ProviderCapabilities {
    return { supportsImage: false, supportsDocument: false }
  }
  ```
  →
  ```ts
  export function textOnly(): ProviderCapabilities {
    return { supportsImage: false, supportsDocument: false, supportsMcp: false }
  }
  ```
  ```ts
  export function withImage(): ProviderCapabilities {
    return { supportsImage: true, supportsDocument: false }
  }
  ```
  →
  ```ts
  export function withImage(): ProviderCapabilities {
    return { supportsImage: true, supportsDocument: false, supportsMcp: false }
  }
  ```
  ```ts
  export function fullCaps(): ProviderCapabilities {
    return { supportsImage: true, supportsDocument: true }
  }
  ```
  →
  ```ts
  export function fullCaps(): ProviderCapabilities {
    return { supportsImage: true, supportsDocument: true, supportsMcp: true }
  }
  ```

  Then add `minimaxCaps` immediately AFTER `fullCaps`:
  ```ts
  /**
   * MiniMax capabilities: text-only (no images/documents) but MCP-capable,
   * because MiniMax routes through the Anthropic-compatible wire (contract §5/§6).
   * Distinct from `textOnly()` so MCP isn't blanket-blocked on the text-only path.
   */
  export function minimaxCaps(): ProviderCapabilities {
    return { supportsImage: false, supportsDocument: false, supportsMcp: true }
  }
  ```

- [ ] **Step 3: Extend `validateRequest` to reject MCP when unsupported.**
  In `sdks/typescript/src/provider.ts`, replace the `validateRequest` body:
  ```ts
  export function validateRequest(req: ChatRequest, caps: ProviderCapabilities): void {
    for (const msg of req.messages) {
      const blocks: ContentBlock[] = msg.contentBlocks ?? []
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
  ```
  with:
  ```ts
  export function validateRequest(req: ChatRequest, caps: ProviderCapabilities): void {
    for (const msg of req.messages) {
      const blocks: ContentBlock[] = msg.contentBlocks ?? []
      for (const block of blocks) {
        if (block.type === 'image' && !caps.supportsImage) {
          throw new UnsupportedFeatureError('provider does not support image input')
        }
        if (block.type === 'document' && !caps.supportsDocument) {
          throw new UnsupportedFeatureError('provider does not support document input')
        }
      }
    }

    // TS-only MCP gating (contract §6): reject MCP config on non-MCP providers.
    const hasMcp = (req.mcpServers?.length ?? 0) > 0 || (req.mcpToolConfigs?.length ?? 0) > 0
    if (hasMcp && !caps.supportsMcp) {
      throw new UnsupportedFeatureError('provider does not support MCP server config')
    }
  }
  ```
  Note: Anthropic's `capabilities()` returns `fullCaps()` (anthropic.ts:293-295), which now
  carries `supportsMcp:true` — no change needed in `providers/anthropic.ts`. OpenAI uses
  `withImage()` (`supportsMcp:false`) — no change needed there either.

- [ ] **Step 4: Set MiniMax capabilities to `minimaxCaps()`.**
  In `sdks/typescript/src/providers/minimax.ts`, change the `capabilities()` return. After
  Task 4 the method reads:
  ```ts
    capabilities(): ProviderCapabilities {
      return textOnly()
    }
  ```
  Replace with:
  ```ts
    capabilities(): ProviderCapabilities {
      return minimaxCaps()
    }
  ```
  And update the import in `minimax.ts` from:
  ```ts
  import { textOnly, type ProviderCapabilities } from '../provider.js'
  ```
  to:
  ```ts
  import { minimaxCaps, type ProviderCapabilities } from '../provider.js'
  ```
  (`textOnly` is no longer referenced in `minimax.ts` once `capabilities()` uses `minimaxCaps`.
  If Task 4 has NOT yet landed and the legacy MinimaxProvider is still present, its
  `capabilities()` also returns `textOnly()` — apply the same `minimaxCaps()` swap there. Either
  way, only the `capabilities()` return + its import change in this file.)

- [ ] **Step 5: Fix every exact-shape caps assertion across src AND tests (the shape-change blast radius).**
  Adding a required field breaks both `{ supportsImage, supportsDocument }` literal construction
  sites AND any vitest `toEqual({ supportsImage..., supportsDocument... })` assertion (tests are
  not tsc-checked, so these fail only at runtime in the final gate — grep them now). Scan BOTH trees:
  ```bash
  grep -rn "supportsImage" sdks/typescript/src sdks/typescript/tests
  ```
  Handle each hit:
  - `src/provider.ts` — the factories (already updated in Step 2). No further change.
  - `client.ts` `asDispatchProvider` fallback uses `capabilities: textOnly` (a function reference,
    not a literal) — unaffected.
  - **`tests/providers-anthropic.test.ts:168`** asserts the EXACT shape of `AnthropicProvider.capabilities()`,
    which returns `fullCaps()` (now `supportsMcp:true`). This task owns the shape change, so fix it here:
    ```typescript
    // tests/providers-anthropic.test.ts — was:
    //   expect(provider.capabilities()).toEqual({ supportsImage: true, supportsDocument: true })
    expect(provider.capabilities()).toEqual({
      supportsImage: true,
      supportsDocument: true,
      supportsMcp: true,
    })
    ```
  - `tests/client-builder.test.ts:140,302` return inline `{ supportsImage, supportsDocument }` mock
    literals to `validateRequest` (which reads `supportsMcp` as `undefined`→falsy on non-MCP tests).
    Tests are not tsc-checked, so these are NOT broken — leave them.
  - `tests/index.test.ts` exact-shape factory assertions are owned and fixed by Task 6 Step 3 (which
    runs last). Note them, do not edit here.
  - Any OTHER `{ supportsImage, supportsDocument }` literal → add `supportsMcp: false` minimally.

- [ ] **Step 6: Verify the capabilities tests pass.**
  ```bash
  npx vitest run tests/capabilities.test.ts
  ```
  Expected: all green, including the new MCP-gating block and updated factory tests.

- [ ] **Step 7: Run the full suite (the new required field may surface in other tests).**
  ```bash
  npx vitest run
  ```
  Expected: all green. If `tests/index.test.ts` asserts the exact caps object shape
  (`{ supportsImage:..., supportsDocument:... }`), that is owned/updated by Task 6 — if running
  Task 5 standalone before Task 6, that one assertion may fail; note it for Task 6 and proceed.

- [ ] **Step 8: Build (strict type-check).**
  ```bash
  npm run build
  ```
  Expected: `tsc` exits 0.

- [ ] **Step 9: Commit.**
  ```bash
  git add sdks/typescript/src/provider.ts sdks/typescript/src/providers/minimax.ts sdks/typescript/tests/capabilities.test.ts sdks/typescript/tests/providers-anthropic.test.ts
  git commit -m "feat(ts): reject MCP config on providers without supportsMcp"
  ```

---

### Task 6: index.ts exports + M4 done-criteria smoke test

Explicitly export the three MCP types from the package entrypoint and add a smoke test proving
the end-to-end M4 contract: a `ChatRequest` with `mcpServers` + `thinking` serializes correctly
for Anthropic and is rejected for OpenAI. Also fix the `index.test.ts` caps-shape assertions to
include the new `supportsMcp` field (Task 5 added the required field; this test asserts the
exact shape). Finishes with a full build + test gate.

Source of truth: contract §7 (Done-when smoke assertions); existing `index.ts:1-22`;
`tests/index.test.ts`.

**Files:**
- `sdks/typescript/src/index.ts` (modify — explicit MCP type exports)
- `sdks/typescript/tests/index.test.ts` (modify — fix caps shape assertions)
- `sdks/typescript/tests/m4-smoke.test.ts` (NEW — done-criteria smoke test)

DEPENDENCY NOTE: depends on Task 1 (MCP types), Task 2 (serializer), Task 5 (caps +
`validateRequest` MCP gating + the `supportsMcp` field that changes the caps object shape). Run
this task LAST.

**Steps:**

- [ ] **Step 1: Add explicit MCP type exports to `index.ts`.**
  `index.ts` already does `export * from './types.js'` (line 1), which transitively re-exports
  the MCP types. The contract still requires an EXPLICIT export for discoverability/intent. In
  `sdks/typescript/src/index.ts`, replace the trailing block:
  ```ts
  export type { ProviderCapabilities, Provider } from './provider.js'
  export { textOnly, withImage, fullCaps, validateRequest } from './provider.js'
  ```
  with:
  ```ts
  export type { ProviderCapabilities, Provider } from './provider.js'
  export { textOnly, withImage, fullCaps, minimaxCaps, validateRequest } from './provider.js'

  // M4: server-side MCP types (also covered by `export * from './types.js'`; listed
  // explicitly for discoverability). No internal http/serialize symbols are exported.
  export type { McpServerType, McpServerConfig, McpToolConfig } from './types.js'
  ```
  Note: `export * from './types.js'` already exports these names, and TS permits a redundant
  explicit `export type` for the same names without a conflict (identical bindings). If
  `npm run build` ever complains about a duplicate export, drop the explicit
  `export type { McpServerType, ... }` line — the `export *` already satisfies the requirement —
  but prefer keeping the explicit line for clarity. Also note: `minimaxCaps` is added to the
  value export so the M4 caps helper is public alongside the others.

- [ ] **Step 2: Write the M4 smoke test (failing first).**
  Create `sdks/typescript/tests/m4-smoke.test.ts`:
  ```ts
  import { describe, expect, it } from 'vitest'
  import { serializeAnthropicRequest } from '../src/serialize/anthropic.js'
  import { validateRequest, withImage, fullCaps } from '../src/provider.js'
  import { UnsupportedFeatureError } from '../src/error.js'
  import type { ChatRequest } from '../src/types.js'

  describe('M4 done-criteria smoke', () => {
    const req: ChatRequest = {
      messages: [{ role: 'user', content: 'use the tools' }],
      mcpServers: [{ type: 'url', url: 'https://m.example/sse', name: 'm' }],
      mcpToolConfigs: [{ kind: 'all', mcpServerName: 'm' }],
      thinking: { budgetTokens: 2048 },
    }

    it('serializes MCP + thinking for anthropic (non-adaptive model)', () => {
      const body = serializeAnthropicRequest(req, 'claude-sonnet-4-6')
      // MCP servers on the body.
      expect(body.mcp_servers).toEqual([
        { type: 'url', url: 'https://m.example/sse', name: 'm' },
      ])
      // mcp_toolset folded into the tools array.
      expect(body.tools).toEqual([{ type: 'mcp_toolset', mcp_server_name: 'm' }])
      // Non-adaptive thinking → enabled shape + forced temperature.
      expect(body.thinking).toEqual({
        type: 'enabled',
        budget_tokens: 2048,
        display: 'summarized',
      })
      expect(body.temperature).toBe(1.0)
    })

    it('serializes MCP + thinking for anthropic (adaptive model)', () => {
      const body = serializeAnthropicRequest(req, 'claude-opus-4-8')
      expect(body.thinking).toEqual({ type: 'adaptive', display: 'summarized' })
      expect(body.output_config).toEqual({ effort: 'high' })
      expect('temperature' in body).toBe(false)
    })

    it('passes validateRequest for an MCP-capable provider (anthropic/fullCaps)', () => {
      expect(() => validateRequest(req, fullCaps())).not.toThrow()
    })

    it('rejects MCP for a non-MCP provider (openai/withImage) before any HTTP', () => {
      expect(() => validateRequest(req, withImage())).toThrow(UnsupportedFeatureError)
      expect(() => validateRequest(req, withImage())).toThrow(
        'provider does not support MCP server config',
      )
    })

    it('exposes the MCP types and caps helpers from the package entrypoint', async () => {
      const mod = await import('../src/index.js')
      expect(typeof mod.validateRequest).toBe('function')
      expect(typeof mod.minimaxCaps).toBe('function')
      // McpServerConfig is a type (erased at runtime) — assert usage compiles by
      // constructing a value typed as it via the imported module's type surface.
      const cfg: import('../src/index.js').McpServerConfig = {
        type: 'url',
        url: 'https://x/sse',
        name: 'x',
      }
      expect(cfg.name).toBe('x')
    })
  })
  ```
  Run:
  ```bash
  npx vitest run tests/m4-smoke.test.ts
  ```
  Expected: passes IF Tasks 1/2/5 have landed. (If `minimaxCaps` export in Step 1 hasn't been
  applied yet, the last `it` fails on `mod.minimaxCaps` — apply Step 1 first.)

- [ ] **Step 3: Fix the `index.test.ts` caps-shape assertions.**
  Task 5 made `supportsMcp` a required field, changing the exact caps object shape. In
  `sdks/typescript/tests/index.test.ts`, replace:
  ```ts
      const caps: ProviderCapabilities = mod.textOnly()
      expect(caps).toEqual({ supportsImage: false, supportsDocument: false })
      expect(mod.withImage()).toEqual({ supportsImage: true, supportsDocument: false })
      expect(mod.fullCaps()).toEqual({ supportsImage: true, supportsDocument: true })
  ```
  with:
  ```ts
      const caps: ProviderCapabilities = mod.textOnly()
      expect(caps).toEqual({ supportsImage: false, supportsDocument: false, supportsMcp: false })
      expect(mod.withImage()).toEqual({
        supportsImage: true,
        supportsDocument: false,
        supportsMcp: false,
      })
      expect(mod.fullCaps()).toEqual({
        supportsImage: true,
        supportsDocument: true,
        supportsMcp: true,
      })
  ```

- [ ] **Step 4: Verify the smoke + index tests pass.**
  ```bash
  npx vitest run tests/m4-smoke.test.ts tests/index.test.ts
  ```
  Expected: both files green.

- [ ] **Step 5: FINAL GATE — full build + full test suite.**
  This is the M4 done-criteria gate (no `npm run format` script; gate = build + test).
  ```bash
  npm run build && npm run test
  ```
  Expected: `tsc` exits 0, then vitest reports ALL test files passing (every M4 task's tests
  plus all pre-existing tests). Live tests (`integration.*`, MiniMax live) are SKIPPED unless
  their env keys are set. Zero failures.

- [ ] **Step 6: Commit.**
  ```bash
  git add sdks/typescript/src/index.ts sdks/typescript/tests/index.test.ts sdks/typescript/tests/m4-smoke.test.ts
  git commit -m "feat(ts): export MCP types and add M4 done-criteria smoke test"
  ```

---

## Milestone Done Criteria (verify all before tagging v0.7.0)

- [ ] `ChatRequest` accepts `mcpServers`/`mcpToolConfigs`; the Anthropic serializer emits `mcp_servers` + `mcp_toolset` items (in the tools array) correctly.
- [ ] Thinking serialization: adaptive model → `{type:adaptive, display:summarized}` + `output_config.effort`; non-adaptive → `{type:enabled, budget_tokens, display:summarized}` with `temperature` forced to 1.0 (overriding user temperature); user temperature honored only when thinking absent.
- [ ] `anthropic-beta` header carries the mcp-client beta when MCP is set and the interleaved-thinking beta when non-adaptive thinking is set; absent otherwise.
- [ ] MiniMax posts the Anthropic wire to `{base}/v1/messages` (NOT the legacy `/v1/text/chatcompletion`), parses content/thinking/toolCalls/usage/stopReason and the Anthropic SSE stream; default model `MiniMax-M2.7`; `ClientBuilder.minimaxBaseUrl` honored; env-gated live test passes.
- [ ] A request with `mcpServers` on OpenAI throws `UnsupportedFeatureError` before any HTTP call; on Anthropic (and Anthropic-wire MiniMax) it passes.
- [ ] `index.ts` exports the MCP types; `npm run build` + `npm run test` green.

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet accompanies this plan):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks (superpowers:subagent-driven-development).
2. **Inline:** execute tasks in-session with checkpoints (superpowers:executing-plans).

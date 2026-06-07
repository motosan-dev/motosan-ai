# Milestone 7 — Release Hardening (→ 0.10.0) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> ## ⚠️ VERSION RULE — binding, overrides the spec
> M7 targets **`0.10.0`** — **NOT `1.0` / `1.0.0`**. The spec (§4 M7) says "v1.0 / 1.0.0"; that is **overridden by explicit user decision**. Everywhere the spec says "1.0", read "0.10.0". `package.json` version `0.3.0` → **`0.10.0`**; publish tag **`ts-v0.10.0`**; CHANGELOG top entry **`0.10.0`**; publish workflow triggers on `ts-v*`. Do NOT write `1.0.0` as the TS SDK version anywhere.

**Goal:** Final release-hardening of `@motosan-ai/sdk` — version bump to 0.10.0, ESM `exports` + packaging, README, CHANGELOG, edge-case/cross-provider tests, CI enhancement + npm publish workflow, and repo-level SDK mentions. **Zero new wire/provider work.**

**Architecture:** Builds on merged M1–M6 (all 5 HTTP providers + setup-token OAuth on main). Touches no `src/**` provider/serialize/http logic (at most trivial `index.ts` re-export tidying). Adds packaging metadata, docs, a publish pipeline, and a thin layer of edge/parity tests on top of the existing ~480 unit tests.

**Tech Stack:** TypeScript (strict, NodeNext ESM), vitest, npm publish. Mirrors the python/rust SDK README/CHANGELOG/publish-workflow conventions.

**Spec:** `docs/superpowers/specs/2026-06-06-typescript-rust-parity-design.md` (§4 M7 — remap every "1.0" → "0.10.0"). **Depends on:** M1–M6, all merged. Branch M7 off `main`.

---

## Conventions (apply to EVERY task)

- **Repo root:** `/Users/daiwanwei/Projects/wade/motosan-ai`. Package: `sdks/typescript/`. Commands run from `sdks/typescript/` unless a task says otherwise. Paths repo-relative.
- **Workflow:** feature branch, land via **PR + CI**. Commit after each task. (From a git worktree the pre-push hook can't run Rust — verify `npm run build` + `npm run test` locally and `git push --no-verify`; CI runs the full gate.)
- **Module system:** strict + NodeNext, ESM-only (`type: module`). Relative imports end in `.js`. Tests in `tests/` (vitest, not tsc-checked).
- **Per-task gate:** `npm run build` + `npm run test` green; for packaging tasks also `npm pack --dry-run`; for workflow tasks, YAML validity. **No eslint/prettier gate** (CLAUDE.md notes treefmt skips for TS) — the only added check is `tsc --noEmit` (`npm run typecheck`).
- **License is MIT** (confirmed: repo `LICENSE`).

## Canonical homes & cross-task ownership (single source of truth)

| Task | Owns | Type |
|---|---|---|
| **T1** | `sdks/typescript/package.json` (version 0.10.0, exports, engines, metadata, sideEffects, scripts) | code/PR |
| **T2** | `sdks/typescript/README.md` (NEW) | docs |
| **T3** | `sdks/typescript/CHANGELOG.md` (NEW; top entry 0.10.0 + breaking-change history) | docs |
| **T4** | `sdks/typescript/tests/edge-cases.test.ts`, `tests/cross-provider-parity.test.ts`, `tests/pack-smoke.test.ts` (all NEW) | code/PR (TDD) |
| **T5** | `.github/workflows/ci-typescript.yml` (edit), `.github/workflows/publish-typescript.yml` (NEW, `ts-v*`) | code/PR |
| **T6** | root `README.md`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md` (conditional) + final 0.10.0 done-gate + open PR | docs/verify |

Strictly non-overlapping — no path owned by two tasks. **Order:** 1→6 (T6 is the done-gate; T1 should land before T5's publish-version guard is exercised).

**Breaking-change history for the CHANGELOG (0.4.0→0.10.0):** dropped `@anthropic-ai/sdk` (M1) + `openai` (M2) peer deps (self-implemented wire); `minimaxEndpoint`→`minimaxBaseUrl` rename (M4); `ToolCall.input` widened `Record<string,unknown>`→`unknown` (M5); default-model changes; added Ollama (M5) + Gemini (M6); MCP config + extended-thinking (M4); setup-token OAuth (`sk-ant-oat01-`).

---

### Task 1: package.json hardening + ESM exports (0.10.0)

> **VERSION RULE — binding:** this task bumps `version` to **`0.10.0`**, NOT `1.0` / `1.0.0`. The spec §4 says "1.0"; that is overridden by explicit user decision. The string `1.0.0` / `v1.0.0` must NOT appear anywhere in this file.
>
> **Why the jump 0.3.0 → 0.10.0 (skipping 0.4.0–0.9.0):** the per-milestone versions M1→0.4.0 … M6→0.9.0 were *milestone labels only* — never tagged or published to npm. M7 is the **first npm-published release**, so it jumps straight to 0.10.0 (a single CHANGELOG entry documents the whole M1–M7 evolution). This is intentional, not a skipped-release bug.

**Files:**
- `sdks/typescript/package.json` (edit — full replacement below)
- `sdks/typescript/package-lock.json` (regenerate ONLY if a devDep changes — none expected; leave untouched)

**Type:** CODE / ships in the M7 release PR. All commands run **from `sdks/typescript/`**.

Grounding facts already verified in the repo (do not re-discover):
- Current `package.json`: `version: "0.3.0"`, `type: "module"`, `main: "dist/index.js"`, `types: "dist/index.d.ts"`, `files: ["dist"]`, scripts `build`/`test`/`test:watch`, `peerDependencies: {}` + `peerDependenciesMeta: {}`, devDeps `@types/node`/`typescript`/`vitest`.
- `tsconfig.json`: `module: NodeNext`, `moduleResolution: NodeNext`, `declaration: true`, `outDir: dist`, `rootDir: src` → so the `exports` map must resolve under NodeNext; ESM-only, no CJS `dist`, therefore **no `require` condition**.
- `LICENSE` (repo root) is **MIT** ("Copyright (c) 2026 motosan-dev") → `license: "MIT"`.
- `src/index.ts` has **no top-level side-effectful code** (only `export *` / `export { … }`) → `"sideEffects": false` is truthful.
- `grep -rE "@anthropic-ai/sdk|from 'openai'" src` is EMPTY → peerDeps stay `{}`.

---

- [ ] **Step 1: Create the M7 branch off `main`.**
  ```bash
  git checkout main && git pull --ff-only
  git checkout -b m7-ts-hardening-0.10.0
  ```
  Expected: `Switched to a new branch 'm7-ts-hardening-0.10.0'`.

- [ ] **Step 2: Confirm the pre-change baseline is green** (so any later failure is attributable to M7, not pre-existing).
  ```bash
  npm ci
  npm run build && npm run test
  ```
  Expected: build emits `dist/`; vitest reports all existing tests pass (30 files, ~480 unit; the 9 env-gated live tests are skipped without keys). No `typecheck` script yet — that's added in this task.

- [ ] **Step 3: Confirm the license + side-effect + clean-deps assertions before writing them.**
  ```bash
  head -1 ../../LICENSE                       # → "MIT License"
  grep -rnE "@anthropic-ai/sdk|from 'openai'" src || echo CLEAN   # → CLEAN
  node -p "require('./tsconfig.json')" 2>/dev/null || cat tsconfig.json   # confirm module/moduleResolution = NodeNext
  ```
  Expected: `MIT License`; `CLEAN`; NodeNext confirmed. If the license is NOT MIT, write whatever `LICENSE` actually says instead of `"MIT"`.

- [ ] **Step 4: Overwrite `sdks/typescript/package.json` with the full hardened manifest below** (no placeholders).
  Key changes vs current: `version` `0.3.0`→`0.10.0`; add `exports` (types/import only — ESM, no `require`); add `engines.node >=18`; add `repository`/`homepage`/`bugs`/`license`/`author`/`keywords`; add `sideEffects: false`; add `README.md`+`CHANGELOG.md` to `files`; add scripts `typecheck` (`tsc --noEmit`) + `prepublishOnly` (`npm run build`); keep `main`/`types` as legacy fallbacks; keep `peerDependencies`/`peerDependenciesMeta` empty; NO `prepare`/`postinstall`.

  ```json
  {
    "name": "@motosan-ai/sdk",
    "version": "0.10.0",
    "description": "Multi-provider TypeScript/ESM SDK for Anthropic, OpenAI, MiniMax, Ollama, and Gemini. Self-implemented wire protocol via native fetch — zero official-provider-SDK dependencies.",
    "type": "module",
    "main": "dist/index.js",
    "types": "dist/index.d.ts",
    "exports": {
      ".": {
        "types": "./dist/index.d.ts",
        "import": "./dist/index.js"
      }
    },
    "sideEffects": false,
    "engines": {
      "node": ">=18"
    },
    "files": [
      "dist",
      "README.md",
      "CHANGELOG.md"
    ],
    "scripts": {
      "build": "tsc -p tsconfig.json",
      "typecheck": "tsc -p tsconfig.json --noEmit",
      "test": "vitest run",
      "test:watch": "vitest",
      "prepublishOnly": "npm run build"
    },
    "repository": {
      "type": "git",
      "url": "git+https://github.com/motosan-dev/motosan-ai.git",
      "directory": "sdks/typescript"
    },
    "homepage": "https://github.com/motosan-dev/motosan-ai/tree/main/sdks/typescript#readme",
    "bugs": {
      "url": "https://github.com/motosan-dev/motosan-ai/issues"
    },
    "license": "MIT",
    "author": "motosan-dev",
    "keywords": [
      "anthropic",
      "openai",
      "gemini",
      "ollama",
      "minimax",
      "llm",
      "ai",
      "sdk",
      "streaming",
      "claude"
    ],
    "peerDependencies": {},
    "peerDependenciesMeta": {},
    "devDependencies": {
      "@types/node": "^22.13.10",
      "typescript": "^5.8.2",
      "vitest": "^3.0.8"
    }
  }
  ```

- [ ] **Step 5: Verify the manifest is valid JSON and the version is exactly `0.10.0`.**
  ```bash
  node -p "require('./package.json').version"          # → 0.10.0
  node -p "JSON.stringify(require('./package.json').exports)"   # → {".":{"types":"./dist/index.d.ts","import":"./dist/index.js"}}
  node -p "require('./package.json').engines.node"     # → >=18
  node -p "require('./package.json').sideEffects"      # → false
  node -p "Object.keys(require('./package.json').peerDependencies).length"   # → 0
  grep -n "1\.0\.0" package.json || echo "NO 1.0.0 — good"
  ```
  Expected: every assertion matches; `NO 1.0.0 — good`.

- [ ] **Step 6: Run the gate — build + the NEW typecheck script + test.**
  ```bash
  npm run build && npm run typecheck && npm run test
  ```
  Expected: `tsc -p tsconfig.json` emits `dist/index.js` + `dist/index.d.ts`; `tsc -p tsconfig.json --noEmit` exits 0 (no type errors, no emit); vitest green (no regression). If `typecheck` flags pre-existing type issues in `src/`, STOP — that is out of M7 scope (M7 touches no `src` logic); record it and do not "fix" by loosening tsconfig.

- [ ] **Step 7: Confirm `dist/` did not gain a `require`/CJS entry** (the package is ESM-only; an accidental CJS condition would be a lie).
  ```bash
  test -f dist/index.js && test -f dist/index.d.ts && echo "ESM dist present"
  node -p "require('./package.json').exports['.'].require ?? 'no require condition — correct'"
  ```
  Expected: `ESM dist present`; `no require condition — correct`.

- [ ] **Step 8: `package-lock.json` check** — only regenerate if devDeps changed (they did not).
  ```bash
  git status --short package-lock.json     # expect NO output (lockfile untouched)
  ```
  Expected: empty. If `npm ci`/`npm run build` somehow touched the lockfile, run `npm install --package-lock-only` and include it; otherwise leave it.

- [ ] **Step 9: Commit (conventional).**
  ```bash
  git add package.json
  git commit -m "chore(ts): harden package.json for 0.10.0 (exports, engines, metadata, typecheck script)"
  ```

**Verification (Task 1 done-gate):** `npm run build && npm run typecheck && npm run test` all green; `node -p "require('./package.json').version"` prints `0.10.0`; `exports` resolves under NodeNext (types + import only, no require); peerDeps empty; no `1.0.0` in the file.

---

### Task 2: README.md (TypeScript SDK)

**Files:**
- `sdks/typescript/README.md` (NEW — full content below)

**Type:** DOCS / ships in the M7 release PR. Commands run **from `sdks/typescript/`**.

API-grounding facts the examples MUST honor (verified in `src/`, do NOT invent a `Message.user()` namespace):
- **Message factories are STANDALONE EXPORTED FUNCTIONS**, not a `Message` class/namespace. Python uses `Message.user(...)`; TS exports `user`, `assistant`, `assistantWithToolCalls`, `system`, `tool`, `toolResult`, `userWithImage`, `userWithBlocks`, `userWithPdfBase64`, `userWithPdfUrl`, `userWithPdfBytes`, `userWithCache`, `withCache` (`src/message.ts`). Import them by name from `@motosan-ai/sdk`. Every README example uses `user('…')`, NOT `Message.user('…')`.
- **`client.chat(request)` takes a `ChatRequest` OBJECT** (`{ messages, tools?, system?, toolChoice?, thinking?, … }`), NOT a positional messages array. `src/client.ts:347` + `src/types.ts:153` `ChatRequest`. So: `await client.chat({ messages: [user('Hello')] })`.
- Builder entry: `Client.builder().provider('anthropic').apiKey('sk-ant-…').model('claude-sonnet-4-6').build()` (`src/client.ts:287,86-99`).
- `ToolCall.input` is `unknown` (`src/types.ts:34-38`) — examples that read it must narrow/cast.
- `ChatResponse.toolCalls` is **always an array** (`src/types.ts:176`), never null. Field name is `toolCalls` (camelCase).
- `Tool` is an interface (`src/types.ts:41-47`): `{ name, description?, inputSchema?, cache? }`. (This is the TS `Tool` — the Rust ToolSchema change in CLAUDE.md does NOT apply to TS.)
- Default models (`src/models.ts`): Anthropic `claude-sonnet-4-6`, OpenAI `gpt-5.3-codex`, MiniMax `MiniMax-M2.7`, Ollama `llama3.2`, Gemini `gemini-2.5-flash`. Anthropic catalog includes `claude-opus-4-8`.
- Env vars (`src/client.ts:41-49`): `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `MINIMAX_API_KEY`, `GEMINI_API_KEY`; Ollama needs none.
- Builder provider-config methods (`src/client.ts:111-180`): `anthropicBaseUrl`, `geminiBaseUrl`, `minimaxBaseUrl`, `openaiAuthBearer`/`openaiAuthXApiKey`/`openaiAuthCustomHeader(name)`, `openaiChatUrl(url)`, `openaiResponsesFallback(bool)`, `openaiResponsesUrl(url)`, `ollamaBaseUrl`, `ollamaNative(bool)`, `ollamaThink(str)`, `ollamaKeepAlive(str)`, `ollamaNumCtx(num)`, `retryPolicy(rp)`, `streamReadTimeoutSecs(n)`.
- Stream API: `client.stream(request)` returns `AsyncIterable<StreamEvent>`; `StreamEvent` fields `content`/`done`/`eventType`/`stopReason`/`usage`/`toolCall*` (`src/types.ts:128-137`). `client.streamCollect(request)` / `client.streamCollectWith(request)` (`src/client.ts:367-378`). Top-level `collectStream(stream)` (`src/stream.ts:101`, exported via `src/index.ts`).
- Error classes (`src/error.ts:1-36`): `MotosanError` (base, has `status?`/`retryAfterMs?`), `AuthError`, `RateLimitError`, `InvalidRequestError`, `ConfigError`, `ProviderError`, `NetworkError`, `StreamError`, `StreamReadTimeoutError` (has `timeoutSecs`), `UnsupportedFeatureError`.
- Setup-token OAuth: `AnthropicProvider` auto-detects `sk-ant-oat01-` prefix (`src/providers/anthropic.ts:35-36`), Bearer + `oauth-2025-04-20` beta + Claude Code system identity (`:47`). Pass either key type into `.apiKey(...)`.
- `MinimaxProvider`: Anthropic-compat wire, final URL `{base}/v1/messages`, default base `https://api.minimax.io/anthropic` (`src/providers/minimax.ts`). Rename: `minimaxBaseUrl` (was `minimaxEndpoint`).

---

- [ ] **Step 1: Write `sdks/typescript/README.md` with the full content below.** (Mirrors `sdks/python/README.md` flow + `sdks/rust/README.md` per-feature sections; TS/ESM idioms; 18-section outline.)

````markdown
# motosan-ai (TypeScript SDK)

Multi-provider TypeScript/ESM SDK for Anthropic, OpenAI, MiniMax, Ollama, and Gemini.
Self-implemented wire protocol via native `fetch` — **zero official-provider-SDK dependencies**
(no `@anthropic-ai/sdk`, no `openai`). All five providers ship in one package and tree-shake
cleanly via ESM.

## Installation

```bash
npm install @motosan-ai/sdk
# or
pnpm add @motosan-ai/sdk
# or
yarn add @motosan-ai/sdk
```

There are no extras or feature flags: all five providers ship in the single npm package and
are tree-shaken via ESM (only what you import lands in your bundle).

## Requirements

- **Node >= 18** (the SDK uses native `fetch`, `ReadableStream`, and `TextDecoder`, stable from Node 18).
- An **ESM project** — set `"type": "module"` in your `package.json`, or use `.mjs` files.
- One provider credential (env var or passed to `.apiKey(...)`):

| Provider  | Env var             |
|-----------|---------------------|
| Anthropic | `ANTHROPIC_API_KEY` |
| OpenAI    | `OPENAI_API_KEY`    |
| MiniMax   | `MINIMAX_API_KEY`   |
| Gemini    | `GEMINI_API_KEY`    |
| Ollama    | (none — local)      |

## Quick Start

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .provider('anthropic')
  .apiKey(process.env.ANTHROPIC_API_KEY!)
  .model('claude-sonnet-4-6')
  .build()

const response = await client.chat({ messages: [user('Hello')] })
console.log(response.content)
```

`client.chat(...)` takes a `ChatRequest` object (`{ messages, tools?, system?, … }`). Message
factories (`user`, `assistant`, `toolResult`, …) are top-level named exports, not methods on a class.

## Providers

Swap providers by changing `.provider(...)` (and the credential). The request/response surface is identical.

### Anthropic

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .provider('anthropic')
  .apiKey('sk-ant-api03-...')
  .model('claude-sonnet-4-6') // default; override with 'claude-opus-4-8' for Opus
  .build()

const resp = await client.chat({ messages: [user('Hello')] })
```

Default model is `claude-sonnet-4-6`; the catalog includes `claude-opus-4-8`. For Opus 4.8/4.7/4.6,
`ChatRequest.thinking` maps to Anthropic adaptive thinking (`thinking.type = "adaptive"`, summarized
display, `output_config.effort = "high"`) instead of the older budget-token shape, matching pi.

### OpenAI

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .provider('openai')
  .apiKey('sk-...')
  .model('gpt-5.3-codex') // default
  .build()

const resp = await client.chat({ messages: [user('Hello')] })
```

OpenAI-compatible providers (Groq, DeepSeek, Together, self-hosted proxies) and auth variants are
configured on the builder:

```ts
const groq = Client.builder()
  .provider('openai')
  .apiKey('...')
  .openaiAuthXApiKey()                  // or .openaiAuthCustomHeader('X-Auth-Token') / .openaiAuthBearer()
  .openaiChatUrl('https://api.groq.com/openai/v1/chat/completions') // full URL POSTed; no /v1 injection
  .openaiResponsesFallback(true)        // fall back to the Responses API on 404
  .build()
```

- `.openaiChatUrl(url)` — the exact URL POSTed for chat completions (a single trailing `/` is trimmed; no other normalization).
- `.openaiResponsesUrl(url)` — URL for the Responses-API fallback (only used when `.openaiResponsesFallback(true)`).
- `.openaiAuthXApiKey()` / `.openaiAuthCustomHeader(name)` / `.openaiAuthBearer()` — auth header style.

### MiniMax

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .provider('minimax')
  .apiKey('...')
  .model('MiniMax-M2.7') // default
  .build()

const resp = await client.chat({ messages: [user('Hello')] })

// CN endpoint:
const cn = Client.builder()
  .provider('minimax')
  .apiKey('...')
  .minimaxBaseUrl('https://api.minimaxi.com/anthropic')
  .build()
```

MiniMax uses the **Anthropic-compatible** wire format. `.minimaxBaseUrl(...)` is the **base** URL —
the final request goes to `{base}/v1/messages` (default base `https://api.minimax.io/anthropic`).

> **Migration note:** the builder method is `minimaxBaseUrl`, replacing the old `minimaxEndpoint`.
> The value is now a base URL (`{base}/v1/messages` is appended), not a full endpoint.

### Ollama

```ts
import { Client, user } from '@motosan-ai/sdk'

// OpenAI-compatible mode (default) — no API key, talks to localhost:11434
const compat = Client.builder().provider('ollama').model('llama3.2').build()

// Native /api/chat mode — any of ollamaNative/ollamaThink/ollamaKeepAlive/ollamaNumCtx auto-routes to native
const native = Client.builder()
  .provider('ollama')
  .model('llama3.2')
  .ollamaNative(true)          // or just set one of the tuning options below
  .ollamaThink('true')
  .ollamaKeepAlive('5m')
  .ollamaNumCtx(8192)
  .build()

const resp = await native.chat({ messages: [user('Hello')] })
```

Setting any of `ollamaNative`/`ollamaThink`/`ollamaKeepAlive`/`ollamaNumCtx` auto-routes to the native
`/api/chat` NDJSON path; otherwise Ollama uses the OpenAI-compatible `/v1/chat/completions` path.

### Gemini

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .provider('gemini')
  .apiKey(process.env.GEMINI_API_KEY!)
  .model('gemini-2.5-flash') // default
  .build()

const resp = await client.chat({ messages: [user('Hello')] })
```

Gemini targets the `generativelanguage` REST API. It supports image content blocks; document/PDF
input is rejected before any HTTP call (capabilities check).

## Streaming

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder().provider('openai').apiKey('sk-...').build()

for await (const event of client.stream({ messages: [user('Write a haiku about rain')] })) {
  if (event.content) process.stdout.write(event.content)
  if (event.done) {
    if (event.stopReason) console.error(`\n[stop_reason: ${event.stopReason}]`)
    break
  }
}
```

Each provider stream emits **exactly one** terminal `done` event, even when the upstream provider closes
the connection without a sentinel and without a finish-reason chunk. `event.stopReason` carries the
provider's reported reason when present.

> **Mid-stream partial success (important):** if a transport error occurs *after* the stream has started,
> the stream terminates **silently** — it does NOT throw mid-stream. The events yielded so far form a
> partial, success-looking response (and `collectStream` will return a `ChatResponse` with a fabricated
> `stopReason`). Retries apply only to the *initial* fetch, never mid-stream (see Retry). If you need
> strict completeness, inspect `usage`/`stopReason` on the terminal event.

### Streaming → assembled response

```ts
// Convenience: stream + collect in one call
const resp = await client.streamCollect({ messages: [user('hi')] })

// Same, preferring the request's model override in the result
const resp2 = await client.streamCollectWith({ messages: [user('hi')], model: 'gpt-4o' })
```

The lower-level helper is also exported for custom stream callers:

```ts
import { collectStream } from '@motosan-ai/sdk'

const resp = await collectStream(someStreamEventIterable)
```

## Tools / tool_choice (multi-turn)

```ts
import { Client, user, assistantWithToolCalls, toolResult } from '@motosan-ai/sdk'
import type { Tool } from '@motosan-ai/sdk'

const client = Client.builder().provider('anthropic').apiKey('sk-ant-...').build()

const tools: Tool[] = [
  {
    name: 'get_weather',
    description: 'Get current weather',
    inputSchema: {
      type: 'object',
      properties: { city: { type: 'string' } },
      required: ['city'],
    },
  },
]

const messages = [user("What's the weather in Tokyo?")]
const response = await client.chat({ messages, tools, toolChoice: { type: 'auto' } })

// ChatResponse.toolCalls is ALWAYS an array — never null.
if (response.toolCalls.length > 0) {
  const tc = response.toolCalls[0]
  const { city } = tc.input as { city: string } // ToolCall.input is `unknown` — narrow it
  const result = `Sunny in ${city}`

  messages.push(assistantWithToolCalls(response.content, response.toolCalls))
  messages.push(toolResult(tc.id, result))
  const final = await client.chat({ messages, tools })
  console.log(final.content)
}
```

The tool-call field is `input` (not `args`/`params`). `ToolChoice` is a discriminated union:
`{ type: 'auto' }`, `{ type: 'required' }`, `{ type: 'none' }`, or `{ type: 'tool'; name: string }`.

## Vision / images

```ts
import { Client, userWithImage, userWithBlocks } from '@motosan-ai/sdk'

const client = Client.builder().provider('anthropic').apiKey('sk-ant-...').build()

// Single image (base64)
const resp = await client.chat({
  messages: [userWithImage('What is in this image?', base64Png, 'image/png')],
})

// Multiple content blocks (base64 and/or URL)
const msg = userWithBlocks([
  { type: 'text', text: 'Compare these two images' },
  { type: 'image', source: { type: 'base64', mediaType: 'image/png', data: firstB64 } },
  { type: 'image', source: { type: 'url', url: 'https://example.com/second.png' } },
])
const resp2 = await client.chat({ messages: [msg] })
```

Images work on Anthropic, OpenAI, and Gemini. Text-only providers (MiniMax) reject image content blocks
with `UnsupportedFeatureError` **before any HTTP call** (capabilities are validated client-side).

## Documents / PDFs

```ts
import { Client, userWithPdfBase64, userWithPdfUrl, userWithPdfBytes } from '@motosan-ai/sdk'

const client = Client.builder().provider('anthropic').apiKey('sk-ant-...').build()

await client.chat({ messages: [userWithPdfBase64('Summarize this PDF', base64Pdf)] })
await client.chat({ messages: [userWithPdfUrl('Summarize this PDF', 'https://example.com/doc.pdf')] })
await client.chat({ messages: [userWithPdfBytes('Summarize this PDF', pdfBytes /* Uint8Array */)] })
```

Document/PDF input is **Anthropic-only**; other providers reject it with `UnsupportedFeatureError`
before any HTTP call.

## Extended thinking

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder().provider('anthropic').apiKey('sk-ant-...').build()

const resp = await client.chat({
  messages: [user('Solve: 13 * 17, show concise reasoning')],
  thinking: { budgetTokens: 2048 },
})
console.log(resp.thinking) // reasoning (when present)
console.log(resp.content)

// Streaming emits thinking_delta / thinking_done events; collectStream folds them into resp.thinking
for await (const event of client.stream({
  messages: [user('think it through')],
  thinking: { budgetTokens: 1024 },
})) {
  if (event.eventType === 'thinking_delta') process.stdout.write(event.content)
  if (event.done) break
}
```

For Opus 4.8/4.7/4.6 the budget config is mapped to Anthropic adaptive thinking; for budget-based models
it is sent as budget tokens.

## MCP (server-side)

```ts
import { Client, user } from '@motosan-ai/sdk'
import type { McpServerConfig, McpToolConfig } from '@motosan-ai/sdk'

const client = Client.builder().provider('anthropic').apiKey('sk-ant-...').build()

const mcpServers: McpServerConfig[] = [
  { type: 'url', name: 'docs', url: 'https://mcp.example.com', authorizationToken: 'tok' },
]
const mcpToolConfigs: McpToolConfig[] = [
  { kind: 'allowed', mcpServerName: 'docs', allowedTools: ['search'] },
]

const resp = await client.chat({ messages: [user('search the docs')], mcpServers, mcpToolConfigs })
```

Server-side MCP is **Anthropic-only**. Non-Anthropic providers throw `UnsupportedFeatureError` when MCP
config is present.

## Retry / RetryPolicy

Retries are enabled by default for transient failures (`429`, `5xx`, network/timeout): 3 retries with
exponential backoff, respecting the `Retry-After` header.

```ts
import { Client, RetryPolicy } from '@motosan-ai/sdk'

const rp = new RetryPolicy() // tune via its builder methods; e.g. maxRetries / baseDelay / respectRetryAfter

const client = Client.builder()
  .provider('openai')
  .apiKey('...')
  .retryPolicy(rp)
  .build()
```

Retries apply only to the **initial** fetch — never mid-stream. A failure after the stream has started
terminates the stream silently (see Streaming).

## ClientBuilder + model defaults

| Provider  | Default model        |
|-----------|----------------------|
| Anthropic | `claude-sonnet-4-6`  |
| OpenAI    | `gpt-5.3-codex`      |
| MiniMax   | `MiniMax-M2.7`       |
| Ollama    | `llama3.2`           |
| Gemini    | `gemini-2.5-flash`   |

Override per client with `.model('...')`, or per request via `request.model`:

```ts
const client = Client.builder().provider('openai').apiKey('...').model('gpt-4o').build()
// per-request override:
const resp = await client.chat({ messages: [user('hi')], model: 'gpt-4o' })
```

Set a per-chunk stream read timeout (terminates the stream after N seconds of silence) with
`.streamReadTimeoutSecs(n)`:

```ts
const client = Client.builder()
  .provider('anthropic')
  .apiKey('...')
  .streamReadTimeoutSecs(30)
  .build()
```

## Error model

All errors extend `MotosanError` (carrying optional `status` and `retryAfterMs`):

| Error                     | Raised when |
|---------------------------|-------------|
| `AuthError`               | HTTP 401 (bad/missing credentials) |
| `RateLimitError`          | HTTP 429 (rate limited; honors `Retry-After`) |
| `InvalidRequestError`     | HTTP 400 (malformed request) |
| `ConfigError`             | builder misconfiguration (missing provider / missing key) |
| `ProviderError`           | unmapped non-OK HTTP status / null response body |
| `NetworkError`            | transport-level failure |
| `StreamError`             | stream-processing failure |
| `StreamReadTimeoutError`  | no stream data within `streamReadTimeoutSecs` (carries `timeoutSecs`) |
| `UnsupportedFeatureError` | feature unsupported by the provider (e.g. image on MiniMax, MCP on non-Anthropic) — thrown **before** any HTTP call |

```ts
import { Client, user, RateLimitError, AuthError } from '@motosan-ai/sdk'

try {
  const resp = await client.chat({ messages: [user('hello')] })
  console.log(resp.content)
} catch (err) {
  if (err instanceof RateLimitError) console.error('rate limited, retryAfterMs =', err.retryAfterMs)
  else if (err instanceof AuthError) console.error('auth failed')
  else throw err
}
```

## Anthropic Auth / setup-token OAuth

The Anthropic provider auto-detects the credential type by prefix — pass either into `.apiKey(...)`:

- `sk-ant-api*` (standard API key) → `x-api-key` header.
- `sk-ant-oat01-*` (setup-token / OAuth) → `Authorization: Bearer <token>` + `anthropic-beta: …,oauth-2025-04-20,…`
  headers + the Claude Code OAuth system identity (system prompt sent as blocks with the Claude Code prefix).

```ts
import { Client, user } from '@motosan-ai/sdk'

// Standard API key
const a = Client.builder().provider('anthropic').apiKey('sk-ant-api03-...').build()

// Setup-token / OAuth — same interface, auto-detected
const b = Client.builder().provider('anthropic').apiKey('sk-ant-oat01-...').build()
const resp = await b.chat({ messages: [user('Hello')] })
```

**⚠️ ToS disclosure:** an `sk-ant-oat01-*` token uses the OAuth `client_id` registered by Anthropic's
Claude Code CLI. The resulting token authenticates **as a Claude Code CLI session**. Anthropic has not
published this `client_id` for third-party use; usage for purposes other than running `claude` may be
subject to change, rate limited, or in violation of Anthropic's terms. You are responsible for
compliance. If you have an `sk-ant-api*` key, prefer it.

## Testing

```bash
# Unit tests (mocked fetch — no network, no keys needed)
npm run test

# Type check (no emit)
npm run typecheck
```

Live integration tests are env-gated (one per provider; skipped when the matching key is absent). For
example, set `ANTHROPIC_API_KEY` to run the Anthropic live test, `GEMINI_API_KEY` for the Gemini live
test, etc.

## Publishing

Automated via `publish-typescript.yml` on a `ts-v*` tag push → npm.

```bash
# Bump sdks/typescript/package.json version + CHANGELOG, then:
git tag ts-v0.10.0
git push origin ts-v0.10.0
```

The Rust, Python, and TypeScript SDKs are versioned and released independently.

## For AI Agents

If you're an AI coding assistant, fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt)
for a quick-start guide with API examples, tool-use patterns, and streaming setup.
````

- [ ] **Step 2: Confirm the README contains no `1.0.0` and no `Message.user(` namespace pattern** (the #1 grounding mistake).
  ```bash
  grep -n "1\.0\.0" README.md || echo "NO 1.0.0 — good"
  grep -n "Message\.\(user\|assistant\|toolResult\)" README.md && echo "BUG: namespace form — fix to standalone fn" || echo "standalone factories — good"
  grep -n "ts-v0.10.0" README.md   # publish section uses the correct tag
  ```
  Expected: `NO 1.0.0 — good`; `standalone factories — good`; the `ts-v0.10.0` line is present.

- [ ] **Step 3: Spot-check that every imported symbol in the README actually exists in the public API.**
  ```bash
  npm run build   # ensures dist/index.d.ts is current
  node --input-type=module -e "
    import('@motosan-ai/sdk').catch(()=>import('./dist/index.js')).then(m =>
      ['Client','user','assistant','assistantWithToolCalls','toolResult','userWithImage','userWithBlocks','userWithPdfBase64','userWithPdfUrl','userWithPdfBytes','collectStream','RetryPolicy']
        .forEach(n => { if (!(n in m)) throw new Error('MISSING export: '+n) })
    ).then(()=>console.log('all README exports resolve')).catch(e=>{console.error(e);process.exit(1)})
  "
  ```
  Expected: `all README exports resolve`. (If `@motosan-ai/sdk` is not yet linked, the fallback imports `./dist/index.js`.) Types-only symbols (`Tool`, `McpServerConfig`, `McpToolConfig`) are not runtime exports — verify those by `npm run typecheck` instead, which is already green from Task 1.

- [ ] **Step 4: Gate — build + typecheck + test** (README is docs, but it ships in the PR; keep the gate green).
  ```bash
  npm run build && npm run typecheck && npm run test
  ```
  Expected: all green (README adds no code; nothing should change).

- [ ] **Step 5: Commit.**
  ```bash
  git add README.md
  git commit -m "docs(ts): add README for @motosan-ai/sdk 0.10.0"
  ```

**Verification (Task 2 done-gate):** `README.md` exists; uses standalone message factories + `client.chat({ messages })` (no `Message.` namespace); all runtime example imports resolve against `dist/index.js`; no `1.0.0`; publish section shows `ts-v0.10.0`; build+typecheck+test green.

---

### Task 3: CHANGELOG.md (TypeScript SDK)

> **VERSION RULE — binding:** the top heading is **`## [0.10.0]`**, NOT `1.0.0`. The string `1.0.0` / `v1.0.0` must NOT appear.

**Files:**
- `sdks/typescript/CHANGELOG.md` (NEW — full content below)

**Type:** DOCS / ships in the M7 release PR. Commands run **from `sdks/typescript/`**.

Format facts (verified): mirror `sdks/python/CHANGELOG.md` Keep-a-Changelog style — header `# Changelog` + a one-line "All notable changes …" sentence, then bracketed `## [X.Y.Z] - YYYY-MM-DD` headings with `### Added` / `### Changed` / `### Removed (BREAKING)` / `### Changed (BREAKING)` / `### Notes` subsections (Python's order). This is the **first** CHANGELOG for the TS SDK, so a single consolidated `## [0.10.0]` entry documents the whole `0.4.0 → 0.10.0` evolution (no intermediate CHANGELOG was ever written).

The full breaking-change list to enumerate (from the binding contract + spec §4/§8):
1. **Removed (BREAKING):** dropped `@anthropic-ai/sdk` peer dep (M1) + `openai` peer dep (M2) — SDK now self-implements the Anthropic `/v1/messages` and OpenAI `/v1/chat/completions` wire via native `fetch` + SSE/NDJSON. `peerDependencies` is now `{}`.
2. **Changed (BREAKING):** `minimaxEndpoint` → `minimaxBaseUrl` (M4) — builder method + option renamed; value is the Anthropic-compat **base** URL (`{base}/v1/messages` appended), not a full endpoint.
3. **Changed (BREAKING):** `ToolCall.input` widened `Record<string, unknown>` → `unknown` (M5).
4. **Changed (BREAKING):** default-model changes (spec §8 "silent default-model change" — must be explicit). Current defaults from `src/models.ts`: OpenAI `gpt-5.3-codex`, MiniMax `MiniMax-M2.7`, Gemini `gemini-2.5-flash`, Anthropic `claude-sonnet-4-6`, Ollama `llama3.2`.

> **old→new accuracy guard:** the prior `0.3.0` defaults are NOT in any committed file. Before listing `old → new`, recover them: `git show $(git rev-list -1 ts-v0.3.0 2>/dev/null || git log --oneline | tail -1):sdks/typescript/src/models.ts 2>/dev/null` — OR `git log -p -- sdks/typescript/src/models.ts | grep -A2 -B2 DEFAULT_`. If the prior values cannot be recovered with confidence, write the **new** defaults and frame the note as "defaults are now: …; if you relied on a prior default, pin it explicitly via `.model(...)`" rather than inventing an `old` value. Do NOT fabricate an old model id.

---

- [ ] **Step 1: Recover the prior 0.3.0 default-model values (best effort) so the old→new note is accurate.**
  ```bash
  git log --oneline -- sdks/typescript/src/models.ts | tail -5
  git log -p -- sdks/typescript/src/models.ts | grep -E "DEFAULT_(OPENAI|MINIMAX|GEMINI|ANTHROPIC|OLLAMA)_MODEL" | head -40
  ```
  Record any `old` values found. If none are recoverable, use the "pin explicitly" phrasing in Step 2's `### Changed (BREAKING)` model bullet (do not invent).

- [ ] **Step 2: Write `sdks/typescript/CHANGELOG.md` with the full content below.** Set the date to today (the release-PR date). Fill the `old →` model values from Step 1 if recovered; otherwise keep the "pin explicitly" wording shown.

  ````markdown
  # Changelog

  All notable changes to `@motosan-ai/sdk` TypeScript SDK are documented in this file.

  The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

  ## [0.10.0] - 2026-06-07

  First published release of the TypeScript SDK. This consolidated entry documents the
  evolution from the unreleased `0.3.0`/`0.4.0` line through milestones M1–M7: the SDK
  now self-implements every provider wire protocol over native `fetch` and ships with
  **zero official-provider-SDK dependencies**.

  ### Added
  - **Anthropic + OpenAI raw wire (M1/M2):** self-hosted Anthropic `/v1/messages` and
    OpenAI `/v1/chat/completions` clients over native `fetch`, with the full `StreamEvent`
    taxonomy (`text` / `tool_call_start` / `tool_call_args` / `tool_call_end` / `usage` /
    `thinking_delta` / `thinking_done`) and the `collectStream` reassembly helper.
  - **Per-provider serializers (M2):** `serialize/{anthropic,openai,gemini}.ts` with
    `tool_choice`, `system_blocks`, `cache_control`, and `stop_sequences` support.
  - **Client + routing (M3):** `Client.builder()` (`ClientBuilder`), `Provider` routing,
    `RetryPolicy`, `ProviderCapabilities`, `ThinkStripper`, a models registry, and a
    configurable per-chunk stream-read timeout (`.streamReadTimeoutSecs(n)`).
  - **MiniMax + MCP + thinking (M4):** MiniMax via the Anthropic-compatible wire;
    server-side MCP config (`McpServerConfig` / `McpServerType` / `McpToolConfig`,
    Anthropic-only); extended-thinking request config (`ChatRequest.thinking`).
  - **Ollama (M5):** native `/api/chat` NDJSON mode and OpenAI-compatible mode, with
    auto-routing (any of `ollamaNative` / `ollamaThink` / `ollamaKeepAlive` /
    `ollamaNumCtx` selects the native path).
  - **Gemini (M6):** `generativelanguage` REST provider with image content blocks.
  - **Anthropic setup-token OAuth:** `sk-ant-oat01-*` tokens auto-detected by prefix →
    `Authorization: Bearer` + `oauth-2025-04-20` beta + Claude Code system identity.
  - **Packaging / docs / release (M7):** README, this CHANGELOG, an ESM `exports` map plus
    `engines` / repository metadata, an edge-case + cross-provider parity test layer, a
    `publish-typescript.yml` workflow (triggered on `ts-v*` tags), and CI `tsc --noEmit`
    + `npm pack` smoke steps.

  ### Removed (BREAKING)
  - Dropped the **`@anthropic-ai/sdk`** peer dependency (M1) and the **`openai`** peer
    dependency (M2). The SDK now self-implements the Anthropic and OpenAI wire protocols
    via native `fetch` + SSE/NDJSON, so `peerDependencies` is intentionally `{}`.
    **Migration:** remove `@anthropic-ai/sdk` / `openai` from your dependencies — no code
    change is needed for `Client` / message-factory callers.

  ### Changed (BREAKING)
  - **`minimaxEndpoint` → `minimaxBaseUrl` (M4).** The builder method and option were
    renamed, and the value is now the Anthropic-compatible **base** URL — the SDK appends
    `/v1/messages` (default base `https://api.minimax.io/anthropic`).
    **Migration:** rename the call and pass the base (e.g.
    `.minimaxBaseUrl('https://api.minimaxi.com/anthropic')`), not a full endpoint URL.
  - **`ToolCall.input` widened from `Record<string, unknown>` → `unknown` (M5).**
    **Migration:** narrow before use, e.g. `const { city } = tc.input as { city: string }`.
  - **Default models changed.** Current defaults: Anthropic `claude-sonnet-4-6`, OpenAI
    `gpt-5.3-codex`, MiniMax `MiniMax-M2.7`, Ollama `llama3.2`, Gemini `gemini-2.5-flash`.
    If you relied on a prior default model, pin it explicitly with `.model('...')` (per
    client) or `request.model` (per request) — relying on the implicit default may select
    a different model than before.

  ### Notes
  - **Zero official-provider-SDK dependencies** — `peerDependencies` and
    `peerDependenciesMeta` are intentionally empty; the entire point of M1/M2 was to drop
    `@anthropic-ai/sdk` and `openai`.
  - **ESM-only** (NodeNext resolution). Requires **Node >= 18** (native `fetch`,
    `ReadableStream`, `TextDecoder`).
  - **Streaming contract:** each stream emits exactly one terminal `done` event; a
    transport error after the stream starts terminates silently with a partial,
    success-looking response (retries apply only to the initial fetch).
  ````

- [ ] **Step 3: Confirm format + no forbidden version string.**
  ```bash
  head -1 CHANGELOG.md                                 # → "# Changelog"
  grep -n "^## \[0.10.0\]" CHANGELOG.md                # heading uses bracketed form
  grep -nE "v?1\.0\.0" CHANGELOG.md | grep -v "keepachangelog" || echo "NO 1.0.0 (except the keepachangelog spec URL) — good"
  ```
  Expected: `# Changelog`; the `## [0.10.0]` heading is present; the only `1.0.0` occurrence is the Keep-a-Changelog spec URL (`keepachangelog.com/en/1.0.0/`) — which is a doc-format reference, NOT the SDK version. If you want zero ambiguity, you may drop the spec-URL line; otherwise this single allowed occurrence is acceptable per the contract (the ban is on `1.0.0` as the *TS SDK version*).

  > If the reviewer insists on literally zero `1.0.0` substrings, change the format line to `The format is based on Keep a Changelog.` (no versioned URL).

- [ ] **Step 4: Gate — build + typecheck + test** (docs, but ships in the PR).
  ```bash
  npm run build && npm run typecheck && npm run test
  ```
  Expected: all green.

- [ ] **Step 5: Commit.**
  ```bash
  git add CHANGELOG.md
  git commit -m "docs(ts): add CHANGELOG with consolidated 0.10.0 entry"
  ```

**Verification (Task 3 done-gate):** `CHANGELOG.md` exists; top entry is `## [0.10.0]`; it enumerates all four breaking changes (dropped peer deps, `minimaxEndpoint→minimaxBaseUrl`, `ToolCall.input` widening, default-model change) with migration notes plus the M1–M6 Added list and M7 hardening; no `1.0.0` as the SDK version; build+typecheck+test green.

---

### Task 4: Edge-case + cross-provider parity tests

**Files:**
- `sdks/typescript/tests/edge-cases.test.ts` (NEW)
- `sdks/typescript/tests/cross-provider-parity.test.ts` (NEW)
- `sdks/typescript/tests/pack-smoke.test.ts` (NEW — optional; the authoritative tarball-resolution check is the CI `npm pack` step in Task 5, so this is a thin "dist exists / exports targets present" guard)

**Type:** CODE / ships in the M7 release PR. TDD (failing → run → confirm → green). Commands run **from `sdks/typescript/`**.

> **Scope discipline:** ADD only the gaps below. Do NOT re-test what the existing 30 files already cover (per-provider serialize/parse, SSE/NDJSON partial-chunk reassembly in `http.sse.test.ts`/`http.ndjson.test.ts`, retry, capabilities, think_stripper, builder). These tests touch **no `src/` logic** — they assert existing behavior. If a new test fails, the bug is in the *test's* expectation, not in `src` (M7 ships no wire changes) — fix the test, do not change `src`.

Test-style facts (verified, match the siblings exactly):
- vitest, 2-space indent, single quotes, **no semicolons**. Header: `import { describe, it, expect, vi, afterEach } from 'vitest'`.
- Mock the network with `vi.stubGlobal('fetch', vi.fn().mockResolvedValue(mockResponse))` and `afterEach(() => vi.unstubAllGlobals())` (pattern from `tests/http-fetch.test.ts`).
- Streaming providers read `response.body` as a `ReadableStream<Uint8Array>`; build one from an SSE/NDJSON string transcript and return it inside a real `new Response(stream, { status: 200, headers: { 'content-type': 'text/event-stream' } })` (pattern from `tests/providers-anthropic.test.ts`).
- Serializer signatures: `serializeAnthropicRequest(req, model)`, `serializeOpenAiRequest(req, model)`, `serializeGeminiRequest(req, model)` — each returns a plain object. Import from `../src/serialize/{anthropic,openai,gemini}.js`.
- Message factories are standalone fns from `../src/message.js`. `Client`/`ClientBuilder` from `../src/client.js`. Error classes from `../src/error.js`. `collectStream` from `../src/stream.js`.
- `validateRequest(req, caps)` from `../src/provider.js` throws `UnsupportedFeatureError` for unsupported image/document/MCP — the empty-messages case must NOT throw there.
- A reusable SSE-stream helper (write once at the top of `edge-cases.test.ts`):
  ```ts
  function sseStream(text: string): ReadableStream<Uint8Array> {
    return new ReadableStream({
      start(controller) {
        controller.enqueue(new TextEncoder().encode(text))
        controller.close()
      },
    })
  }
  ```

---

- [ ] **Step 1: Confirm which providers already have mid-stream-reset / partial-success coverage** (so we add only the missing ones, per the contract's "CONFIRM … do not duplicate").
  ```bash
  grep -nliE "mid-stream|partial.success|fabricat|terminates? (silently|without)" tests/*.test.ts
  grep -nE "without .*throw|terminat|partial" tests/providers-anthropic.test.ts tests/providers-openai.test.ts tests/providers-gemini.test.ts | head -30
  ```
  Record per provider whether a "stream errors after first text → terminates silently with a partial response" test already exists. In Step 3 add the mid-stream-reset case ONLY for providers lacking it. (`readTimeoutStream`'s silent-terminate is unit-tested in `client-builder.test.ts`; that is the wrapper, not the provider-adapter mid-flight drop — the gap is the adapter-layer drop.)

- [ ] **Step 2 (RED): Write `tests/edge-cases.test.ts`** covering the four gaps. Use the helpers above; assert behavior, not internals.

  Sub-blocks (each a `describe`):
  1. **Empty messages** — for each serializer, `serialize*Request({ messages: [] }, MODEL)` must NOT throw and must produce a well-formed body shape (Anthropic: object with `model` + `messages: []`; OpenAI: object with `model` + `messages` array; Gemini: object with `contents: []`). Also `validateRequest({ messages: [] }, fullCaps())` must not throw.
  2. **Null / malformed SSE JSON mid-stream (adapter layer)** — build an SSE transcript that interleaves a valid event, a `data:` line with invalid JSON (`data: {not json`), and a `data: null` line, then a valid terminal event; drive it through the provider's stream adapter (Anthropic, since its SSE adapter is the canonical one) via a mocked `fetch` that returns `new Response(sseStream(transcript), …)`. Assert: the malformed/null lines are **skipped** (no throw), valid text is still yielded, and the stream ends with a `done` event. (Extends `http.sse.test.ts`'s raw-parser malformed-skip to the *adapter* layer.)
  3. **Mid-stream reset / partial success** — for each provider NOT already covered (per Step 1), build an SSE transcript that emits `message_start` + first text, then the stream simply **ends without a terminal/finish event** (simulating a dropped connection). Drive it through `client.stream(...)` and through `collectStream(client.stream(...))`. Assert: iterating the stream yields the partial text and a `done` event and **never throws**; `collectStream` returns a `ChatResponse` whose `content` is the partial text, `toolCalls` is an array (possibly empty), and `stopReason` is a fabricated terminal reason (e.g. `'end_turn'`, per `collectStream`'s heuristic in `src/stream.ts:182-186`). The stream must never yield a thrown error mid-flight.
  4. **Unexpected / non-error status codes** — with mocked `fetch`, call `client.chat(...)` (or the provider's `chat`) against responses with status `204` (no body), `301`, an empty-body `200`, and an unmapped `4xx` (`418`). Assert each either resolves to a sensible result OR throws a mapped `MotosanError` subclass (`ProviderError` for unmapped non-OK; `InvalidRequestError` only for 400) — **never** an unhandled/unexpected throw type. (`mapHttpError` in `src/error.ts:38-57` maps 401/429/400 specifically and everything else to `ProviderError`; `postJson` throws for any `!response.ok`. So `418`→`ProviderError`; `301` is not `ok`→`ProviderError`; `204`/empty-200 exercise the empty-body parse path — assert the thrown/handled type, do not assert a specific message.)

  Run it and watch it FAIL ONLY for the right reason (file is new):
  ```bash
  npx vitest run tests/edge-cases.test.ts
  ```
  Expected: the file runs. If any assertion fails, decide: (a) the assertion encodes a wrong expectation about existing `src` behavior → fix the assertion to match reality (e.g. if Anthropic adapter actually maps 418 differently, assert what it does); (b) genuine `src` bug → STOP and report (out of M7 scope to fix wire logic). Iterate until all assertions reflect real, observed behavior and pass.

- [ ] **Step 3 (GREEN): Make `tests/edge-cases.test.ts` pass.** Since no `src` change is allowed, "green" means each assertion matches the SDK's actual behavior. Re-run:
  ```bash
  npx vitest run tests/edge-cases.test.ts
  ```
  Expected: all edge-case tests pass.

- [ ] **Step 4 (RED→GREEN): Write `tests/cross-provider-parity.test.ts`.** One canonical `ChatRequest` (system + one user text + one `Tool` with `inputSchema` + `toolChoice: { type: 'auto' }`):
  - **Serialize-shape invariants:**
    - Anthropic (`serializeAnthropicRequest`): top-level `system` present; `tools` is `[{ name, description, input_schema }]` (snake_case `input_schema`).
    - OpenAI (`serializeOpenAiRequest`): a `role: 'system'` message in `messages`; `tools` is `[{ type: 'function', function: { name, description, parameters } }]`.
    - Gemini (`serializeGeminiRequest`): `contents` array present; tool defs under `functionDeclarations` (assert the key the serializer actually emits — confirm against `tests/serialize.gemini.test.ts` / `src/serialize/gemini.ts` before locking the assertion).
  - **Parse-parity invariant:** a minimal canonical *response* per provider, fed through that provider's chat parse path (mocked `fetch`), yields a `ChatResponse` with the same surface: `content` is a string, `toolCalls` is an array, `usage` has `inputTokens`/`outputTokens`. (This is a parity *sanity* check — NOT a re-test of each serializer's full matrix, which already exists.)

  ```bash
  npx vitest run tests/cross-provider-parity.test.ts
  ```
  Expected: runs; iterate assertions to match real serializer output (read `src/serialize/*.ts` to confirm exact key names — e.g. Gemini `functionDeclarations` vs `function_declarations`) until green.

- [ ] **Step 5 (optional): Write `tests/pack-smoke.test.ts`** — a thin guard that the built artifacts the `exports` map points at exist (the authoritative NodeNext-tarball check is the CI `npm pack` step, Task 5).
  ```ts
  import { describe, it, expect } from 'vitest'
  import { existsSync } from 'node:fs'

  describe('packaging smoke', () => {
    it('exports-map targets exist after build', () => {
      // Run `npm run build` before `npm run test` so dist/ is fresh (CI does build→test).
      expect(existsSync('dist/index.js')).toBe(true)
      expect(existsSync('dist/index.d.ts')).toBe(true)
    })
  })
  ```
  ```bash
  npm run build && npx vitest run tests/pack-smoke.test.ts
  ```
  Expected: passes when run after `build`. (Note in the file's top comment that it depends on a prior `npm run build`.)

- [ ] **Step 6: Style-match check** — confirm the new files follow 2-space / single-quote / no-semicolon style of the siblings.
  ```bash
  grep -nE ";\s*$" tests/edge-cases.test.ts tests/cross-provider-parity.test.ts tests/pack-smoke.test.ts && echo "BUG: trailing semicolons — remove" || echo "no trailing semicolons — good"
  ```
  Expected: `no trailing semicolons — good`.

- [ ] **Step 7: Full gate — build + typecheck + the WHOLE suite** (confirm zero regression across the existing 30 files + the new ones).
  ```bash
  npm run build && npm run typecheck && npm run test
  ```
  Expected: all green; the test-file count is now 33 (30 existing + 3 new, or 32 if `pack-smoke` is skipped). Existing ~480 unit tests unchanged.

- [ ] **Step 8: Commit.**
  ```bash
  git add tests/edge-cases.test.ts tests/cross-provider-parity.test.ts tests/pack-smoke.test.ts
  git commit -m "test(ts): add edge-case + cross-provider parity tests for 0.10.0"
  ```

**Verification (Task 4 done-gate):** new test files exist and pass; full `npm run test` green with no regression in the existing 30 files; new tests cover empty messages, malformed/null SSE skip at the adapter layer, mid-stream-reset partial success (only for providers not already covered), and unexpected status codes (204/301/empty-200/418); cross-provider serialize-shape + parse-parity sanity asserted; no `src/` change.

---

### Task 5: CI enhancement + publish workflow

> **VERSION RULE — binding:** the publish workflow triggers on **`ts-v*`** and the example tag is **`ts-v0.10.0`**. No `1.0.0` anywhere.

**Files:**
- `.github/workflows/ci-typescript.yml` (edit — add two steps; repo-root path, NOT under `sdks/`)
- `.github/workflows/publish-typescript.yml` (NEW — repo-root path, NOT under `sdks/`)

**Type:** CODE / ships in the M7 release PR. The workflow files live at the **repo root** `.github/workflows/`; `working-directory: sdks/typescript` scopes the run steps. `npm`/`git` verification commands run **from `sdks/typescript/`** or repo root as noted.

Grounding facts (verified):
- Current `ci-typescript.yml`: `on.push.paths`/`on.pull_request.paths` = `sdks/typescript/**` + the workflow file; one job `typescript`, `runs-on: ubuntu-latest`, `defaults.run.working-directory: sdks/typescript`; steps checkout → `setup-node@v4` (node 20, npm cache, `cache-dependency-path: sdks/typescript/package-lock.json`) → `npm ci` → `npm run build` → `npm run test`.
- `publish-python.yml` / `publish-rust.yml` shape: trigger on `<lang>-v*` tag + `workflow_dispatch`; one `publish` job; `defaults.run.working-directory: sdks/<lang>`; build/test then publish; rust runs fmt/clippy/test BEFORE `cargo publish`.
- `npm run typecheck` script is added in Task 1 (`tsc -p tsconfig.json --noEmit`). The package is scoped `@motosan-ai/sdk` → `npm publish` needs `--access public`. Provenance needs `--provenance` + `id-token: write`. Auth via `NPM_TOKEN` secret as `NODE_AUTH_TOKEN`.

---

- [ ] **Step 1 (5a): Edit `.github/workflows/ci-typescript.yml`** — add `npm run typecheck` immediately AFTER `npm run build`, and `npm pack --dry-run` AFTER `npm run test`. Do NOT change the trigger `paths`, the Node version, or add any eslint/prettier step (CLAUDE.md: treefmt skips TS; the gate is build+test+typecheck).

  The steps list becomes exactly:
  ```yaml
      steps:
        - uses: actions/checkout@v4
        - uses: actions/setup-node@v4
          with:
            node-version: "20"
            cache: "npm"
            cache-dependency-path: sdks/typescript/package-lock.json
        - run: npm ci
        - run: npm run build
        - run: npm run typecheck
        - run: npm run test
        - run: npm pack --dry-run
  ```
  (Leave `name`, `on`, `jobs.typescript.runs-on`, and `defaults.run.working-directory` untouched.)

- [ ] **Step 2 (5b): Create `.github/workflows/publish-typescript.yml`** with this exact content (mirrors publish-python.yml/publish-rust.yml; adds the version-matches-tag guard the spec wants):

  ```yaml
  name: publish-typescript

  on:
    push:
      tags: ["ts-v*"]
    workflow_dispatch: {}

  jobs:
    publish:
      runs-on: ubuntu-latest
      defaults:
        run:
          working-directory: sdks/typescript
      permissions:
        contents: read
        id-token: write # required for npm provenance
      steps:
        - uses: actions/checkout@v4
        - uses: actions/setup-node@v4
          with:
            node-version: "20"
            registry-url: "https://registry.npmjs.org"
            cache: "npm"
            cache-dependency-path: sdks/typescript/package-lock.json
        - run: npm ci
        - run: npm run build
        - run: npm run test
        - name: Verify tag matches package.json version
          run: |
            TAG="${GITHUB_REF_NAME#ts-v}"
            PKG=$(node -p "require('./package.json').version")
            test "$TAG" = "$PKG" || { echo "tag $TAG != package.json $PKG"; exit 1; }
        - run: npm publish --provenance --access public
          env:
            NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}
  ```

  Binding details (do not drift): trigger `ts-v*`; `working-directory: sdks/typescript`; `id-token: write` + `--provenance`; `--access public` (scoped pkg defaults to restricted); auth `NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}`; build+test run before publish; the guard strips the `ts-v` prefix from `GITHUB_REF_NAME` and compares to `package.json` version (so `ts-v0.10.0` requires `version: "0.10.0"` — which Task 1 set).

- [ ] **Step 3: YAML lint / parse both workflows** (cheap structural check; no act/CI run needed locally).
  ```bash
  node -e "const fs=require('fs');for(const f of ['.github/workflows/ci-typescript.yml','.github/workflows/publish-typescript.yml']){const s=fs.readFileSync(f,'utf8');if(!s.includes('working-directory: sdks/typescript'))throw new Error('missing working-directory in '+f);console.log(f+': ok')}"
  ```
  Run from the **repo root**. Expected: both `: ok`. If `yamllint` or `actionlint` is available, prefer it:
  ```bash
  command -v actionlint >/dev/null && actionlint .github/workflows/ci-typescript.yml .github/workflows/publish-typescript.yml || echo "actionlint not installed — node parse check above suffices"
  ```

- [ ] **Step 4: Assert the publish trigger + guard are correct.**
  ```bash
  grep -n 'tags: \["ts-v\*"\]' .github/workflows/publish-typescript.yml
  grep -n 'GITHUB_REF_NAME#ts-v' .github/workflows/publish-typescript.yml
  grep -n -- '--provenance --access public' .github/workflows/publish-typescript.yml
  grep -n 'NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}' .github/workflows/publish-typescript.yml
  grep -rn "1\.0\.0" .github/workflows/publish-typescript.yml .github/workflows/ci-typescript.yml || echo "NO 1.0.0 — good"
  ```
  Expected: each grep matches its line; `NO 1.0.0 — good`.

- [ ] **Step 5: Locally simulate the version-matches-tag guard** (proves it would PASS for `ts-v0.10.0` after Task 1, and FAIL for a wrong tag).
  ```bash
  cd sdks/typescript
  PKG=$(node -p "require('./package.json').version")        # 0.10.0
  TAG="ts-v0.10.0"; T="${TAG#ts-v}"; test "$T" = "$PKG" && echo "guard PASS for $TAG"
  TAG="ts-v9.9.9";  T="${TAG#ts-v}"; test "$T" = "$PKG" || echo "guard correctly REJECTS $TAG"
  ```
  Expected: `guard PASS for ts-v0.10.0` and `guard correctly REJECTS ts-v9.9.9`.

- [ ] **Step 6: Locally exercise the new CI steps** (the `pack --dry-run` smoke + typecheck the CI now runs).
  ```bash
  cd sdks/typescript
  npm run build && npm run typecheck && npm run test
  npm pack --dry-run 2>&1 | grep -E "dist/index\.(js|d\.ts)" && echo "tarball includes dist entrypoints"
  ```
  Expected: all green; the dry-run listing includes `dist/index.js` and `dist/index.d.ts` (plus `README.md`/`CHANGELOG.md` from `files`). If `dist/` entries are missing from the listing, `files` in `package.json` is wrong — fix Task 1.

- [ ] **Step 7: Commit (one commit for both workflow files).**
  ```bash
  git add .github/workflows/ci-typescript.yml .github/workflows/publish-typescript.yml
  git commit -m "ci(ts): add typecheck + npm pack to CI and a ts-v* publish workflow"
  ```

**Verification (Task 5 done-gate):** `ci-typescript.yml` has `npm run typecheck` (after build) and `npm pack --dry-run` (after test), no eslint/prettier; `publish-typescript.yml` exists, triggers on `ts-v*`, runs build+test+version-guard then `npm publish --provenance --access public` with `NODE_AUTH_TOKEN: NPM_TOKEN` and `id-token: write`; the guard passes for `ts-v0.10.0`; `npm pack --dry-run` lists `dist/index.js` + `dist/index.d.ts`; no `1.0.0` in either file.

---

### Task 6: Repo-level mentions + final 0.10.0 done-gate + open PR

> **VERSION RULE — binding:** the root README Languages row is **`v0.10.0`**; the llms.txt example tag is **`ts-v0.10.0`**. No `1.0.0` anywhere.

**Files:**
- `README.md` (repo root — Languages table row + Install snippet)
- `AGENTS.md` (repo root — line 3 + "Where To Find Things" rows + Releasing paragraph)
- `llms.txt` (repo root — §Release intro line + Tag-Convention row + a TypeScript release block)
- `skills/motosan-ai/SKILL.md` (CONDITIONAL — edit only if it lists SDKs/versions; see Step 5)

**Type:** DOCS + VERIFY / ships in the M7 release PR. Commands run from the **repo root** unless noted. This is the final task: it also runs the whole-milestone done-gate and opens the single release PR.

Verified anchor strings to edit (exact, from the current files):
- Root `README.md` Languages table has rows for Rust (`v0.18.0`) and Python (`v0.12.1`), and an Install block with Rust `Cargo.toml` + Python `pip install` snippets. The Providers table is keyed by Rust feature / Python extra.
- `AGENTS.md` line 3: `Multi-provider AI SDK. Rust (\`sdks/rust/\`) + Python (\`sdks/python/\`). Independent idiomatic implementations — no shared runtime.` The "Where To Find Things" table has rows `Rust SDK entry point | \`sdks/rust/src/lib.rs\` → \`client.rs\`` and `Python SDK entry point | \`sdks/python/motosan_ai/client.py\`` and `Provider implementations | \`sdks/rust/src/providers/\`, \`sdks/python/motosan_ai/providers/\``. The "Releasing" paragraph lists `rust-vX.Y.Z` / `python-vX.Y.Z` triggers.
- `llms.txt` §Release intro line: `Python and Rust SDKs are versioned and released **independently**.` Tag-Convention table has Python/Rust/oauth rows. Release-Steps blocks exist for Python and Rust.

---

- [ ] **Step 1: Root `README.md` — add the TypeScript Languages row + Install snippet.**
  Add to the Languages table (after the Python row):
  ```
  | TypeScript | [`@motosan-ai/sdk`](https://www.npmjs.com/package/@motosan-ai/sdk) | v0.10.0 |
  ```
  Add a TypeScript snippet to the Install block (after the Python `pip install` block):
  ````
  ```bash
  # TypeScript / Node (ESM, Node >= 18)
  npm install @motosan-ai/sdk
  ```
  ````
  Optional (lower priority): add a one-line note under the Providers table — "All five providers ship in the single `@motosan-ai/sdk` npm package (tree-shaken via ESM)." Do NOT add a per-provider TypeScript column unless trivial.
  > Per the contract, do NOT touch the stale Rust `v0.18.0`→`v0.19.0` / Python `v0.12.1` rows unless the user explicitly asks — the binding requirement is only the TS row. (If the user later confirms, that's a separate edit.)

- [ ] **Step 2: `AGENTS.md` — four edits.**
  - Line 3: change to include TypeScript:
    `Multi-provider AI SDK. Rust (\`sdks/rust/\`) + Python (\`sdks/python/\`) + TypeScript (\`sdks/typescript/\`). Independent idiomatic implementations — no shared runtime.`
  - "Where To Find Things" table: add a row for the TS entry point (after the Python entry-point row):
    `| TypeScript SDK entry point | \`sdks/typescript/src/index.ts\` → \`client.ts\` |`
    and extend the "Provider implementations" row to include the TS path:
    `| Provider implementations | \`sdks/rust/src/providers/\`, \`sdks/python/motosan_ai/providers/\`, \`sdks/typescript/src/providers/\` |`
  - **Workflow-inventory rows** (AGENTS.md ~lines 29-30) — extend BOTH so the new TS workflows are listed (they are added by Task 5):
    `| CI workflows | \`.github/workflows/ci-rust.yml\`, \`ci-python.yml\`, \`ci-typescript.yml\` |`
    `| Release workflows | \`.github/workflows/publish-rust.yml\`, \`publish-python.yml\`, \`publish-typescript.yml\`, \`publish-motosan-ai-oauth.yml\`, \`publish-codex-oauth.yml\`, \`publish-anthropic-oauth.yml\` |`
    (Note: `ci-typescript.yml` already exists on main; this row was simply never updated for it. `publish-typescript.yml` is new in Task 5.)
  - "Releasing" paragraph: add a sentence: `Tag \`ts-vX.Y.Z\` triggers \`publish-typescript.yml\` → npm.` (Mirror the existing rust/python sentences.)

- [ ] **Step 3: `llms.txt` — three edits in §Release.**
  - Intro line: change `Python and Rust SDKs are versioned and released **independently**.` → `Python, Rust, and TypeScript SDKs are versioned and released **independently**.`
  - Tag-Convention table: add a row:
    `| TypeScript   | \`ts-vX.Y.Z\`           | \`ts-v0.10.0\`         |`
  - Add a "### Release Steps (TypeScript)" block paralleling the Python/Rust ones (place it after the Rust block):
    ````markdown
    ### Release Steps (TypeScript)

    ```bash
    # 1. Bump version
    #    sdks/typescript/package.json → "version": "X.Y.Z"

    # 2. Update CHANGELOG
    #    sdks/typescript/CHANGELOG.md → ## [X.Y.Z] - YYYY-MM-DD

    # 3. Update version references in:
    #    - README.md (root) — Languages table
    #    - AGENTS.md — Releasing paragraph
    #    - llms.txt — Tag Convention + this section

    # 4. Commit
    git add sdks/typescript/package.json sdks/typescript/CHANGELOG.md README.md AGENTS.md llms.txt skills/motosan-ai/SKILL.md
    git commit -m "chore: release ts-vX.Y.Z"

    # 5. Tag + push (triggers publish-typescript.yml → npm)
    git tag ts-vX.Y.Z
    git push origin main ts-vX.Y.Z
    ```
    ````
    Also, in the "### CI Pipeline" list, add: `- **publish-typescript.yml**: \`npm ci\` → \`npm run build\` → \`npm run test\` → version-matches-tag guard → \`npm publish --provenance --access public\` (secret: \`NPM_TOKEN\`)`.
    And in the "### Emergency Manual Publish" block add:
    ````bash
    # TypeScript
    cd sdks/typescript && npm ci && npm run build && npm publish --access public
    ````

- [ ] **Step 4: Verify the doc edits landed and contain no forbidden version.**
  ```bash
  grep -n "@motosan-ai/sdk" README.md            # Languages row + install snippet
  grep -n "v0.10.0" README.md                    # the TS version
  grep -n "typescript" AGENTS.md                  # line 3 + entry-point/providers rows
  grep -n "ts-v" llms.txt                         # tag convention + release block
  grep -rn "1\.0\.0" README.md AGENTS.md llms.txt | grep -v "keepachangelog" || echo "NO 1.0.0 in repo docs — good"
  ```
  Expected: each grep matches; `NO 1.0.0 in repo docs — good`.

- [ ] **Step 5 (CONDITIONAL): `skills/motosan-ai/SKILL.md`.** It currently says `Multi-provider LLM SDK — Python 0.12.1 / Rust 0.19.0` and lists Python/Rust install snippets + an env-var/model-defaults table (it does NOT currently mention TypeScript). Per the repo Release Checklist this file is in scope. Add TypeScript minimally:
  ```bash
  grep -n "Python 0.12.1 / Rust 0.19.0" skills/motosan-ai/SKILL.md
  ```
  If that header line exists, update it to mention TypeScript (e.g. `Multi-provider LLM SDK — Python 0.12.1 / Rust 0.19.0 / TypeScript 0.10.0`) and add a TypeScript install one-liner (`npm install @motosan-ai/sdk`) to the Install block. The model-defaults / env-var tables already match TS defaults, so no further change. If the header line is NOT present (file changed), skip — do not invent structure.

- [ ] **Step 6: Commit the docs.**
  ```bash
  git add README.md AGENTS.md llms.txt skills/motosan-ai/SKILL.md
  git commit -m "docs: list TypeScript SDK (0.10.0) in root README, AGENTS, llms.txt, SKILL"
  ```

- [ ] **Step 7: FULL MILESTONE DONE-GATE** (run every contract Done-criterion).
  ```bash
  # (a) zero official-provider SDKs in src AND empty peerDeps
  grep -rE "@anthropic-ai/sdk|from 'openai'" sdks/typescript/src && echo "FAIL: official SDK ref" || echo "PASS: no official SDK in src"
  node -p "Object.keys(require('./sdks/typescript/package.json').peerDependencies).length === 0 ? 'PASS: peerDeps empty' : 'FAIL'"

  # (b) build + typecheck + test green (from sdks/typescript)
  ( cd sdks/typescript && npm run build && npm run typecheck && npm run test ) && echo "PASS: build+typecheck+test"

  # (c) npm pack includes dist entrypoints
  ( cd sdks/typescript && npm pack --dry-run 2>&1 | grep -E "dist/index\.(js|d\.ts)" ) && echo "PASS: pack includes dist"

  # (d) version is exactly 0.10.0
  node -p "require('./sdks/typescript/package.json').version === '0.10.0' ? 'PASS: version 0.10.0' : 'FAIL: '+require('./sdks/typescript/package.json').version"

  # (e) README + CHANGELOG exist; CHANGELOG top entry is [0.10.0]
  test -f sdks/typescript/README.md && test -f sdks/typescript/CHANGELOG.md && echo "PASS: README+CHANGELOG present"
  grep -m1 "^## \[0.10.0\]" sdks/typescript/CHANGELOG.md && echo "PASS: CHANGELOG top entry 0.10.0"

  # (f) workflows in place
  test -f .github/workflows/publish-typescript.yml && grep -q "ts-v" .github/workflows/publish-typescript.yml && echo "PASS: publish workflow on ts-v*"
  grep -q "npm run typecheck" .github/workflows/ci-typescript.yml && grep -q "npm pack" .github/workflows/ci-typescript.yml && echo "PASS: CI typecheck+pack"

  # (g) root README Languages row + AGENTS + llms mention TS
  grep -q "@motosan-ai/sdk" README.md && echo "PASS: root README TS row"
  grep -q "typescript" AGENTS.md && grep -q "ts-v" llms.txt && echo "PASS: AGENTS + llms TS mention"

  # (h) NO 1.0.0 / v1.0.0 as the TS version anywhere in M7 deliverables
  grep -rnE "v?1\.0\.0" sdks/typescript/package.json sdks/typescript/README.md sdks/typescript/CHANGELOG.md .github/workflows/publish-typescript.yml .github/workflows/ci-typescript.yml | grep -v "keepachangelog" && echo "FAIL: 1.0.0 found" || echo "PASS: no 1.0.0 as TS version"
  ```
  Expected: every line prints `PASS: …` (and the two `FAIL` branches do NOT fire). If any FAILs, fix in the owning task before opening the PR.

- [ ] **Step 8: Push the branch and open ONE release PR** (let `ci-typescript.yml` run; the publish tag is created only AFTER merge).
  ```bash
  git push -u origin m7-ts-hardening-0.10.0
  gh pr create --base main --head m7-ts-hardening-0.10.0 \
    --title "M7: TypeScript SDK release hardening → 0.10.0" \
    --body "$(cat <<'EOF'
  M7 (final milestone) — release hardening for the TypeScript SDK, no new wire/provider work.

  - package.json: 0.3.0 → 0.10.0; ESM `exports` map, `engines.node>=18`, repository/homepage/bugs/license/author/keywords metadata, `sideEffects:false`, `typecheck`+`prepublishOnly` scripts; peerDependencies stay `{}`.
  - README.md + CHANGELOG.md (consolidated 0.10.0 entry: dropped `@anthropic-ai/sdk`+`openai` peer deps, `minimaxEndpoint`→`minimaxBaseUrl`, `ToolCall.input` widening, default-model changes).
  - Edge-case + cross-provider parity tests (empty messages, malformed/null SSE skip, mid-stream partial success, unexpected status codes, serialize/parse parity).
  - CI: added `npm run typecheck` + `npm pack --dry-run`. New `publish-typescript.yml` on `ts-v*` (provenance, `--access public`, version-matches-tag guard).
  - Root README / AGENTS.md / llms.txt now list the TypeScript SDK.

  Done-gate: no official provider SDKs in `src`; peerDeps empty; build+typecheck+test green; `npm pack` includes `dist/index.js`+`dist/index.d.ts`; version exactly `0.10.0` (NOT 1.0).

  🤖 Generated with [Claude Code](https://claude.com/claude-code)
  EOF
  )"
  ```
  Expected: PR opened against `main`; CI (`ci-typescript.yml`) starts.

- [ ] **Step 9: After CI is green and the PR is merged, tag the release** (NOT before merge):
  ```bash
  git checkout main && git pull --ff-only
  git tag ts-v0.10.0
  git push origin ts-v0.10.0   # triggers publish-typescript.yml → npm
  ```
  Expected: `publish-typescript.yml` runs, the version-matches-tag guard passes (`0.10.0` == `0.10.0`), and `npm publish --provenance --access public` publishes `@motosan-ai/sdk@0.10.0`.

**Verification (Task 6 / whole-milestone done-gate):** every `PASS:` in Step 7 prints; root README Languages table has the `@motosan-ai/sdk … v0.10.0` row + install snippet; AGENTS.md line 3 + entry-point/providers rows + Releasing sentence mention TypeScript; llms.txt §Release intro + Tag-Convention + a TypeScript release block + CI-pipeline line added; SKILL.md updated iff it lists SDK versions; single release PR opened through CI; `ts-v0.10.0` tagged only after merge; the literal `1.0.0` / `v1.0.0` appears NOWHERE as the TS SDK version in any M7 deliverable.

---

## Milestone Done Criteria (verify all before tagging `ts-v0.10.0`)

- [ ] `package.json` version is **`0.10.0`** (NOT 1.0.0), has an ESM `exports` map, `engines.node>=18`, repository/homepage/bugs/license(MIT)/keywords/`sideEffects:false`, and **zero `peerDependencies`**.
- [ ] `README.md` exists and documents install, the 5 providers (incl. Ollama native+compat & Gemini), streaming/collectStream, tools/tool_choice, vision/images, extended thinking, MCP, retry, ClientBuilder + model defaults, error model, setup-token OAuth, testing.
- [ ] `CHANGELOG.md` exists (Keep-a-Changelog style) with a `0.10.0` top entry documenting the full breaking-change history above.
- [ ] New edge-case + cross-provider parity tests pass; `npm pack --dry-run` lists `dist/index.js` + `dist/index.d.ts`; full suite green.
- [ ] `ci-typescript.yml` runs build + `typecheck` (tsc --noEmit) + test (+ pack dry-run); `publish-typescript.yml` exists, triggers on `ts-v*`, has a version-matches-tag guard, and publishes with provenance.
- [ ] Root `README.md` / `AGENTS.md` / `llms.txt` list the TypeScript SDK; `npm run build` + `npm run test` green.
- [ ] **`1.0.0` / `v1.0.0` appears NOWHERE as the TS SDK version** in any M7 deliverable (the T6 done-gate greps for this).

## Execution Handoff

Two ways to execute (the user runs their own subagents — a copy-paste prompt sheet can accompany this plan):
1. **Subagent-driven (recommended):** one fresh subagent per task, review between tasks.
2. **Inline:** execute tasks in-session with checkpoints.

After merge, tag `ts-v0.10.0` to trigger the publish workflow. (Do NOT tag 1.0.)

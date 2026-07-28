# motosan-ai (TypeScript SDK)

Multi-provider TypeScript/ESM SDK for Anthropic, OpenAI, MiniMax, Ollama, Gemini, and ChatGPT Codex.
Self-implemented wire protocol via native `fetch` — **zero official-provider-SDK dependencies**
(no `@anthropic-ai/sdk`, no `openai`). All six providers ship in one package and tree-shake
cleanly via ESM.

## Installation

```bash
npm install @motosan-ai/sdk
# or
pnpm add @motosan-ai/sdk
# or
yarn add @motosan-ai/sdk
```

There are no extras or feature flags: all six providers ship in the single npm package and
are tree-shaken via ESM (only what you import lands in your bundle).

## Requirements

- **Node >= 20.3** (the SDK uses `AbortSignal.any` / `AbortSignal.timeout` for timeout and cancellation composition).
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

### ChatGPT Codex

```ts
import { Client, user } from '@motosan-ai/sdk'

const client = Client.builder()
  .chatgptCodex(accessToken, accountId, undefined, { reasoningEffort: 'medium' })
  .build()

const resp = await client.chat({ messages: [user('Hello')] })
```

ChatGPT Codex streams the OpenAI Responses API at `https://chatgpt.com/backend-api/codex/responses`
using a caller-supplied OAuth `accessToken` + `accountId` (codex CLI headers) — **no API key**.
The default model is `gpt-5.5`; it is text-only. A per-request `providerOptions.reasoning_effort`
(string) overrides the provider-default `reasoningEffort`; otherwise no `reasoning` is sent.

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

A stream is complete only when the provider sends its terminal event (OpenAI `[DONE]` or a final
`finish_reason` chunk — either suffices, Anthropic `message_stop`, Gemini / chatgpt-codex terminal
frames). Since v0.14.0, if the upstream closes without any such event the stream throws
`IncompleteStreamError` (subclass of `StreamError`) — `"incomplete stream: <provider> ended without a
terminal event"`. `event.stopReason` carries the provider's reported reason when present.

> **Mid-stream failures:** provider `error` frames and transport faults reject the stream (since 0.12.0),
> and truncation (EOF without a terminal event) rejects with `IncompleteStreamError` (since 0.14.0).
> Retries apply only to the *initial* fetch, never mid-stream (see Retry). Aborting via your own
> `AbortSignal` throws `CancelledError` and is never retried.

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

const rp = new RetryPolicy() // tune via its builder methods, e.g. .withMaxRetries(5).withBaseDelayMs(200).withRespectRetryAfter(true)

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
| ChatGPT Codex | `gpt-5.5`        |

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
git tag ts-vX.Y.Z
git push origin ts-vX.Y.Z
```

The Rust, Python, and TypeScript SDKs are versioned and released independently.

## For AI Agents

If you're an AI coding assistant, fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt)
for a quick-start guide with API examples, tool-use patterns, and streaming setup.

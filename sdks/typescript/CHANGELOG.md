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

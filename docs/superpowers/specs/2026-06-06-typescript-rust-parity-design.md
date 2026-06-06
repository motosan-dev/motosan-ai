# TypeScript SDK → Rust SDK Parity — Design & Milestone Plan

**Date:** 2026-06-06
**Status:** Reviewed (accept-with-revisions applied) — awaiting user sign-off
**Reference target:** Rust SDK `motosan-ai` v0.19.0
**Starting point:** TypeScript SDK `@motosan-ai/sdk` v0.3.0

---

## 1. Background

The three SDKs have diverged sharply:

| | TypeScript | Rust | Python |
|---|---|---|---|
| Version | **0.3.0** | **0.19.0** | 0.12.1 |
| Core LOC (no tests) | 621 | ~12,894 | 4,709 |
| Source files | 7 | 38 | — |
| Tests | 5 files / 133 lines | 34 files / 8,702 lines | 68 files |
| README / CHANGELOG | none / none | yes / yes | yes / yes |

The core architectural difference: **TS today is a thin wrapper** that delegates `chat`/`stream` to the official `@anthropic-ai/sdk` and `openai` npm packages (peer deps). Rust and Python **self-implement the wire protocol** (raw HTTP + SSE + own serialization), which is what unlocks thinking, `cache_control`, MCP, multimodal, retry, and provider-precise behavior.

## 2. Locked Decisions

1. **Architecture — self-implement the wire protocol.** TS will use raw `fetch` + SSE/NDJSON parsing + its own serialization, like Rust/Python, and **drop the dependency on `@anthropic-ai/sdk` and `openai`**. Target: zero official-SDK peer dependencies at 1.0.
2. **Scope — core full feature parity** for all **API-key HTTP providers** (anthropic, openai, minimax, gemini, ollama) and the **full type system**: structured content blocks, multimodal (images/documents), extended thinking, MCP server config, `tool_choice`, `cache_control`/`SystemBlock`, full streaming event taxonomy, retry, `ProviderCapabilities`, models registry, `think_stripper`.
3. **Deferred (future, second phase):** OAuth (anthropic/codex/gemini login flows, PKCE, callback server), CLI backends (`claude_code`, `codex_cli`, `gemini_cli`), and **`gemini-code-assist`** (it authenticates via OAuth, so it is coupled to the OAuth phase). The `ClientBuilder` will reserve the seam for optional-api-key (CLI) providers so this is not a later retrofit.

## 3. Architecture (target TS module layout)

```
sdks/typescript/src/
  types.ts            # structured types: ContentBlock, Role, Usage, StopReason,
                      #   StreamEventType, StreamEvent, Message, Tool, ToolChoice,
                      #   ThinkingConfig, SystemBlock, McpServerConfig, ChatRequest/Response
  message.ts          # message factory (user/assistant/tool + multimodal + cache helpers)
  stream.ts           # BoxStream (AsyncIterable<StreamEvent>), StreamEvent ctors, collectStream()
  error.ts            # error classes + classification utils (retryable status/network, parseRetryAfter)
  retry.ts            # RetryPolicy (M3)
  think_stripper.ts   # ThinkStripper (M3)
  models.ts           # model constant registries + defaults (M3)
  client.ts           # Client + ClientBuilder (M3)
  provider.ts         # Provider union + dispatchChat/dispatchStream routing (M3)
  http/
    fetch.ts          # raw fetch wrapper (postJson/postStream, header injection, error extraction)
    sse.ts            # SSE line parser (partial-chunk safe; [DONE] is ADVISORY)
    ndjson.ts         # NDJSON parser (skeleton M1, finalized M5 for Ollama)
  serialize/
    anthropic.ts      # Anthropic request serializer (nested input_schema, content blocks, cache)
    openai.ts         # OpenAI request serializer (flat function.parameters, role:system, tool_calls)
    gemini.ts         # Gemini serializer (contents/parts, functionDeclarations) (M6)
  providers/
    anthropic.ts      # self-implemented /v1/messages (M1)
    openai.ts         # self-implemented /v1/chat/completions (M2)
    minimax.ts        # Anthropic-compat re-route (M4)
    ollama.ts         # native /api/chat NDJSON + OpenAI-compat (M5)
    gemini.ts         # generativelanguage REST (M6)
```

**Unit of isolation:** each provider owns wire I/O only; serialization lives in `serialize/`; cross-cutting concerns (retry, routing, capabilities validation, think-stripping) live in `client.ts`/`provider.ts`. The `serialize/` split is the single most important boundary — it is where the Anthropic-vs-OpenAI tool-call format divergence (CLAUDE.md's #1 bug source) is contained and tested.

## 4. Milestone Roadmap

Dependency spine: **structured types + raw fetch/SSE foundation** → validate against **Anthropic** (only provider exercising every event type) → **per-provider serialization split** (OpenAI) → **orchestration layer** (builder/routing/retry/capabilities) → **leaf providers** (MiniMax/Ollama/Gemini) → **hardening/1.0**.

| M | Version | Theme | Depends | Effort |
|---|---|---|---|---|
| M1 | 0.4.0 | Foundation: structured types + Anthropic raw wire (SSE) + StreamEvent taxonomy + collectStream; drop `@anthropic-ai/sdk` | — | **4–5 wk** |
| M2 | 0.5.0 | OpenAI raw wire + per-provider serialization split + `tool_choice` + `system_blocks` + `cache_control`; drop `openai`; interim MiniMax rewire | M1 | 3 wk |
| M3 | 0.6.0 | ClientBuilder + Provider routing + RetryPolicy + ProviderCapabilities + think_stripper + models registry | M2 | 3 wk |
| M4 | 0.7.0 | MiniMax → Anthropic-compat wire + MCP server config + extended-thinking request config | M3 | 2.5 wk |
| M5 | 0.8.0 | Ollama provider (native `/api/chat` NDJSON + OpenAI-compat + auto-routing) | M3 | 2.5 wk |
| M6 | 0.9.0 | Gemini provider (generativelanguage REST) + image content blocks | M3 | 2.5 wk |
| M7 | 1.0.0 | Hardening: full test matrix, edge cases, README/CHANGELOG, CI, npm publish, ESM packaging | M4,M5,M6 | 2.5 wk |

**Total: ~20–21 person-weeks** (one engineer ≈ 5 months). The **M1→M2→M3 spine is strictly serial** (~10–11 wk) and cannot be parallelized — a second engineer is idle through it. After M3, M4/M5/M6 are symmetric independent 2.5-wk tasks (each depends on M3 only), so two engineers finish them in ≈5 wk, then M7 (+2.5 wk): **two engineers ≈ 4 months** wall-clock, not less.

Each milestone leaves the SDK in a shippable, working state: M1 = Anthropic-only self-hosted; M2 adds OpenAI (+ keeps MiniMax working via interim rewire); M3 makes it configurable + resilient; M4–M6 add remaining providers; M7 declares stable 1.0 with zero official-SDK deps.

### Milestone detail

**M1 — Foundation (0.4.0).** Replace the `@anthropic-ai/sdk` wrapper with a self-implemented raw fetch + SSE Anthropic provider on a full structured type system, the StreamEvent taxonomy, and `collectStream`. Done when: no `@anthropic-ai/sdk` import remains (grep clean) and removed from package.json; mocked-fetch tests prove content-block / tool_use / tool_result / system serialization and response parsing incl cache tokens, thinking, tool_calls, stopReason; a hand-authored-SSE-transcript test asserts the full StreamEvent sequence (Text, ToolCall Start/Args/End, ThinkingDelta/Done, Usage, done+stopReason); `collectStream` reassembles a synthetic stream; env-gated live Anthropic test passes; `npm run build` + `npm run test` green.

**M2 — OpenAI + serialization split (0.5.0).** Self-implement `/v1/chat/completions` (chat + SSE), introduce `serialize/openai.ts`, and pull `tool_choice`/`system_blocks`/`cache_control`/`stop_sequences` into a shared, provider-aware serialization layer. Drop `openai`. **Import topology** (decide explicitly — relocating the serializer silently breaks MiniMax otherwise): `serialize/openai.ts` is the canonical serializer export; `providers/openai.ts` imports and uses it internally; `providers/minimax.ts` is updated in M2 to import the serializer **from `serialize/openai.ts`** (not via `OpenAIProvider.serializeMessages`, which `minimax.ts:22,102` calls today). This is a same-format source swap, not a cross-format migration — current `minimax.ts` already uses the OpenAI serializer + OpenAI tool format against MiniMax's OpenAI-compat endpoint. **OpenAI tool-call accumulation differs from Anthropic**: each streaming `delta.tool_calls[]` element carries an `index`; map index→current tool id to assemble args (contrast Anthropic's incremental `input_json_delta` on the open block). Done when: zero official-SDK imports remain; tests prove the schema divergence (Anthropic `tools:[{name,description,input_schema}]` vs OpenAI `tools:[{type:function,function:{name,description,parameters}}]`, assistant `tool_calls.function.arguments` as JSON string); `tool_choice` + `system_blocks` tests pass for both; collectStream OpenAI-style accumulation test passes; cached-user-message invariant (cf39e8c) test passes; **MiniMax verification gate — a mocked-fetch serialization test asserts `minimax.ts` compiles and still serializes via the relocated `serialize/openai.ts` after `openai.ts` is rewritten to raw fetch** (closes the "keeps MiniMax working" promise at the 0.5.0/0.6.0 tags; first live MiniMax test lands at M4); live OpenAI test passes.

**M3 — Orchestration (0.6.0).** Fluent `ClientBuilder`, `Provider` discriminated union + dispatch routing (designed extensible / pluggable provider arms), full `RetryPolicy` wired into every provider's chat() and initial stream() fetch, `ProviderCapabilities` validation, `ThinkStripper`, models registry, stream-read-timeout. **think_stripper buffer pinned to `'</think>'.length - 1 = 7`** with an explicit flush-before-done regression test (guards the v0.15.3 4-char truncation bug). Builder reserves the optional-api-key seam for future CLI backends. Done when: builder produces a working client and throws `ConfigError` on missing key for HTTP providers; RetryPolicy unit tests (backoff cap, deterministic jitter, parseRetryAfter, isRetryableStatus/network) pass; mock-server test: 429→200 retries honoring Retry-After, 400 does not retry, streaming retries only on initial fetch; ThinkStripper suite (cross-chunk, split tags, flush-in-think) passes; capabilities reject image on text-only provider before any HTTP call.

> **Mid-stream failure contract** (must match Rust, not intuition): a transport/SSE error *after* the stream has started is **silently swallowed** — the stream terminates without a `done` event and `collectStream` returns a **partial, success-looking `ChatResponse` with a fabricated `stop_reason`** (NOT a surfaced error). `BoxStream` yields `StreamEvent`, never `Result`. Retries apply ONLY to the initial fetch, never mid-stream. Ref: Rust `providers/anthropic.rs:1114` (`Err(_) => continue`) + `stream.rs:111-115` (fabricated stop_reason on early `None`). Add a test feeding a stream that errors mid-flight and asserting this partial-success behavior.

**M4 — MiniMax + MCP + thinking config (0.7.0).** Switch MiniMax to the Anthropic-compatible `/anthropic/v1/messages` wire (text-only, MiniMax-M2.7 default); add `McpServerConfig`/`McpServerType`/`McpToolConfig` + Anthropic serialization + beta headers; add extended-thinking **request** serialization (adaptive for Opus 4.x, `enabled`+`budget_tokens` otherwise with forced `temperature=1.0`). Done when: MiniMax posts Anthropic-wire body (not legacy `/v1/text/chatcompletion`); thinking-config tests (adaptive vs enabled, temperature override collision); MCP serialization + beta header tests; non-Anthropic providers reject MCP with `UnsupportedFeatureError`; live MiniMax test passes.

**M5 — Ollama (0.8.0).** Native `/api/chat` NDJSON path (`think`/`keep_alive`/`num_ctx`, thinking extraction, 3-event tool-call streaming) + OpenAI-compat path + dispatch **auto-routing** (native when `ollamaNative` or any tuning field set). Done when: native chat posts to `{base}/api/chat` with correct options; NDJSON streaming reconstructs text/thinking/tool-calls terminating on `{done:true}`; auto-routing test routes to native in both chat and stream; builder validation rejects tuning fields on non-Ollama providers; live Ollama test passes.

**M6 — Gemini (0.9.0).** `generateContent` + `streamGenerateContent?alt=sse`, full request/response serialization (contents/parts, `systemInstruction`, `functionDeclarations`, `toolConfig`), SSE adapter with synthesized tool-call events (client-generated ids) and `finishReason` mapping, retry, image content blocks. Reuses M1's SSE parser — which treats `[DONE]` as advisory so Gemini's no-sentinel/`finishReason`-terminated stream works. Done when: request-body test asserts `role:model`, `inlineData` for base64 images, `fileData.fileUri` for url, `functionResponse` for tool results, `tool_choice` mapping; SSE streaming test emits synthesized tool-calls + usage + done with mapped finishReason; image works, document rejected; live Gemini test passes.

**M7 — Hardening / 1.0 (1.0.0).** ~80+ mocked-fetch unit tests across all 5 providers + env-gated live tests; type roundtrip + edge-case tests (empty messages, malformed SSE JSON, mid-stream reset, unexpected status); eslint + prettier + `tsc --noEmit` CI gates; README + CHANGELOG (documents 0.4.0→1.0.0 evolution **and the default-model change** as a breaking change); ESM packaging verified via `npm pack`; `publish-typescript.yml` on `ts-v*` tags. Done when: grep confirms zero `@anthropic-ai/sdk`/`openai` references and zero peer deps; lint/format/type-check pass; full suite green; tarball resolves under NodeNext ESM.

## 5. Milestone 1 — Detailed Task Breakdown

> M1 is the foundation and the highest-density milestone. Tasks are ordered so each builds on the previous; tests are written per task (TDD).

1. **Rewrite `types.ts`** — structured content blocks + enums + expanded request/response/stream types. `Role` union (lowercase-serialized); `ContentBlock` discriminated union (`text`|`image`|`document`) with `ImageSource`/`DocumentSource` (`base64`|`url`); `ToolCall {id,name,input}` (field name `input` per CLAUDE.md); `Usage {inputTokens,outputTokens,cacheCreationInputTokens?,cacheReadInputTokens?}`; `StopReason` union; `StreamEventType` union (text|tool_call_start|tool_call_args|tool_call_end|usage|thinking_delta|thinking_done); expanded `StreamEvent {content,done,eventType,toolCallId?,toolCallName?,toolCallArgsDelta?,usage?,stopReason?}`; `Message {role,content,contentBlocks?,toolCallId?,toolCalls?,cache?}`; `ThinkingConfig {budgetTokens}`; `SystemBlock {text,cacheControl?}`; `ToolChoice` (placeholder, fully serialized M2); `ChatRequest` + `system?/systemBlocks?/systemCache?/thinking?/stopSequences?`; `ChatResponse.thinking?`. *Tests:* JSON roundtrip for each ContentBlock variant, Message with blocks, each StreamEvent eventType, Usage with/without cache tokens; assert undefined optionals omitted. *Done:* compiles strict; roundtrip passes; no flat-string-only assumption remains.

2. **`message.ts` factory** — `user`, `userWithCache`, `assistant`, `assistantWithToolCalls`, `system`, `tool`, `toolResult`, `userWithImage`, `userWithBlocks`, `userWithPdfBase64`, `userWithPdfUrl`, `userWithPdfBytes` (base64 via Buffer), `withCache`. `content` stays a flat string extracted from the first text block for backward compat; `contentBlocks` holds the structured form. *Tests:* image/PDF/cache/toolResult shapes. *Done:* constructors match documented shape.

3. **`http/sse.ts` + `http/ndjson.ts` skeleton** — async generator over a `ReadableStream<Uint8Array>`; `TextDecoder`; buffer across chunks; split on `\n\n`; parse `event:`/`data:`; `JSON.parse` (skip malformed); **`[DONE]` is recognized but advisory — completion is decided by the per-provider adapter** (required for Gemini M6). *Tests:* synthetic byte stream split mid-line and mid-JSON reassembles; malformed line skipped; `[DONE]` recognized. *Done:* handles partial chunks/split JSON without throwing.

4. **`http/fetch.ts`** — `postJson(url,headers,body)` (parsed JSON or mapped error); `postStream(url,headers,body)` (returns body reader); `extractErrorMessage` (`{error:{message}}` with fallbacks); `AbortController` hook for future timeout; **no retry here** (M3). *Tests:* mocked global fetch — postJson on 200, error extraction for Anthropic/OpenAI shapes. *Done:* works against mocked fetch.

5. **`error.ts` extensions** — add `StreamReadTimeoutError` (carries `timeoutSecs`), `UnsupportedFeatureError`, keep `StreamError`; `isRetryableStatus` (429 || ≥500), `isRetryableNetworkError` (AbortError/TypeError/ECONNREFUSED/ENOTFOUND/ETIMEDOUT), `parseRetryAfter` (int seconds → ms). *Tests:* status matrix, parseRetryAfter valid/invalid, network classification. *Done:* matches Rust `providers/mod.rs` semantics.

6. **`stream.ts` — BoxStream + constructors + `collectStream`** — `BoxStream = AsyncIterable<StreamEvent>`; constructor helpers (text, done, doneWithStopReason, usage, toolCallStart, toolCallArgs[WithId], toolCallEnd[WithId], thinkingDelta, thinkingDone); `collectStream` accumulates text, builds tool calls (start→buffer, args→append, end→`JSON.parse`), sums usage (cache tokens optional). **Thinking is three-way** (mirror `stream.rs:100-122`): prefer `thinkingDone` text *only if non-empty*; an **empty** `thinkingDone` (block existed but produced nothing) yields `thinking: undefined`, not `""`; fall back to the delta buffer *only when `thinkingDone` never fired*. stopReason explicit > heuristic (`toolCalls.length>0 ? tool_use : end_turn`). *Tests:* (a) no thinking, (b) empty thinking block → undefined, (c) delta-only, (d) both present → prefer thinkingDone; correct content/tool input/usage/stopReason. *Done:* matches Rust `collect_stream`.

7. **`serialize/anthropic.ts`** — `serializeAnthropicRequest(req,model)` → `{model,max_tokens,messages,system?,tools?,thinking?,stop_sequences?,temperature?,...providerOptions}`. Messages: contentBlocks → block array; `cache=true` sets `cache_control:{type:ephemeral}` on last block; assistant tool_calls → `tool_use` blocks; tool role → user message with `tool_result`. system: systemBlocks → per-block cache_control array; else `systemCache=true` → single-block array (**`system_cache` forces system-as-array even for non-OAuth requests**, per `anthropic.rs:345-351`); else plain string. tools: flatten to `{name,description,input_schema}`, cache on last. (tool_choice/MCP/beta-headers/full thinking-config land M2/M4.) *Tests:* content blocks, tool_use/tool_result, **cached-user-message → block array with cache_control (cf39e8c invariant — pure in-memory body assertion, fully testable now)**, system string with cache=false → plain string, system_cache=true → array, system_blocks → per-block array. *Done:* output matches Anthropic wire for the M1 subset.

8. **`providers/anthropic.ts` rewrite** — remove `@anthropic-ai/sdk`; `constructor(apiKey, model?, baseUrl='https://api.anthropic.com')`. `chat()` → postJson to `{base}/v1/messages` with `x-api-key` + `anthropic-version:2023-06-01`; parse content (text→content, thinking→thinking, tool_use→toolCalls), usage incl cache tokens, stopReason. `stream()` → postStream + drive sse.ts: `message_start`→Usage; `content_block_start` (tool_use→ToolCallStart, thinking→open accumulator, redacted_thinking→ignore); `content_block_delta` (text_delta→Text, input_json_delta→ToolCallArgs with current tool id, thinking_delta→ThinkingDelta+accumulate, signature_delta→ignore); `content_block_stop` (close tool→ToolCallEnd, close thinking→ThinkingDone); `message_delta` (stash stop_reason + emit Usage — **note: `message_delta` Usage carries NO cache tokens; only `message_start` carries `cache_creation_input_tokens`/`cache_read_input_tokens`**, per `anthropic.rs:918-986`); `message_stop`→done with stashed stopReason. `capabilities()` → `{supportsImage:true,supportsDocument:true}`. *Tests:* mocked-fetch chat request-body + response parsing (assert cache tokens present from message_start, absent from message_delta); stream over a **hand-authored SSE transcript string** asserting full StreamEvent sequence; env-gated live chat+stream+tool. *Done:* works with no official SDK.
   > Note: M1 ships thinking *parsing* but the thinking *request* config lands in M4 — so the M1 streaming-thinking assertions are satisfied by the **hand-authored SSE transcript**, not by a live request. This is intentional; the live M1 test does not cover thinking. Template to copy: `sdks/rust/tests/anthropic_stream.rs` hand-authors small inline SSE strings fed to a mock server — there is no live-capture pipeline to build.

9. **Wire up `index.ts` / `client.ts` / `package.json`** — export new types, stream helpers, message factory, error additions, Anthropic provider. `client.ts` keeps the minimal constructor working for anthropic/openai/minimax (OpenAI/MiniMax stay on M1-era wrappers until M2/M4) but routes Anthropic through the new self-hosted provider; full builder comes M3. `package.json`: remove `@anthropic-ai/sdk` from peer/peerMeta/dev deps; keep `openai` until M2. *Tests:* `provider:'anthropic'` yields self-hosted provider; existing tests green. *Done:* `grep -r '@anthropic-ai/sdk' src` empty; not in package.json; build + test green.

## 6. Testing Strategy

- **Mocked-fetch unit tests** per provider: request-body serialization assertions + response parsing.
- **Hand-authored transcript stream tests**: feed small inline SSE/NDJSON byte strings (no live-capture pipeline — see `sdks/rust/tests/anthropic_stream.rs` for the portable template), assert the exact `StreamEvent` sequence.
- **Mid-stream failure test**: a stream that errors after starting must terminate with a partial, success-looking `ChatResponse` (fabricated stop_reason), not a surfaced error — matching Rust (see M3 contract).
- **`collectStream` tests**: synthetic streams (both Anthropic incremental and OpenAI indexed tool-call styles) → `ChatResponse`.
- **Mock-server retry tests** (M3): 429→200 with Retry-After, 400 no-retry, stream retries only on initial fetch.
- **Env-gated live integration tests**: one per provider, skipped when the key is absent.
- **Edge cases** (M7): empty messages, null fields, malformed SSE JSON, mid-stream reset, unexpected status codes.

## 7. Out of Scope (future second phase)

- **OAuth** — anthropic/codex/gemini login flows, PKCE, local callback server. Note: Anthropic OAuth changes wire behavior (Bearer vs x-api-key, always system-as-array, beta headers, forced streaming). The **cf39e8c cached-user-message invariant is fully testable in M1** via in-memory body assertion (the cached-USER path is not OAuth-branched); only the OAuth-specific *system-as-array forcing* and header changes are deferred — so M1/M2 cover the cache invariant fully, and only the OAuth wire-mode deltas remain for phase 2.
- **CLI backends** — `claude_code`, `codex_cli`, `gemini_cli` (subprocess spawn + stream-json). The `ClientBuilder` reserves an optional-api-key validation seam so these are not a retrofit.
- **gemini-code-assist** — OAuth-credential authenticated; coupled to the OAuth phase (GCA envelope wrapper, response-unwrap stream parser, tool-id dedup, `cachedContentTokenCount` cache-token math).

## 8. Risks (from adversarial review)

| Risk | Mitigation |
|---|---|
| MiniMax functional regression at 0.5.0/0.6.0 tags | M2 interim-rewire task + explicit import topology + mocked-fetch MiniMax verification gate in M2 Done-when |
| Mid-stream failure semantics mis-ported (clean-error vs swallow) | M3 behavior contract: swallow + partial success-looking response with fabricated stop_reason (matches Rust); explicit test |
| M1 under-estimated (~42 items, XL+L) | Re-budgeted to 4–5 weeks |
| Tool-call streaming format divergence (#1 bug source) | Contained in `serialize/`; collectStream tested for both styles (M1 Anthropic, M2 OpenAI) |
| Shared SSE parser breaks Gemini (no `[DONE]`) | `[DONE]` advisory; adapter decides completion |
| think_stripper wrong buffer → silent truncation | Buffer pinned to 7; flush-before-done regression test |
| Silent default-model change | Dedicated M7 breaking-change/migration note |
| Feature-gating (no Cargo features in TS) | ESM tree-shaking; per-provider modules |
| Anthropic thinking forces temperature=1.0 | M4 serializer overrides user temperature; test the collision case |

## 9. Provenance

Grounded in a 14-agent workflow: 12 parallel per-dimension deep readers (types, client, anthropic, openai, gemini, ollama, minimax, streaming, retry, errors, models/think_stripper, testing/tooling) comparing Rust `src/` against TS `src/` line-by-line, a synthesis pass, and an adversarial completeness critic whose findings are folded into §4–§8.

Then a second 17-agent adversarial review (4 lenses — wire-format correctness, sequencing/shippability, completeness vs Rust, estimation/testability — each verified against the actual code, blocker/major findings adversarially re-checked, synthesized to a verdict). Verdict: **accept-with-revisions**; 9 findings were knocked down as overblown or factually wrong about the code, 5 survived (4 minor + 1 nit) and are folded into §4–§8: the M2 MiniMax verification gate + import topology, the M3 mid-stream-failure contract, the corrected two-engineer estimate, the cf39e8c-is-fully-testable correction, and the hand-authored-transcript terminology.

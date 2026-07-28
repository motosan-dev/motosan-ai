# Freeform Tool Parity — Milestone Plan

**Tracking issue:** [#270](https://github.com/motosan-dev/motosan-ai/issues/270)
**Baseline:** `origin/main` @ `19020b4` · Rust 0.27.1 · Python 0.19.0 · TypeScript 0.15.0
**Source:** two read-only surveys run 2026-07-28 — one mapping the Rust 0.26.0 native API surface, one
mapping the Python/TypeScript landing zones.

**Goal:** port Rust's native model / Freeform-custom-tool API to Python and TypeScript, so all three SDKs
implement `specs/types.md` § Native Model API rather than one of them.

---

## Why this is the only parity milestone

Rust and the other two SDKs were last aligned at M4 (Rust 0.25.0 / Python 0.18.0 / TS 0.15.0). Since
then:

| Rust version | Content | Needs a port? |
|---|---|---|
| **0.26.0** | Native model API: `ModelChatRequest`, `ModelToolSpec`, `FreeformTool`, `ModelStreamDelta`, shared Responses codec | **Yes — this milestone** |
| 0.27.0 | `ThinkStripper` UTF-8 panic fix | **No.** The panic is a Rust artefact: `String` slices by byte index and panics off a char boundary. Verified empirically — Python (code-point slicing) and TypeScript (an `isCharBoundary` guard, plus an unguarded in-think branch whose buffer is always discarded) both handle the exact inputs that panicked Rust. |
| 0.27.0 | Native stream-termination contract | Rides this milestone — it describes the API being ported. |
| 0.27.1 | README only | No. |

Python 0.19.0 (central capability enforcement, `py.typed`) moved the other way: Python catching up to
things Rust and TS already had.

Separately and **out of scope here**: TypeScript still lacks a `GeminiCodeAssist` provider and all three
CLI backends. Those are deliberate deferrals from the TS rebuild, unrelated to Freeform.

---

## Locked decisions

These are settled; implementation plans should not re-litigate them. **D1 is the one worth challenging
before work starts** — it is the milestone's biggest scope lever.

### D1 — Both SDKs get an OpenAI Responses opt-in, not just `chatgpt_codex`

Rust exposes native support on two providers: ChatGPT Codex (native by default) and OpenAI **only** when
the caller opts in via `ClientBuilder::openai_responses_api(true)`. The landing zones differ:

- Python's `OpenAIProvider` speaks only `/v1/chat/completions` (`providers/openai.py:82`). No responses
  URL, no flag. A Responses path is genuinely new code.
- TypeScript is halfway: `responsesUrl` / `withResponsesUrl` / `withResponsesFallback` exist, but the
  fallback fires only on a 404 from chat-completions, is non-streaming, and hard-codes `toolCalls: []`
  and `stopReason: 'stop'` (`providers/openai.ts:242-248`). It is not an opt-in and cannot carry tools.

**Decision: implement the opt-in in both.** Without it `specs/types.md`'s "OpenAI supports the native API
only when callers opt into `openai_responses_api`" is unimplementable outside Rust, and the flag has no
analogue to port. The marginal cost is small because the codec is shared — what is actually new is an
endpoint, a boolean, and the non-streaming decode path.

**How the flag is exposed differs per SDK, and Python has no builder to hang it on.** Rust's
`ClientBuilder::openai_responses_api(bool)` has no Python analogue: Python constructs clients through
`Client(provider=..., ...)` and classmethod shortcuts. Thread it explicitly through all three levels, or
the flag ends up provider-only and unreachable from the facade:

| | Provider level | Client level | Shortcut |
|---|---|---|---|
| Python | `OpenAIProvider(..., responses_api: bool = False, responses_url: str \| None = None)` plus `_responses_endpoint()` beside the existing `_endpoint()` | new keyword-only `openai_responses_api: bool = False` on `Client.__init__`, forwarded when it builds `OpenAIProvider` | new keyword-only argument on `Client.openai(...)` — today its signature is a fixed list (`api_key`, `model`, `max_retries`, `retry_policy`) and silently drops anything else |
| TypeScript | `OpenAIProvider.withResponsesApi(boolean)`, beside the existing and **semantically different** `withResponsesFallback` | `ClientBuilder.openaiResponsesApi(boolean)` | — |

TypeScript's two flags must stay distinguishable in docs and in the builder: `withResponsesFallback` is a
404 recovery path, `withResponsesApi` is a native opt-in.

*The cheaper alternative, if this milestone needs to shrink:* ship `chatgpt_codex` only, and amend the
spec to say native OpenAI is Rust-only for now. That keeps the codec and every type identical; it only
drops two provider methods and a builder flag per SDK. Decide **before** drafting the implementation
plans, not during.

### D2 — Model the types in each SDK's own idiom, not by transliterating Rust

Four Rust types carry hand-written codecs because the discriminator value differs from the variant name
(`Freeform` → `"custom"` / `"custom_tool_call"` / `"custom_tool_call_output"`), and `FreeformTool`
injects a `type` field it never stores. Neither SDK can get these from automatic serialization.

- **Python:** variant dataclasses plus a union alias, following the `McpToolConfig` precedent
  (`types.py:288-314`) where the wire tag differs from the model — `isinstance`-discriminated, with free
  `*_to_dict` encoders. Frozen dataclasses for value types, non-frozen for requests, `StrEnum` for closed
  string sets, a separate fluent `ModelChatRequestBuilder` class with a `builder()` classmethod.
- **TypeScript:** inline discriminated unions of object literals. Use a **`kind`** tag on `ModelToolCall`
  / `ModelToolOutput`, not `type` — this is exactly the case `McpToolConfig` (`types.ts:86-89`) already
  uses `kind` for: the model shape and the wire shape disagree. Optional fields are omitted, never
  `undefined`.

Wire encoding never lives in the type modules: Python `providers/responses.py`, TypeScript
`serialize/responses.ts` (joining the existing `serialize/{anthropic,gemini,openai}.ts`).

### D3 — Do not port the three reject-only request fields

`ModelChatRequest` carries `thinking`, `mcp_servers`, and `mcp_tool_configs` with builder methods that
exist solely so validation can reject them (`providers/mod.rs:82-140`). Porting dead fields plus their
rejection tests buys no capability.

**Decision: omit them.** Both ports document that native requests carry neither thinking nor MCP, and
that provider-specific reasoning controls go through `provider_options` — which is what the Rust error
message already tells callers. A caller who reaches for the field gets an attribute/type error instead
of a runtime `UnsupportedFeature`; equally clear, one less surface.

### D4 — Python gains `UnsupportedFeatureError(InvalidRequestError)`

Rust raises `MotosanError::UnsupportedFeature` and TypeScript raises `UnsupportedFeatureError`, but
Python has no such class — `provider_base.validate_request` raises `InvalidRequestError`.

**Decision: add `UnsupportedFeatureError` as a subclass of `InvalidRequestError`.** Existing
`except InvalidRequestError` handlers keep working while callers that need to distinguish can match the
subclass — the same softener M3 used for `IncompleteStreamError(StreamError)`. It stays non-retryable by
inheritance. Add to `error.py` and to `__init__.py`'s `__all__`.

### D5 — Capabilities grow by one field, and `full()` stays freeform-false

Add `supports_freeform_tools` / `supportsFreeformTools`, with `with_freeform_tools()` and
`with_image_and_freeform_tools()` constructors. Rust's `full()` deliberately leaves freeform **false**;
both ports must match, or a provider silently claims support it lacks.

TypeScript ends up with four fields because it keeps its TS-only `supportsMcp`. That is fine and
intentional — but it means the ported constructors are not 1:1 copies of Rust's.

**Flip list:** both SDKs assert exact capability object shapes (`tests/capabilities.test.ts`,
`tests/test_provider_capabilities.py`). Those assertions change; they are expected flips, not failures.

Provider declarations: ChatGPT Codex = freeform yes / image no / document no. OpenAI = freeform only when
the Responses opt-in is on. Everything else unchanged.

### D6 — Dispatch plumbing differs per SDK, and both have a trap

- **Python:** `BaseProvider` is subclassed by only 4 of 11 providers — `OpenAIProvider`, `Minimax`,
  `Ollama`, and the three CLI clients are structurally-typed classes. A default `model_chat` on the ABC
  therefore never reaches `OpenAIProvider`. `Client` must duck-type, exactly as the capability
  enforcement shipped in 0.19.0 does (`client.py:475`).
- **TypeScript:** `ProviderImpl` is a structural contract third parties implement, so `modelChat` /
  `modelStream` must be **optional**. More subtly, `asDispatchProvider` (`client.ts:70-80`) rebuilds a
  plain object exposing only `capabilities`/`chat`/`stream` — model methods are **silently dropped**
  unless that shim is updated. This is the single easiest way to ship a broken port.

### D7 — Method names follow each SDK's existing trio

| | Existing | Native |
|---|---|---|
| Python `Client` | `chat_with` / `stream_with` / `stream_collect_with` | `model_chat_with` / `model_stream_with` / `model_stream_collect_with` |
| TypeScript `Client` | `chat` / `stream` / `streamCollect` | `modelChat` / `modelStream` / `modelStreamCollect` |
| Provider level | `chat` / `stream` | `model_chat` / `model_stream` · `modelChat` / `modelStream` |

TypeScript has no `chatWith` — its `chat` already takes a full request — so the native methods take no
`With` suffix either. `modelStreamCollect` absorbs the model-backfill behaviour that
`streamCollectWith` has today.

### D8 — Stream semantics are contract, not implementation detail

Both ports must reproduce, and the conformance suite must pin:

- Exactly one terminal `Done` per successfully completed stream.
- EOF without a terminal ⇒ `IncompleteStream` with the payload
  `"<provider> ended without a terminal event"`, provider strings exactly `openai` and `chatgpt-codex`.
- `ToolCallDone` is **authoritative**: accumulated `FunctionArguments` / `FreeformInput` deltas are
  bookkeeping and must never be lowered into the returned call.
- Freeform `input` survives byte-for-byte: never parsed as JSON, never lowered into `arguments`.
- `Usage` **replaces** rather than merges.
- `ThinkingDone` wins over accumulated thinking deltas (mirrors the existing `collect_stream` rule).
- Pending deltas drain before a stored stream error surfaces.
- A read-idle timeout wraps the native stream on HTTP providers, mirroring Rust's
  `ReadTimeoutModelStream`.

### D9 — One spec-anchored conformance suite per SDK, Rust included

M2 shipped `*retry-conformance*` and M3 shipped `*m3-stream-conformance*` in all three SDKs, anchored to
`specs/`. This milestone adds a third, anchored to `specs/types.md` § Native Model API, covering D8 plus
ordered history replay and pre-network rejection. Rust gets the file too even though the behaviour is
already implemented — a cross-SDK gate that skips one SDK is not a gate.

Rust's 30 existing native tests are the expected-value source; do not invent new fixtures where one
exists.

### D10 — Versions and spec relabel

Python **0.20.0**, TypeScript **0.16.0** (additive minors). Rust needs no version change — it gains only
a conformance test file.

**The spec changes in two steps, deliberately.** PR S widens the *normative contract* — every "MUST" in
§ Native Model API becomes cross-SDK, and D3's omission and D4's Python error type are written down —
because the implementations are written against it. But S must **not** claim the API ships in Python and
TypeScript, which would make the spec lie for the duration of the milestone. So S replaces the
"(Rust, v0.26.0+)" heading label with an explicit implementation-status line:

> Implemented in Rust 0.26.0+. Python and TypeScript ports in progress — see #270.

REL rewrites that line to the shipped versions and widens the Provider-support paragraph and the
stream-termination table row. Anything that asserts *which SDKs ship it* belongs to REL; anything that
asserts *what the API must do* belongs to S.

---

## Behaviour that must be copied exactly

Both surveys flagged these as the places a plausible-looking port goes silently wrong.

**Wire keys that differ from field names.** `ModelToolCall.id` ↔ wire `call_id`; `max_tokens` ↔ body
`max_output_tokens`; `Tool.input_schema` ↔ `parameters`. Deserialization accepts `call_id` **or** `id`.

**`build_model_request_body` has two non-obvious rules.** System messages inside `context` are hoisted
into `instructions` **and removed from `input`**; and `provider_options` is shallow-merged **last**, so
it can override anything the encoder produced.

**The two `model_chat` shapes are different.** ChatGPT Codex implements `model_chat` as
`model_stream` + collect (it has no non-streaming endpoint). OpenAI does a genuine non-streaming POST and
decodes via `model_chat_response_from_output`. Both need porting; TypeScript's `extractResponsesText` is
only a partial stand-in for the latter.

**ChatGPT Codex's body overrides the caller.** It hard-sets `store=false`,
`include=["reasoning.encrypted_content"]`, `parallel_tool_calls=true`, and **`tool_choice="auto"`
regardless of what the caller passed**. Reproduce it or document the divergence deliberately.

**Codex also normalizes reasoning effort, and this is easy to miss.** Effort resolves as per-request
`provider_options["reasoning_effort"]` **first**, provider default second, omitted if neither. When one
resolves, the body gets `reasoning = {"effort": <value>, "summary": "auto"}` — and any top-level
`reasoning_effort` key is **removed**, because the `provider_options` shallow merge described just
above will have injected the raw key onto the body. Pin both halves: that per-request effort beats the provider
default, and that `reasoning_effort` never reaches the wire.

**The existing SSE adapters are missing frames.** Neither Python's `_parse_sse_event` nor TypeScript's
`streamImpl` handles `response.custom_tool_call_input.delta`, `custom_tool_call` output items,
`response.reasoning_text.done` / `response.reasoning_summary_text.done`, or `response.incomplete`. The
legacy adapters stay function-tool-only — these live only in the new codec.

**`call_id` resolution order** in stream events: event `call_id` → `item_id` looked up in the
item→call map → raw `item_id` as a last resort.

---

## Reusable machinery

Neither port starts from zero.

- **Python** `providers/chatgpt_codex.py` already has the Responses SSE dispatcher (`_parse_sse_event`,
  ~85% of what the model adapter needs), the adapter state it needs, instructions assembly in
  `_build_responses_body`, and the streaming transport loop with `IncompleteStreamError` and
  `ReadTimeout` mapping. There is no shared SSE module in Python — every provider inlines the loop.
- **TypeScript** `http/sse.ts` is a provider-agnostic SSE parser (CRLF handling, `[DONE]`, malformed-JSON
  skip) that is reusable as-is. `providers/chatgpt_codex.ts` has the same frame switch plus the
  item→call map, and `buildResponsesBody` is already public as a test seam — the new
  `buildModelRequestBody` should follow that convention.
- **TypeScript builds the Responses body twice today** (`chatgpt_codex.ts:126-219` and
  `openai.ts:145-249`). The new codec is the natural place to consolidate the native path.

---

## Milestone breakdown

Python and TypeScript are independent; only the spec PR is ordered before them and only the conformance
and release PRs after.

| PR | Scope | Depends on |
|---|---|---|
| **S** | Widen `specs/types.md` § Native Model API to all three SDKs; state D3's omission and D4's Python error type | — |
| **P1** | Python types + `providers/responses.py` codec + `UnsupportedFeatureError`; no provider wiring. **Every new public symbol is added to `motosan_ai/__init__.py`'s explicit imports and `__all__`** | S |
| **P2** | Python `chatgpt_codex` / `openai` native methods, capabilities, `Client` dispatch, `collect_model_stream` — again exported from the package root | P1 |
| **T1** | TypeScript types + `serialize/responses.ts` codec | S |
| **T2** | TypeScript provider native methods, capabilities, `Client` + `ClientBuilder`, `asDispatchProvider` shim, `collectModelStream` | T1 |
| **C** | Freeform conformance suite ×3 SDKs | P2, T2 |
| **REL** | Python 0.20.0 / TypeScript 0.16.0 | C |

Ship P1→P2 and T1→T2 as two parallel tracks. Each PR is diff-verified against the plan before merge, per
house practice.

**Python's package root is not automatic.** `motosan_ai/__init__.py` re-exports through explicit imports
and a hand-maintained `__all__` (~54 entries today), so a type that is never listed is invisible to
callers no matter how correct it is — unlike TypeScript, where `export * from './types.js'` picks new
types up for free. P1 and P2 each carry that duty, and the milestone adds the export-surface test Python
lacks: TypeScript pins its exports in `tests/index.test.ts`, Python pins nothing. A
`tests/test_public_exports.py` that imports every native symbol from `motosan_ai` and asserts it is in
`__all__` closes that gap and is the cheapest possible guard against a half-exported port.

---

## Explicitly out of scope

- Porting `GeminiCodeAssist` or the CLI backends to TypeScript — a separate, older deferral.
- Native Freeform support on any provider beyond OpenAI-in-Responses-mode and ChatGPT Codex.
- Any change to the legacy `ChatRequest` / `Tool` / `ToolCall` / `ChatResponse` / `StreamEvent` APIs.
  `specs/types.md` pins them as function-tool-only; the native API is parallel, not a widening.
- Rust changes beyond the conformance test file.

---

## Next step

Draft the per-PR implementation plans, one per track, after D1 is confirmed or overridden. Rust's 30
native tests and this document's "copied exactly" section are the raw material — the plans' job is to
turn them into ordered, individually-testable tasks.

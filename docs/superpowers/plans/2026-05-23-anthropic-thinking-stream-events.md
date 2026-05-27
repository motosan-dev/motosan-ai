# Anthropic Thinking Stream Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Surface Anthropic's streaming extended-thinking content (`content_block_delta { type: thinking_delta }`) through the `motosan-ai` Rust SDK by adding `StreamEventType::ThinkingDelta` and `StreamEventType::ThinkingDone` variants and wiring them through the Anthropic SSE adapter, so downstream consumers (notably `motosan-agent-loop` v0.21.4) can render live reasoning instead of only seeing the final post-turn thinking block.

**Architecture:** Two new variants on `StreamEventType` plus matching constructors on `StreamEvent`. The `AnthropicStreamAdapter` gains a `current_thinking_buf: Option<String>` field that tracks an in-flight thinking block: `content_block_start { type: "thinking" }` opens it, `content_block_delta { type: "thinking_delta", thinking: "..." }` accumulates and emits `ThinkingDelta`, `content_block_stop` emits `ThinkingDone` with the full accumulated text and clears state. `signature_delta` and `redacted_thinking` are silently consumed (no streaming surface yet). The high-level `collect_stream` is taught to populate `ChatResponse.thinking` from accumulated thinking deltas so streaming and non-streaming responses match. No other provider changes — only the Anthropic backend currently has a wire format for streaming thinking.

**Tech Stack:** Rust 2021, `motosan-ai` crate (`sdks/rust/`), `tokio`, `eventsource-stream`, `mockito` (tests), `serde_json`.

## Context for the implementer

You are working in `~/Projects/wade/motosan-ai`. This is a multi-SDK workspace (Rust + Python under `sdks/`) for a multi-provider LLM client library, published as `motosan-ai` on crates.io. Current Rust version is `0.15.3`; you will bump to `0.15.4`. **Only the Rust SDK changes in this plan** — see Anti-scope.

The motivating consumer is `motosan-agent-loop` v0.21.4, which already ships `StreamChunk::ThinkingDelta`/`ThinkingDone` (loop-layer) and `CoreEvent::ThinkingChunk`/`ThinkingDone` (engine-layer) with TODO markers at `src/motosan_ai_impl.rs:171` and `:346` saying:

> when motosan-ai SDK adds `StreamEventType::ThinkingDelta` and `StreamEventType::ThinkingDone`, add arms here that yield `StreamChunk::ThinkingDelta(event.content)` and `StreamChunk::ThinkingDone(event.content)` respectively.

This plan ships the SDK side. **You do NOT touch motosan-agent-loop** — the consumer-side wiring happens in a separate follow-up plan after this is published.

**Anthropic wire format for streaming thinking** (reference, from the API docs at the time of writing):

```
event: content_block_start
data: {"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"Let me "}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"think..."}}

event: content_block_delta
data: {"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"abc..."}}

event: content_block_stop
data: {"type":"content_block_stop","index":0}

event: content_block_start
data: {"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}
...
```

Key wire-format facts:
- The thinking text lives in `delta.thinking`, **not** `delta.text`. The current code path at `providers/anthropic.rs:978-989` reads `delta.text` and so silently drops thinking deltas today.
- `signature_delta` arrives at the end of a thinking block; its `signature` field is a cryptographic verification token used for re-feeding thinking blocks back to the model in subsequent turns. The non-streaming `ChatResponse.thinking: Option<String>` field doesn't expose signatures, so we will not expose them in streaming either. Silently consume.
- `redacted_thinking` is an Anthropic safety mechanism: `content_block_start { content_block: { type: "redacted_thinking", data: "..." } }` arrives when policy redacts the model's reasoning. The `data` field is opaque encrypted bytes. We silently drop the entire block (no `ThinkingDelta`/`Done`). If anyone needs to round-trip these later, that's a separate plan.

**Read these files before starting:**
- `sdks/rust/src/types.rs` (lines 638-647 for `StreamEventType`; lines 697-815 for `StreamEvent` constructors)
- `sdks/rust/src/providers/anthropic.rs` (lines 820-1013 for `AnthropicStreamAdapter` — the struct, the `poll_next` impl, and the `match event_type` block)
- `sdks/rust/src/stream.rs` (lines 30-113 for `collect_stream` — the exhaustive `match event.event_type` you must update)
- `sdks/rust/tests/anthropic_stream.rs` (the mockito-based SSE test pattern — your new tests follow this shape)
- `sdks/rust/tests/anthropic_live.rs` (lines 1-100 — the live-API test pattern: `client()`/`api_key()`/`cooldown()` helpers, no `#[ignore]`, skip-on-missing-key idiom)
- `sdks/rust/CHANGELOG.md` (style/voice for the entry — see `[0.15.3]` and `[0.15.2]` entries)
- `CLAUDE.md` (release checklist at the bottom)

**Convention notes:**
- All Anthropic tests live in `sdks/rust/tests/anthropic_stream.rs` and use `mockito::Server` with explicit SSE bodies. Follow this pattern.
- All `StreamEvent` constructors follow `StreamEvent::<snake_case>(...)` returning `Self`. Adding two new ones (`thinking_delta`, `thinking_done`) follows that.
- `cargo` commands run from the workspace root (`~/Projects/wade/motosan-ai`), not from `sdks/rust/`. Use `cargo test -p motosan-ai --features anthropic` etc.
- `check-rust` (a `nix develop` shell command, also runnable as `cd sdks/rust && cargo fmt --all -- --check && cargo clippy --all-features --all-targets -- -D warnings && cargo test --all-features`) is the local CI gate.

**Version policy:** `StreamEventType` is not `#[non_exhaustive]`, so adding variants is technically wire-breaking for any downstream that does an exhaustive `match event.event_type { ... }` without `_ =>`. The internal `collect_stream` and `codex_cli` integration test are the only such sites — both will be updated here. Pre-1.0 we ship as **patch** (`0.15.3 → 0.15.4`). This mirrors the `motosan-agent-loop` 0.21.3 → 0.21.4 precedent.

**Forward-compatibility doc contract:** Task 1's variant docstrings explicitly tell consumers to include a `_ =>` arm when matching on `StreamEventType`, anticipating future thinking-related additions (e.g. `ThinkingSignature` for Anthropic re-feed signatures, `ThinkingStart` for block boundaries, or reasoning-summary variants for OpenAI o-series). This is the **only** mitigation we ship for the non-`#[non_exhaustive]` constraint — we are not retrofitting `#[non_exhaustive]` (anti-scope #3). Authors of follow-up plans adding new variants should still treat them as wire-breaking under strict semver but can lean on this documented contract when arguing for a patch bump.

---

## File Structure

**Modified files** (no new files in `src/`; one new test fixture file is acceptable but the plan keeps everything in existing test files):

- `sdks/rust/src/types.rs` — add 2 `StreamEventType` variants + 2 `StreamEvent` constructors + unit tests
- `sdks/rust/src/providers/anthropic.rs` — add `current_thinking_buf` field to `AnthropicStreamAdapter`; handle thinking in `content_block_start`, `content_block_delta`, `content_block_stop`; silently consume `signature_delta` and `redacted_thinking`
- `sdks/rust/src/stream.rs` — extend the exhaustive match in `collect_stream` to handle the new variants and populate `ChatResponse.thinking`
- `sdks/rust/src/providers/codex_cli/mod.rs` — verify the `_ => {}` arm at `:625` still covers (no change expected, but verify in Task 6)
- `sdks/rust/tests/anthropic_stream.rs` — append new mockito tests for the SSE → events mapping
- `sdks/rust/tests/anthropic_live.rs` — append one live test (no `#[ignore]`, skip-on-missing-key per repo convention) that hits real Anthropic API with `thinking(...)` enabled
- `sdks/rust/Cargo.toml` — version 0.15.3 → 0.15.4
- `sdks/rust/CHANGELOG.md` — new `[0.15.4]` entry
- `AGENTS.md` — version header + Recent Additions entry
- `llms.txt` — version header + `StreamEventType` table row + relevant section update
- `skills/motosan-ai/SKILL.md` — version header (line ~8)

---

### Task 1: Add `StreamEventType::ThinkingDelta`/`ThinkingDone` variants and constructors

**Files:**
- Modify: `sdks/rust/src/types.rs` (the `StreamEventType` enum at lines 638-647; the `StreamEvent` impl at lines 697-815)

- [x] **Step 1: Write the failing test**

Add this test at the bottom of `sdks/rust/src/types.rs`, inside (or appending to if absent) a `#[cfg(test)] mod tests` block. If the module does not exist, create it at the very end of the file:

```rust
#[cfg(test)]
mod stream_event_thinking_tests {
    use super::*;

    #[test]
    fn stream_event_type_has_thinking_variants() {
        // Compile-time exhaustive guard: any addition/removal will require updating.
        let _all: [StreamEventType; 7] = [
            StreamEventType::Text,
            StreamEventType::ToolCallStart,
            StreamEventType::ToolCallArgs,
            StreamEventType::ToolCallEnd,
            StreamEventType::Usage,
            StreamEventType::ThinkingDelta,
            StreamEventType::ThinkingDone,
        ];
    }

    #[test]
    fn stream_event_thinking_delta_constructor_sets_fields() {
        let ev = StreamEvent::thinking_delta("Let me think...");
        assert_eq!(ev.content, "Let me think...");
        assert_eq!(ev.event_type, StreamEventType::ThinkingDelta);
        assert!(!ev.done);
        assert!(ev.tool_call_id.is_none());
        assert!(ev.usage.is_none());
        assert!(ev.stop_reason.is_none());
    }

    #[test]
    fn stream_event_thinking_done_constructor_sets_fields() {
        let ev = StreamEvent::thinking_done("complete thought");
        assert_eq!(ev.content, "complete thought");
        assert_eq!(ev.event_type, StreamEventType::ThinkingDone);
        assert!(!ev.done);
        assert!(ev.tool_call_id.is_none());
        assert!(ev.usage.is_none());
        assert!(ev.stop_reason.is_none());
    }

    #[test]
    fn stream_event_type_thinking_delta_serializes_snake_case() {
        let s = serde_json::to_string(&StreamEventType::ThinkingDelta).unwrap();
        assert_eq!(s, "\"thinking_delta\"");
        let d: StreamEventType = serde_json::from_str("\"thinking_delta\"").unwrap();
        assert_eq!(d, StreamEventType::ThinkingDelta);
    }

    #[test]
    fn stream_event_type_thinking_done_serializes_snake_case() {
        let s = serde_json::to_string(&StreamEventType::ThinkingDone).unwrap();
        assert_eq!(s, "\"thinking_done\"");
        let d: StreamEventType = serde_json::from_str("\"thinking_done\"").unwrap();
        assert_eq!(d, StreamEventType::ThinkingDone);
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:
```bash
cd sdks/rust && cargo test --lib -p motosan-ai stream_event_thinking_tests
```
Expected: compile error — `no variant or associated item named 'ThinkingDelta' found for enum 'StreamEventType'` and `no method named 'thinking_delta' found for struct 'StreamEvent'`.

- [x] **Step 3: Add the variants to `StreamEventType`**

In `sdks/rust/src/types.rs` modify the enum at lines 638-647. Insert the two new variants **after** `Usage` so existing serde-encoded values keep the same ordinal (irrelevant for snake_case serde but a good habit):

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StreamEventType {
    #[default]
    Text,
    ToolCallStart,
    ToolCallArgs,
    ToolCallEnd,
    Usage,
    /// A partial extended-thinking delta from the LLM, emitted as the
    /// model reasons before producing its final answer. The `content`
    /// field of the parent [`StreamEvent`] carries the delta text.
    /// Currently only the Anthropic provider emits this (sourced from
    /// the SSE `content_block_delta { type: "thinking_delta" }` event).
    /// Other providers never emit it. Consumers can render these live;
    /// the high-level [`collect_stream`](crate::stream::collect_stream)
    /// concatenates them into [`ChatResponse::thinking`].
    ///
    /// # Forward compatibility
    ///
    /// `StreamEventType` is intentionally not `#[non_exhaustive]` so
    /// callers can rely on exhaustive matching for the current variants,
    /// but the set may grow as more providers gain streaming-thinking
    /// wire formats (e.g. signature/re-feed metadata, structured block
    /// boundaries, per-block effort hints). New thinking-related variants
    /// will be additive (`ThinkingSignature`, `ThinkingStart`, etc.) —
    /// never repurposing `ThinkingDelta`/`ThinkingDone`. **Consumers that
    /// match on `StreamEventType` should always include a `_ =>` arm**
    /// so future patch releases adding new variants do not break their
    /// build. The same rule applies to [`ThinkingDone`](Self::ThinkingDone).
    ThinkingDelta,
    /// Marks the end of a thinking content block, carrying the full
    /// concatenated thinking text in the parent [`StreamEvent`]'s
    /// `content` field. Always preceded by zero or more
    /// [`ThinkingDelta`](Self::ThinkingDelta) events for the same block,
    /// and always precedes any [`Text`](Self::Text) events for the
    /// final answer. Sourced from Anthropic's `content_block_stop`
    /// event when the corresponding `content_block_start` was a
    /// `thinking` block.
    ///
    /// See [`ThinkingDelta`](Self::ThinkingDelta) for the forward-
    /// compatibility contract (include `_ =>` when matching).
    ThinkingDone,
}
```

- [x] **Step 4: Add the constructors to `StreamEvent`**

In `sdks/rust/src/types.rs`, append two methods to the `impl StreamEvent { ... }` block (currently ends around line 815, after `tool_call_end_with_id`):

```rust
    /// Build a `ThinkingDelta` event carrying a partial extended-thinking
    /// text fragment. Used by the Anthropic stream adapter when it
    /// receives a `content_block_delta { type: "thinking_delta" }` SSE
    /// event. See [`StreamEventType::ThinkingDelta`].
    pub fn thinking_delta(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::ThinkingDelta,
            usage: None,
            stop_reason: None,
        }
    }

    /// Build a `ThinkingDone` event carrying the full concatenated
    /// thinking text for a just-closed thinking block. Used by the
    /// Anthropic stream adapter on `content_block_stop` when the
    /// corresponding `content_block_start` opened a `thinking` block.
    /// See [`StreamEventType::ThinkingDone`].
    pub fn thinking_done(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            done: false,
            tool_call_id: None,
            tool_call_name: None,
            tool_call_args_delta: None,
            event_type: StreamEventType::ThinkingDone,
            usage: None,
            stop_reason: None,
        }
    }
```

- [x] **Step 5: Run tests to verify they pass**

Run:
```bash
cd sdks/rust && cargo test --lib -p motosan-ai stream_event_thinking_tests
```
Expected: all 4 tests pass.

- [x] **Step 6: Verify the rest of the crate still compiles (it will NOT — that's expected)**

Run:
```bash
cd sdks/rust && cargo build --all-features 2>&1 | head -30
```
Expected: **compile error** on `stream.rs` and `providers/codex_cli/mod.rs` test, both of which have exhaustive `match event.event_type { ... }` arms without `_ =>`. Specifically, you should see something like:

```
error[E0004]: non-exhaustive patterns: `StreamEventType::ThinkingDelta` and `StreamEventType::ThinkingDone` not covered
  --> src/stream.rs:55:15
```

**Do not work around this here.** The real arms are added in Task 6 (`collect_stream`); for now, add a placeholder so this task can commit independently. In `sdks/rust/src/stream.rs` around line 91 (just before the closing brace of the `match event.event_type { ... }` block), append:

```rust
            StreamEventType::ThinkingDelta | StreamEventType::ThinkingDone => {
                // Placeholder; real handling lands in Task 6 of the
                // anthropic-thinking-stream-events plan. Do not commit
                // this past Task 6.
            }
```

Also in `sdks/rust/src/providers/codex_cli/mod.rs` at line 625, the existing `_ => {}` arm already covers (verified at plan-write time). **No change needed there**; just confirm by reading the test code. If for some reason a non-wildcard arm is in place, add the same `ThinkingDelta | ThinkingDone => {}` arm.

Re-run:
```bash
cd sdks/rust && cargo build --all-features
```
Expected: clean.

- [x] **Step 7: Commit**

```bash
cd sdks/rust
git add src/types.rs src/stream.rs
git commit -m "feat(types): add StreamEventType::ThinkingDelta/Done variants

Two new variants on the streaming event surface so the Anthropic
provider can forward content_block_delta { type: thinking_delta }
events to consumers. Adds matching StreamEvent::thinking_delta and
StreamEvent::thinking_done constructors. collect_stream gets a
placeholder arm; real accumulation comes in Task 6.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 2: Anthropic SSE adapter — track in-flight thinking blocks via `current_thinking_buf`

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` (the `AnthropicStreamAdapter` struct at lines 820-838; the `content_block_start` arm at lines 938-955)

This task adds the state field and teaches `content_block_start` to recognize `thinking` blocks. It does not yet emit any events — that comes in Task 3 (deltas) and Task 4 (close). One small test asserts the state initializes correctly without changing observable output yet.

- [x] **Step 1: Write the failing test**

Append to `sdks/rust/tests/anthropic_stream.rs`. Find the file's end and add:

```rust
#[tokio::test]
async fn thinking_block_start_then_immediate_stop_emits_nothing_yet() {
    // Pre-Task-3/4 state check: opening and immediately closing a thinking
    // block with no deltas must not crash and must not leak any spurious
    // events. After Task 4 the closing block will emit ThinkingDone(""),
    // and this test will be updated then.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"ok\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut text_seen = String::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::Text {
            text_seen.push_str(&ev.content);
        }
    }

    assert!(done_seen, "must terminate");
    assert_eq!(text_seen, "ok", "text after the thinking block must survive");
    mock.assert_async().await;
}
```

- [x] **Step 2: Run test to verify it currently passes (sanity baseline)**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic thinking_block_start_then_immediate_stop_emits_nothing_yet
```
Expected: **PASS already** — because today the adapter silently ignores unknown content block types and `content_block_stop` only does anything if `current_tool_id` is set. This baseline confirms our test fixture is well-formed; we will deliberately tighten it in Tasks 3 and 4.

If this test fails today, the SSE fixture is malformed or the adapter regressed independently. Stop and investigate.

- [x] **Step 3: Add the `current_thinking_buf` field**

In `sdks/rust/src/providers/anthropic.rs`, modify the `AnthropicStreamAdapter` struct (lines 820-838):

```rust
/// Stream adapter that parses Anthropic SSE events including tool_use blocks.
struct AnthropicStreamAdapter {
    inner: Pin<
        Box<
            dyn Stream<
                    Item = Result<
                        eventsource_stream::Event,
                        eventsource_stream::EventStreamError<reqwest::Error>,
                    >,
                > + Send,
        >,
    >,
    pending: std::collections::VecDeque<StreamEvent>,
    current_tool_id: Option<String>,
    /// Captured from `message_delta.delta.stop_reason`; emitted on the
    /// terminal `message_stop` event so callers see the reason in the
    /// final `done` `StreamEvent`.
    current_stop_reason: Option<crate::types::StopReason>,
    /// Accumulator for the in-flight thinking block, if any.
    ///
    /// - `None` = not currently inside a `thinking` content block.
    /// - `Some(buf)` = open thinking block; each `thinking_delta` appends
    ///   to `buf`, and `content_block_stop` emits a `ThinkingDone` event
    ///   carrying `buf.clone()` and resets to `None`.
    ///
    /// `redacted_thinking` blocks are silently consumed and do **not**
    /// open this accumulator (we don't surface redacted content as
    /// thinking deltas).
    current_thinking_buf: Option<String>,
}
```

- [x] **Step 4: Initialize the field at every construction site**

Search for places the struct is constructed:

```bash
cd sdks/rust && rg -n 'AnthropicStreamAdapter\s*\{' src/
```

There is exactly one (around `providers/anthropic.rs:809`, inside the `stream` method of `impl ProviderImpl for AnthropicProvider`). The current construction looks like:

```rust
        let adapter = AnthropicStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            current_tool_id: None,
            current_stop_reason: None,
        };
```

Add the new field:

```rust
        let adapter = AnthropicStreamAdapter {
            inner: Box::pin(raw_stream),
            pending: std::collections::VecDeque::new(),
            current_tool_id: None,
            current_stop_reason: None,
            current_thinking_buf: None,
        };
```

If the compiler reports additional construction sites, add the field there too — but at plan-write time the only site is the one above.

- [x] **Step 5: Recognize `thinking` blocks in `content_block_start`**

In `sdks/rust/src/providers/anthropic.rs`, modify the `"content_block_start"` arm (lines 938-955). The current code only handles `tool_use`; extend it to also flag `thinking`:

```rust
                        "content_block_start" => {
                            let block = payload.get("content_block");
                            if let Some(block) = block {
                                let block_type =
                                    block.get("type").and_then(Value::as_str).unwrap_or("");
                                match block_type {
                                    "tool_use" => {
                                        let id = block
                                            .get("id")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        let name = block
                                            .get("name")
                                            .and_then(Value::as_str)
                                            .unwrap_or_default();
                                        self.current_tool_id = Some(id.to_string());
                                        return Poll::Ready(Some(
                                            StreamEvent::tool_call_start(id, name),
                                        ));
                                    }
                                    "thinking" => {
                                        // Open the thinking accumulator. Deltas append
                                        // to it; content_block_stop emits ThinkingDone
                                        // with the full text and clears it (Task 4).
                                        // No event is emitted at start — the loop-side
                                        // event protocol does not have a ThinkingStart.
                                        self.current_thinking_buf = Some(String::new());
                                    }
                                    "redacted_thinking" => {
                                        // Silently consume; we do not surface redacted
                                        // content. The block_stop will be a no-op
                                        // because current_thinking_buf stays None.
                                    }
                                    _ => {}
                                }
                            }
                            continue;
                        }
```

- [x] **Step 6: Run the Task-2 test (still passes) plus the broader anthropic_stream suite**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic anthropic_stream
```
Expected: ALL PASS, including the new `thinking_block_start_then_immediate_stop_emits_nothing_yet`. No regressions.

- [x] **Step 7: Commit**

```bash
cd sdks/rust
git add src/providers/anthropic.rs tests/anthropic_stream.rs
git commit -m "feat(anthropic): track current_thinking_buf in stream adapter

Adds a current_thinking_buf: Option<String> accumulator to the
Anthropic SSE adapter and teaches content_block_start to open it
when a thinking block begins. Recognizes redacted_thinking as a
distinct (silently-consumed) case. No new StreamEvents emitted
yet; deltas and close handling come in Tasks 3-4.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 3: Emit `ThinkingDelta` on `content_block_delta { type: "thinking_delta" }`

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` (the `content_block_delta` arm at lines 956-990)
- Test: `sdks/rust/tests/anthropic_stream.rs`

- [x] **Step 1: Write the failing test**

Append to `sdks/rust/tests/anthropic_stream.rs`:

```rust
#[tokio::test]
async fn thinking_delta_events_emitted_in_order() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"think...\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut thinking_chunks: Vec<String> = Vec::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::ThinkingDelta {
            thinking_chunks.push(ev.content);
        }
    }

    assert!(done_seen, "stream must terminate");
    assert_eq!(
        thinking_chunks,
        vec!["Let me ".to_string(), "think...".to_string()],
        "ThinkingDelta events must arrive in order with exact content"
    );
    mock.assert_async().await;
}
```

- [x] **Step 2: Run test to verify it fails**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic thinking_delta_events_emitted_in_order
```
Expected: FAIL — `thinking_chunks` is empty because the current delta-type fall-through reads `delta.text` (not `delta.thinking`) and so finds nothing.

- [x] **Step 3: Handle `thinking_delta` in `content_block_delta`**

In `sdks/rust/src/providers/anthropic.rs`, modify the `"content_block_delta"` arm (lines 956-990). Insert a new match arm for `"thinking_delta"` **before** the fall-through default that reads `delta.text`:

```rust
                        "content_block_delta" => {
                            let delta = match payload.get("delta") {
                                Some(d) => d,
                                None => continue,
                            };
                            let delta_type = delta.get("type").and_then(Value::as_str);

                            match delta_type {
                                Some("input_json_delta") => {
                                    let partial = delta
                                        .get("partial_json")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !partial.is_empty() {
                                        let id =
                                            self.current_tool_id.as_deref().unwrap_or_default();
                                        return Poll::Ready(Some(
                                            StreamEvent::tool_call_args_with_id(id, partial),
                                        ));
                                    }
                                    continue;
                                }
                                Some("thinking_delta") => {
                                    // The thinking text lives in `delta.thinking`,
                                    // NOT `delta.text`. Accumulate into the buffer
                                    // (so content_block_stop can emit ThinkingDone
                                    // with the full text in Task 4) and forward as
                                    // a ThinkingDelta event.
                                    let text = delta
                                        .get("thinking")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if text.is_empty() {
                                        continue;
                                    }
                                    if let Some(buf) = self.current_thinking_buf.as_mut() {
                                        buf.push_str(text);
                                    }
                                    return Poll::Ready(Some(StreamEvent::thinking_delta(text)));
                                }
                                Some("signature_delta") => {
                                    // Cryptographic signature for re-feeding thinking
                                    // blocks. Not surfaced in the streaming API (the
                                    // non-streaming ChatResponse.thinking field is
                                    // also signature-less). Silently consume.
                                    continue;
                                }
                                _ => {
                                    // text_delta or untyped delta with "text" field
                                    let text = delta
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .unwrap_or_default();
                                    if !text.is_empty() {
                                        return Poll::Ready(Some(StreamEvent::text(text)));
                                    }
                                    continue;
                                }
                            }
                        }
```

Two important behaviors baked in:
1. **Defensive `if let Some(buf) = self.current_thinking_buf.as_mut()`** — if a malformed stream sends a `thinking_delta` without a preceding `content_block_start { type: "thinking" }`, we still emit the event but skip the accumulator update. We do not crash.
2. **Empty deltas are dropped** (`if text.is_empty() { continue; }`). This matches the existing `text_delta` behavior on the same line.

- [x] **Step 4: Run test to verify it passes**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic thinking_delta_events_emitted_in_order
```
Expected: PASS.

- [x] **Step 5: Run the broader anthropic_stream suite for regressions**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic anthropic_stream
```
Expected: ALL PASS. In particular the existing `anthropic_stream_emits_content_and_done_event`, `anthropic_stream_emits_tool_use_events`, and `anthropic_stream_propagates_*_stop_reason` tests must continue to pass — `text_delta` and `input_json_delta` paths are untouched.

- [x] **Step 6: Commit**

```bash
cd sdks/rust
git add src/providers/anthropic.rs tests/anthropic_stream.rs
git commit -m "feat(anthropic): emit StreamEvent::thinking_delta on thinking_delta SSE

content_block_delta { type: thinking_delta } now produces a
ThinkingDelta StreamEvent carrying the delta.thinking text (NOT
delta.text — a previous bug-by-omission silently dropped these).
Also silently consumes signature_delta (no streaming surface for
cryptographic re-feed signatures, matching the non-streaming
ChatResponse.thinking field's behavior).

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 4: Emit `ThinkingDone` on `content_block_stop` for thinking blocks

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs` (the `content_block_stop` arm at lines 991-996)
- Test: `sdks/rust/tests/anthropic_stream.rs`

- [x] **Step 1: Write the failing test**

Append to `sdks/rust/tests/anthropic_stream.rs`:

```rust
#[tokio::test]
async fn thinking_done_emitted_with_full_text_after_deltas() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"A \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"B \"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"C\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"signature_delta\",\"signature\":\"sig...\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"answer\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut labels: Vec<String> = Vec::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        match ev.event_type {
            StreamEventType::ThinkingDelta => labels.push(format!("td:{}", ev.content)),
            StreamEventType::ThinkingDone => labels.push(format!("tD:{}", ev.content)),
            StreamEventType::Text => labels.push(format!("t:{}", ev.content)),
            _ => {}
        }
    }

    assert!(done_seen, "stream must terminate");
    assert_eq!(
        labels,
        vec![
            "td:A ".to_string(),
            "td:B ".to_string(),
            "td:C".to_string(),
            "tD:A B C".to_string(),
            "t:answer".to_string(),
        ],
        "Per-turn order: ThinkingDelta* -> ThinkingDone(full) -> Text*"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn thinking_done_not_emitted_for_non_thinking_block_stop() {
    // Regression guard: a content_block_stop that closes a tool_use or
    // text block must NOT emit a stray ThinkingDone. Only triggers when
    // current_thinking_buf was Some.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"text\":\"hi\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut saw_thinking_done = false;
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        if ev.event_type == StreamEventType::ThinkingDone {
            saw_thinking_done = true;
        }
    }

    assert!(done_seen);
    assert!(!saw_thinking_done, "no thinking block was opened; no ThinkingDone expected");
    mock.assert_async().await;
}
```

- [x] **Step 2: Run the new tests to verify they fail**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic thinking_done_emitted_with_full_text_after_deltas thinking_done_not_emitted_for_non_thinking_block_stop
```
Expected: the first test FAILS (no `tD:` label appears — `content_block_stop` doesn't yet check `current_thinking_buf`). The second test should ALREADY PASS as a baseline (no thinking block opened = `current_thinking_buf` stays `None` = nothing to emit).

- [x] **Step 3: Update `content_block_stop` to emit `ThinkingDone`**

In `sdks/rust/src/providers/anthropic.rs`, modify the `"content_block_stop"` arm (lines 991-996). It currently looks like:

```rust
                        "content_block_stop" => {
                            if let Some(id) = self.current_tool_id.take() {
                                return Poll::Ready(Some(StreamEvent::tool_call_end_with_id(id)));
                            }
                            continue;
                        }
```

Replace with:

```rust
                        "content_block_stop" => {
                            if let Some(id) = self.current_tool_id.take() {
                                return Poll::Ready(Some(StreamEvent::tool_call_end_with_id(id)));
                            }
                            if let Some(buf) = self.current_thinking_buf.take() {
                                // Closing a thinking block: emit ThinkingDone with
                                // the full concatenated text. Note we emit even if
                                // buf is empty — consumers can distinguish "thinking
                                // block existed but produced nothing" from "no
                                // thinking block" by the presence/absence of the
                                // event. This matches the contract documented on
                                // StreamEventType::ThinkingDone.
                                return Poll::Ready(Some(StreamEvent::thinking_done(buf)));
                            }
                            continue;
                        }
```

Order matters: check `current_tool_id` first because the same SSE event closes either kind of block, and only one of the two state slots is `Some` at a time. Closing a text block leaves both `None` and falls through to `continue`.

- [x] **Step 4: Run the Task-4 tests to verify they pass**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic thinking_done_emitted_with_full_text_after_deltas thinking_done_not_emitted_for_non_thinking_block_stop
```
Expected: both PASS.

- [x] **Step 5: Run the full anthropic_stream suite for regressions**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic anthropic_stream
```
Expected: ALL PASS.

- [x] **Step 6: Commit**

```bash
cd sdks/rust
git add src/providers/anthropic.rs tests/anthropic_stream.rs
git commit -m "feat(anthropic): emit ThinkingDone on content_block_stop for thinking blocks

content_block_stop now emits StreamEvent::thinking_done with the
full accumulated thinking text when the just-closed block was a
thinking block. tool_use blocks still take priority (same SSE
event closes either). text blocks leave both state slots None and
fall through. Two new tests lock in the ordering contract and
guard against stray ThinkingDone emission for non-thinking
content_block_stops.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 5: Verify `redacted_thinking` and edge cases are handled silently

**Files:**
- Test only: `sdks/rust/tests/anthropic_stream.rs`

This task adds defensive coverage for the `redacted_thinking` case (already handled silently in Task 2) and asserts that a malformed stream with `thinking_delta` outside a `content_block_start` doesn't crash. **No source changes expected** — if a test fails, there's a bug in Task 2's `content_block_start` arm or Task 3's defensive `if let Some(buf)`.

- [x] **Step 1: Write the tests**

Append to `sdks/rust/tests/anthropic_stream.rs`:

```rust
#[tokio::test]
async fn redacted_thinking_block_is_silently_consumed() {
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"redacted_thinking\",\"data\":\"encrypted_blob_xyz\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"text\":\"answer\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut events: Vec<StreamEventType> = Vec::new();
    let mut content = String::new();
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        events.push(ev.event_type.clone());
        if ev.event_type == StreamEventType::Text {
            content.push_str(&ev.content);
        }
    }

    assert!(done_seen);
    assert_eq!(content, "answer");
    assert!(
        !events.contains(&StreamEventType::ThinkingDelta),
        "redacted_thinking must not produce ThinkingDelta events"
    );
    assert!(
        !events.contains(&StreamEventType::ThinkingDone),
        "redacted_thinking must not produce a ThinkingDone event"
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn orphan_thinking_delta_without_start_does_not_crash() {
    // Malformed/unexpected stream: a thinking_delta arrives without a
    // preceding content_block_start. Defensive Task-3 code emits the
    // event without crashing or polluting the accumulator.
    let mut server = mockito::Server::new_async().await;
    let sse_body = concat!(
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"orphan\"}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let mock = server
        .mock("POST", "/v1/messages")
        .match_header("x-api-key", "test-key")
        .with_status(200)
        .with_header("content-type", "text/event-stream")
        .with_body(sse_body)
        .create_async()
        .await;

    let provider = AnthropicProvider::new("test-key", None, Some(server.url()));
    let request = ChatRequest::builder().message(Message::user("hi")).build();
    let mut stream = provider.stream(request).await.expect("stream");

    let mut saw_thinking_delta = false;
    let mut saw_thinking_done = false;
    let mut done_seen = false;
    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        match ev.event_type {
            StreamEventType::ThinkingDelta => {
                saw_thinking_delta = true;
                assert_eq!(ev.content, "orphan");
            }
            StreamEventType::ThinkingDone => saw_thinking_done = true,
            _ => {}
        }
    }

    assert!(done_seen);
    assert!(saw_thinking_delta, "orphan ThinkingDelta should still be emitted");
    assert!(
        !saw_thinking_done,
        "no thinking block was opened or closed; no ThinkingDone expected"
    );
    mock.assert_async().await;
}
```

- [x] **Step 2: Run the new tests**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic redacted_thinking_block_is_silently_consumed orphan_thinking_delta_without_start_does_not_crash
```
Expected: both PASS without any code change. If either fails, fix the underlying code path before continuing.

- [x] **Step 3: Commit**

```bash
cd sdks/rust
git add tests/anthropic_stream.rs
git commit -m "test(anthropic): cover redacted_thinking and orphan thinking_delta

Adds two defensive tests that lock in already-correct behavior
from Tasks 2 and 3: redacted_thinking blocks are silently
consumed (no ThinkingDelta/Done emitted), and a malformed stream
with a thinking_delta but no preceding content_block_start still
emits the event without crashing or accumulating into a missing
buffer.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 6: Update `collect_stream` to populate `ChatResponse.thinking` from accumulated deltas

**Files:**
- Modify: `sdks/rust/src/stream.rs` (the `collect_stream` function at lines 30-113)
- Test: `sdks/rust/src/stream.rs` (add to its `#[cfg(test)] mod tests` block, or create one if missing)

The placeholder arm from Task 1 swallowed thinking events. Replace it with real accumulation so the streaming path produces a `ChatResponse` with the same `thinking: Option<String>` field shape as the non-streaming path.

- [x] **Step 1: Write the failing test**

In `sdks/rust/src/stream.rs`, locate the `#[cfg(test)] mod tests` block (if absent, append one at the end of the file). Add:

```rust
#[cfg(test)]
mod thinking_collect_tests {
    use super::*;
    use crate::types::{StreamEvent, StreamEventType};
    use tokio_stream::iter;

    #[tokio::test]
    async fn collect_stream_accumulates_thinking_into_response_thinking() {
        let events = vec![
            StreamEvent::thinking_delta("Let me "),
            StreamEvent::thinking_delta("think..."),
            StreamEvent::thinking_done("Let me think..."),
            StreamEvent::text("Answer: "),
            StreamEvent::text("42"),
            StreamEvent::done(),
        ];
        let stream = iter(events);
        let resp = collect_stream(stream).await;
        assert_eq!(resp.content, "Answer: 42");
        assert_eq!(
            resp.thinking.as_deref(),
            Some("Let me think..."),
            "thinking field must come from ThinkingDone (or accumulated deltas if no Done)"
        );
    }

    #[tokio::test]
    async fn collect_stream_no_thinking_keeps_thinking_none() {
        let events = vec![
            StreamEvent::text("hello"),
            StreamEvent::done(),
        ];
        let stream = iter(events);
        let resp = collect_stream(stream).await;
        assert_eq!(resp.content, "hello");
        assert!(resp.thinking.is_none());
    }

    #[tokio::test]
    async fn collect_stream_falls_back_to_accumulated_deltas_if_no_done() {
        // Defensive: if a provider somehow emits ThinkingDelta but skips
        // ThinkingDone, collect_stream still produces a thinking field
        // from the accumulated deltas. This matches the StreamEventType
        // docstring: "Backends that only know the final thinking text
        // may emit a single ThinkingDone with the full text and skip
        // ThinkingDelta entirely" — and the symmetric tolerance.
        let events = vec![
            StreamEvent::thinking_delta("A "),
            StreamEvent::thinking_delta("B"),
            StreamEvent::text("ok"),
            StreamEvent::done(),
        ];
        let stream = iter(events);
        let resp = collect_stream(stream).await;
        assert_eq!(resp.thinking.as_deref(), Some("A B"));
        assert_eq!(resp.content, "ok");
    }
}
```

- [x] **Step 2: Run tests to verify they fail**

Run:
```bash
cd sdks/rust && cargo test --lib -p motosan-ai thinking_collect_tests
```
Expected: all three FAIL — `resp.thinking` is `None` because the placeholder arm from Task 1 discards thinking events.

- [x] **Step 3: Update `collect_stream` to handle the new variants**

In `sdks/rust/src/stream.rs`, locate the function (lines 30-113). You need three changes:

(a) Add two new local accumulators near the top of the function (alongside the existing `content`, `tool_calls`, etc.). Find this block (lines ~33-42):

```rust
    let mut content = String::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut current_tc_id = String::new();
    let mut current_tc_name = String::new();
    let mut current_tc_args = String::new();
    let mut input_tokens = 0u32;
    let mut output_tokens = 0u32;
    let mut cache_creation_input_tokens: Option<u32> = None;
    let mut cache_read_input_tokens: Option<u32> = None;
    let mut explicit_stop_reason: Option<StopReason> = None;
```

Append after `explicit_stop_reason`:

```rust
    // Thinking accumulation. `thinking_delta_buf` collects every
    // ThinkingDelta as a fallback in case the provider does not emit
    // ThinkingDone. `thinking_done_buf` holds the explicit final text
    // from the most recent ThinkingDone and takes priority on assembly.
    let mut thinking_delta_buf = String::new();
    let mut thinking_done_buf: Option<String> = None;
```

(b) Replace the placeholder match arm (added in Task 1) with real handling. Find the match (lines ~55-91, plus the Task-1 placeholder appended at ~91). The full arm block should now read:

```rust
        match event.event_type {
            StreamEventType::Text => {
                content.push_str(&event.content);
            }
            StreamEventType::Usage => {
                if let Some(ref usage) = event.usage {
                    input_tokens += usage.input_tokens;
                    output_tokens += usage.output_tokens;
                    if let Some(v) = usage.cache_creation_input_tokens {
                        *cache_creation_input_tokens.get_or_insert(0) += v;
                    }
                    if let Some(v) = usage.cache_read_input_tokens {
                        *cache_read_input_tokens.get_or_insert(0) += v;
                    }
                }
            }
            StreamEventType::ToolCallStart => {
                current_tc_id = event.tool_call_id.unwrap_or_default();
                current_tc_name = event.tool_call_name.unwrap_or_default();
                current_tc_args.clear();
            }
            StreamEventType::ToolCallArgs => {
                if let Some(delta) = &event.tool_call_args_delta {
                    current_tc_args.push_str(delta);
                }
            }
            StreamEventType::ToolCallEnd => {
                let input: serde_json::Value = serde_json::from_str(&current_tc_args)
                    .unwrap_or_else(|_| serde_json::json!({}));
                tool_calls.push(ToolCall {
                    id: std::mem::take(&mut current_tc_id),
                    name: std::mem::take(&mut current_tc_name),
                    input,
                });
                current_tc_args.clear();
            }
            StreamEventType::ThinkingDelta => {
                thinking_delta_buf.push_str(&event.content);
            }
            StreamEventType::ThinkingDone => {
                // ThinkingDone carries the full text. Prefer it over the
                // delta accumulator when available — the provider knows
                // the authoritative concatenation. Also clear the delta
                // buffer so a second thinking block starts fresh.
                thinking_done_buf = Some(event.content.clone());
                thinking_delta_buf.clear();
            }
        }
```

(c) Update the `ChatResponse` construction at the bottom (lines ~100-113). Change the `thinking: None` line:

```rust
    let thinking = match thinking_done_buf {
        Some(text) if !text.is_empty() => Some(text),
        Some(_) => None, // explicit empty thinking block -> treat as none
        None if !thinking_delta_buf.is_empty() => Some(thinking_delta_buf),
        None => None,
    };

    ChatResponse {
        content,
        thinking,
        model: String::new(),
        usage: Usage {
            input_tokens,
            output_tokens,
            cache_creation_input_tokens,
            cache_read_input_tokens,
        },
        stop_reason,
        tool_calls,
    }
```

Note we **moved `thinking` out of the struct literal** into a local for clarity. Remove the literal `thinking: None,` line from the struct construction (it's now `thinking,`).

- [x] **Step 4: Verify no placeholder arms remain**

Run:
```bash
cd sdks/rust && grep -n 'Placeholder; real handling lands in Task' src/stream.rs
```
Expected: **no output**. If any matches, you missed deleting the Task-1 placeholder.

- [x] **Step 5: Run the new tests + full lib tests**

Run:
```bash
cd sdks/rust && cargo test --lib -p motosan-ai thinking_collect_tests
```
Expected: all three PASS.

Then:
```bash
cd sdks/rust && cargo test --lib -p motosan-ai
```
Expected: ALL lib tests PASS, no regression in existing `collect_stream` coverage.

- [x] **Step 6: End-to-end sanity check: anthropic_stream tests still pass with the real arm in place**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic anthropic_stream
```
Expected: ALL PASS.

- [x] **Step 7: Commit**

```bash
cd sdks/rust
git add src/stream.rs
git commit -m "feat(stream): populate ChatResponse.thinking from accumulated thinking events

collect_stream now accumulates ThinkingDelta content and prefers
ThinkingDone's authoritative full-text payload, producing the
same ChatResponse.thinking field shape the non-streaming path
already provides. Replaces the Task-1 placeholder arm. Three new
unit tests cover: explicit ThinkingDone, no thinking events at
all, and the defensive ThinkingDelta-without-Done fallback path.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 7: Ripple-check exhaustive matches, clippy, fmt

**Files:**
- Inspect (likely no changes): every `.rs` file in `sdks/rust/`, including `tests/`, that does `match event.event_type` or destructures `StreamEventType::`

`StreamEventType` is not `#[non_exhaustive]`. Tasks 1 and 6 covered the only internal exhaustive match sites we knew about. This task hunts for any others that the variant additions silently broke (or got hidden behind a wildcard arm that should now explicitly handle thinking).

- [x] **Step 1: Find every match site on `StreamEventType` in `sdks/rust/`**

Run:
```bash
cd sdks/rust && rg -n 'match.*event_type|StreamEventType::' src/ tests/ examples/ 2>/dev/null
```

Read every result. Note these were already triaged at plan-write time:
- `src/stream.rs` — handled in Task 6.
- `src/client.rs:1125, :1248` — `==` comparisons, not match. No change needed.
- `src/providers/anthropic.rs:859, :864` — these match on raw SSE event-name strings, not on `StreamEventType`. No change needed.
- `src/providers/codex_cli/mod.rs:616` — has `_ => {}` wildcard. No change needed.
- `src/providers/gemini_cli/stream_json.rs:211` — a test function name, not a match. No change needed.
- `src/providers/gemini_code_assist.rs:431, :450, :466` — `==` filters, not match. No change needed.
- `tests/anthropic_*.rs`, `tests/core_types.rs`, `tests/collect_stream.rs` — most are `==` checks or `_ => {}` matches. Spot-check any non-test exhaustive matches.

If a new exhaustive match site exists, add explicit arms:

```rust
StreamEventType::ThinkingDelta | StreamEventType::ThinkingDone => {
    // advisory; not relevant to <this code path's purpose>
}
```

- [x] **Step 2: Format check**

Run:
```bash
cd sdks/rust && cargo fmt --all -- --check
```
Expected: clean. If it fails, run `cargo fmt --all` and stage the changes.

- [x] **Step 3: Clippy with warnings as errors**

Run:
```bash
cd sdks/rust && cargo clippy --all-features --all-targets -- -D warnings
```
Expected: clean.

If clippy flags anything new, fix it inline. Likely candidates:
- `match_same_arms` if you wrote `ThinkingDelta => {} ThinkingDone => {}` — collapse to `ThinkingDelta | ThinkingDone => {}`.
- `single_match` on the `block_type` match introduced in Task 2 — only relevant if you reduced it back to one arm.

- [x] **Step 4: Full test sweep across all features**

Run:
```bash
cd sdks/rust && cargo test --all-features
```
Expected: ALL PASS. Total new test count: 5 in `types.rs` (Task 1) + 1 in `anthropic_stream.rs` (Task 2) + 1 (Task 3) + 2 (Task 4) + 2 (Task 5) + 3 in `stream.rs` (Task 6) = **14 new tests**.

- [x] **Step 5: Commit (only if Step 1 found any ripple fixes)**

If you made changes in Step 1:

```bash
cd sdks/rust
git add -p src/ tests/ examples/
git commit -m "chore: ripple-fix exhaustive matches for new ThinkingDelta/Done variants

Adds explicit advisory arms in any non-core site that exhaustively
matches on StreamEventType and was broken by the new variants.
Behavior is unchanged.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

If no changes were needed, skip the commit and note "no ripple fixes required" in the final report.

---

### Task 8: Live integration test against real Anthropic API

**Files:**
- Modify: `sdks/rust/tests/anthropic_live.rs` (append one live test at the end, following the existing convention)

This test defends against future regressions in the SSE parsing of real Anthropic responses — mockito tests can drift from the live wire format over time.

**Important: this repo does NOT use `#[ignore]` for live tests.** The convention (see existing `live_chat_basic` at `:44`, `live_stream_basic` at `:69`, etc.) is:
- Plain `#[tokio::test]` with no `#[ignore]`
- Call the `client()` helper at the top — if it returns `None`, `eprintln!` and early-return
- Call `cooldown().await` at the end to rate-limit the API
- Run command: `ANTHROPIC_API_KEY=... cargo test --features anthropic --test anthropic_live -- --nocapture` (no `--ignored`)

The test silently skips when `ANTHROPIC_API_KEY` is unset (so `cargo test --all-features` in CI without keys is a no-op), and runs against the real API when set.

- [ ] **Step 1: Inspect existing live test conventions**

Read `sdks/rust/tests/anthropic_live.rs` to confirm:
- The `client()`/`api_key()`/`cooldown()` helpers and their signatures.
- The model + builder shape used by existing streaming tests (`live_stream_basic`, `live_stream_tool_use`).
- That `stream_with(request)` is the API for full `ChatRequest`, while `stream(messages)` takes `Vec<Message>`.

Run:
```bash
cd sdks/rust && head -50 tests/anthropic_live.rs && rg -n 'fn client\(|fn api_key\(|fn cooldown\(|stream_with' tests/anthropic_live.rs | head -10
```

- [ ] **Step 2: Write the test**

Append to `sdks/rust/tests/anthropic_live.rs`, mirroring the style of `live_stream_basic` (`:68-102`) and `live_stream_tool_use` (`:259+`):

```rust
// ---------------------------------------------------------------------------
// N. streaming thinking
// ---------------------------------------------------------------------------

#[tokio::test]
async fn live_stream_thinking_events() {
    let Some(client) = client() else {
        eprintln!("ANTHROPIC_API_KEY not set, skipping");
        return;
    };

    // Use a model + budget that reliably produces thinking output.
    // claude-sonnet-4-5 with 4000 budget tokens is a known-good baseline;
    // adjust if Anthropic deprecates it.
    let request = ChatRequest::builder()
        .model("claude-sonnet-4-5")
        .message(Message::user(
            "Think step-by-step about whether 17 is prime, then answer yes or no.",
        ))
        .thinking(4000)
        .max_tokens(2048)
        .build();

    let mut stream = client.stream_with(request).await.expect("stream failed");

    let mut thinking_chunks = 0usize;
    let mut thinking_done_text: Option<String> = None;
    let mut answer = String::new();
    let mut done_seen = false;

    while let Some(ev) = stream.next().await {
        if ev.done {
            done_seen = true;
            break;
        }
        match ev.event_type {
            StreamEventType::ThinkingDelta => {
                thinking_chunks += 1;
            }
            StreamEventType::ThinkingDone => {
                thinking_done_text = Some(ev.content.clone());
            }
            StreamEventType::Text => {
                answer.push_str(&ev.content);
            }
            _ => {}
        }
    }

    assert!(done_seen, "stream must terminate with done");
    assert!(
        thinking_chunks > 0,
        "Anthropic should have emitted at least one ThinkingDelta for a step-by-step prompt"
    );
    assert!(
        thinking_done_text
            .as_deref()
            .map(|s| !s.is_empty())
            .unwrap_or(false),
        "ThinkingDone must carry the full concatenated thinking text"
    );
    assert!(
        !answer.is_empty(),
        "the model must produce a final answer after thinking"
    );
    // Sanity: thinking content should not leak into the answer.
    if let Some(t) = thinking_done_text.as_deref() {
        // First 30 chars of thinking should not appear verbatim in answer
        // (loose check; thinking is reasoning, answer is conclusion).
        let probe = t.chars().take(30).collect::<String>();
        if !probe.is_empty() {
            assert!(
                !answer.contains(&probe),
                "thinking content leaked into answer — bug in adapter"
            );
        }
    }
    cooldown().await;
}
```

- [x] **Step 3: Verify the test compiles (but does not run)**

Run:
```bash
cd sdks/rust && cargo test -p motosan-ai --features anthropic --test anthropic_live --no-run
```
Expected: clean build.

- [ ] **Step 4: (Optional) Run the live test if you have an API key handy**

```bash
ANTHROPIC_API_KEY=sk-ant-... cargo test --features anthropic --test anthropic_live live_stream_thinking_events -- --nocapture
```
Expected: PASS. If the user has no key handy, **skip** — the test silently early-returns when `ANTHROPIC_API_KEY` is unset, so it's safe to leave in the normal suite.

- [x] **Step 5: Commit**

```bash
cd sdks/rust
git add tests/anthropic_live.rs
git commit -m "test(anthropic-live): live regression for streaming thinking events

Live test that hits real Anthropic API with thinking(4000) and
asserts: stream terminates, at least one ThinkingDelta is seen,
ThinkingDone carries non-empty text, the final answer is
non-empty, and thinking content does not leak into the answer.
Follows the repo's live-test convention (skip if no API key, no
#[ignore], cooldown at end).

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

---

### Task 9: Version bump, CHANGELOG, AGENTS.md, llms.txt, SKILL.md

**Files:**
- Modify: `sdks/rust/Cargo.toml` — version 0.15.3 → 0.15.4
- Modify: `sdks/rust/CHANGELOG.md` — new `[0.15.4]` entry at top
- Modify: `AGENTS.md` — version header + entry
- Modify: `llms.txt` — version header + entry / table update
- Modify: `skills/motosan-ai/SKILL.md` — version header

Per `CLAUDE.md`: "Files to update: `Cargo.toml`/`pyproject.toml`, `CHANGELOG.md`, `AGENTS.md`, `llms.txt`, `skills/motosan-ai/SKILL.md`." We touch `Cargo.toml` (not `pyproject.toml` — no Python change).

- [x] **Step 1: Bump `sdks/rust/Cargo.toml`**

In `sdks/rust/Cargo.toml`, change:

```toml
version = "0.15.3"
```

to:

```toml
version = "0.15.4"
```

- [x] **Step 2: Add the CHANGELOG entry**

In `sdks/rust/CHANGELOG.md`, insert a new section directly under the `# Changelog` header and the boilerplate paragraph, **before** the existing `## [0.15.3] - 2026-05-17` entry:

```markdown
## [0.15.4] - 2026-05-23

### Added

- **`StreamEventType::ThinkingDelta` and `StreamEventType::ThinkingDone`** plus matching `StreamEvent::thinking_delta(...)` / `StreamEvent::thinking_done(...)` constructors. (`sdks/rust/src/types.rs`.) Variant count goes 5 → 7. **Wire-breaking** for any downstream that does an exhaustive `match event.event_type { ... }` on `StreamEventType` without a `_ =>` arm; internal `collect_stream` and `codex_cli` test updated. Pre-1.0 we ship as patch.
- **Anthropic streaming thinking support.** `AnthropicStreamAdapter` (`sdks/rust/src/providers/anthropic.rs`) gains a `current_thinking_buf: Option<String>` accumulator. `content_block_start { type: "thinking" }` opens it. `content_block_delta { type: "thinking_delta", thinking: "..." }` accumulates the text **from `delta.thinking`** (a previous bug-by-omission read `delta.text` and silently dropped these) and emits `StreamEvent::thinking_delta`. `content_block_stop` for a thinking block emits `StreamEvent::thinking_done` carrying the full concatenated text and clears the accumulator. `signature_delta` and `redacted_thinking` blocks are silently consumed — no streaming surface for cryptographic re-feed signatures or redacted content, matching the non-streaming `ChatResponse.thinking` field's shape.
- **`collect_stream` populates `ChatResponse.thinking`** from accumulated `ThinkingDelta`s, preferring `ThinkingDone`'s authoritative payload when present. Streaming and non-streaming Anthropic responses now produce the same `ChatResponse.thinking: Option<String>` shape. (`sdks/rust/src/stream.rs`.)

### Notes

- Fourteen new tests across `types.rs`, `tests/anthropic_stream.rs`, and `src/stream.rs` lock the behavior in: variant existence + serde round-trip; constructor field shape; SSE → event mapping for thinking-only / thinking-then-text / redacted_thinking / orphan-delta / signature-delta cases; collect_stream accumulation including the `ThinkingDelta`-without-`ThinkingDone` fallback path.
- New live test `live_stream_thinking_events` in `tests/anthropic_live.rs` hits the real API with `thinking(4000)` and asserts stream terminates, ThinkingDelta count > 0, ThinkingDone non-empty, answer non-empty, no content leak. Follows the repo convention: no `#[ignore]`, silently skips when `ANTHROPIC_API_KEY` is unset, calls `cooldown()` at end.
- Python SDK unchanged — Anthropic streaming thinking on the Python side is a separate plan (per `CLAUDE.md` "No FFI or shared code between SDKs"). Other providers (OpenAI, Gemini, MiniMax, Ollama, Codex CLI, Claude Code CLI) do not emit `StreamEventType::ThinkingDelta`/`ThinkingDone` — only Anthropic currently has a wire format for streaming extended thinking.

### Consumer impact

- Unblocks `motosan-agent-loop` v0.21.4's `TODO(thinking-stream)` markers at `src/motosan_ai_impl.rs:171` and `:346` — once that crate bumps its `motosan-ai` dep to `^0.15.4` and wires the two new arms, `CoreEvent::ThinkingChunk`/`ThinkingDone` will flow end-to-end from Anthropic SSE to consumers (capo TUI).
```

- [x] **Step 3: Update `AGENTS.md`**

In the workspace-root `AGENTS.md`, find the version header (looks like `Version: 0.15.3 (crates.io)`) and bump it. Also add a Recent Additions entry following the existing pattern (one bullet, dense). Example diff:

```markdown
Version: 0.15.4 (crates.io)
```

And add at the top of the "Recent Additions" or equivalent section:

```markdown
- **0.15.4 — Anthropic streaming thinking events.** `StreamEventType::ThinkingDelta` + `ThinkingDone` variants; `StreamEvent::thinking_delta`/`thinking_done` constructors. `AnthropicStreamAdapter` parses `content_block_delta { type: thinking_delta }` (reads `delta.thinking`, not `delta.text` — fixing a silent drop), opens/closes a `current_thinking_buf` accumulator, emits `ThinkingDone` on `content_block_stop` with the full text. `signature_delta` and `redacted_thinking` silently consumed. `collect_stream` now populates `ChatResponse.thinking` so streaming/non-streaming shapes match. Unblocks `motosan-agent-loop` v0.21.4's TODO markers. Wire-breaking for exhaustive `match` on `StreamEventType` without `_ =>`; shipped as patch under pre-1.0 semver.
```

(Match the exact heading style of the existing Recent Additions entries — re-read AGENTS.md to confirm.)

- [x] **Step 4: Update `llms.txt`**

In `llms.txt`:
1. Bump the version header (`Version: 0.15.3` → `0.15.4`).
2. Find the `StreamEventType` reference (if any) and extend its variant list to include `ThinkingDelta`, `ThinkingDone`.
3. If there's a streaming-events section, add a one-paragraph note about thinking events mirroring the CHANGELOG.

If `llms.txt` does not currently enumerate `StreamEventType` variants, skip the variant list update but do still bump the version header.

- [x] **Step 5: Update `skills/motosan-ai/SKILL.md`**

Find the version line near the top (likely line 6-10, modeled after the equivalent `motosan-agent-loop` SKILL.md at `skills/motosan-agent-loop/SKILL.md:8`). Open the file and edit the version string from `v0.15.3` → `v0.15.4`. Use the `Edit` tool (or your editor) rather than `sed`, since `sed -i` syntax differs between BSD (macOS) and GNU (Linux).

Verify the result:
```bash
cd ~/Projects/wade/motosan-ai && grep -n '0\.15\.' skills/motosan-ai/SKILL.md
```
Expected: every match shows `0.15.4`.

- [x] **Step 6: Run the full test suite one more time**

Run:
```bash
cd sdks/rust && cargo test --all-features
```
Expected: ALL PASS.

- [x] **Step 7: Clippy + fmt one more time**

Run:
```bash
cd sdks/rust && cargo fmt --all -- --check && cargo clippy --all-features --all-targets -- -D warnings
```
Expected: clean both.

- [x] **Step 8: rustdoc check**

Run:
```bash
cd sdks/rust && cargo doc --all-features --no-deps -p motosan-ai 2>&1 | grep -E 'warning|error'
```
Expected: no warnings, no errors. Intra-doc links to `StreamEventType::ThinkingDelta` and `crate::stream::collect_stream` must resolve.

- [x] **Step 9: Confirm publishable**

Run:
```bash
cd sdks/rust && cargo publish --dry-run -p motosan-ai --all-features
```
Expected: success. Do **not** actually publish — that's the user's call.

- [x] **Step 10: Commit**

```bash
cd ~/Projects/wade/motosan-ai
git add sdks/rust/Cargo.toml sdks/rust/CHANGELOG.md AGENTS.md llms.txt skills/motosan-ai/SKILL.md
git commit -m "release(motosan-ai): v0.15.4 - Anthropic streaming thinking events

Adds StreamEventType::ThinkingDelta/Done + Anthropic SSE adapter
support so consumers can render extended-thinking content as it
streams. collect_stream populates ChatResponse.thinking from the
new events. Unblocks motosan-agent-loop v0.21.4's TODO markers.

Refs: docs/superpowers/plans/2026-05-23-anthropic-thinking-stream-events.md"
```

- [x] **Step 11: Report back**

Summarise to the requester:

- Final commit SHA on the working branch (one per task that committed).
- Test count delta (expected **+14 unit/integration** + **+1 live** = +15 tests).
- Output of `cargo publish --dry-run` (last few lines).
- Whether Task 7 required any ripple fixes, and if so where.
- Whether the live test (Task 8 Step 4) was actually run, and if so its result.
- Confirmation that `motosan-agent-loop`'s TODO marker file path (`src/motosan_ai_impl.rs:171, :346`) is now actionable — but do NOT modify it.

**Do NOT publish to crates.io. Do NOT push to the remote.** The user will handle release.

---

## Anti-scope (things explicitly NOT in this plan)

1. **Do not modify the Python SDK** (`sdks/python/`). Streaming thinking on the Python side is a separate plan. Per `CLAUDE.md`: "No FFI or shared code between SDKs — each language is idiomatic."
2. **Do not modify `motosan-agent-loop`**. Its TODO markers at `src/motosan_ai_impl.rs:171, :346` get cleared by a follow-up plan in that repo once `motosan-ai` v0.15.4 is published.
3. **Do not add `#[non_exhaustive]`** to `StreamEventType`. That would itself be a breaking change requiring a major bump and is out of scope.
4. **Do not surface `signature_delta` content** in the streaming API. The non-streaming `ChatResponse.thinking: Option<String>` field doesn't expose signatures either — keep them consistent. If signature round-tripping for multi-turn thinking re-feeds is ever needed, it's a separate plan with new types (`StreamEvent::thinking_signature(...)` etc.).
5. **Do not surface `redacted_thinking` content** as `ThinkingDelta`/`Done`. The opaque encrypted `data` field is not useful for rendering and exposing it as text would be misleading. Silently drop the entire block. If round-tripping is needed, separate plan.
6. **Do not add streaming-thinking support to OpenAI, Gemini, MiniMax, Ollama, or any CLI provider.** None of them currently have a documented streaming wire format for extended thinking. The non-streaming MiniMax `live_minimax_thinking_blocks_exposed` test exercises a non-streaming path. If OpenAI's o-series streaming "reasoning" surface is ever added, that's a separate plan.
7. **Do not bump dependencies** (`reqwest`, `tokio`, `eventsource-stream`, etc.) as part of this work.
8. **Do not publish.** Dry-run only.
9. **Do not change `ChatResponse.thinking` field type or add a signature field.** It stays `Option<String>`. Future expansion (e.g. `ChatResponse.thinking_signature: Option<String>`) is out of scope.

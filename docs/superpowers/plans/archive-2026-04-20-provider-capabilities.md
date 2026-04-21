# Provider Capabilities Implementation Plan

> ⚠️ **Archive note:** This is a historical implementation plan. API snippets here may not match current released interfaces. Use `README.md`, `sdks/rust/README.md`, `sdks/python/README.md`, and `specs/types.md` as source of truth.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `capabilities()` method to `ProviderImpl` so providers declare what content types they support, with automatic pre-flight validation in `LlmClient` that returns `Err(UnsupportedFeature)` before sending unsupported content to an API.

**Architecture:** `ProviderCapabilities` struct holds `supports_image` and `supports_document` booleans. `ProviderImpl` gains two provided methods: `capabilities()` (defaults to text-only) and `validate_request()` (iterates `content_blocks`, returns `Err` on unsupported types). `LlmClient::dispatch_chat` and `dispatch_stream_inner` call `validate_request` before dispatching to each provider. The existing `reject_document_blocks()` helper is deleted — superseded by the framework layer.

**Tech Stack:** Rust, `async_trait`, `thiserror`. No new dependencies.

---

## File Map

| File | Change |
|------|--------|
| `sdks/rust/src/types.rs` | Add `ProviderCapabilities` struct |
| `sdks/rust/src/providers/mod.rs` | Add `capabilities()` + `validate_request()` to trait; remove `reject_document_blocks()`; fix `ContentBlock` import cfg gate |
| `sdks/rust/src/client.rs` | Call `validate_request` in `dispatch_chat` and `dispatch_stream_inner` |
| `sdks/rust/src/providers/anthropic.rs` | Override `capabilities()` → `full()` |
| `sdks/rust/src/providers/openai.rs` | Override `capabilities()` → `with_image()`; remove `reject_document_blocks` calls |
| `sdks/rust/src/providers/gemini.rs` | Override `capabilities()` → `with_image()` |
| `sdks/rust/src/providers/gemini_code_assist.rs` | Override `capabilities()` → `with_image()` |
| `sdks/rust/src/providers/minimax.rs` | Remove `reject_document_blocks` calls and import |

---

## Task 1: Add `ProviderCapabilities` to `types.rs`

**Files:**
- Modify: `sdks/rust/src/types.rs`

- [ ] **Step 1: Write the failing test**

At the bottom of `sdks/rust/src/types.rs`, add:

```rust
#[cfg(test)]
mod capabilities_tests {
    use super::*;

    #[test]
    fn text_only_has_no_capabilities() {
        let caps = ProviderCapabilities::text_only();
        assert!(!caps.supports_image);
        assert!(!caps.supports_document);
    }

    #[test]
    fn with_image_supports_image_only() {
        let caps = ProviderCapabilities::with_image();
        assert!(caps.supports_image);
        assert!(!caps.supports_document);
    }

    #[test]
    fn full_supports_everything() {
        let caps = ProviderCapabilities::full();
        assert!(caps.supports_image);
        assert!(caps.supports_document);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test capabilities_tests 2>&1 | grep -E "error|FAILED|cannot find"
```

Expected: compile error — `ProviderCapabilities` not found.

- [ ] **Step 3: Add `ProviderCapabilities` to `types.rs`**

Find the end of the public type definitions in `sdks/rust/src/types.rs` (before the first `#[cfg(test)]` block) and add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCapabilities {
    pub supports_image: bool,
    pub supports_document: bool,
}

impl ProviderCapabilities {
    pub fn text_only() -> Self {
        Self { supports_image: false, supports_document: false }
    }

    pub fn with_image() -> Self {
        Self { supports_image: true, supports_document: false }
    }

    pub fn full() -> Self {
        Self { supports_image: true, supports_document: true }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd sdks/rust && cargo test capabilities_tests 2>&1 | grep -E "test.*ok|FAILED|error"
```

Expected:
```
test types::capabilities_tests::text_only_has_no_capabilities ... ok
test types::capabilities_tests::with_image_supports_image_only ... ok
test types::capabilities_tests::full_supports_everything ... ok
```

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/types.rs
git commit -m "feat(capabilities): add ProviderCapabilities type"
```

---

## Task 2: Add `capabilities()` and `validate_request()` to `ProviderImpl`

**Files:**
- Modify: `sdks/rust/src/providers/mod.rs`

- [ ] **Step 1: Write the failing tests**

At the bottom of `sdks/rust/src/providers/mod.rs`, add:

```rust
#[cfg(test)]
mod validate_tests {
    use super::*;
    use crate::types::{ContentBlock, ImageSource, Message, ProviderCapabilities};

    struct TextOnlyProvider;

    #[async_trait]
    impl ProviderImpl for TextOnlyProvider {
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
            unimplemented!()
        }
        async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
            unimplemented!()
        }
    }

    struct FullProvider;

    #[async_trait]
    impl ProviderImpl for FullProvider {
        fn capabilities(&self) -> ProviderCapabilities {
            ProviderCapabilities::full()
        }
        async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError> {
            unimplemented!()
        }
        async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError> {
            unimplemented!()
        }
    }

    fn req_with_image() -> ChatRequest {
        let msg = Message::user_with_image("look", "abc123", "image/png");
        ChatRequest::builder().messages(vec![msg]).build()
    }

    fn req_with_document() -> ChatRequest {
        let msg = Message::user_with_pdf_base64("read this", "abc123");
        ChatRequest::builder().messages(vec![msg]).build()
    }

    fn req_text_only() -> ChatRequest {
        ChatRequest::builder().messages(vec![Message::user("hello")]).build()
    }

    #[test]
    fn text_only_provider_rejects_image() {
        let p = TextOnlyProvider;
        let result = p.validate_request(&req_with_image());
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[test]
    fn text_only_provider_rejects_document() {
        let p = TextOnlyProvider;
        let result = p.validate_request(&req_with_document());
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[test]
    fn full_provider_accepts_image() {
        let p = FullProvider;
        assert!(p.validate_request(&req_with_image()).is_ok());
    }

    #[test]
    fn full_provider_accepts_document() {
        let p = FullProvider;
        assert!(p.validate_request(&req_with_document()).is_ok());
    }

    #[test]
    fn any_provider_accepts_plain_text() {
        let p = TextOnlyProvider;
        assert!(p.validate_request(&req_text_only()).is_ok());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test validate_tests 2>&1 | grep -E "error|FAILED|cannot find"
```

Expected: compile errors — `capabilities`, `validate_request` not found on trait.

- [ ] **Step 3: Update the `ContentBlock` import and the `ProviderImpl` trait**

In `sdks/rust/src/providers/mod.rs`:

**3a.** Find this import block (around line 12–13):
```rust
#[cfg(any(feature = "openai", feature = "minimax", feature = "ollama_native"))]
use crate::types::ContentBlock;
```

Replace with (add `ProviderCapabilities` and remove the cfg gate on `ContentBlock`):
```rust
use crate::types::{ContentBlock, ProviderCapabilities};
```

**3b.** Replace the `ProviderImpl` trait (lines 72–76) with:
```rust
#[async_trait]
pub trait ProviderImpl: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities::text_only()
    }

    fn validate_request(&self, req: &ChatRequest) -> Result<(), MotosanError> {
        let caps = self.capabilities();
        for msg in &req.messages {
            for block in &msg.content_blocks {
                match block {
                    ContentBlock::Image { .. } if !caps.supports_image => {
                        return Err(MotosanError::UnsupportedFeature(
                            "provider does not support image input".into(),
                        ));
                    }
                    ContentBlock::Document { .. } if !caps.supports_document => {
                        return Err(MotosanError::UnsupportedFeature(
                            "provider does not support document input".into(),
                        ));
                    }
                    _ => {}
                }
            }
        }
        Ok(())
    }

    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse, MotosanError>;
    async fn stream(&self, _req: ChatRequest) -> Result<BoxStream, MotosanError>;
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd sdks/rust && cargo test validate_tests 2>&1 | grep -E "test.*ok|FAILED|error\["
```

Expected: 5 tests pass.

- [ ] **Step 5: Run full test suite to catch regressions**

```bash
cd sdks/rust && cargo test --all-features 2>&1 | tail -20
```

Expected: all existing tests still pass.

- [ ] **Step 6: Commit**

```bash
git add sdks/rust/src/providers/mod.rs
git commit -m "feat(capabilities): add capabilities() and validate_request() to ProviderImpl"
```

---

## Task 3: Override `capabilities()` in Anthropic, OpenAI, Gemini, GeminiCodeAssist

**Files:**
- Modify: `sdks/rust/src/providers/anthropic.rs`
- Modify: `sdks/rust/src/providers/openai.rs`
- Modify: `sdks/rust/src/providers/gemini.rs`
- Modify: `sdks/rust/src/providers/gemini_code_assist.rs`

- [ ] **Step 1: Write failing tests for each provider's capabilities**

At the bottom of each provider file, in its existing `#[cfg(test)]` block, add:

**`anthropic.rs`** — inside `mod tests`:
```rust
#[test]
fn capabilities_are_full() {
    let p = AnthropicProvider::new("key", None, None);
    let caps = p.capabilities();
    assert!(caps.supports_image);
    assert!(caps.supports_document);
}
```

**`openai.rs`** — inside `mod tests`:
```rust
#[test]
fn capabilities_support_image_only() {
    let p = OpenAIProvider::new("key", None, None, None);
    let caps = p.capabilities();
    assert!(caps.supports_image);
    assert!(!caps.supports_document);
}
```

**`gemini.rs`** — inside `mod tests`:
```rust
#[test]
fn capabilities_support_image_only() {
    let p = GeminiProvider::new("key", None, None);
    let caps = p.capabilities();
    assert!(caps.supports_image);
    assert!(!caps.supports_document);
}
```

**`gemini_code_assist.rs`** — inside the test module:
```rust
#[test]
fn capabilities_support_image_only() {
    let p = GeminiCodeAssistProvider::new("token".into(), None, None);
    let caps = p.capabilities();
    assert!(caps.supports_image);
    assert!(!caps.supports_document);
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd sdks/rust && cargo test --all-features "capabilities_are_full\|capabilities_support_image_only" 2>&1 | grep -E "FAILED|ok"
```

Expected: all 4 tests FAILED (default is text-only, assertions fail).

- [ ] **Step 3: Add `capabilities()` overrides**

In each `impl ProviderImpl for XProvider` block, add the `capabilities()` method before `async fn chat`:

**`anthropic.rs`** — in `impl ProviderImpl for AnthropicProvider`:
```rust
fn capabilities(&self) -> crate::types::ProviderCapabilities {
    crate::types::ProviderCapabilities::full()
}
```

**`openai.rs`** — in `impl ProviderImpl for OpenAIProvider`:
```rust
fn capabilities(&self) -> crate::types::ProviderCapabilities {
    crate::types::ProviderCapabilities::with_image()
}
```

**`gemini.rs`** — in `impl ProviderImpl for GeminiProvider`:
```rust
fn capabilities(&self) -> crate::types::ProviderCapabilities {
    crate::types::ProviderCapabilities::with_image()
}
```

**`gemini_code_assist.rs`** — in `impl ProviderImpl for GeminiCodeAssistProvider`:
```rust
fn capabilities(&self) -> crate::types::ProviderCapabilities {
    crate::types::ProviderCapabilities::with_image()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cd sdks/rust && cargo test --all-features "capabilities_are_full\|capabilities_support_image_only" 2>&1 | grep -E "FAILED|ok"
```

Expected: all 4 tests pass.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/providers/anthropic.rs sdks/rust/src/providers/openai.rs sdks/rust/src/providers/gemini.rs sdks/rust/src/providers/gemini_code_assist.rs
git commit -m "feat(capabilities): declare capabilities in Anthropic, OpenAI, Gemini providers"
```

---

## Task 4: Wire validation into `LlmClient` dispatch

**Files:**
- Modify: `sdks/rust/src/client.rs`

The pattern in both `dispatch_chat` and `dispatch_stream_inner` is:

```rust
// Before:
use crate::providers::ProviderImpl;
return self.build_X_provider().chat(request).await;

// After:
use crate::providers::ProviderImpl;
let p = self.build_X_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

Apply the same pattern to `stream`:
```rust
// Before:
use crate::providers::ProviderImpl;
return self.build_X_provider().stream(request).await;

// After:
use crate::providers::ProviderImpl;
let p = self.build_X_provider();
p.validate_request(&request)?;
return p.stream(request).await;
```

- [ ] **Step 1: Write a failing integration test**

At the bottom of `sdks/rust/src/client.rs`, add inside an existing `#[cfg(test)]` block (or create one):

```rust
#[cfg(test)]
mod dispatch_validation_tests {
    use super::*;
    use crate::types::{ContentBlock, ImageSource, Message};

    #[cfg(feature = "ollama_native")]
    #[tokio::test]
    async fn dispatch_chat_rejects_image_for_ollama() {
        let client = LlmClient::ollama_native("http://localhost:11434", None);
        let msg = Message::user_with_image("look", "abc123", "image/png");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.chat_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }

    #[cfg(feature = "ollama_native")]
    #[tokio::test]
    async fn dispatch_stream_rejects_image_for_ollama() {
        let client = LlmClient::ollama_native("http://localhost:11434", None);
        let msg = Message::user_with_image("look", "abc123", "image/png");
        let req = ChatRequest::builder().messages(vec![msg]).build();
        let result = client.stream_with(req).await;
        assert!(matches!(result, Err(MotosanError::UnsupportedFeature(_))));
    }
}
```

- [ ] **Step 2: Run to verify they fail**

```bash
cd sdks/rust && cargo test --features ollama_native dispatch_validation_tests 2>&1 | grep -E "FAILED|ok|error"
```

Expected: tests FAILED — network error or wrong error type (validation not wired yet).

- [ ] **Step 3: Update `dispatch_chat` in `client.rs`**

Apply the provider/validate/call pattern to every provider arm in `dispatch_chat`. The arms to update (all inside `#[cfg(feature = "...")]` blocks):

For **Anthropic** (feature = "anthropic"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_anthropic_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **OpenAI** (feature = "openai"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_openai_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **Minimax** (feature = "minimax"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_minimax_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **Ollama** native (feature = "ollama_native", `if self.ollama_native`):
```rust
use crate::providers::ProviderImpl;
let p = self.build_ollama_native_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **Ollama** non-native (feature = "ollama"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_ollama_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **ClaudeCode** (feature = "claude-code"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_claude_code_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **CodexCli** (feature = "codex-cli"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_codex_cli_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **GeminiCli** (feature = "gemini-cli"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_gemini_cli_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **Gemini** (feature = "gemini"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_gemini_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

For **GeminiCodeAssist** (feature = "gemini-code-assist"):
```rust
use crate::providers::ProviderImpl;
let p = self.build_gemini_code_assist_provider();
p.validate_request(&request)?;
return p.chat(request).await;
```

- [ ] **Step 4: Apply the same pattern to `dispatch_stream_inner`**

Same providers, same pattern, replace `.chat(request)` with `.stream(request)`.

- [ ] **Step 5: Run to verify tests pass**

```bash
cd sdks/rust && cargo test --features ollama_native dispatch_validation_tests 2>&1 | grep -E "FAILED|ok"
```

Expected: both tests pass.

- [ ] **Step 6: Run full test suite**

```bash
cd sdks/rust && cargo test --all-features 2>&1 | tail -20
```

Expected: all existing tests still pass.

- [ ] **Step 7: Commit**

```bash
git add sdks/rust/src/client.rs
git commit -m "feat(capabilities): wire validate_request into LlmClient dispatch"
```

---

## Task 5: Remove `reject_document_blocks()`

**Files:**
- Modify: `sdks/rust/src/providers/mod.rs`
- Modify: `sdks/rust/src/providers/openai.rs`
- Modify: `sdks/rust/src/providers/minimax.rs`

- [ ] **Step 1: Remove all call sites in `openai.rs`**

In `sdks/rust/src/providers/openai.rs`:

Remove from the import line (around line 5):
```rust
parse_retry_after, reject_document_blocks, sleep_before_retry,
```
→ Replace with:
```rust
parse_retry_after, sleep_before_retry,
```

Remove the two `reject_document_blocks(&req, "OpenAI")?;` lines (around lines 475 and 592). These are the only call sites in openai.rs — the `unreachable!()` comment on the Document arm of `build_request` can also be removed and replaced with `ContentBlock::Document { .. } => {}`.

- [ ] **Step 2: Remove all call sites in `minimax.rs`**

In `sdks/rust/src/providers/minimax.rs`:

Remove from the import line (around line 5):
```rust
parse_retry_after, reject_document_blocks, sleep_before_retry,
```
→ Replace with:
```rust
parse_retry_after, sleep_before_retry,
```

Remove the two `reject_document_blocks(&req, "MiniMax")?;` lines (around lines 343 and 463).

- [ ] **Step 3: Remove `reject_document_blocks` from `providers/mod.rs`**

In `sdks/rust/src/providers/mod.rs`, delete the entire function:
```rust
/// Return an `UnsupportedFeature` error if any message contains a `Document` block.
#[cfg(any(feature = "openai", feature = "minimax", feature = "ollama_native"))]
pub(crate) fn reject_document_blocks(
    req: &ChatRequest,
    provider_name: &str,
) -> Result<(), MotosanError> {
    for message in &req.messages {
        for block in &message.content_blocks {
            if matches!(block, ContentBlock::Document { .. }) {
                return Err(MotosanError::UnsupportedFeature(format!(
                    "Document content blocks (PDF) are not supported by the {} provider",
                    provider_name
                )));
            }
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Compile and test**

```bash
cd sdks/rust && cargo test --all-features 2>&1 | tail -20
```

Expected: all tests pass, no unused import warnings.

- [ ] **Step 5: Commit**

```bash
git add sdks/rust/src/providers/mod.rs sdks/rust/src/providers/openai.rs sdks/rust/src/providers/minimax.rs
git commit -m "refactor(capabilities): remove reject_document_blocks — superseded by validate_request"
```

---

## Task 6: Final verification

- [ ] **Step 1: Run the full CI gate**

```bash
cd /Users/daiwanwei/Projects/wade/motosan-ai && check-rust 2>&1 | tail -30
```

Expected: `fmt`, `clippy`, and `test --all-features` all pass with no warnings.

- [ ] **Step 2: Verify capabilities coverage across all providers**

```bash
cd sdks/rust && cargo test --all-features "capabilities" 2>&1 | grep -E "test.*ok|FAILED"
```

Expected: 7 capability tests pass (1 per provider that declares non-default capabilities, plus the `ProviderCapabilities` named-constructor tests).

- [ ] **Step 3: Commit if any final fixes were needed**

```bash
git add -p
git commit -m "fix(capabilities): final cleanup from CI gate"
```

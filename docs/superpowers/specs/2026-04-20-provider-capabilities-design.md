# Provider Capabilities — Design Spec

**Date:** 2026-04-20  
**Scope:** Rust SDK only (`sdks/rust/`)

## Problem

Message input handling across providers is inconsistent:

| Provider     | content_blocks | Image | Document        |
|--------------|---------------|-------|-----------------|
| Anthropic    | ✓ full        | ✓     | ✓               |
| OpenAI       | ✓ partial     | ✓     | explicit error  |
| Gemini HTTP  | ✓ partial     | ✓     | silently dropped |
| MiniMax      | ✗             | ✗     | explicit error  |
| Ollama       | ✗             | ✗     | explicit error  |
| Gemini CLI   | ✗             | ✗     | ✗               |
| Claude Code  | ✗             | ✗     | ✗               |

Issues:
- Gemini silently drops documents (data loss)
- Validation logic (`reject_document_blocks`) duplicated across providers
- No single source of truth for what a provider supports

## Design

### 1. `ProviderCapabilities` type

Added to `types.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub struct ProviderCapabilities {
    pub supports_image: bool,
    pub supports_document: bool,
}

impl ProviderCapabilities {
    pub fn text_only() -> Self { Self { supports_image: false, supports_document: false } }
    pub fn with_image() -> Self { Self { supports_image: true, supports_document: false } }
    pub fn full() -> Self { Self { supports_image: true, supports_document: true } }
}
```

### 2. `ProviderImpl` trait changes

Two methods added to the trait in `providers/mod.rs`:

```rust
fn capabilities(&self) -> ProviderCapabilities {
    ProviderCapabilities::text_only()  // safe default
}

fn validate_request(&self, req: &ChatRequest) -> Result<(), MotosanError> {
    let caps = self.capabilities();
    for msg in &req.messages {
        for block in &msg.content_blocks {
            match block {
                ContentBlock::Image { .. } if !caps.supports_image =>
                    return Err(MotosanError::UnsupportedFeature(
                        "provider does not support image input".into()
                    )),
                ContentBlock::Document { .. } if !caps.supports_document =>
                    return Err(MotosanError::UnsupportedFeature(
                        "provider does not support document input".into()
                    )),
                _ => {}
            }
        }
    }
    Ok(())
}
```

- `capabilities()` has a safe default (text-only) — existing providers compile without changes
- `validate_request()` is a provided method — validation logic lives in one place
- New `MotosanError::UnsupportedFeature(String)` variant required

### 3. `LlmClient` dispatch

`chat()` and `stream()` each get one validation line before dispatch:

```rust
pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, MotosanError> {
    self.provider.validate_request(&req)?;
    self.provider.chat(req).await
}

pub async fn stream(&self, req: ChatRequest) -> Result<BoxStream, MotosanError> {
    self.provider.validate_request(&req)?;
    self.provider.stream(req).await
}
```

### 4. Per-provider `capabilities()` overrides

Only three providers override the default:

| Provider    | Override                       |
|-------------|-------------------------------|
| Anthropic   | `ProviderCapabilities::full()` |
| OpenAI      | `ProviderCapabilities::with_image()` |
| Gemini HTTP | `ProviderCapabilities::with_image()` |
| Gemini Code Assist | `ProviderCapabilities::with_image()` |

All others (MiniMax, Ollama, Gemini CLI, Claude Code) use the text-only default.

`GeminiCodeAssistProvider` reuses `GeminiProvider::build_request` for message serialization, so it also gets `with_image()`.

**Cleanup:** `reject_document_blocks()` helper removed from OpenAI and MiniMax — superseded by framework-level validation.

## Error Handling

```rust
// New variant in MotosanError
UnsupportedFeature(String)
```

Error is returned before any HTTP request is made. Callers receive a clear error message identifying which content type is unsupported.

## Testing

**Unit — `ProviderCapabilities`**
- Named constructors produce correct field values

**Unit — `validate_request()`**
- text-only provider + Image block → `Err(UnsupportedFeature)`
- text-only provider + Document block → `Err(UnsupportedFeature)`
- full provider + Image + Document → `Ok(())`
- empty `content_blocks` → always `Ok(())`

**Unit — per-provider `capabilities()`**
- Anthropic → `full()`
- OpenAI, Gemini → `with_image()`
- MiniMax, Ollama, Gemini CLI, Claude Code → `text_only()`

No live API tests needed — capability declaration is pure logic.

## Files Changed

| File | Change |
|------|--------|
| `src/types.rs` | Add `ProviderCapabilities` |
| `src/error.rs` | Add `UnsupportedFeature` variant |
| `src/providers/mod.rs` | Add `capabilities()` + `validate_request()` to `ProviderImpl` |
| `src/client.rs` | Add `validate_request()` call in `chat()` and `stream()` |
| `src/providers/anthropic.rs` | Override `capabilities()` → `full()` |
| `src/providers/openai.rs` | Override `capabilities()` → `with_image()`; remove `reject_document_blocks()` |
| `src/providers/gemini.rs` | Override `capabilities()` → `with_image()` |
| `src/providers/gemini_code_assist.rs` | Override `capabilities()` → `with_image()` |
| `src/providers/minimax.rs` | Remove `reject_document_blocks()` |

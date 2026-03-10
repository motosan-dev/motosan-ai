# Changelog

All notable changes to `motosan-ai` Rust SDK are documented in this file.

## [0.1.0] - 2026-03-10

### Added
- Feature-gated provider support: Anthropic, OpenAI, MiniMax (`anthropic`, `openai`, `minimax`, `full`).
- Unified core types: `Message`, `ChatRequest`, `ChatResponse`, `Usage`, `StopReason`, `StreamEvent`.
- `Message` helper constructors and `ChatRequestBuilder` for ergonomic request construction.
- `Client` APIs: `chat`, `chat_with`, and `stream`.
- Provider implementations for chat + streaming on Anthropic/OpenAI/MiniMax.
- Shared provider mapping utilities and robust SSE parsing behavior.
- Integration tests for provider happy paths, streaming behavior, and error mapping.
- Rust CI workflow (`fmt`, `clippy`, `test`).

### Changed
- Centralized model defaults and model catalog in `src/models.rs`.
- Migrated SDK error type to `thiserror`-based `MotosanError`.
- Set MSRV to Rust `1.82` and added CI lane to validate no-feature builds/tests.

### Notes
- Default model baselines are maintained in `src/models.rs` and can be overridden via `ClientBuilder::model(...)` or `ChatRequest::builder().model(...)`.

# motosan-ai

Multi-language, multi-provider AI SDK. One unified interface for Anthropic, OpenAI, MiniMax — and more.

## Why motosan-ai?

Most AI SDKs are provider-specific. Switch providers = rewrite your integration.

`motosan-ai` gives you a **single interface** across providers. Switch models by changing one line.

```rust
// Rust — swap provider without touching business logic
let client = Client::builder()
    .provider(Provider::Anthropic)  // → Provider::OpenAI — done
    .api_key(&api_key)
    .build()?;
```

```python
# Python — same interface, any provider
client = Client(provider="openai")  # → "anthropic" — done
response = await client.chat([{"role": "user", "content": "Hello"}])
```

## Languages

| Language | Package | Status |
|----------|---------|--------|
| 🦀 Rust | `motosan-ai` (crates.io) | 🚧 v0.1.0 in progress |
| 🐍 Python | `motosan-ai` (PyPI) | 🚧 v0.2.0 planned |
| 🔷 TypeScript | `@motosan-ai/core` (npm) | 📋 v0.3.0 planned |

## Providers

| Provider | Models | Rust feature | Python extra |
|----------|--------|-------------|-------------|
| Anthropic | claude-opus-4, claude-sonnet-4 | `anthropic` | `[anthropic]` |
| OpenAI | gpt-4o, o3 | `openai` | `[openai]` |
| MiniMax | MiniMax-Text-01 | `minimax` | `[minimax]` |

## Repository Structure

```
motosan-ai/
├── sdks/
│   ├── rust/       Rust crate (feature-flagged providers)
│   ├── python/     Python package (optional deps per provider)
│   └── typescript/ TypeScript package (planned)
├── specs/          Shared type definitions
└── docs/           Architecture decisions and plans
```

## License

MIT

# motosan-ai

Multi-language, multi-provider AI SDK. One unified interface for Anthropic, OpenAI, MiniMax, Ollama — and more.

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
client = Client.anthropic()  # → Client.openai() — done
response = await client.chat([Message.user("Hello")])
```

## Languages

| Language | Package | Version |
|----------|---------|---------|
| Rust | [`motosan-ai`](https://crates.io/crates/motosan-ai) | v0.14.0 |
| Python | [`motosan-ai`](https://pypi.org/project/motosan-ai/) | v0.5.0 |

## Install

```toml
# Rust (Cargo.toml)
[dependencies]
motosan-ai = { version = "0.14.0", features = ["anthropic"] }
# features: anthropic | openai | minimax | ollama | ollama_native | full
#           gemini | gemini-code-assist | claude-code | codex-cli | gemini-cli
```

```bash
# Python
pip install "motosan-ai[anthropic]"
pip install "motosan-ai[full]"   # all providers
```

## Providers

| Provider | Default model | Rust feature | Python extra |
|----------|---------------|-------------|-------------|
| Anthropic | `claude-sonnet-4-6` | `anthropic` | `[anthropic]` |
| OpenAI | `gpt-5.3-codex` | `openai` | `[openai]` |
| MiniMax | `MiniMax-M2.7` | `minimax` | `[minimax]` |
| Ollama | `llama3.2` | `ollama` / `ollama_native` | `[ollama]` |
| Gemini | `gemini-2.0-flash` | `gemini` | — |
| Gemini Code Assist | `gemini-2.5-flash` | `gemini-code-assist` | — |
| Claude Code CLI | (CLI default) | `claude-code` | — |
| Codex CLI | (CLI default) | `codex-cli` | — |
| Gemini CLI | (CLI default) | `gemini-cli` | — |

> **OpenAI-compatible providers** (Groq, DeepSeek, Together, self-hosted proxies, etc.) work via the `openai` feature with a custom chat URL — pass the full endpoint you want POSTed:
>
> ```rust
> Client::builder()
>     .provider(Provider::OpenAI)
>     .api_key("...")
>     .openai_chat_url("https://api.groq.com/openai/v1/chat/completions")
>     .build()?;
> ```

## Backends (Rust)

`motosan-ai` supports five ways to run LLM turns, all returning the same `ChatResponse` / `StreamEvent` types:

```rust
use motosan_ai::{Client, Message, Provider};

// 1. API key — direct HTTP to Anthropic
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-api03-...")
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;

// 2. OAuth token — same Client, auto-detected from token prefix
let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key("sk-ant-oat01-...")   // OAuth format
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;
```

```rust
// 3. Claude Code CLI via unified Client::builder() (since v0.11.0)
// Requires: cargo add motosan-ai --features claude-code
// No api_key needed — CLI backends authenticate via local login state.
use motosan_ai::claude_code::{EffortLevel, PermissionMode};
use motosan_ai::ClaudeCodeProvider;

let client = Client::builder()
    .provider(Provider::ClaudeCode)
    .claude_code(
        ClaudeCodeProvider::new()
            .bare(true)                               // --bare (daemon-safe; skip hooks/plugins/auto-memory)
            .model("sonnet")
            .system_prompt("Be terse.")              // --system-prompt
            .permission_mode(PermissionMode::Plan)    // --permission-mode plan
            .effort(EffortLevel::Low)                 // --effort low
            .allow_tool("Edit")                       // --allowed-tools
            .max_budget_usd(2.5),                     // --max-budget-usd
    )
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;
```

```rust
// 4. Codex CLI via unified Client::builder() (since v0.11.0)
// Requires: cargo add motosan-ai --features codex-cli
use motosan_ai::codex_cli::SandboxMode;
use motosan_ai::{CodexCliProvider, Client, Provider};

let client = Client::builder()
    .provider(Provider::CodexCli)
    .codex_cli(
        CodexCliProvider::new()
            .sandbox(SandboxMode::WorkspaceWrite)
            .ephemeral(true),
    )
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;
```

```rust
// 5. Gemini CLI via unified Client::builder()
// Requires: cargo add motosan-ai --features gemini-cli
// No api_key needed — Gemini CLI uses local auth (`gemini auth` once).
use motosan_ai::gemini_cli::ApprovalMode;
use motosan_ai::{Client, GeminiCliProvider, Provider};

let client = Client::builder()
    .provider(Provider::GeminiCli)
    .gemini_cli(
        GeminiCliProvider::new()
            .model("gemini-2.5-pro")
            .approval_mode(ApprovalMode::Yolo),
    )
    .build()?;
let response = client.chat(vec![Message::user("Hello")]).await?;
```

> **CLI backend limitations (Claude Code / Codex CLI / Gemini CLI):** Tool calls run internally by the CLI and are **not** surfaced on `ChatResponse.tool_calls` (always empty). All three require the corresponding binary installed and authenticated. Enable with `--features claude-code`, `--features codex-cli`, or `--features gemini-cli`. You can also hold a raw `ClaudeCodeProvider` / `CodexCliProvider` / `GeminiCliProvider` directly if you don't need the `Client` abstraction.

## Features

- **Chat & Streaming** — `chat()`, `stream()`, `chat_with()`, `stream_with()`, `stream_collect()`
- **Unified dispatch** — a single `Client::builder()` handles HTTP and CLI backends alike; `Provider::ClaudeCode`, `Provider::CodexCli`, and `Provider::GeminiCli` are first-class variants (since v0.11.0)
- **Tool Use** — define tools, multi-turn tool loops, streaming tool calls
- **Vision** — send images alongside text (base64 or URL)
- **ThinkStripper** — auto-strips `<think>` reasoning blocks from streaming output
- **Retry** — configurable exponential backoff with jitter and `Retry-After` support
- **Stream Read Timeout** — configurable per-chunk timeout to prevent SSE hanging
- **Extended Thinking** — first-class support for Anthropic thinking mode
- **MCP** — server-side MCP support in `ChatRequest`
- **Claude Code Backend** — shell out to `claude` CLI via `ClaudeCodeProvider` with full flag coverage: `--model`, `--system-prompt`, `--permission-mode`, `--effort`, `--fallback-model`, `--add-dir`, `--allowed-tools` / `--disallowed-tools`, `--mcp-config` / `--strict-mcp-config`, `--settings` / `--setting-sources`, `--session-id` / `--resume` / `--continue` / `--fork-session` / `--no-session-persistence`, `--plugin-dir`, `--agent`, `--max-budget-usd` (`--features claude-code`)
- **Codex CLI Backend** — shell out to `codex exec --json` via `CodexCliProvider` with sandbox / profile / config-override support (`--features codex-cli`)
- **Gemini CLI Backend** — shell out to `gemini -p -o stream-json` via `GeminiCliProvider` with `--yolo` / `--sandbox` / `--approval-mode` support (`--features gemini-cli`)

## Quick Example

```rust
use motosan_ai::{Client, Message, Provider};
use tokio_stream::StreamExt;

let client = Client::builder()
    .provider(Provider::Anthropic)
    .api_key(std::env::var("ANTHROPIC_API_KEY")?)
    .stream_read_timeout_secs(30)
    .build()?;

let mut stream = client.stream(vec![Message::user("Hello")]).await?;
while let Some(event) = stream.next().await {
    if event.done {
        // Terminal event carries the provider-reported stop reason when available.
        // Streams emit exactly one `done` event, even on non-conformant proxies.
        if let Some(reason) = event.stop_reason {
            eprintln!("\n[done: {reason:?}]");
        }
        break;
    }
    print!("{}", event.content);
}
```

```python
from motosan_ai import Client, Message

client = Client.anthropic()
async for event in client.stream([Message.user("Hello")]):
    if event.done:
        break
    print(event.content, end="", flush=True)
```

## Development

```bash
# Optional: Nix + direnv for a fully reproducible environment
# cd into the project — direnv auto-activates nix develop

fmt           # Format everything (Rust + Python + TOML + Nix)
check-all     # Full CI gate (lint + test both SDKs)
test-live     # Anthropic integration tests
```

See [`AGENTS.md`](AGENTS.md) for full development guide.

## For AI Agents

Fetch [`llms.txt`](https://raw.githubusercontent.com/motosan-dev/motosan-ai/main/llms.txt) for a quick-start API reference.

## License

MIT

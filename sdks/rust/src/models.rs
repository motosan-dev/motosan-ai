pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5.3-codex";
pub const DEFAULT_MINIMAX_MODEL: &str = "MiniMax-M2.5-highspeed";

pub const ANTHROPIC_MODELS: &[&str] = &[
    "claude-opus-4-6",
    "claude-sonnet-4-6",
    "claude-haiku-4-5-20251001",
    "claude-sonnet-4-5-20250929",
    "claude-opus-4-5-20251101",
    "claude-opus-4-1-20250805",
    "claude-sonnet-4-20250514",
    "claude-opus-4-20250514",
    "claude-3-haiku-20240307",
];

pub const OPENAI_MODELS: &[&str] = &["gpt-5.3-codex", "gpt-4o"];

pub const MINIMAX_MODELS: &[&str] = &[
    "MiniMax-M2.5-highspeed",
    "MiniMax-M2.5",
    "MiniMax-M2.1-highspeed",
    "MiniMax-M2.1",
    "MiniMax-M2",
    "MiniMax-Text-01",
];

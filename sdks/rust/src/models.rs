pub const DEFAULT_ANTHROPIC_MODEL: &str = "claude-sonnet-4-6";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5.3-codex";
pub const DEFAULT_OLLAMA_MODEL: &str = "llama3.2";

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

pub const MINIMAX_MODELS: &[&str] = &["MiniMax-M2.7", "MiniMax-M2.7-highspeed"];

pub const DEFAULT_GEMINI_MODEL: &str = "gemini-2.5-flash";

pub const GEMINI_MODELS: &[&str] = &[
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-pro",
    "gemini-flash-latest",
    "gemini-2.0-flash",
    "gemini-2.0-flash-lite",
    "gemini-1.5-pro",
    "gemini-1.5-flash",
];

pub const DEFAULT_GEMINI_CODE_ASSIST_MODEL: &str = "gemini-2.5-flash";
pub const GEMINI_CODE_ASSIST_BASE_URL: &str = "https://cloudcode-pa.googleapis.com";

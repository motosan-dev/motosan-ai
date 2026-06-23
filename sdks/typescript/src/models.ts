/** Anthropic model IDs (models.rs:5-16) */
export const ANTHROPIC_MODELS = [
  'claude-opus-4-8',
  'claude-opus-4-6',
  'claude-sonnet-4-6',
  'claude-haiku-4-5-20251001',
  'claude-sonnet-4-5-20250929',
  'claude-opus-4-5-20251101',
  'claude-opus-4-1-20250805',
  'claude-sonnet-4-20250514',
  'claude-opus-4-20250514',
  'claude-3-haiku-20240307',
] as const

/** Default Anthropic model (models.rs:1) */
export const DEFAULT_ANTHROPIC_MODEL = 'claude-sonnet-4-6'

/** OpenAI model IDs (models.rs:18) */
export const OPENAI_MODELS = ['gpt-5.3-codex', 'gpt-4o'] as const

/** Default OpenAI model (models.rs:2) */
export const DEFAULT_OPENAI_MODEL = 'gpt-5.3-codex'

/** MiniMax model IDs (models.rs:20) */
export const MINIMAX_MODELS = ['MiniMax-M2.7', 'MiniMax-M2.7-highspeed'] as const

/** Default MiniMax model (models.rs convention, first element) */
export const DEFAULT_MINIMAX_MODEL = 'MiniMax-M2.7'

/** Default Ollama model (models.rs:3) */
export const DEFAULT_OLLAMA_MODEL = 'llama3.2'

/** Gemini model IDs (models.rs:24-33) */
export const GEMINI_MODELS = [
  'gemini-2.5-flash',
  'gemini-2.5-flash-lite',
  'gemini-2.5-pro',
  'gemini-flash-latest',
  'gemini-2.0-flash',
  'gemini-2.0-flash-lite',
  'gemini-1.5-pro',
  'gemini-1.5-flash',
] as const

/** Default Gemini model (models.rs:22) */
export const DEFAULT_GEMINI_MODEL = 'gemini-2.5-flash'

/** ChatGPT-Codex model IDs */
export const CHATGPT_CODEX_MODELS = ['gpt-5.5'] as const

/** Default ChatGPT-Codex model (distinct from DEFAULT_OPENAI_MODEL = 'gpt-5.3-codex') */
export const DEFAULT_CHATGPT_CODEX_MODEL = 'gpt-5.5'

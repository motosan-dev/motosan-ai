import { describe, expect, it } from 'vitest'
import {
  ANTHROPIC_MODELS,
  DEFAULT_ANTHROPIC_MODEL,
  OPENAI_MODELS,
  DEFAULT_OPENAI_MODEL,
  MINIMAX_MODELS,
  DEFAULT_MINIMAX_MODEL,
} from '../src/models.js'
import {
  DEFAULT_ANTHROPIC_MODEL as INDEX_DEFAULT_ANTHROPIC_MODEL,
  DEFAULT_OPENAI_MODEL as INDEX_DEFAULT_OPENAI_MODEL,
  DEFAULT_MINIMAX_MODEL as INDEX_DEFAULT_MINIMAX_MODEL,
} from '../src/index.js'
import { AnthropicProvider } from '../src/providers/anthropic.js'
import { MinimaxProvider } from '../src/providers/minimax.js'
import { OpenAIProvider } from '../src/providers/openai.js'

describe('models', () => {
  describe('ANTHROPIC_MODELS', () => {
    it('should be a non-empty array', () => {
      expect(Array.isArray(ANTHROPIC_MODELS)).toBe(true)
      expect(ANTHROPIC_MODELS.length).toBeGreaterThan(0)
    })

    it('should contain the default model', () => {
      expect(ANTHROPIC_MODELS).toContain(DEFAULT_ANTHROPIC_MODEL)
    })

    it('should have unique model IDs', () => {
      const ids = new Set(ANTHROPIC_MODELS)
      expect(ids.size).toBe(ANTHROPIC_MODELS.length)
    })
  })

  describe('DEFAULT_ANTHROPIC_MODEL', () => {
    it('should be a non-empty string', () => {
      expect(typeof DEFAULT_ANTHROPIC_MODEL).toBe('string')
      expect(DEFAULT_ANTHROPIC_MODEL.length).toBeGreaterThan(0)
    })

    it('should match the value from models.rs', () => {
      expect(DEFAULT_ANTHROPIC_MODEL).toBe('claude-sonnet-4-6')
    })
  })

  describe('OPENAI_MODELS', () => {
    it('should be a non-empty array', () => {
      expect(Array.isArray(OPENAI_MODELS)).toBe(true)
      expect(OPENAI_MODELS.length).toBeGreaterThan(0)
    })

    it('should contain the default model', () => {
      expect(OPENAI_MODELS).toContain(DEFAULT_OPENAI_MODEL)
    })

    it('should have unique model IDs', () => {
      const ids = new Set(OPENAI_MODELS)
      expect(ids.size).toBe(OPENAI_MODELS.length)
    })
  })

  describe('DEFAULT_OPENAI_MODEL', () => {
    it('should be a non-empty string', () => {
      expect(typeof DEFAULT_OPENAI_MODEL).toBe('string')
      expect(DEFAULT_OPENAI_MODEL.length).toBeGreaterThan(0)
    })

    it('should match the value from models.rs', () => {
      expect(DEFAULT_OPENAI_MODEL).toBe('gpt-5.3-codex')
    })
  })

  describe('MINIMAX_MODELS', () => {
    it('should be a non-empty array', () => {
      expect(Array.isArray(MINIMAX_MODELS)).toBe(true)
      expect(MINIMAX_MODELS.length).toBeGreaterThan(0)
    })

    it('should contain the default model', () => {
      expect(MINIMAX_MODELS).toContain(DEFAULT_MINIMAX_MODEL)
    })

    it('should have unique model IDs', () => {
      const ids = new Set(MINIMAX_MODELS)
      expect(ids.size).toBe(MINIMAX_MODELS.length)
    })
  })

  describe('DEFAULT_MINIMAX_MODEL', () => {
    it('should be a non-empty string', () => {
      expect(typeof DEFAULT_MINIMAX_MODEL).toBe('string')
      expect(DEFAULT_MINIMAX_MODEL.length).toBeGreaterThan(0)
    })

    it('should match the value from models.rs', () => {
      expect(DEFAULT_MINIMAX_MODEL).toBe('MiniMax-M2.7')
    })

    it('should be the first element of MINIMAX_MODELS', () => {
      expect(DEFAULT_MINIMAX_MODEL).toBe(MINIMAX_MODELS[0])
    })
  })

  describe('cross-provider consistency', () => {
    it('should have defaults that are reasonable strings', () => {
      expect(DEFAULT_ANTHROPIC_MODEL).toMatch(/^claude-/)
      expect(DEFAULT_OPENAI_MODEL).toMatch(/^gpt-/)
      expect(DEFAULT_MINIMAX_MODEL).toMatch(/^MiniMax-/)
    })
  })

  describe('index exports', () => {
    it('should re-export provider default constants', () => {
      expect(INDEX_DEFAULT_ANTHROPIC_MODEL).toBe(DEFAULT_ANTHROPIC_MODEL)
      expect(INDEX_DEFAULT_OPENAI_MODEL).toBe(DEFAULT_OPENAI_MODEL)
      expect(INDEX_DEFAULT_MINIMAX_MODEL).toBe(DEFAULT_MINIMAX_MODEL)
    })
  })

  describe('provider defaults integration', () => {
    it('AnthropicProvider should use DEFAULT_ANTHROPIC_MODEL', () => {
      const provider = new AnthropicProvider('test-api-key')
      expect((provider as any).model).toBe(DEFAULT_ANTHROPIC_MODEL)
    })

    it('OpenAIProvider should use DEFAULT_OPENAI_MODEL', () => {
      const provider = new OpenAIProvider('test-api-key')
      expect((provider as any).model).toBe(DEFAULT_OPENAI_MODEL)
    })

    it('MinimaxProvider should use DEFAULT_MINIMAX_MODEL', () => {
      const provider = new MinimaxProvider('test-api-key')
      expect((provider as any).inner.model).toBe(DEFAULT_MINIMAX_MODEL)
    })
  })
})

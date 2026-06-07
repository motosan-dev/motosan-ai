import type { ChatRequest, ContentBlock } from '../types.js'

/**
 * Gemini generateContent / streamGenerateContent request serializer.
 *
 * Mirrors Rust `GeminiProvider::build_request` (gemini.rs:80-237). Projects the
 * provider-agnostic ChatRequest onto the Gemini REST wire, which diverges from
 * OpenAI/Anthropic in several load-bearing ways (contract §3):
 *
 *   1. The MODEL is NOT in the body — it lives in the URL path. The `model`
 *      param is accepted for signature symmetry with serializeOpenAiRequest but
 *      is never written to the returned object.
 *   2. systemInstruction is a SEPARATE top-level field (NOT a role:system
 *      message in `contents`). (gemini.rs:151-192)
 *   3. Assistant role serializes to `'model'` (gemini.rs:120,136).
 *   4. Tool calls use the WIRE field `args` (functionCall.args) even though the
 *      SDK type field is `input`. (gemini.rs:129) Images use `inlineData`
 *      (camelCase, key `mimeType`) / `fileData.fileUri`. Tool declarations use
 *      `parameters` (NOT `input_schema`). (gemini.rs:99-110,196-207)
 *   5. Tool messages become a `role:'user'` part with `functionResponse`, and
 *      `toolCallId` carries the FUNCTION NAME by this SDK's convention.
 *      (gemini.rs:138-147)
 *
 * Capability validation (provider.ts validateRequest) rejects document blocks
 * BEFORE serialization, so the document branch throws defensively (mirrors
 * serialize/openai.ts:58 and Rust's `unreachable!()` at gemini.rs:112).
 */

const DEFAULT_MAX_TOKENS = 8192

function serializeUserPart(block: ContentBlock): Record<string, unknown> {
  if (block.type === 'text') {
    return { text: block.text }
  }
  if (block.type === 'image') {
    const source = block.source
    if (source.type === 'base64') {
      // Field is inlineData (camelCase); key mimeType (NOT media_type),
      // maps from TS source.mediaType. (gemini.rs:99-107)
      return { inlineData: { mimeType: source.mediaType, data: source.data } }
    }
    // source.type === 'url' (gemini.rs:108-110)
    return { fileData: { fileUri: source.url } }
  }
  // Document blocks are not supported by Gemini; capability validation rejects
  // them before serialization (gemini.rs:112 `unreachable!()`).
  throw new Error('Gemini does not support document content blocks')
}

// `_model` is accepted for signature symmetry with the other serializers but
// is unused here — Gemini puts the model in the URL path, not the body.
export function serializeGeminiRequest(
  req: ChatRequest,
  _model: string,
): Record<string, unknown> {
  const contents: Record<string, unknown>[] = []
  let extractedSystem: string | undefined

  for (const message of req.messages) {
    switch (message.role) {
      case 'system': {
        // NOT pushed to contents; captured for systemInstruction fallback
        // (last system message wins). (gemini.rs:86-88)
        extractedSystem = message.content
        break
      }
      case 'user': {
        const parts: Record<string, unknown>[] = []
        if (message.content !== '') {
          parts.push({ text: message.content }) // (gemini.rs:91-93)
        }
        for (const block of message.contentBlocks ?? []) {
          parts.push(serializeUserPart(block)) // (gemini.rs:94-114)
        }
        if (parts.length === 0) {
          parts.push({ text: '' }) // (gemini.rs:115-117)
        }
        contents.push({ role: 'user', parts })
        break
      }
      case 'assistant': {
        const parts: Record<string, unknown>[] = []
        if (message.content !== '') {
          parts.push({ text: message.content }) // (gemini.rs:122-124)
        }
        for (const tc of message.toolCalls ?? []) {
          // Wire uses `args` for functionCall input; SDK type field is `input`.
          // (gemini.rs:125-132)
          parts.push({ functionCall: { name: tc.name, args: tc.input } })
        }
        if (parts.length === 0) {
          parts.push({ text: '' }) // (gemini.rs:133-135)
        }
        // Assistant role serializes to 'model'. (gemini.rs:136)
        contents.push({ role: 'model', parts })
        break
      }
      case 'tool': {
        // toolCallId holds the function name by this SDK's convention.
        // (gemini.rs:139-140)
        const name = message.toolCallId ?? ''
        let response: unknown
        try {
          response = JSON.parse(message.content) // (gemini.rs:141)
        } catch {
          response = { result: message.content } // (gemini.rs:142)
        }
        contents.push({
          role: 'user',
          parts: [{ functionResponse: { name, response } }],
        })
        break
      }
    }
  }

  // systemInstruction resolution priority (gemini.rs:151-172):
  // 1. systemBlocks joined with '\n' (when present AND non-empty)
  // 2. else req.system ?? extractedSystem ?? ''
  let systemText: string
  if (req.systemBlocks !== undefined) {
    const joined = req.systemBlocks.map((b) => b.text).join('\n')
    systemText = joined !== '' ? joined : req.system ?? extractedSystem ?? ''
  } else {
    systemText = req.system ?? extractedSystem ?? ''
  }

  // generationConfig is ALWAYS present. (gemini.rs:174-188)
  const generationConfig: Record<string, unknown> = {
    maxOutputTokens: req.maxTokens ?? DEFAULT_MAX_TOKENS,
  }
  if (req.temperature !== undefined) {
    generationConfig.temperature = req.temperature // (gemini.rs:176-178)
  }
  if (req.stopSequences && req.stopSequences.length > 0) {
    generationConfig.stopSequences = req.stopSequences // (gemini.rs:179-183)
  }

  const body: Record<string, unknown> = {
    contents,
    generationConfig,
  }

  // systemInstruction: SEPARATE top-level field, emitted only when non-empty.
  // (gemini.rs:190-192)
  if (systemText !== '') {
    body.systemInstruction = { parts: [{ text: systemText }] }
  }

  // tools + toolConfig only when req.tools is non-empty. (gemini.rs:194-195)
  if (req.tools && req.tools.length > 0) {
    const declarations = req.tools.map((tool) => ({
      name: tool.name,
      description: tool.description ?? '',
      parameters: tool.inputSchema ?? { type: 'object', properties: {} },
    }))
    body.tools = [{ functionDeclarations: declarations }]

    // tool_choice -> toolConfig.functionCallingConfig.mode. (gemini.rs:209-224)
    let mode: 'AUTO' | 'ANY' | 'NONE'
    const choice = req.toolChoice
    if (choice === undefined || choice.type === 'auto') {
      mode = 'AUTO'
    } else if (choice.type === 'required') {
      mode = 'ANY'
    } else if (choice.type === 'none') {
      // NONE: remove tools, emit NO toolConfig. (gemini.rs:212-215,218)
      delete body.tools
      mode = 'NONE'
    } else {
      // choice.type === 'tool'
      mode = 'ANY'
    }

    if (mode !== 'NONE') {
      const fcConfig: Record<string, unknown> = { mode }
      if (choice && choice.type === 'tool') {
        fcConfig.allowedFunctionNames = [choice.name] // (gemini.rs:220-222)
      }
      body.toolConfig = { functionCallingConfig: fcConfig }
    }
  }

  // providerOptions merge LAST (top-level). (gemini.rs:228-234,
  // serialize/openai.ts:181-183)
  if (req.providerOptions && typeof req.providerOptions === 'object') {
    Object.assign(body, req.providerOptions)
  }

  return body
}

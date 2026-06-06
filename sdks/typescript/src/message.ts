import type { ContentBlock, Message, ToolCall } from './types.js'

/**
 * Create a user message with plain text content.
 */
export function user(content: string): Message {
  return {
    role: 'user',
    content,
  }
}

/**
 * Create a user message marked as cacheable (Anthropic prompt caching).
 * Non-Anthropic providers silently ignore the cache flag.
 */
export function userWithCache(content: string): Message {
  return {
    role: 'user',
    content,
    cache: true,
  }
}

/**
 * Create an assistant message with plain text content.
 */
export function assistant(content: string): Message {
  return {
    role: 'assistant',
    content,
  }
}

/**
 * Create an assistant message with tool calls.
 */
export function assistantWithToolCalls(content: string, toolCalls: ToolCall[]): Message {
  return {
    role: 'assistant',
    content,
    toolCalls,
  }
}

/**
 * Create a system message.
 */
export function system(content: string): Message {
  return {
    role: 'system',
    content,
  }
}

/**
 * Create a tool message. Alias for toolResult.
 */
export function tool(content: string, toolCallId: string): Message {
  return toolResult(toolCallId, content)
}

/**
 * Create a tool message for returning results to a tool call.
 */
export function toolResult(toolCallId: string, content: string): Message {
  return {
    role: 'tool',
    content,
    toolCallId,
  }
}

/**
 * Create a user message with text and an image.
 *
 * @param text - The text content accompanying the image.
 * @param base64Data - Base64-encoded image data.
 * @param mediaType - The MIME type of the image (e.g., 'image/png', 'image/jpeg').
 */
export function userWithImage(text: string, base64Data: string, mediaType: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'image',
        source: {
          type: 'base64',
          mediaType,
          data: base64Data,
        },
      },
    ],
  }
}

/**
 * Create a user message with multiple content blocks.
 * The content field (for backward compatibility) is extracted from the first text block.
 */
export function userWithBlocks(blocks: ContentBlock[]): Message {
  const firstTextBlock = blocks.find((block) => block.type === 'text')
  const content = firstTextBlock?.type === 'text' ? firstTextBlock.text : ''

  return {
    role: 'user',
    content,
    contentBlocks: blocks,
  }
}

/**
 * Create a user message with a PDF document from base64-encoded data.
 *
 * @param text - The text content accompanying the PDF.
 * @param base64Data - Base64-encoded PDF data.
 */
export function userWithPdfBase64(text: string, base64Data: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'document',
        source: {
          type: 'base64',
          mediaType: 'application/pdf',
          data: base64Data,
        },
      },
    ],
  }
}

/**
 * Create a user message with a PDF document from a URL.
 *
 * @param text - The text content accompanying the PDF.
 * @param url - The URL of the PDF document.
 */
export function userWithPdfUrl(text: string, url: string): Message {
  return {
    role: 'user',
    content: text,
    contentBlocks: [
      { type: 'text', text },
      {
        type: 'document',
        source: {
          type: 'url',
          url,
        },
      },
    ],
  }
}

/**
 * Create a user message with a PDF document from raw bytes.
 * The bytes are automatically base64-encoded.
 *
 * @param text - The text content accompanying the PDF.
 * @param bytes - Raw PDF bytes.
 */
export function userWithPdfBytes(text: string, bytes: Uint8Array): Message {
  const base64Data = Buffer.from(bytes).toString('base64')
  return userWithPdfBase64(text, base64Data)
}

/**
 * Mark a message as cacheable by returning a copy with cache flag set to true.
 * Non-Anthropic providers silently ignore the cache flag.
 */
export function withCache(message: Message): Message {
  return {
    ...message,
    cache: true,
  }
}

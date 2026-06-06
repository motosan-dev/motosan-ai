/**
 * Provider capabilities and validation.
 *
 * Mirrors Rust `providers/mod.rs:76-96` (validate_request) and
 * `types.rs:903-930` (ProviderCapabilities).
 */

import { UnsupportedFeatureError } from './error.js'
import type { ChatRequest, ContentBlock } from './types.js'

/**
 * Describes what features a provider supports.
 *
 * Mirrors Rust `ProviderCapabilities` (types.rs:903-907).
 */
export interface ProviderCapabilities {
  supportsImage: boolean
  supportsDocument: boolean
}

/**
 * Provider with text only — no images, no documents.
 *
 * Mirrors Rust `ProviderCapabilities::text_only()` (types.rs:910-915).
 */
export function textOnly(): ProviderCapabilities {
  return { supportsImage: false, supportsDocument: false }
}

/**
 * Provider with image support — images but no documents.
 *
 * Mirrors Rust `ProviderCapabilities::with_image()` (types.rs:917-922).
 */
export function withImage(): ProviderCapabilities {
  return { supportsImage: true, supportsDocument: false }
}

/**
 * Provider with full support — images and documents.
 *
 * Mirrors Rust `ProviderCapabilities::full()` (types.rs:924-929).
 */
export function fullCaps(): ProviderCapabilities {
  return { supportsImage: true, supportsDocument: true }
}

/**
 * Validate a chat request against provider capabilities.
 *
 * Iterates request.messages[].contentBlocks and throws UnsupportedFeatureError if:
 * - Content block is an image and !caps.supportsImage
 * - Content block is a document and !caps.supportsDocument
 *
 * Mirrors Rust Provider::validate_request (providers/mod.rs:76-96).
 * Throws BEFORE any HTTP call.
 */
export function validateRequest(
  req: ChatRequest,
  caps: ProviderCapabilities
): void {
  for (const msg of req.messages) {
    const blocks: ContentBlock[] = msg.contentBlocks ?? []
    for (const block of blocks) {
      if (block.type === 'image' && !caps.supportsImage) {
        throw new UnsupportedFeatureError(
          'provider does not support image input'
        )
      }
      if (block.type === 'document' && !caps.supportsDocument) {
        throw new UnsupportedFeatureError(
          'provider does not support document input'
        )
      }
    }
  }
}

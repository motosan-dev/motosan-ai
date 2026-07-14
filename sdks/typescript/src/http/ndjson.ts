/**
 * Newline-Delimited JSON (NDJSON) streaming parser.
 *
 * Parses a ReadableStream<Uint8Array> into an async generator of JSON objects.
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete lines, splits on \n boundaries, and JSON-parses each non-empty line.
 * Malformed JSON lines are silently skipped.
 *
 * Finalized in M5: verified sufficient for Ollama native /api/chat NDJSON.
 * The parser is provider-agnostic — it yields each parsed object (including
 * any {done:true} object) and never inspects `done`. Termination on
 * {done:true} is the Ollama stream adapter's responsibility (providers/ollama.ts),
 * mirroring Rust where NdjsonStream only splits lines (ollama.rs:500-551) and
 * OllamaStreamAdapter decides done (ollama.rs:441-447).
 */

/**
 * Parse a ReadableStream<Uint8Array> as newline-delimited JSON.
 *
 * Yields parsed JSON objects (any type). Malformed JSON lines are silently
 * skipped. Empty lines are ignored.
 */
export async function* parseNdjson(
  body: ReadableStream<Uint8Array>
): AsyncGenerator<any> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''

  try {
    while (true) {
      const { done, value } = await reader.read()

      if (value) {
        buffer += decoder.decode(value, { stream: true })
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += decoder.decode()
        // Process final line if buffer is non-empty
        if (buffer.trim()) {
          const obj = parseJsonLine(buffer.trim())
          if (obj !== undefined) {
            yield obj
          }
        }
        break
      }

      // Process complete lines (split by \n)
      while (true) {
        const newlineIndex = buffer.indexOf('\n')
        if (newlineIndex === -1) {
          break // Wait for more data
        }

        const line = buffer.substring(0, newlineIndex)
        buffer = buffer.substring(newlineIndex + 1)

        const obj = parseJsonLine(line.trim())
        if (obj !== undefined) {
          yield obj
        }
      }
    }
  } finally {
    try {
      await reader.cancel()
    } catch {
      // Ignore cancel errors — the stream may already be closed or errored.
    }
    reader.releaseLock()
  }
}

/**
 * Parse a single line as JSON.
 * Returns undefined if line is empty or JSON parsing fails (malformed).
 */
function parseJsonLine(line: string): any {
  if (!line) {
    return undefined
  }

  try {
    return JSON.parse(line)
  } catch {
    return undefined // Skip malformed JSON
  }
}

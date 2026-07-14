/**
 * Server-Sent Events (SSE) streaming parser.
 *
 * Parses a ReadableStream<Uint8Array> into an async generator of SSE events.
 * Uses TextDecoder to handle UTF-8 decoding across chunk boundaries, buffers
 * incomplete events, normalizes \r\n / bare \r to \n, splits on blank-line
 * boundaries, and parses event:/data: lines (at most one leading space is
 * removed after the colon, per the WHATWG SSE spec).
 * Malformed JSON in data fields is silently skipped. [DONE] is recognized as
 * data but does NOT terminate the stream (completion is adapter-specific).
 */

export interface SseEvent {
  event?: string
  data: any
}

interface ParsedEventText {
  event?: string
  data?: string
  hasData: boolean
}

/**
 * Parse a ReadableStream<Uint8Array> as Server-Sent Events.
 *
 * Yields SseEvent objects. Each event consists of optional 'event' field
 * and a 'data' field (JSON-parsed or string '[DONE]'). Malformed JSON is
 * skipped; [DONE] is recognized but does not terminate parsing.
 */
export async function* parseSse(
  body: ReadableStream<Uint8Array>
): AsyncGenerator<SseEvent> {
  const reader = body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let pendingCr = false
  let pendingEventName: string | undefined

  // Normalize \r\n and bare \r line terminators to \n (WHATWG SSE spec).
  // A chunk ending in \r is held back (pendingCr) so a \r\n pair split
  // across chunk boundaries is not mistaken for two terminators.
  function normalizeNewlines(text: string): string {
    if (pendingCr) {
      text = '\r' + text
      pendingCr = false
    }
    if (text.endsWith('\r')) {
      pendingCr = true
      text = text.substring(0, text.length - 1)
    }
    return text.replace(/\r\n/g, '\n').replace(/\r/g, '\n')
  }

  function eventFromText(text: string): SseEvent | null {
    const parsed = parseEventText(text)
    const event = parsed.event ?? pendingEventName

    if (!parsed.hasData) {
      if (parsed.event) {
        pendingEventName = parsed.event
      }
      return null
    }

    pendingEventName = undefined
    return parseData(parsed.data ?? '', event)
  }

  try {
    while (true) {
      const { done, value } = await reader.read()

      if (value) {
        buffer += normalizeNewlines(decoder.decode(value, { stream: true }))
      }

      if (done) {
        // Flush any remaining decoded bytes
        buffer += normalizeNewlines(decoder.decode())
        if (pendingCr) {
          // A trailing lone \r is a line terminator too
          buffer += '\n'
          pendingCr = false
        }
        // Process final event if buffer is non-empty
        if (buffer.trim()) {
          const event = eventFromText(buffer)
          if (event) {
            yield event
          }
        }
        break
      }

      // Process complete events (split by \n\n)
      while (true) {
        const eventEndIndex = buffer.indexOf('\n\n')
        if (eventEndIndex === -1) {
          break // Wait for more data
        }

        const eventText = buffer.substring(0, eventEndIndex)
        buffer = buffer.substring(eventEndIndex + 2)

        const event = eventFromText(eventText)
        if (event) {
          yield event
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
 * Parse a single event text block (lines before \n\n).
 * Extracts event: and data: fields.
 */
function parseEventText(text: string): ParsedEventText {
  const lines = text.split('\n')
  let eventName: string | undefined
  const dataLines: string[] = []

  for (const line of lines) {
    if (!line || line.startsWith(':')) {
      continue // empty line or comment
    }

    const colonIndex = line.indexOf(':')
    const field = colonIndex === -1 ? line : line.substring(0, colonIndex)
    let value = colonIndex === -1 ? '' : line.substring(colonIndex + 1)
    // WHATWG SSE spec: remove at most ONE leading space after the colon;
    // the rest of the value is preserved verbatim.
    if (value.startsWith(' ')) {
      value = value.substring(1)
    }

    if (field === 'event') {
      eventName = value
    } else if (field === 'data') {
      dataLines.push(value)
    }
  }

  return {
    event: eventName,
    data: dataLines.join('\n'),
    hasData: dataLines.length > 0
  }
}

/**
 * Parse an event data field.
 * Returns null if parsing fails (e.g., no data field, malformed JSON).
 */
function parseData(dataStr: string, eventName: string | undefined): SseEvent | null {
  // Must have at least a data field
  if (dataStr === '') {
    return null
  }

  // Special case: [DONE] is passed through as a string, not parsed as JSON
  if (dataStr === '[DONE]') {
    return {
      event: eventName,
      data: '[DONE]'
    }
  }

  // Try to parse data as JSON; skip malformed silently
  let parsedData: any
  try {
    parsedData = JSON.parse(dataStr)
  } catch {
    return null // Skip malformed JSON
  }

  return {
    event: eventName,
    data: parsedData
  }
}

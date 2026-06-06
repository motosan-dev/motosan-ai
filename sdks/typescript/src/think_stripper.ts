import type { BoxStream } from './stream.js'
import { textEvent } from './stream.js'

const OPEN_TAG = '<think>'
const CLOSE_TAG = '</think>'
const OPEN_TAG_TAIL_LEN = OPEN_TAG.length - 1
const CLOSE_TAG_TAIL_LEN = CLOSE_TAG.length - 1

/**
 * Stateful filter that buffers and removes `<think>...</think>` tags across
 * streaming chunks.
 *
 * Buffer-tail lengths are pinned to the tag length minus one so split tags at
 * chunk boundaries remain detectable:
 * - outside think blocks: keep `<think>`.length - 1 = 6 code units
 * - inside think blocks: keep `</think>`.length - 1 = 7 code units
 */
export class ThinkStripper {
  private buf = ''
  private inThink = false

  /**
   * Feed a chunk of text and return non-think text that is safe to emit now.
   */
  feed(chunk: string): string {
    this.buf += chunk
    let output = ''

    while (true) {
      if (this.inThink) {
        const closePos = this.buf.indexOf(CLOSE_TAG)
        if (closePos === -1) {
          if (this.buf.length > CLOSE_TAG_TAIL_LEN) {
            this.buf = this.buf.slice(this.buf.length - CLOSE_TAG_TAIL_LEN)
          }
          break
        }

        this.buf = this.buf.slice(closePos + CLOSE_TAG.length)
        this.inThink = false
        continue
      }

      const openPos = this.buf.indexOf(OPEN_TAG)
      if (openPos === -1) {
        let safe = Math.max(0, this.buf.length - OPEN_TAG_TAIL_LEN)
        while (safe > 0 && !this.isCharBoundary(safe)) {
          safe -= 1
        }

        if (safe > 0) {
          output += this.buf.slice(0, safe)
          this.buf = this.buf.slice(safe)
        }
        break
      }

      output += this.buf.slice(0, openPos)
      this.buf = this.buf.slice(openPos + OPEN_TAG.length)
      this.inThink = true
    }

    return output
  }

  /**
   * Flush any buffered tail. If a think block was never closed, its buffered
   * hidden content is discarded and the state is reset.
   */
  flush(): string {
    const remaining = this.buf
    this.buf = ''

    if (this.inThink) {
      this.inThink = false
      return ''
    }

    return remaining
  }

  private isCharBoundary(pos: number): boolean {
    if (pos <= 0 || pos >= this.buf.length) {
      return true
    }

    const previous = this.buf.charCodeAt(pos - 1)
    const current = this.buf.charCodeAt(pos)
    const previousIsHighSurrogate = previous >= 0xd800 && previous <= 0xdbff
    const currentIsLowSurrogate = current >= 0xdc00 && current <= 0xdfff

    return !(previousIsHighSurrogate && currentIsLowSurrogate)
  }
}

/**
 * Wrap a stream and strip `<think>...</think>` tags from text events.
 * Buffered non-think tail text is emitted before a done event; non-text events
 * pass through unchanged.
 */
export function stripThink(inner: BoxStream): BoxStream {
  return (async function* () {
    const stripper = new ThinkStripper()

    for await (const event of inner) {
      if (event.done) {
        const tail = stripper.flush()
        if (tail.length > 0) {
          yield textEvent(tail)
        }
        yield event
        continue
      }

      if (event.eventType === 'text' && event.content.length > 0) {
        const stripped = stripper.feed(event.content)
        if (stripped.length > 0) {
          yield textEvent(stripped)
        }
        continue
      }

      yield event
    }

    const tail = stripper.flush()
    if (tail.length > 0) {
      yield textEvent(tail)
    }
  })()
}

import type {
  ChatResponse,
  ModelChatResponse,
  ModelStreamDelta,
  ModelToolCall,
  StopReason,
  StreamEvent,
  ToolCall,
  Usage,
} from './types.js'

export type BoxStream = AsyncIterable<StreamEvent>

export function textEvent(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'text',
  }
}

export function doneEvent(): StreamEvent {
  return {
    content: '',
    done: true,
    eventType: 'text',
  }
}

export function doneWithStopReason(stopReason: StopReason): StreamEvent {
  return {
    content: '',
    done: true,
    eventType: 'text',
    stopReason,
  }
}

export function usageEvent(usage: Usage): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'usage',
    usage,
  }
}

export function toolCallStart(id: string, name: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_start',
    toolCallId: id,
    toolCallName: name,
  }
}

export function toolCallArgs(delta: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_args',
    toolCallArgsDelta: delta,
  }
}

export function toolCallArgsWithId(id: string, delta: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_args',
    toolCallId: id,
    toolCallArgsDelta: delta,
  }
}

export function toolCallEnd(): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_end',
  }
}

export function toolCallEndWithId(id: string): StreamEvent {
  return {
    content: '',
    done: false,
    eventType: 'tool_call_end',
    toolCallId: id,
  }
}

export function thinkingDelta(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'thinking_delta',
  }
}

export function thinkingDone(content: string): StreamEvent {
  return {
    content,
    done: false,
    eventType: 'thinking_done',
  }
}

export async function collectStream(stream: BoxStream): Promise<ChatResponse> {
  let content = ''
  const toolCalls: ToolCall[] = []
  let currentToolId = ''
  let currentToolName = ''
  let currentToolArgs = ''
  let inputTokens = 0
  let outputTokens = 0
  let cacheCreationInputTokens: number | undefined
  let cacheReadInputTokens: number | undefined
  let explicitStopReason: StopReason | undefined
  let thinkingDeltaBuf = ''
  let thinkingDoneBuf: string | undefined

  for await (const event of stream) {
    if (event.done) {
      if (event.stopReason) {
        explicitStopReason = event.stopReason
      }
      break
    }

    switch (event.eventType) {
      case 'text':
        content += event.content
        break

      case 'usage':
        if (event.usage) {
          // Replace-with-fallback, mirroring Python _stream_collect.py:
          // Anthropic message_delta usage is CUMULATIVE, so summing would
          // double-count output tokens. A zero field keeps the previous
          // value (message_delta carries no input_tokens, mapped to 0);
          // an absent cache field keeps the previous cache value.
          inputTokens = event.usage.inputTokens || inputTokens
          outputTokens = event.usage.outputTokens || outputTokens
          if (event.usage.cacheCreationInputTokens !== undefined) {
            cacheCreationInputTokens = event.usage.cacheCreationInputTokens
          }
          if (event.usage.cacheReadInputTokens !== undefined) {
            cacheReadInputTokens = event.usage.cacheReadInputTokens
          }
        }
        break

      case 'tool_call_start':
        currentToolId = event.toolCallId ?? ''
        currentToolName = event.toolCallName ?? ''
        currentToolArgs = ''
        break

      case 'tool_call_args':
        if (event.toolCallArgsDelta) {
          currentToolArgs += event.toolCallArgsDelta
        }
        break

      case 'tool_call_end': {
        let input: unknown = {}
        try {
          input = JSON.parse(currentToolArgs) as unknown
        } catch {
          // Fallback to empty object on parse failure
        }
        toolCalls.push({
          id: event.toolCallId ?? currentToolId,
          name: currentToolName,
          input,
        })
        currentToolArgs = ''
        break
      }

      case 'thinking_delta':
        thinkingDeltaBuf += event.content
        break

      case 'thinking_done':
        thinkingDoneBuf = event.content
        thinkingDeltaBuf = ''
        break
    }
  }

  const stopReason: StopReason = explicitStopReason
    ? explicitStopReason
    : toolCalls.length > 0
      ? 'tool_use'
      : 'end_turn'

  let thinking: string | undefined
  if (thinkingDoneBuf !== undefined) {
    thinking = thinkingDoneBuf.length > 0 ? thinkingDoneBuf : undefined
  } else if (thinkingDeltaBuf.length > 0) {
    thinking = thinkingDeltaBuf
  }

  const usage: Usage = {
    inputTokens,
    outputTokens,
  }
  if (cacheCreationInputTokens !== undefined) {
    usage.cacheCreationInputTokens = cacheCreationInputTokens
  }
  if (cacheReadInputTokens !== undefined) {
    usage.cacheReadInputTokens = cacheReadInputTokens
  }

  return {
    content,
    thinking,
    toolCalls,
    model: '',
    usage,
    stopReason,
  }
}

/** An async iterable of native model stream deltas. Mirrors Rust `BoxModelStream`. */
export type BoxModelStream = AsyncIterable<ModelStreamDelta>

/**
 * Reassemble a native model stream into a ModelChatResponse.
 *
 * Mirrors Rust `collect_model_stream` (stream.rs:155-239). Three rules differ
 * from `collectStream` above and are contract, not implementation detail
 * (milestone D8):
 *
 *   1. `tool_call_done` is AUTHORITATIVE. The accumulated `function_arguments`
 *      / `freeform_input` buffers exist so a caller can render progress; they
 *      are discarded for the completed call and NEVER lowered into it. In
 *      particular a freeform `input` is never JSON-parsed.
 *   2. `usage` REPLACES the running value. (collectStream uses
 *      replace-with-fallback because Anthropic's message_delta usage is
 *      cumulative; the Responses terminal frame is not.)
 *   3. `thinking_done` wins over concatenated `thinking_delta` content, and an
 *      empty `thinking_done` payload means "no thinking", not "empty string".
 *
 * `model` is returned EMPTY — the caller (provider or Client) backfills it
 * from the request/provider default.
 */
export async function collectModelStream(stream: BoxModelStream): Promise<ModelChatResponse> {
  let content = ''
  let thinkingDeltaBuf = ''
  let thinkingDoneBuf: string | undefined
  const functionArguments = new Map<string, string>()
  const freeformInputs = new Map<string, string>()
  const toolCalls: ModelToolCall[] = []
  let usage: Usage = { inputTokens: 0, outputTokens: 0 }
  let explicitStopReason: StopReason | undefined

  for await (const delta of stream) {
    if (delta.type === 'done') {
      explicitStopReason = delta.stopReason
      break
    }

    switch (delta.type) {
      case 'text':
        content += delta.delta
        break

      case 'thinking_delta':
        thinkingDeltaBuf += delta.delta
        break

      case 'thinking_done':
        thinkingDoneBuf = delta.thinking
        thinkingDeltaBuf = ''
        break

      case 'function_arguments':
        functionArguments.set(
          delta.callId,
          (functionArguments.get(delta.callId) ?? '') + delta.delta,
        )
        break

      case 'freeform_input':
        freeformInputs.set(delta.callId, (freeformInputs.get(delta.callId) ?? '') + delta.delta)
        break

      case 'tool_call_done':
        // Discard the bookkeeping buffer; the completed call is the truth.
        if (delta.call.kind === 'function') {
          functionArguments.delete(delta.call.id)
        } else {
          freeformInputs.delete(delta.call.id)
        }
        toolCalls.push(delta.call)
        break

      case 'usage':
        usage = delta.usage
        break
    }
  }

  let thinking: string | undefined
  if (thinkingDoneBuf !== undefined) {
    thinking = thinkingDoneBuf.length > 0 ? thinkingDoneBuf : undefined
  } else if (thinkingDeltaBuf.length > 0) {
    thinking = thinkingDeltaBuf
  }

  const stopReason: StopReason =
    explicitStopReason ?? (toolCalls.length > 0 ? 'tool_use' : 'end_turn')

  const response: ModelChatResponse = {
    content,
    toolCalls,
    model: '',
    usage,
    stopReason,
  }
  if (thinking !== undefined) response.thinking = thinking
  return response
}

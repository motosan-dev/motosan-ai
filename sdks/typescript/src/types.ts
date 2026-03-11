export type Role = 'user' | 'assistant' | 'system' | 'tool'

export interface ToolCall {
  id: string
  name: string
  input: Record<string, unknown>
}

export interface Message {
  role: Role
  content: string
  toolCallId?: string
  toolCalls?: ToolCall[]
}

export interface Tool {
  name: string
  description?: string
  inputSchema?: Record<string, unknown>
}

export interface ChatRequest {
  messages: Message[]
  tools?: Tool[]
  system?: string
  model?: string
  maxTokens?: number
  temperature?: number
  providerOptions?: Record<string, unknown>
}

export interface ChatResponse {
  content: string
  toolCalls: ToolCall[]
  model: string
  usage: { inputTokens: number; outputTokens: number }
  stopReason: 'end_turn' | 'tool_use' | 'max_tokens' | 'stop_sequence' | 'stop' | 'other'
}

export interface StreamEvent {
  content: string
  done: boolean
}

export const MessageFactory = {
  user(content: string): Message {
    return { role: 'user', content }
  },
  assistant(content: string): Message {
    return { role: 'assistant', content }
  },
  assistantWithToolCalls(content: string, toolCalls: ToolCall[]): Message {
    return { role: 'assistant', content, toolCalls }
  },
  system(content: string): Message {
    return { role: 'system', content }
  },
  toolResult(toolCallId: string, content: string): Message {
    return { role: 'tool', content, toolCallId }
  }
}

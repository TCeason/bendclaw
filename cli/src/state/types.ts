/**
 * State types for the CLI UI layer.
 */

// ---------------------------------------------------------------------------
// Message types
// ---------------------------------------------------------------------------

export type MessageRole = 'user' | 'assistant'

export interface UIMessage {
  id: string
  role: MessageRole
  text: string
  timestamp: number
  toolCalls?: UIToolCall[]
  /** Run stats attached to the final assistant message of a run */
  runStats?: RunStats
  /** Verbose events that occurred before this message */
  verboseEvents?: VerboseEvent[]
  /** Text was already streamed to stdout — skip rendering in Message component */
  streamed?: boolean
}

export interface UIToolCall {
  id: string
  name: string
  args: Record<string, unknown>
  status: 'running' | 'done' | 'error'
  result?: string
  previewCommand?: string
  durationMs?: number
}

// ---------------------------------------------------------------------------
// Run stats — accumulated during a run, shown in verbose mode
// ---------------------------------------------------------------------------

export interface RunStats {
  durationMs: number
  turnCount: number
  toolCallCount: number
  toolErrorCount: number
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  cacheWriteTokens: number
  llmCalls: number
  contextTokens: number
  contextWindow: number
  toolBreakdown: ToolBreakdownEntry[]
  llmCallDetails: LlmCallDetail[]
  compactHistory: CompactRecord[]
  lastMessageStats: MessageStats | null
  systemPromptTokens: number
}

export interface LlmCallDetail {
  model: string
  durationMs: number
  inputTokens: number
  outputTokens: number
  ttfbMs: number
  ttftMs: number
  tokPerSec: number
}

export interface ToolBreakdownEntry {
  name: string
  count: number
  totalDurationMs: number
  errors: number
}

export interface CompactRecord {
  level: number
  beforeTokens: number
  afterTokens: number
}

// ---------------------------------------------------------------------------
// Message stats — token breakdown by role (estimated from JSON size)
// ---------------------------------------------------------------------------

export interface MessageStats {
  userCount: number
  assistantCount: number
  toolResultCount: number
  userTokens: number
  assistantTokens: number
  toolResultTokens: number
  /** Per-tool token breakdown: [name, tokens], sorted by tokens desc */
  toolDetails: [string, number][]
}

// ---------------------------------------------------------------------------
// Verbose events
// ---------------------------------------------------------------------------

export interface VerboseEvent {
  kind: 'llm_call' | 'llm_completed' | 'compact_call' | 'compact_done'
  text: string
}

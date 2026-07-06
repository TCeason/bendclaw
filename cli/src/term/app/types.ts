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
  /** Structured tool-result metadata (e.g. a plan artifact's task list),
   *  restored from the transcript so artifacts render on resume. */
  details?: unknown
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
  /** Last LLM call snapshot (used for per-call verbose display) */
  lastMessageStats: MessageStats | null
  /** Cumulative token breakdown across all LLM calls (kept for compatibility) */
  cumulativeStats: MessageStats
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
  beforeTokens?: number
  afterTokens?: number
  from_tokens?: number
  to_tokens?: number
  fromTokens?: number
  toTokens?: number
}

// ---------------------------------------------------------------------------
// Message stats — token breakdown by role
// ---------------------------------------------------------------------------

export interface MessageStats {
  userCount: number
  assistantCount: number
  toolResultCount: number
  imageCount: number
  userTokens: number
  assistantTokens: number
  toolResultTokens: number
  imageTokens: number
  /** Per-tool token breakdown: [name, tokens], sorted by tokens desc */
  toolDetails: [string, number][]
}

// ---------------------------------------------------------------------------
// Verbose events
// ---------------------------------------------------------------------------

export interface VerboseEvent {
  kind: 'llm_call' | 'llm_retry' | 'llm_completed' | 'compact_call' | 'compact_done'
  text: string
  expandedText?: string
}

// ---------------------------------------------------------------------------
// AskUser types — structured questions from the agent
// ---------------------------------------------------------------------------

export interface AskUserOption {
  label: string
  description: string
}

export interface AskUserQuestion {
  header: string
  question: string
  options: AskUserOption[]
}

export interface AskUserRequest {
  questions: AskUserQuestion[]
}

export interface AskUserAnswer {
  header: string
  question: string
  answer: string
}

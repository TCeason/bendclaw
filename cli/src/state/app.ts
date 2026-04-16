/**
 * AppState — top-level UI state and factory.
 */

import type { UIMessage, UIToolCall, RunStats, VerboseEvent, AskUserRequest } from './types.js'

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

export interface AppState {
  messages: UIMessage[]
  isLoading: boolean
  sessionId: string | null
  model: string
  cwd: string
  error: string | null
  verbose: boolean
  currentStreamText: string
  currentThinkingText: string
  activeToolCalls: Map<string, UIToolCall>
  /** Accumulated tool calls for the current turn, merged into assistant_completed */
  turnToolCalls: UIToolCall[]
  /** Stats accumulated during the current run */
  currentRunStats: RunStats
  /** Start time of the current run */
  runStartTime: number
  /** Verbose inline events (LLM calls, compaction) shown during streaming */
  verboseEvents: VerboseEvent[]
  /** Timestamp of the last received token (for stall detection) */
  lastTokenAt: number
  /** Pending ask_user request from the agent (null = none) */
  askUserRequest: AskUserRequest | null
}

export function emptyRunStats(): RunStats {
  return {
    durationMs: 0,
    turnCount: 0,
    toolCallCount: 0,
    toolErrorCount: 0,
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    cacheWriteTokens: 0,
    llmCalls: 0,
    contextTokens: 0,
    contextWindow: 0,
    toolBreakdown: [],
    llmCallDetails: [],
    compactHistory: [],
    lastMessageStats: null,
    systemPromptTokens: 0,
  }
}

export function createInitialState(model: string, cwd: string): AppState {
  return {
    messages: [],
    isLoading: false,
    sessionId: null,
    model,
    cwd,
    error: null,
    verbose: true,
    currentStreamText: '',
    currentThinkingText: '',
    activeToolCalls: new Map(),
    turnToolCalls: [],
    currentRunStats: emptyRunStats(),
    runStartTime: 0,
    verboseEvents: [],
    lastTokenAt: 0,
    askUserRequest: null,
  }
}

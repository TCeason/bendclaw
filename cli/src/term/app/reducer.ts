/**
 * Reducer-style state updates from RunEvents.
 */

import type { RunEvent } from '../../native/index.js'
import { formatLlmCallStarted, formatLlmCallRetry, formatLlmCallCompleted, formatCompactionStarted, formatCompactionCompleted } from '../../render/verbose.js'
import { emptyRunStats, type AppState } from './state.js'
import type { CompactRecord, MessageStats, UIMessage, UIToolCall } from './types.js'


export function applyEvent(state: AppState, event: RunEvent): AppState {
  const kind = event.kind
  const p = event.payload as Record<string, any>

  switch (kind) {
    case 'run_started':
      return {
        ...state,
        isLoading: true,
        sessionId: event.session_id,
        error: null,
        currentStreamText: '',
        currentThinkingText: '',
        liveToolCalls: new Map(),
        currentRunStats: emptyRunStats(),
        runStartTime: Date.now(),
        verboseEvents: [],
      }

    case 'turn_started':
      return {
        ...state,
        currentStreamText: '',
        currentThinkingText: '',
        currentRunStats: {
          ...state.currentRunStats,
          turnCount: state.currentRunStats.turnCount + 1,
        },
      }

    case 'assistant_delta': {
      const delta = p.delta as string | undefined
      const thinkingDelta = p.thinking_delta as string | undefined
      return {
        ...state,
        currentStreamText: state.currentStreamText + (delta ?? ''),
        currentThinkingText: state.currentThinkingText + (thinkingDelta ?? ''),
        lastTokenAt: Date.now(),
      }
    }

    case 'assistant_tool_call': {
      const id = p.tool_call_id as string
      if (!id) return state
      const next = new Map(state.liveToolCalls)
      const current = next.get(id)
      next.set(id, {
        ...current,
        id,
        name: (p.tool_name as string) || current?.name || '',
        args: (p.args as Record<string, unknown>) ?? current?.args ?? {},
        status: current?.status ?? 'running',
        contentIndex: (p.content_index as number | undefined) ?? current?.contentIndex,
      })
      return { ...state, liveToolCalls: next }
    }

    case 'assistant_completed': {
      const content = p.content as any[] | undefined
      const textParts = (content ?? [])
        .filter((b: any) => b.type === 'text')
        .map((b: any) => b.text)
      const text = textParts.join('') || state.currentStreamText

      const liveToolCalls = new Map(state.liveToolCalls)
      for (const [contentIndex, block] of (content ?? []).entries()) {
        if (block.type !== 'tool_call') continue
        const current = liveToolCalls.get(block.id)
        liveToolCalls.set(block.id, {
          ...current,
          id: block.id as string,
          name: block.name as string,
          args: (block.input ?? {}) as Record<string, unknown>,
          status: current?.status ?? 'running',
          argsComplete: true,
          contentIndex: current?.contentIndex ?? contentIndex,
        })
      }

      const toolCalls = [...liveToolCalls.values()].sort(
        (a, b) => (a.contentIndex ?? Number.MAX_SAFE_INTEGER) - (b.contentIndex ?? Number.MAX_SAFE_INTEGER),
      )
      const msg: UIMessage = {
        id: event.event_id,
        role: 'assistant',
        text,
        timestamp: Date.now(),
        toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
        verboseEvents: state.verboseEvents.length > 0 ? [...state.verboseEvents] : undefined,
        streamed: state.currentStreamText.length > 0,
      }

      return {
        ...state,
        messages: [...state.messages, msg],
        currentStreamText: '',
        currentThinkingText: '',
        liveToolCalls,
        verboseEvents: [],
      }
    }

    case 'tool_started': {
      const id = p.tool_call_id as string
      if (!id) return state
      const next = new Map(state.liveToolCalls)
      const current = next.get(id)
      next.set(id, {
        ...current,
        id,
        name: (p.tool_name as string) ?? current?.name ?? 'unknown',
        args: (p.args as Record<string, unknown>) ?? current?.args ?? {},
        status: 'running',
        contentIndex: current?.contentIndex,
        argsComplete: true,
        startedAt: current?.startedAt ?? Date.now(),
        previewCommand: p.preview_command ?? current?.previewCommand,
      })
      return { ...state, liveToolCalls: next }
    }

    case 'tool_progress': {
      const id = p.tool_call_id as string
      if (!id) return state
      const next = new Map(state.liveToolCalls)
      const current = next.get(id)
      if (!current) return state
      const text = p.text as string | undefined
      const progress = text && !/^Running\.\.\. \d+s$/.test(text.trim()) ? text : current.progress
      next.set(id, {
        ...current,
        progress: progress || undefined,
        details: p.details ?? current.details,
      })
      return { ...state, liveToolCalls: next }
    }

    case 'tool_finished': {
      const id = p.tool_call_id as string
      const isError = !!p.is_error
      const current = state.liveToolCalls.get(id)
      const toolName = p.tool_name ?? current?.name ?? 'unknown'
      const durationMs = (p.duration_ms as number) ?? 0

      const finished: UIToolCall = {
        id,
        name: toolName,
        args: current?.args ?? (p.args as Record<string, unknown>) ?? {},
        status: isError ? 'error' : 'done',
        contentIndex: current?.contentIndex,
        result: p.content,
        details: p.details,
        previewCommand: current?.previewCommand,
        durationMs,
      }

      const details = p.details as Record<string, any> | undefined
      if (details?.diff && typeof details.diff === 'string') {
        finished.details = { ...(finished.details as Record<string, unknown> ?? {}), diff: details.diff }
      }

      const liveToolCalls = new Map(state.liveToolCalls)
      liveToolCalls.set(id, finished)

      const stats = { ...state.currentRunStats }
      stats.toolCallCount++
      if (isError) stats.toolErrorCount++

      const breakdown = stats.toolBreakdown.map((e) =>
        e.name === toolName
          ? { ...e, count: e.count + 1, totalDurationMs: e.totalDurationMs + durationMs, errors: e.errors + (isError ? 1 : 0) }
          : e,
      )
      if (!breakdown.some((e) => e.name === toolName)) {
        breakdown.push({
          name: toolName,
          count: 1,
          totalDurationMs: durationMs,
          errors: isError ? 1 : 0,
        })
      }
      stats.toolBreakdown = breakdown

      return {
        ...state,
        liveToolCalls,
        messages: updateToolCallInMessages(state.messages, id, finished),
        currentRunStats: stats,
      }
    }

    case 'llm_call_started': {
      const model = (p.model as string) ?? state.model
      const turn = event.turn
      const sysTok = (p.system_prompt_tokens as number) ?? 0
      const toolDefTok = (p.tool_definition_tokens as number) ?? 0

      // Pre-computed message stats from Rust side (always present)
      const ms = p.message_stats as Record<string, any> | undefined
      const msgStats: MessageStats | null = ms
        ? {
            userCount: (ms.user_count as number) ?? 0,
            assistantCount: (ms.assistant_count as number) ?? 0,
            toolResultCount: (ms.tool_result_count as number) ?? 0,
            imageCount: (ms.image_count as number) ?? 0,
            userTokens: (ms.user_tokens as number) ?? 0,
            assistantTokens: (ms.assistant_tokens as number) ?? 0,
            toolResultTokens: (ms.tool_result_tokens as number) ?? 0,
            imageTokens: (ms.image_tokens as number) ?? 0,
            toolDetails: (ms.tool_details as [string, number][]) ?? [],
          }
        : null

      const data: Record<string, unknown> = {
        ...p,
        model,
        turn,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatLlmCallStarted(data)

      // Accumulate cumulative stats across all LLM calls
      const prev = state.currentRunStats.cumulativeStats
      const cumulative: MessageStats = msgStats
        ? {
            userCount: prev.userCount + msgStats.userCount,
            assistantCount: prev.assistantCount + msgStats.assistantCount,
            toolResultCount: prev.toolResultCount + msgStats.toolResultCount,
            imageCount: prev.imageCount + msgStats.imageCount,
            userTokens: prev.userTokens + msgStats.userTokens,
            assistantTokens: prev.assistantTokens + msgStats.assistantTokens,
            toolResultTokens: prev.toolResultTokens + msgStats.toolResultTokens,
            imageTokens: prev.imageTokens + msgStats.imageTokens,
            toolDetails: [...prev.toolDetails, ...msgStats.toolDetails],
          }
        : prev

      return {
        ...state,
        currentRunStats: {
          ...state.currentRunStats,
          contextTokens: (p.estimated_context_tokens as number) ?? state.currentRunStats.contextTokens,
          contextWindow: (p.context_window as number) ?? state.currentRunStats.contextWindow,
          lastMessageStats: msgStats,
          cumulativeStats: cumulative,
          systemPromptTokens: sysTok + toolDefTok,
        },
        sessionTokens: {
          ...state.sessionTokens,
          contextTokens: (p.estimated_context_tokens as number) ?? state.sessionTokens.contextTokens,
          contextWindow: (p.context_window as number) ?? state.sessionTokens.contextWindow,
        },
        verboseEvents: [...state.verboseEvents, { kind: 'llm_call', text }],
      }
    }

    case 'llm_call_retry':
    case 'api_retry': {
      const text = formatLlmCallRetry(p)
      return {
        ...state,
        verboseEvents: [...state.verboseEvents, { kind: 'llm_retry', text }],
      }
    }

    case 'llm_call_completed': {
      const usage = p.usage as Record<string, any> | undefined
      const metrics = p.metrics as Record<string, any> | undefined
      const error = p.error as string | undefined
      const stats = { ...state.currentRunStats }
      stats.llmCalls++
      const inputTok = (usage?.input as number) ?? 0
      const outputTok = (usage?.output as number) ?? 0
      const durationMs = (metrics?.duration_ms as number) ?? 0
      const ttfbMs = (metrics?.ttfb_ms as number) ?? 0
      const ttftMs = (metrics?.ttft_ms as number) ?? 0
      const streamingMs = (metrics?.streaming_ms as number) ?? 0
      // Real generation speed: output tokens over the pure streaming window
      // (first delta → done), not total wall-clock. duration_ms would dilute the
      // rate with the ttfb wait (queueing + prompt processing).
      const tokPerSec = streamingMs > 0 ? outputTok / (streamingMs / 1000) : 0

      if (usage) {
        stats.inputTokens += inputTok
        stats.outputTokens += outputTok
        stats.cacheReadTokens += (usage.cache_read as number) ?? 0
        stats.cacheWriteTokens += (usage.cache_write as number) ?? 0

        // Provider usage buckets are disjoint.
        const realContextTokens =
          inputTok +
          ((usage.cache_read as number) ?? 0) +
          ((usage.cache_write as number) ?? 0) +
          outputTok
        if (realContextTokens > 0) {
          stats.contextTokens = realContextTokens
        }
      }

      stats.llmCallDetails = [...stats.llmCallDetails, {
        model: (p.model as string) ?? state.model,
        durationMs,
        inputTokens: inputTok,
        outputTokens: outputTok,
        ttfbMs,
        ttftMs,
        tokPerSec,
      }]

      const data: Record<string, unknown> = {
        ...p,
        model: (p.model as string) ?? state.model,
        turn: event.turn,
        estimated_context_tokens: state.currentRunStats.contextTokens,
        context_window: state.currentRunStats.contextWindow,
      }
      const result = formatLlmCallCompleted(data)

      return {
        ...state,
        currentRunStats: stats,
        sessionTokens: {
          inputTokens: state.sessionTokens.inputTokens + inputTok,
          outputTokens: state.sessionTokens.outputTokens + outputTok,
          cacheReadTokens: state.sessionTokens.cacheReadTokens + ((usage?.cache_read as number) ?? 0),
          contextTokens: stats.contextTokens,
          contextWindow: stats.contextWindow,
        },
        verboseEvents: [...state.verboseEvents, { kind: 'llm_completed', text: result.text, expandedText: result.expandedText }],
      }
    }

    case 'context_compaction_started': {
      const data: Record<string, unknown> = {
        ...p,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatCompactionStarted(data)

      return {
        ...state,
        currentRunStats: { ...state.currentRunStats, contextTokens: (p.estimated_tokens as number) ?? 0, contextWindow: (p.context_window as number) ?? 0 },
        sessionTokens: { ...state.sessionTokens, contextTokens: (p.estimated_tokens as number) ?? state.sessionTokens.contextTokens, contextWindow: (p.context_window as number) ?? state.sessionTokens.contextWindow },
        verboseEvents: [...state.verboseEvents, { kind: 'compact_call', text }],
      }
    }

    case 'context_compaction_completed': {
      const result = p.result as Record<string, any> | undefined
      const type = (result?.type as string) ?? 'done'

      const data: Record<string, unknown> = {
        ...p,
        context_window: state.currentRunStats.contextWindow,
      }
      const text = formatCompactionCompleted(data)

      const compactRecord: CompactRecord | null =
        type === 'level_compacted' || type === 'level_done' || type === 'compacted'
          ? {
              level: (result?.level as number) ?? (result?.messages_evicted ? 3 : 1),
              beforeTokens: ((result?.before_estimated_tokens as number) ?? (result?.before_tokens as number) ?? (result?.tokens_before as number)) ?? 0,
              afterTokens: ((result?.after_estimated_tokens as number) ?? (result?.after_tokens as number) ?? (result?.tokens_after as number)) ?? 0,
            }
          : type === 'run_once_cleared'
            ? {
                level: 0,
                beforeTokens: ((result?.before_estimated_tokens as number) ?? state.currentRunStats.contextTokens) ?? 0,
                afterTokens: ((result?.after_estimated_tokens as number) ?? (state.currentRunStats.contextTokens - ((result?.saved_tokens as number) ?? 0))) ?? 0,
              }
            : null

      const updatedStats = compactRecord
        ? { ...state.currentRunStats, compactHistory: [...state.currentRunStats.compactHistory, compactRecord] }
        : state.currentRunStats

      return {
        ...state,
        currentRunStats: updatedStats,
        verboseEvents: [...state.verboseEvents, { kind: 'compact_done', text }],
      }
    }

    case 'run_finished': {
      const serverDuration = (p.duration_ms as number) ?? 0
      const stats = {
        ...state.currentRunStats,
        durationMs: serverDuration || (Date.now() - state.runStartTime),
        turnCount: (p.turn_count as number) ?? state.currentRunStats.turnCount,
      }

      return {
        ...state,
        isLoading: false,
        currentStreamText: '',
        currentThinkingText: '',
        liveToolCalls: new Map(),
        currentRunStats: stats,
      }
    }

    case 'error':
      return {
        ...state,
        isLoading: false,
        error: p.message ?? 'Unknown error',
      }

    default:
      return state
  }
}

/** Rough token estimate: ~4 chars per token. */
function estimateTokens(text: string): number {
  return Math.ceil(text.length / 4)
}

/**
 * Count messages by role and estimate token usage.
 * Unknown roles are counted as user.
 */
export function countMessagesByRole(messages: { role: string; content?: string; toolName?: string }[]): MessageStats {
  let userCount = 0
  let assistantCount = 0
  let toolResultCount = 0
  let userTokens = 0
  let assistantTokens = 0
  let toolResultTokens = 0
  const toolDetails: [string, number][] = []

  for (const msg of messages) {
    const tokens = estimateTokens(msg.content ?? '')
    if (msg.role === 'assistant') {
      assistantCount++
      assistantTokens += tokens
    } else if (msg.role === 'tool_result') {
      toolResultCount++
      toolResultTokens += tokens
      toolDetails.push([msg.toolName ?? 'unknown', tokens])
    } else {
      userCount++
      userTokens += tokens
    }
  }

  toolDetails.sort((a, b) => b[1] - a[1])

  return { userCount, assistantCount, toolResultCount, imageCount: 0, userTokens, assistantTokens, toolResultTokens, imageTokens: 0, toolDetails }
}

function updateToolCallInMessages(messages: UIMessage[], toolCallId: string, finished: UIToolCall): UIMessage[] {
  for (let i = messages.length - 1; i >= 0; i--) {
    const msg = messages[i]!
    if (!msg.toolCalls) continue
    const idx = msg.toolCalls.findIndex((tc) => tc.id === toolCallId)
    if (idx >= 0) {
      const newMessages = [...messages]
      const newToolCalls = [...msg.toolCalls]
      newToolCalls[idx] = finished
      newMessages[i] = { ...msg, toolCalls: newToolCalls }
      return newMessages
    }
  }
  return messages
}

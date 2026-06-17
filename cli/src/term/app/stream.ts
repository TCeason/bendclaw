import { buildError, buildRunSummary, buildToolCall, buildToolProgress, buildToolResult, buildVerboseEvent, buildLlmCard, isVisibleLlmEvent, buildAssistantLines, buildThinkingSummary, type OutputLine } from '../../render/output.js'
import { formatDuration } from '../../render/format.js'
import { setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'

let sepId = 0

function assistantContinuationSpacer(): OutputLine {
  return { id: `sep-${sepId++}`, kind: 'assistant', text: '', isContinuationSpacer: true }
}

export interface StreamMachineState {
  appState: AppState
  spinnerState: SpinnerState
  pendingText: string
  pendingThinkingText: string
  toolProgress: string
  lastToolProgress: string
  streamingText: string
  streamingThinkingText: string
  thinkingTokenCount: number
  prefixEmitted: boolean
  assistantCommitted: boolean
  activeLlmCall: boolean
}

export interface StreamContext {
  termRows: number
}

export interface StreamUpdate {
  state: StreamMachineState
  commitLines: OutputLine[]
  expandedCommitLines?: OutputLine[]
  writeLines: OutputLine[]
  rerenderStatus: boolean
  suppressToolStarted: boolean
  suppressToolFinished: boolean
}

function isHeartbeatProgress(text: string): boolean {
  return /^Running\.\.\. \d+s$/.test(text.trim())
}

function parseSpillProgress(text: string): Record<string, unknown> | undefined {
  const prefix = '__evot_spill_event__ '
  if (!text.startsWith(prefix)) return undefined
  try {
    const parsed = JSON.parse(text.slice(prefix.length))
    return parsed && typeof parsed === 'object' ? parsed as Record<string, unknown> : undefined
  } catch {
    return undefined
  }
}

function buildSpillEventLines(event: Record<string, unknown>, toolName?: string): OutputLine[] {
  const kind = event.kind === 'read' ? 'read' : 'write'
  const path = typeof event.path === 'string' ? event.path : ''
  const sizeBytes = typeof event.size_bytes === 'number' ? event.size_bytes : 0
  const previewBytes = typeof event.preview_bytes === 'number' ? event.preview_bytes : undefined
  const durationMs = typeof event.duration_ms === 'number' ? event.duration_ms : undefined
  const bits = [`${humanBytes(sizeBytes)} ${kind === 'read' ? 'read' : 'written'}`]
  if (previewBytes !== undefined) bits.push(`${humanBytes(previewBytes)} preview`)
  if (durationMs !== undefined) bits.push(formatDuration(durationMs))
  if (toolName) bits.push(toolName)
  return [
    { id: `spill-${Date.now()}-0`, kind: 'verbose', text: `[SPILL] ${kind === 'read' ? '↩' : '↪'} ${bits.join(' · ')}` },
    ...(path ? [{ id: `spill-${Date.now()}-1`, kind: 'verbose' as const, text: `  ${path}` }] : []),
  ]
}

function humanBytes(n: number): string {
  if (!Number.isFinite(n) || n < 0) return '0 B'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

export function createStreamMachineState(appState: AppState, spinnerState: SpinnerState): StreamMachineState {
  return {
    appState,
    spinnerState,
    pendingText: '',
    pendingThinkingText: '',
    toolProgress: '',
    lastToolProgress: '',
    streamingText: '',
    streamingThinkingText: '',
    thinkingTokenCount: 0,
    prefixEmitted: false,
    assistantCommitted: false,
    activeLlmCall: false,
  }
}

export function reduceRunEvent(prev: StreamMachineState, event: RunEvent, ctx: StreamContext): StreamUpdate {
  const p = (event.payload ?? {}) as Record<string, any>
  let state = event.kind === 'ask_user' ? prev : { ...prev, appState: applyEvent(prev.appState, event) }
  const commitLines: OutputLine[] = []
  const writeLines: OutputLine[] = []
  let expandedCommitLines: OutputLine[] | undefined
  let rerenderStatus = false
  let suppressToolStarted = false
  let suppressToolFinished = false

  function mergeFlushExpanded(flushed: { expandedLines?: OutputLine[] }) {
    if (flushed.expandedLines) {
      if (!expandedCommitLines) expandedCommitLines = []
      expandedCommitLines.push(...flushed.expandedLines)
    }
  }

  // LLM / COMPACT / SPILL stats are always produced but only belong in
  // screen.log. The exceptions are LLM errors and retries, which render as
  // tool-style cards in the TUI so the user always sees them.
  const routeVerbose = (text: string, target: { commit: OutputLine[]; write: OutputLine[] }) => {
    if (isVisibleLlmEvent(text)) target.commit.push(...buildLlmCard(text))
    else target.write.push(...buildVerboseEvent(text))
  }

  if (event.kind === 'llm_call_started' || event.kind === 'llm_call_retry' || event.kind === 'api_retry' || event.kind === 'context_compaction_started') {
    const flushed = flushStreaming(state)
    state = { ...flushed.state, toolProgress: '', lastToolProgress: '', activeLlmCall: event.kind === 'llm_call_started' || event.kind === 'llm_call_retry' || event.kind === 'api_retry' }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      routeVerbose(evt.text, { commit: commitLines, write: writeLines })
    }
  }

  if (event.kind === 'assistant_delta') {
    const thinkingDelta = p.thinking_delta as string | undefined
    if (thinkingDelta) {
      state = {
        ...state,
        streamingThinkingText: state.streamingThinkingText + thinkingDelta,
        pendingThinkingText: state.streamingThinkingText + thinkingDelta,
        thinkingTokenCount: state.thinkingTokenCount + 1,
        spinnerState: {
          ...state.spinnerState,
          lastTokenAt: Date.now(),
          streaming: true,
          tokenCount: state.spinnerState.tokenCount + 1,
        },
      }
      rerenderStatus = true
    }

    const delta = p.delta as string | undefined
    if (delta) {
      // When first text delta arrives after thinking, flush thinking content
      if (state.streamingThinkingText) {
        const thinkingDurationMs = Date.now() - state.spinnerState.phaseStartedAt
        const compactLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs)
        const expLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs, true)
        commitLines.push(...compactLines)
        if (!expandedCommitLines) expandedCommitLines = []
        expandedCommitLines.push(...expLines)
        state = { ...state, streamingThinkingText: '', pendingThinkingText: '' }
      }

      state = { ...state, streamingText: state.streamingText + delta }
      if (!state.prefixEmitted) {
        const trimmed = state.streamingText.replace(/^[\n\r]+/, '')
        if (trimmed.length > 0) {
          state = { ...state, streamingText: trimmed, prefixEmitted: true }
        }
      }

      state = {
        ...state,
        spinnerState: {
          ...state.spinnerState,
          lastTokenAt: Date.now(),
          streaming: true,
          tokenCount: state.spinnerState.tokenCount + 1,
        },
      }

      // Update pendingText for the viewport's streaming display.
      state = { ...state, pendingText: state.streamingText }
      rerenderStatus = true
    }
  }

  if (event.kind === 'assistant_completed' || event.kind === 'turn_started') {
    const flushed = flushStreaming(state)
    state = {
      ...flushed.state,
      toolProgress: '',
      lastToolProgress: '',
      activeLlmCall: event.kind === 'assistant_completed' ? false : flushed.state.activeLlmCall,
      spinnerState: { ...flushed.state.spinnerState, streaming: false },
    }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    rerenderStatus = true
  }

  if (event.kind === 'llm_call_completed' || event.kind === 'context_compaction_completed') {
    const flushed = flushStreaming(state)
    state = { ...flushed.state, activeLlmCall: event.kind === 'llm_call_completed' ? false : flushed.state.activeLlmCall }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      routeVerbose(evt.text, { commit: commitLines, write: writeLines })
    }
  }

  if (event.kind === 'tool_started') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    const toolName = (p.tool_name as string) ?? 'unknown'
    const isAskUser = toolName === 'AskUser' || toolName === 'ask_user'
    const spinnerPhase = isAskUser ? 'thinking' : 'executing'
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, spinnerPhase, toolName),
    }
    suppressToolStarted = isAskUser
    rerenderStatus = true
  }

  if (event.kind === 'tool_progress') {
    const text = p.text as string | undefined
    const details = p.details as Record<string, any> | undefined

    // Preview diff — render immediately before tool finishes
    if (details?.preview && details?.diff) {
      const flushed = flushStreaming(state)
      state = { ...flushed.state, toolProgress: '', lastToolProgress: '' }
      commitLines.push(...flushed.lines)
      mergeFlushExpanded(flushed)
      const toolName = (p.tool_name as string) ?? 'unknown'
      const previewArgs = { diff: details.diff as string }
      const previewLines = buildToolResult(toolName, previewArgs, 'done', undefined, undefined)
      commitLines.push(...previewLines)
      if (!expandedCommitLines) expandedCommitLines = []
      expandedCommitLines.push(...previewLines)
      rerenderStatus = true
    } else if (text) {
      const spill = parseSpillProgress(text)
      if (spill) {
        const flushed = flushStreaming(state)
        state = { ...flushed.state, toolProgress: '', lastToolProgress: '' }
        commitLines.push(...flushed.lines)
        mergeFlushExpanded(flushed)
        const spillLines = buildSpillEventLines(spill, p.tool_name as string | undefined)
        commitLines.push(...spillLines)
      } else {
        state = isHeartbeatProgress(text)
          ? { ...state, toolProgress: '' }
          : { ...state, toolProgress: text, lastToolProgress: text }
      }
      rerenderStatus = true
    }
  }

  if (event.kind === 'tool_finished') {
    const toolName = (p.tool_name as string) ?? 'unknown'
    const isAskUser = toolName === 'AskUser' || toolName === 'ask_user'
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, 'thinking'),
    }
    suppressToolFinished = isAskUser
    rerenderStatus = true
  }

  if (event.kind === 'error') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    writeLines.push(...flushed.lines)
    commitLines.push(...buildError((p.message as string) ?? 'Unknown error'))
  }

  if (event.kind === 'run_finished') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    commitLines.push(...buildRunSummary(state.appState.currentRunStats))
  }

  return {
    state,
    commitLines,
    expandedCommitLines,
    writeLines,
    rerenderStatus,
    suppressToolStarted,
    suppressToolFinished,
  }
}

export function buildToolFinishedLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const args = (p.args as Record<string, unknown>) ?? {}
  const details = p.details as Record<string, any> | undefined
  const diff = details?.diff as string | undefined
  // Skip diff if it was already rendered as a preview
  const skipDiff = !!details?.preview_rendered && !!diff
  const mergedArgs = diff && !skipDiff
    ? { ...args, diff }
    : toolName === 'update_goal_tasks' && Array.isArray(details?.goal?.tasks)
      ? { ...args, tasks: details.goal.tasks }
      : args
  const status = p.is_error ? 'error' as const : 'done' as const
  const rawSlim = details?.slim as Record<string, any> | undefined
  const slim = rawSlim && typeof rawSlim.filter === 'string'
    ? {
        filter: rawSlim.filter as string,
        original: Number(rawSlim.original ?? 0),
        slimmed: Number(rawSlim.slimmed ?? 0),
      }
    : undefined
  return buildToolResult(toolName, mergedArgs, status, p.content as string | undefined, p.duration_ms as number | undefined, expanded, slim)
}

export function buildToolStartedLines(event: RunEvent): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const previewCommand = p.preview_command as string | undefined
  return buildToolCall(toolName, (p.args as Record<string, unknown>) ?? {}, previewCommand)
}

export function buildToolProgressLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const text = (p.text as string) ?? ''
  const spill = parseSpillProgress(text)
  if (spill) return buildSpillEventLines(spill, toolName)
  return text ? buildToolProgress(toolName, text, expanded) : []
}

export function flushStreaming(state: StreamMachineState): { state: StreamMachineState; lines: OutputLine[]; expandedLines?: OutputLine[] } {
  const lines: OutputLine[] = []
  let expandedLines: OutputLine[] | undefined

  // Flush any remaining thinking content first
  if (state.streamingThinkingText.trim()) {
    const thinkingDurationMs = Date.now() - state.spinnerState.phaseStartedAt
    const compactLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs)
    const expLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs, true)
    lines.push(...compactLines)
    expandedLines = [...expLines]
  }

  if (state.streamingText.trim()) {
    const assistantLines = buildAssistantLines(state.streamingText)
    if (state.assistantCommitted && assistantLines.length > 0) {
      lines.unshift(assistantContinuationSpacer())
      if (expandedLines) expandedLines.unshift(assistantContinuationSpacer())
    }
    lines.push(...assistantLines)
    if (expandedLines) expandedLines.push(...assistantLines)
  }

  if (lines.length === 0) {
    return {
      state: { ...state, streamingText: '', streamingThinkingText: '', pendingText: '', pendingThinkingText: '', assistantCommitted: false },
      lines: [],
    }
  }

  return {
    state: { ...state, streamingText: '', streamingThinkingText: '', pendingText: '', pendingThinkingText: '', assistantCommitted: false },
    lines,
    expandedLines,
  }
}

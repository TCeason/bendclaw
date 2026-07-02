import { buildError, buildRunSummary, buildToolCall, buildToolProgress, buildToolResult, buildVerboseEvent, buildLlmCard, isVisibleLlmEvent, buildAssistantLines, buildThinkingSummary, type OutputLine } from '../../render/output.js'
import { formatDuration } from '../../render/format.js'
import { setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'
import { findStreamingCommitPoint, findNaturalPlainTextCommitPoint, isInsideOpenMathBlock, isInsideOpenCodeFence } from '../../markdown/streaming/commit.js'

// Detect pipe tables: two or more consecutive lines matching `| ... |`.
// Splitting mid-table produces broken rendering, so the force-split path
// leaves a building table in the pending tail until a safe boundary appears.
const PIPE_TABLE_LINE_RE = /^\s*\|.*\|\s*$/
function isInsidePipeTable(text: string): boolean {
  const lines = text.split('\n')
  let tableLines = 0
  for (const l of lines) {
    if (PIPE_TABLE_LINE_RE.test(l)) {
      tableLines++
      if (tableLines >= 2) return true
    } else if (l.trim()) {
      tableLines = 0
    }
  }
  return false
}

/**
 * Commit completed markdown blocks from the streaming buffer to scrollback,
 * keeping only the still-forming tail in `streamingText` (the dynamic zone).
 *
 * This is the core of streaming smoothness: without it, every delta re-lexes
 * and re-renders the entire accumulated response (O(n²) for a long answer,
 * hundreds of ms per frame). By draining finished blocks into the committed
 * history — which the incremental HistoryRenderCache flattens once and never
 * touches again — per-frame cost drops to O(pending tail).
 *
 * Boundaries come from findStreamingCommitPoint (paragraph/heading/closed-fence
 * boundaries, never mid code-fence or mid-table). For long plain prose with no
 * markdown boundary, findNaturalPlainTextCommitPoint commits complete leading
 * lines. As a last resort, when the pending tail grows past a fraction of the
 * viewport, force-split at the last safe newline so structural content flows
 * into scrollback instead of ballooning the re-rendered dynamic zone.
 *
 * Mutates nothing; returns the updated state and any lines to commit.
 */
function commitCompletedBlocks(
  state: StreamMachineState,
  termRows: number,
): { state: StreamMachineState; lines: OutputLine[] } {
  const lines: OutputLine[] = []

  // 1. Commit at a markdown-safe boundary, or (for long boundary-free prose)
  //    at complete leading lines.
  const mathBlockOpen = isInsideOpenMathBlock(state.streamingText)
  const markdownCommitPoint = findStreamingCommitPoint(state.streamingText)
  const naturalCommitPoint =
    markdownCommitPoint > 0 || mathBlockOpen
      ? 0
      : findNaturalPlainTextCommitPoint(state.streamingText, termRows)
  const commitPoint = markdownCommitPoint || naturalCommitPoint
  if (commitPoint > 0 && commitPoint <= state.streamingText.length) {
    const completed = state.streamingText.slice(0, commitPoint)
    const pending = state.streamingText.slice(commitPoint)
    const built = buildAssistantLines(completed)
    if (built.length > 0) {
      // A markdown-boundary commit that continues an already-committed block
      // needs a spacer so the next chunk isn't tagged as a fresh assistant
      // message (which would draw a second leading dot).
      if (markdownCommitPoint > 0 && state.assistantCommitted) {
        lines.push(assistantContinuationSpacer())
      }
      lines.push(...built)
      state = { ...state, streamingText: pending, assistantCommitted: true }
    }
  }

  // 2. Force-split an over-tall pending tail so it flows into scrollback
  //    instead of re-rendering in the dynamic zone every delta.
  const pendingLineCount = state.streamingText.split('\n').length
  const forceThreshold = Math.max(4, Math.floor(termRows / 3))
  if (pendingLineCount > forceThreshold) {
    const markdownSplitAt = findStreamingCommitPoint(state.streamingText)
    const naturalSplitAt =
      markdownSplitAt > 0 || isInsideOpenMathBlock(state.streamingText)
        ? 0
        : findNaturalPlainTextCommitPoint(state.streamingText, termRows)
    let splitAt = markdownSplitAt || naturalSplitAt
    // Last resort: commit at the last newline so structural markdown (lists,
    // headings, blockquotes) doesn't accumulate in the dynamic zone. Skip when
    // inside an open math block, an open code fence, or a building pipe table —
    // splitting there commits a partial block and tears the rendering.
    if (
      splitAt <= 0 &&
      !isInsideOpenMathBlock(state.streamingText) &&
      !isInsideOpenCodeFence(state.streamingText) &&
      !isInsidePipeTable(state.streamingText)
    ) {
      const lastNl = state.streamingText.lastIndexOf('\n')
      if (lastNl > 0) splitAt = lastNl + 1
    }
    if (splitAt > 0 && splitAt < state.streamingText.length) {
      const chunk = state.streamingText.slice(0, splitAt)
      const rest = state.streamingText.slice(splitAt)
      const built = buildAssistantLines(chunk)
      if (built.length > 0) {
        if (state.assistantCommitted) lines.push(assistantContinuationSpacer())
        lines.push(...built)
        state = { ...state, streamingText: rest, assistantCommitted: true }
      }
    }
  }

  return { state, lines }
}

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
  /** Last error message surfaced via an LLM error card, so a following
   *  `error` event carrying the same text doesn't render it twice. */
  lastLlmErrorMessage: string | null
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
    { id: `spill-${Date.now()}-0`, kind: 'verbose', text: `  ${kind === 'read' ? '↩' : '↪'} ${bits.join(' · ')}` },
    ...(path ? [{ id: `spill-${Date.now()}-1`, kind: 'verbose' as const, text: `    ${path}` }] : []),
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
    lastLlmErrorMessage: null,
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
  // Tracks an LLM error message surfaced as a card this tick (or carried from a
  // prior tick via state), so a following `error` event won't duplicate it.
  let capturedLlmError: string | null = prev.lastLlmErrorMessage

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
    if (isVisibleLlmEvent(text)) {
      target.commit.push(...buildLlmCard(text))
      // Remember the error message (the `    error     <msg>` tail) so a
      // following `error` event with the same text isn't rendered twice.
      const m = text.match(/\n\s*error\s+(.+)$/s)
      if (text.includes('✗') && m) capturedLlmError = m[1]!.trim()
    } else {
      target.write.push(...buildVerboseEvent(text))
    }
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
    const appendVisibleTextDelta = (textDelta: string) => {
      state = { ...state, streamingText: state.streamingText + textDelta }
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

      // Drain completed markdown blocks into scrollback so only the forming
      // tail stays in the re-rendered dynamic zone (keeps per-frame cost at
      // O(pending tail) instead of O(whole response)).
      const committed = commitCompletedBlocks(state, ctx.termRows)
      state = committed.state
      if (committed.lines.length > 0) {
        commitLines.push(...committed.lines)
        if (!expandedCommitLines) expandedCommitLines = []
        expandedCommitLines.push(...committed.lines)
      }

      // pendingText mirrors the still-forming tail for the viewport's
      // streaming display (set after block-commit trims what already landed
      // in scrollback).
      state = { ...state, pendingText: state.streamingText }
      rerenderStatus = true
    }

    const flushThinkingBeforeText = () => {
      if (!state.streamingThinkingText) return
      const thinkingDurationMs = Date.now() - state.spinnerState.phaseStartedAt
      const compactLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs)
      const expLines = buildThinkingSummary(state.streamingThinkingText, thinkingDurationMs, true)
      commitLines.push(...compactLines)
      if (!expandedCommitLines) expandedCommitLines = []
      expandedCommitLines.push(...expLines)
      state = { ...state, streamingThinkingText: '', pendingThinkingText: '' }
    }

    const thinkingDelta = p.thinking_delta as string | undefined
    if (thinkingDelta) {
      // Anthropic/pi preserves content blocks by index. Our public
      // AssistantDelta event currently drops that index, so the TUI only knows
      // whether visible text has already started. A thinking delta after text
      // has begun is almost certainly an upstream/proxy block-classification
      // glitch (seen with prose that literally mentions `<think>`); treating it
      // as hidden reasoning would tear the visible markdown in half. Preserve
      // it as assistant text instead.
      const visibleTextStarted = state.prefixEmitted || state.streamingText.replace(/^[\n\r]+/, '').length > 0
      if (visibleTextStarted) {
        appendVisibleTextDelta(thinkingDelta)
      } else {
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
    }

    const delta = p.delta as string | undefined
    if (delta) {
      // When first text delta arrives after thinking, flush thinking content
      flushThinkingBeforeText()
      appendVisibleTextDelta(delta)
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
    const message = (p.message as string) ?? 'Unknown error'
    // Skip the standalone `Error:` line if an LLM error card already showed
    // this same message (the provider error surfaces via both events).
    const alreadyShown = capturedLlmError != null &&
      (message.trim() === capturedLlmError || message.includes(capturedLlmError) || capturedLlmError.includes(message.trim()))
    if (alreadyShown) writeLines.push(...buildError(message))
    else commitLines.push(...buildError(message))
  }

  if (event.kind === 'run_finished') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    commitLines.push(...buildRunSummary(state.appState.currentRunStats))
  }

  return {
    state: { ...state, lastLlmErrorMessage: capturedLlmError },
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

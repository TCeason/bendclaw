import { buildError, buildVerboseEvent, buildLlmCard, isVisibleLlmEvent, type OutputLine } from '../../render/output.js'
import { formatDuration } from '../../render/format.js'
import { recordStreamDelta, resetStreamStats, setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { assistantToolCalls } from './assistant-content.js'
import { assistantMessageToOutputLines } from '../../render/assistant.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'

export interface StreamMachineState {
  appState: AppState
  spinnerState: SpinnerState
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
    activeLlmCall: false,
    lastLlmErrorMessage: null,
  }
}

export function reduceRunEvent(prev: StreamMachineState, event: RunEvent, _ctx: StreamContext): StreamUpdate {
  const p = (event.payload ?? {}) as Record<string, any>
  let state = event.kind === 'host_tool_call' ? prev : { ...prev, appState: applyEvent(prev.appState, event) }
  const commitLines: OutputLine[] = []
  const writeLines: OutputLine[] = []
  let expandedCommitLines: OutputLine[] | undefined
  let rerenderStatus = false
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
    const activeLlmCall = event.kind === 'llm_call_started' || event.kind === 'llm_call_retry' || event.kind === 'api_retry'
    state = {
      ...flushed.state,
      activeLlmCall,
      spinnerState: activeLlmCall ? resetStreamStats(flushed.state.spinnerState) : flushed.state.spinnerState,
    }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      routeVerbose(evt.text, { commit: commitLines, write: writeLines })
    }
  }

  if (event.kind === 'assistant_delta') {
    if (p.content_type === 'text') {
      const textDelta = p.delta as string | undefined
      if (textDelta) {
        state = {
          ...state,
          spinnerState: recordStreamDelta(state.spinnerState, textDelta),
        }
        rerenderStatus = true
      }
    } else {
      const thinkingDelta = p.delta as string | undefined
      if (thinkingDelta) {
        state = {
          ...state,
          spinnerState: recordStreamDelta(state.spinnerState, thinkingDelta, Date.now()),
        }
        rerenderStatus = true
      }
    }
  }

  if (event.kind === 'assistant_tool_call') {
    const toolName = (p.tool_name as string) ?? ''
    if (toolName) {
      state = {
        ...state,
        spinnerState: setSpinnerPhase(state.spinnerState, 'executing', toolName),
      }
      rerenderStatus = true
    }
  }

  if (event.kind === 'assistant_completed') {
    // applyEvent has already replaced streamed blocks with the provider's
    // authoritative completed content. A tool-bearing assistant message stays
    // live while its tools execute, then repl commits the entire ordered block
    // atomically. Text-only messages can commit immediately.
    const hasToolCalls = state.appState.currentAssistantContent.some(block => block.type === 'tool_call')
    const flushed = hasToolCalls
      ? { state, lines: [] as OutputLine[], expandedLines: undefined }
      : flushStreaming(state)
    state = {
      ...flushed.state,
      activeLlmCall: false,
      spinnerState: { ...flushed.state.spinnerState, streaming: false },
    }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    // Surface an output-token truncation so a response cut off mid-sentence is
    // not mistaken for a clean finish. Mirrors pi's assistant-message length
    // notice. `resolved_max_tokens` clamps the budget to the window, so this
    // only fires on a genuine max-output-tokens stop.
    if (p.stop_reason === 'length') {
      const notice = buildError(
        'Model stopped because it reached the maximum output token limit. The response may be incomplete.',
      )
      commitLines.push(...notice)
      if (!expandedCommitLines) expandedCommitLines = []
      expandedCommitLines.push(...notice)
    }
    rerenderStatus = true
  }

  if (event.kind === 'turn_started') {
    // A normal turn starts after the previous assistant_completed flush. This
    // is only a fallback for interrupted or synthetic event sequences.
    const flushed = flushStreaming(state)
    state = {
      ...flushed.state,
      spinnerState: { ...flushed.state.spinnerState, streaming: false },
    }
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    rerenderStatus = true
  }

  if (event.kind === 'llm_call_completed') {
    // LLM accounting completes before tool execution and is not an assistant
    // content boundary. Keep any tool-bearing ordered message live.
    state = { ...state, activeLlmCall: false }
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      routeVerbose(evt.text, { commit: commitLines, write: writeLines })
    }
  }

  if (event.kind === 'context_compaction_completed') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      routeVerbose(evt.text, { commit: commitLines, write: writeLines })
    }
  }

  if (event.kind === 'tool_started') {
    const toolName = (p.tool_name as string) ?? 'unknown'
    const isAskUser = toolName === 'AskUser' || toolName === 'ask_user'
    const spinnerPhase = isAskUser ? 'thinking' : 'executing'
    state = {
      ...state,
      spinnerState: setSpinnerPhase(state.spinnerState, spinnerPhase, toolName),
    }
    rerenderStatus = true
  }

  if (event.kind === 'tool_progress') {
    const text = p.text as string | undefined
    const spill = text ? parseSpillProgress(text) : undefined
    if (spill) {
      commitLines.push(...buildSpillEventLines(spill, p.tool_name as string | undefined))
    }
    rerenderStatus = true
  }

  if (event.kind === 'tool_finished') {
    const toolCalls = assistantToolCalls(state.appState.currentAssistantContent)
    // Prefer a still-running tool; otherwise keep the next queued tool in the
    // executing phase so the footer never flickers back to Thinking… between
    // serial tool calls. Only fall back to thinking when nothing remains.
    const running = toolCalls.find(call => call.status === 'running' && call.startedAt !== undefined)
    const queued = running ? undefined : toolCalls.find(call => call.status === 'queued')
    const next = running ?? queued
    state = {
      ...state,
      spinnerState: next
        ? setSpinnerPhase(state.spinnerState, 'executing', next.name)
        : setSpinnerPhase(state.spinnerState, 'thinking'),
    }
    // Tool-bearing assistant messages stay live through execution. Commit the
    // complete ordered message when the last tool settles, exactly once.
    if (toolCalls.length > 0 && toolCalls.every(call => call.status === 'done' || call.status === 'error')) {
      const flushed = flushStreaming(state)
      state = flushed.state
      commitLines.push(...flushed.lines)
      mergeFlushExpanded(flushed)
    }
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
    // Do not let applyEvent discard partial content before an abnormal run end
    // gets its final preservation flush.
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    mergeFlushExpanded(flushed)
  }

  return {
    state: { ...state, lastLlmErrorMessage: capturedLlmError },
    commitLines,
    expandedCommitLines,
    writeLines,
    rerenderStatus,
  }
}

export function flushStreaming(state: StreamMachineState): { state: StreamMachineState; lines: OutputLine[]; expandedLines?: OutputLine[] } {
  const content = state.appState.currentAssistantContent
  const lines = assistantMessageToOutputLines(content)
  const expandedLines = lines.length > 0
    ? assistantMessageToOutputLines(content, true)
    : undefined

  const resetState = {
    ...state,
    appState: {
      ...state.appState,
      currentAssistantContent: [],
    },
  }
  if (lines.length === 0) return { state: resetState, lines: [] }

  return { state: resetState, lines, expandedLines }
}

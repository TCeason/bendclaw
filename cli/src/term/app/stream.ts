import { buildError, buildRunSummary, buildToolCall, buildToolProgress, buildToolResult, buildVerboseEvent, buildAssistantLines, buildThinkingLines, type OutputLine } from '../../render/output.js'
import { formatDuration } from '../../render/format.js'
import { findStreamingCommitPoint, findNaturalPlainTextCommitPoint, highlightCodeLine } from '../../render/markdown.js'
import { setSpinnerPhase, type SpinnerState } from '../spinner.js'
import { applyEvent } from './reducer.js'
import type { AppState } from './state.js'
import type { RunEvent } from '../../native/index.js'

const PACE_INTERVAL_MS = 90
let sepId = 0

// Commit at the last safe newline so the pending tail is always a single
// growing line (enables the renderer's setStatus append fast-path, which
// produces a true character-by-character typing effect on the last line).
//
// Narrow path: only kicks in when the head above the last newline is plain
// prose. Anything that changes shape with more context (lists, tables,
// headings, blockquotes, tree diagrams, indented code, open code fences)
// stays in the pending buffer so a later complete-block commit can render
// it with correct widths and alignment.
//
// Also: only commit when at least one character already sits after the
// trailing newline. Committing a line the moment `\n` arrives (before the
// next token's first glyph lands) leaves the status tail momentarily empty,
// which forces a cursorUp + eraseDown cycle that reads as "bottom flashes"
// on every newline. Holding the commit until the next line has content
// keeps the tail transition to a single-character diff → fast-path hit.
function findLineByLineCommitPoint(text: string): number {
  const idx = text.lastIndexOf('\n')
  if (idx < 0) return 0
  if (idx === text.length - 1) return 0
  const head = text.slice(0, idx + 1)
  if (!isPlainProseHead(head)) return 0
  return idx + 1
}

function isPlainProseHead(head: string): boolean {
  for (const rawLine of head.split('\n')) {
    const line = rawLine
    const trimmed = line.trimStart()
    if (!trimmed) continue
    if (/^(```|~~~)/.test(trimmed)) return false
    if (/^#{1,6}(?:\s|$)/.test(trimmed)) return false
    if (/^[-*+]\s+/.test(trimmed)) return false
    if (/^\d+\.\s+/.test(trimmed)) return false
    if (/^>\s?/.test(trimmed)) return false
    if (/^\|.*\|\s*$/.test(trimmed)) return false
    if (/^[│├└─]/.test(trimmed)) return false
    if (/^(?: {4}|\t)\S/.test(line)) return false
  }
  return true
}

const FENCE_LINE_RE = /^([ \t]*)(```+|~~~+)([^\n]*)$/

/**
 * One step of the streaming code-fence state machine.
 *
 * - Before a fence: detect an opening ``` line, commit any text before it
 *   as ordinary prose, then switch into fence mode.
 * - Inside a fence: commit each complete body line directly to the scroll
 *   area as a `code_line` (syntax-highlighted when a language is known),
 *   so the user sees code flow in line by line rather than all at once
 *   when the fence finally closes.
 * - At the closing ``` line: emit a trailing separator and leave fence mode.
 *
 * Returns `null` when no more progress is possible in the current
 * `streamingText`; callers loop until null so a single delta can process
 * "prose → open → body → close → prose" in one pass.
 */
function advanceCodeFence(state: StreamMachineState, commitLines: OutputLine[]): StreamMachineState | null {
  if (!state.codeFence) {
    // Find an opening ``` or ~~~ line somewhere in the streaming buffer.
    // Scanning line by line lets us commit any prose that precedes the
    // fence before switching into fence mode.
    const text = state.streamingText
    let cursor = 0
    while (cursor < text.length) {
      const nl = text.indexOf('\n', cursor)
      if (nl < 0) return null
      const line = text.slice(cursor, nl)
      const match = FENCE_LINE_RE.exec(line)
      if (match) {
        // Commit any prose ahead of the fence as a separate assistant
        // paragraph so it doesn't get swallowed by the unclosed-fence
        // repair logic in findStreamingCommitPoint.
        if (cursor > 0) {
          const prose = text.slice(0, cursor)
          const built = buildAssistantLines(prose)
          if (built.length > 0) {
            if (state.assistantCommitted) {
              commitLines.push({ id: `sep-${sepId++}`, kind: 'assistant', text: '' })
            }
            commitLines.push(...built)
          }
        }
        const lang = (match[3] ?? '').trim() || null
        let next: StreamMachineState = {
          ...state,
          streamingText: text.slice(nl + 1),
          codeFence: { lang, linesCommitted: 0 },
          assistantCommitted: true,
        }
        // Blank separator so the fence starts on its own row.
        if (state.assistantCommitted || cursor > 0) {
          commitLines.push({ id: `sep-${sepId++}`, kind: 'assistant', text: '' })
        }
        return next
      }
      cursor = nl + 1
    }
    return null
  }

  // Inside an open fence. Commit each fully-terminated body line, stopping
  // at the closing ``` line (if present).
  const { lang } = state.codeFence
  const text = state.streamingText
  let cursor = 0
  let linesCommitted = state.codeFence.linesCommitted
  let closed = false
  let closedAt = -1
  while (cursor < text.length) {
    const nl = text.indexOf('\n', cursor)
    if (nl < 0) break
    const line = text.slice(cursor, nl)
    if (FENCE_LINE_RE.test(line)) {
      closed = true
      closedAt = nl + 1
      break
    }
    const highlighted = highlightCodeLine(line, lang ?? undefined)
    commitLines.push({
      id: `code-${sepId++}`,
      kind: 'code_line',
      text: highlighted,
    })
    linesCommitted += 1
    cursor = nl + 1
  }

  if (cursor === 0 && !closed) return null

  if (closed) {
    commitLines.push({ id: `code-${sepId++}`, kind: 'code_line', text: '' })
    return {
      ...state,
      streamingText: text.slice(closedAt),
      codeFence: null,
    }
  }
  return {
    ...state,
    streamingText: text.slice(cursor),
    codeFence: { lang, linesCommitted },
  }
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
  lastPendingRender: number
  /** Reveal cursor into the *rendered* last line of streaming text. Only
   *  advances — never rewinds — so consecutive reveal prefixes are always
   *  extensions of each other. Resets when streamingText is cleared. */
  revealCursor: number
  /** When inside an open ``` fence, `lang` is the resolved language (or null
   *  for "no language tag") and `lineCount` tracks how many content lines
   *  have already been committed to the scroll area so flushStreaming knows
   *  to skip them if the stream ends mid-fence. While active, streamingText
   *  only holds the *partial last line* of the fence body (or the empty
   *  string between newlines). */
  codeFence: { lang: string | null; linesCommitted: number } | null
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
    lastPendingRender: 0,
    revealCursor: 0,
    codeFence: null,
  }
}

export function reduceRunEvent(prev: StreamMachineState, event: RunEvent, ctx: StreamContext): StreamUpdate {
  const p = (event.payload ?? {}) as Record<string, any>
  let state = event.kind === 'ask_user' ? prev : { ...prev, appState: applyEvent(prev.appState, event) }
  const commitLines: OutputLine[] = []
  const writeLines: OutputLine[] = []
  let rerenderStatus = false
  let suppressToolStarted = false
  let suppressToolFinished = false

  // Verbose events (LLM / COMPACT / SPILL) are always produced; the `verbose`
  // flag only controls whether they land in the TUI or only in screen.log.
  // Error and retry events are force-visible so the user always sees them.
  const verboseOn = prev.appState.verbose
  const pickVerboseTarget = (kind: string): OutputLine[] => {
    if (verboseOn) return commitLines
    if (kind === 'llm_retry' || kind === 'llm_error') return commitLines
    return writeLines
  }

  if (event.kind === 'llm_call_started' || event.kind === 'llm_call_retry' || event.kind === 'api_retry' || event.kind === 'context_compaction_started') {
    const flushed = flushStreaming(state)
    state = { ...flushed.state, toolProgress: '', lastToolProgress: '' }
    commitLines.push(...flushed.lines)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    for (const evt of newEvents) {
      const verboseLines = buildVerboseEvent(evt.text)
      const target = pickVerboseTarget(evt.kind)
      target.push(...verboseLines)
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
        const thinkingLines = buildThinkingLines(state.streamingThinkingText)
        commitLines.push(...thinkingLines)
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

      // Code fence streaming: commit each completed line inside an open
      // ``` fence directly to the scroll area so the user sees code flow
      // line by line, not a single burst at close time. Runs repeatedly
      // because a single delta can contain "prose → open fence → lines →
      // close fence → more prose". Anything unhandled falls through to the
      // markdown commit logic below.
      for (;;) {
        const advanced = advanceCodeFence(state, commitLines)
        if (!advanced) break
        state = advanced
      }

      // While inside an open code fence, skip normal markdown block
      // commits — the body lines are being committed incrementally by
      // advanceCodeFence.  The markdown commit-point logic would otherwise
      // try to re-commit the whole fence as a single rendered block,
      // producing duplicate output.
      if (!state.codeFence) {
        // Commit completed markdown blocks directly to scroll area. If markdown
        // has no safe block boundary yet, allow long plain-text prose to flow by
        // committing complete leading lines while keeping the active tail dynamic.
        const markdownCommitPoint = findStreamingCommitPoint(state.streamingText)
        const naturalCommitPoint = markdownCommitPoint > 0 ? 0 : findNaturalPlainTextCommitPoint(state.streamingText, ctx.termRows)
        // Fall back to committing at the last safe newline so the pending tail
        // is a single growing line — this is what enables the renderer's
        // setStatus append fast-path (character-by-character typing on the
        // last status line, no full-area repaint).
        const lineCommitPoint = (markdownCommitPoint > 0 || naturalCommitPoint > 0)
          ? 0
          : findLineByLineCommitPoint(state.streamingText)
        const commitPoint = markdownCommitPoint || naturalCommitPoint || lineCommitPoint
        if (commitPoint > 0) {
          const completed = state.streamingText.slice(0, commitPoint)
          const pending = state.streamingText.slice(commitPoint)
          const builtLines = buildAssistantLines(completed)
          if (markdownCommitPoint > 0 && state.assistantCommitted && builtLines.length > 0) {
            const sep: OutputLine = { id: `sep-${sepId++}`, kind: 'assistant', text: '' }
            commitLines.push(sep)
          }
          commitLines.push(...builtLines)
          state = { ...state, streamingText: pending, assistantCommitted: true }
        }

        // Force-split when pending text exceeds a fraction of the visible area
        // so content flows into the scroll zone (append) instead of staying in
        // the status area (re-render in place). Prefer markdown-safe boundaries,
        // then fall back to plain prose line boundaries.
        const pendingLineCount = state.streamingText.split('\n').length
        const forceThreshold = Math.max(4, Math.floor(ctx.termRows / 3))
        if (pendingLineCount > forceThreshold) {
          const markdownSplitAt = findStreamingCommitPoint(state.streamingText)
          const naturalSplitAt = markdownSplitAt > 0 ? 0 : findNaturalPlainTextCommitPoint(state.streamingText, ctx.termRows)
          const splitAt = markdownSplitAt || naturalSplitAt
          if (splitAt > 0 && splitAt < state.streamingText.length) {
            const chunk = state.streamingText.slice(0, splitAt)
            const rest = state.streamingText.slice(splitAt)
            const builtLines = buildAssistantLines(chunk)
            if (markdownSplitAt > 0 && state.assistantCommitted && builtLines.length > 0) {
              const sep: OutputLine = { id: `sep-${sepId++}`, kind: 'assistant', text: '' }
              commitLines.push(sep)
            }
            commitLines.push(...builtLines)
            state = { ...state, streamingText: rest, assistantCommitted: true }
          }
        }
      }

      // Update pendingText for status area so the dynamic tail shows the
      // growing current line incrementally. The actual typewriter reveal is
      // driven by repl.ts' tail reveal timer, not by token arrival cadence;
      // this keeps large provider chunks from appearing as whole-line bursts.
      const now = Date.now()
      const shouldPace = now - state.lastPendingRender >= PACE_INTERVAL_MS
      const revealCursor = state.streamingText.length === 0 ? 0 : state.revealCursor
      state = { ...state, pendingText: state.streamingText, revealCursor }
      if (shouldPace || state.streamingText.length === 0) {
        state = { ...state, lastPendingRender: now }
        rerenderStatus = true
      }
    }
  }

  if (event.kind === 'assistant_completed' || event.kind === 'turn_started') {
    const flushed = flushStreaming(state)
    state = {
      ...flushed.state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: { ...flushed.state.spinnerState, streaming: false },
    }
    commitLines.push(...flushed.lines)
    rerenderStatus = true
  }

  let expandedCommitLines: OutputLine[] | undefined

  if (event.kind === 'llm_call_completed' || event.kind === 'context_compaction_completed') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    const newEvents = state.appState.verboseEvents.slice(prev.appState.verboseEvents.length)
    // Error / retry flows must always surface, regardless of verbose flag.
    const hasForceVisible = newEvents.some(evt =>
      /^[↻✗]\s+LLM\b/.test(evt.text) || /^\[LLM\]\s+[↻✗]/.test(evt.text)
    )
    let hasExpanded = false
    for (const evt of newEvents) {
      const verboseLines = buildVerboseEvent(evt.text)
      const forceVisible = /^[↻✗]\s+LLM\b/.test(evt.text) || /^\[LLM\]\s+[↻✗]/.test(evt.text)
      const target = verboseOn || forceVisible ? commitLines : writeLines
      target.push(...verboseLines)
      if (evt.expandedText) hasExpanded = true
    }
    if (hasExpanded && (verboseOn || hasForceVisible)) {
      expandedCommitLines = [...flushed.lines]
      for (const evt of newEvents) {
        const expLines = buildVerboseEvent(evt.expandedText ?? evt.text)
        expandedCommitLines.push(...expLines)
      }
    }
  }

  if (event.kind === 'tool_started') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    const toolName = (p.tool_name as string) ?? 'unknown'
    // ask_user is waiting for user input, not "executing" — keep thinking phase
    const spinnerPhase = toolName === 'ask_user' ? 'thinking' : 'executing'
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, spinnerPhase, toolName),
    }
    suppressToolStarted = toolName === 'ask_user'
    rerenderStatus = true
  }

  if (event.kind === 'tool_progress') {
    const text = p.text as string | undefined
    if (text) {
      const spill = parseSpillProgress(text)
      if (spill) {
        const flushed = flushStreaming(state)
        state = { ...flushed.state, toolProgress: '', lastToolProgress: '' }
        commitLines.push(...flushed.lines)
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
    state = {
      ...state,
      toolProgress: '',
      lastToolProgress: '',
      spinnerState: setSpinnerPhase(state.spinnerState, 'thinking'),
    }
    suppressToolFinished = toolName === 'ask_user'
    rerenderStatus = true
  }

  if (event.kind === 'error') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
    writeLines.push(...flushed.lines)
    commitLines.push(...buildError((p.message as string) ?? 'Unknown error'))
  }

  if (event.kind === 'run_finished') {
    const flushed = flushStreaming(state)
    state = flushed.state
    commitLines.push(...flushed.lines)
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
  const mergedArgs = details?.diff ? { ...args, diff: details.diff } : args
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

export function buildToolStartedLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const previewCommand = p.preview_command as string | undefined
  return buildToolCall(toolName, (p.args as Record<string, unknown>) ?? {}, previewCommand, expanded)
}

export function buildToolProgressLines(event: RunEvent, expanded?: boolean): OutputLine[] {
  const p = (event.payload ?? {}) as Record<string, any>
  const toolName = (p.tool_name as string) ?? 'unknown'
  const text = (p.text as string) ?? ''
  const spill = parseSpillProgress(text)
  if (spill) return buildSpillEventLines(spill, toolName)
  return text ? buildToolProgress(toolName, text, expanded) : []
}

export function flushStreaming(state: StreamMachineState): { state: StreamMachineState; lines: OutputLine[] } {
  const lines: OutputLine[] = []

  // Flush any remaining thinking content first
  if (state.streamingThinkingText.trim()) {
    lines.push(...buildThinkingLines(state.streamingThinkingText))
  }

  if (state.codeFence) {
    // Stream ended inside an open fence — commit whatever body lines are
    // still buffered as code_line output (syntax-highlighted to match what
    // was streamed earlier), then drop the fence state. Don't re-run
    // buildAssistantLines on the same text or it would emit the body a
    // second time as a full markdown code block.
    const text = state.streamingText
    const { lang } = state.codeFence
    let cursor = 0
    while (cursor < text.length) {
      const nl = text.indexOf('\n', cursor)
      const end = nl < 0 ? text.length : nl
      const line = text.slice(cursor, end)
      if (FENCE_LINE_RE.test(line)) {
        // Treat a stray closing ``` as the fence close and stop.
        cursor = nl < 0 ? text.length : nl + 1
        break
      }
      if (line.length > 0 || nl >= 0) {
        const highlighted = highlightCodeLine(line, lang ?? undefined)
        lines.push({ id: `code-${sepId++}`, kind: 'code_line', text: highlighted })
      }
      if (nl < 0) { cursor = text.length; break }
      cursor = nl + 1
    }
    lines.push({ id: `code-${sepId++}`, kind: 'code_line', text: '' })
    const rest = text.slice(cursor)
    return {
      state: { ...state, streamingText: '', streamingThinkingText: '', pendingText: '', pendingThinkingText: '', assistantCommitted: false, codeFence: null },
      lines: rest.trim()
        ? lines.concat(buildAssistantLines(rest))
        : lines,
    }
  }

  if (state.streamingText.trim()) {
    const assistantLines = buildAssistantLines(state.streamingText)
    if (state.assistantCommitted && assistantLines.length > 0) {
      lines.unshift({ id: `sep-${sepId++}`, kind: 'assistant', text: '' })
    }
    lines.push(...assistantLines)
  }

  if (lines.length === 0) {
    return {
      state: { ...state, streamingText: '', streamingThinkingText: '', pendingText: '', pendingThinkingText: '', assistantCommitted: false, codeFence: null },
      lines: [],
    }
  }

  return {
    state: { ...state, streamingText: '', streamingThinkingText: '', pendingText: '', pendingThinkingText: '', assistantCommitted: false, codeFence: null },
    lines,
  }
}

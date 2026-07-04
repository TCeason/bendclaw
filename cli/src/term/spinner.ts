/**
 * Spinner — ANSI-based animated loading indicator.
 * Pure logic: returns the string to display, no React.
 */

function getSpinnerChars(): string[] {
  if (process.env.TERM === 'xterm-ghostty') {
    return ['·', '✢', '✳', '✶', '✻', '*']
  }
  return process.platform === 'darwin'
    ? ['·', '✢', '✳', '✶', '✻', '✽']
    : ['·', '✢', '*', '✶', '✻', '✽']
}

const SPINNER_CHARS = getSpinnerChars()
const SPINNER_FRAMES = [...SPINNER_CHARS, ...[...SPINNER_CHARS].reverse()]
const SLOW_THRESHOLD_MS = 8000

export type SpinnerPhase = 'thinking' | 'executing'

/**
 * Map a tool name to a human action label shown on the spinner while it runs.
 * Grouped into verbs that mirror the tool-card glyphs (read/search/edit/...).
 * Unknown tools fall back to a generic "Working".
 */
export function toolActionLabel(toolName: string): string {
  switch (toolName.toLowerCase()) {
    case 'read': case 'read_code': return 'Reading'
    case 'grep': case 'glob': case 'find': case 'search': case 'semantic_code_search': return 'Searching'
    case 'edit': case 'file_edit': case 'write': case 'file_write': return 'Editing'
    case 'bash': return 'Running'
    case 'web_fetch': case 'webfetch': return 'Fetching'
    case 'update_goal_tasks': case 'todowrite': return 'Planning'
    case 'skill': return 'Loading skill'
    case 'ask_user': case 'askuser': return 'Waiting for you'
    default: return 'Working'
  }
}

export interface SpinnerState {
  frame: number
  phase: SpinnerPhase
  phaseStartedAt: number
  lastTokenAt: number | null
  streaming: boolean
  toolName: string | null
  tokenCount: number
  streamStartedAt: number | null
  glimmerPos: number
}

export function createSpinnerState(): SpinnerState {
  return {
    frame: 0,
    phase: 'thinking',
    phaseStartedAt: Date.now(),
    lastTokenAt: null,
    streaming: false,
    toolName: null,
    tokenCount: 0,
    streamStartedAt: null,
    glimmerPos: -2,
  }
}

export function advanceSpinner(state: SpinnerState): SpinnerState {
  return {
    ...state,
    frame: (state.frame + 1) % SPINNER_FRAMES.length,
    glimmerPos: state.glimmerPos + 1 > 30 ? -2 : state.glimmerPos + 1,
  }
}

export function setSpinnerPhase(state: SpinnerState, phase: SpinnerPhase, toolName?: string | null): SpinnerState {
  if (state.phase === phase && state.toolName === (toolName ?? null)) return state
  return {
    ...state,
    phase,
    phaseStartedAt: Date.now(),
    toolName: toolName ?? null,
  }
}

export function recordStreamDelta(state: SpinnerState, textDelta: string, now = Date.now()): SpinnerState {
  const tokens = estimateTokens(textDelta)
  return {
    ...state,
    lastTokenAt: now,
    streaming: true,
    tokenCount: state.tokenCount + tokens,
    streamStartedAt: state.streamStartedAt ?? now,
  }
}

export function resetStreamStats(state: SpinnerState): SpinnerState {
  return {
    ...state,
    lastTokenAt: null,
    streaming: false,
    tokenCount: 0,
    streamStartedAt: null,
  }
}

export function isSlow(state: SpinnerState, now: number): boolean {
  if (state.streaming) return false
  const elapsed = now - state.phaseStartedAt
  if (elapsed <= SLOW_THRESHOLD_MS) return false
  if (state.phase === 'thinking' && state.lastTokenAt != null) {
    return (now - state.lastTokenAt) > SLOW_THRESHOLD_MS
  }
  return true
}

export interface SpinnerStats {
  inputTokens?: number
  outputTokens?: number
  cacheReadTokens?: number
}

export function formatSpinnerLine(state: SpinnerState, now: number, stats?: SpinnerStats): string {
  const elapsed = now - state.phaseStartedAt
  const slow = isSlow(state, now)
  const char = SPINNER_FRAMES[state.frame]!

  const isTool = state.phase === 'executing'
  const action = state.toolName ? toolActionLabel(state.toolName) : 'Working'
  let label: string
  if (slow) {
    label = isTool ? `${action} slow…` : 'LLM slow…'
  } else {
    label = isTool ? `${action}…` : 'Thinking…'
  }

  const status = formatFixedDuration(elapsed)
  const tokenSuffix = formatSpinnerTokenSuffix(state, now, stats)

  if (slow) {
    return `\x1b[31m${char}\x1b[0m \x1b[31m${label}\x1b[0m\x1b[2m (${status}${tokenSuffix}) · esc to interrupt\x1b[0m`
  }

  const glimmerLabel = glimmerText(label, state.glimmerPos)
  return `\x1b[36m${char}\x1b[0m ${glimmerLabel}\x1b[2m (${status}${tokenSuffix}) · esc to interrupt\x1b[0m`
}

function glimmerText(text: string, pos: number): string {
  const start = pos - 1
  const end = pos + 1
  let result = ''
  for (let i = 0; i < text.length; i++) {
    if (i >= start && i <= end) {
      result += `\x1b[1;37m${text[i]}\x1b[0m`
    } else {
      result += `\x1b[2m${text[i]}\x1b[0m`
    }
  }
  return result
}

function formatFixedDuration(ms: number): string {
  return humanDuration(ms).padStart(5)
}

function humanDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  const secs = Math.floor(ms / 100) / 10
  if (secs < 60) return `${secs.toFixed(1)}s`
  const totalSecs = Math.floor(ms / 1000)
  const mins = Math.floor(totalSecs / 60)
  const rem = totalSecs % 60
  return rem > 0 ? `${mins}m${rem}s` : `${mins}m`
}

function formatSpinnerTokenSuffix(state: SpinnerState, now: number, stats?: SpinnerStats): string {
  const inputTokens = stats?.inputTokens ?? 0
  const outputTokens = stats?.outputTokens ?? 0
  const cacheReadTokens = stats?.cacheReadTokens ?? 0
  const liveRate = formatLiveTokPerSec(state, now)
  if (inputTokens > 0 || outputTokens > 0 || cacheReadTokens > 0) {
    const parts: string[] = []
    if (inputTokens > 0) parts.push(`↑${formatTokens(inputTokens)}`)
    if (outputTokens > 0) parts.push(`↓${formatTokens(outputTokens)}`)
    if (cacheReadTokens > 0) parts.push(`cache ${formatCacheHitPercent(inputTokens, cacheReadTokens)}`)
    return ` · ${parts.join(' ')}${liveRate ? ` · ${liveRate}` : ''}`
  }
  const tokenSuffix = state.tokenCount > 0 ? ` · ↓ ${formatTokens(state.tokenCount)} tokens` : ''
  return `${tokenSuffix}${liveRate ? ` · ${liveRate}` : ''}`
}

function formatLiveTokPerSec(state: SpinnerState, now: number): string {
  if (!state.streaming || state.phase !== 'thinking' || !state.streamStartedAt || state.tokenCount <= 0) return ''
  const elapsedMs = Math.max(0, now - state.streamStartedAt)
  if (elapsedMs < 500) return ''
  const rate = state.tokenCount / (elapsedMs / 1000)
  if (!Number.isFinite(rate) || rate <= 0) return ''
  return `~${Math.round(rate)} tok/s`
}
function formatCacheHitPercent(inputTokens: number, cacheReadTokens: number): string {
  const total = inputTokens + cacheReadTokens
  if (total <= 0) return '0%'
  return `${Math.round(cacheReadTokens / total * 100)}%`
}

function estimateTokens(text: string): number {
  if (!text) return 0
  return Math.max(1, Math.round(text.length / 4))
}

function formatTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`
  if (count < 1000000) return `${Math.round(count / 1000)}k`
  if (count < 10000000) return `${(count / 1000000).toFixed(1)}M`
  return `${Math.round(count / 1000000)}M`
}

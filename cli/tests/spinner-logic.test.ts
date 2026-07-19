import { describe, test, expect } from 'bun:test'
import {
  createSpinnerState,
  advanceSpinner,
  setSpinnerPhase,
  recordStreamDelta,
  isSlow,
  formatSpinnerLine,
  spinnerStatsFromLastUsage,
} from '../src/term/spinner.js'
import stripAnsi from 'strip-ansi'

describe('createSpinnerState', () => {
  test('creates initial state', () => {
    const state = createSpinnerState()
    expect(state.frame).toBe(0)
    expect(state.phase).toBe('preparing')
    expect(state.streaming).toBe(false)
    expect(state.toolName).toBeNull()
    expect(state.tokenCount).toBe(0)
  })
})

describe('advanceSpinner', () => {
  test('increments frame', () => {
    const state = createSpinnerState()
    const next = advanceSpinner(state)
    expect(next.frame).toBe(1)
  })

  test('wraps around at end of frames', () => {
    let state = createSpinnerState()
    // Advance through all frames (12 total: 6 + 6 reversed)
    for (let i = 0; i < 12; i++) {
      state = advanceSpinner(state)
    }
    expect(state.frame).toBe(0)
  })

  test('does not mutate other fields', () => {
    const state = { ...createSpinnerState(), tokenCount: 42 }
    const next = advanceSpinner(state)
    expect(next.tokenCount).toBe(42)
    expect(next.phase).toBe('preparing')
  })
})

describe('setSpinnerPhase', () => {
  test('changes phase to executing', () => {
    const state = createSpinnerState()
    const next = setSpinnerPhase(state, 'executing', 'bash')
    expect(next.phase).toBe('executing')
    expect(next.toolName).toBe('bash')
  })

  test('changes phase to thinking', () => {
    let state = createSpinnerState()
    state = setSpinnerPhase(state, 'executing', 'bash')
    const next = setSpinnerPhase(state, 'thinking')
    expect(next.phase).toBe('thinking')
    expect(next.toolName).toBeNull()
  })
  test('resets phaseStartedAt on change', () => {
    const state = { ...createSpinnerState(), phaseStartedAt: 1000 }
    const next = setSpinnerPhase(state, 'executing', 'read')
    expect(next.phaseStartedAt).toBeGreaterThan(1000)
  })

  test('returns same state if phase unchanged', () => {
    const state = createSpinnerState()
    const next = setSpinnerPhase(state, 'preparing')
    expect(next).toBe(state) // same reference
  })
})

describe('isSlow', () => {
  test('not slow when just started', () => {
    const state = createSpinnerState()
    expect(isSlow(state, Date.now())).toBe(false)
  })

  test('slow after threshold with no tokens', () => {
    const state = { ...createSpinnerState(), phaseStartedAt: Date.now() - 9000 }
    expect(isSlow(state, Date.now())).toBe(true)
  })

  test('not slow when streaming', () => {
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: Date.now() - 9000,
      streaming: true,
    }
    expect(isSlow(state, Date.now())).toBe(false)
  })

  test('not slow when recent tokens received while emitting', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'thinking' as const,
      phaseStartedAt: now - 9000,
      lastTokenAt: now - 1000, // 1s ago — recent
    }
    expect(isSlow(state, now)).toBe(false)
  })

  test('slow when the stream stalls (stale tokens)', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'responding' as const,
      phaseStartedAt: now - 9000,
      lastTokenAt: now - 9000, // 9s ago — stalled
      streaming: true,
    }
    expect(isSlow(state, now)).toBe(true)
  })

  test('slow in executing phase after threshold', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'executing' as const,
      phaseStartedAt: now - 9000,
      toolName: 'edit',
    }
    expect(isSlow(state, now)).toBe(true)
  })

  test('long-running tools use wider slow thresholds', () => {
    const now = Date.now()
    const executing = (toolName: string, elapsedMs: number) => ({
      ...createSpinnerState(),
      phase: 'executing' as const,
      phaseStartedAt: now - elapsedMs,
      toolName,
    })
    // bash regularly outlives 8s (builds, tests) — slow only after 30s.
    expect(isSlow(executing('bash', 9_000), now)).toBe(false)
    expect(isSlow(executing('bash', 31_000), now)).toBe(true)
    // compact includes an LLM summarization pass with a 30s budget.
    expect(isSlow(executing('compact', 29_000), now)).toBe(false)
    expect(isSlow(executing('compact', 31_000), now)).toBe(true)
    // Waiting on the user is never "slow".
    expect(isSlow(executing('ask_user', 3_600_000), now)).toBe(false)
  })
})

describe('formatSpinnerLine', () => {
  test('labels each run phase', () => {
    const now = Date.now()
    const at = (phase: 'preparing' | 'waiting' | 'thinking' | 'responding') => ({
      ...createSpinnerState(),
      phase,
    })
    expect(stripAnsi(formatSpinnerLine(at('preparing'), now))).toContain('Preparing…')
    expect(stripAnsi(formatSpinnerLine(at('waiting'), now))).toContain('Waiting for model…')
    expect(stripAnsi(formatSpinnerLine(at('thinking'), now))).toContain('Thinking…')
    expect(stripAnsi(formatSpinnerLine(at('responding'), now))).toContain('Responding…')
  })

  test('contains action label when executing', () => {
    const state = setSpinnerPhase(createSpinnerState(), 'executing', 'bash')
    const line = stripAnsi(formatSpinnerLine(state, Date.now()))
    expect(line).toContain('Running command…')
  })

  test('maps tool names to action verbs', () => {
    const cases: [string, string][] = [
      ['read', 'Reading…'],
      ['grep', 'Searching…'],
      ['edit', 'Applying changes…'],
      ['write', 'Writing file…'],
      ['web_fetch', 'Fetching…'],
      ['plan', 'Planning…'],
      ['skill', 'Loading skill…'],
      ['ask_user', 'Waiting for you…'],
      ['some_unknown_tool', 'Working…'],
    ]
    for (const [tool, label] of cases) {
      const state = setSpinnerPhase(createSpinnerState(), 'executing', tool)
      const line = stripAnsi(formatSpinnerLine(state, Date.now()))
      expect(line).toContain(label)
    }
  })

  test('contains slow label after threshold', () => {
    const now = Date.now()
    const waiting = { ...createSpinnerState(), phase: 'waiting' as const, phaseStartedAt: now - 9000 }
    expect(stripAnsi(formatSpinnerLine(waiting, now))).toContain('LLM slow…')
    const preparing = { ...createSpinnerState(), phaseStartedAt: now - 9000 }
    expect(stripAnsi(formatSpinnerLine(preparing, now))).toContain('Preparing slow…')
  })

  test('labels a stalled stream', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'responding' as const,
      streaming: true,
      lastTokenAt: now - 9000,
      phaseStartedAt: now - 20000,
    }
    expect(stripAnsi(formatSpinnerLine(state, now))).toContain('Stream stalled…')
  })

  test('contains action slow label', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phase: 'executing' as const,
      phaseStartedAt: now - 31000,
      toolName: 'bash',
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('Running command slow…')
  })

  test('contains duration', () => {
    const now = Date.now()
    const state = { ...createSpinnerState(), phaseStartedAt: now - 2500 }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('2.5s')
  })

  test('contains esc to interrupt hint', () => {
    const state = createSpinnerState()
    const line = stripAnsi(formatSpinnerLine(state, Date.now()))
    expect(line).toContain('esc to interrupt')
  })

  test('shows token count after 30s', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 35000,
      tokenCount: 1500,
      streaming: true, // prevent slow
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('1.5k tokens')
  })

  test('shows token count with arrow even before 30s', () => {
    const now = Date.now()
    const state = {
      ...createSpinnerState(),
      phaseStartedAt: now - 5000,
      tokenCount: 100,
    }
    const line = stripAnsi(formatSpinnerLine(state, now))
    expect(line).toContain('↓ 100 tokens')
  })

  test('shows last-call token stats with absolute cache amount when provided', () => {
    const now = Date.now()
    const state = { ...createSpinnerState(), phaseStartedAt: now - 5000 }
    const line = stripAnsi(formatSpinnerLine(state, now, {
      inputTokens: 408000,
      outputTokens: 1100,
      cacheReadTokens: 89000,
    }))
    // cache% = 89k / (408k + 89k) ≈ 18%; absolute read is shown so a high
    // percentage can be sanity-checked against the real volume (pi: CH% from
    // latest call + R absolute separately).
    expect(line).toContain('↑408k ↓1.1k cache 89k 18%')
    expect(line).not.toContain('tokens')
  })

  test('cache hit percent includes cache-write tokens in the denominator', () => {
    const now = Date.now()
    const state = { ...createSpinnerState(), phaseStartedAt: now - 5000 }
    // 80 read / (10 + 80 + 10) = 80% — same formula as pi CH%
    const line = stripAnsi(formatSpinnerLine(state, now, {
      inputTokens: 10_000,
      outputTokens: 100,
      cacheReadTokens: 80_000,
      cacheWriteTokens: 10_000,
    }))
    expect(line).toContain('cache 80k 80%')
  })

  test('spinnerStatsFromLastUsage hides prior usage until the active call completes', () => {
    const last = {
      inputTokens: 12_000,
      outputTokens: 800,
      cacheReadTokens: 450_000,
      cacheWriteTokens: 0,
    }
    expect(spinnerStatsFromLastUsage(last)).toEqual({
      inputTokens: 12_000,
      outputTokens: 800,
      cacheReadTokens: 450_000,
      cacheWriteTokens: 0,
    })
    expect(spinnerStatsFromLastUsage(last, 320, true)).toEqual({
      inputTokens: 0,
      outputTokens: 320,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    })
    expect(spinnerStatsFromLastUsage(last, 0, true)).toEqual({
      inputTokens: 0,
      outputTokens: 0,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    })
    expect(spinnerStatsFromLastUsage(null, 50, true)).toEqual({
      inputTokens: 0,
      outputTokens: 50,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
    })
  })

  test('shows live tok/s while streaming text', () => {
    const start = 10_000
    let state = setSpinnerPhase(createSpinnerState(), 'responding')
    state = recordStreamDelta(state, 'x'.repeat(400), start)
    const line = stripAnsi(formatSpinnerLine(state, start + 2000))
    expect(line).toContain('↓ 100 tokens')
    expect(line).toContain('~50 tok/s')
  })
})

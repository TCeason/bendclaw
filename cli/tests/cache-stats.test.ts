import { describe, test, expect } from 'bun:test'
import {
  CACHE_TTL_MS,
  detectCacheMiss,
  formatCacheHitPercent,
  formatCacheMissNotice,
  nextPromptCacheSnapshot,
  type PromptCacheSnapshot,
} from '../src/render/cache.js'
import { applyEvent } from '../src/term/app/reducer.js'
import { createInitialState, type AppState } from '../src/term/app/state.js'
import { isVisibleEvent, buildEventCard } from '../src/render/output.js'
import type { RunEvent } from '../src/native/index.js'

// ---------------------------------------------------------------------------
// formatCacheHitPercent
// ---------------------------------------------------------------------------

describe('formatCacheHitPercent', () => {
  test('zero prompt or zero read shows 0', () => {
    expect(formatCacheHitPercent(0, 0, 0)).toBe('0')
    expect(formatCacheHitPercent(5000, 0, 1000)).toBe('0')
  })

  test('integer percent below 99', () => {
    expect(formatCacheHitPercent(408_000, 89_000, 0)).toBe('18')
    expect(formatCacheHitPercent(10_000, 80_000, 10_000)).toBe('80')
  })

  test('one decimal in [99, 100) so a near-hit is not rounded to 100', () => {
    // 200000 / 200504 = 99.7487…% — Math.round would show a fake 100%.
    expect(formatCacheHitPercent(4, 200_000, 500)).toBe('99.7')
    // 200000 / 200004 = 99.998% — still not a full hit.
    expect(formatCacheHitPercent(4, 200_000, 0)).toBe('99.9')
  })

  test('100 only when every billed prompt token was a cache read', () => {
    expect(formatCacheHitPercent(0, 150_000, 0)).toBe('100')
  })
})

// ---------------------------------------------------------------------------
// Cache miss detection (pi cache-stats semantics)
// ---------------------------------------------------------------------------

function snapshot(overrides: Partial<PromptCacheSnapshot> = {}): PromptCacheSnapshot {
  return {
    promptTokens: 100_000,
    usage: { inputTokens: 4, cacheReadTokens: 99_196, cacheWriteTokens: 800 },
    model: 'claude-fable-5',
    timestamp: 1_000_000,
    reportedCache: true,
    ...overrides,
  }
}

describe('detectCacheMiss', () => {
  test('first call has no previous request to miss against', () => {
    const usage = { inputTokens: 50_000, cacheReadTokens: 0, cacheWriteTokens: 0 }
    expect(detectCacheMiss(null, usage, 'm', 2_000_000)).toBeNull()
  })

  test('steady-state loop stays below the noise floor', () => {
    // Whole previous prompt read back; only the new delta was written.
    const usage = { inputTokens: 4, cacheReadTokens: 100_000, cacheWriteTokens: 800 }
    expect(detectCacheMiss(snapshot(), usage, 'claude-fable-5', 1_060_000)).toBeNull()
  })

  test('TTL-expired prompt re-billed as cache write is a miss with idle gap', () => {
    const idleMs = CACHE_TTL_MS + 120_000
    const usage = { inputTokens: 4, cacheReadTokens: 0, cacheWriteTokens: 100_800 }
    const miss = detectCacheMiss(snapshot(), usage, 'claude-fable-5', 1_000_000 + idleMs)
    expect(miss).toEqual({ missedTokens: 100_000, idleMs, modelChanged: false })
  })

  test('model switch re-bills the prompt and is flagged', () => {
    const usage = { inputTokens: 2_000, cacheReadTokens: 0, cacheWriteTokens: 99_000 }
    const miss = detectCacheMiss(snapshot(), usage, 'gpt-5.2', 1_010_000)
    expect(miss?.modelChanged).toBe(true)
    expect(miss?.missedTokens).toBe(100_000)
  })

  test('zero-cache call counts only when cache activity was reported before', () => {
    const usage = { inputTokens: 100_000, cacheReadTokens: 0, cacheWriteTokens: 0 }
    // Provider never reported caching: not a miss, it just doesn't report.
    expect(detectCacheMiss(snapshot({ reportedCache: false }), usage, 'm', 1_010_000)).toBeNull()
    // Cache-read-only provider (OpenAI-style) that reported before: total miss.
    const miss = detectCacheMiss(snapshot(), usage, 'claude-fable-5', 1_010_000)
    expect(miss?.missedTokens).toBe(100_000)
  })

  test('missed tokens are capped by the smaller of the two prompts', () => {
    // Post-shrink prompt (e.g. edited history): only its own size can miss.
    const usage = { inputTokens: 30_000, cacheReadTokens: 0, cacheWriteTokens: 0 }
    const miss = detectCacheMiss(snapshot(), usage, 'claude-fable-5', 1_010_000)
    expect(miss?.missedTokens).toBe(30_000)
  })
})

describe('nextPromptCacheSnapshot', () => {
  test('rolls forward and keeps reportedCache sticky', () => {
    const usage = { inputTokens: 10_000, cacheReadTokens: 0, cacheWriteTokens: 40_000 }
    const first = nextPromptCacheSnapshot(null, usage, 'm1', 500)
    expect(first).toEqual({ promptTokens: 50_000, usage, model: 'm1', timestamp: 500, reportedCache: true })
    const second = nextPromptCacheSnapshot(
      first,
      { inputTokens: 51_000, cacheReadTokens: 0, cacheWriteTokens: 0 },
      'm1',
      900,
    )
    expect(second?.reportedCache).toBe(true)
  })

  test('calls without prompt usage keep the previous snapshot', () => {
    const prev = snapshot()
    const next = nextPromptCacheSnapshot(
      prev,
      { inputTokens: 0, cacheReadTokens: 0, cacheWriteTokens: 0 },
      'm',
      2_000_000,
    )
    expect(next).toBe(prev)
  })
})

describe('formatCacheMissNotice', () => {
  test('small misses stay log-only', () => {
    expect(formatCacheMissNotice({ missedTokens: 19_000, idleMs: 0, modelChanged: false })).toBeNull()
  })

  test('states the re-billed volume and the observable cause', () => {
    expect(formatCacheMissNotice({ missedTokens: 98_000, idleMs: 0, modelChanged: false }))
      .toBe('[LLM] ⚠ · cache miss · 98k tokens re-billed')
    expect(formatCacheMissNotice({ missedTokens: 98_000, idleMs: 7 * 60_000, modelChanged: false }))
      .toBe('[LLM] ⚠ · cache miss after 7m idle · 98k tokens re-billed')
    expect(formatCacheMissNotice({ missedTokens: 98_000, idleMs: 10_000, modelChanged: true }))
      .toBe('[LLM] ⚠ · cache miss after model switch · 98k tokens re-billed')
  })

  test('notice renders as a visible tool-style card', () => {
    const notice = formatCacheMissNotice({ missedTokens: 98_000, idleMs: 7 * 60_000, modelChanged: false })
    expect(notice).not.toBeNull()
    if (notice === null) return
    expect(isVisibleEvent(notice)).toBe(true)
    const card = buildEventCard(notice)
    expect(card[0]?.text).toBe('✦ llm  cache miss after 7m idle')
    expect(card[1]?.text).toBe('  ⚠ · 98k tokens re-billed')
  })
})

// ---------------------------------------------------------------------------
// Reducer integration
// ---------------------------------------------------------------------------

function llmCompleted(usage: { input: number; cache_read: number; cache_write: number; output?: number }): RunEvent {
  return {
    kind: 'llm_call_completed',
    session_id: 's',
    event_id: 'e',
    turn: 1,
    payload: { model: 'claude-fable-5', usage: { output: 100, ...usage } },
  } as unknown as RunEvent
}

describe('reducer prompt-cache tracking', () => {
  test('accumulates cache writes into session totals and rolls the snapshot', () => {
    let state: AppState = createInitialState('claude-fable-5', '/tmp')
    state = applyEvent(state, llmCompleted({ input: 10_000, cache_read: 0, cache_write: 40_000 }))
    expect(state.sessionTokens.cacheWriteTokens).toBe(40_000)
    expect(state.promptCache?.promptTokens).toBe(50_000)
    expect(state.promptCache?.reportedCache).toBe(true)

    state = applyEvent(state, llmCompleted({ input: 4, cache_read: 50_000, cache_write: 900 }))
    expect(state.sessionTokens.cacheWriteTokens).toBe(40_900)
    expect(state.promptCache?.promptTokens).toBe(50_904)
    // Steady-state follow-up: no miss notice.
    expect(state.verboseEvents.some(e => e.text.includes('cache miss'))).toBe(false)
  })

  test('emits a visible miss notice when a large prompt is re-billed', () => {
    let state: AppState = createInitialState('claude-fable-5', '/tmp')
    state = applyEvent(state, llmCompleted({ input: 4, cache_read: 100_000, cache_write: 900 }))
    state = applyEvent(state, llmCompleted({ input: 4, cache_read: 0, cache_write: 101_000 }))
    const notice = state.verboseEvents.find(e => e.text.includes('cache miss'))
    expect(notice?.text).toContain('tokens re-billed')
    expect(isVisibleEvent(notice?.text ?? '')).toBe(true)
  })

  test('compaction resets the snapshot so the next cold call is not a miss', () => {
    let state: AppState = createInitialState('claude-fable-5', '/tmp')
    state = applyEvent(state, llmCompleted({ input: 4, cache_read: 100_000, cache_write: 900 }))
    state = applyEvent(state, {
      kind: 'context_compaction_completed',
      session_id: 's',
      event_id: 'e2',
      turn: 1,
      payload: {
        result: { type: 'compacted', before_tokens: 100_000, after_tokens: 20_000, before_message_count: 30, after_message_count: 8 },
      },
    } as unknown as RunEvent)
    expect(state.promptCache).toBeNull()
    state = applyEvent(state, llmCompleted({ input: 500, cache_read: 0, cache_write: 20_000 }))
    expect(state.verboseEvents.some(e => e.text.includes('cache miss'))).toBe(false)
  })
})

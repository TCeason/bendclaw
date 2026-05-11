import { describe, test, expect, beforeEach } from 'bun:test'
import { buildToolResult, resetIdCounter } from '../src/render/output.js'
import type { SlimStats } from '../src/term/app/types.js'

beforeEach(() => {
  resetIdCounter()
})

function headerText(lines: { kind: string; text: string }[]): string {
  const header = lines.find((l) => l.kind === 'tool')
  if (!header) throw new Error('no tool header line')
  return header.text
}

function makeResult(slim?: SlimStats): { kind: string; text: string }[] {
  return buildToolResult(
    'bash',
    { command: 'git diff' },
    'done',
    'small output',
    42,
    false,
    slim,
  )
}

describe('tool result slim suffix', () => {
  test('no slim stats renders no suffix', () => {
    const text = headerText(makeResult())
    expect(text.includes('slim')).toBe(false)
    expect(text.includes('cache hit')).toBe(false)
  })

  test('off / raw_error / none produce no suffix', () => {
    for (const filter of ['off', 'raw_error', 'none']) {
      const text = headerText(
        makeResult({ filter, original: 1000, slimmed: 1000 }),
      )
      expect(text.includes('slim')).toBe(false)
    }
  })

  test('ack renders token range', () => {
    const text = headerText(
      makeResult({ filter: 'ack', original: 300, slimmed: 20 }),
    )
    expect(text.includes('· slim(ack) ~75→~5 tok −93%')).toBe(true)
  })

  test('cache_hit renders token range', () => {
    const text = headerText(
      makeResult({ filter: 'cache_hit', original: 4096, slimmed: 80 }),
    )
    expect(text.includes('· cache hit ~1.0k→~20 tok')).toBe(true)
  })

  test('git_diff renders filter token range and percentage when >= 10%', () => {
    const text = headerText(
      makeResult({ filter: 'git_diff', original: 1000, slimmed: 130 }),
    )
    expect(text.includes('· slim(git_diff) ~250→~33 tok −87%')).toBe(true)
  })

  test('saving under 10% is silent', () => {
    const text = headerText(
      makeResult({ filter: 'git_diff', original: 1000, slimmed: 950 }),
    )
    expect(text.includes('slim')).toBe(false)
  })

  test('error status still renders slim suffix', () => {
    // Engine won't normally attach slim on errors (raw_error) but the
    // renderer must be robust if it does.
    const lines = buildToolResult(
      'bash',
      { command: 'git diff' },
      'error',
      'boom',
      17,
      false,
      { filter: 'tail', original: 20000, slimmed: 4000 },
    )
    const text = headerText(lines)
    expect(text.startsWith('[BASH] ✗')).toBe(true)
    expect(text.includes('· slim(tail) ~5.0k→~1.0k tok −80%')).toBe(true)
  })
})

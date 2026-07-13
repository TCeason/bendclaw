import { describe, expect, test } from 'bun:test'
import { formatQueuedMessageLines } from '../src/term/viewmodel/queued-messages.js'

describe('formatQueuedMessageLines', () => {
  test('returns empty when there is nothing queued', () => {
    expect(formatQueuedMessageLines([])).toEqual([])
    expect(formatQueuedMessageLines(['', '  '])).toEqual([])
  })

  test('labels each message and adds an esc restore hint', () => {
    expect(formatQueuedMessageLines(['fix the layout', 'then ship'])).toEqual([
      'Queued: fix the layout',
      'Queued: then ship',
      '↳ esc to pull back into input',
    ])
  })

  test('collapses whitespace and truncates long lines', () => {
    const long = 'word '.repeat(40).trim()
    const lines = formatQueuedMessageLines([`  multi\nline\tmsg  `, long], { maxChars: 40 })
    expect(lines[0]).toBe('Queued: multi line msg')
    expect(lines[1]!.startsWith('Queued: ')).toBe(true)
    expect(lines[1]!.endsWith('…')).toBe(true)
    expect(lines[1]!.length).toBeLessThanOrEqual('Queued: '.length + 40)
    expect(lines[2]).toBe('↳ esc to pull back into input')
  })
})

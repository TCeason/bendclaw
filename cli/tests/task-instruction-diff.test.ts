import { describe, expect, test } from 'bun:test'
import { instructionDiff } from '../src/task/instruction-diff.js'

describe('instructionDiff', () => {
  test('equal text has nothing to show', () => {
    expect(instructionDiff('same', 'same')).toEqual([])
  })

  test('a small edit inside one line is marked inline, not shown twice', () => {
    const before = 'Summarise the Hacker News front page every morning at 9.'
    const after = 'Summarise the Hacker News front page every morning at 8.'
    const rows = instructionDiff(before, after)
    expect(rows).toHaveLength(1)
    expect(rows[0]).toBe('~ Summarise the Hacker News front page every morning at [-9-]{+8+}.')
  })

  test('a rewritten line is shown as removed and added', () => {
    const rows = instructionDiff('Do the old thing.', 'Something entirely different and much longer.')
    expect(rows).toEqual([
      '- Do the old thing.',
      '+ Something entirely different and much longer.',
    ])
  })

  test('unchanged stretches collapse around the change; a fully changed line is not inlined', () => {
    const before = ['one', 'two', 'three', 'four', 'five', 'six'].join('\n')
    const after = ['one', 'two', 'three', 'four', 'FIVE', 'six'].join('\n')
    expect(instructionDiff(before, after)).toEqual([
      '  … 3 unchanged lines',
      '  four',
      '- five',
      '+ FIVE',
      '  six',
    ])
  })

  test('added and removed lines keep their own marks', () => {
    // One unchanged line is cheaper to show than to count.
    expect(instructionDiff('a\nb', 'a\nb\nc')).toEqual(['  a', '  b', '+ c'])
    expect(instructionDiff('a\nb\nc', 'a\nb')).toEqual(['  a', '  b', '- c'])
  })
})

/**
 * Tests for paste reference protocol.
 */

import { describe, test, expect } from 'bun:test'
import {
  formatPastedTextRef,
  parsePasteRefs,
  expandPasteRefs,
  snapCursor,
  deleteRefBackspace,
  skipRefOnMove,
  shouldCollapse,
  cleanPastedText,
} from '../src/input/paste_refs.js'

// ---------------------------------------------------------------------------
// formatPastedTextRef
// ---------------------------------------------------------------------------

describe('formatPastedTextRef', () => {
  test('0 lines → no line count', () => {
    expect(formatPastedTextRef(1, 0)).toBe('[Pasted text #1]')
  })

  test('negative lines → no line count', () => {
    expect(formatPastedTextRef(2, -1)).toBe('[Pasted text #2]')
  })

  test('positive lines → includes line count', () => {
    expect(formatPastedTextRef(1, 10)).toBe('[Pasted text #1 +10 lines]')
  })

  test('1 line → +1 lines', () => {
    expect(formatPastedTextRef(3, 1)).toBe('[Pasted text #3 +1 lines]')
  })
})

// ---------------------------------------------------------------------------
// parsePasteRefs
// ---------------------------------------------------------------------------

describe('parsePasteRefs', () => {
  test('no refs → empty array', () => {
    expect(parsePasteRefs('hello world')).toEqual([])
  })

  test('single ref without line count', () => {
    const refs = parsePasteRefs('hello [Pasted text #1] world')
    expect(refs).toEqual([{
      id: 1,
      start: 6,
      end: 22,
      match: '[Pasted text #1]',
    }])
  })

  test('single ref with line count', () => {
    const refs = parsePasteRefs('hello [Pasted text #2 +10 lines] world')
    expect(refs).toEqual([{
      id: 2,
      start: 6,
      end: 32,
      match: '[Pasted text #2 +10 lines]',
    }])
  })

  test('multiple refs', () => {
    const text = '[Pasted text #1] and [Pasted text #2 +5 lines]'
    const refs = parsePasteRefs(text)
    expect(refs).toHaveLength(2)
    expect(refs[0]!.id).toBe(1)
    expect(refs[1]!.id).toBe(2)
  })

  test('ref at start of string', () => {
    const refs = parsePasteRefs('[Pasted text #1]')
    expect(refs[0]!.start).toBe(0)
  })

  test('ref at end of string', () => {
    const text = 'hello [Pasted text #1]'
    const refs = parsePasteRefs(text)
    expect(refs[0]!.end).toBe(text.length)
  })
})

// ---------------------------------------------------------------------------
// expandPasteRefs
// ---------------------------------------------------------------------------

describe('expandPasteRefs', () => {
  test('expands ref with stored content', () => {
    const store = new Map([[1, 'line1\nline2\nline3']])
    const result = expandPasteRefs('before [Pasted text #1 +3 lines] after', store)
    expect(result).toBe('before line1\nline2\nline3 after')
  })

  test('missing ref left as-is', () => {
    const store = new Map<number, string>()
    const text = 'hello [Pasted text #99] world'
    expect(expandPasteRefs(text, store)).toBe(text)
  })

  test('multiple refs expanded correctly', () => {
    const store = new Map([[1, 'AAA'], [2, 'BBB']])
    const result = expandPasteRefs('[Pasted text #1] and [Pasted text #2]', store)
    expect(result).toBe('AAA and BBB')
  })

  test('only matching refs expanded', () => {
    const store = new Map([[1, 'AAA']])
    const result = expandPasteRefs('[Pasted text #1] and [Pasted text #2]', store)
    expect(result).toBe('AAA and [Pasted text #2]')
  })
})

// ---------------------------------------------------------------------------
// snapCursor
// ---------------------------------------------------------------------------

describe('snapCursor', () => {
  const refs = parsePasteRefs('hello [Pasted text #1 +5 lines] end')
  // ref is at start=6, end=31

  test('cursor before ref → unchanged', () => {
    expect(snapCursor(3, refs)).toBe(3)
  })

  test('cursor at ref start → unchanged', () => {
    expect(snapCursor(6, refs)).toBe(6)
  })

  test('cursor at ref end → unchanged', () => {
    expect(snapCursor(31, refs)).toBe(31)
  })

  test('cursor inside ref, left half → snap to start', () => {
    expect(snapCursor(10, refs)).toBe(6)
  })

  test('cursor inside ref, right half → snap to end', () => {
    expect(snapCursor(25, refs)).toBe(31)
  })

  test('cursor after ref → unchanged', () => {
    expect(snapCursor(33, refs)).toBe(33)
  })

  test('no refs → unchanged', () => {
    expect(snapCursor(5, [])).toBe(5)
  })
})

// ---------------------------------------------------------------------------
// deleteRefBackspace
// ---------------------------------------------------------------------------

describe('deleteRefBackspace', () => {
  const line = 'hello [Pasted text #1 +5 lines] end'
  const refs = parsePasteRefs(line)
  // ref at start=6, end=31

  test('cursor at ref end → delete entire ref', () => {
    const result = deleteRefBackspace(line, 31, refs)
    expect(result).toEqual({
      newLine: 'hello  end',
      newCursorCol: 6,
    })
  })

  test('cursor not at any ref boundary → null', () => {
    expect(deleteRefBackspace(line, 3, refs)).toBeNull()
  })

  test('cursor at ref start → null (normal backspace)', () => {
    expect(deleteRefBackspace(line, 6, refs)).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// skipRefOnMove
// ---------------------------------------------------------------------------

describe('skipRefOnMove', () => {
  const refs = parsePasteRefs('hello [Pasted text #1 +5 lines] end')
  // ref at start=6, end=31

  test('right arrow at ref start → skip to end', () => {
    expect(skipRefOnMove(6, 'right', refs)).toBe(31)
  })

  test('left arrow at ref end → skip to start', () => {
    expect(skipRefOnMove(31, 'left', refs)).toBe(6)
  })

  test('right arrow not at ref → null', () => {
    expect(skipRefOnMove(3, 'right', refs)).toBeNull()
  })

  test('left arrow not at ref → null', () => {
    expect(skipRefOnMove(3, 'left', refs)).toBeNull()
  })

  test('no refs → null', () => {
    expect(skipRefOnMove(5, 'right', [])).toBeNull()
  })
})

// ---------------------------------------------------------------------------
// shouldCollapse
// ---------------------------------------------------------------------------

describe('shouldCollapse', () => {
  test('short single line → false', () => {
    expect(shouldCollapse('hello')).toBe(false)
  })

  test('2 lines → false (at threshold)', () => {
    expect(shouldCollapse('line1\nline2')).toBe(false)
  })

  test('3 newlines → true (exceeds line threshold)', () => {
    expect(shouldCollapse('a\nb\nc\nd')).toBe(true)
  })

  test('long single line → true (exceeds char threshold)', () => {
    expect(shouldCollapse('x'.repeat(801))).toBe(true)
  })

  test('exactly 800 chars → false', () => {
    expect(shouldCollapse('x'.repeat(800))).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// cleanPastedText
// ---------------------------------------------------------------------------

describe('cleanPastedText', () => {
  test('strips ANSI escape codes', () => {
    expect(cleanPastedText('\x1b[31mhello\x1b[0m')).toBe('hello')
  })

  test('normalizes CRLF to LF', () => {
    expect(cleanPastedText('a\r\nb')).toBe('a\nb')
  })

  test('normalizes CR to LF', () => {
    expect(cleanPastedText('a\rb')).toBe('a\nb')
  })

  test('converts tabs to 4 spaces', () => {
    expect(cleanPastedText('\thello')).toBe('    hello')
  })

  test('combined cleanup', () => {
    expect(cleanPastedText('\x1b[32m\thello\r\nworld\x1b[0m'))
      .toBe('    hello\nworld')
  })

  test('plain text unchanged', () => {
    expect(cleanPastedText('hello world')).toBe('hello world')
  })
})

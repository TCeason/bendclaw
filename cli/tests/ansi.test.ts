import { describe, test, expect } from 'bun:test'
import {
  cursorTo,
  cursorUp,
  cursorDown,
  cursorToColumn,
  eraseLine,
  eraseToEndOfLine,
  eraseDown,
  saveCursor,
  restoreCursor,
  hideCursor,
  showCursor,
  setScrollRegion,
  resetScrollRegion,
  requestCursorPosition,
  cursorToBottom,
} from '../src/term/ansi.js'

describe('cursorTo', () => {
  test('generates correct escape sequence', () => {
    expect(cursorTo(5, 10)).toBe('\x1b[5;10H')
  })

  test('row=1, col=1 (top-left)', () => {
    expect(cursorTo(1, 1)).toBe('\x1b[1;1H')
  })

  test('large values', () => {
    expect(cursorTo(100, 200)).toBe('\x1b[100;200H')
  })
})

describe('cursorUp', () => {
  test('moves up N lines', () => {
    expect(cursorUp(3)).toBe('\x1b[3A')
  })

  test('n=0 returns empty', () => {
    expect(cursorUp(0)).toBe('')
  })

  test('negative returns empty', () => {
    expect(cursorUp(-1)).toBe('')
  })

  test('n=1', () => {
    expect(cursorUp(1)).toBe('\x1b[1A')
  })
})

describe('cursorDown', () => {
  test('moves down N lines', () => {
    expect(cursorDown(2)).toBe('\x1b[2B')
  })

  test('n=0 returns empty', () => {
    expect(cursorDown(0)).toBe('')
  })
})

describe('cursorToColumn', () => {
  test('moves to column', () => {
    expect(cursorToColumn(5)).toBe('\x1b[5G')
  })
})

describe('eraseLine', () => {
  test('generates erase entire line', () => {
    expect(eraseLine()).toBe('\x1b[2K')
  })
})

describe('eraseToEndOfLine', () => {
  test('generates erase to end', () => {
    expect(eraseToEndOfLine()).toBe('\x1b[0K')
  })
})

describe('eraseDown', () => {
  test('generates erase down', () => {
    expect(eraseDown()).toBe('\x1b[J')
  })
})

describe('saveCursor / restoreCursor', () => {
  test('save', () => {
    expect(saveCursor()).toBe('\x1b[s')
  })

  test('restore', () => {
    expect(restoreCursor()).toBe('\x1b[u')
  })
})

describe('hideCursor / showCursor', () => {
  test('hide', () => {
    expect(hideCursor()).toBe('\x1b[?25l')
  })

  test('show', () => {
    expect(showCursor()).toBe('\x1b[?25h')
  })
})

describe('setScrollRegion', () => {
  test('sets region', () => {
    expect(setScrollRegion(1, 20)).toBe('\x1b[1;20r')
  })
})

describe('resetScrollRegion', () => {
  test('resets', () => {
    expect(resetScrollRegion()).toBe('\x1b[r')
  })
})

describe('requestCursorPosition', () => {
  test('generates DSR', () => {
    expect(requestCursorPosition()).toBe('\x1b[6n')
  })
})

describe('cursorToBottom', () => {
  test('moves to last row col 1', () => {
    expect(cursorToBottom(24)).toBe('\x1b[24;1H')
  })
})

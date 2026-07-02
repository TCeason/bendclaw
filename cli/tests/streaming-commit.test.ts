import { describe, expect, test } from 'bun:test'
import {
  splitMarkdownBlocks,
  findStreamingCommitPoint,
  findNaturalPlainTextCommitPoint,
  isInsideOpenMathBlock,
  isInsideOpenCodeFence,
} from '../src/markdown/streaming/commit.js'

// The streaming block splitter is the core of streaming smoothness: it drains
// completed markdown blocks into scrollback so only the forming tail is
// re-rendered each delta. These tests lock the boundary rules that keep the
// split safe (never mid code-fence, mid-table, or mid-math).

describe('splitMarkdownBlocks', () => {
  test('empty input yields empty completed and pending', () => {
    expect(splitMarkdownBlocks('')).toEqual({ completed: '', pending: '' })
  })

  test('commits a finished paragraph, holds the forming one', () => {
    const { completed, pending } = splitMarkdownBlocks('First para.\n\nSecond para forming')
    expect(completed).toBe('First para.\n\n')
    expect(pending).toBe('Second para forming')
  })

  test('reconstructs the original text from completed + pending', () => {
    const text = '## Heading\n\nBody paragraph.\n\nTail'
    const { completed, pending } = splitMarkdownBlocks(text)
    expect(completed + pending).toBe(text)
  })

  test('holds a single unfinished paragraph entirely pending', () => {
    const { completed, pending } = splitMarkdownBlocks('still typing one line')
    expect(completed).toBe('')
    expect(pending).toBe('still typing one line')
  })
})

describe('findStreamingCommitPoint', () => {
  test('commits up to a paragraph boundary', () => {
    const text = 'Hello world.\n\nSecond'
    expect(findStreamingCommitPoint(text)).toBe('Hello world.\n\n'.length)
  })

  test('commits a completed heading before the next block', () => {
    const text = '## Title\n\nBody'
    expect(findStreamingCommitPoint(text)).toBe('## Title\n\n'.length)
  })

  test('holds an open code fence pending (no split)', () => {
    const text = 'Intro\n\n```js\nconst a = 1\nconst b = 2'
    // Only the prose before the fence is committed; the open fence stays pending.
    expect(findStreamingCommitPoint(text)).toBe('Intro\n\n'.length)
  })

  test('commits past a closed code fence', () => {
    const text = 'Intro\n\n```js\nconst a = 1\n```\n\nAfter'
    const point = findStreamingCommitPoint(text)
    expect(point).toBe('Intro\n\n```js\nconst a = 1\n```\n\n'.length)
  })

  test('returns 0 when nothing is safely committable', () => {
    expect(findStreamingCommitPoint('one line still typing')).toBe(0)
  })
})

describe('findNaturalPlainTextCommitPoint', () => {
  test('commits complete leading lines of long boundary-free prose', () => {
    const text = Array.from({ length: 12 }, (_, i) => `line ${i}`).join('\n')
    const point = findNaturalPlainTextCommitPoint(text, 24)
    expect(point).toBeGreaterThan(0)
    expect(point).toBeLessThan(text.length)
    // Split falls on a line boundary.
    expect(text[point - 1]).toBe('\n')
  })

  test('does not commit short prose (stays pending)', () => {
    expect(findNaturalPlainTextCommitPoint('short\nprose', 24)).toBe(0)
  })

  test('does not commit when markdown block syntax is present', () => {
    const text = Array.from({ length: 12 }, (_, i) => `- item ${i}`).join('\n')
    expect(findNaturalPlainTextCommitPoint(text, 24)).toBe(0)
  })
})

describe('isInsideOpenCodeFence', () => {
  test('true for an unterminated fence', () => {
    expect(isInsideOpenCodeFence('```js\nconst a = 1')).toBe(true)
  })

  test('false for a closed fence', () => {
    expect(isInsideOpenCodeFence('```js\nconst a = 1\n```')).toBe(false)
  })

  test('false for plain prose', () => {
    expect(isInsideOpenCodeFence('just some text\nmore text')).toBe(false)
  })

  test('tilde fences are recognized', () => {
    expect(isInsideOpenCodeFence('~~~\ncode here')).toBe(true)
    expect(isInsideOpenCodeFence('~~~\ncode here\n~~~')).toBe(false)
  })
})

describe('isInsideOpenMathBlock', () => {
  test('true for an unterminated $$ block', () => {
    expect(isInsideOpenMathBlock('$$\n\\int x dx')).toBe(true)
  })

  test('false for a closed $$ block', () => {
    expect(isInsideOpenMathBlock('$$\n\\int x dx\n$$')).toBe(false)
  })

  test('ignores $$ inside a code fence', () => {
    expect(isInsideOpenMathBlock('```\n$$ not math $$\n```')).toBe(false)
  })
})

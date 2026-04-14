import { describe, test, expect } from 'bun:test'
import { formatDiff, colorizeUnifiedDiff } from '../src/utils/diff.js'
import stripAnsi from 'strip-ansi'

describe('formatDiff', () => {
  test('detects added lines', () => {
    const result = formatDiff('a\nb\n', 'a\nb\nc\n')
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(0)
    const plain = stripAnsi(result.text)
    expect(plain).toContain('+c')
  })

  test('detects removed lines', () => {
    const result = formatDiff('a\nb\nc\n', 'a\nc\n')
    expect(result.linesRemoved).toBe(1)
    const plain = stripAnsi(result.text)
    expect(plain).toContain('-b')
  })

  test('detects changed lines', () => {
    const result = formatDiff('hello\n', 'world\n')
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(1)
    const plain = stripAnsi(result.text)
    expect(plain).toContain('-hello')
    expect(plain).toContain('+world')
  })

  test('returns empty for identical text', () => {
    const result = formatDiff('same\n', 'same\n')
    expect(result.linesAdded).toBe(0)
    expect(result.linesRemoved).toBe(0)
  })

  test('shows line numbers', () => {
    const result = formatDiff('a\nb\n', 'a\nc\n')
    const plain = stripAnsi(result.text)
    // Line 2 should appear as a number in the gutter
    expect(plain).toMatch(/2\s*[-+]/)
  })

  test('shows ellipsis between hunks', () => {
    // Create a diff with two separate hunks (changes far apart)
    const oldLines = Array.from({ length: 20 }, (_, i) => `line${i}`).join('\n') + '\n'
    const newLines = oldLines.replace('line1', 'changed1').replace('line18', 'changed18')
    const result = formatDiff(oldLines, newLines)
    const plain = stripAnsi(result.text)
    expect(plain).toContain('…')
  })
})

describe('colorizeUnifiedDiff', () => {
  test('colorizes diff lines with line numbers', () => {
    const diff = '--- a/file\n+++ b/file\n@@ -1,2 +1,2 @@\n-old\n+new\n context'
    const result = colorizeUnifiedDiff(diff)
    const plain = stripAnsi(result)
    expect(plain).toContain('+new')
    expect(plain).toContain('-old')
    // Should NOT contain raw --- / +++ headers
    expect(plain).not.toContain('--- a/file')
  })

  test('shows line numbers in gutter', () => {
    const diff = '@@ -1,3 +1,3 @@\n line1\n-old\n+new\n line3'
    const result = colorizeUnifiedDiff(diff)
    const plain = stripAnsi(result)
    expect(plain).toMatch(/1\s/)
    expect(plain).toMatch(/2\s*[-+]/)
  })
})

describe('word-level diff', () => {
  test('highlights changed words within a line', () => {
    const result = formatDiff(
      'function oldName(param) {\n',
      'function newName(param) {\n',
    )
    // The output should contain both the old and new function names
    const plain = stripAnsi(result.text)
    expect(plain).toContain('oldName')
    expect(plain).toContain('newName')
    // Should have add and remove lines
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(1)
  })

  test('falls back to line-level for large changes', () => {
    // Completely different lines should not use word-level diff
    const result = formatDiff(
      'completely different content here\n',
      'nothing similar at all whatsoever\n',
    )
    expect(result.linesAdded).toBe(1)
    expect(result.linesRemoved).toBe(1)
  })
})

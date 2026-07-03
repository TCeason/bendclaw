import { describe, test, expect, beforeAll } from 'bun:test'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines, styledLineToAnsi, line, colored, dim } from '../src/term/viewmodel/types.js'
import { buildUserMessage, buildAssistantLines, type OutputLine } from '../src/render/output.js'
import { colorizeUnifiedDiff } from '../src/render/diff.js'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import chalk from 'chalk'

const OSC133_ZONE_START = '\x1b]133;A\x07'
const OSC133_ZONE_END = '\x1b]133;B\x07\x1b]133;C\x07'

beforeAll(() => {
  chalk.level = 3
})

function render(lines: OutputLine[]): string {
  return blocksToLines(buildOutputBlocks(lines)).join('\n')
}

function renderPlain(lines: OutputLine[]): string {
  return stripAnsi(render(lines))
}

function renderWithColumns(lines: OutputLine[], columns: number): string {
  return blocksToLines(buildOutputBlocks(lines, { columns })).join('\n')
}

function renderPlainWithColumns(lines: OutputLine[], columns: number): string {
  return stripAnsi(renderWithColumns(lines, columns))
}

describe('buildOutputBlocks', () => {
  test('user message has marginTop=1 and bold prefix', () => {
    const result = renderPlain([{ id: 'u1', kind: 'user', text: 'hello' }])
    expect(result).toContain('❯ hello')
    expect(result.startsWith('\n')).toBe(true)
  })

  test('assistant block starts with marginTop=1', () => {
    const result = renderPlain([
      { id: 'u1', kind: 'user', text: 'hi' },
      { id: 'a1', kind: 'assistant', text: 'response line 1' },
    ])
    const lines = result.split('\n')
    const assistantIdx = lines.findIndex(l => l.includes('response line 1'))
    expect(lines[assistantIdx - 1]).toBe('')
  })

  test('consecutive assistant lines have no margin', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'line 1' },
      { id: 'a2', kind: 'assistant', text: 'line 2' },
      { id: 'a3', kind: 'assistant', text: 'line 3' },
    ])
    const lines = result.split('\n')
    const contentLines = lines.filter(l => l.includes('line'))
    expect(contentLines.length).toBe(3)
    const emptyBetween = lines.slice(
      lines.indexOf(contentLines[0]!),
      lines.indexOf(contentLines[2]!) + 1
    ).filter(l => l === '')
    expect(emptyBetween.length).toBe(0)
  })

  test('tool card has marginTop=1', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'text' },
      { id: 't1', kind: 'tool', text: '⌘ bash  ls -la' },
    ])
    const lines = result.split('\n')
    const toolIdx = lines.findIndex(l => l.includes('bash'))
    expect(lines[toolIdx - 1]).toBe('')
  })

  test('long tool command wraps instead of truncating', () => {
    const cmd = 'cd /Users/bohu/github/evotai/evot && rg -n "first_line|before_turn|after_turn" src/ --glob "*.rs" | head -20'
    const result = renderPlainWithColumns([{ id: 't1', kind: 'tool', text: `⌘ bash  ${cmd}` }], 72)
    // No ellipsis truncation — the full command survives across wrapped lines.
    expect(result).not.toContain('…')
    expect(result.replace(/\n\s*/g, '')).toContain('head -20')
    // Continuation lines are indented to align under the arg (after `⌘ bash  `).
    const lines = result.split('\n').filter(l => l.length > 0)
    expect(lines.length).toBeGreaterThan(1)
    expect(lines[1]!.startsWith('        ')).toBe(true)
  })

  test('long diff line wraps instead of truncating', () => {
    const longAdded = 'x'.repeat(200)
    const diff = `@@ -1,1 +1,1 @@\n-short old line\n+${longAdded}`
    const colored = colorizeUnifiedDiff(diff)
    const result = renderPlainWithColumns([{ id: 'd1', kind: 'tool', text: colored }], 40)
    // The full added content survives across wrapped lines (no truncation).
    expect(result.replace(/\n/g, '')).toContain(longAdded)
    // Every rendered line fits within the terminal width.
    for (const l of result.split('\n')) {
      expect(stringWidth(l)).toBeLessThanOrEqual(40)
    }
  })

  test('tool detail lines have no margin', () => {
    const result = renderPlain([
      { id: 't1', kind: 'tool', text: '⌘ bash  ls -la' },
      { id: 't2', kind: 'tool_result', text: '  output' },
    ])
    const lines = result.split('\n')
    const detailIdx = lines.findIndex(l => l.includes('output'))
    expect(lines[detailIdx - 1]).toContain('bash')
  })

  test('verbose badge has marginTop=1', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'text' },
      { id: 'v1', kind: 'verbose', text: '[LLM] ● · started model=gpt-4' },
    ])
    const lines = result.split('\n')
    const verboseIdx = lines.findIndex(l => l.includes('LLM'))
    expect(lines[verboseIdx - 1]).toBe('')
  })

  test('verbose status colors are unified', () => {
    const result = render([
      { id: 'v1', kind: 'verbose', text: '[COMPACT] ● · 1 msgs' },
      { id: 'v2', kind: 'verbose', text: '[COMPACT] ✓ · skipped · within budget' },
      { id: 'v3', kind: 'verbose', text: '[LLM] ✓ · gpt-5.5 · turn 1 · 3.1s' },
    ])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[32m')
    expect(result).not.toContain('\x1b[31m')
  })

  test('tool card glyph uses unified color', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '⌘ bash  ls -la' }])
    expect(result).toContain('\x1b[36m')
    expect(result).not.toContain('\x1b[32m')
  })

  test('tool status line ok mark uses green', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '  ✓ · 1.2s' }])
    expect(result).toContain('\x1b[32m')
  })

  test('tool status line fail mark uses red', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '  ✗ · exit 1' }])
    expect(result).toContain('\x1b[31m')
  })

  test('tool status line retry mark uses yellow', () => {
    const result = render([{ id: 't1', kind: 'tool', text: '  ↻ · retrying' }])
    expect(result).toContain('\x1b[33m')
  })

  test('JSON result body is not dimmed', () => {
    const result = render([{ id: 'r1', kind: 'tool_result', text: '  {"status":"ok"}' }])
    expect(result).not.toContain('\x1b[2m')
  })

  test('continuation spacer keeps assistant marker from repeating', () => {
    const result = renderPlain([
      { id: 'a1', kind: 'assistant', text: 'Intro' },
      { id: 'sep', kind: 'assistant', text: '', isContinuationSpacer: true },
      { id: 'a2', kind: 'assistant', text: 'Long paragraph' },
    ])

    expect(result).toContain('⏺ Intro\n\n  Long paragraph')
    expect(result).not.toContain('⏺ Long paragraph')
  })

  test('long assistant line reflows on resize instead of truncating', () => {
    const longText = 'reflow '.repeat(30).trim()
    // Committed assistant text must wrap to the current render width (prefix
    // is 2 cols) so a terminal shrink reflows rather than truncates.
    const result = renderPlainWithColumns([{ id: 'a1', kind: 'assistant', text: longText }], 40)
    for (const l of result.split('\n')) {
      expect(stringWidth(l)).toBeLessThanOrEqual(40)
    }
    // Full content survives across wrapped lines.
    expect(result.replace(/\n\s*/g, ' ')).toContain(longText)
    // Continuation lines align under the text (2-space indent).
    const lines = result.split('\n').filter(l => l.length > 0)
    expect(lines.length).toBeGreaterThan(1)
    expect(lines[1]!.startsWith('  ')).toBe(true)
  })

  test('system lines are dim', () => {
    const result = render([{ id: 's1', kind: 'system', text: '  some info' }])
    expect(result).toContain('\x1b[38;2;119;119;119m')
  })

  test('error lines are red', () => {
    const result = render([{ id: 'e1', kind: 'error', text: 'something broke' }])
    expect(result).toContain('\x1b[31m')
  })

  test('long error wraps instead of truncating', () => {
    const msg = '  rate_limit_error: You have reached your usage limit for this period. Your quota will be refreshed in the next period. Upgrade to get more at the console.'
    const result = renderPlainWithColumns([{ id: 'e1', kind: 'error', text: msg }], 72)
    expect(result).not.toContain('…')
    expect(result.replace(/\n\s*/g, '')).toContain('console.')
    const lines = result.split('\n').filter(l => l.length > 0)
    expect(lines.length).toBeGreaterThan(1)
    // Wrapped continuations keep the 2-space indent.
    expect(lines[1]!.startsWith('  ')).toBe(true)
  })

  test('run_summary is dim', () => {
    const result = render([{ id: 'r1', kind: 'run_summary', text: '  3 turns · 1.2k tokens' }])
    expect(result).toContain('\x1b[38;2;119;119;119m')
  })

  test('user message wraps when columns is provided', () => {
    // 20 columns minus 2 for prefix = 18 chars per line
    const longText = 'a'.repeat(40)
    const result = renderPlainWithColumns([{ id: 'u1', kind: 'user', text: longText }], 20)
    const lines = result.split('\n').filter(l => l.trim() !== '')
    // Should wrap into 3 lines: 18 + 18 + 4
    expect(lines.length).toBe(3)
    expect(lines[0]).toContain('❯ ' + 'a'.repeat(18))
    expect(lines[1]).toContain('  ' + 'a'.repeat(18))
    expect(lines[2]).toContain('  ' + 'a'.repeat(4))
  })

  test('user message wraps CJK characters correctly', () => {
    // Each CJK char is 2 columns wide. With 22 columns, avail = 20.
    // Each char takes 2 cols, so 10 chars per line.
    const cjkText = '你'.repeat(25)
    const result = renderPlainWithColumns([{ id: 'u1', kind: 'user', text: cjkText }], 22)
    const lines = result.split('\n').filter(l => l.trim() !== '')
    // 25 chars at 2-width each = 50 cols, avail = 20, so 10 chars/line => 3 lines
    expect(lines.length).toBe(3)
    expect(lines[0]).toContain('❯ ' + '你'.repeat(10))
    expect(lines[1]).toContain('  ' + '你'.repeat(10))
    expect(lines[2]).toContain('  ' + '你'.repeat(5))
  })
})

describe('OSC 133 semantic zone markers', () => {
  test('a user message is wrapped in one balanced zone', () => {
    const raw = render(buildUserMessage('hello there'))
    expect(raw).toContain(OSC133_ZONE_START)
    expect(raw).toContain(OSC133_ZONE_END)
    // Exactly one zone (one start, one end) for a single message.
    expect(raw.split(OSC133_ZONE_START).length - 1).toBe(1)
    expect(raw.split(OSC133_ZONE_END).length - 1).toBe(1)
    // The start marker precedes the visible prompt glyph.
    expect(raw.indexOf(OSC133_ZONE_START)).toBeLessThan(raw.indexOf('❯'))
  })

  test('a multi-line assistant message has exactly one zone spanning all lines', () => {
    const raw = render(buildAssistantLines('line one\n\nline two\n\nline three'))
    expect(raw.split(OSC133_ZONE_START).length - 1).toBe(1)
    expect(raw.split(OSC133_ZONE_END).length - 1).toBe(1)
    // Start comes before the first content, end after the last.
    expect(raw.indexOf(OSC133_ZONE_START)).toBeLessThan(raw.indexOf('line one'))
    expect(raw.indexOf('line three')).toBeLessThan(raw.indexOf(OSC133_ZONE_END))
  })

  test('markers are stripped by strip-ansi so line widths are unaffected', () => {
    const withMarkers = render(buildUserMessage('hello'))
    const plain = stripAnsi(withMarkers)
    expect(plain).not.toContain('133')
    expect(plain).toContain('❯ hello')
  })

  test('non-message kinds (tool, system) get no zone markers', () => {
    const raw = render([
      { id: 't1', kind: 'tool', text: '⌘ bash  ls' },
      { id: 's1', kind: 'system', text: 'note' },
    ])
    expect(raw).not.toContain(OSC133_ZONE_START)
    expect(raw).not.toContain(OSC133_ZONE_END)
  })

  test('consecutive user and assistant messages form separate zones', () => {
    const raw = render([
      ...buildUserMessage('question'),
      ...buildAssistantLines('answer line 1\n\nanswer line 2'),
    ])
    // Two messages => two zones.
    expect(raw.split(OSC133_ZONE_START).length - 1).toBe(2)
    expect(raw.split(OSC133_ZONE_END).length - 1).toBe(2)
  })
})

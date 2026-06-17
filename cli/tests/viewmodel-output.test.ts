import { describe, test, expect, beforeAll } from 'bun:test'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines, styledLineToAnsi, line, colored, dim } from '../src/term/viewmodel/types.js'
import type { OutputLine } from '../src/render/output.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

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

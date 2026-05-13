import { describe, test, expect } from 'bun:test'
import { buildActiveResponseBlocks, renderedPendingTailWidth, type ActiveResponseInput } from '../src/term/viewmodel/active-response.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { createSpinnerState, setSpinnerPhase } from '../src/term/spinner.js'
import stripAnsi from 'strip-ansi'

function defaultInput(overrides?: Partial<ActiveResponseInput>): ActiveResponseInput {
  return {
    isLoading: true,
    pendingText: '',
    toolProgress: '',
    pendingThinkingText: '',
    spinner: createSpinnerState(),
    termRows: 24,
    revealCursor: 999,
    ...overrides,
  }
}

function render(input: ActiveResponseInput): string {
  return blocksToLines(buildActiveResponseBlocks(input)).join('\n')
}

function renderPlain(input: ActiveResponseInput): string {
  return stripAnsi(render(input))
}

describe('buildActiveResponseBlocks', () => {
  test('returns empty when not loading', () => {
    const blocks = buildActiveResponseBlocks(defaultInput({ isLoading: false }))
    expect(blocks).toEqual([])
  })

  test('shows spinner when loading', () => {
    const result = renderPlain(defaultInput())
    expect(result).toContain('Thinking')
  })

  test('shows pending text when streaming', () => {
    const result = renderPlain(defaultInput({ pendingText: 'hello world' }))
    expect(result).toContain('hello world')
    expect(result).toContain('Thinking')
  })

  test('multi-line pending text falls back to spinner to avoid fixed-area flicker', () => {
    const longText = Array.from({ length: 30 }, (_, i) => `line ${i}`).join('\n')
    const result = renderPlain(defaultInput({ pendingText: longText, termRows: 20 }))
    expect(result).toContain('Thinking')
    expect(result).not.toContain('line 29')
  })

  test('structured markdown pending text falls back to spinner', () => {
    const listText = Array.from({ length: 8 }, (_, i) => `- item ${i}`).join('\n')
    const result = renderPlain(defaultInput({ pendingText: listText, termRows: 24 }))
    expect(result).toContain('Thinking')
    expect(result).not.toContain('item 7')
  })

  test('reveals pending text by display width', () => {
    const result = renderPlain(defaultInput({ pendingText: 'hello world', revealCursor: 5 }))
    expect(result).toContain('hello')
    expect(result).not.toContain('hello world')
  })

  test('preserves non-SGR ANSI sequences while revealing pending text', () => {
    const link = '\x1b]8;;https://example.com\x1b\\click\x1b]8;;\x1b\\'
    const result = buildActiveResponseBlocks(defaultInput({ pendingText: link, revealCursor: 3 }))
    const rendered = blocksToLines(result).join('\n')
    expect(rendered).toContain('\x1b]8;;https://example.com\x1b\\')
    expect(stripAnsi(rendered)).toContain('cli')
    expect(stripAnsi(rendered)).not.toContain('click')
  })

  test('keeps a blank separator between revealed tail and spinner', () => {
    const lines = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: 'hello world', revealCursor: 5 })))
    expect(stripAnsi(lines[lines.length - 1]!)).toContain('Thinking')
    expect(stripAnsi(lines[lines.length - 2]!)).toBe('')
    expect(stripAnsi(lines[lines.length - 3]!)).toContain('hello')
  })

  test('keeps pending tail height stable when markdown becomes structural', () => {
    const simple = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: 'hello world', revealCursor: 5 })))
    const structural = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: '| a | b |\n| - | - |', revealCursor: 0 })))
    expect(structural.length).toBe(simple.length)
    expect(stripAnsi(structural[structural.length - 1]!)).toContain('Thinking')
    expect(stripAnsi(structural[structural.length - 2]!)).toBe('')
    expect(stripAnsi(structural[structural.length - 3]!)).toBe(' ')
  })

  test('computes rendered pending tail width for timer reveal', () => {
    expect(renderedPendingTailWidth('hello world')).toBe(11)
    expect(renderedPendingTailWidth('| a | b |\n| - | - |')).toBe(0)
  })

  test('shows tool progress with fixed height', () => {
    const result = renderPlain(defaultInput({ toolProgress: 'running...\noutput line 1\noutput line 2' }))
    expect(result).toContain('output line 2')
  })

  test('tool progress omits expand hint when all lines are visible', () => {
    const result = renderPlain(defaultInput({ toolProgress: 'single line' }))
    const lines = result.split('\n')
    expect(lines).toContain('  single line')
    expect(result).not.toContain('ctrl+o to expand')
  })

  test('tool progress shows expand hint only when truncated', () => {
    const progress = Array.from({ length: 7 }, (_, i) => `line ${i}`).join('\n')
    const result = renderPlain(defaultInput({ toolProgress: progress }))
    expect(result).toContain('  line 6')
    expect(result).toContain('  +2 lines  (ctrl+o to expand)')
    expect(result).not.toContain('  line 0')
  })

  test('shows Executing when tool phase', () => {
    const spinner = setSpinnerPhase(createSpinnerState(), 'executing', 'bash')
    const result = renderPlain(defaultInput({ spinner }))
    expect(result).toContain('Executing')
  })

  test('keeps a stable output area while executing before progress arrives', () => {
    const spinner = setSpinnerPhase(createSpinnerState(), 'executing', 'bash')
    const result = renderPlain(defaultInput({ spinner, toolProgress: '' }))
    expect(result).toContain('Waiting for output…')
    expect(result).toContain('Executing')
  })

  test('shows collapse hint when expanded', () => {
    const result = renderPlain(defaultInput({ expanded: true, toolProgress: 'line 1\nline 2' }))
    expect(result).toContain('ctrl+o to collapse')
  })

  test('truncates long progress lines', () => {
    const longLine = 'x'.repeat(200)
    const result = renderPlain(defaultInput({ toolProgress: longLine }))
    const lines = result.split('\n')
    const progressLine = lines.find(l => l.includes('xxx'))
    expect(progressLine!.length).toBeLessThan(200)
    expect(progressLine).toContain('…')
  })
})

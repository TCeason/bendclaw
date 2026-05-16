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

  test('multi-line pending text shows rendered content in status area', () => {
    const longText = Array.from({ length: 30 }, (_, i) => `line ${i}`).join('\n')
    const result = renderPlain(defaultInput({ pendingText: longText, termRows: 20 }))
    // Multi-line content is now shown in the status area for smooth growth
    expect(result).toContain('line 29')
    expect(result).toContain('Thinking')
  })

  test('structured markdown pending text shows in status area', () => {
    const listText = Array.from({ length: 8 }, (_, i) => `- item ${i}`).join('\n')
    const result = renderPlain(defaultInput({ pendingText: listText, termRows: 24 }))
    // Lists are safe to show in status area (stable rendering)
    expect(result).toContain('item 7')
    expect(result).toContain('Thinking')
  })

  test('reveals pending text by display width', () => {
    // Use text between 61-76 chars: won't wrap but exceeds SHORT_THRESHOLD (60)
    const longText = 'This sentence is exactly long enough to exceed the short threshold ok'
    const result = renderPlain(defaultInput({ pendingText: longText, revealCursor: 10 }))
    expect(result).toContain('This sente')
    expect(result).not.toContain('exactly')
  })


  test('preserves non-SGR ANSI sequences while revealing pending text', () => {
    // Link with visible text > 60 chars to exceed SHORT_THRESHOLD
    const longUrl = 'https://example.com/path'
    const linkText = 'click here to visit this very long hyperlink text that exceeds threshold'
    const link = `\x1b]8;;${longUrl}\x1b\\${linkText}\x1b]8;;\x1b\\`
    const result = buildActiveResponseBlocks(defaultInput({ pendingText: link, revealCursor: 3 }))
    const rendered = blocksToLines(result).join('\n')
    expect(rendered).toContain(`\x1b]8;;${longUrl}\x1b\\`)
    expect(stripAnsi(rendered)).toContain('cli')
    expect(stripAnsi(rendered)).not.toContain('click here to visit')
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

  test('keeps spinner-only height aligned with pending states', () => {
    const spinnerOnly = blocksToLines(buildActiveResponseBlocks(defaultInput()))
    const pending = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: 'hello world', revealCursor: 5 })))
    expect(spinnerOnly.length).toBe(pending.length)
    expect(stripAnsi(spinnerOnly[spinnerOnly.length - 3]!)).toBe(' ')
    expect(stripAnsi(spinnerOnly[spinnerOnly.length - 2]!)).toBe('')
    expect(stripAnsi(spinnerOnly[spinnerOnly.length - 1]!)).toContain('Thinking')
  })

  test('renders wrapped single-line pending text as full markdown instead of clipped tail', () => {
    const text = 'This is a long single-line answer that will wrap in a narrow terminal and should remain visible as a full pending block.'
    const lines = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: text, revealCursor: 12, termColumns: 48 })))
    expect(stripAnsi(lines.join('\n'))).toContain('This is a long single-line answer')
    expect(stripAnsi(lines.join('\n'))).toContain('full pending block')
  })

  test('keeps short single-line pending text on reveal cursor', () => {
    const lines = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: 'hello world', revealCursor: 5, termColumns: 120 })))
    expect(stripAnsi(lines.join('\n'))).toContain('hello')
    expect(stripAnsi(lines.join('\n'))).not.toContain('hello world')
  })

  test('reveals wide single-line pending text by cursor on wide terminals', () => {
    // 110-cell single-line tail on a 200-column terminal: must not be
    // mistaken for multi-line and must respect revealCursor so the typewriter
    // can catch up before commit (paired with TAIL_REVEAL_FINAL_MAX_WIDTH=140).
    const prev = process.stdout.columns
    process.stdout.columns = 200
    try {
      const text = 'a'.repeat(110)
      const lines = blocksToLines(buildActiveResponseBlocks(defaultInput({ pendingText: text, revealCursor: 30, termColumns: 200 })))
      const plain = stripAnsi(lines.join('\n'))
      expect(plain).toContain('a'.repeat(30))
      expect(plain).not.toContain('a'.repeat(110))
      expect(renderedPendingTailWidth(text)).toBe(110)
    } finally {
      process.stdout.columns = prev
    }
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

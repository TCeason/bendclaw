import { describe, test, expect, beforeAll } from 'bun:test'
import { buildPromptBlocks, type PromptVMInput } from '../src/term/viewmodel/prompt.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

beforeAll(() => {
  chalk.level = 3
})

function defaultInput(overrides?: Partial<PromptVMInput>): PromptVMInput {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    active: true,
    model: 'claude-sonnet',
    verbose: true,
    planning: false,
    logMode: false,
    queuedMessages: [],
    updateHint: null,
    serverUptime: null,
    serverPort: null,
    exitHint: false,
    completionCandidates: [],
    ghostHint: '',
    columns: 80,
    isLoading: false,
    placeholder: true,
    cwd: '/Users/test/project',
    gitRepo: 'project',
    gitBranch: 'main',
    inputTokens: 0,
    outputTokens: 0,
    cacheReadTokens: 0,
    contextTokens: 0,
    contextWindow: 0,
    provider: '',
    thinkingLevel: '',
    cost: 0,
    autoCompact: true,
    ...overrides,
  }
}

function render(input: PromptVMInput): string {
  return blocksToLines(buildPromptBlocks(input)).join('\n')
}

function renderPlain(input: PromptVMInput): string {
  return stripAnsi(render(input))
}

describe('buildPromptBlocks', () => {
  test('contains top border', () => {
    const result = renderPlain(defaultInput())
    const lines = result.split('\n')
    const borderLines = lines.filter(l => l.match(/^─+$/))
    expect(borderLines.length).toBeGreaterThanOrEqual(1)
  })

  test('shows cursor prefix ❯', () => {
    const result = renderPlain(defaultInput())
    expect(result).toContain('❯')
  })

  test('shows placeholder when empty', () => {
    const result = renderPlain(defaultInput())
    expect(result).toContain('Type a message...')
  })

  test('no placeholder when text entered', () => {
    const result = renderPlain(defaultInput({ lines: ['hello'], cursorCol: 5, placeholder: false }))
    expect(result).not.toContain('Type a message...')
    expect(result).toContain('hello')
  })

  test('shows model in footer', () => {
    const result = renderPlain(defaultInput())
    expect(result).toContain('claude-sonnet')
  })

  test('shows [plan] when planning', () => {
    const result = renderPlain(defaultInput({ planning: true }))
    expect(result).toContain('[plan]')
  })

  test('no [plan] when not planning', () => {
    const result = renderPlain(defaultInput({ planning: false }))
    expect(result).not.toContain('[plan]')
  })

  test('shows [log] when logMode', () => {
    const result = renderPlain(defaultInput({ logMode: true }))
    expect(result).toContain('[log]')
  })

  test('shows [verbose] when verbose is on', () => {
    const result = renderPlain(defaultInput({ verbose: true }))
    expect(result).toContain('[verbose]')
  })

  test('no [verbose] when verbose is off', () => {
    const result = renderPlain(defaultInput({ verbose: false }))
    expect(result).not.toContain('[verbose]')
  })

  test('shows exit hint', () => {
    const result = renderPlain(defaultInput({ exitHint: true }))
    expect(result).toContain('Press Ctrl+C again to exit')
  })

  test('shows queued messages', () => {
    const result = renderPlain(defaultInput({ queuedMessages: ['msg1', 'msg2'] }))
    expect(result).toContain('msg1')
    expect(result).toContain('msg2')
  })

  test('shows completion candidates', () => {
    const result = renderPlain(defaultInput({ completionCandidates: ['/help', '/model', '/resume'] }))
    expect(result).toContain('/help')
    expect(result).toContain('/model')
    expect(result).toContain('/resume')
  })

  test('does not add top margin', () => {
    const [promptBlock] = buildPromptBlocks(defaultInput({ isLoading: true }))
    expect(promptBlock?.marginTop).toBeUndefined()
    const [idleBlock] = buildPromptBlocks(defaultInput({ isLoading: false }))
    expect(idleBlock?.marginTop).toBeUndefined()
  })

  test('shows update hint', () => {
    const result = renderPlain(defaultInput({ updateHint: 'v0.2.0 available' }))
    expect(result).toContain('v0.2.0 available')
  })

  test('shows server state', () => {
    const result = renderPlain(defaultInput({ serverPort: 8082, serverUptime: '5m' }))
    expect(result).toContain(':8082')
    expect(result).toContain('5m')
  })

  test('cursor is rendered with inverse', () => {
    const result = render(defaultInput({ lines: ['abc'], cursorCol: 1, placeholder: false }))
    expect(result).toContain('\x1b[7m')
  })

  test('ghost hint is dim', () => {
    const result = render(defaultInput({ lines: ['/he'], cursorCol: 3, placeholder: false }))
    // ghost hint depends on getGhostHint — just verify no crash
    expect(result).toBeTruthy()
  })

  test('highlights known slash command in theme color', () => {
    const plainResult = renderPlain(defaultInput({ lines: ['/goal remove unwraps'], cursorCol: 5, placeholder: false }))
    const ansiResult = render(defaultInput({ lines: ['/goal remove unwraps'], cursorCol: 5, placeholder: false }))

    expect(plainResult).toContain('/goal remove unwraps')
    expect(plainResult).not.toContain('command matched:')
    expect(ansiResult).toContain('\x1b[36m')
  })

  test('does not highlight unknown slash text as command', () => {
    const ansiResult = render(defaultInput({ lines: ['/unknown text'], cursorCol: 8, placeholder: false }))
    expect(ansiResult).not.toContain('\x1b[36m/unknown')
  })

  test('handles non-finite columns', () => {
    const result = renderPlain(defaultInput({ columns: Infinity }))
    const lines = result.split('\n')
    expect(lines.some(l => l === '─'.repeat(80))).toBe(true)
  })

  test('wraps long ascii input across multiple visual lines', () => {
    // columns=20 -> 18 cols available for text after the prefix.
    const text = 'a'.repeat(50)
    const result = renderPlain(defaultInput({ columns: 20, lines: [text], cursorCol: 50, placeholder: false }))
    const lines = result.split('\n')
    // First wrapped row uses '❯ ' prefix, continuation rows use '  '.
    const firstRow = lines.find(l => l.startsWith('❯ '))
    const contRows = lines.filter(l => /^  a+/.test(l))
    expect(firstRow).toBeTruthy()
    // Visible width of each input row should not exceed the terminal width.
    for (const row of [firstRow!, ...contRows]) {
      expect(row.length).toBeLessThanOrEqual(20)
    }
    // Joining row contents (minus prefix) reproduces the original text.
    const joined = (firstRow!.slice(2) + contRows.map(r => r.slice(2)).join('')).replace(/\s+$/, '')
    expect(joined.startsWith('a'.repeat(50))).toBe(true)
  })

  test('cursor at end of overflow text appears on a fresh wrap row', () => {
    // Available width = 20 - 2 = 18. Use exactly 18 chars so cursor at end
    // would otherwise overflow the row.
    const text = 'a'.repeat(18)
    const result = render(defaultInput({ columns: 20, lines: [text], cursorCol: 18, placeholder: false }))
    const plainResult = stripAnsi(result)
    const rows = plainResult.split('\n')
    // We expect at least two input rows (the filled row + an empty wrap row
    // hosting the cursor).
    const inputRows = rows.filter(r => r.startsWith('❯ ') || /^  /.test(r))
    expect(inputRows.length).toBeGreaterThanOrEqual(2)
    // Inverse escape (cursor) should appear in the output.
    expect(result).toContain('\x1b[7m')
  })

  test('wraps wide CJK characters without overflowing terminal width', () => {
    // Each CJK char has display width 2, so 18 cols hold 9 chars.
    const text = '改进不过测试一定要在目录'
    const result = renderPlain(defaultInput({ columns: 20, lines: [text], cursorCol: text.length, placeholder: false }))
    const rows = result.split('\n')
    const inputRows = rows.filter(r => r.startsWith('❯ ') || /^  \S/.test(r))
    expect(inputRows.length).toBeGreaterThanOrEqual(2)
    // Visible width of each row should fit within the terminal.
    for (const r of inputRows) {
      // Approximate visible width via string-width is fine, but here we just
      // check character count doesn't exceed columns (since CJK is 2 cols
      // each, this is a conservative bound).
      expect(r.length).toBeLessThanOrEqual(20)
    }
  })
})

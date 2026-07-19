import { beforeAll, describe, expect, test } from 'bun:test'
import chalk from 'chalk'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'
import { CURSOR_MARKER } from '../src/term/renderer.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { buildPromptBlocks, buildPromptFooterBlocks, type PromptVMInput } from '../src/term/viewmodel/prompt.js'

beforeAll(() => { chalk.level = 3 })

function defaultInput(overrides: Partial<PromptVMInput> = {}): PromptVMInput {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    active: true,
    completion: null,
    ghostHint: '',
    columns: 80,
    rows: 24,
    placeholder: true,
    model: 'claude-sonnet',
    provider: '',
    thinkingLevel: '',
    planning: false,
    logMode: false,
    dashboardUrl: null,
    exitHint: false,
    cwd: '/Users/test/project',
    gitBranch: 'main',
    contextTokens: 0,
    contextWindow: 0,
    ...overrides,
  }
}

function render(input: PromptVMInput): string {
  return blocksToLines(buildPromptBlocks(input)).join('\n')
}

function renderPlain(input: PromptVMInput): string {
  return stripAnsi(render(input)).replaceAll(CURSOR_MARKER, '')
}

function completion(labels: string[], selectedIndex = 0) {
  return {
    items: labels.map(label => ({ label, value: `${label} `, description: `Description for ${label}` })),
    selectedIndex,
    replaceStart: 0,
    replaceEnd: 2,
  }
}

describe('prompt editor', () => {
  test('renders border, prompt, cursor and placeholder', () => {
    const ansi = render(defaultInput())
    const plain = stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')
    expect(plain).toContain('─'.repeat(80))
    expect(plain).toContain('❯')
    expect(plain).toContain('Type a message...')
    expect(ansi).toContain('\x1b[7m')
  })

  test('renders input and known command styling', () => {
    const input = defaultInput({ lines: ['/plan remove unwraps'], cursorCol: 5, placeholder: false })
    expect(renderPlain(input)).toContain('/plan remove unwraps')
    expect(render(input)).toContain('\x1b[36m')
  })

  test('does not style unknown slash text as a command', () => {
    const ansi = render(defaultInput({ lines: ['/unknown text'], cursorCol: 8, placeholder: false }))
    expect(ansi).not.toContain('\x1b[36m/unknown')
  })

  test('wraps ASCII and CJK input within terminal width', () => {
    for (const text of ['a'.repeat(50), '改进不过测试一定要在目录']) {
      const plain = renderPlain(defaultInput({ columns: 20, lines: [text], cursorCol: text.length, placeholder: false }))
      for (const row of plain.split('\n')) expect(stringWidth(row)).toBeLessThanOrEqual(20)
    }
  })

  test('puts an end cursor on a fresh row when the previous row is full', () => {
    const ansi = render(defaultInput({ columns: 20, lines: ['a'.repeat(18)], cursorCol: 18, placeholder: false }))
    const rows = stripAnsi(ansi).split('\n').filter(row => row.startsWith('❯ ') || row.startsWith('  '))
    expect(rows.length).toBeGreaterThanOrEqual(2)
    expect(ansi).toContain('\x1b[7m')
  })

  test('limits long input to 30 percent of terminal rows and follows the cursor', () => {
    const lines = Array.from({ length: 12 }, (_, index) => `line ${index + 1}`)
    const plain = renderPlain(defaultInput({
      lines,
      cursorLine: 11,
      cursorCol: lines[11]!.length,
      rows: 20,
      placeholder: false,
    }))
    expect(plain).toContain('↑ 6 lines')
    expect(plain).not.toContain('line 1\n')
    expect(plain).toContain('line 12')
  })

  test('shows lines below when the cursor is near the top', () => {
    const lines = Array.from({ length: 10 }, (_, index) => `row ${index + 1}`)
    const plain = renderPlain(defaultInput({ lines, cursorLine: 0, cursorCol: 0, rows: 20, placeholder: false }))
    expect(plain).toContain('↓ 4 lines')
    expect(plain).toContain('row 1')
    expect(plain).not.toContain('row 10')
  })

  test('renders a five-row completion viewport with descriptions and position', () => {
    const plain = renderPlain(defaultInput({ completion: completion(['/a', '/b', '/c', '/d', '/e', '/f'], 5) }))
    expect(plain).not.toContain('/a')
    expect(plain).toContain('/f')
    expect(plain).toContain('Description for /f')
    expect(plain).toContain('6/6')
  })

  test('keeps completion rows within terminal width', () => {
    const plain = renderPlain(defaultInput({
      columns: 24,
      completion: completion(['/very-long-command-one', '/very-long-command-two']),
    }))
    for (const row of plain.split('\n')) expect(stringWidth(row)).toBeLessThanOrEqual(24)
  })

  test('preserves prompt spacing and attached layout', () => {
    expect(buildPromptBlocks(defaultInput())[0]!.marginTop).toBe(1)
    expect(buildPromptBlocks(defaultInput(), { attachedAbove: true })[0]!.marginTop).toBe(0)
  })

  test('shows exit hint', () => {
    expect(renderPlain(defaultInput({ exitHint: true }))).toContain('Press Ctrl+C again to exit')
  })

  test('uses fallback dimensions for non-finite terminal sizes', () => {
    const plain = renderPlain(defaultInput({ columns: Infinity, rows: Infinity }))
    expect(plain.split('\n')).toContain('─'.repeat(80))
  })
})

describe('prompt footer', () => {
  test('renders modes, repository state and model identity', () => {
    const plain = renderPlain(defaultInput({
      planning: true,
      logMode: true,
      provider: 'anthropic',
      thinkingLevel: 'xhigh',
      columns: 160,
    }))
    expect(plain).toContain('[log] [plan]')
    expect(plain).toContain('/Users/test/project (main)')
    expect(plain).toContain('claude-sonnet@anthropic • xhigh')
  })

  test('labels disabled thinking', () => {
    expect(renderPlain(defaultInput({ thinkingLevel: 'off' }))).toContain('thinking off')
  })

  test('renders context and dashboard when space allows', () => {
    const plain = renderPlain(defaultInput({
      columns: 220,
      contextTokens: 105800,
      contextWindow: 272000,
      dashboardUrl: 'http://127.0.0.1:8788',
    }))
    expect(plain).toContain('context: 38.9% (105.8k/272.0k)')
    expect(plain).toContain('http://127.0.0.1:8788')
    // Session token totals are call/log data, not footer state.
    expect(plain).not.toContain('↑')
    expect(plain).not.toContain('cache')
  })

  test('matches the full context footer format from the terminal', () => {
    const home = process.env.HOME || process.env.USERPROFILE || '/tmp/home'
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns: 160,
      cwd: `${home}/github/evotai/evot`,
      gitBranch: 'main',
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'high',
      contextTokens: 105800,
      contextWindow: 272000,
    }))).map(stripAnsi)[0]!

    expect(footer).toBe('~/github/evotai/evot (main) context: 38.9% (105.8k/272.0k) gpt-5.6-sol@anthropic • high')
  })

  test('drops low-priority segments as width narrows', () => {
    const plain = renderPlain(defaultInput({
      columns: 45,
      provider: 'anthropic',
      thinkingLevel: 'xhigh',
      dashboardUrl: 'http://127.0.0.1:8788',
      contextTokens: 86400,
      contextWindow: 320000,
    }))
    const footer = plain.split('\n').at(-2)!
    expect(footer).toContain('/Users/test/project')
    expect(footer).not.toContain('context:')
    expect(footer).not.toContain('dashboard')
    expect(stringWidth(footer)).toBeLessThanOrEqual(45)
  })

  test('truncates a wide CJK cwd only after optional segments are gone', () => {
    const columns = 24
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      cwd: '/项目/非常长的中文目录名称/子目录',
      gitBranch: 'feature/very-long-branch',
      model: 'a-very-long-model-name',
      provider: 'provider',
    }))).map(stripAnsi)[0]!
    expect(stringWidth(footer)).toBeLessThanOrEqual(columns)
    expect(footer).toStartWith('…')
  })

  test('footer remains available without the editor', () => {
    const lines = blocksToLines(buildPromptFooterBlocks(defaultInput({ provider: 'openai', model: 'gpt-5.6-sol' }))).map(stripAnsi)
    expect(lines).toHaveLength(2)
    expect(lines[0]).toContain('gpt-5.6-sol@openai')
    expect(lines[1]).toBe('')
    expect(lines.join('\n')).not.toContain('Type a message...')
  })
})

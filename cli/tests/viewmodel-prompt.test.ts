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

  test('cursor overlays the first ghost hint character without inserting a blank cell', () => {
    const input = defaultInput({ lines: ['/mo'], cursorCol: 3, placeholder: false, ghostHint: 'del  [<name>]' })
    const ansi = render(input)
    // No blank cell between typed text and ghost suffix: `/model  [<name>]`
    expect(stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')).toContain('/model  [<name>]')
    // Cursor block sits on the ghost's first character `d`
    expect(ansi).toContain('\x1b[7md\x1b[27m')
  })

  test('cursor at end of line without ghost hint still renders an inverse space', () => {
    const ansi = render(defaultInput({ lines: ['/mo'], cursorCol: 3, placeholder: false }))
    expect(ansi).toContain('\x1b[7m \x1b[27m')
  })

  test('ghost hint follows the cursor character when the cursor is not at end of line', () => {
    const input = defaultInput({ lines: ['/model '], cursorCol: 6, placeholder: false, ghostHint: '[<name>]' })
    const ansi = render(input)
    expect(stripAnsi(ansi).replaceAll(CURSOR_MARKER, '')).toContain('/model [<name>]')
    expect(ansi).toContain('\x1b[7m \x1b[27m')
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
    expect(plain).toContain('context: 38.9% (105.8k/272k)')
    expect(plain).toContain('http://127.0.0.1:8788')
    // Session token totals are call/log data, not footer state.
    expect(plain).not.toContain('↑')
    expect(plain).not.toContain('cache')
  })

  test('right-aligns the dashboard link', () => {
    const columns = 120
    const footer = blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      dashboardUrl: 'http://127.0.0.1:8082',
    }))).map(stripAnsi)[0]!

    expect(stringWidth(footer)).toBe(columns)
    expect(footer).toEndWith('dashboard http://127.0.0.1:8082')
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

    expect(footer).toBe('~/github/evotai/evot (main) │ gpt-5.6-sol@anthropic • high │ context: 38.9% (105.8k/272k)')
  })

  test('shows last-call cache hit rate and drops it before provider when narrow', () => {
    const footerAt = (columns: number) => blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'high',
      contextTokens: 105800,
      contextWindow: 272000,
      cacheUsage: { inputTokens: 4, cacheReadTokens: 200_000, cacheWriteTokens: 500 },
    }))).map(stripAnsi)[0]!

    const wide = footerAt(160)
    expect(wide).toContain('context: 38.9% (105.8k/272k)')
    expect(wide).toContain('cache: 99.7%')

    // cache is dropped before provider/branch/context.
    const narrow = footerAt(80)
    expect(narrow).not.toContain('cache:')
    expect(narrow).toContain('@anthropic')
    expect(narrow).toContain('context: 38.9%')
  })

  test('hides the cache segment when the last call reported no cache activity', () => {
    const plain = renderPlain(defaultInput({
      columns: 200,
      contextTokens: 105800,
      contextWindow: 272000,
      cacheUsage: { inputTokens: 50_000, cacheReadTokens: 0, cacheWriteTokens: 0 },
    }))
    expect(plain).not.toContain('cache:')
  })

  test('cold cache write shows 0% instead of hiding', () => {
    const plain = renderPlain(defaultInput({
      columns: 200,
      contextTokens: 105800,
      contextWindow: 272000,
      cacheUsage: { inputTokens: 500, cacheReadTokens: 0, cacheWriteTokens: 20_000 },
    }))
    expect(plain).toContain('cache: 0%')
  })

  test('degrades footer details in priority order as width narrows', () => {
    const footerAt = (columns: number) => blocksToLines(buildPromptFooterBlocks(defaultInput({
      columns,
      model: 'gpt-5.6-sol',
      provider: 'anthropic',
      thinkingLevel: 'max',
      dashboardUrl: 'http://127.0.0.1:8082',
      contextTokens: 105800,
      contextWindow: 272000,
    }))).map(stripAnsi)[0]!

    const withoutDashboard = footerAt(119)
    expect(withoutDashboard).not.toContain('dashboard')
    expect(withoutDashboard).toContain('context: 38.9% (105.8k/272k)')

    const compactContext = footerAt(80)
    expect(compactContext).toContain('gpt-5.6-sol@anthropic • max')
    expect(compactContext).toContain('context: 38.9%')
    expect(compactContext).not.toContain('105.8k')

    const withoutProvider = footerAt(70)
    expect(withoutProvider).toContain('gpt-5.6-sol • max')
    expect(withoutProvider).not.toContain('@anthropic')
    expect(withoutProvider).toContain('(main)')

    const withoutBranch = footerAt(60)
    expect(withoutBranch).not.toContain('(main)')
    expect(withoutBranch).toContain('context: 38.9%')

    const withoutContext = footerAt(50)
    expect(withoutContext).toContain('gpt-5.6-sol • max')
    expect(withoutContext).not.toContain('context:')

    for (const columns of [119, 80, 70, 60, 50, 30, 20]) {
      expect(stringWidth(footerAt(columns))).toBeLessThanOrEqual(columns)
    }
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

import stringWidth from 'string-width'
import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
import type { CompletionMenu } from '../input/editor.js'
import { nextGraphemeBoundary, wrapEditorText } from '../input/grapheme.js'
import { CURSOR_MARKER } from '../renderer.js'
import { line, block, plain, dim, colored, inverse, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'
import { formatCacheHitPercent, type PromptUsageBuckets } from '../../render/cache.js'

export interface PromptVMInput {
  lines: string[]
  cursorLine: number
  cursorCol: number
  active: boolean
  completion: CompletionMenu | null
  ghostHint: string
  columns: number
  rows: number
  placeholder: boolean
  model: string
  provider: string
  thinkingLevel: string
  planning: boolean
  logMode: boolean
  dashboardUrl: string | null
  exitHint: boolean
  cwd: string
  gitBranch: string | null
  contextTokens: number
  contextWindow: number
  /** Prompt buckets of the last billed request; null hides the cache segment. */
  cacheUsage?: PromptUsageBuckets | null
}

export interface PromptLayoutOptions {
  attachedAbove?: boolean
}

const KNOWN_COMMANDS = new Set(
  [...COMMANDS, ...HIDDEN_COMMANDS].flatMap(command => [command.name, ...(command.aliases ?? [])]),
)
const COMPLETION_ROWS = 5

export function buildPromptBlocks(input: PromptVMInput, options: PromptLayoutOptions = {}): ViewBlock[] {
  const columns = finiteSize(input.columns, 80)
  const rows = finiteSize(input.rows, 24)
  const visual = buildInputLines(input, columns)
  const maxInputRows = Math.max(5, Math.floor(rows * 0.3))
  const start = Math.max(0, Math.min(visual.cursorIndex - maxInputRows + 1, visual.lines.length - maxInputRows))
  const end = Math.min(visual.lines.length, start + maxInputRows)
  const above = start
  const below = visual.lines.length - end

  const blocks: ViewBlock[] = [
    block([borderLine(columns, above > 0 ? `↑ ${above} ${above === 1 ? 'line' : 'lines'}` : '')], options.attachedAbove ? 0 : 1),
    block(visual.lines.slice(start, end)),
  ]

  const completionLines = buildCompletionLines(input.completion, columns)
  if (completionLines.length > 0) blocks.push(block(completionLines))
  blocks.push(block([borderLine(columns, below > 0 ? `↓ ${below} ${below === 1 ? 'line' : 'lines'}` : '')]))

  if (input.exitHint) blocks.push(block([line(dim('  Press Ctrl+C again to exit'))]))
  blocks.push(...buildPromptFooterBlocks(input))
  return blocks
}

export function buildPromptFooterBlocks(input: PromptVMInput): ViewBlock[] {
  return [buildFooter(input, finiteSize(input.columns, 80)), block([line(plain(''))])]
}

function buildInputLines(input: PromptVMInput, columns: number): { lines: StyledLine[]; cursorIndex: number } {
  const lines: StyledLine[] = []
  const width = Math.max(1, columns - 2)
  let cursorIndex = 0

  for (let lineIndex = 0; lineIndex < input.lines.length; lineIndex++) {
    const text = input.lines[lineIndex]!
    const active = input.active && lineIndex === input.cursorLine
    if (active && text === '' && input.lines.length === 1 && input.placeholder) {
      cursorIndex = lines.length
      lines.push(line(colored('❯ ', 'cyan', { bold: true }), plain(CURSOR_MARKER), inverse(' '), dim(' Type a message...')))
      continue
    }

    const chunks = wrapTextByWidth(text, width)
    let cursorChunk = -1
    if (active) {
      cursorChunk = chunks.findIndex(chunk => input.cursorCol >= chunk.start && input.cursorCol < chunk.end)
      if (cursorChunk < 0) {
        const last = chunks[chunks.length - 1]!
        if (input.cursorCol === text.length && stringWidth(text.slice(last.start, last.end)) >= width) {
          chunks.push({ start: text.length, end: text.length })
        }
        cursorChunk = chunks.length - 1
      }
    }

    for (let chunkIndex = 0; chunkIndex < chunks.length; chunkIndex++) {
      const chunk = chunks[chunkIndex]!
      const prefix = lineIndex === 0 && chunkIndex === 0
        ? colored('❯ ', 'cyan', { bold: true })
        : plain('  ')
      const textChunk = text.slice(chunk.start, chunk.end)
      if (!active || chunkIndex !== cursorChunk) {
        lines.push(line(prefix, ...(textChunk ? styleInputText(textChunk) : [plain(' ')])))
        continue
      }

      cursorIndex = lines.length
      const cursorCol = Math.max(0, input.cursorCol - chunk.start)
      const cursorEnd = nextGraphemeBoundary(textChunk, cursorCol)
      const before = textChunk.slice(0, cursorCol)
      const cursorChar = textChunk.slice(cursorCol, cursorEnd) || ' '
      const after = textChunk.slice(cursorEnd)
      const ghost = !input.completion && chunk.end === text.length ? input.ghostHint : ''
      const spans: StyledSpan[] = [prefix, ...styleInputText(before), plain(CURSOR_MARKER)]
      if (ghost && cursorCol >= textChunk.length) {
        // Cursor sits on the first ghost grapheme (pi-style: overlay, don't
        // insert a blank cell that would visually split the suggested word).
        const ghostEnd = nextGraphemeBoundary(ghost, 0)
        spans.push(inverse(ghost.slice(0, ghostEnd)), dim(ghost.slice(ghostEnd)))
      } else {
        spans.push(inverse(cursorChar), ...styleInputText(after))
        if (ghost) spans.push(dim(ghost))
      }
      lines.push(line(...spans))
    }
  }

  return { lines, cursorIndex }
}

function buildCompletionLines(menu: CompletionMenu | null, columns: number): StyledLine[] {
  if (!menu || menu.items.length === 0) return []
  const start = Math.max(0, Math.min(menu.selectedIndex - COMPLETION_ROWS + 1, menu.items.length - COMPLETION_ROWS))
  const end = Math.min(menu.items.length, start + COMPLETION_ROWS)
  const labelWidth = Math.min(
    Math.max(...menu.items.slice(start, end).map(item => stringWidth(item.label))),
    Math.max(1, Math.floor(columns * 0.45)),
  )
  const lines: StyledLine[] = []

  for (let index = start; index < end; index++) {
    const item = menu.items[index]!
    const selected = index === menu.selectedIndex
    const label = truncateToWidth(item.label, labelWidth)
    const padding = ' '.repeat(Math.max(0, labelWidth - stringWidth(label)))
    const prefix = selected ? colored('❯ ', 'cyan', { bold: true }) : plain('  ')
    const labelSpan = selected ? colored(label, 'cyan', { bold: true }) : plain(label)
    const descriptionWidth = Math.max(0, columns - 2 - labelWidth - 2)
    const description = item.description && descriptionWidth > 0
      ? truncateToWidth(item.description, descriptionWidth)
      : ''
    lines.push(line(prefix, labelSpan, plain(padding), description ? dim(`  ${description}`) : plain('')))
  }

  if (menu.items.length > COMPLETION_ROWS) {
    lines.push(line(dim(`  ${menu.selectedIndex + 1}/${menu.items.length}`)))
  }
  return lines
}

function buildFooter(input: PromptVMInput, columns: number): ViewBlock {
  const mode = `${input.logMode ? '[log] ' : ''}${input.planning ? '[plan] ' : ''}`
  const cwd = compactCwd(input.cwd)
  const contextPercent = input.contextWindow > 0
    ? input.contextTokens / input.contextWindow * 100
    : 0
  const layouts: FooterLayout[] = [
    { dashboard: true, context: 'full', cache: true, provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'full', cache: true, provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'compact', cache: true, provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'compact', cache: false, provider: true, branch: true, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'compact', cache: false, provider: false, branch: true, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'compact', cache: false, provider: false, branch: false, thinking: true, model: true, truncateCwd: false },
    { dashboard: false, context: 'hidden', cache: false, provider: false, branch: false, thinking: true, model: true, truncateCwd: true },
    { dashboard: false, context: 'hidden', cache: false, provider: false, branch: false, thinking: false, model: true, truncateCwd: true },
    { dashboard: false, context: 'hidden', cache: false, provider: false, branch: false, thinking: false, model: false, truncateCwd: true },
  ]

  for (const layout of layouts) {
    const candidate = buildFooterCandidate(input, mode, cwd, contextPercent, layout, columns)
    if (footerCandidateWidth(candidate) <= columns) return block([renderFooterCandidate(candidate, columns)])
  }

  return block([line(dim(truncateTailToWidth(`${mode}${cwd}`, columns)))])
}

type FooterContextDetail = 'full' | 'compact' | 'hidden'

interface FooterLayout {
  dashboard: boolean
  context: FooterContextDetail
  cache: boolean
  provider: boolean
  branch: boolean
  thinking: boolean
  model: boolean
  truncateCwd: boolean
}

interface FooterCandidate {
  left: StyledSpan[]
  dashboard: StyledSpan[] | null
}

function buildFooterCandidate(
  input: PromptVMInput,
  mode: string,
  cwd: string,
  contextPercent: number,
  layout: FooterLayout,
  columns: number,
): FooterCandidate {
  const dashboard = layout.dashboard && input.dashboardUrl
    ? [
        { text: 'dashboard ', hex: '#7fae7f' } satisfies StyledSpan,
        { text: input.dashboardUrl, hex: '#7fae7f', link: input.dashboardUrl } satisfies StyledSpan,
      ]
    : null

  const buildLeft = (location: string): StyledSpan[] => {
    const groups: StyledSpan[][] = [[dim(location)]]
    if (layout.model && input.model) {
      const identity: StyledSpan[] = [dim(input.model)]
      if (layout.provider && input.provider) identity.push(dim(`@${input.provider}`))
      if (layout.thinking && input.thinkingLevel) {
        const thinking = input.thinkingLevel === 'off' ? 'thinking off' : input.thinkingLevel
        identity.push(dim(` • ${thinking}`))
      }
      groups.push(identity)
    }
    if (layout.context !== 'hidden' && contextPercent > 0) {
      const warning = contextPercent > 90 ? ' ⚠' : ''
      const detail = layout.context === 'full'
        ? ` (${formatContextTokens(input.contextTokens)}/${formatContextTokens(input.contextWindow)})`
        : ''
      const text = `context: ${contextPercent.toFixed(1)}%${detail}${warning}`
      groups.push([
        contextPercent > 90
          ? colored(text, 'red')
          : contextPercent > 70
            ? colored(text, 'yellow')
            : dim(text),
      ])
    }
    if (layout.cache && input.cacheUsage
      && input.cacheUsage.cacheReadTokens + input.cacheUsage.cacheWriteTokens > 0) {
      const pct = formatCacheHitPercent(
        input.cacheUsage.inputTokens,
        input.cacheUsage.cacheReadTokens,
        input.cacheUsage.cacheWriteTokens,
      )
      groups.push([dim(`cache: ${pct}%`)])
    }
    return groups.flatMap((group, index) => index === 0 ? group : [dim(' │ '), ...group])
  }

  const branch = layout.branch && input.gitBranch ? ` (${input.gitBranch})` : ''
  const fullLocation = `${mode}${cwd}${branch}`
  let left = buildLeft(fullLocation)
  let candidate = { left, dashboard }
  if (!layout.truncateCwd || footerCandidateWidth(candidate) <= columns) return candidate

  const fixedWidth = footerCandidateWidth(candidate) - stringWidth(fullLocation)
  const availableLocationWidth = Math.max(1, columns - fixedWidth)
  left = buildLeft(truncateTailToWidth(`${mode}${cwd}`, availableLocationWidth))
  candidate = { left, dashboard }
  return candidate
}

function footerCandidateWidth(candidate: FooterCandidate): number {
  const left = spansWidth(candidate.left)
  return candidate.dashboard ? left + 2 + spansWidth(candidate.dashboard) : left
}

function renderFooterCandidate(candidate: FooterCandidate, columns: number): StyledLine {
  if (!candidate.dashboard) return line(...candidate.left)
  const padding = columns - spansWidth(candidate.left) - spansWidth(candidate.dashboard)
  return line(...candidate.left, plain(' '.repeat(Math.max(2, padding))), ...candidate.dashboard)
}

function spansWidth(spans: StyledSpan[]): number {
  return stringWidth(spans.map(span => span.text).join(''))
}

function compactCwd(cwd: string): string {
  const home = process.env.HOME || process.env.USERPROFILE || ''
  return home && cwd.startsWith(home) ? `~${cwd.slice(home.length)}` : cwd
}

function styleInputText(text: string): StyledSpan[] {
  const match = /^(\/[a-z]+)(\s.*)?$/.exec(text)
  if (!match || !KNOWN_COMMANDS.has(match[1]!)) return [plain(text)]
  return [
    colored(match[1]!, 'cyan', { bold: true }),
    ...(match[2] ? [plain(match[2])] : []),
  ]
}

function borderLine(columns: number, label: string): StyledLine {
  if (!label) return line(dim('─'.repeat(columns)))
  const prefix = `── ${label} `
  return line(dim(truncateToWidth(prefix, columns) + '─'.repeat(Math.max(0, columns - stringWidth(prefix)))))
}

function finiteSize(value: number, fallback: number): number {
  return Number.isFinite(value) ? Math.max(1, Math.floor(value)) : fallback
}

function truncateToWidth(text: string, width: number): string {
  if (width <= 0) return ''
  if (stringWidth(text) <= width) return text
  if (width <= 1) return '…'.slice(0, width)
  let result = ''
  let used = 0
  for (const char of text) {
    const charWidth = stringWidth(char)
    if (used + charWidth > width - 1) break
    result += char
    used += charWidth
  }
  return `${result}…`
}

function truncateTailToWidth(text: string, width: number): string {
  if (width <= 0) return ''
  if (stringWidth(text) <= width) return text
  if (width <= 1) return '…'.slice(0, width)
  let result = ''
  let used = 0
  for (const char of [...text].reverse()) {
    const charWidth = stringWidth(char)
    if (used + charWidth > width - 1) break
    result = char + result
    used += charWidth
  }
  return `…${result}`
}

function formatContextTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 1000000) return `${(count / 1000).toFixed(1)}k`
  return `${(count / 1000000).toFixed(1)}M`
}

export function wrapTextByWidth(text: string, width: number): { start: number; end: number }[] {
  return wrapEditorText(text, width)
}

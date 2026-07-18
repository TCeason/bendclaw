import stringWidth from 'string-width'
import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
import type { CompletionMenu } from '../input/editor.js'
import { CURSOR_MARKER } from '../renderer.js'
import { line, block, plain, dim, colored, inverse, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'

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
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  contextTokens: number
  contextWindow: number
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
      const cursorCol = input.cursorCol - chunk.start
      const before = textChunk.slice(0, cursorCol)
      const cursorChar = textChunk[cursorCol] ?? ' '
      const after = textChunk.slice(cursorCol + 1)
      const spans: StyledSpan[] = [
        prefix,
        ...styleInputText(before),
        plain(CURSOR_MARKER),
        inverse(cursorChar),
        ...styleInputText(after),
      ]
      if (!input.completion && input.ghostHint && chunk.end === text.length) spans.push(dim(input.ghostHint))
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

  const segments: FooterSegment[] = [
    { priority: 100, spans: [dim(`${mode}${cwd}`)] },
  ]
  if (input.gitBranch) segments.push({ priority: 90, spans: [dim(` (${input.gitBranch})`)] })
  if (contextPercent > 0) {
    const context = ` context: ${contextPercent.toFixed(1)}% (${formatContextTokens(input.contextTokens)}/${formatContextTokens(input.contextWindow)})`
    const span = contextPercent > 90
      ? colored(context, 'red')
      : contextPercent > 70
        ? colored(context, 'yellow')
        : dim(context)
    segments.push({ priority: 80, spans: [span] })
  }
  if (input.model) segments.push({ priority: 70, spans: [dim(` ${input.model}`)] })
  if (input.provider) segments.push({ priority: 65, spans: [dim(`@${input.provider}`)] })
  if (input.thinkingLevel) {
    const thinking = input.thinkingLevel === 'off' ? 'thinking off' : input.thinkingLevel
    segments.push({ priority: 60, spans: [dim(` • ${thinking}`)] })
  }

  const stats = footerStats(input)
  if (stats) segments.push({ priority: 50, spans: [dim(` ${stats}`)] })
  if (input.dashboardUrl) {
    segments.push({
      priority: 40,
      spans: [
        { text: ' dashboard ', hex: '#7fae7f' },
        { text: input.dashboardUrl, hex: '#7fae7f', link: input.dashboardUrl },
      ],
    })
  }

  while (footerWidth(segments) > columns && segments.length > 1) {
    let removeIndex = 1
    for (let index = 2; index < segments.length; index++) {
      if (segments[index]!.priority < segments[removeIndex]!.priority) removeIndex = index
    }
    segments.splice(removeIndex, 1)
  }

  if (footerWidth(segments) > columns) {
    return block([line(dim(truncateTailToWidth(`${mode}${cwd}`, columns)))])
  }
  return block([line(...segments.flatMap(segment => segment.spans))])
}

interface FooterSegment {
  priority: number
  spans: StyledSpan[]
}

function footerStats(input: PromptVMInput): string {
  const parts: string[] = []
  if (input.inputTokens > 0) parts.push(`↑${formatTokens(input.inputTokens)}`)
  if (input.outputTokens > 0) parts.push(`↓${formatTokens(input.outputTokens)}`)
  if (input.cacheReadTokens > 0) parts.push(`cache ${formatTokens(input.cacheReadTokens)}`)
  return parts.join(' ')
}

function footerWidth(segments: FooterSegment[]): number {
  return stringWidth(segments.flatMap(segment => segment.spans).map(span => span.text).join(''))
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

function formatTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`
  if (count < 1000000) return `${Math.round(count / 1000)}k`
  return `${(count / 1000000).toFixed(1)}M`
}

function formatContextTokens(count: number): string {
  if (count < 1000) return `${count}`
  if (count < 1000000) return `${(count / 1000).toFixed(1)}k`
  return `${(count / 1000000).toFixed(1)}M`
}

export function wrapTextByWidth(text: string, width: number): { start: number; end: number }[] {
  if (width <= 0 || text.length === 0) return [{ start: 0, end: text.length }]
  const chunks: { start: number; end: number }[] = []
  let start = 0
  let used = 0
  for (let index = 0; index < text.length; index++) {
    const charWidth = stringWidth(text[index]!)
    if (used + charWidth > width && index > start) {
      chunks.push({ start, end: index })
      start = index
      used = 0
    }
    used += charWidth
  }
  chunks.push({ start, end: text.length })
  return chunks
}

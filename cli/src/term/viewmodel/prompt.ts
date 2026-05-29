import stringWidth from 'string-width'
import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
import { CURSOR_MARKER } from '../renderer.js'
import { line, block, plain, dim, colored, inverse, type ViewBlock, type StyledLine, type StyledSpan } from './types.js'

export interface PromptVMInput {
  lines: string[]
  cursorLine: number
  cursorCol: number
  active: boolean
  model: string
  verbose: boolean
  planning: boolean
  logMode: boolean
  queuedMessages: string[]
  updateHint: string | null
  serverUptime: string | null
  serverPort: number | null
  exitHint: boolean
  completionCandidates: string[]
  ghostHint: string
  columns: number
  isLoading: boolean
  placeholder: boolean
  cwd: string
  gitRepo: string | null
  gitBranch: string | null
  // Footer stats
  inputTokens: number
  outputTokens: number
  cacheReadTokens: number
  contextTokens: number
  contextWindow: number
  provider: string
  thinkingLevel: string
  cost: number
  autoCompact: boolean
}

const KNOWN_COMMANDS = new Set(
  [...COMMANDS, ...HIDDEN_COMMANDS].flatMap(command => [command.name, ...(command.aliases ?? [])])
)

function styleInputText(text: string): StyledSpan[] {
  const match = /^(\/[a-z]+)(\s.*)?$/.exec(text)
  if (!match || !KNOWN_COMMANDS.has(match[1]!)) return [plain(text)]
  return [
    colored(match[1]!, 'cyan', { bold: true }),
    ...(match[2] ? [plain(match[2])] : []),
  ]
}
export function buildPromptBlocks(input: PromptVMInput): ViewBlock[] {
  const blocks: ViewBlock[] = []
  const columns = Number.isFinite(input.columns) ? Math.max(1, Math.floor(input.columns)) : 80
  const border = '─'.repeat(columns)

  blocks.push(block([line(dim(border))]))

  const inputLines: StyledLine[] = []
  // Visual width available for input text after the 2-column prefix (`❯ ` / `  `).
  const availWidth = Math.max(1, columns - 2)
  for (let i = 0; i < input.lines.length; i++) {
    const text = input.lines[i]!
    const isActiveLine = i === input.cursorLine && input.active

    if (isActiveLine && text === '' && input.lines.length === 1 && input.placeholder) {
      const prefix = colored('❯ ', 'cyan', { bold: true })
      inputLines.push(line(prefix, plain(CURSOR_MARKER), inverse(' '), dim(' Type a message...')))
      continue
    }

    const chunks = wrapTextByWidth(text, availWidth)
    let cursorChunkIdx = -1
    if (isActiveLine) {
      for (let k = 0; k < chunks.length; k++) {
        const c = chunks[k]!
        if (input.cursorCol >= c.start && input.cursorCol < c.end) {
          cursorChunkIdx = k
          break
        }
      }
      if (cursorChunkIdx === -1) {
        // Cursor is at the very end of text. If the last chunk filled the row,
        // append an empty chunk so the cursor renders on a fresh wrap line
        // instead of falling off-screen.
        const last = chunks[chunks.length - 1]!
        const lastWidth = stringWidth(text.slice(last.start, last.end))
        if (lastWidth >= availWidth && input.cursorCol === text.length) {
          chunks.push({ start: text.length, end: text.length })
        }
        cursorChunkIdx = chunks.length - 1
      }
    }

    for (let k = 0; k < chunks.length; k++) {
      const c = chunks[k]!
      const isFirstVisual = i === 0 && k === 0
      const prefix: StyledSpan = isFirstVisual
        ? colored('❯ ', 'cyan', { bold: true })
        : plain('  ')
      const chunkText = text.slice(c.start, c.end)

      if (isActiveLine && k === cursorChunkIdx) {
        const localCursorCol = input.cursorCol - c.start
        const before = chunkText.slice(0, localCursorCol)
        const cursorChar = chunkText[localCursorCol] ?? ' '
        const after = chunkText.slice(localCursorCol + 1)
        const spans: StyledSpan[] = [prefix, ...styleInputText(before), plain(CURSOR_MARKER), inverse(cursorChar), ...styleInputText(after)]
        // Only show the ghost hint on the wrap row that contains the end of the text.
        if (input.ghostHint && c.end === text.length) spans.push(dim(input.ghostHint))
        inputLines.push(line(...spans))
      } else {
        inputLines.push(line(prefix, ...(chunkText ? styleInputText(chunkText) : [plain(' ')])))
      }
    }
  }
  blocks.push(block(inputLines))

  if (input.completionCandidates.length > 1) {
    blocks.push(block([line(dim('  ' + input.completionCandidates.join('  ')))]))
  }

  blocks.push(block([line(dim(border))]))

  if (input.exitHint) {
    blocks.push(block([line(dim('  Press Ctrl+C again to exit'))]))
  }

  if (input.queuedMessages.length > 0) {
    const qLines: StyledLine[] = input.queuedMessages.map(msg =>
      line(dim('  ❯ '), dim(msg))
    )
    blocks.push(block(qLines))
  }

  const footerBlocks = buildFooter(input, columns)
  blocks.push(footerBlocks)
  blocks.push(block([line(plain(''))]))

  return blocks
}

function buildFooter(input: PromptVMInput, columns: number): ViewBlock {
  // Single line: [plan][verbose] cwd (branch) stats    (provider) model • thinking
  const leftSpans: StyledSpan[] = []

  if (input.logMode) {
    leftSpans.push(dim('[log] '))
  }
  if (input.planning) {
    leftSpans.push(dim('[plan] '))
  }
  if (input.verbose) {
    leftSpans.push(dim('[verbose] '))
  }

  // cwd + branch
  let cwd = input.cwd
  const home = process.env.HOME || process.env.USERPROFILE || ''
  if (home && cwd.startsWith(home)) {
    cwd = '~' + cwd.slice(home.length)
  }
  leftSpans.push(dim(cwd))
  if (input.gitBranch) {
    leftSpans.push(dim(` (${input.gitBranch})`))
  }

  // Context only; session token totals are shown in the loading spinner.
  const statParts: string[] = []
  if (input.contextWindow > 0 && input.contextTokens > 0) {
    const pct = (input.contextTokens / input.contextWindow * 100).toFixed(1)
    const ctxText = input.autoCompact
      ? `${pct}%/${formatFooterTokens(input.contextWindow)} (auto)`
      : `${pct}%/${formatFooterTokens(input.contextWindow)}`
    statParts.push(ctxText)
  }
  if (statParts.length > 0) {
    leftSpans.push(dim(' '))
    const pctNum = input.contextWindow > 0 && input.contextTokens > 0
      ? input.contextTokens / input.contextWindow * 100
      : 0
    if (pctNum > 90) {
      leftSpans.push(colored(statParts.join(' '), 'red'))
    } else if (pctNum > 70) {
      leftSpans.push(colored(statParts.join(' '), 'yellow'))
    } else {
      leftSpans.push(dim(statParts.join(' ')))
    }
  }

  // Model info follows stats on the left. Provider is omitted to keep the footer compact.
  if (input.model) {
    leftSpans.push(dim(' '))
    let modelDisplay = input.model
    if (input.thinkingLevel && input.thinkingLevel !== 'off') {
      modelDisplay += ` \u2022 ${input.thinkingLevel}`
    }
    leftSpans.push(dim(modelDisplay))
  }

  // Right side: server/update (rightmost)
  const rightSpans: StyledSpan[] = []
  if (input.serverPort != null && input.serverUptime) {
    rightSpans.push(colored(`[server :${input.serverPort} \u00b7 ${input.serverUptime}]`, 'green'))
  }
  if (input.updateHint) {
    if (rightSpans.length > 0) rightSpans.push(plain('  '))
    rightSpans.push(colored(input.updateHint, 'yellow'))
  }

  let leftText = leftSpans.map(s => s.text).join('')
  const rightText = rightSpans.map(s => s.text).join('')
  const totalWidth = leftText.length + rightText.length

  if (totalWidth >= columns) {
    // Overflow: drop right side. If left still too wide, truncate cwd.
    let leftWidth = leftText.length
    if (leftWidth >= columns) {
      const cwdIdx = leftSpans.findIndex(s => s.text === cwd)
      if (cwdIdx >= 0) {
        const otherWidth = leftWidth - cwd.length
        const maxCwd = Math.max(8, columns - otherWidth - 1)
        if (cwd.length > maxCwd) {
          const shortened = '...' + cwd.slice(cwd.length - maxCwd + 3)
          leftSpans[cwdIdx] = dim(shortened)
          leftWidth = leftSpans.map(s => s.text).join('').length
        }
      }
      // If still too wide, just let it be — terminal will clip the end
    }
    return block([line(...leftSpans)])
  }

  const gap = Math.max(1, columns - leftText.length - rightText.length)
  const spans: StyledSpan[] = [...leftSpans, plain(' '.repeat(gap)), ...rightSpans]

  return block([line(...spans)])
}

function formatFooterTokens(count: number): string {
  if (count < 1000) return count.toString()
  if (count < 10000) return `${(count / 1000).toFixed(1)}k`
  if (count < 1000000) return `${Math.round(count / 1000)}k`
  if (count < 10000000) return `${(count / 1000000).toFixed(1)}M`
  return `${Math.round(count / 1000000)}M`
}

// Wrap a logical input line into visual chunks that each fit within `width`
// columns of display width. Returns chunks expressed as character index ranges
// `[start, end)` over the original string.
//
// Wrapping is character-based (not word-based) so the cursor column maps
// cleanly to a chunk. CJK and other wide characters are accounted for via
// `string-width`, so a wide character at the boundary is pushed to the next
// chunk instead of overflowing.
export function wrapTextByWidth(text: string, width: number): { start: number; end: number }[] {
  if (width <= 0) return [{ start: 0, end: text.length }]
  if (text.length === 0) return [{ start: 0, end: 0 }]

  const chunks: { start: number; end: number }[] = []
  let start = 0
  let used = 0
  for (let i = 0; i < text.length; i++) {
    const ch = text[i]!
    const w = stringWidth(ch)
    if (used + w > width && i > start) {
      chunks.push({ start, end: i })
      start = i
      used = 0
    }
    used += w
  }
  chunks.push({ start, end: text.length })
  return chunks
}


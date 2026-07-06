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
  planning: boolean
  logMode: boolean
  queuedMessages: string[]
  /** Dashboard URL, shown as a clickable link above the footer. Null hides it. */
  dashboardUrl: string | null
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
  // Reasoning effort shown next to the model name. Empty string hides it.
  thinkingLevel: string
  // Persistent plan progress indicator (e.g. "📋 2/5"). Null hides it.
  planLabel: string | null
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

  // Guarantee one blank line between the message area and the prompt's top
  // border, independent of how the preceding block ended (history / streaming /
  // spinner). Mirrors pi's always-present widgetContainerAbove spacer, and
  // matches the marginTop:1 every other frame section already uses.
  blocks.push(block([line(dim(border))], 1))

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
  // Single line: [plan] cwd (branch) context: N% (used/window) model
  const leftSpans: StyledSpan[] = []

  if (input.logMode) {
    leftSpans.push(dim('[log] '))
  }
  if (input.planning) {
    leftSpans.push(dim('[plan] '))
  }
  if (input.planLabel) {
    leftSpans.push(dim(`${input.planLabel}  `))
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

  // Context usage + current model
  if (input.contextWindow > 0 && input.contextTokens > 0) {
    const pctNum = input.contextTokens / input.contextWindow * 100
    const ctxText = `context: ${pctNum.toFixed(1)}% (${formatContextTokens(input.contextTokens)}/${formatContextTokens(input.contextWindow)})`
    leftSpans.push(dim(' '))
    if (pctNum > 90) {
      leftSpans.push(colored(ctxText, 'red'))
    } else if (pctNum > 70) {
      leftSpans.push(colored(ctxText, 'yellow'))
    } else {
      leftSpans.push(dim(ctxText))
    }
  }
  if (input.model) {
    leftSpans.push(dim(` ${input.model}`))
    // Reasoning effort indicator, aligned with pi's footer (model • level).
    if (input.thinkingLevel) {
      const label = input.thinkingLevel === 'off' ? 'thinking off' : input.thinkingLevel
      leftSpans.push(dim(` • ${label}`))
    }
  }

  // Right side: clickable dashboard link in a faint green. The URL text is the
  // visible label (used for width math); the OSC 8 link is attached separately
  // so escapes don't inflate the layout.
  const rightSpans: StyledSpan[] = []
  if (input.dashboardUrl) {
    rightSpans.push({ text: 'dashboard ', hex: '#7fae7f' })
    rightSpans.push({ text: input.dashboardUrl, hex: '#7fae7f', link: input.dashboardUrl })
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

function formatContextTokens(count: number): string {
  if (count < 1000) return count.toString()
  if (count < 1000000) return `${(count / 1000).toFixed(1)}k`
  return `${(count / 1000000).toFixed(1)}M`
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


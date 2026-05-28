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
  for (let i = 0; i < input.lines.length; i++) {
    const text = input.lines[i]!
    const prefix: StyledSpan = i === 0
      ? colored('❯ ', 'cyan', { bold: true })
      : plain('  ')

    if (i === input.cursorLine && input.active) {
      if (text === '' && input.lines.length === 1 && input.placeholder) {
        inputLines.push(line(prefix, plain(CURSOR_MARKER), inverse(' '), dim(' Type a message...')))
      } else {
        const before = text.slice(0, input.cursorCol)
        const cursorChar = text[input.cursorCol] ?? ' '
        const after = text.slice(input.cursorCol + 1)
        const spans: StyledSpan[] = [prefix, ...styleInputText(before), plain(CURSOR_MARKER), inverse(cursorChar), ...styleInputText(after)]
        if (input.ghostHint) spans.push(dim(input.ghostHint))
        inputLines.push(line(...spans))
      }
    } else {
      inputLines.push(line(prefix, ...(text ? styleInputText(text) : [plain(' ')])))
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

  // Token stats + context
  const statParts: string[] = []
  if (input.inputTokens > 0) statParts.push(`\u2191${formatFooterTokens(input.inputTokens)}`)
  if (input.outputTokens > 0) statParts.push(`\u2193${formatFooterTokens(input.outputTokens)}`)
  if (input.cacheReadTokens > 0) statParts.push(`R${formatFooterTokens(input.cacheReadTokens)}`)
  if (input.cost > 0) statParts.push(`$${input.cost.toFixed(3)}`)
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

  // Model info follows stats on the left
  if (input.provider || input.model) {
    leftSpans.push(dim(' '))
    let modelDisplay = input.model
    if (input.provider) {
      modelDisplay = `(${input.provider}) ${input.model}`
    }
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

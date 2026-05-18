import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
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

  blocks.push(block([line(dim(border))], input.isLoading ? 0 : 1))

  const inputLines: StyledLine[] = []
  for (let i = 0; i < input.lines.length; i++) {
    const text = input.lines[i]!
    const prefix: StyledSpan = i === 0
      ? colored('❯ ', 'cyan', { bold: true })
      : plain('  ')

    if (i === input.cursorLine && input.active) {
      if (text === '' && input.lines.length === 1 && input.placeholder) {
        inputLines.push(line(prefix, inverse(' '), dim(' Type a message...')))
      } else {
        const before = text.slice(0, input.cursorCol)
        const cursorChar = text[input.cursorCol] ?? ' '
        const after = text.slice(input.cursorCol + 1)
        const spans: StyledSpan[] = [prefix, ...styleInputText(before), inverse(cursorChar), ...styleInputText(after)]
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

  return blocks
}

function buildFooter(input: PromptVMInput, columns: number): ViewBlock {
  const leftSpans: StyledSpan[] = []

  // model
  leftSpans.push(dim(input.model))

  // [log] / [plan] modes
  if (input.logMode) {
    leftSpans.push(plain('  '))
    leftSpans.push(colored('[log]', 'magenta', { bold: true }))
    leftSpans.push(dim(' Esc to exit'))
  }
  if (input.planning) {
    leftSpans.push(plain('  '))
    leftSpans.push(colored('[plan]', 'yellow', { bold: true }))
  }
  if (input.verbose) {
    leftSpans.push(plain('  '))
    leftSpans.push(colored('[verbose]', 'cyan', { bold: true }))
    leftSpans.push(dim(' /v to toggle'))
  }

  // git repo · branch
  if (input.gitRepo) {
    leftSpans.push(dim(' · '))
    let cwd = input.cwd
    const home = process.env.HOME || process.env.USERPROFILE || ''
    if (home && cwd.startsWith(home)) {
      cwd = '~' + cwd.slice(home.length)
    }
    leftSpans.push(dim(cwd))
    if (input.gitBranch) {
      leftSpans.push(dim(' (' + input.gitBranch + ')'))
    }
  }

  const rightSpans: StyledSpan[] = []
  if (input.serverPort != null && input.serverUptime) {
    rightSpans.push(colored(`[server :${input.serverPort} · ${input.serverUptime}]`, 'green'))
  }
  if (input.updateHint) {
    if (rightSpans.length > 0) rightSpans.push(plain('  '))
    rightSpans.push(colored(input.updateHint, 'yellow'))
  }

  const leftText = leftSpans.map(s => s.text).join('')
  const rightText = rightSpans.map(s => s.text).join('')
  const gap = Math.max(1, columns - leftText.length - rightText.length)

  const spans: StyledSpan[] = [...leftSpans, plain(' '.repeat(gap)), ...rightSpans]
  return block([line(...spans)])
}

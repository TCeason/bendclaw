import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, colored, ansi, type ViewBlock, type StyledLine } from './types.js'
import { renderMarkdownCached } from '../../render/markdown.js'
import stripAnsi from 'strip-ansi'

const TABLE_TAIL_RE = /^[│|├┌└]/
const FENCE_TAIL_RE = /(^|\n)[ \t]*(```+|~~~+)/

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120

let lastPendingKey = ''
let lastPlainTail = ''

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  pendingThinkingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
  expanded?: boolean
  assistantCommitted?: boolean
  /** Current character-index position into the *rendered* markdown tail.
   *  Incremented by the reducer each paced frame; capped in this function
   *  at the actual rendered length so tests can pass a large value for
   *  "fully revealed". */
  revealCursor: number
}

export function buildActiveResponseBlocks(input: ActiveResponseInput): ViewBlock[] {
  if (!input.isLoading) return []

  const blocks: ViewBlock[] = []

  if (input.pendingThinkingText) {
    const lines = input.pendingThinkingText.split('\n')
    const maxLines = Math.max(1, input.termRows - 10)
    const visible = lines.slice(-maxLines)
    const styledLines: StyledLine[] = visible.map((l, i) =>
      i === 0
        ? line(colored('  🤔 ', 'cyan'), dim(l))
        : line(dim(`     ${l}`))
    )
    if (styledLines.length > 0) {
      blocks.push(block(styledLines, 1))
    }
  }

  if (input.toolProgress && !input.expanded) {
    const progLines = input.toolProgress.split('\n')
    const extraLines = Math.max(0, progLines.length - MAX_PROGRESS_LINES)
    const tail = progLines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l)
    const styledLines: StyledLine[] = tail.map(l => line(dim(`  ${l}`)))
    if (extraLines > 0) {
      styledLines.push(line(dim(`  +${extraLines} lines  (ctrl+o to expand)`)))
    }
    blocks.push(block(styledLines, 1))
  } else if (input.spinner.phase === 'executing' && !input.expanded) {
    blocks.push(block([line(dim('  Waiting for output…'))], 1))
  }

  if (input.expanded) {
    blocks.push(block([line(dim('  ctrl+o to collapse'))], 1))
  }

  // Assistant text streaming: render the FULL pending text through the
  // real markdown pipeline, then reveal its last line character by
  // character using the reducer-supplied revealCursor. The cursor only
  // advances (the reducer guarantees monotonic increase), so every frame
  // is a strict prefix extension of the previous one — the renderer's
  // setStatus always hits its append fast-path, producing a true typing
  // effect without flicker.
  if (input.pendingText) {
    const renderedFull = renderMarkdownCached(input.pendingText)
    const renderedLines = renderedFull.split('\n')
    // Find the last non-empty line
    let lastIdx = renderedLines.length - 1
    while (lastIdx >= 0 && !renderedLines[lastIdx]!.trim()) lastIdx--
    const renderedTail = lastIdx >= 0 ? renderedLines[lastIdx]! : ''

    const cursor = Math.min(input.revealCursor, renderedTail.length)
    // Structural output whose shape depends on unseen future lines (tables,
    // box drawings, tree diagrams) can't be shown character-by-character
    // without column-width jitter. Show the spinner until the completed
    // block is committed by the stream machine.
    const plainTail = stripAnsi(renderedTail)
    const pendingKey = input.pendingText
    const isPrefixExtension = pendingKey.startsWith(lastPendingKey) && plainTail.startsWith(lastPlainTail)
    const isStructuralTail = TABLE_TAIL_RE.test(plainTail) || FENCE_TAIL_RE.test(input.pendingText)
    if (cursor <= 0 || !renderedTail || isStructuralTail || !isPrefixExtension) {
      lastPendingKey = pendingKey
      lastPlainTail = plainTail
      // Still in pure-buffer phase, structural markdown, or a render shape
      // change that would require rewriting the line. Keep the prompt stable
      // and wait for the completed block to append to the scroll area.
      const spinnerText = formatSpinnerLine(input.spinner, Date.now())
      blocks.push(block([line(plain(spinnerText))], 1))
      return blocks
    }

    lastPendingKey = pendingKey
    lastPlainTail = plainTail
    const revealed = renderedTail.slice(0, cursor)
    const isBlockStart = !input.assistantCommitted
    const styledLines: StyledLine[] = [
      isBlockStart
        ? line(colored('⏺ ', 'cyan'), ansi(revealed))
        : line(ansi(`  ${revealed}`)),
    ]
    blocks.push(block(styledLines, isBlockStart ? 1 : 0))
    return blocks
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    1,
  ))

  return blocks
}

import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, colored, ansi, type ViewBlock, type StyledLine } from './types.js'
import { renderMarkdownCached } from '../../render/markdown.js'
import stripAnsi from 'strip-ansi'
import ansiRegex from 'ansi-regex'
import stringWidth from 'string-width'

const TABLE_TAIL_RE = /^[│|├┌└]/
const FENCE_TAIL_RE = /(^|\n)[ \t]*(```+|~~~+)/
const PIPE_TABLE_RE = /(?:^|\n)\s*\|.*\|[ \t]*\n\s*\|.*\|/
const STRUCTURAL_PENDING_RE = /(^|\n)[ \t]*(?:#{1,6}\s|[-*+]\s+|\d+\.\s+|>\s?|\|.*\||[│├└┌]|(?: {4}|\t)\S)/

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120
const RESERVED_PENDING_LINE = ' '

let lastPendingKey = ''
let lastPlainTail = ''

export function renderedPendingTailWidth(pendingText: string): number {
  if (!pendingText) return 0
  const renderedFull = renderMarkdownCached(pendingText)
  const renderedLines = renderedFull.split('\n')
  let lastIdx = renderedLines.length - 1
  while (lastIdx >= 0 && !renderedLines[lastIdx]!.trim()) lastIdx--
  if (lastIdx < 0) return 0
  const renderedTail = renderedLines[lastIdx]!
  const plainTail = stripAnsi(renderedTail)
  if (TABLE_TAIL_RE.test(plainTail) || FENCE_TAIL_RE.test(pendingText) || STRUCTURAL_PENDING_RE.test(pendingText) || pendingText.includes('\n')) return 0
  return stringWidth(plainTail)
}

function sliceAnsiByWidth(input: string, maxWidth: number): string {
  if (maxWidth <= 0) return ''
  let width = 0
  let out = ''
  for (let i = 0; i < input.length;) {
    if (input[i] === '\x1b') {
      const match = ansiRegex().exec(input.slice(i))
      if (match?.index === 0) {
        out += match[0]
        i += match[0].length
        continue
      }
    }
    const code = input.codePointAt(i)
    if (code === undefined) break
    const ch = String.fromCodePoint(code)
    const w = stringWidth(ch)
    if (width + w > maxWidth) break
    out += ch
    width += w
    i += ch.length
  }
  return out
}

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  pendingThinkingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
  expanded?: boolean
  assistantCommitted?: boolean
  /** Current display-cell position into the *rendered* markdown tail.
   *  Incremented by the reducer/reveal timer; capped in this function at
   *  the actual rendered width so tests can pass a large value for
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
      blocks.push(block(styledLines, 0))
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
  //
  // For multi-line pending text (lists, headings, blockquotes), show all
  // rendered lines in the status area so content appears to grow smoothly
  // rather than accumulating behind a spinner and bursting out at commit.
  // Tables and box drawings still fall back to spinner because their
  // column widths depend on unseen future lines.
  if (input.pendingText) {
    const renderedFull = renderMarkdownCached(input.pendingText)
    const renderedLines = renderedFull.split('\n')
    // Find the last non-empty line
    let lastIdx = renderedLines.length - 1
    while (lastIdx >= 0 && !renderedLines[lastIdx]!.trim()) lastIdx--
    const renderedTail = lastIdx >= 0 ? renderedLines[lastIdx]! : ''

    // Tables, box drawings, and open code fences depend on unseen future
    // lines — their shape can change as more content arrives. Fall back to
    // spinner for those. Other structural content (lists, headings,
    // blockquotes) renders stably line-by-line.
    const plainTail = stripAnsi(renderedTail)
    const cursor = Math.min(input.revealCursor, stringWidth(plainTail))
    const pendingKey = input.pendingText
    const isSamePendingRun = pendingKey.startsWith(lastPendingKey)
    const isPrefixExtension = !isSamePendingRun || plainTail.startsWith(lastPlainTail)
    const isUnsafeStructural = TABLE_TAIL_RE.test(plainTail) || FENCE_TAIL_RE.test(input.pendingText) || PIPE_TABLE_RE.test(input.pendingText)
    const isMultiLine = input.pendingText.includes('\n')

    if (isUnsafeStructural || (cursor <= 0 && !isMultiLine) || (!renderedTail && !isMultiLine) || (!isMultiLine && !isPrefixExtension)) {
      lastPendingKey = pendingKey
      lastPlainTail = plainTail
      // Still in pure-buffer phase or unsafe structural content.
      // Show spinner and wait for the completed block to append.
      const spinnerText = formatSpinnerLine(input.spinner, Date.now())
      blocks.push(block([line(plain(RESERVED_PENDING_LINE))], 1))
      blocks.push(block([line(plain(spinnerText))], 1))
      return blocks
    }

    lastPendingKey = pendingKey
    lastPlainTail = plainTail

    if (isMultiLine) {
      // Multi-line pending: show all rendered lines in the status area.
      // This produces a smooth growth effect — new lines appear at the
      // bottom as they stream in, matching Claude Code's behavior.
      const isBlockStart = !input.assistantCommitted
      const styledLines: StyledLine[] = []
      for (let i = 0; i <= lastIdx; i++) {
        const rl = renderedLines[i]!
        if (i === 0 && isBlockStart) {
          styledLines.push(line(colored('⏺ ', 'cyan'), ansi(rl)))
        } else {
          styledLines.push(line(ansi(`  ${rl}`)))
        }
      }
      blocks.push(block(styledLines, 1))
      const spinnerText = formatSpinnerLine(input.spinner, Date.now())
      blocks.push(block([line(plain(spinnerText))], 1))
      return blocks
    }

    // Single-line pending: typewriter reveal on the last line.
    const revealed = sliceAnsiByWidth(renderedTail, cursor)
    const isBlockStart = !input.assistantCommitted
    const styledLines: StyledLine[] = [
      isBlockStart
        ? line(colored('⏺ ', 'cyan'), ansi(revealed))
        : line(ansi(`  ${revealed}`)),
    ]
    blocks.push(block(styledLines, 1))
    const spinnerText = formatSpinnerLine(input.spinner, Date.now())
    blocks.push(block([line(plain(spinnerText))], 1))
    return blocks
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    1,
  ))

  return blocks
}

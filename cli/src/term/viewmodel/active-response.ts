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
const OSC8_CLOSE = '\x1b]8;;\x1b\\'
const graphemeSegmenter = typeof Intl !== 'undefined' && 'Segmenter' in Intl
  ? new Intl.Segmenter(undefined, { granularity: 'grapheme' })
  : null

let lastPendingKey = ''
let lastPlainTail = ''
let lastAnalysisKey = ''
let lastAnalysis: PendingAnalysis | null = null

interface PendingAnalysis {
  renderedLines: string[]
  lastIdx: number
  renderedTail: string
  plainTail: string
  tailWidth: number
  isUnsafeForReveal: boolean
  revealWidth: number
}

function pendingAnalysisKey(pendingText: string): string {
  const columns = process.stdout.columns
  const safeColumns = Number.isFinite(columns) && columns > 0 ? Math.floor(columns) : 80
  return `${safeColumns}\0${pendingText}`
}

function analyzePendingText(pendingText: string): PendingAnalysis {
  const key = pendingAnalysisKey(pendingText)
  if (lastAnalysis && lastAnalysisKey === key) return lastAnalysis

  const renderedFull = renderMarkdownCached(pendingText)
  const renderedLines = renderedFull.split('\n')
  let lastIdx = renderedLines.length - 1
  while (lastIdx >= 0 && !renderedLines[lastIdx]!.trim()) lastIdx--
  const renderedTail = lastIdx >= 0 ? renderedLines[lastIdx]! : ''
  const plainTail = stripAnsi(renderedTail)
  const tailWidth = stringWidth(plainTail)
  const isUnsafeForReveal = TABLE_TAIL_RE.test(plainTail)
    || FENCE_TAIL_RE.test(pendingText)
    || STRUCTURAL_PENDING_RE.test(pendingText)
    || pendingText.includes('\n')
  lastAnalysisKey = key
  lastAnalysis = {
    renderedLines,
    lastIdx,
    renderedTail,
    plainTail,
    tailWidth,
    isUnsafeForReveal,
    revealWidth: isUnsafeForReveal ? 0 : tailWidth,
  }
  return lastAnalysis
}

export function renderedPendingTailWidth(pendingText: string): number {
  if (!pendingText) return 0
  return analyzePendingText(pendingText).revealWidth
}

function sliceAnsiByWidth(input: string, maxWidth: number): string {
  if (maxWidth <= 0) return ''
  let width = 0
  let out = ''
  let openHyperlink = false
  const segmenter = graphemeSegmenter
  for (let i = 0; i < input.length;) {
    if (input[i] === '\x1b') {
      const match = ansiRegex().exec(input.slice(i))
      if (match?.index === 0) {
        const seq = match[0]
        out += seq
        if (seq.startsWith('\x1b]8;;')) openHyperlink = seq !== OSC8_CLOSE
        i += seq.length
        continue
      }
    }
    const plainStart = i
    while (i < input.length && input[i] !== '\x1b') i++
    const plainChunk = input.slice(plainStart, i)
    const segments = segmenter
      ? Array.from(segmenter.segment(plainChunk), part => part.segment)
      : Array.from(plainChunk)
    for (const ch of segments) {
      const w = stringWidth(ch)
      if (width + w > maxWidth) {
        if (openHyperlink) out += OSC8_CLOSE
        return out
      }
      out += ch
      width += w
    }
  }
  if (openHyperlink) out += OSC8_CLOSE
  return out
}

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  pendingThinkingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
  termColumns?: number
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
    const totalLines = lines.length
    const MAX_THINKING_PREVIEW = 4
    const visible = lines.slice(-MAX_THINKING_PREVIEW)
    const extraLines = Math.max(0, totalLines - MAX_THINKING_PREVIEW)
    const styledLines: StyledLine[] = [
      line(colored('[REASONING]', 'cyan', { bold: true }), dim(` ${totalLines} lines...`)),
    ]
    for (const l of visible) {
      const truncated = l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l
      styledLines.push(line(dim(`  ${truncated}`)))
    }
    if (extraLines > 0) {
      styledLines.push(line(dim(`  +${extraLines} lines  (ctrl+o to expand)`)))
    }
    blocks.push(block(styledLines, 1))
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
    const analysis = analyzePendingText(input.pendingText)
    const { renderedLines, lastIdx, renderedTail, plainTail, tailWidth } = analysis

    // Tables, box drawings, and open code fences depend on unseen future
    // lines — their shape can change as more content arrives. Fall back to
    // spinner for those. Other structural content (lists, headings,
    // blockquotes) renders stably line-by-line.
    const cursor = Math.min(input.revealCursor, tailWidth)
    const pendingKey = input.pendingText
    const isSamePendingRun = pendingKey.startsWith(lastPendingKey)
    const isPrefixExtension = !isSamePendingRun || plainTail.startsWith(lastPlainTail)
    const isUnsafeStructural = TABLE_TAIL_RE.test(plainTail) || FENCE_TAIL_RE.test(input.pendingText) || PIPE_TABLE_RE.test(input.pendingText)
    const textColumns = input.assistantCommitted ? 2 : 2
    const visibleColumns = Math.max(20, (input.termColumns ?? 80) - textColumns)
    const fullPendingWidth = input.pendingText.includes('\n') ? 0 : stringWidth(stripAnsi(renderMarkdownCached(input.pendingText)))
    const wrapsInStatus = fullPendingWidth > visibleColumns
    const isMultiLine = input.pendingText.includes('\n') || wrapsInStatus

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
      const maxPendingLines = Math.max(3, Math.floor(input.termRows / 2) - 2)
      const firstVisible = Math.max(0, lastIdx - maxPendingLines + 1)
      if (firstVisible > 0) {
        styledLines.push(line(dim(`  ... (+${firstVisible} lines)`)))
      }
      for (let i = firstVisible; i <= lastIdx; i++) {
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
  blocks.push(block([line(plain(RESERVED_PENDING_LINE))], 1))
  blocks.push(block(
    [line(plain(spinnerText))],
    1,
  ))

  return blocks
}

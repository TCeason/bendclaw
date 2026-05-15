/**
 * TermRenderer — manages terminal output with two zones:
 *
 * 1. Scroll zone: content written here scrolls naturally (completed output)
 * 2. Status area: fixed N lines at the bottom, updated in-place via cursor control
 *
 * The status area uses line-level diffing — only changed lines are redrawn.
 * This eliminates the flicker caused by Ink's clear+redraw model.
 */

import {
  cursorUp,
  cursorDown,
  eraseDown,
  eraseLine,
  eraseToEndOfLine,
  hideCursor,
  showCursor,
  cursorToColumn,
  cursorTo,
} from './ansi.js'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'

function safeDimension(n: number | undefined, fallback: number): number {
  return n != null && Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback
}

function safeCount(n: number): number {
  return Number.isFinite(n) ? Math.max(0, Math.floor(n)) : 0
}

export interface TermRendererOptions {
  /** Stream to write to (default: process.stdout) */
  stdout?: NodeJS.WriteStream
}

export class TermRenderer {
  private stdout: NodeJS.WriteStream
  private prevStatusLines: string[] = []
  private statusHeight = 0
  private rows: number
  private cols: number
  private destroyed = false
  private resizeHandler: (() => void) | null = null
  private buf = ''
  private buffering = false

  constructor(opts?: TermRendererOptions) {
    this.stdout = opts?.stdout ?? process.stdout
    this.rows = safeDimension(this.stdout.rows, 24)
    this.cols = safeDimension(this.stdout.columns, 80)
  }

  /** Initialize renderer: hide cursor, listen for resize. */
  init(): void {
    this.write(hideCursor())
    this.resizeHandler = () => {
      this.rows = safeDimension(this.stdout.rows, 24)
      this.cols = safeDimension(this.stdout.columns, 80)
      this.redrawStatus()
    }
    this.stdout.on('resize', this.resizeHandler)
  }

  /** Restore terminal state. */
  destroy(): void {
    if (this.destroyed) return
    if (this.resizeHandler) {
      this.stdout.off('resize', this.resizeHandler)
      this.resizeHandler = null
    }
    // Clear status area and show cursor
    this.clearStatusArea()
    this.write(showCursor())
    this.destroyed = true
  }

  /** Get terminal dimensions. */
  get termRows(): number { return this.rows }
  get termCols(): number { return this.cols }

  /**
   * Append content to the scroll zone.
   * Moves status area down to make room, then writes content.
   */
  appendScroll(text: string): void {
    if (!text) return
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    // Clear status area first
    this.clearStatusArea()
    // Write content (it scrolls naturally)
    this.write(text)
    // Ensure every logical output line is committed to the scrollback.  When
    // text already ends with "\n", the cursor is sitting on that final empty
    // line; moving one more row makes trailing separator lines durable instead
    // of letting the next status redraw overwrite them.
    this.write('\n')
    // Do NOT redraw status here — caller is responsible for calling
    // setStatus() after appendScroll to avoid stale content being redrawn.
    if (!outerBatch) this.flushBatch()
  }

  /**
   * Update the status area (fixed bottom lines).
   * Redraws only the changed suffix so stable pending markdown does not flash
   * when the spinner or prompt line changes.
   */
  setStatus(lines: string[]): void {
    const prev = this.prevStatusLines
    const next = lines

    let firstChanged = 0
    const shared = Math.min(prev.length, next.length)
    while (firstChanged < shared && prev[firstChanged] === next[firstChanged]) {
      firstChanged++
    }

    if (firstChanged === prev.length && firstChanged === next.length) return

    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()

    if (this.statusHeight === 0 || prev.length === 0) {
      this.prevStatusLines = [...next]
      this.statusHeight = next.length
      this.drawStatus()
      if (!outerBatch) this.flushBatch()
      return
    }

    if (firstChanged < prev.length && firstChanged < next.length) {
      const prevLine = prev[firstChanged] ?? ''
      const nextLine = next[firstChanged] ?? ''
      const endCol = this.appendColumn(prevLine, nextLine)
      const unchangedSuffix = prev.length === next.length
        && prev.slice(firstChanged + 1).every((line, idx) => line === next[firstChanged + 1 + idx])
      if (unchangedSuffix && endCol !== null && nextLine.startsWith(prevLine) && nextLine.length > prevLine.length) {
        const rowsAfter = this.screenRows(prev.slice(firstChanged + 1))
        this.write(cursorUp(rowsAfter + 1) + cursorToColumn(endCol) + eraseToEndOfLine())
        this.write(nextLine.slice(prevLine.length) + cursorToColumn(1) + cursorDown(rowsAfter + 1))
        this.prevStatusLines = [...next]
        this.statusHeight = next.length
        if (!outerBatch) this.flushBatch()
        return
      }
    }

    // Single-line change with all following lines identical: only redraw that
    // one physical line instead of eraseDown(), which would clear through the
    // prompt area and cause visible jumping when spinner frames update.
    if (firstChanged < prev.length
        && firstChanged < next.length
        && prev.length === next.length
        && prev.length > 1
        && this.screenRows([prev[firstChanged] ?? '']) === 1
        && this.screenRows([next[firstChanged] ?? '']) === 1
        && prev.slice(firstChanged + 1).every((line, idx) => line === next[firstChanged + 1 + idx])) {
      const rowsAfter = this.screenRows(prev.slice(firstChanged + 1))
      this.write(cursorUp(rowsAfter + 1) + eraseLine())
      this.write(this.truncateLine(next[firstChanged]!))
      // +1 accounts for the newline after the changed line from the original
      // drawStatus() call, keeping the cursor below the status area.
      this.write(cursorDown(rowsAfter + 1))
      this.write(cursorToColumn(1))
      this.prevStatusLines = [...next]
      this.statusHeight = next.length
      if (!outerBatch) this.flushBatch()
      return
    }

    const rowsToReplace = this.screenRows(prev.slice(firstChanged))
    this.write(cursorUp(rowsToReplace) + cursorToColumn(1) + eraseDown())

    this.prevStatusLines = [...next]
    this.statusHeight = next.length
    for (const line of next.slice(firstChanged)) {
      this.write(this.truncateLine(line) + '\n')
    }

    if (!outerBatch) this.flushBatch()
  }

  /** Begin a batch — all writes are buffered until flushBatch(). */
  beginBatch(): void {
    this.buffering = true
    this.buf = ''
  }

  /** Flush buffered writes as a single stdout.write(). */
  flushBatch(): void {
    this.buffering = false
    if (this.buf) {
      this.stdout.write(this.buf)
      this.buf = ''
    }
  }

  /** Calculate actual screen rows a set of lines occupies (accounting for wrapping). */
  private screenRows(lines: string[]): number {
    let total = 0
    const cols = this.cols || 80
    for (const line of lines) {
      const width = stringWidth(stripAnsi(line))
      total += width === 0 ? 1 : Math.ceil(width / cols)
    }
    return total
  }

  private appendColumn(prevLine: string, nextLine: string): number | null {
    const prevWidth = stringWidth(stripAnsi(prevLine))
    const nextWidth = stringWidth(stripAnsi(nextLine))
    if (prevWidth >= this.cols || nextWidth >= this.cols) return null
    return prevWidth + 1
  }

  /** Clear the status area (move up and erase). */
  private clearStatusArea(): void {
    if (this.statusHeight <= 0) return
    const rows = this.screenRows(this.prevStatusLines)
    this.write(cursorUp(rows) + cursorToColumn(1) + eraseDown())
    this.statusHeight = 0
    this.prevStatusLines = []
  }

  /** Draw status area from scratch. */
  private drawStatus(): void {
    if (this.prevStatusLines.length === 0) return
    for (const line of this.prevStatusLines) {
      this.write(this.truncateLine(line) + '\n')
    }
  }

  /** Redraw status area (after resize). */
  private redrawStatus(): void {
    if (this.prevStatusLines.length === 0) return
    const lines = [...this.prevStatusLines]
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    this.clearStatusArea()
    this.prevStatusLines = lines
    this.statusHeight = lines.length
    this.drawStatus()
    if (!outerBatch) this.flushBatch()
  }

  /**
   * Redraw the current viewport in place, removing previous scroll content from the viewport.
   * Keeps the normal screen buffer so terminal scrollback remains available.
   */
  redrawViewport(text: string): void {
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    // Reset status bookkeeping directly — cursorTo(1,1)+eraseDown handles
    // clearing the visible viewport regardless of scroll position.
    // clearStatusArea() would use relative cursorUp() which breaks when
    // the user has scrolled up and auto-scroll is off.
    this.statusHeight = 0
    this.prevStatusLines = []
    this.write(cursorTo(1, 1) + eraseDown() + '\x1b[0m')
    const lines = text ? text.split('\n') : []
    if (text) {
      this.write(text)
      if (!text.endsWith('\n')) this.write('\n')
    }
    const usedRows = text ? this.screenRows(lines) : 0
    const remainingRows = safeCount(this.rows - usedRows)
    if (remainingRows > 0) this.write('\n'.repeat(remainingRows))
    if (!outerBatch) this.flushBatch()
  }

  /**
   * Redraw the current viewport tightly from the top, without padding to the bottom.
   * Used when idle so the prompt can sit directly after the latest output.
   */
  redrawViewportTight(text: string): void {
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    // Reset state directly — cursorTo(1,1)+eraseDown handles clearing.
    this.statusHeight = 0
    this.prevStatusLines = []
    this.write(cursorTo(1, 1) + eraseDown() + '\x1b[0m')
    if (text) {
      this.write(text)
      if (!text.endsWith('\n')) this.write('\n')
    }
    if (!outerBatch) this.flushBatch()
  }

  /** Clear the current viewport redraw without leaving normal scrollback. */
  restoreViewport(): void {
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    this.statusHeight = 0
    this.prevStatusLines = []
    this.write(cursorTo(1, 1) + eraseDown() + '\x1b[0m')
    this.write('\n'.repeat(safeCount(this.rows)))
    if (!outerBatch) this.flushBatch()
  }

  /**
   * Clear the entire screen and reset status state.
   * Used for mode switches (e.g. verbose toggle) where all content
   * is re-rendered from scratch.
   */
  clearScreen(): void {
    const outerBatch = this.buffering
    if (!outerBatch) this.beginBatch()
    this.statusHeight = 0
    this.prevStatusLines = []
    // Push old content into scrollback with blank lines instead of erasing
    // the viewport. This feels more natural — the user can still scroll up
    // to see previous output, and the prompt lands at the bottom.
    this.write('\n'.repeat(safeCount(this.rows)))
    if (!outerBatch) this.flushBatch()
  }

  /** Truncate a line to terminal width to prevent wrapping artifacts. */
  private truncateLine(line: string): string {
    // Fast path: if visible width fits, return as-is
    if (stringWidth(line) <= this.cols) return line
    // Slow path: truncate visible content
    // For simplicity, just return the line — terminal will wrap
    // A proper implementation would do ANSI-aware truncation
    return line
  }

  private write(data: string): void {
    if (this.destroyed) return
    if (this.buffering) {
      this.buf += data
    } else {
      this.stdout.write(data)
    }
  }
}

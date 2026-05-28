/**
 * TermRenderer — differential rendering engine for terminal output.
 *
 * Renders a full frame each cycle, diffs against the previous frame, and only
 * redraws changed lines. Uses synchronized output (DEC mode 2026) to eliminate
 * flicker.
 *
 * Two zones:
 *   - Frozen scrollback: lines that have been "frozen" scroll naturally into
 *     the terminal's scrollback buffer and are never touched again.
 *   - Viewport: the active rendering area, redrawn each frame via diffing.
 *
 * Inspired by github.com/anthropics/claude-code (pi) TUI architecture.
 */

import { performance } from 'node:perf_hooks'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'

// --- Constants ---

const MIN_RENDER_INTERVAL_MS = 16
const SYNC_START = '\x1b[?2026h'
const SYNC_END = '\x1b[?2026l'
const CLEAR_LINE = '\x1b[2K'
const CLEAR_SCREEN = '\x1b[2J\x1b[H\x1b[3J'
const HIDE_CURSOR = '\x1b[?25l'
const SHOW_CURSOR = '\x1b[?25h'
const NOWRAP = '\x1b[?7l'   // Disable auto-wrap (DECAWM off)
const WRAP = '\x1b[?7h'     // Re-enable auto-wrap

// --- Types ---

export interface RenderFrame {
  lines: string[]
}

/** Zero-width marker embedded in rendered output to indicate cursor position for IME. */
export const CURSOR_MARKER = '\x1b]evot:c\x07'

export interface TermRendererOptions {
  stdout?: NodeJS.WriteStream
}

// --- Renderer ---

export class TermRenderer {
  private stdout: NodeJS.WriteStream
  private previousLines: string[] = []
  private previousWidth = 0
  private previousHeight = 0
  private hardwareCursorRow = 0
  private maxLinesRendered = 0
  private previousViewportTop = 0

  // Render scheduling
  private renderCallback: (() => RenderFrame | string[]) | null = null
  private renderRequested = false
  private renderTimer: ReturnType<typeof setTimeout> | undefined
  private lastRenderAt = 0
  private destroyed = false

  // --- Constructor ---

  constructor(opts?: TermRendererOptions) {
    this.stdout = opts?.stdout ?? process.stdout
  }

  // --- Accessors ---

  get termRows(): number {
    return safeDimension(this.stdout.rows, 24)
  }

  get termCols(): number {
    return safeDimension(this.stdout.columns, 80)
  }

  // --- Lifecycle ---

  init(): void {
    this.destroyed = false
    this.write(NOWRAP + HIDE_CURSOR)
    this.stdout.on('resize', this.onResize)
  }

  destroy(): void {
    if (this.renderTimer) {
      clearTimeout(this.renderTimer)
      this.renderTimer = undefined
    }
    // Move cursor below rendered content so shell prompt appears cleanly
    if (this.previousLines.length > 0) {
      const targetRow = this.previousLines.length
      const diff = targetRow - this.hardwareCursorRow
      if (diff > 0) this.write(`\x1b[${diff}B`)
      this.write('\r\n')
    }
    this.write(WRAP + SHOW_CURSOR)
    this.destroyed = true
    this.stdout.off('resize', this.onResize)
  }

  // --- Public API ---

  setRenderCallback(cb: () => RenderFrame | string[]): void {
    this.renderCallback = cb
  }

  requestRender(force = false): void {
    if (this.destroyed) return
    if (force) {
      this.previousLines = []
      this.previousWidth = -1
      this.previousHeight = -1
      this.hardwareCursorRow = 0
      this.maxLinesRendered = 0
      this.previousViewportTop = 0
      if (this.renderTimer) {
        clearTimeout(this.renderTimer)
        this.renderTimer = undefined
      }
      this.renderRequested = true
      process.nextTick(() => {
        if (this.destroyed || !this.renderRequested) return
        this.renderRequested = false
        this.lastRenderAt = performance.now()
        this.doRender()
      })
      return
    }
    if (this.renderRequested) return
    this.renderRequested = true
    process.nextTick(() => this.scheduleRender())
  }

  fullRedraw(): void {
    this.requestRender(true)
  }

  /**
   * Freeze the top `count` lines of the current viewport into scrollback.
   * After freezing, those lines are no longer part of the diffed viewport —
   * they live permanently in the terminal's scrollback buffer.
   */
  freezeLines(count: number): void {
    if (count <= 0 || count > this.previousLines.length) return
    // The frozen lines are already rendered on screen at their current position.
    // We just need to adjust our tracking so we no longer diff them.
    this.previousLines = this.previousLines.slice(count)
    // Adjust cursor tracking: the hardware cursor row is relative to the
    // full buffer, so subtract the frozen lines.
    this.hardwareCursorRow = Math.max(0, this.hardwareCursorRow - count)
    this.maxLinesRendered = Math.max(0, this.maxLinesRendered - count)
    this.previousViewportTop = Math.max(0, this.previousViewportTop - count)
  }

  /**
   * Clear the viewport and scrollback. Used for /clear command.
   */
  clearScreen(): void {
    this.write(SYNC_START + CLEAR_SCREEN + SYNC_END)
    this.previousLines = []
    this.hardwareCursorRow = 0
    this.maxLinesRendered = 0
    this.previousViewportTop = 0
    this.previousWidth = this.termCols
    this.previousHeight = this.termRows
  }

  // --- Private: scheduling ---

  private scheduleRender(): void {
    if (this.destroyed || this.renderTimer || !this.renderRequested) return
    const elapsed = performance.now() - this.lastRenderAt
    const delay = Math.max(0, MIN_RENDER_INTERVAL_MS - elapsed)
    this.renderTimer = setTimeout(() => {
      this.renderTimer = undefined
      if (this.destroyed || !this.renderRequested) return
      this.renderRequested = false
      this.lastRenderAt = performance.now()
      this.doRender()
      if (this.renderRequested) this.scheduleRender()
    }, delay)
  }

  private onResize = (): void => {
    this.requestRender(true)
  }

  // --- Private: core render ---

  private doRender(): void {
    if (this.destroyed || !this.renderCallback) return

    const width = this.termCols
    const height = this.termRows
    const widthChanged = this.previousWidth !== 0 && this.previousWidth !== width
    const heightChanged = this.previousHeight !== 0 && this.previousHeight !== height

    // Get new frame from callback
    const raw = this.renderCallback()
    const newLines = Array.isArray(raw) ? raw : raw.lines
    const cursorPos = this.extractCursorPosition(newLines, height)

    // --- Full render helper ---
    const fullRender = (clear: boolean): void => {
      let buffer = SYNC_START
      if (clear) buffer += CLEAR_SCREEN
      for (let i = 0; i < newLines.length; i++) {
        if (i > 0) buffer += '\r\n'
        buffer += newLines[i]
      }
      buffer += SYNC_END
      this.write(buffer)
      this.hardwareCursorRow = Math.max(0, newLines.length - 1)
      this.maxLinesRendered = clear ? newLines.length : Math.max(this.maxLinesRendered, newLines.length)
      const bufferLength = Math.max(height, newLines.length)
      this.previousViewportTop = Math.max(0, bufferLength - height)
      this.previousLines = newLines
      this.previousWidth = width
      this.previousHeight = height
      this.positionHardwareCursor(cursorPos, newLines.length)
    }

    // First render
    if (this.previousLines.length === 0 && !widthChanged && !heightChanged) {
      fullRender(false)
      return
    }

    // Width changed — wrapping changes, must full redraw
    if (widthChanged) {
      fullRender(true)
      return
    }

    // Height changed
    if (heightChanged) {
      fullRender(true)
      return
    }

    // --- Differential render ---
    const previousBufferLength = this.previousHeight > 0
      ? this.previousViewportTop + this.previousHeight
      : height
    let prevViewportTop = this.previousViewportTop
    let viewportTop = prevViewportTop
    let hardwareCursorRow = this.hardwareCursorRow

    const computeLineDiff = (targetRow: number): number => {
      const currentScreenRow = hardwareCursorRow - prevViewportTop
      const targetScreenRow = targetRow - viewportTop
      return targetScreenRow - currentScreenRow
    }

    // Find first and last changed lines
    let firstChanged = -1
    let lastChanged = -1
    const maxLines = Math.max(newLines.length, this.previousLines.length)
    for (let i = 0; i < maxLines; i++) {
      const oldLine = i < this.previousLines.length ? this.previousLines[i] : ''
      const newLine = i < newLines.length ? newLines[i] : ''
      if (oldLine !== newLine) {
        if (firstChanged === -1) firstChanged = i
        lastChanged = i
      }
    }

    const appendedLines = newLines.length > this.previousLines.length
    if (appendedLines) {
      if (firstChanged === -1) firstChanged = this.previousLines.length
      lastChanged = newLines.length - 1
    }
    const appendStart = appendedLines && firstChanged === this.previousLines.length && firstChanged > 0

    // No changes
    if (firstChanged === -1) {
      this.previousViewportTop = prevViewportTop
      this.previousHeight = height
      return
    }

    // All changes are in deleted lines (content shrunk)
    if (firstChanged >= newLines.length) {
      if (this.previousLines.length > newLines.length) {
        let buffer = SYNC_START
        const targetRow = Math.max(0, newLines.length - 1)
        if (targetRow < prevViewportTop) {
          fullRender(true)
          return
        }
        const lineDiff = computeLineDiff(targetRow)
        if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`
        else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`
        buffer += '\r'
        const extraLines = this.previousLines.length - newLines.length
        if (extraLines > height) {
          fullRender(true)
          return
        }
        if (extraLines > 0) buffer += '\x1b[1B'
        for (let i = 0; i < extraLines; i++) {
          buffer += `\r${CLEAR_LINE}`
          if (i < extraLines - 1) buffer += '\x1b[1B'
        }
        if (extraLines > 0) buffer += `\x1b[${extraLines}A`
        buffer += SYNC_END
        this.write(buffer)
        this.hardwareCursorRow = targetRow
      }
      this.previousLines = newLines
      this.previousWidth = width
      this.previousHeight = height
      this.previousViewportTop = prevViewportTop
      return
    }

    // First changed line is above viewport — need full redraw
    if (firstChanged < prevViewportTop) {
      fullRender(true)
      return
    }

    // --- Build differential update buffer ---
    let buffer = SYNC_START
    const prevViewportBottom = prevViewportTop + height - 1
    const moveTargetRow = appendStart ? firstChanged - 1 : firstChanged

    // If target is below visible viewport, scroll down
    if (moveTargetRow > prevViewportBottom) {
      const currentScreenRow = Math.max(0, Math.min(height - 1, hardwareCursorRow - prevViewportTop))
      const moveToBottom = height - 1 - currentScreenRow
      if (moveToBottom > 0) buffer += `\x1b[${moveToBottom}B`
      const scroll = moveTargetRow - prevViewportBottom
      buffer += '\r\n'.repeat(scroll)
      prevViewportTop += scroll
      viewportTop += scroll
      hardwareCursorRow = moveTargetRow
    }

    // Move cursor to first changed line
    const lineDiff = computeLineDiff(moveTargetRow)
    if (lineDiff > 0) buffer += `\x1b[${lineDiff}B`
    else if (lineDiff < 0) buffer += `\x1b[${-lineDiff}A`

    buffer += appendStart ? '\r\n' : '\r'

    // Render changed lines
    const renderEnd = Math.min(lastChanged, newLines.length - 1)
    for (let i = firstChanged; i <= renderEnd; i++) {
      if (i > firstChanged) buffer += '\r\n'
      buffer += CLEAR_LINE
      buffer += newLines[i]
    }

    // Track where cursor ended up
    let finalCursorRow = renderEnd

    // If content shrunk, clear extra lines
    if (this.previousLines.length > newLines.length) {
      if (renderEnd < newLines.length - 1) {
        const moveDown = newLines.length - 1 - renderEnd
        buffer += `\x1b[${moveDown}B`
        finalCursorRow = newLines.length - 1
      }
      const extraLines = this.previousLines.length - newLines.length
      for (let i = newLines.length; i < this.previousLines.length; i++) {
        buffer += `\r\n${CLEAR_LINE}`
      }
      buffer += `\x1b[${extraLines}A`
    }

    buffer += SYNC_END
    this.write(buffer)

    // Update state
    this.hardwareCursorRow = finalCursorRow
    this.maxLinesRendered = Math.max(this.maxLinesRendered, newLines.length)
    this.previousViewportTop = Math.max(prevViewportTop, finalCursorRow - height + 1)
    this.previousLines = newLines
    this.previousWidth = width
    this.previousHeight = height
    this.positionHardwareCursor(cursorPos, newLines.length)
  }

  // --- Private: cursor positioning for IME ---

  private extractCursorPosition(lines: string[], height: number): { row: number; col: number } | null {
    const viewportTop = Math.max(0, lines.length - height)
    for (let row = lines.length - 1; row >= viewportTop; row--) {
      const line = lines[row]
      const markerIndex = line.indexOf(CURSOR_MARKER)
      if (markerIndex !== -1) {
        const beforeMarker = line.slice(0, markerIndex)
        const col = stripAnsiVisibleWidth(beforeMarker)
        lines[row] = line.slice(0, markerIndex) + line.slice(markerIndex + CURSOR_MARKER.length)
        return { row, col }
      }
    }
    return null
  }

  private positionHardwareCursor(cursorPos: { row: number; col: number } | null, totalLines: number): void {
    if (!cursorPos || totalLines <= 0) {
      this.write(HIDE_CURSOR)
      return
    }
    const targetRow = Math.max(0, Math.min(cursorPos.row, totalLines - 1))
    const targetCol = Math.max(0, cursorPos.col)
    const rowDelta = targetRow - this.hardwareCursorRow
    let buffer = ''
    if (rowDelta > 0) buffer += `\x1b[${rowDelta}B`
    else if (rowDelta < 0) buffer += `\x1b[${-rowDelta}A`
    buffer += `\x1b[${targetCol + 1}G`
    buffer += SHOW_CURSOR
    this.write(buffer)
    this.hardwareCursorRow = targetRow
  }

  // --- Private: output ---

  private write(data: string): void {
    if (this.destroyed) return
    this.stdout.write(data)
  }
}

// --- Helpers ---

function safeDimension(n: number | undefined, fallback: number): number {
  return n != null && Number.isFinite(n) && n > 0 ? Math.floor(n) : fallback
}

function stripAnsiVisibleWidth(text: string): number {
  return stringWidth(stripAnsi(text))
}

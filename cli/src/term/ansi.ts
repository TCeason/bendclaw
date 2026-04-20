/**
 * ANSI escape sequence helpers for direct terminal control.
 * No dependencies — pure string builders.
 */

const ESC = '\x1b['

/** Move cursor to absolute position (1-indexed). */
export function cursorTo(row: number, col: number): string {
  return `${ESC}${row};${col}H`
}

/** Move cursor up N lines. */
export function cursorUp(n: number): string {
  if (n <= 0) return ''
  return `${ESC}${n}A`
}

/** Move cursor down N lines. */
export function cursorDown(n: number): string {
  if (n <= 0) return ''
  return `${ESC}${n}B`
}

/** Move cursor to beginning of line. */
export function cursorToColumn(col: number): string {
  return `${ESC}${col}G`
}

/** Erase entire current line. */
export function eraseLine(): string {
  return `${ESC}2K`
}

/** Erase from cursor to end of line. */
export function eraseToEndOfLine(): string {
  return `${ESC}0K`
}

/** Erase from cursor to end of screen. */
export function eraseDown(): string {
  return `${ESC}J`
}

/** Save cursor position. */
export function saveCursor(): string {
  return `${ESC}s`
}

/** Restore cursor position. */
export function restoreCursor(): string {
  return `${ESC}u`
}

/** Hide cursor. */
export function hideCursor(): string {
  return `${ESC}?25l`
}

/** Show cursor. */
export function showCursor(): string {
  return `${ESC}?25h`
}

/** Set scroll region (1-indexed, inclusive). */
export function setScrollRegion(top: number, bottom: number): string {
  return `${ESC}${top};${bottom}r`
}

/** Reset scroll region to full terminal. */
export function resetScrollRegion(): string {
  return `${ESC}r`
}

/** Request cursor position (response: ESC[row;colR). */
export function requestCursorPosition(): string {
  return `${ESC}6n`
}

/** Move cursor to bottom of screen. */
export function cursorToBottom(rows: number): string {
  return cursorTo(rows, 1)
}

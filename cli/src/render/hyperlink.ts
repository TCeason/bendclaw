/**
 * OSC 8 terminal hyperlinks.
 *
 * Format: \x1b]8;;URL\x07 TEXT \x1b]8;;\x07
 * Falls back to plain text when the terminal doesn't support hyperlinks.
 */

import chalk from 'chalk'

const OSC8_START = '\x1b]8;;'
const OSC8_END = '\x07'

/**
 * Detect OSC 8 hyperlink support.
 * Most modern terminals (iTerm2, WezTerm, Ghostty, Windows Terminal,
 * GNOME Terminal ≥ 3.26, foot, kitty) support it.
 * We check common env vars; conservative — defaults to false.
 */
export function supportsHyperlinks(): boolean {
  const env = process.env
  // Force override
  if (env.FORCE_HYPERLINK === '1') return true
  if (env.FORCE_HYPERLINK === '0') return false
  // CI / dumb terminals — no
  if (env.CI || env.TERM === 'dumb') return false
  // Known supporting terminals
  if (env.TERM_PROGRAM === 'iTerm.app') return true
  if (env.TERM_PROGRAM === 'WezTerm') return true
  if (env.TERM_PROGRAM === 'ghostty') return true
  // WarpTerminal does NOT support OSC 8 (warpdotdev/Warp#4194)
  // but it auto-detects file paths in plain text — see isWarpTerminal()
  if (env.TERM_PROGRAM === 'WarpTerminal') return false
  if (env.WT_SESSION) return true // Windows Terminal
  if (env.TERM_PROGRAM === 'vscode') return true
  if (env.KITTY_PID) return true
  // macOS Terminal.app does NOT support OSC 8
  if (env.TERM_PROGRAM === 'Apple_Terminal') return false
  // VTE-based terminals (GNOME Terminal, Tilix, etc.)
  if (env.VTE_VERSION) {
    const v = parseInt(env.VTE_VERSION, 10)
    if (!isNaN(v) && v >= 5000) return true
  }
  // Default: off (safe fallback)
  return false
}

/**
 * Warp Terminal auto-detects file paths in plain text and makes them clickable,
 * but ANSI color codes break this detection. Use this to skip coloring file paths.
 */
export function isWarpTerminal(): boolean {
  return process.env.TERM_PROGRAM === 'WarpTerminal'
}

/**
 * Create a clickable hyperlink using OSC 8 escape sequences.
 * Falls back to the plain URL when the terminal doesn't support hyperlinks.
 *
 * @param url - The URL to link to
 * @param text - Display text (shown as clickable link when supported).
 *               When not supported, only the URL is shown.
 */
export function createHyperlink(url: string, text?: string): string {
  if (!supportsHyperlinks()) {
    return url
  }
  const display = text ?? url
  const colored = chalk.blue(display)
  return `${OSC8_START}${url}${OSC8_END}${colored}${OSC8_START}${OSC8_END}`
}

/**
 * Wrap pre-styled text in an OSC 8 hyperlink without changing its color.
 * Falls back to the original text when the terminal doesn't support hyperlinks.
 */
export function wrapHyperlink(url: string, styledText: string): string {
  if (!supportsHyperlinks()) {
    return styledText
  }
  return `${OSC8_START}${url}${OSC8_END}${styledText}${OSC8_START}${OSC8_END}`
}

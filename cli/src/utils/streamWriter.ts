/**
 * StreamWriter — writes streaming markdown directly to stdout, line by line.
 *
 * Mirrors the Rust MarkdownStream approach:
 * - Tokens are buffered until a newline is found
 * - Complete lines are rendered as markdown and written to stdout
 * - The current incomplete line is written as raw text
 * - On the next update, the incomplete line is overwritten (via \r + erase)
 *
 * Ink's dynamic zone (Spinner + PromptInput) sits below our output.
 */

import { renderMarkdown } from './markdown.js'

const MAGENTA_BOLD = '\x1b[35;1m'
const RESET = '\x1b[0m'
const ERASE_LINE = '\x1b[K'

export class StreamWriter {
  private buffer = ''
  private started = false
  private hasPendingLine = false

  /**
   * Push a token (partial text) from the LLM stream.
   */
  push(token: string): void {
    if (!token) return
    this.buffer += token

    // Skip leading whitespace before starting
    if (!this.started) {
      this.buffer = this.buffer.replace(/^[\n\r]+/, '')
      if (this.buffer.length === 0) return
      this.started = true
      process.stdout.write(`\n${MAGENTA_BOLD}⏺${RESET} `)
    }

    // Clear the previous pending (incomplete) line before writing new content
    if (this.hasPendingLine) {
      process.stdout.write(`\r${ERASE_LINE}`)
      this.hasPendingLine = false
    }

    // Render and write all complete lines
    let nlPos = this.buffer.indexOf('\n')
    while (nlPos !== -1) {
      const line = this.buffer.slice(0, nlPos)
      this.buffer = this.buffer.slice(nlPos + 1)

      const rendered = renderMarkdown(line)
      if (rendered && rendered.trim().length > 0) {
        process.stdout.write(rendered.replace(/^\n+/, '').replace(/\n+$/, ''))
      }
      process.stdout.write('\n')

      nlPos = this.buffer.indexOf('\n')
    }

    // Write the current incomplete line as raw text (will be overwritten on next push)
    if (this.buffer.length > 0) {
      process.stdout.write(this.buffer)
      this.hasPendingLine = true
    }
  }

  /** Whether any content has been written to stdout. */
  get hasOutput(): boolean {
    return this.started
  }

  /**
   * Finalize the stream — flush remaining buffer as rendered markdown.
   */
  finish(): void {
    if (!this.started) return

    if (this.hasPendingLine) {
      process.stdout.write(`\r${ERASE_LINE}`)
      this.hasPendingLine = false
    }

    if (this.buffer.trim().length > 0) {
      const rendered = renderMarkdown(this.buffer)
      if (rendered && rendered.trim().length > 0) {
        process.stdout.write(rendered.replace(/^\n+/, '').replace(/\n+$/, ''))
      }
      process.stdout.write('\n')
    }

    this.buffer = ''
    this.started = false
  }
}

/**
 * StreamWriter — writes streaming markdown directly to stdout.
 *
 * Mirrors the Rust MarkdownStream approach: completed lines are rendered
 * and written permanently to the terminal. Only the current incomplete
 * line stays in Ink's dynamic zone. This eliminates flicker from
 * re-rendering the entire output on every delta.
 *
 * Coordinates with Ink by calling stdout.write directly. Ink's <Static>
 * zone is above us, and the dynamic zone (Spinner + PromptInput) is
 * below — they don't overlap with our direct writes.
 */

import { renderMarkdown } from './markdown.js'

const DIM = '\x1b[2m'
const RESET = '\x1b[0m'
const MAGENTA_BOLD = '\x1b[35;1m'

export class StreamWriter {
  private buffer = ''
  private flushedLines = 0
  private started = false

  /**
   * Push a token (partial text) from the LLM stream.
   * Completed lines are rendered and written to stdout immediately.
   */
  push(token: string): void {
    this.buffer += token

    // Find completed lines
    let nlPos = this.buffer.indexOf('\n')
    if (nlPos === -1) return

    // Collect all complete lines
    const lines: string[] = []
    while (nlPos !== -1) {
      lines.push(this.buffer.slice(0, nlPos))
      this.buffer = this.buffer.slice(nlPos + 1)
      nlPos = this.buffer.indexOf('\n')
    }

    if (lines.length === 0) return

    // Render the completed lines as markdown
    const text = lines.join('\n') + '\n'
    const rendered = renderMarkdown(text)
    if (!rendered || rendered.trim().length === 0) return

    // Write prefix on first output
    let output = ''
    if (!this.started) {
      this.started = true
      output += `\n${MAGENTA_BOLD}⏺${RESET} `
    }

    output += rendered.replace(/^\n+/, '')
    // Ensure trailing newline
    if (!output.endsWith('\n')) output += '\n'

    process.stdout.write(output)
    this.flushedLines += output.split('\n').length - 1
  }

  /**
   * Get the current incomplete line (not yet flushed).
   * This is what the Ink dynamic zone should display.
   */
  get pendingText(): string {
    return this.buffer
  }

  /** Whether any content has been written to stdout. */
  get hasOutput(): boolean {
    return this.started
  }

  /** Number of lines written to stdout. */
  get lineCount(): number {
    return this.flushedLines
  }

  /**
   * Flush any remaining buffered content and finalize.
   * Called when the assistant response is complete.
   */
  finish(): void {
    if (this.buffer.trim().length > 0) {
      const rendered = renderMarkdown(this.buffer)
      if (rendered && rendered.trim().length > 0) {
        let output = ''
        if (!this.started) {
          this.started = true
          output += `\n${MAGENTA_BOLD}⏺${RESET} `
        }
        output += rendered.replace(/^\n+/, '')
        if (!output.endsWith('\n')) output += '\n'
        process.stdout.write(output)
        this.flushedLines += output.split('\n').length - 1
      }
    }
    this.buffer = ''
  }

  /** Reset state for a new turn. */
  reset(): void {
    // Flush remaining content before reset
    this.finish()
    this.buffer = ''
    this.flushedLines = 0
    this.started = false
  }
}

/**
 * ScreenLog — writes OutputLines to ~/.evotai/logs/{session_id}.screen.log.
 *
 * Session-level logger that records the expanded (full) version of all
 * screen output for post-hoc debugging.  Callers use:
 *
 *   screenLog.bind(sessionId)   — attach to a session (lazy, idempotent)
 *   screenLog.logMarkdownTrace(entry) — append human-readable markdown trace
 *   screenLog.logMarkdownRenderTrace(entry) — append machine-readable TUI render trace JSONL
 *
 * All I/O errors are silently swallowed so callers never need try/catch.
 */

import { appendFileSync, mkdirSync } from 'fs'
import { join } from 'path'

function homeDir(): string {
  return process.env.HOME || process.env.USERPROFILE || ''
}

function logsDir(): string {
  return join(homeDir(), '.evotai', 'logs')
}

const MARKDOWN_TRACE_SCHEMA_VERSION = 1
const MARKDOWN_RENDER_TRACE_SCHEMA_VERSION = 1
const MARKDOWN_TRACE_FILE_SUFFIX = 'markdown.log'
const MARKDOWN_RENDER_TRACE_FILE_SUFFIX = 'markdown.jsonl'

export interface MarkdownTraceEntry {
  messageId: string
  rendererVersion: string
  rawMarkdown: string
  renderedLines: string[]
}

export interface MarkdownRenderTraceEntry {
  messageId: string
  rendererVersion: string
  columns: number
  rows: number
  source: {
    rawMarkdown: string
  }
  outputLines: Array<{
    id: string
    kind: string
    textAnsi: string
    rawMarkdown?: string
    codeBlockId?: string
    codeLanguage?: string
  }>
  viewBlocks: Array<{
    marginTop: number
    linesAnsi: string[]
  }>
  renderedLinesAnsi: string[]
  appendTextAnsi: string
  statusLinesAnsi: string[]
}

export class ScreenLog {
  private path: string | null = null
  private markdownTracePath: string | null = null
  private markdownRenderTracePath: string | null = null
  private boundSessionId: string | null = null
  private buffer: string[] = []
  private markdownTraceBuffer: MarkdownTraceEntry[] = []
  private markdownRenderTraceBuffer: MarkdownRenderTraceEntry[] = []

  /** Bind (or re-bind) to a session. Flushes any buffered lines. */
  bind(sessionId: string): void {
    if (this.boundSessionId === sessionId) return
    try {
      const dir = logsDir()
      mkdirSync(dir, { recursive: true })
      this.path = join(dir, `${sessionId}.screen.log`)
      this.markdownTracePath = join(dir, `${sessionId}.${MARKDOWN_TRACE_FILE_SUFFIX}`)
      this.markdownRenderTracePath = join(dir, `${sessionId}.${MARKDOWN_RENDER_TRACE_FILE_SUFFIX}`)
      this.boundSessionId = sessionId
      // Flush lines that were logged before bind
      if (this.buffer.length > 0) {
        for (const line of this.buffer) this.appendLine(line)
        this.buffer = []
      }
      if (this.markdownTraceBuffer.length > 0) {
        for (const entry of this.markdownTraceBuffer) this.appendMarkdownTrace(entry)
        this.markdownTraceBuffer = []
      }
      if (this.markdownRenderTraceBuffer.length > 0) {
        for (const entry of this.markdownRenderTraceBuffer) this.appendMarkdownRenderTrace(entry)
        this.markdownRenderTraceBuffer = []
      }
    } catch { /* silently ignore */ }
  }

  get filePath(): string | null {
    return this.path
  }

  get markdownTraceFilePath(): string | null {
    return this.markdownTracePath
  }

  get markdownRenderTraceFilePath(): string | null {
    return this.markdownRenderTracePath
  }

  /** Append rendered lines (with ANSI-stripped) to the log. Buffers if not yet bound. */
  logLines(rendered: string[]): void {
    if (rendered.length === 0) return
    for (const raw of rendered) {
      const line = stripAnsi(raw)
      if (this.path) {
        this.appendLine(line)
      } else {
        this.buffer.push(line)
      }
    }
  }

  logMarkdownTrace(entry: MarkdownTraceEntry): void {
    if (!entry.rawMarkdown.trim()) return
    if (this.markdownTracePath) {
      this.appendMarkdownTrace(entry)
    } else {
      this.markdownTraceBuffer.push(entry)
    }
  }

  logMarkdownRenderTrace(entry: MarkdownRenderTraceEntry): void {
    if (!entry.source.rawMarkdown.trim()) return
    if (this.markdownRenderTracePath) {
      this.appendMarkdownRenderTrace(entry)
    } else {
      this.markdownRenderTraceBuffer.push(entry)
    }
  }

  private appendLine(line: string): void {
    if (!this.path) return
    try {
      const ts = formatTimestamp()
      appendFileSync(this.path, `[${ts}] ${line}\n`, { mode: 0o600 })
    } catch { /* silently ignore */ }
  }

  private appendMarkdownTrace(entry: MarkdownTraceEntry): void {
    if (!this.markdownTracePath) return
    try {
      const trace = [
        `--- markdown trace ${entry.messageId} ---`,
        `ts: ${formatTimestamp()}`,
        `schema_version: ${MARKDOWN_TRACE_SCHEMA_VERSION}`,
        `renderer_version: ${entry.rendererVersion}`,
        '',
        '[raw markdown]',
        entry.rawMarkdown,
        '',
        '[rendered lines]',
        ...entry.renderedLines.map(stripAnsi),
        `--- end markdown trace ${entry.messageId} ---`,
        '',
      ].join('\n')
      appendFileSync(this.markdownTracePath, trace, { mode: 0o600 })
    } catch { /* silently ignore */ }
  }

  private appendMarkdownRenderTrace(entry: MarkdownRenderTraceEntry): void {
    if (!this.markdownRenderTracePath) return
    try {
      const plainRenderedLines = entry.renderedLinesAnsi.map(stripAnsi)
      const plainAppendText = stripAnsi(entry.appendTextAnsi)
      const plainStatusLines = entry.statusLinesAnsi.map(stripAnsi)
      const trace = {
        schema_version: MARKDOWN_RENDER_TRACE_SCHEMA_VERSION,
        kind: 'markdown_render',
        message_id: entry.messageId,
        ts: new Date().toISOString(),
        renderer_version: entry.rendererVersion,
        term: {
          columns: entry.columns,
          rows: entry.rows,
        },
        source: entry.source,
        output_lines: entry.outputLines.map(line => ({
          id: line.id,
          kind: line.kind,
          text_plain: stripAnsi(line.textAnsi),
          text_ansi: line.textAnsi,
          ...(line.rawMarkdown !== undefined ? { raw_markdown: line.rawMarkdown } : {}),
          ...(line.codeBlockId !== undefined ? { code_block_id: line.codeBlockId } : {}),
          ...(line.codeLanguage !== undefined ? { code_language: line.codeLanguage } : {}),
        })),
        view_blocks: entry.viewBlocks.map(block => ({
          margin_top: block.marginTop,
          lines_plain: block.linesAnsi.map(stripAnsi),
          lines_ansi: block.linesAnsi,
        })),
        rendered_lines_plain: plainRenderedLines,
        rendered_lines_ansi: entry.renderedLinesAnsi,
        append_text_plain: plainAppendText,
        append_text_ansi: entry.appendTextAnsi,
        status_lines_plain: plainStatusLines,
        status_lines_ansi: entry.statusLinesAnsi,
      }
      appendFileSync(this.markdownRenderTracePath, `${JSON.stringify(trace)}\n`, { mode: 0o600 })
    } catch { /* silently ignore */ }
  }
}

function stripAnsi(s: string): string {
  return s.replace(/\x1b\[[0-9;]*m/g, '')
}

/** Format current time as YYYY-MM-DD HH:MM:SS.mmm */
function formatTimestamp(): string {
  const d = new Date()
  const y = d.getFullYear()
  const mo = (d.getMonth() + 1).toString().padStart(2, '0')
  const day = d.getDate().toString().padStart(2, '0')
  const h = d.getHours().toString().padStart(2, '0')
  const mi = d.getMinutes().toString().padStart(2, '0')
  const s = d.getSeconds().toString().padStart(2, '0')
  const ms = d.getMilliseconds().toString().padStart(3, '0')
  return `${y}-${mo}-${day} ${h}:${mi}:${s}.${ms}`
}

/**
 * OutputLine — a single line of REPL output.
 *
 * All REPL output (user messages, assistant text, tool results, verbose events)
 * is modeled as an append-only list of OutputLines. These are rendered by
 * Ink's <Static> component, which writes them once and never re-renders.
 *
 * This module is pure logic — no React, no stdout. Easy to test.
 */

import { renderMarkdown, renderThinkingMarkdown } from './markdown.js'
import { colorizeUnifiedDiff } from './diff.js'
import { truncate, formatDuration, toolResultLines } from './format.js'
import type { UIMessage, UIToolCall } from '../term/app/types.js'

// ---------------------------------------------------------------------------
// Tool presentation — icon + primary-arg per tool, in the spirit of
// pi-thinking-steps' semantic glyphs. The status (✓ / ✗ + duration) is
// rendered inline on the same line at finish time, so a tool reads as a
// single “card” line followed only by its real output.
// ---------------------------------------------------------------------------

interface ToolGlyph { icon: string }

/** Map an engine tool name to a compact glyph. Unknown tools fall back to `·`. */
function toolGlyph(name: string): ToolGlyph {
  switch (name.toLowerCase()) {
    case 'bash': return { icon: '⌘' }
    case 'read': case 'read_code': return { icon: '◫' }
    case 'grep': case 'glob': case 'find': case 'search': return { icon: '⌕' }
    case 'web_fetch': case 'webfetch': return { icon: '⊕' }
    case 'edit': case 'file_edit': case 'write': case 'file_write': return { icon: '✎' }
    default: return { icon: '·' }
  }
}

/** The single most useful argument to show beside the tool name. */
function toolPrimaryArg(name: string, args: Record<string, unknown>, previewCommand?: string): string {
  const n = name.toLowerCase()
  if (n === 'bash') {
    // Show the full command — the viewmodel wraps it to terminal width so the
    // tail is never lost. Newlines collapse to spaces for a single logical line.
    return (previewCommand ?? (args?.command as string) ?? '').replace(/\r?\n/g, ' ').trim()
  }
  const path = (args?.path ?? args?.file ?? args?.file_path) as string | undefined
  if (path) return path
  const pattern = (args?.pattern ?? args?.query ?? args?.url) as string | undefined
  // Show the full value — the viewmodel wraps the card arg to terminal width,
  // so the tail is never lost. Newlines collapse to a single logical line.
  if (pattern) return String(pattern).replace(/\r?\n/g, ' ').trim()
  return ''
}

/** Tool call line text: `<glyph> <name>  <primary-arg>`. The viewmodel paints
 *  the glyph and parts; status (✓/✗) lives on the subordinate result line. */
function toolCallText(name: string, args: Record<string, unknown>, previewCommand?: string): string {
  const glyph = toolGlyph(name).icon
  const primary = toolPrimaryArg(name, args, previewCommand)
  return primary ? `${glyph} ${name}  ${primary}` : `${glyph} ${name}`
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  id: string
  kind: 'user' | 'assistant' | 'thinking' | 'tool' | 'tool_result' | 'verbose' | 'error' | 'system'
  text: string
  rawMarkdown?: string
  /** Thinking text already contains markdown ANSI; apply only the outer pi tint. */
  thinkingStyle?: boolean
  codeBlockId?: string
  codeLanguage?: string
  /** Visual spacer inserted between streamed markdown chunks. It creates a
   *  blank line but must not start a new assistant message marker. */
  isContinuationSpacer?: boolean
  /** First line of a committed user/assistant message: gets an OSC 133 zone
   *  start marker so terminals can select/copy the whole message. */
  zoneStart?: boolean
  /** Last line of a committed user/assistant message: gets the OSC 133 zone
   *  end marker. */
  zoneEnd?: boolean
}

// ---------------------------------------------------------------------------
// ID generator
// ---------------------------------------------------------------------------

let nextId = 0

function genId(prefix: string): string {
  return `${prefix}-${nextId++}`
}

/** Reset ID counter (for tests). */
export function resetIdCounter(): void {
  nextId = 0
}

// ---------------------------------------------------------------------------
// Builders — pure functions that create OutputLines from events
// ---------------------------------------------------------------------------

export function buildUserMessage(text: string): OutputLine[] {
  if (!text) return []
  return [{ id: genId('user'), kind: 'user', text, zoneStart: true, zoneEnd: true }]
}

export function buildAssistantLines(markdownText: string): OutputLine[] {
  if (!markdownText.trim()) return []
  const rendered = renderMarkdown(markdownText)
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  const parts = cleaned.split('\n')
  return parts.map((line, i) => ({
    id: genId('asst'),
    kind: 'assistant' as const,
    text: line,
    rawMarkdown: markdownText,
    // Wrap the whole assistant message in one OSC 133 zone (first line starts,
    // last line ends) so it selects/copies as a single block.
    zoneStart: i === 0,
    zoneEnd: i === parts.length - 1,
  }))
}

export function buildThinkingLines(text: string): OutputLine[] {
  if (!text.trim()) return []
  const rendered = renderThinkingMarkdown(text)
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  return cleaned.split('\n').map((line) => ({
    id: genId('think'),
    kind: 'thinking' as const,
    text: line,
    thinkingStyle: true,
  }))
}

export function buildToolCall(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
): OutputLine[] {
  // Reason fields surface the model's justification up-front.
  const lines: OutputLine[] = []
  for (const line of formatReasonLines(args)) {
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${line}` })
  }
  // Call line: `<glyph> <name>  <primary-arg>` — shown as soon as the model
  // finishes decoding the call. Execution state is intentionally separate:
  // only a later tool_started event activates the animated footer spinner.
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: toolCallText(name, args, previewCommand),
  })
  return lines
}

export function buildToolCard(call: UIToolCall, expanded?: boolean, _now = Date.now()): OutputLine[] {
  const details = call.details as Record<string, unknown> | undefined
  const diff = typeof details?.diff === 'string' ? details.diff : undefined
  const args = diff ? { ...call.args, diff } : call.args
  const lines = buildToolCall(call.name, args, call.previewCommand)

  if (call.status === 'queued') return lines

  if (call.status !== 'running') {
    lines.push(...buildToolResult(
      call.name,
      args,
      call.status,
      call.result,
      call.durationMs,
      expanded,
    ))
    return lines
  }

  if (diff) {
    lines.push({ id: genId('tool-diff'), kind: 'tool', text: colorizeUnifiedDiff(diff) })
  }
  if (call.progress && !call.progress.startsWith('__evot_spill_event__ ')) {
    const progressLines = toolResultLines(formatToolResultContent(call.progress), false, call.name, expanded)
    for (const text of progressLines) {
      lines.push({ id: genId('tool-progress'), kind: 'tool_result', text: `  ${text}` })
    }
  }

  // Execution activity is rendered by the animated footer spinner. Keeping a
  // second static `● running` line here makes the tool look stalled between
  // frames, especially immediately after thinking ends.
  return lines
}

export function buildToolResult(
  name: string,
  args: Record<string, unknown>,
  status: 'done' | 'error',
  result?: string,
  durationMs?: number,
  expanded?: boolean,
): OutputLine[] {
  const lines: OutputLine[] = []
  const isError = status === 'error'

  const resultInfo = result ? formatToolResultInfo(result) : ''
  // The status line is appended at the END (after diff/output) so a tool reads
  // top-to-bottom: command → output → closing status. Built here, pushed last.
  const mark = isError ? '✗' : '✓'
  const dur = durationMs !== undefined ? ` · ${formatDuration(durationMs)}` : ''
  const statusLine: OutputLine = {
    id: genId('tool'),
    kind: 'tool',
    text: `  ${mark}${dur}${resultInfo}`,
  }

  // Diff (for write/edit tools)
  const diff = args?.diff as string | undefined
  if (diff && typeof diff === 'string' && diff.length > 0) {
    lines.push({
      id: genId('tool-diff'),
      kind: 'tool',
      text: colorizeUnifiedDiff(diff),
    })
  }

  // Tool result content. Collapsed view is a single `... (+N lines, ctrl+o to
  // expand)` hint; ctrl+o expands to the full body. This applies uniformly to
  // every tool, including Read: previously successful reads rendered no body
  // (the status line's size was considered enough), but that left Read as the
  // only tool whose output couldn't be expanded. Now Read collapses/expands
  // like the rest.
  if (result) {
    const formattedResult = formatToolResultContent(result)
    const resultLines = toolResultLines(formattedResult, isError, name, expanded)
    for (const rl of resultLines) {
      lines.push({
        id: genId('tool-res'),
        kind: isError ? 'error' : 'tool_result',
        text: `  ${rl}`,
      })
    }
    // Show a collapse hint under expanded multiline results. The collapsed
    // view no longer previews content lines: toolResultLines() returns a
    // single `... (+N lines, ctrl+o to expand)` hint, so no extra expand hint
    // is appended here.
    if (expanded && resultLines.length > 1) {
      lines.push({
        id: genId('tool-hint'),
        kind: 'tool_result',
        text: '  \x1b[2m(ctrl+o to collapse)\x1b[0m',
      })
    }
  }

  // Closing status line, after the output.
  lines.push(statusLine)
  return lines
}

export function buildToolProgress(name: string, text: string, expanded?: boolean): OutputLine[] {
  const progressLines = text.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n')
  const total = progressLines.length
  const header = `${toolGlyph(name).icon} ${name}  · ${total} ${total === 1 ? 'line' : 'lines'}`
  const lines: OutputLine[] = [{ id: genId('tool'), kind: 'tool', text: header }]
  if (expanded) {
    // Expanded: full progress body + collapse hint.
    for (const l of progressLines) {
      lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${l}` })
    }
    if (progressLines.length > 1) {
      lines.push({ id: genId('tool-hint'), kind: 'tool_result', text: '  \x1b[2m(ctrl+o to collapse)\x1b[0m' })
    }
    return lines
  }
  // Collapsed: no content preview — the header already carries the line count,
  // so a multiline body just adds a single expand hint (matching tool results).
  if (total > 1) {
    lines.push({ id: genId('tool-hint'), kind: 'tool_result', text: `  \x1b[2m... (+${total} lines, ctrl+o to expand)\x1b[0m` })
  } else {
    lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${progressLines[0] ?? ''}` })
  }
  return lines
}

export function buildVerboseEvent(eventText: string): OutputLine[] {
  if (!eventText) return []
  return eventText.split('\n').map((line) => ({
    id: genId('verb'),
    kind: 'verbose' as const,
    text: line,
  }))
}

/** True for LLM events that must always reach the TUI (errors and retries),
 *  as opposed to per-call stats that only belong in screen.log. */
export function isVisibleLlmEvent(text: string): boolean {
  return /^\[LLM\]\s+[↻✗]/u.test(text)
}

/**
 * Render a visible LLM event (error / retry) as a tool-style card so it reads
 * like any other tool in the stream:
 *   ✦ llm  <model|retry>
 *     ✗|↻ · <meta>
 *     <error message>
 * Falls back to plain verbose lines if the text isn't in the expected shape.
 */
export function buildLlmCard(text: string): OutputLine[] {
  const rawLines = text.split('\n')
  const head = (rawLines[0] ?? '').match(/^\[LLM\]\s+([↻✗])\s*·?\s*(.*)$/u)
  if (!head) return buildVerboseEvent(text)
  const mark = head[1]!
  const rest = (head[2] ?? '').trim()
  const isRetry = mark === '↻'
  // Body: drop the `    error     ` label, keep the message text.
  const body = rawLines.slice(1)
    .map((l) => l.replace(/^\s*error\s+/u, '').trim())
    .filter((l) => l.length > 0)

  const lines: OutputLine[] = []
  if (isRetry) {
    lines.push({ id: genId('tool'), kind: 'tool', text: '✦ llm  retry' })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark} · ${rest}` })
  } else {
    const parts = rest.split(' · ')
    const model = parts[0] ?? 'unknown'
    const meta = parts.slice(1).join(' · ')
    lines.push({ id: genId('tool'), kind: 'tool', text: `✦ llm  ${model}` })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark}${meta ? ` · ${meta}` : ''}` })
  }
  for (const b of body) {
    lines.push({ id: genId('tool-res'), kind: 'error', text: `  ${b}` })
  }
  return lines
}

export function buildError(message: string): OutputLine[] {
  return [{ id: genId('err'), kind: 'error', text: `Error: ${message}` }]
}

export function buildSystem(text: string): OutputLine[] {
  return [{ id: genId('sys'), kind: 'system', text }]
}

// ---------------------------------------------------------------------------
// Convert UIMessages to OutputLines (for resume)
// ---------------------------------------------------------------------------

export function messagesToOutputLines(messages: UIMessage[]): OutputLine[] {
  const lines: OutputLine[] = []
  for (const msg of messages) {
    if (msg.role === 'user') {
      lines.push(...buildUserMessage(msg.text))
      continue
    }

    // Replay only the LLM errors/retries (as cards), matching live behavior.
    if (msg.verboseEvents) {
      for (const evt of msg.verboseEvents) {
        if (isVisibleLlmEvent(evt.text)) lines.push(...buildLlmCard(evt.text))
      }
    }

    if (msg.content) {
      for (const block of [...msg.content].sort((a, b) => a.contentIndex - b.contentIndex)) {
        if (block.type === 'thinking') lines.push(...buildThinkingLines(block.text))
        else if (block.type === 'text') lines.push(...buildAssistantLines(block.text))
        else lines.push(...buildToolCard(block.toolCall))
      }
    } else if (msg.text.trim()) {
      // Legacy UI messages created before ordered assistant content existed.
      lines.push(...buildAssistantLines(msg.text))
    }
  }
  return lines
}

// ---------------------------------------------------------------------------
// Code-block-aware split (inspired by qwen-code's markdownUtilities)
// ---------------------------------------------------------------------------

/**
 * Check if a character index falls inside an unclosed fenced code block.
 */
function isInsideCodeBlock(content: string, index: number): boolean {
  let fenceCount = 0
  let pos = 0
  while (pos < content.length) {
    const next = content.indexOf('```', pos)
    if (next === -1 || next >= index) break
    fenceCount++
    pos = next + 3
  }
  return fenceCount % 2 === 1
}

/**
 * Find the last safe split point in `content` — a position where we can
 * cut without breaking a code block.  Prefers `\n\n` (paragraph boundary),
 * falls back to `\n`.  Returns `content.length` when no safe split exists.
 */
export function findSafeSplitPoint(content: string): number {
  // If the tail is inside an unclosed code block, don't split at all.
  if (isInsideCodeBlock(content, content.length)) return content.length

  // Prefer paragraph boundary (\n\n) not inside a code block.
  let search = content.length
  while (search >= 0) {
    const idx = content.lastIndexOf('\n\n', search)
    if (idx === -1) break
    const splitAt = idx + 2
    if (!isInsideCodeBlock(content, splitAt)) return splitAt
    search = idx - 1
  }

  // Fall back to last single newline not inside a code block.
  const nlPos = content.lastIndexOf('\n')
  if (nlPos > 0 && !isInsideCodeBlock(content, nlPos + 1)) return nlPos + 1

  return content.length
}

// ---------------------------------------------------------------------------
// AssistantStreamBuffer — accumulates streaming tokens, emits lines
// ---------------------------------------------------------------------------

export class AssistantStreamBuffer {
  private buffer = ''
  private started = false

  /** Push a token. Returns OutputLines to append (may be empty). */
  push(token: string): OutputLine[] {
    if (!token) return []
    this.buffer += token

    if (!this.started) {
      this.buffer = this.buffer.replace(/^[\n\r]+/, '')
      if (this.buffer.length === 0) return []
      this.started = true
    }

    return this.flushSafe()
  }

  /** Flush remaining buffer. Returns OutputLines to append. */
  finish(): OutputLine[] {
    if (!this.started) return []
    const result: OutputLine[] = []
    if (this.buffer.trim().length > 0) {
      result.push(...buildAssistantLines(this.buffer))
    }
    this.buffer = ''
    this.started = false
    return result
  }

  /** The current incomplete text (for display in dynamic zone). */
  get pendingText(): string {
    return this.started ? this.buffer : ''
  }

  get isStarted(): boolean {
    return this.started
  }

  /**
   * Flush completed content using code-block-aware splitting.
   * Only the portion before the safe split point is rendered and emitted;
   * the rest stays in the buffer for the dynamic zone.
   */
  private flushSafe(): OutputLine[] {
    if (!this.buffer.includes('\n')) return []

    const splitAt = findSafeSplitPoint(this.buffer)
    if (splitAt === this.buffer.length || splitAt === 0) return []

    const completeText = this.buffer.slice(0, splitAt)
    this.buffer = this.buffer.slice(splitAt)

    return buildAssistantLines(completeText)
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

function parseJsonResult(content: string): unknown | undefined {
  const trimmed = content.trim()
  if (!trimmed) return undefined
  const first = trimmed[0]
  if (first !== '{' && first !== '[') return undefined
  try {
    return JSON.parse(trimmed)
  } catch {
    return undefined
  }
}

function formatToolResultInfo(content: string): string {
  const bytes = Buffer.byteLength(content, 'utf-8')
  // The result body (or its head/tail) is rendered right below this line, so
  // we don't restate its shape ("JSON · N keys"). Keep only what the body
  // doesn't already convey: how many lines and how big.
  const lineCount = content.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n').length
  return lineCount > 1 ? ` · ${lineCount} lines · ${humanBytes(bytes)}` : ` · ${humanBytes(bytes)}`
}

function formatToolResultContent(content: string): string {
  const parsed = parseJsonResult(content)
  if (parsed === undefined) return content
  return JSON.stringify(parsed, null, 2)
}

/** Reason-style fields the model fills to justify a call. Rendered separately
 *  as ↳ lines and excluded from the generic arg list. */
function isReasonKey(key: string): boolean {
  return key === 'reason' || key.startsWith('reason_to_')
}

/** Human label for a reason field key. */
function reasonLabel(key: string): string {
  switch (key) {
    case 'reason':
      return 'reason'
    case 'reason_to_increase_timeout':
      return 'why longer timeout'
    case 'reason_to_use_instead_of_read_file_tool':
      return 'why not read'
    case 'reason_to_use_instead_of_edit_file_tool':
      return 'why not edit'
    case 'reason_to_use_instead_of_glob_files_tool':
      return 'why not glob'
    default:
      return key.replace(/^reason_to_/, 'why ').replace(/_/g, ' ')
  }
}

/** Build ↳ lines for any reason fields present, skipping empty or 'N/A'. */
function formatReasonLines(args: Record<string, unknown>): string[] {
  if (!args || typeof args !== 'object') return []
  const lines: string[] = []
  for (const [k, v] of Object.entries(args)) {
    if (!isReasonKey(k)) continue
    if (typeof v !== 'string') continue
    const val = v.trim()
    if (val === '' || val === 'N/A') continue
    lines.push(`↳ ${reasonLabel(k)}: ${truncate(val, 120)}`)
  }
  return lines
}

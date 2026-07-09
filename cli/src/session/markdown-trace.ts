/**
 * Parse and locate entries in `*.markdown.log` (markdown trace format).
 *
 * Format (schema_version 1):
 *
 *   --- markdown trace <messageId> ---
 *   ts: ...
 *   schema_version: 1
 *   renderer_version: ...
 *
 *   [raw markdown]
 *   <raw>
 *
 *   [rendered lines]
 *   <line>
 *   ...
 *   --- end markdown trace <messageId> ---
 */

import { readFileSync } from 'fs'
import { joinMarkdownChunks } from './assistant-markdown.js'

export interface ParsedMarkdownTrace {
  messageId: string
  ts?: string
  schemaVersion?: number
  rendererVersion?: string
  rawMarkdown: string
  renderedLines: string[]
}

const START_RE = /^--- markdown trace (.+?) ---$/
const END_PREFIX = '--- end markdown trace '

/**
 * Parse every complete markdown-trace block from a log file body.
 * Incomplete trailing blocks are skipped.
 */
export function parseMarkdownTraces(content: string): ParsedMarkdownTrace[] {
  if (!content) return []
  const lines = content.replace(/\r\n/g, '\n').split('\n')
  const traces: ParsedMarkdownTrace[] = []
  let i = 0

  while (i < lines.length) {
    const start = lines[i]!.match(START_RE)
    if (!start) {
      i++
      continue
    }
    const messageId = start[1]!.trim()
    i++

    let ts: string | undefined
    let schemaVersion: number | undefined
    let rendererVersion: string | undefined

    // Header metadata until blank line or a section marker.
    while (i < lines.length) {
      const line = lines[i]!
      if (line === '' || line === '[raw markdown]' || line === '[rendered lines]' || line.startsWith(END_PREFIX)) {
        break
      }
      if (line.startsWith('ts: ')) ts = line.slice(4).trim()
      else if (line.startsWith('schema_version: ')) {
        const n = Number(line.slice('schema_version: '.length).trim())
        if (Number.isFinite(n)) schemaVersion = n
      } else if (line.startsWith('renderer_version: ')) {
        rendererVersion = line.slice('renderer_version: '.length).trim()
      }
      i++
    }

    // Optional blank after header.
    while (i < lines.length && lines[i] === '') i++

    if (lines[i] !== '[raw markdown]') {
      // Malformed — skip to next start marker.
      continue
    }
    i++ // skip [raw markdown]

    const rawParts: string[] = []
    while (i < lines.length && lines[i] !== '[rendered lines]' && !lines[i]!.startsWith(END_PREFIX)) {
      rawParts.push(lines[i]!)
      i++
    }
    // Drop a single trailing blank that the writer inserts before [rendered lines].
    while (rawParts.length > 0 && rawParts[rawParts.length - 1] === '') rawParts.pop()

    let renderedLines: string[] = []
    if (lines[i] === '[rendered lines]') {
      i++
      const renderedParts: string[] = []
      while (i < lines.length && !lines[i]!.startsWith(END_PREFIX)) {
        renderedParts.push(lines[i]!)
        i++
      }
      // Drop trailing blanks before end marker.
      while (renderedParts.length > 0 && renderedParts[renderedParts.length - 1] === '') renderedParts.pop()
      renderedLines = renderedParts
    }

    if (i >= lines.length || !lines[i]!.startsWith(END_PREFIX)) {
      // Incomplete block — abandon.
      continue
    }
    i++ // skip end marker
    // Optional blank after end.
    while (i < lines.length && lines[i] === '') i++

    const rawMarkdown = rawParts.join('\n')
    if (!rawMarkdown.trim()) continue

    traces.push({
      messageId,
      ts,
      schemaVersion,
      rendererVersion,
      rawMarkdown,
      renderedLines,
    })
  }

  return traces
}

/** Last complete trace in a log body, or null. */
export function lastMarkdownTrace(content: string): ParsedMarkdownTrace | null {
  const all = parseMarkdownTraces(content)
  return all.length > 0 ? all[all.length - 1]! : null
}

/**
 * Max gap between consecutive stream-flush traces that still count as one
 * assistant turn. Streaming flushes are seconds apart; a new user prompt +
 * model think is typically well over a minute.
 */
export const MARKDOWN_TURN_GAP_MS = 90_000

/** Parse `YYYY-MM-DD HH:MM:SS.mmm` (local) to epoch ms, or null. */
export function parseMarkdownTraceTs(ts: string | undefined): number | null {
  if (!ts) return null
  // Accept both `2026-07-09 14:05:54.842` and ISO-ish forms.
  const m = ts.trim().match(
    /^(\d{4})-(\d{2})-(\d{2})[ T](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,3}))?/,
  )
  if (!m) {
    const t = Date.parse(ts)
    return Number.isFinite(t) ? t : null
  }
  const ms = (m[7] ?? '0').padEnd(3, '0')
  const d = new Date(
    Number(m[1]),
    Number(m[2]) - 1,
    Number(m[3]),
    Number(m[4]),
    Number(m[5]),
    Number(m[6]),
    Number(ms),
  )
  const t = d.getTime()
  return Number.isFinite(t) ? t : null
}

export interface MarkdownTurn {
  /** Chunks in chronological order (oldest first). */
  traces: ParsedMarkdownTrace[]
  /** Joined raw markdown for the whole turn. */
  rawMarkdown: string
  /** First chunk id (stable label for the turn). */
  messageId: string
  /** Last chunk id. */
  lastMessageId: string
  ts?: string
  rendererVersion?: string
}

/**
 * Group the trailing stream-flush traces into one assistant turn.
 * Walks backward from the newest trace while the inter-chunk gap stays
 * under {@link MARKDOWN_TURN_GAP_MS}.
 */
export function lastMarkdownTurn(
  traces: readonly ParsedMarkdownTrace[],
  gapMs: number = MARKDOWN_TURN_GAP_MS,
): MarkdownTurn | null {
  if (!traces || traces.length === 0) return null

  const last = traces[traces.length - 1]!
  const group: ParsedMarkdownTrace[] = [last]

  for (let i = traces.length - 2; i >= 0; i--) {
    const older = traces[i]!
    const newer = traces[i + 1]!
    const tOlder = parseMarkdownTraceTs(older.ts)
    const tNewer = parseMarkdownTraceTs(newer.ts)
    if (tOlder !== null && tNewer !== null && tNewer - tOlder > gapMs) break
    // If either timestamp is missing, be conservative: stop (don't glue
    // unrelated sessions together).
    if (tOlder === null || tNewer === null) break
    group.unshift(older)
  }

  const first = group[0]!
  const end = group[group.length - 1]!
  return {
    traces: group,
    rawMarkdown: joinMarkdownChunks(group.map(t => t.rawMarkdown)),
    messageId: first.messageId,
    lastMessageId: end.messageId,
    ts: end.ts ?? first.ts,
    rendererVersion: end.rendererVersion ?? first.rendererVersion,
  }
}

/** Find a trace by message id; falls back to last when id is omitted. */
export function findMarkdownTrace(content: string, messageId?: string): ParsedMarkdownTrace | null {
  if (!messageId) return lastMarkdownTrace(content)
  const all = parseMarkdownTraces(content)
  for (let i = all.length - 1; i >= 0; i--) {
    if (all[i]!.messageId === messageId) return all[i]!
  }
  return null
}

/** Read + parse a markdown.log path. Returns null on missing/empty/unreadable. */
export function readMarkdownTraceFile(
  path: string,
  messageId?: string,
): ParsedMarkdownTrace | null {
  try {
    const content = readFileSync(path, 'utf8')
    return findMarkdownTrace(content, messageId)
  } catch {
    return null
  }
}

/**
 * Expand a chunk id to the contiguous stream-flush turn that contains it
 * (same gap rule as {@link lastMarkdownTurn}). Falls back to a single-chunk
 * turn when timestamps are missing.
 */
export function markdownTurnContaining(
  traces: readonly ParsedMarkdownTrace[],
  messageId: string,
  gapMs: number = MARKDOWN_TURN_GAP_MS,
): MarkdownTurn | null {
  if (!traces || traces.length === 0) return null
  const idx = traces.findIndex(t => t.messageId === messageId)
  if (idx < 0) return null

  let start = idx
  for (let i = idx; i > 0; i--) {
    const older = traces[i - 1]!
    const newer = traces[i]!
    const tOlder = parseMarkdownTraceTs(older.ts)
    const tNewer = parseMarkdownTraceTs(newer.ts)
    if (tOlder === null || tNewer === null) break
    if (tNewer - tOlder > gapMs) break
    start = i - 1
  }

  let end = idx
  for (let i = idx; i < traces.length - 1; i++) {
    const older = traces[i]!
    const newer = traces[i + 1]!
    const tOlder = parseMarkdownTraceTs(older.ts)
    const tNewer = parseMarkdownTraceTs(newer.ts)
    if (tOlder === null || tNewer === null) break
    if (tNewer - tOlder > gapMs) break
    end = i + 1
  }

  const group = traces.slice(start, end + 1)
  const first = group[0]!
  const last = group[group.length - 1]!
  return {
    traces: group,
    rawMarkdown: joinMarkdownChunks(group.map(t => t.rawMarkdown)),
    messageId: first.messageId,
    lastMessageId: last.messageId,
    ts: last.ts ?? first.ts,
    rendererVersion: last.rendererVersion ?? first.rendererVersion,
  }
}

/**
 * Read a markdown.log and resolve either the turn containing `messageId`
 * (full stream-flush group) or the trailing last assistant turn.
 */
export function readMarkdownTurnFile(
  path: string,
  messageId?: string,
): MarkdownTurn | null {
  try {
    const content = readFileSync(path, 'utf8')
    const all = parseMarkdownTraces(content)
    if (all.length === 0) return null
    if (messageId) return markdownTurnContaining(all, messageId)
    return lastMarkdownTurn(all)
  } catch {
    return null
  }
}

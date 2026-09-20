/**
 * A compact, colour-free diff of a Task instruction for the confirmation
 * panel: changed lines are marked `-` / `+`, a line with a small edit shows
 * the edit inline as `[-old-]{+new+}`, and unchanged stretches collapse to a
 * count. The reader sees what changes without reading the instruction twice.
 */

import { diffLines, diffWordsWithSpace } from 'diff'

/** Unchanged lines kept on each side of a change. */
const CONTEXT_LINES = 1
/** A removed/added line pair is shown inline when at most this share of
 *  its characters changed; beyond that two full lines read better. */
const INLINE_MAX_CHANGE = 0.5

type Row =
  | { kind: 'same'; text: string }
  | { kind: 'remove'; text: string }
  | { kind: 'add'; text: string }
  | { kind: 'inline'; text: string }

/** Lines describing how `before` becomes `after`. Empty when equal. */
export function instructionDiff(before: string, after: string): string[] {
  if (before === after) return []
  // A final line without a newline must not differ from the same line with
  // one; compare whole lines only.
  const rows = pairRows(diffLines(withNewline(before), withNewline(after)))
  return collapse(rows).map(row => {
    switch (row.kind) {
      case 'same': return `  ${row.text}`
      case 'remove': return `- ${row.text}`
      case 'add': return `+ ${row.text}`
      case 'inline': return `~ ${row.text}`
    }
  })
}

/** Turn line-diff parts into rows, folding a removed line followed by an
 *  added one into an inline row when the edit within it is small. */
function pairRows(parts: ReturnType<typeof diffLines>): Row[] {
  const rows: Row[] = []
  let pendingRemoved: string[] = []
  const flushRemoved = () => {
    for (const text of pendingRemoved) rows.push({ kind: 'remove', text })
    pendingRemoved = []
  }
  for (const part of parts) {
    const lines = splitLines(part.value)
    if (part.removed) {
      pendingRemoved.push(...lines)
      continue
    }
    if (part.added) {
      for (const text of lines) {
        const old = pendingRemoved.shift()
        if (old === undefined) {
          rows.push({ kind: 'add', text })
          continue
        }
        const inline = inlineEdit(old, text)
        if (inline) rows.push({ kind: 'inline', text: inline })
        else rows.push({ kind: 'remove', text: old }, { kind: 'add', text })
      }
      flushRemoved()
      continue
    }
    flushRemoved()
    for (const text of lines) rows.push({ kind: 'same', text })
  }
  flushRemoved()
  return rows
}

/** `[-old-]{+new+}` marks inside one line, or null when the line changed
 *  too much for that to read well. */
function inlineEdit(before: string, after: string): string | null {
  const words = diffWordsWithSpace(before, after)
  let changed = 0
  let out = ''
  for (const word of words) {
    if (word.removed) {
      changed += word.value.length
      out += `[-${word.value}-]`
    } else if (word.added) {
      changed += word.value.length
      out += `{+${word.value}+}`
    } else {
      out += word.value
    }
  }
  const size = Math.max(before.length, after.length, 1)
  return changed / size <= INLINE_MAX_CHANGE ? out : null
}

/** Keep `CONTEXT_LINES` unchanged lines around each change; replace longer
 *  unchanged runs with a count. */
function collapse(rows: Row[]): Row[] {
  const keep = new Array<boolean>(rows.length).fill(false)
  rows.forEach((row, index) => {
    if (row.kind === 'same') return
    for (let i = Math.max(0, index - CONTEXT_LINES); i <= Math.min(rows.length - 1, index + CONTEXT_LINES); i++) {
      keep[i] = true
    }
  })
  const out: Row[] = []
  let hidden: Row[] = []
  const flushHidden = () => {
    // A count only earns its line when it replaces more than one.
    if (hidden.length === 1) out.push(hidden[0]!)
    else if (hidden.length > 1) out.push({ kind: 'same', text: `… ${hidden.length} unchanged lines` })
    hidden = []
  }
  rows.forEach((row, index) => {
    if (keep[index]) {
      flushHidden()
      out.push(row)
    } else {
      hidden.push(row)
    }
  })
  flushHidden()
  return out
}

function withNewline(text: string): string {
  return text.endsWith('\n') ? text : `${text}\n`
}

function splitLines(value: string): string[] {
  const lines = value.split('\n')
  if (lines[lines.length - 1] === '') lines.pop()
  return lines
}

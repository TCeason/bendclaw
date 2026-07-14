/**
 * Escape bare `|` inside inline code spans on GFM table rows.
 *
 * Models often write cells like `` `InMemory | Grace` ``. GFM treats the
 * unescaped pipe as a column boundary, so the cell is truncated and the rest
 * of the row shifts. Convert those pipes to `\|` before lexing.
 *
 * Only table-shaped lines are touched; prose, code fences, and already-escaped
 * pipes are left alone.
 */

/** Line looks like a pipe table row (not a prose sentence with one |). */
export function looksLikeTableRow(line: string): boolean {
  const t = line.trim()
  if (!t.includes('|')) return false
  if (/^\|/.test(t)) return true
  // "a | b | c" style without leading pipe
  return (t.match(/\|/g) ?? []).length >= 2
}

/** Alignment / separator row: |---|:---:| */
function isTableSeparatorRow(line: string): boolean {
  return /^\s*\|?[\s\-:|]+\|?\s*$/.test(line) && /[-:]/.test(line)
}

/**
 * Escape unescaped `|` that sit inside single-backtick inline code on one line.
 */
export function escapePipesInTableRow(line: string): string {
  if (!looksLikeTableRow(line) || isTableSeparatorRow(line)) return line

  let out = ''
  let i = 0
  let inCode = false

  while (i < line.length) {
    const ch = line[i]!

    if (ch === '`') {
      let j = i
      while (j < line.length && line[j] === '`') j++
      const runLen = j - i
      // Only toggle on single-backtick spans (codespan). Multi-tick runs are
      // rare inside table rows; leave them literal without toggling.
      if (runLen === 1) inCode = !inCode
      out += line.slice(i, j)
      i = j
      continue
    }

    if (ch === '|' && inCode) {
      // Keep already-escaped pipes as-is.
      if (out.endsWith('\\')) out += '|'
      else out += '\\|'
      i++
      continue
    }

    out += ch
    i++
  }

  return out
}

/** Full-document pass: table rows only. */
export function escapePipesInTableInlineCode(text: string): string {
  if (!text.includes('|') || !text.includes('`')) return text
  const lines = text.split('\n')
  let changed = false
  for (let i = 0; i < lines.length; i++) {
    const next = escapePipesInTableRow(lines[i]!)
    if (next !== lines[i]) {
      lines[i] = next
      changed = true
    }
  }
  return changed ? lines.join('\n') : text
}

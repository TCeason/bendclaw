/**
 * MarkdownTable — renders a markdown table as an Ink component.
 *
 * Features:
 * - Terminal-width aware column sizing
 * - Alignment support (left/center/right)
 * - Dim borders, bold headers, clean data rows (no inter-row separators)
 * - Falls back to vertical key-value format when columns are too narrow
 */

import React from 'react'
import { Text } from 'ink'
import { type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import chalk from 'chalk'
import { formatToken } from '../render/markdown.js'

const MIN_COL_WIDTH = 3
const SAFETY_MARGIN = 4

interface Props {
  token: Tokens.Table
  termWidth: number
}

export function MarkdownTable({ token, termWidth }: Props) {
  const d = chalk.dim

  function formatCell(tokens: Token[] | undefined): string {
    return tokens?.map(t => formatToken(t)).join('') ?? ''
  }

  function displayWidth(tokens: Token[] | undefined): number {
    return stringWidth(stripAnsi(formatCell(tokens)))
  }

  // Column widths: max of header + all rows
  const columnWidths = token.header.map((header, i) => {
    let max = displayWidth(header.tokens)
    for (const row of token.rows) {
      max = Math.max(max, displayWidth(row[i]?.tokens))
    }
    return Math.max(max, MIN_COL_WIDTH)
  })

  // Check if table fits terminal
  const numCols = token.header.length
  const borderOverhead = 1 + numCols * 3 // │ + (space + content + space│) per col
  const totalWidth = columnWidths.reduce((s, w) => s + w, 0) + borderOverhead
  const availableWidth = termWidth - SAFETY_MARGIN

  // If table is too wide, fall back to vertical format
  if (totalWidth > availableWidth && numCols >= 2) {
    return <Text>{renderVertical(token, termWidth)}</Text>
  }

  const lines: string[] = []

  // Top border
  lines.push(d('┌') + columnWidths.map((w, i) =>
    d('─'.repeat(w + 2)) + d(i < numCols - 1 ? '┬' : '┐'),
  ).join(''))

  // Header row (bold)
  lines.push(d('│') + token.header.map((header, i) => {
    const content = chalk.bold(formatCell(header.tokens))
    const dw = displayWidth(header.tokens)
    const width = columnWidths[i] ?? MIN_COL_WIDTH
    const align = token.align?.[i] ?? 'left'
    return ' ' + padAligned(content, dw, width, align) + ' ' + d('│')
  }).join(''))

  // Header separator
  lines.push(d('├') + columnWidths.map((w, i) =>
    d('─'.repeat(w + 2)) + d(i < numCols - 1 ? '┼' : '┤'),
  ).join(''))

  // Data rows (with separators between each row)
  for (let ri = 0; ri < token.rows.length; ri++) {
    const row = token.rows[ri]!
    lines.push(d('│') + row.map((cell, i) => {
      const content = formatCell(cell.tokens)
      const dw = displayWidth(cell.tokens)
      const width = columnWidths[i] ?? MIN_COL_WIDTH
      const align = token.align?.[i] ?? 'left'
      return ' ' + padAligned(content, dw, width, align) + ' ' + d('│')
    }).join(''))
    if (ri < token.rows.length - 1) {
      lines.push(d('├') + columnWidths.map((w, i) =>
        d('─'.repeat(w + 2)) + d(i < numCols - 1 ? '┼' : '┤'),
      ).join(''))
    }
  }

  // Bottom border
  lines.push(d('└') + columnWidths.map((w, i) =>
    d('─'.repeat(w + 2)) + d(i < numCols - 1 ? '┴' : '┘'),
  ).join(''))

  return <Text>{lines.join('\n')}</Text>
}

/**
 * Pad content to targetWidth respecting alignment.
 */
function padAligned(
  content: string,
  displayWidth: number,
  targetWidth: number,
  align: string | null | undefined,
): string {
  const padding = Math.max(0, targetWidth - displayWidth)
  if (align === 'center') {
    const left = Math.floor(padding / 2)
    return ' '.repeat(left) + content + ' '.repeat(padding - left)
  }
  if (align === 'right') {
    return ' '.repeat(padding) + content
  }
  return content + ' '.repeat(padding)
}

/**
 * Vertical key-value format for narrow terminals.
 */
function renderVertical(token: Tokens.Table, _termWidth: number): string {
  const headers = token.header.map(h =>
    stripAnsi(h.tokens?.map(t => formatToken(t)).join('') ?? ''),
  )
  const lines: string[] = []
  const separator = chalk.dim('─'.repeat(Math.min(_termWidth - 1, 40)))

  for (let ri = 0; ri < token.rows.length; ri++) {
    if (ri > 0) lines.push(separator)
    const row = token.rows[ri]!
    for (let ci = 0; ci < row.length; ci++) {
      const label = headers[ci] ?? `Col ${ci + 1}`
      const value = (row[ci]?.tokens?.map(t => formatToken(t)).join('') ?? '').trim()
      lines.push(`${chalk.bold(label + ':')} ${value}`)
    }
  }
  return lines.join('\n')
}

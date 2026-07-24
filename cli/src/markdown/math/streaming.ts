import { CODE_FENCE_RE } from '../primitives.js'

export interface MathBlockScan {
  /** Offset of the display-math block that is still open at end-of-input. */
  openStart: number | null
}

interface OpenMathBlock {
  closing: '$$' | '\\]'
  start: number
}

function unescapedDelimiterAt(line: string, delimiter: string, from: number): number {
  let cursor = from
  while (cursor < line.length) {
    const found = line.indexOf(delimiter, cursor)
    if (found === -1) return -1
    let slashes = 0
    for (let index = found - 1; index >= 0 && line[index] === '\\'; index--) slashes++
    if (slashes % 2 === 0) return found
    cursor = found + delimiter.length
  }
  return -1
}

function openingMathDelimiter(line: string): { closing: '$$' | '\\]', contentStart: number } | null {
  const match = /^( {0,3})(\$\$|\\\[)[ \t]*/.exec(line)
  if (!match) return null
  return {
    closing: match[2] === '$$' ? '$$' : '\\]',
    contentStart: match[0].length,
  }
}

/**
 * Scan display-math boundaries without parsing formula contents. Code fences
 * are deliberately excluded so examples containing `$$` remain ordinary code.
 */
export function scanMathBlocks(text: string): MathBlockScan {
  let inFence = false
  let fenceMarker = ''
  let open: OpenMathBlock | null = null
  let offset = 0

  for (const line of text.split('\n')) {
    const fence = CODE_FENCE_RE.exec(line)
    if (!open && fence) {
      const marker = fence[2]!
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (marker[0] === fenceMarker[0] && marker.length >= fenceMarker.length) {
        inFence = false
        fenceMarker = ''
      }
      offset += line.length + 1
      continue
    }

    if (inFence) {
      offset += line.length + 1
      continue
    }

    if (open) {
      const closeAt = unescapedDelimiterAt(line, open.closing, 0)
      const closesLine = closeAt >= 0
        && line.slice(0, closeAt).trim() === ''
        && line.slice(closeAt + open.closing.length).trim() === ''
      if (closesLine) open = null
      offset += line.length + 1
      continue
    }

    const opening = openingMathDelimiter(line)
    if (!opening) {
      offset += line.length + 1
      continue
    }

    const closeAt = unescapedDelimiterAt(line, opening.closing, opening.contentStart)
    const closesLine = closeAt >= opening.contentStart
      && line.slice(closeAt + opening.closing.length).trim() === ''
    if (!closesLine) open = { closing: opening.closing, start: offset }
    offset += line.length + 1
  }

  return {
    openStart: open?.start ?? null,
  }
}

export function isInsideOpenMathBlock(text: string): boolean {
  return scanMathBlocks(text).openStart !== null
}

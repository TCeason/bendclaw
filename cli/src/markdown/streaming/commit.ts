import {
  BOX_DRAWING_RE,
  CODE_FENCE_RE,
  fenceLanguageFromLine,
  isLikelyFenceClose,
  isPlainTextFenceLanguage,
  repairUnclosedFences,
  shouldClosePlainTextFenceBeforeMarkdown,
} from '../normalize/index.js'
import { lexRawMarkdownTokens } from '../parse/marked.js'

// ---------------------------------------------------------------------------
// Streaming markdown block splitter
// ---------------------------------------------------------------------------

export interface MarkdownSplit {
  /** Completed markdown blocks that can be committed to Static */
  completed: string
  /** Incomplete tail that stays in the dynamic zone */
  pending: string
}

/**
 * Split streaming markdown text into completed blocks and a pending tail.
 *
 * A "completed block" is a paragraph, code block, heading, list, table, etc.
 * that is fully formed and won't change with more tokens.
 *
 * Rules:
 * - A blank line (`\n\n`) is a paragraph boundary — everything before it is complete
 * - An open code fence (```) without a matching close is NOT complete
 * - The pending tail is always the text after the last safe split point
 */
export function splitMarkdownBlocks(text: string): MarkdownSplit {
  if (!text) return { completed: '', pending: '' }

  const commitPoint = findStreamingCommitPoint(text)
  return {
    completed: text.slice(0, commitPoint),
    pending: text.slice(commitPoint),
  }
}

function streamingTreeTailCommitPoint(text: string): number | null {
  if (!BOX_DRAWING_RE.test(text) || text.endsWith('\n\n')) return null
  const lines = text.split('\n')
  let lastNonEmptyIndex = lines.length - 1
  while (lastNonEmptyIndex >= 0 && lines[lastNonEmptyIndex]!.trim() === '') {
    lastNonEmptyIndex--
  }
  if (lastNonEmptyIndex < 0) return null

  let tailLineStart = lastNonEmptyIndex
  while (tailLineStart > 0 && lines[tailLineStart - 1]!.trim() !== '') {
    tailLineStart--
  }
  const tailLines = lines.slice(tailLineStart, lastNonEmptyIndex + 1)
  if (!tailLooksLikeTreeBlock(tailLines)) return null
  if (tailLineStart === 0) return 0

  let offset = 0
  for (let i = 0; i < tailLineStart; i++) {
    offset += lines[i]!.length + 1
  }
  return offset
}

function tailLooksLikeTreeBlock(lines: string[]): boolean {
  const meaningful = lines.filter(line => line.trim() !== '')
  if (meaningful.length === 0) return false
  const treeLineCount = meaningful.filter(looksLikeTreeLine).length
  return treeLineCount > 0 && treeLineCount === meaningful.length
}

function looksLikeTreeLine(line: string): boolean {
  const trimmed = line.trimStart()
  return /^⏺\s+\S/.test(trimmed)
    || /^[/~.][^\s]*/.test(trimmed)
    || /^[│├└]\s*$/.test(trimmed)
    || /^[│ ]*[├└]──\s+/.test(trimmed)
    || /^[│ ]+│/.test(trimmed)
}

function openPlainTextDiagramFenceStart(text: string): number | null {
  const lines = text.split('\n')
  let offset = 0
  let openStart: number | null = null
  let openMarker = ''
  let openLength = 0
  let openLang: string | null = null
  let codeLines: string[] = []

  for (const line of lines) {
    const match = CODE_FENCE_RE.exec(line)
    if (openStart === null) {
      if (match) {
        openStart = offset
        openMarker = match[2]![0]!
        openLength = match[2]!.length
        openLang = fenceLanguageFromLine(line)
        codeLines = []
      }
    } else if (isLikelyFenceClose(line, openMarker, openLength)) {
      openStart = null
      openMarker = ''
      openLength = 0
      openLang = null
      codeLines = []
    } else if (shouldClosePlainTextFenceBeforeMarkdown(line, codeLines, openLang)) {
      return null
    } else {
      codeLines.push(line)
    }
    offset += line.length + 1
  }

  if (openStart === null) return null
  if (!isPlainTextFenceLanguage(openLang)) return null
  if (!codeLines.some(codeLine => BOX_DRAWING_RE.test(codeLine))) return null
  return openStart
}

export function isInsideOpenMathBlock(text: string): boolean {
  let inFence = false
  let fenceMarker = ''
  let openMath = false

  for (const line of text.split('\n')) {
    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      const marker = fenceMatch[2]!
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (marker[0] === fenceMarker[0] && marker.length >= fenceMarker.length) {
        inFence = false
        fenceMarker = ''
      }
      continue
    }
    if (inFence) continue

    if (line.trim() === '$$') openMath = !openMath
  }

  return openMath
}

function firstCompleteMathBlockStart(text: string): number | null {
  let inFence = false
  let fenceMarker = ''
  let openMathStart: number | null = null
  let offset = 0

  for (const line of text.split('\n')) {
    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      const marker = fenceMatch[2]!
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

    if (!inFence && line.trim() === '$$') {
      if (openMathStart === null) {
        openMathStart = offset
      } else {
        return openMathStart
      }
    }
    offset += line.length + 1
  }

  return null
}

export function findStreamingCommitPoint(text: string): number {
  if (!text) return 0

  if (isInsideOpenMathBlock(text)) return 0

  const completeMathBlockStart = firstCompleteMathBlockStart(text)
  if (completeMathBlockStart !== null) return completeMathBlockStart

  const openTextDiagramStart = openPlainTextDiagramFenceStart(text)
  if (openTextDiagramStart !== null) return openTextDiagramStart

  const treeTailCommitPoint = streamingTreeTailCommitPoint(text)
  if (treeTailCommitPoint !== null) return treeTailCommitPoint

  const repaired = repairUnclosedFences(text, false)
  if (repaired !== text) {
    const insertedAt = firstDifferenceIndex(text, repaired)
    return insertedAt > 0 ? insertedAt : 0
  }

  const tokens = lexRawMarkdownTokens(text)
  let lastContentIdx = tokens.length - 1
  while (lastContentIdx >= 0 && tokens[lastContentIdx]!.type === 'space') {
    lastContentIdx--
  }
  if (lastContentIdx <= 0) return text.endsWith('\n\n') ? text.length : 0

  let splitAt = 0
  for (let i = 0; i < lastContentIdx; i++) {
    splitAt += tokens[i]!.raw.length
  }
  if (splitAt <= 0 || splitAt >= text.length) return text.endsWith('\n\n') ? text.length : 0
  return splitAt
}

export function findNaturalPlainTextCommitPoint(text: string, termRows: number): number {
  if (!text) return 0
  if (repairUnclosedFences(text, false) !== text) return 0
  if (hasMarkdownBlockSyntax(text)) return 0
  if (hasUnclosedInlineSyntax(text)) return 0

  const lines = text.split('\n')
  const threshold = Math.max(6, Math.floor(termRows / 3))
  if (lines.length <= threshold) return 0

  let commitLineCount = lines.length - 2
  while (commitLineCount > 0 && lines[commitLineCount - 1]!.trim() === '') {
    commitLineCount--
  }
  if (commitLineCount <= 0) return 0

  let splitAt = 0
  for (let i = 0; i < commitLineCount; i++) {
    splitAt += lines[i]!.length + 1
  }
  if (splitAt <= 0 || splitAt >= text.length) return 0
  return splitAt
}

function hasMarkdownBlockSyntax(text: string): boolean {
  for (const line of text.split('\n')) {
    const trimmed = line.trimStart()
    if (!trimmed) continue
    if (/^(```|~~~)/.test(trimmed)) return true
    if (BOX_DRAWING_RE.test(line)) return true
    if (/^⏺\s+\S/.test(trimmed)) return true
    if (/^(?: {4}|\t)\S/.test(line)) return true
    if (/^#{1,6}(?:\s|$)/.test(trimmed)) return true
    if (/^[-*+]\s+/.test(trimmed)) return true
    if (/^\d+\.\s+/.test(trimmed)) return true
    if (/^>\s?/.test(trimmed)) return true
    if (/^\|.*\|\s*$/.test(trimmed)) return true
    if (/^<\/?[A-Za-z][^>]*>\s*$/.test(trimmed)) return true
    if (/^[-*_]{3,}\s*$/.test(trimmed)) return true
  }
  return false
}

function hasUnclosedInlineSyntax(text: string): boolean {
  return countUnescaped(text, '`') % 2 === 1
    || countUnescaped(text, '[') !== countUnescaped(text, ']')
    || countUnescaped(text, '(') !== countUnescaped(text, ')')
    || countUnescaped(text, '**') % 2 === 1
    || countUnescaped(text, '__') % 2 === 1
}

function countUnescaped(text: string, needle: string): number {
  let count = 0
  let idx = 0
  while (idx < text.length) {
    const found = text.indexOf(needle, idx)
    if (found === -1) break
    if (!isEscaped(text, found)) count++
    idx = found + needle.length
  }
  return count
}

function isEscaped(text: string, index: number): boolean {
  let slashCount = 0
  for (let i = index - 1; i >= 0 && text[i] === '\\'; i--) {
    slashCount++
  }
  return slashCount % 2 === 1
}

function firstDifferenceIndex(a: string, b: string): number {
  const limit = Math.min(a.length, b.length)
  for (let i = 0; i < limit; i++) {
    if (a[i] !== b[i]) return i
  }
  return limit
}

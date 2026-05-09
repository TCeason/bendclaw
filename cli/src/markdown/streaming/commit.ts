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

export function findStreamingCommitPoint(text: string): number {
  if (!text) return 0

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

function firstDifferenceIndex(a: string, b: string): number {
  const limit = Math.min(a.length, b.length)
  for (let i = 0; i < limit; i++) {
    if (a[i] !== b[i]) return i
  }
  return limit
}

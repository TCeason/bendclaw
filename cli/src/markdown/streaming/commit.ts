import { BOX_DRAWING_RE, CODE_FENCE_RE } from '../primitives.js'
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

/**
 * True when the streaming buffer ends inside an unterminated code fence
 * (opening ``` / ~~~ with no matching close yet). Force-splitting here would
 * commit the partial fence as a closed code block and render the remaining
 * lines as plain prose — tearing the block. Callers hold such text pending.
 */
export function isInsideOpenCodeFence(text: string): boolean {
  return openCodeFenceStart(text) !== null
}

/**
 * Byte offset where the last still-open code fence begins, or `null` when no
 * fence is open. Used to commit prose before an in-progress fence while
 * holding the fence itself (and its body) pending until the close arrives.
 */
function openCodeFenceStart(text: string): number | null {
  let inFence = false
  let fenceMarker = ''
  let fenceStart = 0
  let offset = 0

  for (const line of text.split('\n')) {
    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      const marker = fenceMatch[2]!
      if (!inFence) {
        inFence = true
        fenceMarker = marker
        fenceStart = offset
      } else if (marker[0] === fenceMarker[0] && marker.length >= fenceMarker.length) {
        inFence = false
        fenceMarker = ''
      }
    }
    offset += line.length + 1
  }

  return inFence ? fenceStart : null
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

  // An open code fence is never committable: commit only the prose before it
  // and hold the fence (plus its still-forming body) pending.
  const openFenceStart = openCodeFenceStart(text)
  if (openFenceStart !== null) return openFenceStart > 0 ? openFenceStart : 0

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
  if (isInsideOpenCodeFence(text)) return 0
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

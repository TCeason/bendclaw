/**
 * Named fence-repair cases.
 *
 * Each case is an independent rule: match (when) + apply (how).
 * The engine only schedules these — new bugs become a new case + fixture,
 * not another branch inside a shared shouldClose* helper.
 *
 * Order in CASES is priority (first match wins).
 */

import { CODE_FENCE_RE } from '../../primitives.js'
import {
  fenceLanguageFromLine,
  isFenceLine,
  isLikelyFenceClose,
  isShellFenceLanguage,
  looksLikeCodeCommentOrContinuation,
  looksLikeCodeCompleted,
  looksLikeMarkdownAfterFenceRest,
  looksLikeMarkdownBoundary,
  looksLikePlainMarkdownAfterCode,
  looksLikeStructuredCode,
  normalizeGluedHeadingRest,
  parseFenceLine,
  startsWithCjkProse,
} from './signals.js'
import type { FenceAction, FenceCase, FenceCtx, FenceOpenState } from './types.js'

function openFromFenceLine(line: string): FenceOpenState | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  const marker = match[2]![0]!
  const length = match[2]!.length
  return {
    marker,
    length,
    close: marker.repeat(length),
    lang: fenceLanguageFromLine(line),
    codeLines: [],
  }
}

/**
 * Bare fence with no info string that looks like a leftover close rather than
 * a real open: the following non-blank content is markdown/prose and no
 * matching close appears later. Dropping it prevents the rest of the document
 * from being swallowed as code (common at overflow chunk boundaries).
 */
function shouldTreatAsStrayFenceClose(lines: string[], index: number): boolean {
  const line = lines[index]!
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return false
  if (match[3]!.trim()) return false

  let sawMarkdownLike = false
  for (let i = index + 1; i < lines.length; i++) {
    const next = lines[i]!
    if (!next.trim()) continue
    if (isLikelyFenceClose(next, match[2]![0]!, match[2]!.length)) {
      // A matching close exists later → this is a real open fence.
      return false
    }
    if (!sawMarkdownLike) {
      sawMarkdownLike = looksLikeMarkdownBoundary(next)
        || startsWithCjkProse(next)
        || looksLikePlainMarkdownAfterCode(next)
      // First content looks like code body, not a stray close.
      if (!sawMarkdownLike) return false
    }
  }
  // No matching close and body looks like markdown/prose → stray.
  return sawMarkdownLike
}

/**
 * Fence open glued onto the end of a content line, with markdown after the
 * fence on the same line — e.g. `` ```## next`` after a close was missed.
 */
function splitGluedMarkdownAfterFenceClose(
  line: string,
  marker: string,
  minLength: number,
): string[] | null {
  const parsed = parseFenceLine(line)
  if (!parsed) return null
  if (parsed.marker[0] !== marker || parsed.marker.length < minLength) return null
  const rest = normalizeGluedHeadingRest(parsed.rest)
  if (!rest) return null
  if (!looksLikeMarkdownAfterFenceRest(rest)) return null
  return [`${parsed.indent}${parsed.marker}`, rest]
}

/**
 * Content line with a trailing close fence glued on, optionally followed by
 * more markdown on the same line.
 */
function splitTrailingFenceClose(line: string, marker: string, minLength: number): string[] | null {
  if (CODE_FENCE_RE.test(line)) return null
  const fenceRun = `${marker === '`' ? '`' : '~'}{${minLength},}`
  const trailing = new RegExp(`^(.*?)(${fenceRun})[ \\t]*$`)
  const trailingMatch = trailing.exec(line)
  if (trailingMatch) {
    const content = trailingMatch[1]!
    const fence = trailingMatch[2]!
    if (!content.trim()) return null
    if (content.includes(marker.repeat(minLength))) return null
    return [content.trimEnd(), fence]
  }

  const gluedMarkdown = new RegExp(`^(.*?)(${fenceRun})(.*)$`)
  const gluedMatch = gluedMarkdown.exec(line)
  if (!gluedMatch) return null
  const content = gluedMatch[1]!
  const fence = gluedMatch[2]!
  const rest = gluedMatch[3]!.trimStart()
  if (!content.trim()) return null
  if (content.includes(marker.repeat(minLength))) return null
  if (!rest) return null
  const normalizedRest = normalizeGluedHeadingRest(rest)
  if (!looksLikeMarkdownAfterFenceRest(normalizedRest)) return null
  return [content.trimEnd(), fence, normalizedRest]
}

const strayBareClose: FenceCase = {
  id: 'stray-bare-close',
  match: (ctx) => !ctx.open && !!CODE_FENCE_RE.exec(ctx.line) && shouldTreatAsStrayFenceClose(ctx.lines, ctx.index),
  apply: (): FenceAction => ({ type: 'skip' }),
}

const realOpen: FenceCase = {
  id: 'real-open',
  match: (ctx) => !ctx.open && !!CODE_FENCE_RE.exec(ctx.line),
  apply: (ctx): FenceAction => {
    const open = openFromFenceLine(ctx.line)
    if (!open) return { type: 'passthrough' }
    return { type: 'emit', lines: [ctx.line], open }
  },
}

const gluedCloseSplit: FenceCase = {
  id: 'glued-close-split',
  match: (ctx) => {
    if (!ctx.open) return false
    return splitGluedMarkdownAfterFenceClose(ctx.line, ctx.open.marker, ctx.open.length) !== null
  },
  apply: (ctx): FenceAction => {
    const parts = splitGluedMarkdownAfterFenceClose(ctx.line, ctx.open!.marker, ctx.open!.length)
    if (!parts) return { type: 'passthrough' }
    return { type: 'emit', lines: parts, closeOpen: true }
  },
}

const trailingCloseSplit: FenceCase = {
  id: 'trailing-close-split',
  match: (ctx) => {
    if (!ctx.open) return false
    return splitTrailingFenceClose(ctx.line, ctx.open.marker, ctx.open.length) !== null
  },
  apply: (ctx): FenceAction => {
    const parts = splitTrailingFenceClose(ctx.line, ctx.open!.marker, ctx.open!.length)
    if (!parts) return { type: 'passthrough' }
    return { type: 'emit', lines: parts, closeOpen: true }
  },
}

const realClose: FenceCase = {
  id: 'real-close',
  match: (ctx) => !!ctx.open && isLikelyFenceClose(ctx.line, ctx.open.marker, ctx.open.length),
  apply: (ctx): FenceAction => ({ type: 'emit', lines: [ctx.line], closeOpen: true }),
}

/**
 * P0: shell fence meets a markdown boundary (heading / table / list / …).
 * Models often omit the blank line between a one-line command and the next
 * section, so we do NOT require the previous code line to be blank.
 */
const shellCloseBeforeMdBoundary: FenceCase = {
  id: 'shell-close-before-md-boundary',
  match: (ctx) => {
    if (!ctx.open) return false
    if (!isShellFenceLanguage(ctx.open.lang)) return false
    if (ctx.open.codeLines.length === 0) return false
    if (isFenceLine(ctx.line)) return false
    if (looksLikeCodeCommentOrContinuation(ctx.line, ctx.open.lang)) return false
    return looksLikeMarkdownBoundary(ctx.line)
  },
  apply: (ctx): FenceAction => ({ type: 'close-then-emit', line: ctx.line }),
}

/**
 * Structured fences (json/sql/…) that look complete, then hit a strong
 * markdown boundary. Plaintext fences are intentionally excluded.
 */
const structuredCloseBeforeMdBoundary: FenceCase = {
  id: 'structured-close-before-md-boundary',
  match: (ctx) => {
    if (!ctx.open) return false
    if (isFenceLine(ctx.line)) return false
    if (!looksLikeStructuredCode(ctx.open.codeLines, ctx.open.lang)) return false
    // Never close before a comment / language continuation — multi-statement SQL
    // with `-- CJK comments` after `;` is the production regression that used to
    // tear the fence open and swallow the rest of the document.
    if (looksLikeCodeCommentOrContinuation(ctx.line, ctx.open.lang)) return false
    // Only strong markdown boundaries (heading / list / table / hr / quote).
    if (!looksLikeMarkdownBoundary(ctx.line)) return false
    return looksLikeCodeCompleted(ctx.open.codeLines, ctx.open.lang)
  },
  apply: (ctx): FenceAction => ({ type: 'close-then-emit', line: ctx.line }),
}

/** First match wins — order is the priority list. */
export const CASES: readonly FenceCase[] = [
  strayBareClose,
  realOpen,
  gluedCloseSplit,
  trailingCloseSplit,
  realClose,
  shellCloseBeforeMdBoundary,
  structuredCloseBeforeMdBoundary,
]

/** Exported for fixture / debug attribution. */
export function matchFenceCase(ctx: FenceCtx): FenceCase | null {
  for (const c of CASES) {
    if (c.match(ctx)) return c
  }
  return null
}

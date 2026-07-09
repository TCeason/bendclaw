/**
 * Minimal fence repair for model-glued / stray code fences.
 *
 * CommonMark only recognizes fences at line start. Models frequently:
 *   1. Glue an opening fence to the end of a heading/prose line
 *      (`### title``` → not a fence for marked)
 *   2. Emit a bare ``` that was meant as a close, which then becomes an
 *      unpaired open and swallows the rest of the document as code
 *   3. Leave a fence unclosed before the next heading/paragraph
 *
 * This module fixes only those fence-boundary failures. It deliberately
 * does NOT restore the broader glue-normalization (tables/headings/hr/…)
 * that was dropped to stay closer to pi's "render as-is" philosophy.
 */

import { CODE_FENCE_RE } from '../primitives.js'

const MARKDOWN_BOUNDARY_RE =
  /^([ \t]{0,3})?(#{1,6}(?:\s|$)|#{2,6}(?=[^#\s])|(?:\*\*|__)|(?:[-*+]\s)|(?:\d+\.\s)|>\s|\|.*\||-{3,}\s*$)/
const SQL_KEYWORD_RE = /^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP|SET|MERGE|TRUNCATE)\b/i
const CODE_LIKE_START_RE = /^[\[{(}\]),;]|^\/\/|^#\s*include\b/
const CODE_KEYWORD_RE =
  /^(return|if|else|for|while|switch|case|break|continue|try|catch|finally|throw|await|async|const|let|var|function|class|def|import|export|from|SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|WHERE|ORDER|GROUP|LIMIT|DROP|SET|ALTER|MERGE|TRUNCATE)\b/i
const CODE_ASSIGNMENT_RE = /^[\w$.'"`-]+\s*[:=]/
// Opening fence glued to the end of a non-empty lead (not at line start).
// Info string must be empty or a plain language tag so prose backticks stay put.
const FENCE_OPEN_GLUED_RE = /^([^\n`]*?[^\s`])(`{3,}|~{3,})([^\n`]*)$/

function looksLikeMarkdownBoundary(line: string): boolean {
  return MARKDOWN_BOUNDARY_RE.test(line.trimStart())
}

function startsWithCjkProse(line: string): boolean {
  return /^(?:\*\*|__)?[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]/.test(line.trimStart())
}

function looksLikePlainMarkdownAfterCode(line: string): boolean {
  const trimmed = line.trim()
  if (!trimmed) return false
  if (looksLikeMarkdownBoundary(line)) return true
  if (CODE_LIKE_START_RE.test(trimmed)) return false
  if (CODE_KEYWORD_RE.test(trimmed)) return false
  if (CODE_ASSIGNMENT_RE.test(trimmed)) return false
  // CJK body text is almost never a code open; treat as prose.
  if (/[\u4e00-\u9fff]/.test(trimmed)) return true
  // English sentence/label prose: capitalized, contains whitespace, long enough
  // that it is unlikely to be a single code token (allows `=`, quotes, etc.).
  if (/^[A-Z]/.test(trimmed) && /\s/.test(trimmed) && trimmed.length >= 12) return true
  return false
}

function isFenceLine(line: string, marker?: string, minLength?: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return false
  if (marker && match[2]![0] !== marker) return false
  if (minLength !== undefined && match[2]!.length < minLength) return false
  return true
}

function parseFenceLine(line: string): { indent: string, marker: string, rest: string } | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  return { indent: match[1]!, marker: match[2]!, rest: match[3]! }
}

function fenceLanguageFromLine(line: string): string | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  const info = match[3]!.trim()
  return /^([A-Za-z0-9_+.#-]+)\s*$/.exec(info)?.[1] ?? null
}

function isLikelyFenceClose(line: string, marker: string, minLength: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  return !!match && match[2]![0] === marker && match[2]!.length >= minLength
}

function looksLikeStructuredCode(lines: string[], lang: string | null): boolean {
  const normalizedLang = lang?.toLowerCase()
  if (
    normalizedLang
    && /^(json|jsonc|javascript|js|typescript|ts|tsx|jsx|sql|python|py|rust|rs|go|java|c|cpp|c\+\+|csharp|cs|bash|sh|zsh|yaml|yml|toml|xml|html|css|diff)$/.test(
      normalizedLang,
    )
  ) {
    return true
  }

  const content = lines.join('\n').trim()
  if (!content) return false
  if (/^[\[{]/.test(content)) return true
  if (SQL_KEYWORD_RE.test(content)) return true
  if (/^(import|export|const|let|var|function|class|def|async|type|interface)\b/.test(content)) return true
  return false
}

function countStructuralBalance(lines: string[]): number {
  let balance = 0
  let inString: string | null = null
  let escaped = false
  for (const ch of lines.join('\n')) {
    if (inString) {
      if (escaped) {
        escaped = false
      } else if (ch === '\\') {
        escaped = true
      } else if (ch === inString) {
        inString = null
      }
      continue
    }
    if (ch === '"' || ch === "'") {
      inString = ch
    } else if (ch === '{' || ch === '[' || ch === '(') {
      balance++
    } else if (ch === '}' || ch === ']' || ch === ')') {
      balance--
    }
  }
  return balance
}

function looksLikeCodeCompleted(lines: string[], lang: string | null): boolean {
  const nonBlank = lines.filter(line => line.trim().length > 0)
  if (nonBlank.length === 0) return false
  const last = nonBlank[nonBlank.length - 1]!.trim()
  if (/^[}\]\);,]*$/.test(last)) return countStructuralBalance(nonBlank) <= 0
  if (lang?.toLowerCase() === 'sql' && /;$/.test(last)) return true
  return false
}

function isShellFenceLanguage(lang: string | null): boolean {
  return !!lang && /^(bash|sh|zsh|shell|fish|nu|nushell)$/i.test(lang)
}

/** Line looks like a source comment / continuation, not markdown prose. */
function looksLikeCodeCommentOrContinuation(line: string, lang: string | null): boolean {
  const trimmed = line.trim()
  if (!trimmed) return false
  // Common line comments across languages (SQL `--`, C-like `//`, shell/python `#`
  // that is not an ATX heading).
  if (/^(?:--|\/\/|\/\*|\*(?:\s|$)|#(?!#{0,5}\s|#))/.test(trimmed)) return true
  // SQL / pseudo-code continuation keywords after a prior statement.
  if (/^(AND|OR|JOIN|LEFT|RIGHT|INNER|OUTER|WHERE|GROUP|ORDER|LIMIT|HAVING|UNION|ELSE|ELIF|THEN|DO|DONE|FI|THEN)\b/i.test(trimmed)) {
    return true
  }
  if (lang && /^(sql)$/i.test(lang) && /;\s*(?:--.*)?$/.test(trimmed)) return true
  return false
}

function shouldCloseShellFenceBeforeProse(line: string, codeLines: string[], lang: string | null): boolean {
  if (!isShellFenceLanguage(lang)) return false
  if (codeLines.length === 0 || codeLines[codeLines.length - 1]!.trim() !== '') return false
  if (looksLikeCodeCommentOrContinuation(line, lang)) return false
  // Only close before a strong markdown boundary, never mid-script on prose.
  return looksLikeMarkdownBoundary(line)
}

function shouldCloseOpenFenceBeforeLine(line: string, codeLines: string[], lang: string | null): boolean {
  if (shouldCloseShellFenceBeforeProse(line, codeLines, lang)) return true
  // Plaintext fences keep literal markdown (e.g. `**not markdown**`) inside the
  // fence — never auto-close them based on following prose.
  if (!looksLikeStructuredCode(codeLines, lang)) return false
  // Never close before a comment / language continuation — multi-statement SQL
  // with `-- CJK comments` after `;` is the production regression that used to
  // tear the fence open and swallow the rest of the document.
  if (looksLikeCodeCommentOrContinuation(line, lang)) return false
  // Only strong markdown boundaries (heading / list / table / hr / quote).
  // Plain CJK/English prose is NOT enough: it false-triggers on SQL comments
  // and mid-block narrative lines inside intentional fences.
  if (!looksLikeMarkdownBoundary(line)) return false
  return looksLikeCodeCompleted(codeLines, lang)
}

/**
 * Bare fence with no info string that looks like a leftover close rather than
 * a real open: the following non-blank content is markdown/prose and no
 * matching close appears later. Dropping it prevents the rest of the document
 * from being swallowed as code (common at overflow chunk boundaries).
 *
 * If a matching close does appear later, this is a real open fence even when
 * the body starts with prose-like text.
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
  const rest = parsed.rest
    .trimStart()
    .replace(/^(#{2,6})(?=[^#\s])/, '$1 ')
    .replace(/^(#)(?=[^\x00-\x7f])/, '$1 ')
  if (!rest) return null
  if (
    !looksLikeMarkdownBoundary(rest)
    && !looksLikePlainMarkdownAfterCode(rest)
    && !/^[A-Z][A-Za-z\s]+\./.test(rest)
  ) {
    return null
  }
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
  const normalizedRest = rest
    .replace(/^(#{2,6})(?=[^#\s])/, '$1 ')
    .replace(/^(#)(?=[^\x00-\x7f])/, '$1 ')
  if (
    !looksLikeMarkdownBoundary(normalizedRest)
    && !looksLikePlainMarkdownAfterCode(normalizedRest)
    && !/^[A-Z][A-Za-z\s]+\./.test(normalizedRest)
  ) {
    return null
  }
  return [content.trimEnd(), fence, normalizedRest]
}

function splitSingleLineFence(line: string): string[] | null {
  const match = /^( {0,3})(`{3,}|~{3,})([^`~\n]*?)(`{3,}|~{3,})[ \t]*$/.exec(line)
  if (!match) return null
  const indent = match[1]!
  const open = match[2]!
  const content = match[3]!.trim()
  const close = match[4]!
  if (!content) return null
  if (close[0] !== open[0] || close.length < open.length) return null
  // Language-only tags like ```json``` are not single-line code.
  if (/^[A-Za-z0-9_+.#-]+$/.test(content)) return null
  return [`${indent}${open}`, content, close]
}

/**
 * Split opening fences that models glued onto the end of a heading/prose line,
 * and single-line ```code``` fences into open/body/close.
 */
export function splitGluedFenceOpens(text: string): string {
  const lines = text.split('\n')
  let inFence = false
  let fenceMarker = ''
  const out: string[] = []

  for (const line of lines) {
    if (!inFence) {
      const singleLineFence = splitSingleLineFence(line)
      if (singleLineFence) {
        out.push(...singleLineFence)
        continue
      }
    }

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
      out.push(line)
      continue
    }

    if (inFence) {
      // Closing fence glued to end of a content line.
      const escChar = fenceMarker[0]!.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const closeGlued = new RegExp(
        `^(.+[^\\s${escChar}])(${escChar}{${fenceMarker.length},})[ \\t]*$`,
      )
      const cm = closeGlued.exec(line)
      if (cm) {
        out.push(cm[1]!)
        out.push(cm[2]!)
        inFence = false
        fenceMarker = ''
        continue
      }
      out.push(line)
      continue
    }

    const m = FENCE_OPEN_GLUED_RE.exec(line)
    if (m) {
      const lead = m[1]!
      const marker = m[2]!
      const info = m[3] ?? ''
      const infoTrim = info.trim()
      if (infoTrim === '' || /^[A-Za-z0-9_+.#-]+$/.test(infoTrim)) {
        out.push(lead.trimEnd(), `${marker}${infoTrim}`)
        inFence = true
        fenceMarker = marker
        continue
      }
    }
    out.push(line)
  }

  return out.join('\n')
}

/**
 * Drop stray bare fences, split glued closes, and auto-close completed
 * structured fences before following markdown prose.
 */
export function repairUnclosedFences(content: string, finalClose: boolean): string {
  const lines = content.split('\n')
  let out = ''
  let openMarker = ''
  let openLength = 0
  let openClose = ''
  let openLang: string | null = null
  let codeLines: string[] = []

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    const newline = i < lines.length - 1 ? '\n' : ''
    const match = CODE_FENCE_RE.exec(line)

    if (!openMarker) {
      if (match) {
        if (shouldTreatAsStrayFenceClose(lines, i)) continue
        openMarker = match[2]![0]!
        openLength = match[2]!.length
        openClose = openMarker.repeat(openLength)
        openLang = fenceLanguageFromLine(line)
        codeLines = []
        out += line + newline
        continue
      }
      out += line + newline
      continue
    }

    const gluedClose = splitGluedMarkdownAfterFenceClose(line, openMarker, openLength)
    if (gluedClose) {
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
      out += `${gluedClose[0]}\n${gluedClose[1]}${newline}`
      continue
    }

    const trailingClose = splitTrailingFenceClose(line, openMarker, openLength)
    if (trailingClose) {
      codeLines.push(trailingClose[0]!)
      out += trailingClose[2] !== undefined
        ? `${trailingClose[0]}\n${trailingClose[1]}\n${trailingClose[2]}${newline}`
        : `${trailingClose[0]}\n${trailingClose[1]}${newline}`
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
      continue
    }

    if (isLikelyFenceClose(line, openMarker, openLength)) {
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
      out += line + newline
      continue
    }

    if (!isFenceLine(line) && shouldCloseOpenFenceBeforeLine(line, codeLines, openLang)) {
      out += `${openClose}\n`
      openMarker = ''
      openLength = 0
      openClose = ''
      openLang = null
      codeLines = []
    }

    out += line + newline
    if (openMarker) {
      codeLines.push(line)
    }
  }

  if (finalClose && openMarker) {
    out += out.endsWith('\n') ? openClose : `\n${openClose}`
  }
  return out
}

/**
 * Prepare markdown for lexing by repairing only fence-boundary failures.
 */
export function prepareMarkdownFences(text: string): string {
  return repairUnclosedFences(splitGluedFenceOpens(text), true)
}

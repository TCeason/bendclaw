/**
 * Pure fence-boundary signals.
 *
 * These helpers answer "what does this line look like?" only.
 * Whether to close / drop / split a fence lives in cases.ts.
 */

import { CODE_FENCE_RE } from '../../primitives.js'

export const MARKDOWN_BOUNDARY_RE =
  /^([ \t]{0,3})?(#{1,6}(?:\s|$)|#{2,6}(?=[^#\s])|(?:\*\*|__)|(?:[-*+]\s)|(?:\d+\.\s)|>\s|\|.*\||-{3,}\s*$)/
const SQL_KEYWORD_RE = /^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP|SET|MERGE|TRUNCATE)\b/i
const CODE_LIKE_START_RE = /^[\[{(}\]),;]|^\/\/|^#\s*include\b/
const CODE_KEYWORD_RE =
  /^(return|if|else|for|while|switch|case|break|continue|try|catch|finally|throw|await|async|const|let|var|function|class|def|import|export|from|SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|WHERE|ORDER|GROUP|LIMIT|DROP|SET|ALTER|MERGE|TRUNCATE)\b/i
const CODE_ASSIGNMENT_RE = /^[\w$.'"`-]+\s*[:=]/

/** Opening fence glued to the end of a non-empty lead (not at line start). */
export const FENCE_OPEN_GLUED_RE = /^([^\n`]*?[^\s`])(`{3,}|~{3,})([^\n`]*)$/

export function looksLikeMarkdownBoundary(line: string): boolean {
  return MARKDOWN_BOUNDARY_RE.test(line.trimStart())
}

export function startsWithCjkProse(line: string): boolean {
  return /^(?:\*\*|__)?[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]/.test(line.trimStart())
}

export function looksLikePlainMarkdownAfterCode(line: string): boolean {
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

export function isFenceLine(line: string, marker?: string, minLength?: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return false
  if (marker && match[2]![0] !== marker) return false
  if (minLength !== undefined && match[2]!.length < minLength) return false
  return true
}

export function parseFenceLine(line: string): { indent: string, marker: string, rest: string } | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  return { indent: match[1]!, marker: match[2]!, rest: match[3]! }
}

export function fenceLanguageFromLine(line: string): string | null {
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return null
  const info = match[3]!.trim()
  return /^([A-Za-z0-9_+.#-]+)\s*$/.exec(info)?.[1] ?? null
}

export function isLikelyFenceClose(line: string, marker: string, minLength: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  return !!match && match[2]![0] === marker && match[2]!.length >= minLength
}

export function looksLikeStructuredCode(lines: string[], lang: string | null): boolean {
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

export function countStructuralBalance(lines: string[]): number {
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

export function looksLikeCodeCompleted(lines: string[], lang: string | null): boolean {
  const nonBlank = lines.filter(line => line.trim().length > 0)
  if (nonBlank.length === 0) return false
  const last = nonBlank[nonBlank.length - 1]!.trim()
  if (/^[}\]\);,]*$/.test(last)) return countStructuralBalance(nonBlank) <= 0
  if (lang?.toLowerCase() === 'sql' && /;$/.test(last)) return true
  return false
}

export function isShellFenceLanguage(lang: string | null): boolean {
  return !!lang && /^(bash|sh|zsh|shell|fish|nu|nushell)$/i.test(lang)
}

/** Line looks like a source comment / continuation, not markdown prose. */
export function looksLikeCodeCommentOrContinuation(line: string, lang: string | null): boolean {
  const trimmed = line.trim()
  if (!trimmed) return false
  // Shell `# foo` is a comment, not ATX H1; `##` still closes the fence.
  if (isShellFenceLanguage(lang) && /^#(?!#)/.test(trimmed)) return true
  // SQL `--`, C-like `//`, python `#` that is not an ATX heading.
  if (/^(?:--|\/\/|\/\*|\*(?:\s|$)|#(?!#{0,5}\s|#))/.test(trimmed)) return true
  // SQL / pseudo-code continuation keywords after a prior statement.
  if (/^(AND|OR|JOIN|LEFT|RIGHT|INNER|OUTER|WHERE|GROUP|ORDER|LIMIT|HAVING|UNION|ELSE|ELIF|THEN|DO|DONE|FI|THEN)\b/i.test(trimmed)) {
    return true
  }
  if (lang && /^(sql)$/i.test(lang) && /;\s*(?:--.*)?$/.test(trimmed)) return true
  return false
}

export function normalizeGluedHeadingRest(rest: string): string {
  return rest
    .trimStart()
    .replace(/^(#{2,6})(?=[^#\s])/, '$1 ')
    .replace(/^(#)(?=[^\x00-\x7f])/, '$1 ')
}

export function looksLikeMarkdownAfterFenceRest(rest: string): boolean {
  const normalized = normalizeGluedHeadingRest(rest)
  return looksLikeMarkdownBoundary(normalized)
    || looksLikePlainMarkdownAfterCode(normalized)
    || /^[A-Z][A-Za-z\s]+\./.test(normalized)
}

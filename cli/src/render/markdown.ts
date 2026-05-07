/**
 * Markdown rendering for terminal output.
 * Uses marked lexer + chalk + cli-highlight for proper code blocks, tables, etc.
 * Approach modeled after Claude Code's formatToken.
 */

import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import wrapAnsi from 'wrap-ansi'
import { createHyperlink, isWarpTerminal, supportsHyperlinks, wrapHyperlink } from './hyperlink.js'
import { linkifyIssueRefs } from './linkify.js'
import { getTheme, type Theme } from './theme.js'

let highlighter: typeof import('cli-highlight') | null = null
try {
  highlighter = await import('cli-highlight')
} catch {
  // cli-highlight not available — code blocks render without syntax highlighting
}

let markedConfigured = false

export function configureMarked(): void {
  if (markedConfigured) return
  markedConfigured = true

  // Disable strikethrough parsing — the model often uses ~ for "approximate"
  // (e.g., ~100) and rarely intends actual strikethrough formatting.
  marked.use({
    tokenizer: {
      del() {
        return undefined as unknown as Tokens.Del
      },
    },
  })
}

// ---------------------------------------------------------------------------
// Markdown syntax fast-path detection
// ---------------------------------------------------------------------------

// Characters/patterns that indicate markdown syntax. If none are present,
// skip the marked.lexer call entirely — render as a single paragraph.
// Covers the majority of short assistant responses that are plain sentences.
// Ordered-list pattern requires `N. ` (digit + dot + space) to avoid
// misinterpreting bare "2." as a list item.
const MD_SYNTAX_RE = /[#*`|[>\-_~]|\n\n|^\d+\. |\n\d+\. /

function hasMarkdownSyntax(s: string): boolean {
  return MD_SYNTAX_RE.test(s)
}

/** Build a plain-text paragraph token (no marked.lexer overhead). */
function plainTextTokens(content: string): Token[] {
  return [{
    type: 'paragraph',
    raw: content,
    text: content,
    tokens: [{ type: 'text', raw: content, text: content }],
  } as Token]
}

const EOL = '\n'
const SAFETY_MARGIN = 4
const MAX_TABLE_ROW_LINES = 4
const MAX_RENDER_WIDTH = 140
const CODE_FENCE_RE = /^( {0,3})(`{3,}|~{3,})(.*)$/
// Heading matches `#{1,6}` followed by either whitespace/EOL (classic ATX)
// OR a non-hash, non-space character (glued form we still want to recognise,
// e.g. `##改进清单`). Only classic ATX (`#{2,6}<non-space>`) is permitted in
// the glued case so `#include`/`#1` don't collide.
const MARKDOWN_BOUNDARY_RE = /^(#{1,6}(?:\s|$)|#{2,6}(?=[^#\s])|(?:[-*+]\s)|(?:\d+\.\s)|>\s|\|.*\||-{3,}\s*$)/
const CODE_LIKE_START_RE = /^[\[{(}\]),;]|^\/\/|^#\s*include\b/
const CODE_KEYWORD_RE = /^(return|if|else|for|while|switch|case|break|continue|try|catch|finally|throw|await|async|const|let|var|function|class|def|import|export|from|SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|WHERE|ORDER|GROUP|LIMIT)\b/i
const CODE_ASSIGNMENT_RE = /^[\w$.'"`-]+\s*[:=]/
// Box-drawing characters used in tree/diagram structures (U+2500–U+257F)
const BOX_DRAWING_RE = /[\u2500-\u257f]/

function terminalDisplayWidth(text: string): number {
  return stringWidth(stripAnsi(text))
}

function terminalContentWidth(): number {
  const columns = process.stdout.columns ?? 80
  return Math.max(20, Math.min(columns - SAFETY_MARGIN, MAX_RENDER_WIDTH))
}

/** Terminal width for tables — no MAX_RENDER_WIDTH cap so wide tables
 *  can use the full terminal on large screens. */
function terminalTableWidth(): number {
  const columns = process.stdout.columns ?? 80
  return Math.max(20, columns - SAFETY_MARGIN)
}

function wrapDisplayTextWithIndent(
  text: string,
  firstIndent: string,
  restIndent: string,
  width = terminalContentWidth(),
): string {
  const innerWidth = Math.max(1, width - terminalDisplayWidth(firstIndent))
  return text
    .split(EOL)
    .flatMap(line => {
      if (!line || BOX_DRAWING_RE.test(stripAnsi(line))) return [line]
      return wrapAnsi(line, innerWidth, { hard: true, trim: false, wordWrap: true }).split('\n')
    })
    .map((line, index) => `${index === 0 ? firstIndent : restIndent}${line}`)
    .join(EOL)
}

/**
 * Soft-wrap a paragraph to fit the terminal width. Skips lines that contain
 * Unicode box-drawing characters — those are structural tree/diagram art and
 * must not be reflowed. Compare with claudecode, which never wraps inside
 * formatToken and relies on Ink/Yoga for layout; we wrap here because the CLI
 * writes ANSI strings directly.
 */
function wrapParagraph(text: string, width = terminalContentWidth()): string {
  return text
    .split(EOL)
    .flatMap(line => {
      if (!line || BOX_DRAWING_RE.test(stripAnsi(line))) return [line]
      if (terminalDisplayWidth(line) <= width) return [line]
      return wrapAnsi(line, width, { hard: true, trim: false, wordWrap: true }).split('\n')
    })
    .join(EOL)
}

function looksLikeMarkdownBoundary(line: string): boolean {
  return MARKDOWN_BOUNDARY_RE.test(line.trimStart())
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

function splitGluedMarkdownAfterFenceClose(line: string, marker: string, minLength: number): string[] | null {
  const parsed = parseFenceLine(line)
  if (!parsed) return null
  if (parsed.marker[0] !== marker || parsed.marker.length < minLength) return null

  // Normalize heading markers like `##改进清单` (and `#改进清单` when the
  // body is non-ASCII) so we look them up against the proper CommonMark
  // shape (`## 改进清单`).
  const rest = parsed.rest
    .trimStart()
    .replace(/^(#{2,6})(?=[^#\s])/, '$1 ')
    .replace(/^(#)(?=[^\x00-\x7f])/, '$1 ')
  if (!rest) return null
  if (!looksLikeMarkdownBoundary(rest) && !looksLikePlainMarkdownAfterCode(rest)) return null
  return [`${parsed.indent}${parsed.marker}`, rest]
}

/**
 * Detect a content line with a trailing fence marker glued to it — e.g.
 * `    }\`\`\`` inside an open JSON fence. Models occasionally emit the
 * closing fence without the required newline, which leaks literal backticks
 * into the rendered output. We split it into the content line plus a
 * standalone fence-close line so marked sees a proper code block.
 *
 * Only triggers when the marker at the end matches the currently open fence
 * (same char and length >= open length) so unrelated prose backticks stay
 * untouched.
 */
function splitTrailingFenceClose(line: string, marker: string, minLength: number): string[] | null {
  // If the whole line is already a fence, nothing to split.
  if (CODE_FENCE_RE.test(line)) return null
  const suffix = new RegExp(`^(.*?)(${marker === '`' ? '`' : '~'}{${minLength},})[ \\t]*$`)
  const match = suffix.exec(line)
  if (!match) return null
  const content = match[1]!
  const fence = match[2]!
  // The content must not be empty (otherwise it's just a fence) and must
  // not itself contain the fence marker — keeps inline backticks alone.
  if (!content.trim()) return null
  if (content.includes(marker.repeat(minLength))) return null
  return [content.trimEnd(), fence]
}

function isLikelyFenceClose(line: string, marker: string, minLength: number): boolean {
  const match = CODE_FENCE_RE.exec(line)
  return !!match && match[2]![0] === marker && match[2]!.length >= minLength
}

function looksLikeStructuredCode(lines: string[], lang: string | null): boolean {
  const normalizedLang = lang?.toLowerCase()
  if (normalizedLang && /^(json|jsonc|javascript|js|typescript|ts|tsx|jsx|sql|python|py|rust|rs|go|java|c|cpp|c\+\+|csharp|cs|bash|sh|zsh|yaml|yml|toml|xml|html|css|diff)$/.test(normalizedLang)) {
    return true
  }

  const content = lines.join('\n').trim()
  if (!content) return false
  if (/^[\[{]/.test(content)) return true
  if (/^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP)\b/i.test(content)) return true
  if (/^(import|export|const|let|var|function|class|def|async|type|interface)\b/.test(content)) return true
  return false
}

function looksLikePlainMarkdownAfterCode(line: string): boolean {
  const trimmed = line.trim()
  if (!trimmed) return false
  if (looksLikeMarkdownBoundary(line)) return true
  if (CODE_LIKE_START_RE.test(trimmed)) return false
  if (CODE_KEYWORD_RE.test(trimmed)) return false
  if (CODE_ASSIGNMENT_RE.test(trimmed)) return false
  return /[\u4e00-\u9fff]/.test(trimmed) || /^[A-Z][\w\s,;:()/-]{12,}$/.test(trimmed)
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

function shouldCloseOpenFenceBeforeLine(line: string, codeLines: string[], lang: string | null): boolean {
  if (!looksLikeStructuredCode(codeLines, lang)) return false
  if (looksLikeMarkdownBoundary(line)) return looksLikeCodeCompleted(codeLines, lang)
  if (!looksLikeCodeCompleted(codeLines, lang)) return false
  return looksLikePlainMarkdownAfterCode(line)
}

function repairUnclosedFences(content: string, finalClose: boolean): string {
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
        openMarker = match[2]![0]!
        openLength = match[2]!.length
        openClose = openMarker.repeat(openLength)
        openLang = fenceLanguageFromLine(line)
        codeLines = []
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
      // Emit the content line (still inside the fence) then the standalone
      // fence-close line, and mark the fence as closed.
      codeLines.push(trailingClose[0]!)
      out += `${trailingClose[0]}\n${trailingClose[1]}${newline}`
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

// Matches a line that contains only a thematic-break marker (---, ***, ___).
// Models often write `foo.\n---\n### heading` without the surrounding blank
// lines CommonMark requires, and marked then swallows `---` as a setext h2
// underline, collapsing the separator and the next heading visually.
// Insert blank lines before/after isolated markers so they're recognized as hr.
const HR_MARKER_INLINE_RE = /([^\n])\n([ \t]*(?:-{3,}|\*{3,}|_{3,})[ \t]*)\n(?!\n)/g
const HR_MARKER_TRAILING_RE = /([^\n])\n([ \t]*(?:-{3,}|\*{3,}|_{3,})[ \t]*)$/g
// Matches a thematic-break marker glued directly to the end of a sentence
// with no intervening space, e.g. `通用框架。---\n核心抽象`. Only trigger
// after strong sentence terminators (CJK or ASCII punctuation that ends a
// clause) so we don't mangle em-dash usage like `foo --- bar`.
const HR_MARKER_GLUED_RE = /([。．！？!?:：])([ \t]*)(-{3,}|\*{3,}|_{3,})[ \t]*(\n|$)/g
const HR_MARKER_BEFORE_HEADING_RE = /(^|\n)([ \t]*(?:-{3,}|\*{3,}|_{3,}))[ \t]*(#{1,6}\s)/g

// Heading marker glued directly to its body with no space, e.g. `##改进清单`.
// CommonMark requires `## text`, but models routinely omit the space when
// writing CJK content. We apply two independent rules:
//   • 2–6 hashes followed by any non-hash/non-space → always normalise
//   • 1 hash followed by a non-ASCII letter → normalise `#改进清单` but leave
//     `#1` issue refs and `#include` preprocessor directives alone
const HEADING_MISSING_SPACE_MANY_RE = /^([ \t]{0,3})(#{2,6})(?=[^#\s])/gm
const HEADING_MISSING_SPACE_ONE_RE = /^([ \t]{0,3})(#)(?=[^\x00-\x7f])/gm
// Heading marker glued to the end of a preceding paragraph, e.g.
// `…零语义风险） ##粘连`. Split it onto its own line before the lexer runs
// so marked recognises it as a heading. Trigger only after a sentence-ending
// punctuation (ASCII or CJK) and require at least 2 hashes with CJK/letter
// body so we don't rewrite `x ## y` in technical discussions or `x: #1`.
const HEADING_GLUED_AFTER_TEXT_RE =
  /([。．！？?!：）】」』》])[ \t]+(#{2,6}(?:\s|$|[^\s#]))/g
// Zero-whitespace variant: the punctuation or a CJK character is glued
// directly to `##…##` with no space in between, e.g. `。###档 1`,
// `这个###档 1`, or `。### 一句话总结` (where only the preceding side is
// glued). We require the heading body to start with a non-hash character so
// the regex still terminates at the correct heading depth; the existing
// HEADING_MISSING_SPACE_* rules then add a space when the body itself is
// glued (`###档` → `### 档`).
const HEADING_GLUED_NO_SPACE_RE =
  /([。．！？?!：）】」』》\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff])(#{2,6})(?=[^#])/g
// Unordered-list marker glued directly to a CJK body, e.g. `-summary 的详略`
// or `*这个改动`. CommonMark requires a space after `-`/`*`/`+`; models
// routinely drop it in CJK contexts. We only rewrite when the body starts
// with a non-ASCII character so that `-1` (negatives), `--flag` (CLI flags),
// `---` (HR) and `*emphasis*` all stay untouched.
const BULLET_MISSING_SPACE_RE = /^([ \t]{0,3})([-*+])(?=[^\s\-*+\x00-\x7f])/gm
// Ordered-list marker glued directly to a non-ASCII body, e.g. `3.多指标`.
// Only rewrite when the body starts with a non-ASCII character so decimals
// (`3.14`), version strings (`v1.2.3`), and IPs (`192.168`) stay untouched.
const ORDERED_MISSING_SPACE_RE = /^([ \t]{0,3})(\d{1,9}[.)])(?=[^\s\x00-\x7f])/gm
// Bullet marker glued to the end of a prose line after a colon, e.g.
// `分歧：- 切片：只有 Phoenix…` or `分歧：-切片`. Split so the bullet starts a
// real list. Trigger only when a bullet marker immediately follows a colon
// with real prose to the left; `$1` captures the preceding character so
// negatives like `key:-1` (digit body) and plain text don't match. The
// body after the bullet must start with whitespace or a non-ASCII character
// so CLI flags (`--foo`) and HR runs (`---`) stay untouched.
const BULLET_GLUED_AFTER_COLON_RE =
  /([^\s:：])([：:])[ \t]*([-*+])[ \t]*(?=[^\s\-*+\x00-\x7f]|[ \t])/g
// Ordered marker glued to the end of a prose line after a colon, e.g.
// `共识：1. 必须有…`. Same treatment as the bullet variant above.
// The lookahead `[ \t]+\D` is essential: require whitespace + non-digit
// after the period so decimals like `task_1: 0.8` (colon + `0.` + digit)
// stay intact. A real ordered-list item always has a space + non-digit
// body after its number.
const ORDERED_GLUED_AFTER_COLON_RE =
  /([^\s:：])([：:])[ \t]*(\d{1,9}[.)])(?=[ \t]+\D)[ \t]*/g
// Ordered list item whose indentation is ≥4 spaces: CommonMark treats this
// as either a code block or a lazy continuation of the previous paragraph,
// so the item silently merges with whatever came before. Models routinely
// over-indent mid-list items in CJK contexts (`     3.多指标…`). Normalise
// to at most 3 leading spaces before the other rules run.
const ORDERED_OVER_INDENT_RE = /^[ \t]{4,}(?=\d{1,9}[.)][\s\u3400-\u4dbf\u4e00-\u9fff])/gm

function normalizeHrLines(text: string): string {
  return text
    .replace(HR_MARKER_BEFORE_HEADING_RE, '$1$2\n\n$3')
    .replace(HR_MARKER_GLUED_RE, '$1\n\n$3\n\n')
    .replace(HR_MARKER_INLINE_RE, '$1\n\n$2\n\n')
    .replace(HR_MARKER_TRAILING_RE, '$1\n\n$2')
}

/**
 * Normalize ATX headings that are glued to their body or to preceding prose.
 * Walks lines so we never touch content inside a fenced code block.
 *
 * Handles three common model outputs:
 *   `##改进清单（共 8 项）`    → `## 改进清单（共 8 项）`
 *   `#改进清单`                 → `# 改进清单`   (single-# only when body is non-ASCII)
 *   `…零语义风险） ##粘连`     → `…零语义风险）\n\n## 粘连`
 *
 * For 2–6 `#`s we're permissive (unambiguous heading intent). For a single
 * `#` we only fix glue when the body starts with a non-ASCII letter so
 * `#1` issue refs and `#include` preprocessor directives stay untouched.
 */
function normalizeGluedHeadings(text: string): string {
  const lines = text.split('\n')
  let inFence = false
  let fenceMarker = ''
  const out: string[] = []
  for (const line of lines) {
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
      out.push(line)
      continue
    }
    // Split glued `…） ##text` into two lines first, then ensure every
    // heading line has a space after its marker. Also split the no-space
    // variants like `。###档` / `这个###档` where the heading marker sits
    // flush against the preceding CJK character or punctuation. Finally
    // normalize bullet markers glued to CJK bodies (`-这个` → `- 这个`)
    // and split list markers glued to the end of a prose line after a
    // colon (`分歧：- 切片` / `共识：1. 必须…`).
    const split = line
      .replace(HEADING_GLUED_AFTER_TEXT_RE, '$1\n\n$2')
      .replace(HEADING_GLUED_NO_SPACE_RE, '$1\n\n$2 ')
      .replace(BULLET_GLUED_AFTER_COLON_RE, '$1$2\n\n$3 ')
      .replace(ORDERED_GLUED_AFTER_COLON_RE, '$1$2\n\n$3 ')
      .replace(ORDERED_OVER_INDENT_RE, '   ')
      .split('\n')
    for (const s of split) {
      out.push(
        s
          .replace(HEADING_MISSING_SPACE_MANY_RE, '$1$2 ')
          .replace(HEADING_MISSING_SPACE_ONE_RE, '$1$2 ')
          .replace(BULLET_MISSING_SPACE_RE, '$1$2 ')
          .replace(ORDERED_MISSING_SPACE_RE, '$1$2 '),
      )
    }
  }
  return out.join('\n')
}

// Markdown table separator line — exclude from box-drawing preservation.
const MD_TABLE_SEP_RE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$/

/**
 * Preserve any paragraph that contains Unicode box-drawing characters
 * (U+2500–U+257F) by wrapping it in a fenced code block. This delegates
 * whitespace preservation to marked's code-block handling instead of
 * trying to identify specific shapes (hand-drawn boxes, tree listings,
 * ASCII tables, …) with fragile per-shape regexes.
 *
 * Why this works: marked emits paragraphs by trimming and joining lines
 * with spaces, so multi-space indentation (`│   ├── foo`) collapses and
 * tree/box columns go out of alignment. A code block keeps every line
 * verbatim. GFM tables use ASCII `|` and do not contain box-drawing
 * characters, so they are skipped here and rendered by marked's table
 * tokenizer.
 */
function preserveBoxDrawingBlocks(text: string): string {
  if (!BOX_DRAWING_RE.test(text)) return text
  const lines = text.split('\n')
  const out: string[] = []
  let inFence = false
  let fenceMarker = ''
  let i = 0
  while (i < lines.length) {
    const line = lines[i]!
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
      i++
      continue
    }
    if (inFence || line.trim() === '') {
      out.push(line)
      i++
      continue
    }

    // Collect a paragraph = contiguous non-empty, non-fence lines.
    let j = i
    while (
      j < lines.length
      && lines[j]!.trim() !== ''
      && !CODE_FENCE_RE.test(lines[j]!)
    ) {
      j++
    }
    const block = lines.slice(i, j)
    const hasBoxDrawing = block.some(l => BOX_DRAWING_RE.test(l))
    const isMdTable = block.some(l => MD_TABLE_SEP_RE.test(l))
    if (hasBoxDrawing && !isMdTable) {
      out.push('```text')
      out.push(...block)
      out.push('```')
    } else {
      out.push(...block)
    }
    i = j
  }
  return out.join('\n')
}

// Model prompts occasionally carry reminder tags that leak into visible text
// when normalization breaks. Strip them before the lexer sees the input so
// the renderer never displays `<system-reminder>…</system-reminder>` blocks
// verbatim. Matches claudecode's stripPromptXMLTags behavior.
const STRIPPED_PROMPT_TAGS_RE =
  /<(system-reminder|commit_analysis|context|function_analysis|pr_analysis)>[\s\S]*?<\/\1>\n?/g

function stripPromptXMLTags(content: string): string {
  return content.replace(STRIPPED_PROMPT_TAGS_RE, '')
}

function prepareMarkdownForLex(text: string): string {
  return repairUnclosedFences(
    normalizeHrLines(preserveBoxDrawingBlocks(normalizeGluedHeadings(text))),
    true,
  )
}

// ---------------------------------------------------------------------------
// CJK ↔ Latin/digit spacing ("pangu")
// ---------------------------------------------------------------------------
//
// Mixed CJK/Latin prose without interstitial whitespace is hard to read
// (`一条trace` looks glued) and starves word-wrap of good break points. We
// insert a regular space between adjacent CJK characters and ASCII
// letters/digits — the classic pangu-style rule. Only applied to plain-text
// leaves, so inline code, links and URLs are preserved verbatim.
//
// Covers CJK Unified (U+4E00–U+9FFF), CJK Extension A (U+3400–U+4DBF),
// CJK Compat (U+F900–U+FAFF), Hiragana (U+3040–U+309F), Katakana
// (U+30A0–U+30FF). Intentionally skipped: CJK punctuation (`，。：`) — those
// already act as visual separators and double-spacing would look wrong.
const CJK_CHAR = '\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff'
const PANGU_CJK_THEN_LATIN_RE = new RegExp(`([${CJK_CHAR}])([A-Za-z0-9])`, 'g')
const PANGU_LATIN_THEN_CJK_RE = new RegExp(`([A-Za-z0-9])([${CJK_CHAR}])`, 'g')

function applyPangu(text: string): string {
  return text
    .replace(PANGU_CJK_THEN_LATIN_RE, '$1 $2')
    .replace(PANGU_LATIN_THEN_CJK_RE, '$1 $2')
}

export function formatToken(
  token: Token,
  listDepth = 0,
  orderedListNumber: number | null = null,
  parent: Token | null = null,
  theme: Theme = getTheme(),
): string {
  switch (token.type) {
    case 'blockquote': {
      const inner = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, null, theme))
        .join('')
      const bar = theme.blockquoteBorder.paint('▎')
      return inner
        .split(EOL)
        .map(line =>
          stripAnsi(line).trim() ? `${bar} ${theme.blockquoteText.paint(line)}` : line,
        )
        .join(EOL)
    }
    case 'code': {
      const text = token.text as string
      const lang = (token as Tokens.Code).lang
      let highlighted = text
      if (highlighter && lang) {
        try {
          if (highlighter.supportsLanguage(lang)) {
            highlighted = highlighter.highlight(text, { language: lang })
          }
        } catch {
          // fallback to plain text
        }
      } else if (highlighter && !lang) {
        try {
          highlighted = highlighter.highlight(text)
        } catch {
          // fallback
        }
      }
      // Paint a left "gutter" column using a background-coloured space
      // instead of a literal `│`. This reads as a shaded left bar in the
      // terminal but copies out as a plain space, so pasted commands are
      // not polluted with U+2502 box-drawing characters.
      const bar = theme.codeBlockGutter.paint(' ')
      const withGutter = highlighted
        .split('\n')
        .map(line => `${bar} ${line}`)
        .join('\n')
      return withGutter + EOL
    }
    case 'codespan': {
      const raw = token.text as string
      const isFilePath = /^[~/][\w./_-]+$/.test(raw)
      // Warp auto-detects file paths in plain text; ANSI codes break detection.
      // Skip coloring for file paths unless hyperlinks are force-enabled.
      if (isFilePath && isWarpTerminal() && process.env.FORCE_HYPERLINK !== '1') {
        return raw
      }
      const colored = theme.codeInline.paint(raw)
      // Make absolute file paths clickable (file:// hyperlink)
      if (supportsHyperlinks() && isFilePath) {
        const resolved = raw.startsWith('~')
          ? raw.replace('~', process.env.HOME ?? '~')
          : raw
        return wrapHyperlink(`file://${resolved}`, colored)
      }
      return colored
    }
    case 'del':
      // del is disabled via configureMarked; if somehow reached, render as-is
      return ''
    case 'em':
      return theme.italic.paint(
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, parent, theme))
          .join(''),
      )
    case 'strong':
      return theme.bold.paint(
        (token.tokens ?? [])
          .map(t => formatToken(t, 0, null, parent, theme))
          .join(''),
      )
    case 'heading': {
      const text = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, null, theme))
        .join('')
      const depth = (token as Tokens.Heading).depth
      const style = depth === 1 ? theme.h1
        : depth === 2 ? theme.h2
          : depth === 3 ? theme.h3
            : depth === 4 ? theme.h4
              : depth === 5 ? theme.h5
                : theme.h6
      return style.paint(text) + EOL
    }
    case 'hr': {
      // Match claudecode: literal `---` (three dashes) instead of a full-width
      // box-drawing rule. Keeps the separator inconspicuous and avoids the
      // visual "second heading" effect on wide terminals.
      return theme.hr.paint('---') + EOL
    }
    case 'link': {
      if (token.href.startsWith('mailto:')) {
        return token.href.replace(/^mailto:/, '')
      }
      const linkText = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, token, theme))
        .join('')
      const plainText = stripAnsi(linkText)
      // If the terminal supports OSC 8 hyperlinks, render as clickable link
      if (supportsHyperlinks()) {
        if (plainText && plainText !== token.href) {
          return createHyperlink(token.href, plainText)
        }
        return createHyperlink(token.href)
      }
      // Fallback (claudecode-style): prefer the display text alone, only
      // surface the URL when there is no meaningful text. Avoids noisy
      // `text (url)` when OSC 8 is not available.
      if (plainText && plainText !== token.href) {
        return linkText
      }
      return theme.link.paint(token.href)
    }
    case 'list':
      return (token as Tokens.List).items
        .map((item: Token, index: number) =>
          formatToken(
            item,
            listDepth,
            (token as Tokens.List).ordered ? ((token as Tokens.List).start as number) + index : null,
            token,
            theme,
          ),
        )
        .join('')
    case 'list_item':
      return (token.tokens ?? [])
        .map(
          t =>
            `${'  '.repeat(listDepth)}${formatToken(t, listDepth + 1, orderedListNumber, token, theme)}`,
        )
        .join('')
    case 'paragraph': {
      const rendered = (token.tokens ?? [])
        .map(t => formatToken(t, 0, null, null, theme))
        .join('')
      // Preserve verbatim whenever the paragraph contains box-drawing
      // characters (U+2500–U+257F) — these indicate tree/diagram art whose
      // indentation must not be reflowed. Otherwise soft-wrap long lines so
      // very wide output stays readable on narrow terminals.
      if (BOX_DRAWING_RE.test(stripAnsi(rendered))) {
        return rendered + EOL
      }
      return wrapParagraph(rendered) + EOL
    }
    case 'space':
      return EOL
    case 'br':
      return EOL
    case 'text': {
      if (parent?.type === 'link') {
        return token.text
      }
      if (parent?.type === 'list_item') {
        const marker = orderedListNumber === null
          ? '-'
          : `${getListNumber(listDepth, orderedListNumber)}.`
        const depthPad = '  '.repeat(Math.max(0, listDepth - 1))
        const firstIndent = `${depthPad}${marker} `
        const restIndent = `${depthPad}${' '.repeat(terminalDisplayWidth(marker) + 1)}`
        const inner = token.tokens
          ? token.tokens.map(t => formatToken(t, listDepth, orderedListNumber, token, theme)).join('')
          : linkifyIssueRefs(applyPangu(token.text))
        return `${wrapDisplayTextWithIndent(inner, firstIndent, restIndent)}${EOL}`
      }
      if (token.tokens) {
        return token.tokens.map(t => formatToken(t, listDepth, orderedListNumber, token, theme)).join('')
      }
      // Plain text nodes: emit verbatim (claudecode-style). Do not soft-wrap
      // here — marked keeps the original newlines/indentation in token.text
      // (including tree-art and box-drawing lines), and re-wrapping here
      // collapses multi-space indentation. Apply pangu spacing so mixed
      // CJK/Latin prose ("一条trace") gets a breathable space.
      return linkifyIssueRefs(applyPangu(token.text))
    }
    case 'table': {
      const tableToken = token as Tokens.Table
      const numCols = tableToken.header.length
      const termWidth = terminalTableWidth()
      const MIN_COL = 3

      // --- helpers ---
      function renderCell(tokens: Token[] | undefined): string {
        return tokens?.map(t => formatToken(t, 0, null, null, theme)).join('').trimEnd() ?? ''
      }
      function plainText(tokens: Token[] | undefined): string {
        return stripAnsi(renderCell(tokens))
      }
      function longestWord(tokens: Token[] | undefined): number {
        const words = plainText(tokens).split(/\s+/).filter(w => w.length > 0)
        if (words.length === 0) return MIN_COL
        return Math.max(...words.map(w => terminalDisplayWidth(w)), MIN_COL)
      }
      function idealWidth(tokens: Token[] | undefined): number {
        return Math.max(terminalDisplayWidth(plainText(tokens)), MIN_COL)
      }

      // --- column width calculation ---
      const minWidths = tableToken.header.map((h, ci) => {
        let w = longestWord(h.tokens)
        for (const row of tableToken.rows) w = Math.max(w, longestWord(row[ci]?.tokens))
        return w
      })
      const idealWidths = tableToken.header.map((h, ci) => {
        let w = idealWidth(h.tokens)
        for (const row of tableToken.rows) w = Math.max(w, idealWidth(row[ci]?.tokens))
        return w
      })

      // border overhead: │ cell │ cell │ = 1 + numCols * 3
      const borderOverhead = 1 + numCols * 3
      const available = Math.max(termWidth - borderOverhead - SAFETY_MARGIN, numCols * MIN_COL)
      const totalIdeal = idealWidths.reduce((s, w) => s + w, 0)
      const totalMin = minWidths.reduce((s, w) => s + w, 0)

      // Track whether columns are narrower than longest words (needs hard wrap)
      let needsHardWrap = false
      let colWidths: number[]
      if (totalIdeal <= available) {
        colWidths = idealWidths
      } else if (totalMin > available) {
        // Table wider than terminal at minimum widths — shrink proportionally
        needsHardWrap = true
        const scaleFactor = available / totalMin
        colWidths = minWidths.map(w => Math.max(Math.floor(w * scaleFactor), MIN_COL))
      } else {
        // give each column its min, distribute remaining proportionally
        colWidths = [...minWidths]
        let remaining = available - totalMin
        const extras = idealWidths.map((ideal, i) => ideal - minWidths[i]!)
        const totalExtra = extras.reduce((s, e) => s + e, 0)
        if (totalExtra > 0) {
          for (let i = 0; i < numCols; i++) {
            const share = Math.floor((extras[i]! / totalExtra) * remaining)
            colWidths[i] = colWidths[i]! + share
          }
        }
      }

      // --- ANSI-aware word wrap (CJK-safe) ---
      function wrapCell(text: string, width: number): string[] {
        if (width <= 0) return [text]
        const trimmed = text.trimEnd()
        const wrapped = wrapAnsi(trimmed, width, {
          hard: needsHardWrap,
          trim: false,
          wordWrap: true,
        })
        const lines = wrapped.split('\n').filter(line => line.length > 0)
        return lines.length > 0 ? lines : ['']
      }

      // --- check if vertical format is needed ---
      const MAX_ROW_LINES = MAX_TABLE_ROW_LINES
      let needVertical = false
      for (const row of tableToken.rows) {
        for (let ci = 0; ci < numCols; ci++) {
          const wrapped = wrapCell(renderCell(row[ci]?.tokens), colWidths[ci]!)
          if (wrapped.length > MAX_ROW_LINES) { needVertical = true; break }
        }
        if (needVertical) break
      }

      // --- vertical key-value format ---
      function renderVerticalFormat(): string {
        const headers = tableToken.header.map(h => plainText(h.tokens))
        const separatorWidth = Math.min(termWidth - 1, 40)
        const separator = '─'.repeat(separatorWidth)
        const wrapIndent = '  '
        const vLines: string[] = []

        tableToken.rows.forEach((row, ri) => {
          if (ri > 0) vLines.push(separator)
          row.forEach((cell, ci) => {
            const label = headers[ci] || `Column ${ci + 1}`
            const rawValue = renderCell(cell.tokens).trimEnd()
            const value = rawValue.replace(/\n+/g, ' ').replace(/\s+/g, ' ').trim()

            // Two-pass wrap: first line is narrower (label takes space),
            // continuation lines get the full width minus indent.
            const firstLineWidth = termWidth - terminalDisplayWidth(label) - 3
            const subsequentLineWidth = termWidth - wrapIndent.length - 1
            const firstPassLines = wrapCell(value, Math.max(firstLineWidth, 10))
            const firstLine = firstPassLines[0] || ''

            let wrappedValue: string[]
            if (firstPassLines.length <= 1 || subsequentLineWidth <= firstLineWidth) {
              wrappedValue = firstPassLines
            } else {
              // Re-join remaining text and re-wrap to the wider continuation width
              const remainingText = firstPassLines.slice(1).map(l => stripAnsi(l).trim()).join(' ')
              const rewrapped = wrapCell(remainingText, subsequentLineWidth)
              wrappedValue = [firstLine, ...rewrapped]
            }

            vLines.push(`${theme.tableHeader.paint(label)}: ${wrappedValue[0] || ''}`)
            for (let i = 1; i < wrappedValue.length; i++) {
              const ln = wrappedValue[i]!
              if (!stripAnsi(ln).trim()) continue
              vLines.push(`${wrapIndent}${ln}`)
            }
          })
        })
        return vLines.join(EOL) + EOL
      }

      if (needVertical) {
        return renderVerticalFormat() + EOL
      }

      // --- horizontal table with wrapping ---
      function borderLine(left: string, mid: string, cross: string, right: string): string {
        let line = left
        colWidths.forEach((w, i) => {
          line += mid.repeat(w + 2)
          line += i < numCols - 1 ? cross : right
        })
        return line
      }
      function renderRow(cells: { tokens?: Token[] }[], forceCenter = false): string {
        const wrapped = cells.map((cell, ci) =>
          wrapCell(renderCell(cell.tokens), colWidths[ci]!),
        )
        const height = Math.max(...wrapped.map(w => w.length))
        const lines: string[] = []
        for (let li = 0; li < height; li++) {
          let line = '│'
          for (let ci = 0; ci < numCols; ci++) {
            // Vertical centering: offset content lines to the middle
            const cellLines = wrapped[ci]!
            const vPad = Math.floor((height - cellLines.length) / 2)
            const vi = li - vPad
            const content = (vi >= 0 && vi < cellLines.length) ? cellLines[vi]! : ''
            const dw = terminalDisplayWidth(content)
            const align = forceCenter ? 'center' : tableToken.align?.[ci]
            line += ' ' + padAligned(content, dw, colWidths[ci]!, align) + ' │'
          }
          lines.push(line)
        }
        return lines.join(EOL)
      }

      const tableLines: string[] = []
      tableLines.push(borderLine('┌', '─', '┬', '┐'))
      tableLines.push(renderRow(tableToken.header, true))
      tableLines.push(borderLine('├', '─', '┼', '┤'))
      tableToken.rows.forEach((row, ri) => {
        tableLines.push(renderRow(row))
        if (ri < tableToken.rows.length - 1) {
          tableLines.push(borderLine('├', '─', '┼', '┤'))
        }
      })
      tableLines.push(borderLine('└', '─', '┴', '┘'))

      // Safety check: if any line exceeds terminal width (e.g. terminal
      // resized between calculation and render), fall back to vertical format.
      const maxLineWidth = Math.max(...tableLines.map(l => terminalDisplayWidth(l)))
      if (maxLineWidth > termWidth) {
        return renderVerticalFormat() + EOL
      }

      return tableLines.join(EOL) + EOL + EOL
    }
    case 'escape':
      return token.text
    case 'image':
      return token.href
    case 'def':
    case 'html':
      return ''
    default:
      return ''
  }
}

/**
 * Pad content to targetWidth respecting alignment.
 * displayWidth is the visible width (caller computes via stringWidth on
 * stripAnsi'd text, so ANSI codes don't affect padding).
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

// ---------------------------------------------------------------------------
// Ordered list numbering — depth-aware (number → letter → roman)
// ---------------------------------------------------------------------------

function getListNumber(listDepth: number, n: number): string {
  switch (listDepth) {
    case 0:
    case 1:
      return n.toString()
    case 2:
      return numberToLetter(n)
    case 3:
      return numberToRoman(n)
    default:
      return n.toString()
  }
}

function numberToLetter(n: number): string {
  let result = ''
  while (n > 0) {
    n--
    result = String.fromCharCode(97 + (n % 26)) + result
    n = Math.floor(n / 26)
  }
  return result
}

const ROMAN_VALUES: ReadonlyArray<[number, string]> = [
  [1000, 'm'], [900, 'cm'], [500, 'd'], [400, 'cd'],
  [100, 'c'], [90, 'xc'], [50, 'l'], [40, 'xl'],
  [10, 'x'], [9, 'ix'], [5, 'v'], [4, 'iv'], [1, 'i'],
]

function numberToRoman(n: number): string {
  let result = ''
  for (const [value, numeral] of ROMAN_VALUES) {
    while (n >= value) {
      result += numeral
      n -= value
    }
  }
  return result
}

const BLOCK_TYPES = new Set([
  'paragraph', 'code', 'heading', 'list', 'blockquote', 'hr', 'table',
])

function formatTokens(tokens: Token[]): string {
  const theme = getTheme()
  let out = ''
  let prevWasBlock = false

  for (const token of tokens) {
    const rendered = formatToken(token, 0, null, null, theme)
    if (!rendered) continue
    const isBlock = BLOCK_TYPES.has(token.type)
    // Insert blank line between consecutive block-level elements
    if (isBlock && prevWasBlock) {
      out += EOL
    }
    out += rendered
    prevWasBlock = isBlock
  }

  return out.trim()
}

/**
 * Render markdown text to terminal-friendly ANSI output.
 */
export function renderMarkdown(text: string): string {
  if (!text || text.trim().length === 0) return text

  configureMarked()
  try {
    // Strip prompt XML tags (system-reminder, commit_analysis, …) first so
    // they never reach the lexer or the plain-text fast path.
    const stripped = stripPromptXMLTags(text)
    const lexText = prepareMarkdownForLex(stripped)
    const tokens = hasMarkdownSyntax(lexText)
      ? marked.lexer(lexText)
      : plainTextTokens(stripped)
    return formatTokens(tokens)
  } catch {
    return text
  }
}

// ---------------------------------------------------------------------------
// Markdown render cache (LRU)
// ---------------------------------------------------------------------------

const CACHE_MAX = 200
const renderCache = new Map<string, string>()

function simpleHash(s: string): string {
  let h = 0
  for (let i = 0; i < s.length; i++) {
    h = ((h << 5) - h + s.charCodeAt(i)) | 0
  }
  return h.toString(36)
}

/**
 * Render markdown with LRU caching.
 * Same as renderMarkdown but caches results by content hash.
 */
export function renderMarkdownCached(text: string): string {
  if (!text || text.trim().length === 0) return text

  const hash = simpleHash(text)
  const cached = renderCache.get(hash)
  if (cached !== undefined) {
    // Move to end (LRU touch)
    renderCache.delete(hash)
    renderCache.set(hash, cached)
    return cached
  }

  const result = renderMarkdown(text)

  renderCache.set(hash, result)
  if (renderCache.size > CACHE_MAX) {
    // Evict oldest entry
    const first = renderCache.keys().next().value
    if (first !== undefined) renderCache.delete(first)
  }

  return result
}

/** Clear the render cache (for tests). */
export function clearRenderCache(): void {
  renderCache.clear()
}

/** Get current cache size (for tests). */
export function getRenderCacheSize(): number {
  return renderCache.size
}

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

export function findStreamingCommitPoint(text: string): number {
  if (!text) return 0

  const repaired = repairUnclosedFences(text, false)
  if (repaired !== text) {
    const insertedAt = firstDifferenceIndex(text, repaired)
    return insertedAt > 0 ? insertedAt : 0
  }

  configureMarked()
  const tokens = marked.lexer(text)
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

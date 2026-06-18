import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import wrapAnsi from 'wrap-ansi'

const EOL = '\n'
const SAFETY_MARGIN = 4
const MAX_RENDER_WIDTH = 140
const CODE_FENCE_RE = /^( {0,3})(`{3,}|~{3,})(.*)$/
// Heading matches `#{1,6}` followed by either whitespace/EOL (classic ATX)
// OR a non-hash, non-space character (glued form we still want to recognise,
// e.g. `##改进清单`). Only classic ATX (`#{2,6}<non-space>`) is permitted in
// the glued case so `#include`/`#1` don't collide.
const MARKDOWN_BOUNDARY_RE = /^([ \t]{0,3})?(#{1,6}(?:\s|$)|#{2,6}(?=[^#\s])|(?:\*\*|__)|(?:[-*+]\s)|(?:\d+\.\s)|>\s|\|.*\||-{3,}\s*$)/
const CODE_LIKE_START_RE = /^[\[{(}\]),;]|^\/\/|^#\s*include\b/
const CODE_KEYWORD_RE = /^(return|if|else|for|while|switch|case|break|continue|try|catch|finally|throw|await|async|const|let|var|function|class|def|import|export|from|SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|WHERE|ORDER|GROUP|LIMIT|DROP|SET|ALTER|MERGE|TRUNCATE)\b/i
const SQL_KEYWORD_RE = /^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP|SET|MERGE|TRUNCATE)\b/i
const CODE_ASSIGNMENT_RE = /^[\w$.'"`-]+\s*[:=]/
const CODE_BLOCK_START_RE = /^(try\s*\(|while\s*\(|for\s*\(|if\s*\(|class\s+\w|def\s+\w|function\s+\w|val\s+\w|const\s+\w|let\s+\w|var\s+\w|from\s+\w+(?:\.\w+)*\s+import\b|import\s+\w+(?:\.\w+)*(?:\s+as\s+\w+)?$)\b/i
// Box-drawing characters used in tree/diagram structures (U+2500–U+257F)
const BOX_DRAWING_RE = /[\u2500-\u257f]/

function terminalDisplayWidth(text: string): number {
  return stringWidth(stripAnsi(text))
}

function safeTerminalColumns(): number {
  const columns = process.stdout.columns
  return Number.isFinite(columns) && columns > 0 ? Math.floor(columns) : 80
}

function terminalContentWidth(): number {
  const columns = safeTerminalColumns()
  return Math.max(20, Math.min(columns - SAFETY_MARGIN, MAX_RENDER_WIDTH))
}

/** Terminal width for tables — no MAX_RENDER_WIDTH cap so wide tables
 *  can use the full terminal on large screens. */
function terminalTableWidth(): number {
  const columns = safeTerminalColumns()
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
  if (!looksLikeMarkdownBoundary(rest) && !looksLikePlainMarkdownAfterCode(rest) && !/^[A-Z][A-Za-z\s]+\./.test(rest)) return null
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
  const fenceRun = `${marker === '`' ? '`' : '~'}{${minLength},}`
  const trailing = new RegExp(`^(.*?)(${fenceRun})[ \\t]*$`)
  const trailingMatch = trailing.exec(line)
  if (trailingMatch) {
    const content = trailingMatch[1]!
    const fence = trailingMatch[2]!
    // The content must not be empty (otherwise it's just a fence) and must
    // not itself contain the fence marker — keeps inline backticks alone.
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
  if (!looksLikeMarkdownBoundary(normalizedRest) && !looksLikePlainMarkdownAfterCode(normalizedRest) && !/^[A-Z][A-Za-z\s]+\./.test(normalizedRest)) return null
  return [content.trimEnd(), fence, normalizedRest]
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
  if (SQL_KEYWORD_RE.test(content)) return true
  if (/^(import|export|const|let|var|function|class|def|async|type|interface)\b/.test(content)) return true
  return false
}

function looksLikeImplicitCodeStart(line: string): boolean {
  const trimmed = line.trimStart()
  if (!trimmed) return false
  if (CODE_BLOCK_START_RE.test(trimmed)) return true
  if (SQL_KEYWORD_RE.test(trimmed)) return true
  if (/^#\s+(databend-driver|mysql-connector-python|PyMySQL)\b/.test(trimmed)) return true
  return false
}

function looksLikeImplicitCodeContinuation(line: string, lang: string): boolean {
  const trimmed = line.trimStart()
  if (!trimmed) return true
  if (/^[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]/.test(trimmed)) return false
  if (lang === 'sql') {
    return SQL_KEYWORD_RE.test(trimmed)
      || /^(FROM|WHERE|ORDER\s+BY|GROUP\s+BY|HAVING|LIMIT|VALUES|AS)\b/i.test(trimmed)
      || /^[(),;]/.test(trimmed)
      || /^\w[\w.]*\s+/.test(trimmed)
      || /^\)/.test(trimmed)
  }
  if (lang === 'java') return !looksLikeMarkdownBoundary(trimmed) || /^[})];?/.test(trimmed)
  if (lang === 'python') return !looksLikeMarkdownBoundary(trimmed)
  if (lang === 'scala') return !looksLikeMarkdownBoundary(trimmed) || /^\./.test(trimmed)
  return false
}

function implicitCodeLanguage(line: string): string {
  const trimmed = line.trimStart()
  if (SQL_KEYWORD_RE.test(trimmed)) return 'sql'
  if (/^try\s*\(|^while\s*\(/.test(trimmed)) return 'java'
  if (/^from\s+\w+(?:\.\w+)*\s+import\b|^import\s+\w+(?:\.\w+)*(?:\s+as\s+\w+)?$/i.test(trimmed)) return 'python'
  if (/^val\s+\w/.test(trimmed)) return 'scala'
  return 'text'
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

function isShellFenceLanguage(lang: string | null): boolean {
  return !!lang && /^(bash|sh|zsh|shell|fish|nu|nushell)$/i.test(lang)
}

function isPlainTextFenceLanguage(lang: string | null): boolean {
  return !!lang && /^(text|txt|plain|plaintext)$/i.test(lang)
}

function startsWithCjkProse(line: string): boolean {
  return /^(?:\*\*|__)?[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]/.test(line.trimStart())
}

function shouldCloseShellFenceBeforeProse(line: string, codeLines: string[], lang: string | null): boolean {
  if (!isShellFenceLanguage(lang)) return false
  if (codeLines.length === 0 || codeLines[codeLines.length - 1]!.trim() !== '') return false
  if (!startsWithCjkProse(line)) return false
  return looksLikePlainMarkdownAfterCode(line)
}

function shouldClosePlainTextFenceBeforeMarkdown(line: string, codeLines: string[], lang: string | null): boolean {
  if (!isPlainTextFenceLanguage(lang)) return false
  if (codeLines.length === 0 || codeLines[codeLines.length - 1]!.trim() !== '') return false
  const hasDiagramContent = codeLines.some(codeLine => BOX_DRAWING_RE.test(codeLine))
  if (!hasDiagramContent) return false
  return looksLikeMarkdownBoundary(line)
}

function shouldCloseOpenFenceBeforeLine(line: string, codeLines: string[], lang: string | null): boolean {
  if (shouldCloseShellFenceBeforeProse(line, codeLines, lang)) return true
  if (shouldClosePlainTextFenceBeforeMarkdown(line, codeLines, lang)) return true
  if (!looksLikeStructuredCode(codeLines, lang)) return false
  if (looksLikeMarkdownBoundary(line)) return looksLikeCodeCompleted(codeLines, lang)
  if (!looksLikeCodeCompleted(codeLines, lang)) return false
  return looksLikePlainMarkdownAfterCode(line)
}

function shouldTreatAsStrayFenceClose(lines: string[], index: number): boolean {
  const line = lines[index]!
  const match = CODE_FENCE_RE.exec(line)
  if (!match) return false
  if (match[3]!.trim()) return false
  for (let i = index + 1; i < lines.length; i++) {
    const next = lines[i]!
    if (!next.trim()) continue
    if (isLikelyFenceClose(next, match[2]![0]!, match[2]!.length)) return false
    return looksLikeMarkdownBoundary(next) || startsWithCjkProse(next) || looksLikePlainMarkdownAfterCode(next)
  }
  return false
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
        if (shouldTreatAsStrayFenceClose(lines, i)) continue
        openMarker = match[2]![0]!
        openLength = match[2]!.length
        openClose = openMarker.repeat(openLength)
        openLang = fenceLanguageFromLine(line)
        codeLines = []
        out += line + newline
        continue
      }
      if (shouldTreatAsStrayFenceClose(lines, i)) {
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
      // Emit the content line (still inside the fence) then the standalone
      // fence-close line, and mark the fence as closed.
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
const HR_MARKER_BEFORE_HEADING_RE = /(^|\n)([ \t]*(?:-{3,}|\*{3,}|_{3,}))[ \t]*(#{1,6})([ \t]*)(?=[\S])/g
// HR marker glued directly to bold/italic emphasis markers on the same line,
// e.g. `---**五、SQL + 解释**`. Models often emit a separator and the next
// section's title (written as bold text rather than a real heading) without
// the required blank lines around the `---`. Only match `-{3,}` here —
// `*{3,}` and `_{3,}` would collide with emphasis syntax.
const HR_MARKER_BEFORE_EMPHASIS_RE =
  /(^|\n)([ \t]*-{3,})[ \t]*(\*\*|__)(?=\S)/g

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
  /([。．.!！？?!：:；;）】」』》])[ \t]+(#{2,6}(?:\s|$|[^\s#]))/g
// Zero-whitespace variant: the punctuation or a CJK character is glued
// directly to `##…##` with no space in between, e.g. `。###档 1`,
// `ready.##Next`, `这个###档 1`, or `。### 一句话总结` (where only the preceding side is
// glued). We require the heading body to start with a non-hash character so
// the regex still terminates at the correct heading depth; the existing
// HEADING_MISSING_SPACE_* rules then add a space when the body itself is
// glued (`###档` → `### 档`).
const HEADING_GLUED_NO_SPACE_RE =
  /([。．.!！？?!：:；;）】」』》\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff])(#{2,6})(?=[^#])/g
// Unordered-list marker glued directly to a CJK body, e.g. `-summary 的详略`
// or `*这个改动`. CommonMark requires a space after `-`/`*`/`+`; models
// routinely drop it in CJK contexts. We only rewrite when the body starts
// with a non-ASCII character so that `-1` (negatives), `--flag` (CLI flags),
// `---` (HR) and `*emphasis*` all stay untouched.
const BULLET_MISSING_SPACE_RE = /^([ \t]{0,3})([-*+])(?=[^\s\-*+\x00-\x7f])/gm
// Bullet marker glued directly to an ASCII-ish label that is clearly followed
// by CJK prose/punctuation, e.g. `-prompt 不是扩展` or `-Layer0-1：dense`.
// Keep this narrower than the CJK rule so CLI options (`-p foo`) and
// negatives (`-1`) are not rewritten.
const BULLET_ASCII_MISSING_SPACE_RE = /^([ \t]{0,3})([-*+])(?=[A-Za-z][A-Za-z0-9_-]*(?:[：:][^\s]|[ \t]+[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]))/gm
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
// ASCII label glued after a CJK colon, e.g. `分层：-Layer0-1：dense`.
// Keep this separate from the generic colon rule so ASCII key/value text like
// `config:-foo:bar` is not rewritten as a list.
const BULLET_ASCII_GLUED_AFTER_CJK_COLON_RE =
  /([^\s:：])(：)[ \t]*([-*+])[ \t]*(?=[A-Za-z][A-Za-z0-9_-]*(?:[：:]|[ \t]+[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]))/g
// Ordered marker glued to the end of a prose line after a colon, e.g.
// `共识：1. 必须有…`. Same treatment as the bullet variant above.
// The lookahead `[ \t]+\D` is essential: require whitespace + non-digit
// after the period so decimals like `task_1: 0.8` (colon + `0.` + digit)
// stay intact. A real ordered-list item always has a space + non-digit
// body after its number.
const ORDERED_GLUED_AFTER_COLON_RE =
  /([^\s:：])([：:])[ \t]*(\d{1,9}[.)])(?=[ \t]+\D)[ \t]*/g
// Bullet marker glued after a completed list/prose clause on the same line,
// e.g. `...秒）- Decode 继续`. Split only after sentence-ish punctuation so
// hyphenated words and ranges stay intact.
const BULLET_GLUED_AFTER_SENTENCE_RE =
  /([。．.!！？?!；;）】」』》])([-+]|\*(?![^*\n]+\*))(?:[ \t]+(?=\S)|(?=[^\s\-*+\x00-\x7f]))/g
// Ordered marker glued directly after a CJK sentence-ending punctuation, e.g.
// `…等它完全加载（书签列表出现）。2. 保持这个标签活动`. Requires the digit
// to be followed by `. ` + a non-digit so we don't break decimals (`见 3.1`),
// IP/version strings, or bare numeric references that happen to appear after
// a period. Also requires the preceding char to be CJK or CJK punctuation —
// ASCII-only prose uses space naturally and `).2` inside code stays intact.
const ORDERED_GLUED_AFTER_CJK_RE =
  /([。．！？!?：:）】」』》\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff])(\d{1,9}[.)])(?=[ \t]+\D)/g
// Ordered list item whose indentation is ≥4 spaces: CommonMark treats this
// as either a code block or a lazy continuation of the previous paragraph,
// so the item silently merges with whatever came before. Models routinely
// over-indent mid-list items in CJK contexts (`     3.多指标…`). Normalise
// to at most 3 leading spaces before the other rules run.
const ORDERED_OVER_INDENT_RE = /^[ \t]{4,}(?=\d{1,9}[.)][\s\u3400-\u4dbf\u4e00-\u9fff])/gm

function normalizeHrLines(text: string): string {
  const lines = text.split('\n')
  const out: string[] = []
  let inFence = false
  let fenceMarker = ''
  const apply = (chunk: string): string => chunk
    .replace(HR_MARKER_BEFORE_HEADING_RE, '$1$2\n\n$3 ')
    .replace(HR_MARKER_BEFORE_EMPHASIS_RE, '$1$2\n\n$3')
    .replace(HR_MARKER_GLUED_RE, '$1\n\n$3\n\n')
    .replace(HR_MARKER_INLINE_RE, '$1\n\n$2\n\n')
    .replace(HR_MARKER_TRAILING_RE, '$1\n\n$2')
  let chunk: string[] = []
  const flush = () => {
    if (chunk.length > 0) {
      out.push(...apply(chunk.join('\n')).split('\n'))
      chunk = []
    }
  }

  for (const line of lines) {
    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      flush()
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
    chunk.push(line)
  }
  flush()
  return out.join('\n')
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
      .replace(BULLET_ASCII_GLUED_AFTER_CJK_COLON_RE, '$1$2\n\n$3 ')
      .replace(ORDERED_GLUED_AFTER_COLON_RE, '$1$2\n\n$3 ')
      .replace(BULLET_GLUED_AFTER_SENTENCE_RE, '$1\n\n$2 ')
      .replace(ORDERED_GLUED_AFTER_CJK_RE, '$1\n\n$2 ')
      .replace(ORDERED_OVER_INDENT_RE, '   ')
      .split('\n')
    for (const s of split) {
      out.push(
        s
          .replace(EMPHASIS_HEADING_GLUED_BULLET_RE, '$1$2$3$2\n$1$4 ')
          .replace(HEADING_MISSING_SPACE_MANY_RE, '$1$2 ')
          .replace(HEADING_MISSING_SPACE_ONE_RE, '$1$2 ')
          .replace(BULLET_MISSING_SPACE_RE, '$1$2 ')
          .replace(BULLET_ASCII_MISSING_SPACE_RE, '$1$2 ')
          .replace(ORDERED_MISSING_SPACE_RE, '$1$2 '),
      )
    }
  }
  return out.join('\n')
}

// Markdown table separator line — exclude from box-drawing preservation.
const MD_TABLE_SEP_RE = /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)+\|?\s*$/

// Strong/emphasis closing marker glued to following CJK text after sentence
// punctuation, e.g. `**...东西。**调 API`. CommonMark/marked does not treat
// the closing `**` as a right-flanking delimiter in this shape, so the literal
// asterisks leak. Insert an HTML comment as an invisible separator; the html
// token is stripped by formatToken, preserving the visible text while letting
// marked parse the strong span.
const EMPHASIS_CLOSING_GLUED_TO_CJK_RE = /((\*\*|__)[^\n]*?[。．.!！？?!][ \t]*(?:\2))(?=[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff])/g
// Bold section label glued directly to a bullet marker, e.g.
// `**内存分配策略**- 静态分配`. Split it into a standalone label and a real
// list item so marked can parse both instead of leaking raw `**` / `-` text.
const EMPHASIS_HEADING_GLUED_BULLET_RE = /^([ \t]{0,3})(\*\*|__)([^\n]+?)\2[ \t]*([-*+])[ \t]*(?=\S)/gm


function parseMarkdownTableSeparator(line: string): number | null {
  const trimmed = line.trim()
  if (!MD_TABLE_SEP_RE.test(trimmed)) return null
  const cells = trimmed
    .replace(/^\|/, '')
    .replace(/\|$/, '')
    .split('|')
  if (cells.length < 2) return null
  return cells.every(cell => /^\s*:?-+:?\s*$/.test(cell)) ? cells.length : null
}

function unescapedPipeIndexes(line: string): number[] {
  const indexes: number[] = []
  for (let i = 0; i < line.length; i++) {
    if (line[i] !== '|') continue
    let slashCount = 0
    for (let j = i - 1; j >= 0 && line[j] === '\\'; j--) slashCount++
    if (slashCount % 2 === 0) indexes.push(i)
  }
  return indexes
}

function splitGluedTableHeaderSeparatorLine(line: string): string[] | null {
  const indexes = unescapedPipeIndexes(line)
  for (const pipeIndex of indexes) {
    const header = line.slice(0, pipeIndex + 1)
    const separator = line.slice(pipeIndex + 1)
    if (!header.trimStart().startsWith('|')) continue
    if (!separator.trimStart().startsWith('|')) continue
    const headerColumns = header.replace(/^\s*\|/, '').replace(/\|\s*$/, '').split('|').length
    const separatorColumns = parseMarkdownTableSeparator(separator)
    if (separatorColumns !== null && separatorColumns === headerColumns) {
      return [header, separator]
    }
  }
  return null
}

function splitGluedTableSeparatorLine(line: string): string[] | null {
  let searchFrom = 0
  while (searchFrom < line.length) {
    const doublePipe = line.indexOf('||', searchFrom)
    if (doublePipe < 0) return null
    const separator = line.slice(0, doublePipe + 1)
    const rest = line.slice(doublePipe + 1)
    if (parseMarkdownTableSeparator(separator) !== null && rest.trimStart().startsWith('|')) {
      return [separator, rest]
    }
    searchFrom = doublePipe + 1
  }
  return null
}

function splitGluedTableRowTrailingText(line: string, columnCount: number): string[] | null {
  if (!line.trimStart().startsWith('|')) return null
  const pipeIndexes = unescapedPipeIndexes(line)
  const finalPipe = pipeIndexes[columnCount]
  if (finalPipe === undefined) return null

  const row = line.slice(0, finalPipe + 1)
  const trailing = line.slice(finalPipe + 1)
  if (!/^\S/.test(trailing) || trailing.startsWith('|')) return null
  return [row, trailing]
}

function splitProseGluedToTableHeader(line: string, nextLine: string | undefined): string[] | null {
  // Detect: `## heading text| col1 | col2 |` followed by `|------|------|`
  // The line does NOT start with `|` but contains a table header glued to prose.
  if (line.trimStart().startsWith('|')) return null
  if (!nextLine || parseMarkdownTableSeparator(nextLine) === null) return null

  // Find the first `|` that starts a valid table header matching the separator columns
  const separatorColumns = parseMarkdownTableSeparator(nextLine)
  if (separatorColumns === null) return null

  const indexes = unescapedPipeIndexes(line)
  for (const pipeIndex of indexes) {
    const candidate = line.slice(pipeIndex)
    if (!candidate.trimStart().startsWith('|')) continue
    // Count columns in the candidate header
    const cells = candidate.replace(/^\s*\|/, '').replace(/\|\s*$/, '').split('|')
    if (cells.length === separatorColumns) {
      const prose = line.slice(0, pipeIndex).trimEnd()
      if (!prose) continue
      return [prose, '', candidate]
    }
  }
  return null
}

function normalizeGluedTables(text: string): string {
  const lines = text.split('\n')
  const expandedLines: string[] = []
  let inFence = false
  let fenceMarker = ''

  for (let i = 0; i < lines.length; i++) {
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
      expandedLines.push(line)
      continue
    }

    if (inFence) {
      expandedLines.push(line)
      continue
    }

    const proseSplit = splitProseGluedToTableHeader(line, lines[i + 1])
    if (proseSplit) {
      expandedLines.push(...proseSplit)
      continue
    }

    const headerSplit = splitGluedTableHeaderSeparatorLine(line)
    if (headerSplit) {
      expandedLines.push(...headerSplit)
      continue
    }

    const split = splitGluedTableSeparatorLine(line)
    if (split) {
      expandedLines.push(...split)
    } else {
      expandedLines.push(line)
    }
  }

  const out: string[] = []
  let activeTableColumns: number | null = null
  inFence = false
  fenceMarker = ''

  for (const line of expandedLines) {
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
      activeTableColumns = null
      out.push(line)
      continue
    }

    if (inFence) {
      out.push(line)
      continue
    }

    const separatorColumns = parseMarkdownTableSeparator(line)
    if (separatorColumns !== null && out.length > 0 && out[out.length - 1]!.includes('|')) {
      activeTableColumns = separatorColumns
      out.push(line)
      continue
    }

    if (activeTableColumns !== null) {
      const split = splitGluedTableRowTrailingText(line, activeTableColumns)
      if (split) {
        out.push(split[0]!, '', split[1]!)
        activeTableColumns = null
        continue
      }
      if (!line.trimStart().startsWith('|')) activeTableColumns = null
    }

    out.push(line)
  }

  return out.join('\n')
}

interface TreeTrailingComment {
  lineIndex: number
  prefix: string
  comment: string
  prefixWidth: number
}

function findTreeTrailingComment(line: string): Omit<TreeTrailingComment, 'lineIndex' | 'prefixWidth'> | null {
  const hashIndex = line.indexOf('#')
  if (hashIndex < 0) return null
  if (hashIndex === 0 || !/\s/.test(line[hashIndex - 1]!)) return null

  const prefix = line.slice(0, hashIndex).trimEnd()
  if (!prefix.trim()) return null
  if (!BOX_DRAWING_RE.test(prefix)) return null

  return {
    prefix,
    comment: line.slice(hashIndex).trimEnd(),
  }
}

function alignTreeTrailingComments(lines: string[]): string[] {
  const comments: TreeTrailingComment[] = []
  for (let i = 0; i < lines.length; i++) {
    const comment = findTreeTrailingComment(lines[i]!)
    if (!comment) continue
    comments.push({
      ...comment,
      lineIndex: i,
      prefixWidth: terminalDisplayWidth(comment.prefix),
    })
  }

  if (comments.length < 2) return lines

  const aligned = [...lines]
  const targetEndColumn = Math.max(
    ...comments.map(comment => comment.prefixWidth + 2 + terminalDisplayWidth(comment.comment)),
    terminalContentWidth(),
  )
  for (const comment of comments) {
    const padding = ' '.repeat(Math.max(2, targetEndColumn - comment.prefixWidth - terminalDisplayWidth(comment.comment)))
    aligned[comment.lineIndex] = `${comment.prefix}${padding}${comment.comment}`
  }
  return aligned
}

function alignTreeBlocks(lines: string[]): string[] {
  const normalized = normalizeTreeSiblingIndent(lines)
  const out = [...normalized]
  let blockStart: number | null = null
  let blockHasComment = false

  function flush(until: number): void {
    if (blockStart === null) return
    const block = normalized.slice(blockStart, until)
    if (blockHasComment) {
      out.splice(blockStart, block.length, ...alignTreeTrailingComments(block))
    }
    blockStart = null
    blockHasComment = false
  }

  for (let i = 0; i < normalized.length; i++) {
    const line = normalized[i]!
    const isTreeLine = BOX_DRAWING_RE.test(line)
      || /^\s*$/.test(line)
      || /^\s*[├└]──\s+/.test(line)
      || (blockStart !== null && /^\s*[^\s].*/.test(line))

    if (!isTreeLine) {
      flush(i)
      continue
    }

    if (blockStart === null) blockStart = i
    if (findTreeTrailingComment(line)) blockHasComment = true
  }
  flush(normalized.length)
  return out
}

function normalizeTreeSiblingIndent(lines: string[]): string[] {
  const normalized = [...lines]
  for (let i = 0; i < normalized.length - 1; i++) {
    const parent = /^(\s*)└──\s+.*\/$/.exec(normalized[i]!)
    if (!parent) continue

    const childIndent = `${parent[1]!}    `
    for (let j = i + 1; j < normalized.length; j++) {
      const line = normalized[j]!
      if (!line.trim()) break
      if (/^\s*[├└]──\s+/.test(line)) {
        normalized[j] = line.replace(/^\s*(?=[├└]──\s+)/, childIndent)
        continue
      }
      break
    }
  }
  return normalized
}

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
      out.push(...alignTreeBlocks(block))
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
//
// Two guards keep this from eating legitimate content when the assistant is
// *discussing* these tags (e.g. an analysis of how prompts inject
// `<system-reminder>` blocks):
//   1. Line-anchored: a genuine leaked envelope occupies whole lines — the
//      opening tag begins a line and the closing tag ends one. Inline
//      mentions (`<system-reminder>` in prose, or wrapped in backticks) and
//      table cells never match because the tag sits mid-line.
//   2. Fence-aware (see stripPromptXMLTags): tags written inside fenced code
//      blocks are left verbatim. Without this, a lazy match from an in-prose
//      tag could span into a code fence's closing tag and delete everything
//      in between — including unrelated tables and headings.
const STRIPPED_PROMPT_TAG_NAMES = 'system-reminder|commit_analysis|context|function_analysis|pr_analysis'
const STRIPPED_PROMPT_TAGS_RE = new RegExp(
  `^[ \\t]*<(${STRIPPED_PROMPT_TAG_NAMES})>[\\s\\S]*?<\\/\\1>[ \\t]*$\\n?`,
  'gm',
)
const STRIPPED_PROMPT_TAG_PRESENT_RE = new RegExp(`<(?:${STRIPPED_PROMPT_TAG_NAMES})>`)

function stripPromptXMLTags(content: string): string {
  if (!STRIPPED_PROMPT_TAG_PRESENT_RE.test(content)) return content

  // Split into fenced / non-fenced regions so tags inside code blocks survive,
  // then apply the line-anchored stripper only to prose regions.
  const lines = content.split('\n')
  const out: string[] = []
  let buffer: string[] = []
  let inFence = false
  let fenceMarker = ''
  const flushBuffer = (): void => {
    if (buffer.length === 0) return
    out.push(buffer.join('\n').replace(STRIPPED_PROMPT_TAGS_RE, ''))
    buffer = []
  }

  for (const line of lines) {
    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      const marker = fenceMatch[2]!
      if (!inFence) {
        flushBuffer()
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
    } else {
      buffer.push(line)
    }
  }
  flushBuffer()

  return out.join('\n')
}

// Opening code fence (```lang) glued to the end of preceding prose on the
// same line, e.g. `四、TypeScript + JSX 片段```tsx`. Models routinely write
// the fence without the required leading newline when introducing a snippet
// mid-sentence. Split it onto its own line so marked sees a proper fence.
// Trigger only when at least one non-space, non-backtick character precedes
// the marker on the same line; this keeps legitimate opening fences at the
// start of a line untouched. The marker must be at the very end of the line
// (optionally with an info string) — inline `\`\`\`` used as emphasis or
// escape should not accidentally match.
const FENCE_OPEN_GLUED_RE = /^([^\n`]*?[^\s`])(`{3,}|~{3,})([^\n`]*)$/gm

function splitSingleLineFence(line: string): string[] | null {
  const match = /^( {0,3})(`{3,}|~{3,})([^`~\n]*?)(`{3,}|~{3,})[ \t]*$/.exec(line)
  if (!match) return null
  const indent = match[1]!
  const open = match[2]!
  const content = match[3]!.trim()
  const close = match[4]!
  if (!content) return null
  if (close[0] !== open[0] || close.length < open.length) return null
  if (/^[A-Za-z0-9_+.#-]+$/.test(content)) return null
  return [`${indent}${open}`, content, close]
}

function splitGluedFenceOpens(text: string): string {
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

    // Track existing fence state so we don't rewrite anything inside a code
    // block — content lines may legitimately contain backticks.
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
      // Detect a closing fence glued to the end of a content line, e.g.
      // `content here``` ` — the model forgot the newline before the closing
      // fence. Split it so the lexer sees a proper close.
      const escChar = fenceMarker[0]!.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const closeGlued = new RegExp(
        `^(.+[^\\s${escChar}])(${escChar}{${fenceMarker.length},})[ \\t]*$`
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
      const [, lead, marker, info] = m
      // Only split when the info string looks like a plain language tag
      // (alphanumerics / +.#-_). Anything else is likely not a real fence.
      const infoTrim = (info ?? '').trim()
      if (infoTrim === '' || /^[A-Za-z0-9_+.#-]+$/.test(infoTrim)) {
        out.push(lead!.trimEnd(), `${marker}${infoTrim}`)
        continue
      }
    }
    out.push(line)
  }
  return out.join('\n')
}

function normalizeEmphasisClosing(text: string): string {
  const lines = text.split('\n')
  const out: string[] = []
  let inFence = false
  let fenceMarker = ''

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

    out.push(inFence ? line : line.replace(EMPHASIS_CLOSING_GLUED_TO_CJK_RE, '$1<!-- -->'))
  }

  return out.join('\n')
}

function implicitCodeCanContinueAcrossBlank(lang: string, nextLine: string | undefined): boolean {
  if (nextLine === undefined) return true
  const trimmed = nextLine.trimStart()
  if (!trimmed) return true
  if (/^[\u3400-\u4dbf\u4e00-\u9fff\u3040-\u30ff]/.test(trimmed)) return false
  if (looksLikeImplicitCodeStart(trimmed)) return true
  if (lang === 'sql') return looksLikeImplicitCodeContinuation(trimmed, lang)
  if (lang === 'scala') return /^\./.test(trimmed)
  return /^[})\]]/.test(trimmed)
}

function normalizeImplicitCodeBlocks(text: string): string {
  const lines = text.split('\n')
  const out: string[] = []
  let inFence = false
  let fenceMarker = ''

  for (let i = 0; i < lines.length; i++) {
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
      continue
    }

    if (inFence || !looksLikeImplicitCodeStart(line)) {
      out.push(line)
      continue
    }

    const lang = implicitCodeLanguage(line)
    const block = [line]
    let j = i + 1
    while (j < lines.length && looksLikeImplicitCodeContinuation(lines[j]!, lang)) {
      if (!lines[j]!.trim() && !implicitCodeCanContinueAcrossBlank(lang, lines[j + 1])) break
      block.push(lines[j]!)
      j++
    }

    const nonBlank = block.filter(blockLine => blockLine.trim())
    if (nonBlank.length >= 2 || /[;{(]$/.test(line.trimEnd())) {
      out.push(`\`\`\`${lang}`, ...block, '```')
      i = j - 1
      continue
    }

    out.push(line)
  }

  return out.join('\n')
}

interface MarkdownNormalizeStage {
  name: string
  apply: (text: string) => string
}

const MARKDOWN_NORMALIZE_STAGES: MarkdownNormalizeStage[] = [
  { name: 'fence-open-glue', apply: splitGluedFenceOpens },
  { name: 'fence-close-repair', apply: text => repairUnclosedFences(text, false) },
  { name: 'heading-list-glue', apply: normalizeGluedHeadings },
  { name: 'implicit-code-blocks', apply: normalizeImplicitCodeBlocks },
  { name: 'table-glue', apply: normalizeGluedTables },
  { name: 'box-drawing-preserve', apply: preserveBoxDrawingBlocks },
  { name: 'hr-boundary', apply: normalizeHrLines },
  { name: 'emphasis-boundary', apply: normalizeEmphasisClosing },
]

function applyMarkdownNormalizeStages(text: string): string {
  let current = text
  for (const stage of MARKDOWN_NORMALIZE_STAGES) {
    current = stage.apply(current)
  }
  return current
}

function prepareMarkdownForLex(text: string): string {
  return repairUnclosedFences(applyMarkdownNormalizeStages(text), true)
}


export {
  EOL,
  SAFETY_MARGIN,
  MAX_RENDER_WIDTH,
  CODE_FENCE_RE,
  BOX_DRAWING_RE,
  MD_TABLE_SEP_RE,
  terminalDisplayWidth,
  terminalContentWidth,
  terminalTableWidth,
  wrapDisplayTextWithIndent,
  wrapParagraph,
  looksLikeMarkdownBoundary,
  isFenceLine,
  parseFenceLine,
  fenceLanguageFromLine,
  isLikelyFenceClose,
  isPlainTextFenceLanguage,
  shouldClosePlainTextFenceBeforeMarkdown,
  shouldCloseOpenFenceBeforeLine,
  repairUnclosedFences,
  normalizeHrLines,
  normalizeGluedHeadings,
  parseMarkdownTableSeparator,
  normalizeGluedTables,
  preserveBoxDrawingBlocks,
  stripPromptXMLTags,
  splitGluedFenceOpens,
  normalizeEmphasisClosing,
  normalizeImplicitCodeBlocks,
  MARKDOWN_NORMALIZE_STAGES,
  applyMarkdownNormalizeStages,
  prepareMarkdownForLex,
}

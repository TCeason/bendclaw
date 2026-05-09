import type { Token, Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import wrapAnsi from 'wrap-ansi'
import type { MarkdownNode } from '../ast.js'
import {
  EOL,
  SAFETY_MARGIN,
  BOX_DRAWING_RE,
  terminalDisplayWidth,
  terminalTableWidth,
  wrapDisplayTextWithIndent,
  wrapParagraph,
} from '../normalize/index.js'
import { createHyperlink, isWarpTerminal, supportsHyperlinks, wrapHyperlink } from '../../render/hyperlink.js'
import { linkifyIssueRefs } from '../../render/linkify.js'
import { getTheme, type Theme } from '../../render/theme.js'

let highlighter: typeof import('cli-highlight') | null = null
try {
  highlighter = await import('cli-highlight')
} catch {
  // cli-highlight not available — code blocks render without syntax highlighting
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

// Common fence tags that highlight.js doesn't recognise directly. Map them
// onto the closest supported language so the code block still gets coloured
// instead of falling through to plaintext. Only map when the target is a
// reasonable visual approximation — we'd rather render plain than paint the
// wrong grammar over genuinely unrelated syntax.
const LANG_ALIASES: Record<string, string> = {
  // Protocol buffers
  proto: 'protobuf',
  // JSON dialects — all share core JSON syntax
  jsonc: 'json',
  json5: 'json',
  ndjson: 'json',
  jsonl: 'json',
  // Markdown + MDX (MDX is markdown with JSX fragments; core tokens match)
  mdx: 'markdown',
  // Generic "plain" / "txt" tags
  plain: 'plaintext',
  txt: 'plaintext',
  text: 'plaintext',
  // .env / dotenv files share KEY=value syntax with ini
  env: 'ini',
  dotenv: 'ini',
  properties: 'ini',
  conf: 'ini',
  // Shell variants — fish/nushell close enough to bash grammar-wise
  fish: 'bash',
  nu: 'bash',
  nushell: 'bash',
  // Logs
  log: 'accesslog',
  logs: 'accesslog',
  // Component files are mostly HTML templates
  vue: 'html',
  svelte: 'html',
  astro: 'html',
}

function resolveLanguage(lang: string | undefined): string | undefined {
  if (!lang) return undefined
  const normalized = lang.toLowerCase()
  return LANG_ALIASES[normalized] ?? normalized
}

type LineCommentMarker = '--' | '//' | '#'

interface TrailingCodeComment {
  lineIndex: number
  prefix: string
  comment: string
  prefixWidth: number
}

const SQL_START_RE = /^(SELECT|CREATE|INSERT|UPDATE|DELETE|WITH|ALTER|DROP|MERGE|TRUNCATE)\b/i

function looksLikeSqlCode(text: string): boolean {
  const firstContentLine = text.split(EOL).find(line => line.trim())
  return firstContentLine ? SQL_START_RE.test(firstContentLine.trimStart()) : false
}

function lineCommentMarkersForCode(lang: string, text: string): LineCommentMarker[] {
  if (/^(sql|pgsql|plsql|mysql|sqlite|postgresql)$/.test(lang) || looksLikeSqlCode(text)) return ['--']
  if (/^(javascript|js|typescript|ts|tsx|jsx|java|c|cpp|c\+\+|csharp|cs|go|rust|rs|swift|kotlin|scala|php|css|scss|less)$/.test(lang)) return ['//']
  if (/^(bash|sh|zsh|fish|nu|nushell|python|py|ruby|rb|perl|pl|yaml|yml|toml|ini|dockerfile|makefile|make|env|dotenv|properties|conf)$/.test(lang)) return ['#']
  return []
}

function findTrailingCodeComment(line: string, markers: LineCommentMarker[]): Omit<TrailingCodeComment, 'lineIndex' | 'prefixWidth'> | null {
  let quote: string | null = null
  let escaped = false

  for (let i = 0; i < line.length; i++) {
    const ch = line[i]!

    if (quote) {
      if (escaped) {
        escaped = false
      } else if (ch === '\\') {
        escaped = true
      } else if (ch === quote) {
        if (line[i + 1] === quote) {
          i++
        } else {
          quote = null
        }
      }
      continue
    }

    if (ch === "'" || ch === '"' || ch === '`') {
      quote = ch
      continue
    }

    for (const marker of markers) {
      if (!line.startsWith(marker, i)) continue
      if (i === 0 || !/\s/.test(line[i - 1]!)) continue

      const prefix = line.slice(0, i).trimEnd()
      if (!prefix.trim()) continue

      return {
        prefix,
        comment: line.slice(i).trimEnd(),
      }
    }
  }

  return null
}

function leadingWhitespace(line: string): string {
  return /^\s*/.exec(line)?.[0] ?? ''
}

function lineIsStandaloneComment(line: string, markers: LineCommentMarker[]): boolean {
  const trimmed = line.trimStart()
  return markers.some(marker => trimmed.startsWith(marker))
}

function lineIsCodeForIndent(line: string, markers: LineCommentMarker[]): boolean {
  return !!line.trim() && !lineIsStandaloneComment(line, markers)
}

function nearestCodeIndent(lines: string[], lineIndex: number, markers: LineCommentMarker[]): string | null {
  for (let i = lineIndex + 1; i < lines.length; i++) {
    if (!lines[i]!.trim()) continue
    if (lineIsCodeForIndent(lines[i]!, markers)) return leadingWhitespace(lines[i]!)
    break
  }

  for (let i = lineIndex - 1; i >= 0; i--) {
    if (!lines[i]!.trim()) continue
    if (lineIsCodeForIndent(lines[i]!, markers)) return leadingWhitespace(lines[i]!)
    break
  }

  return null
}

function alignStandaloneCodeComments(lines: string[], markers: LineCommentMarker[]): boolean {
  let changed = false
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    if (!lineIsStandaloneComment(line, markers)) continue

    const indent = nearestCodeIndent(lines, i, markers)
    if (indent === null) continue
    const aligned = `${indent}${line.trimStart()}`
    if (aligned === line) continue
    lines[i] = aligned
    changed = true
  }
  return changed
}

function alignTrailingCodeComments(text: string, lang: string): string {
  const markers = lineCommentMarkersForCode(lang, text)
  if (markers.length === 0) return text

  const lines = text.split(EOL)
  const standaloneChanged = alignStandaloneCodeComments(lines, markers)

  const comments: TrailingCodeComment[] = []
  for (let i = 0; i < lines.length; i++) {
    const comment = findTrailingCodeComment(lines[i]!, markers)
    if (!comment) continue
    comments.push({
      ...comment,
      lineIndex: i,
      prefixWidth: terminalDisplayWidth(comment.prefix),
    })
  }

  if (comments.length < 2) return standaloneChanged ? lines.join(EOL) : text

  const targetColumn = Math.max(...comments.map(comment => comment.prefixWidth)) + 2
  for (const comment of comments) {
    const padding = ' '.repeat(Math.max(2, targetColumn - comment.prefixWidth))
    lines[comment.lineIndex] = `${comment.prefix}${padding}${comment.comment}`
  }

  return lines.join(EOL)
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
      const lang = resolveLanguage((token as Tokens.Code).lang) ?? 'plaintext'
      let highlighted = alignTrailingCodeComments(text, lang)
      if (highlighter) {
        try {
          if (highlighter.supportsLanguage(lang)) {
            highlighted = highlighter.highlight(highlighted, { language: lang })
          }
        } catch {
          // fallback to plain text
        }
      }
      // Match claudecode: emit the highlighted code verbatim with no left
      // gutter or padding. Syntax highlighting alone is enough to make the
      // block visually distinct from prose, and copying the block yields
      // clean text with no leading characters to strip.
      return highlighted + EOL
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
      function visualLineWidths(tokens: Token[] | undefined): number[] {
        const lines = plainText(tokens).split(EOL)
        return lines.length > 0 ? lines.map(line => terminalDisplayWidth(line)) : [0]
      }
      function longestWord(tokens: Token[] | undefined): number {
        const words = plainText(tokens).split(/\s+/).filter(w => w.length > 0)
        if (words.length === 0) return MIN_COL
        return Math.max(...words.map(w => terminalDisplayWidth(w)), MIN_COL)
      }
      function idealWidth(tokens: Token[] | undefined): number {
        return Math.max(...visualLineWidths(tokens), MIN_COL)
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

      // --- vertical key-value fallback ---
      // Used only when the rendered horizontal table genuinely does not fit
      // in the terminal (see safety check after the table body is built).
      // We intentionally do NOT flip to this form based on per-cell line
      // count: CJK-heavy cells wrap often and demoting a legitimate table
      // into `label: value` lines separated by `────` loses the column
      // structure the author wrote the table for.
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

      // Safety check: if any single rendered line exceeds terminal width
      // (e.g. terminal resized between width computation and render), fall
      // back to the vertical form. Row strings built by renderRow can span
      // multiple visual lines (wrapped cells), so split on EOL first before
      // measuring — otherwise stringWidth effectively sums the widths of
      // every wrapped line in the row, which trips the guard on every CJK
      // row and silently destroys the table layout.
      const maxLineWidth = Math.max(
        ...tableLines.flatMap(chunk => chunk.split(EOL).map(l => terminalDisplayWidth(l))),
      )
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
      return ''
    case 'html': {
      // `marked` lexes `<br>` as an html token. It's the most common inline
      // HTML models emit — especially inside table cells, where it's the
      // canonical way to force a line break (GFM tables don't support
      // literal newlines inside cells). Convert it to an actual newline so
      // downstream wrapping sees the intended break; strip everything else.
      const raw = (token as Tokens.HTML).text ?? (token as Tokens.HTML).raw ?? ''
      if (/^\s*<\s*br\s*\/?\s*>\s*$/i.test(raw)) return EOL
      return ''
    }
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

export function formatTokens(tokens: Token[]): string {
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

  // Strip only leading/trailing newlines. `.trim()` would also eat leading
  // spaces — which corrupts tree/box-drawing art where the first line relies
  // on indentation to line up with deeper nodes below it.
  return out.replace(/^\n+|\n+$/g, '')
}


export function renderMarkdownNodes(nodes: MarkdownNode[]): string {
  return formatTokens(nodes.map(node => node.token))
}

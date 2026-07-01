import { marked, type Token, type Tokens } from 'marked'

let markedConfigured = false

function configureMarked(): void {
  if (markedConfigured) return
  markedConfigured = true

  marked.use({
    tokenizer: {
      // Disable strikethrough parsing — the model often uses ~ for "approximate"
      // (e.g., ~100) and rarely intends actual strikethrough formatting.
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


/**
 * Trim a streamed, partially-arrived closing code fence from the last token.
 *
 * While a code block streams in, its closing ``` arrives one backtick at a
 * time. marked treats the still-incomplete fence (e.g. a lone `` ` `` or `` `` ``)
 * as code *content*, so the block renders one line too tall and then shrinks
 * when the final backtick lands — a visible flicker. Detecting the partial
 * fence and dropping it keeps the block stable across streaming frames.
 *
 * Recurses into the last list item / blockquote since an open code block can be
 * nested. Mutates in place; marked returns fresh tokens per lex call. Ported
 * from pi's TUI markdown component.
 */
function trimPartialClosingFences(tokens: Token[]): void {
  const token = tokens[tokens.length - 1]
  if (!token) return

  if (token.type === 'list') {
    const items = (token as Tokens.List).items
    trimPartialClosingFences(items[items.length - 1]?.tokens ?? [])
    return
  }
  if (token.type === 'blockquote') {
    trimPartialClosingFences((token as Tokens.Blockquote).tokens ?? [])
    return
  }
  if (token.type !== 'code') return

  const code = token as Tokens.Code
  const marker = /^(`{3,}|~{3,})/.exec(code.raw)?.[1]
  const lastLine = code.raw.split('\n').pop()
  // Only trim when the final line is a *partial* fence: shorter than the opener
  // and made entirely of the opener's fence character. A complete fence (length
  // >= marker) is already stripped from `text` by marked, so this is a no-op
  // for finished content.
  if (!marker || !lastLine || lastLine.length >= marker.length || lastLine !== marker[0]?.repeat(lastLine.length)) {
    return
  }
  code.text = code.text.slice(0, -lastLine.length).replace(/\n$/, '')
}

export function lexRawMarkdownTokens(text: string): Token[] {
  configureMarked()
  return marked.lexer(text)
}

export function lexMarkdownTokens(lexText: string, plainTextSource = lexText): Token[] {
  configureMarked()
  if (!hasMarkdownSyntax(lexText)) return plainTextTokens(plainTextSource)
  const tokens = marked.lexer(lexText)
  trimPartialClosingFences(tokens)
  return tokens
}

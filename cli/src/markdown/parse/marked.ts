import { marked, type Token, type Tokens } from 'marked'

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


export function lexRawMarkdownTokens(text: string): Token[] {
  configureMarked()
  return marked.lexer(text)
}

export function lexMarkdownTokens(lexText: string, plainTextSource = lexText): Token[] {
  configureMarked()
  return hasMarkdownSyntax(lexText)
    ? marked.lexer(lexText)
    : plainTextTokens(plainTextSource)
}

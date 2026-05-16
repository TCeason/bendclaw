/**
 * Markdown rendering facade for terminal output.
 *
 * Pipeline:
 *   raw markdown
 *   → prompt stripping + LLM glue normalizers
 *   → marked lexer
 *   → ANSI token renderer
 *
 * Streaming commit planning lives in ../markdown/streaming/commit.js.
 */

import { prepareMarkdownForLex, stripPromptXMLTags } from '../markdown/normalize/index.js'
import { lexMarkdownTokens } from '../markdown/parse/marked.js'
import { formatTokens } from '../markdown/render/ansi.js'

export { formatToken, highlightCodeLine } from '../markdown/render/ansi.js'
export { findStreamingCommitPoint, findNaturalPlainTextCommitPoint, isInsideOpenMathBlock, splitMarkdownBlocks, type MarkdownSplit } from '../markdown/streaming/commit.js'

/**
 * Render markdown text to terminal-friendly ANSI output.
 */
export function renderMarkdown(text: string): string {
  if (!text || text.trim().length === 0) return text

  try {
    // Strip prompt XML tags (system-reminder, commit_analysis, …) first so
    // they never reach the lexer or the plain-text fast path.
    const stripped = stripPromptXMLTags(text)
    const lexText = prepareMarkdownForLex(stripped)
    const tokens = lexMarkdownTokens(lexText, stripped)
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

function cacheKey(text: string): string {
  const columns = process.stdout.columns
  const safeColumns = Number.isFinite(columns) && columns > 0 ? Math.floor(columns) : 80
  if (text.length > 4096) return `${safeColumns}\0#${simpleHash(text)}`
  return `${safeColumns}\0${text}`
}

/**
 * Render markdown with LRU caching.
 * Same as renderMarkdown but caches results by content and terminal width.
 */
export function renderMarkdownCached(text: string): string {
  if (!text || text.trim().length === 0) return text

  const key = cacheKey(text)
  const cached = renderCache.get(key)
  if (cached !== undefined) {
    // Move to end (LRU touch)
    renderCache.delete(key)
    renderCache.set(key, cached)
    return cached
  }

  const result = renderMarkdown(text)

  renderCache.set(key, result)
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

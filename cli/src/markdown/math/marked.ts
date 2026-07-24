import type { MarkedExtension, Tokens } from 'marked'

export type MathTokenType = 'math_inline' | 'math_block'

export interface MathToken extends Tokens.Generic {
  type: MathTokenType
  raw: string
  text: string
  displayMode: boolean
}

interface DelimitedMatch {
  raw: string
  text: string
}

function isEscaped(source: string, index: number): boolean {
  let slashes = 0
  for (let cursor = index - 1; cursor >= 0 && source[cursor] === '\\'; cursor--) {
    slashes++
  }
  return slashes % 2 === 1
}

function findClosingDelimiter(
  source: string,
  delimiter: string,
  from: number,
  requireNonWhitespaceBefore: boolean,
): number {
  let cursor = from
  while (cursor < source.length) {
    const found = source.indexOf(delimiter, cursor)
    if (found === -1) return -1
    if (!isEscaped(source, found)) {
      const before = source[found - 1]
      if (requireNonWhitespaceBefore && (before === undefined || /\s/.test(before))) return -1
      return found
    }
    cursor = found + delimiter.length
  }
  return -1
}

function matchInlineMath(source: string): DelimitedMatch | null {
  if (source.startsWith('\\(')) {
    const close = findClosingDelimiter(source, '\\)', 2, false)
    if (close <= 2) return null
    return {
      raw: source.slice(0, close + 2),
      text: source.slice(2, close),
    }
  }

  if (!source.startsWith('$')) return null
  const delimiter = source.startsWith('$$') ? '$$' : '$'
  const first = source[delimiter.length]
  // `$$$` and whitespace-led bodies are never inline math.
  if (first === undefined || first === '$' || /\s/.test(first)) return null

  const close = findClosingDelimiter(source, delimiter, delimiter.length, true)
  if (close < delimiter.length + 1) return null
  // A digit right after the closing dollar means currency, not math
  // (pandoc's rule): keeps "$5-$10" and "$1,000-$2,000" literal.
  const after = source[close + delimiter.length]
  if (after !== undefined && /\d/.test(after)) return null
  return {
    raw: source.slice(0, close + delimiter.length),
    text: source.slice(delimiter.length, close),
  }
}

function firstInlineMathStart(source: string): number | undefined {
  for (let cursor = 0; cursor < source.length; cursor++) {
    if (source[cursor] === '$' && !isEscaped(source, cursor)) {
      const match = matchInlineMath(source.slice(cursor))
      if (match) return cursor
    }
    if (source[cursor] === '\\' && source[cursor + 1] === '(' && !isEscaped(source, cursor)) {
      const match = matchInlineMath(source.slice(cursor))
      if (match) return cursor
    }
  }
  return undefined
}

function lineEnd(source: string, from: number): number {
  const newline = source.indexOf('\n', from)
  return newline === -1 ? source.length : newline
}

function matchBlockMath(source: string): DelimitedMatch | null {
  const open = /^( {0,3})(\$\$|\\\[)[ \t]*/.exec(source)
  if (!open) return null

  const delimiter = open[2]!
  const closing = delimiter === '$$' ? '$$' : '\\]'
  const contentStart = open[0].length
  const firstEnd = lineEnd(source, contentStart)
  const sameLineClose = findClosingDelimiter(source.slice(0, firstEnd), closing, contentStart, false)

  if (sameLineClose >= contentStart) {
    const afterClose = sameLineClose + closing.length
    if (source.slice(afterClose, firstEnd).trim().length > 0) return null
    const rawEnd = firstEnd < source.length ? firstEnd + 1 : firstEnd
    const text = source.slice(contentStart, sameLineClose).trim()
    if (!text) return null
    return { raw: source.slice(0, rawEnd), text }
  }

  let cursor = firstEnd < source.length ? firstEnd + 1 : firstEnd
  while (cursor < source.length) {
    const end = lineEnd(source, cursor)
    const line = source.slice(cursor, end)
    const close = new RegExp(`^ {0,3}${closing.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}[ \\t]*$`)
    if (close.test(line)) {
      const rawEnd = end < source.length ? end + 1 : end
      const bodyStart = firstEnd < source.length && source.slice(contentStart, firstEnd).trim() === ''
        ? firstEnd + 1
        : contentStart
      const text = source.slice(bodyStart, cursor).replace(/\n$/, '').trim()
      if (!text) return null
      return { raw: source.slice(0, rawEnd), text }
    }
    cursor = end < source.length ? end + 1 : source.length
  }

  return null
}

/**
 * Marked tokenizers only: rendering remains owned by the ANSI layer. Keeping
 * delimiter recognition separate prevents HTML-oriented math libraries from
 * leaking into the terminal renderer.
 */
export function createMathMarkedExtension(): MarkedExtension {
  return {
    extensions: [
      {
        name: 'math_block',
        level: 'block',
        tokenizer(source): MathToken | undefined {
          const match = matchBlockMath(source)
          if (!match) return undefined
          return {
            type: 'math_block',
            raw: match.raw,
            text: match.text,
            displayMode: true,
          }
        },
      },
      {
        name: 'math_inline',
        level: 'inline',
        start(source): number | undefined {
          return firstInlineMathStart(source)
        },
        tokenizer(source): MathToken | undefined {
          const match = matchInlineMath(source)
          if (!match) return undefined
          return {
            type: 'math_inline',
            raw: match.raw,
            text: match.text.trim(),
            displayMode: false,
          }
        },
      },
    ],
  }
}

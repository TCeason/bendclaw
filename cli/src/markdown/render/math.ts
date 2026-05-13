/**
 * Lightweight LaTeX-to-Unicode converter for terminal rendering.
 * Handles Greek letters, superscripts, subscripts, common symbols,
 * and basic operators. Complex expressions fall back to cleaned-up LaTeX.
 */

const GREEK: Record<string, string> = {
  alpha: 'α', beta: 'β', gamma: 'γ', delta: 'δ', epsilon: 'ε',
  zeta: 'ζ', eta: 'η', theta: 'θ', iota: 'ι', kappa: 'κ',
  lambda: 'λ', mu: 'μ', nu: 'ν', xi: 'ξ', pi: 'π',
  rho: 'ρ', sigma: 'σ', tau: 'τ', upsilon: 'υ', phi: 'φ',
  chi: 'χ', psi: 'ψ', omega: 'ω',
  Gamma: 'Γ', Delta: 'Δ', Theta: 'Θ', Lambda: 'Λ', Xi: 'Ξ',
  Pi: 'Π', Sigma: 'Σ', Upsilon: 'Υ', Phi: 'Φ', Psi: 'Ψ', Omega: 'Ω',
  varepsilon: 'ε', varphi: 'φ', varpi: 'ϖ', varrho: 'ϱ',
  varsigma: 'ς', vartheta: 'ϑ',
}

const SYMBOLS: Record<string, string> = {
  sum: '∑', prod: '∏', int: '∫', iint: '∬', iiint: '∭',
  oint: '∮', infty: '∞', partial: '∂', nabla: '∇',
  forall: '∀', exists: '∃', nexists: '∄', emptyset: '∅',
  times: '×', div: '÷', cdot: '·', pm: '±', mp: '∓',
  leq: '≤', geq: '≥', neq: '≠', approx: '≈', equiv: '≡',
  sim: '∼', cong: '≅', propto: '∝',
  subset: '⊂', supset: '⊃', subseteq: '⊆', supseteq: '⊇',
  in: '∈', notin: '∉', ni: '∋', cup: '∪', cap: '∩',
  vee: '∨', wedge: '∧', neg: '¬', oplus: '⊕', otimes: '⊗',
  rightarrow: '→', leftarrow: '←', Rightarrow: '⇒', Leftarrow: '⇐',
  leftrightarrow: '↔', Leftrightarrow: '⇔', uparrow: '↑', downarrow: '↓',
  mapsto: '↦', to: '→', gets: '←',
  langle: '⟨', rangle: '⟩', lceil: '⌈', rceil: '⌉',
  lfloor: '⌊', rfloor: '⌋',
  sqrt: '√', cbrt: '∛',
  prime: '′', dprime: '″',
  star: '⋆', circ: '∘', bullet: '•', diamond: '◇',
  triangle: '△', square: '□',
  dots: '…', cdots: '⋯', ldots: '…', vdots: '⋮', ddots: '⋱',
  hbar: 'ℏ', ell: 'ℓ', Re: 'ℜ', Im: 'ℑ', aleph: 'ℵ',
  // Spacing/formatting (remove)
  quad: ' ', qquad: '  ', ',': ' ', ';': ' ', '!': '',
  left: '', right: '', big: '', Big: '', bigg: '', Bigg: '',
  text: '', mathrm: '', mathbf: '', mathit: '', mathcal: '',
  operatorname: '',
}

const SUPERSCRIPT: Record<string, string> = {
  '0': '⁰', '1': '¹', '2': '²', '3': '³', '4': '⁴',
  '5': '⁵', '6': '⁶', '7': '⁷', '8': '⁸', '9': '⁹',
  '+': '⁺', '-': '⁻', '=': '⁼', '(': '⁽', ')': '⁾',
  'n': 'ⁿ', 'i': 'ⁱ', 'a': 'ᵃ', 'b': 'ᵇ', 'c': 'ᶜ',
  'd': 'ᵈ', 'e': 'ᵉ', 'f': 'ᶠ', 'g': 'ᵍ', 'h': 'ʰ',
  'j': 'ʲ', 'k': 'ᵏ', 'l': 'ˡ', 'm': 'ᵐ', 'o': 'ᵒ',
  'p': 'ᵖ', 'r': 'ʳ', 's': 'ˢ', 't': 'ᵗ', 'u': 'ᵘ',
  'v': 'ᵛ', 'w': 'ʷ', 'x': 'ˣ', 'y': 'ʸ', 'z': 'ᶻ',
  'T': 'ᵀ',
}

const SUBSCRIPT: Record<string, string> = {
  '0': '₀', '1': '₁', '2': '₂', '3': '₃', '4': '₄',
  '5': '₅', '6': '₆', '7': '₇', '8': '₈', '9': '₉',
  '+': '₊', '-': '₋', '=': '₌', '(': '₍', ')': '₎',
  'a': 'ₐ', 'e': 'ₑ', 'h': 'ₕ', 'i': 'ᵢ', 'j': 'ⱼ',
  'k': 'ₖ', 'l': 'ₗ', 'm': 'ₘ', 'n': 'ₙ', 'o': 'ₒ',
  'p': 'ₚ', 'r': 'ᵣ', 's': 'ₛ', 't': 'ₜ', 'u': 'ᵤ',
  'v': 'ᵥ', 'x': 'ₓ',
}

const VULGAR_FRACTIONS: Record<string, string> = {
  '1/2': '½', '1/3': '⅓', '2/3': '⅔', '1/4': '¼', '3/4': '¾',
  '1/5': '⅕', '2/5': '⅖', '3/5': '⅗', '4/5': '⅘',
  '1/6': '⅙', '5/6': '⅚', '1/7': '⅐', '1/8': '⅛',
  '3/8': '⅜', '5/8': '⅝', '7/8': '⅞', '1/9': '⅑', '1/10': '⅒',
}

function convertSuperscript(text: string): string {
  let result = ''
  for (const ch of text) {
    result += SUPERSCRIPT[ch] ?? ch
  }
  return result
}

function convertSubscript(text: string): string {
  let result = ''
  for (const ch of text) {
    result += SUBSCRIPT[ch] ?? ch
  }
  return result
}

/** Strip outer braces: {content} → content */
function stripBraces(s: string): string {
  if (s.startsWith('{') && s.endsWith('}')) return s.slice(1, -1)
  return s
}

/**
 * Match a balanced brace group starting at position `pos` in `s`.
 * `s[pos]` must be '{'. Returns the content inside (excluding outer braces)
 * and the index after the closing '}'.
 */
function matchBraces(s: string, pos: number): { content: string; end: number } | null {
  if (s[pos] !== '{') return null
  let depth = 0
  for (let i = pos; i < s.length; i++) {
    if (s[i] === '{') depth++
    else if (s[i] === '}') {
      depth--
      if (depth === 0) {
        return { content: s.slice(pos + 1, i), end: i + 1 }
      }
    }
  }
  return null
}

/**
 * Replace \cmd{arg1}{arg2} patterns using balanced brace matching.
 */
function replaceCmd2(s: string, cmd: string, fn: (a: string, b: string) => string): string {
  const pattern = `\\${cmd}`
  let result = ''
  let i = 0
  while (i < s.length) {
    const idx = s.indexOf(pattern, i)
    if (idx === -1) {
      result += s.slice(i)
      break
    }
    result += s.slice(i, idx)
    const afterCmd = idx + pattern.length
    const first = matchBraces(s, afterCmd)
    if (!first) {
      result += pattern
      i = afterCmd
      continue
    }
    const second = matchBraces(s, first.end)
    if (!second) {
      result += pattern
      i = afterCmd
      continue
    }
    result += fn(first.content, second.content)
    i = second.end
  }
  return result
}

/**
 * Replace \cmd{arg} patterns using balanced brace matching.
 */
function replaceCmd1(s: string, cmd: string, fn: (a: string) => string): string {
  const pattern = `\\${cmd}`
  let result = ''
  let i = 0
  while (i < s.length) {
    const idx = s.indexOf(pattern, i)
    if (idx === -1) {
      result += s.slice(i)
      break
    }
    result += s.slice(i, idx)
    const afterCmd = idx + pattern.length
    const first = matchBraces(s, afterCmd)
    if (!first) {
      result += pattern
      i = afterCmd
      continue
    }
    result += fn(first.content)
    i = first.end
  }
  return result
}

/**
 * Convert a LaTeX math expression to Unicode text.
 * Best-effort: handles common patterns, passes through what it can't convert.
 */
export function latexToUnicode(latex: string): string {
  let s = latex.trim()

  // 1. Handle structural commands first (balanced brace matching)

  // \frac{a}{b} → a/b or vulgar fraction
  s = replaceCmd2(s, 'frac', (num, den) => {
    const n = latexToUnicode(num.trim())
    const d = latexToUnicode(den.trim())
    const key = `${n}/${d}`
    if (VULGAR_FRACTIONS[key]) return VULGAR_FRACTIONS[key]
    const nStr = n.length > 1 ? `(${n})` : n
    const dStr = d.length > 1 ? `(${d})` : d
    return `${nStr}⁄${dStr}`
  })

  // \sqrt[n]{x} → ⁿ√(x), \sqrt{x} → √(x)
  // First handle \sqrt[n]{body}
  s = (() => {
    const pattern = '\\sqrt['
    let result = ''
    let i = 0
    while (i < s.length) {
      const idx = s.indexOf(pattern, i)
      if (idx === -1) { result += s.slice(i); break }
      result += s.slice(i, idx)
      const closeBracket = s.indexOf(']', idx + pattern.length)
      if (closeBracket === -1) { result += pattern; i = idx + pattern.length; continue }
      const n = s.slice(idx + pattern.length, closeBracket)
      const body = matchBraces(s, closeBracket + 1)
      if (!body) { result += pattern; i = idx + pattern.length; continue }
      result += `${convertSuperscript(n)}√(${latexToUnicode(body.content)})`
      i = body.end
    }
    return result
  })()
  s = replaceCmd1(s, 'sqrt', (body) => `√(${latexToUnicode(body)})`)

  // 2. Resolve all \commands to Unicode (Greek, symbols, etc.)
  s = s.replace(/\\([a-zA-Z]+)/g, (_m, cmd) => {
    if (GREEK[cmd]) return GREEK[cmd]
    if (SYMBOLS[cmd]) return SYMBOLS[cmd]
    return cmd
  })

  // 3. Superscripts: ^{...} or ^x
  s = s.replace(/\^{([^{}]+)}/g, (_m, content) => convertSuperscript(content))
  s = s.replace(/\^([a-zA-Z0-9+\-=()])/g, (_m, ch) => SUPERSCRIPT[ch] ?? `^${ch}`)

  // 4. Subscripts: _{...} or _x
  s = s.replace(/_{([^{}]+)}/g, (_m, content) => convertSubscript(content))
  s = s.replace(/_([a-zA-Z0-9+\-=()])/g, (_m, ch) => SUBSCRIPT[ch] ?? `_${ch}`)

  // 5. Clean up remaining braces and backslashes
  s = s.replace(/[{}]/g, '')
  s = s.replace(/\\\\/g, '\n')
  s = s.replace(/\\/g, '')

  return s
}

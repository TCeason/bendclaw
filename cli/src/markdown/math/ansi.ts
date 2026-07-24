import { parseMath } from '@unified-latex/unified-latex-util-parse'

interface LatexArgument {
  content?: LatexNode[]
  openMark?: string
  closeMark?: string
}

interface LatexNode {
  type: string
  content?: string | LatexNode[]
  args?: LatexArgument[]
  env?: string
  escapeToken?: string
}

interface RenderedMath {
  text: string
  supported: boolean
}

const SYMBOLS: Readonly<Record<string, string>> = {
  alpha: 'α', beta: 'β', gamma: 'γ', delta: 'δ', epsilon: 'ε', varepsilon: 'ϵ',
  zeta: 'ζ', eta: 'η', theta: 'θ', vartheta: 'ϑ', iota: 'ι', kappa: 'κ',
  lambda: 'λ', mu: 'μ', nu: 'ν', xi: 'ξ', omicron: 'ο', pi: 'π', varpi: 'ϖ',
  rho: 'ρ', varrho: 'ϱ', sigma: 'σ', varsigma: 'ς', tau: 'τ', upsilon: 'υ',
  phi: 'φ', varphi: 'ϕ', chi: 'χ', psi: 'ψ', omega: 'ω',
  Gamma: 'Γ', Delta: 'Δ', Theta: 'Θ', Lambda: 'Λ', Xi: 'Ξ', Pi: 'Π',
  Sigma: 'Σ', Upsilon: 'Υ', Phi: 'Φ', Psi: 'Ψ', Omega: 'Ω',

  pm: '±', mp: '∓', times: '×', div: '÷', cdot: '·', ast: '∗', star: '⋆',
  circ: '∘', bullet: '•', oplus: '⊕', otimes: '⊗', oslash: '⊘',
  cap: '∩', cup: '∪', wedge: '∧', land: '∧', vee: '∨', lor: '∨', setminus: '∖',
  le: '≤', leq: '≤', ge: '≥', geq: '≥', ne: '≠', neq: '≠', approx: '≈',
  sim: '∼', simeq: '≃', equiv: '≡', cong: '≅', propto: '∝', ll: '≪', gg: '≫',
  in: '∈', notin: '∉', ni: '∋', subset: '⊂', supset: '⊃', subseteq: '⊆',
  supseteq: '⊇', parallel: '∥', perp: '⊥', mid: '∣', models: '⊨',
  to: '→', rightarrow: '→', gets: '←', leftarrow: '←', leftrightarrow: '↔',
  Rightarrow: '⇒', Leftarrow: '⇐', Leftrightarrow: '⇔', mapsto: '↦',
  uparrow: '↑', downarrow: '↓', updownarrow: '↕',

  sum: '∑', prod: '∏', coprod: '∐', int: '∫', iint: '∬', iiint: '∭', oint: '∮',
  partial: '∂', nabla: '∇', infinity: '∞', infty: '∞', ell: 'ℓ', hbar: 'ℏ',
  top: '⊤', bot: '⊥',
  Re: 'ℜ', Im: 'ℑ', wp: '℘', emptyset: '∅', varnothing: '∅', aleph: 'ℵ',
  forall: '∀', exists: '∃', neg: '¬', lnot: '¬', therefore: '∴', because: '∵',

  ldots: '…', cdots: '⋯', vdots: '⋮', ddots: '⋱', prime: '′', angle: '∠',
  triangle: '△', square: '□', degree: '°',
  langle: '⟨', rangle: '⟩', lceil: '⌈', rceil: '⌉', lfloor: '⌊', rfloor: '⌋',
  vert: '|', Vert: '‖', backslash: '\\',

  sin: 'sin', cos: 'cos', tan: 'tan', cot: 'cot', sec: 'sec', csc: 'csc',
  sinh: 'sinh', cosh: 'cosh', tanh: 'tanh', log: 'log', ln: 'ln', exp: 'exp',
  lim: 'lim', min: 'min', max: 'max', gcd: 'gcd', det: 'det', dim: 'dim',

  '%': '%', '#': '#', '&': '&', '$': '$', '_': '_', '{': '{', '}': '}',
}

const SPACING: Readonly<Record<string, string>> = {
  ',': ' ', ':': ' ', ';': ' ', enspace: ' ', quad: '  ', qquad: '    ',
  '!': '', left: '', right: '', displaystyle: '', textstyle: '', scriptstyle: '',
  scriptscriptstyle: '', limits: '', nolimits: '', big: '', Big: '', bigg: '', Bigg: '',
}

const WRAPPER_MACROS = new Set([
  'text', 'textrm', 'textnormal', 'mathrm', 'mathbf', 'mathit', 'mathsf', 'mathtt',
  'mathnormal', 'mathcal', 'mathbb', 'mathfrak', 'boldsymbol', 'bm', 'operatorname',
  'operatorname*', 'emph', 'mbox', 'boxed',
])

const SUPERSCRIPT: Readonly<Record<string, string>> = {
  '0': '⁰', '1': '¹', '2': '²', '3': '³', '4': '⁴', '5': '⁵', '6': '⁶',
  '7': '⁷', '8': '⁸', '9': '⁹', '+': '⁺', '-': '⁻', '=': '⁼', '(': '⁽', ')': '⁾',
  '⊤': 'ᵀ',
  a: 'ᵃ', b: 'ᵇ', c: 'ᶜ', d: 'ᵈ', e: 'ᵉ', f: 'ᶠ', g: 'ᵍ', h: 'ʰ', i: 'ⁱ',
  j: 'ʲ', k: 'ᵏ', l: 'ˡ', m: 'ᵐ', n: 'ⁿ', o: 'ᵒ', p: 'ᵖ', r: 'ʳ', s: 'ˢ',
  t: 'ᵗ', u: 'ᵘ', v: 'ᵛ', w: 'ʷ', x: 'ˣ', y: 'ʸ', z: 'ᶻ',
}

const SUBSCRIPT: Readonly<Record<string, string>> = {
  '0': '₀', '1': '₁', '2': '₂', '3': '₃', '4': '₄', '5': '₅', '6': '₆',
  '7': '₇', '8': '₈', '9': '₉', '+': '₊', '-': '₋', '=': '₌', '(': '₍', ')': '₎',
  a: 'ₐ', e: 'ₑ', h: 'ₕ', i: 'ᵢ', j: 'ⱼ', k: 'ₖ', l: 'ₗ', m: 'ₘ', n: 'ₙ',
  o: 'ₒ', p: 'ₚ', r: 'ᵣ', s: 'ₛ', t: 'ₜ', u: 'ᵤ', v: 'ᵥ', x: 'ₓ',
}

function renderNodes(nodes: readonly LatexNode[]): RenderedMath {
  let text = ''
  let supported = true
  for (let index = 0; index < nodes.length; index++) {
    const node = nodes[index]!
    const detachedGroup = nodes[index + 1]
    if (node.type === 'macro' && node.content === 'boxed' && !node.args?.length
      && detachedGroup?.type === 'group' && Array.isArray(detachedGroup.content)) {
      const body = renderNodes(detachedGroup.content)
      text += `⟦${body.text}⟧`
      supported = supported && body.supported
      index++
      continue
    }

    const rendered = renderNode(node)
    text += rendered.text
    supported = supported && rendered.supported
  }
  return { text, supported }
}

function argument(node: LatexNode, index: number): RenderedMath {
  const content = node.args?.[index]?.content
  return content ? renderNodes(content) : { text: '', supported: false }
}

function scriptCharacters(text: string, table: Readonly<Record<string, string>>): string | null {
  let converted = ''
  for (const character of text) {
    const replacement = table[character]
    if (replacement === undefined) return null
    converted += replacement
  }
  return converted
}

function grouped(text: string): string {
  return /^[\p{L}\p{N}]+$/u.test(text) ? text : `(${text})`
}

function renderArgumentLiteral(argument: LatexArgument): RenderedMath {
  const rendered = renderNodes(argument.content ?? [])
  return {
    text: `${argument.openMark ?? ''}${rendered.text}${argument.closeMark ?? ''}`,
    supported: rendered.supported,
  }
}

function renderUnknownMacro(node: LatexNode, name: string): RenderedMath {
  const args = (node.args ?? []).map(renderArgumentLiteral)
  return {
    text: `${node.escapeToken ?? '\\'}${name}${args.map(item => item.text).join('')}`,
    supported: false,
  }
}

function renderMatrix(node: LatexNode): RenderedMath {
  if (!Array.isArray(node.content) || !node.env) return { text: '', supported: false }

  const rows: LatexNode[][][] = [[]]
  let cell: LatexNode[] = []
  const flushCell = (): void => {
    rows[rows.length - 1]!.push(cell)
    cell = []
  }

  for (const child of node.content) {
    if (child.type === 'string' && child.content === '&') {
      flushCell()
      continue
    }
    if (child.type === 'macro' && child.content === '\\') {
      flushCell()
      rows.push([])
      continue
    }
    cell.push(child)
  }
  flushCell()
  if (rows[rows.length - 1]?.length === 0) rows.pop()

  let supported = true
  const renderedRows = rows.map(row => row.map(nodes => {
    const rendered = renderNodes(nodes)
    supported = supported && rendered.supported
    return rendered.text.trim()
  }))
  if (!supported) return { text: '', supported: false }

  const body = renderedRows.map(row => row.join(', ')).join('; ')
  const delimiters: Readonly<Record<string, readonly [string, string]>> = {
    matrix: ['', ''],
    bmatrix: ['[', ']'],
    pmatrix: ['(', ')'],
    Bmatrix: ['{', '}'],
    vmatrix: ['|', '|'],
    Vmatrix: ['‖', '‖'],
    array: ['[', ']'],
  }
  const [left, right] = delimiters[node.env] ?? ['', '']
  return { text: `${left}${body}${right}`, supported: true }
}

function renderMacro(node: LatexNode): RenderedMath {
  if (typeof node.content !== 'string') return { text: '', supported: false }
  const name = node.content

  if ((name === '^' || name === '_') && (node.args?.length ?? 0) > 0) {
    const body = argument(node, 0)
    if (!body.supported) return body
    const converted = scriptCharacters(body.text, name === '^' ? SUPERSCRIPT : SUBSCRIPT)
    return {
      text: converted ?? `${name}${body.text.length === 1 ? body.text : `{${body.text}}`}`,
      supported: true,
    }
  }

  const symbol = SYMBOLS[name]
  if (symbol !== undefined) return { text: symbol, supported: true }

  const spacing = SPACING[name]
  if (spacing !== undefined) return { text: spacing, supported: true }

  if (name === 'frac' || name === 'dfrac' || name === 'tfrac') {
    const numerator = argument(node, 0)
    const denominator = argument(node, 1)
    return {
      text: `${grouped(numerator.text)}/${grouped(denominator.text)}`,
      supported: numerator.supported && denominator.supported,
    }
  }

  if (name === 'sqrt') {
    const index = argument(node, 0)
    const radicand = argument(node, 1)
    const renderedIndex = index.text ? scriptCharacters(index.text, SUPERSCRIPT) : ''
    return {
      text: `${renderedIndex ?? `^(${index.text})`}√(${radicand.text})`,
      supported: radicand.supported && (index.text.length === 0 || index.supported),
    }
  }

  if (name === 'binom' || name === 'dbinom' || name === 'tbinom') {
    const top = argument(node, 0)
    const bottom = argument(node, 1)
    return {
      text: `C(${top.text}, ${bottom.text})`,
      supported: top.supported && bottom.supported,
    }
  }

  if (WRAPPER_MACROS.has(name)) {
    const args = node.args ?? []
    const bodyIndex = Math.max(0, args.length - 1)
    const body = argument(node, bodyIndex)
    if (name === 'boxed' && body.supported) return { text: `⟦${body.text}⟧`, supported: true }
    return body
  }

  if (name === '\\') return { text: ' ', supported: true }
  return renderUnknownMacro(node, name)
}

function renderNode(node: LatexNode): RenderedMath {
  switch (node.type) {
    case 'string':
      return { text: typeof node.content === 'string' ? node.content : '', supported: true }
    case 'whitespace':
    case 'parbreak':
      return { text: ' ', supported: true }
    case 'comment':
      return { text: '', supported: true }
    case 'macro':
      return renderMacro(node)
    case 'argument':
    case 'group':
    case 'inlinemath':
    case 'displaymath':
    case 'root':
      return Array.isArray(node.content)
        ? renderNodes(node.content)
        : { text: '', supported: false }
    case 'environment':
    case 'mathenv':
      return node.env && ['matrix', 'bmatrix', 'pmatrix', 'Bmatrix', 'vmatrix', 'Vmatrix', 'array'].includes(node.env)
        ? renderMatrix(node)
        : { text: '', supported: false }
    case 'verbatim':
    case 'verbatimEnvironment':
      return { text: '', supported: false }
    default:
      return { text: '', supported: false }
  }
}

/**
 * Convert common LaTeX math into terminal-safe Unicode. Unsupported syntax is
 * returned verbatim so rendering can never discard mathematical information.
 */
export function renderLatexMath(source: string): string {
  const fallback = source.trim()
  if (!fallback || fallback.length > 10_000) return fallback

  try {
    const ast = parseMath(fallback) as LatexNode[]
    const rendered = renderNodes(ast)
    if (!rendered.supported) return fallback
    const normalized = rendered.text.replace(/\s+/g, ' ').trim()
    return normalized || fallback
  } catch {
    return fallback
  }
}

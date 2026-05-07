/**
 * Dark/light theme for terminal rendering.
 *
 * All ANSI-styled text goes through `theme.<field>.paint(s)` so a theme
 * swap is a single-point change. Colors are kept narrow (two brand hues
 * + three shades of gray) to stay coherent across components.
 */

import chalk, { type ChalkInstance } from 'chalk'

export interface Style {
  paint(text: string): string
}

const plain: Style = { paint: s => s }

function style(fn: (s: string) => string): Style {
  return { paint: fn }
}

export interface Theme {
  // Inline
  text: Style
  bold: Style
  italic: Style
  boldItalic: Style
  strikethrough: Style
  underline: Style
  link: Style
  codeInline: Style

  // Headings (h1..h6)
  h1: Style
  h2: Style
  h3: Style
  h4: Style
  h5: Style
  h6: Style

  // Lists
  bullet: Style
  listNumber: Style

  // Blockquote
  blockquoteBorder: Style
  blockquoteText: Style

  // Code block
  codeBlockGutter: Style

  // Table
  tableBorder: Style
  tableHeader: Style

  // Misc
  hr: Style
  thinkBorder: Style
  thinkText: Style

  // Diff (kept for compatibility)
  addedBg: [number, number, number]
  removedBg: [number, number, number]
  addedWord: [number, number, number]
  removedWord: [number, number, number]

  // Legacy aliases kept for existing call sites
  heading: string
  inlineCode: string
  linkColor: string
}

function darkTheme(): Theme {
  const gray = chalk.hex('#808080')
  const dimGray = chalk.hex('#6a6a6a')
  return {
    text: plain,
    bold: style(s => chalk.bold(s)),
    italic: style(s => chalk.italic(s)),
    boldItalic: style(s => chalk.bold.italic(s)),
    strikethrough: style(s => chalk.dim.strikethrough(s)),
    underline: style(s => chalk.underline(s)),
    // link style follows claudecode: rely on OSC 8 for clickability and keep
    // the URL in normal colour. Fallback is a bare URL without underline/hue.
    link: plain,
    // Inline code colour mirrors claudecode's `permission` hex exactly:
    // rgb(177,185,249) = #b1b9f9 (light blue-purple). Keeps `foo()`
    // references in the same semantic family as links without dominating
    // long prose on dark terminals.
    codeInline: style(s => chalk.hex('#b1b9f9')(s)),

    // Heading style follows claudecode: h1 bold·italic·underline, h2+ plain bold.
    // Colored headings read well in demos but are noisy in long responses.
    h1: style(s => chalk.bold.italic.underline(s)),
    h2: style(s => chalk.bold(s)),
    h3: style(s => chalk.bold(s)),
    h4: style(s => chalk.bold(s)),
    h5: style(s => chalk.bold(s)),
    h6: style(s => chalk.bold(s)),

    bullet: plain,
    listNumber: plain,

    blockquoteBorder: style(s => gray(s)),
    // Italic but not dim — dimGray on dark backgrounds is nearly invisible
    // for long CJK quotes.
    blockquoteText: style(s => chalk.italic(s)),

    codeBlockGutter: style(s => gray(s)),

    tableBorder: style(s => gray(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => gray(s)),
    thinkBorder: style(s => gray(s)),
    thinkText: style(s => dimGray.italic(s)),

    addedBg: [2, 40, 0],
    removedBg: [61, 1, 0],
    addedWord: [4, 71, 0],
    removedWord: [92, 2, 0],

    heading: '#c0c0c0',
    inlineCode: '#5fb3b3',
    linkColor: 'blue',
  }
}

function lightTheme(): Theme {
  const gray = chalk.hex('#6a6a6a')
  const dimGray = chalk.hex('#8a8a8a')
  return {
    text: plain,
    bold: style(s => chalk.bold(s)),
    italic: style(s => chalk.italic(s)),
    boldItalic: style(s => chalk.bold.italic(s)),
    strikethrough: style(s => chalk.dim.strikethrough(s)),
    underline: style(s => chalk.underline(s)),
    // See darkTheme: link stays neutral and relies on OSC 8 for clickability.
    link: plain,
    // Inline code colour mirrors claudecode's `permission` hex exactly:
    // rgb(87,105,247) = #5769f7 (medium blue).
    codeInline: style(s => chalk.hex('#5769f7')(s)),

    // See darkTheme: claudecode-style headings (h1 bold·italic·underline,
    // h2+ plain bold) keep long responses calm.
    h1: style(s => chalk.bold.italic.underline(s)),
    h2: style(s => chalk.bold(s)),
    h3: style(s => chalk.bold(s)),
    h4: style(s => chalk.bold(s)),
    h5: style(s => chalk.bold(s)),
    h6: style(s => chalk.bold(s)),

    bullet: plain,
    listNumber: plain,

    blockquoteBorder: style(s => gray(s)),
    blockquoteText: style(s => chalk.italic(s)),

    codeBlockGutter: style(s => gray(s)),

    tableBorder: style(s => gray(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => gray(s)),
    thinkBorder: style(s => gray(s)),
    thinkText: style(s => dimGray.italic(s)),

    addedBg: [210, 255, 210],
    removedBg: [255, 220, 220],
    addedWord: [170, 235, 170],
    removedWord: [255, 185, 185],

    heading: '#333333',
    inlineCode: '#0d7d7d',
    linkColor: 'blue',
  }
}

function detectDarkBackground(): boolean {
  const env = process.env
  const override = env.EVOT_THEME?.toLowerCase()
  if (override === 'light') return false
  if (override === 'dark') return true
  const colorfgbg = env.COLORFGBG
  if (colorfgbg) {
    const parts = colorfgbg.split(';')
    const bg = parseInt(parts[parts.length - 1] ?? '', 10)
    if (!isNaN(bg) && bg >= 8) return false
  }
  return true
}

let cached: Theme | null = null

export function getTheme(): Theme {
  if (cached) return cached
  cached = detectDarkBackground() ? darkTheme() : lightTheme()
  return cached
}

/** Reset cached theme (for tests). */
export function resetThemeCache(): void {
  cached = null
}

/** Exported for code that only needs the chalk instance. */
export function getChalk(): ChalkInstance {
  return chalk
}

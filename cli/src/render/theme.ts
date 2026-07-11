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

  // Table
  tableBorder: Style
  tableHeader: Style

  // Misc
  hr: Style
  codeBlockBorder: Style
  thinkBorder: Style
  thinkText: Style

  // Legacy aliases kept for existing call sites
  heading: string
  inlineCode: string
  linkColor: string
}

function darkTheme(): Theme {
  // Always call chalk.hex() at paint time. Binding `const accent = chalk.hex(...)`
  // at construction freezes chalk's color-level approximation: if the theme is
  // first built while chalk.level is 0/1 (no TTY, CI), headings/fences stay stuck
  // on 16-color SGR even after log-shot forces level 3. Lazy hex matches codeInline
  // / tableBorder and keeps truecolor stable across environments.
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

    // Headings carry evot's gold accent (matches the banner + pi's mdHeading).
    // h1 keeps the extra italic·underline emphasis; h2+ are accent-bold so
    // every level reads as a distinct section marker.
    h1: style(s => chalk.hex('#f0c674').bold.italic.underline(s)),
    h2: style(s => chalk.hex('#f0c674').bold(s)),
    h3: style(s => chalk.hex('#f0c674').bold(s)),
    h4: style(s => chalk.hex('#f0c674').bold(s)),
    h5: style(s => chalk.hex('#f0c674').bold(s)),
    h6: style(s => chalk.hex('#f0c674').bold(s)),

    // Secondary accent — the teal evot uses for banner links / 'evot update'.
    // pi tints list markers with its accent; we mirror that so bullets and
    // ordinals read as structure without competing with the gold headings.
    bullet: style(s => chalk.hex('#8abeb7')(s)),
    listNumber: style(s => chalk.hex('#8abeb7')(s)),

    blockquoteBorder: style(s => chalk.hex('#808080')(s)),
    // Italic but not dim — dimGray on dark backgrounds is nearly invisible
    // for long CJK quotes.
    blockquoteText: style(s => chalk.italic(s)),

    tableBorder: style(s => chalk.hex('#8a8a8a')(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => chalk.hex('#808080')(s)),
    codeBlockBorder: style(s => chalk.hex('#6a6a6a')(s)),
    thinkBorder: style(s => chalk.hex('#808080')(s)),
    thinkText: style(s => chalk.hex('#6a6a6a').italic(s)),

    heading: '#c0c0c0',
    inlineCode: '#5fb3b3',
    linkColor: 'blue',
  }
}

function lightTheme(): Theme {
  // Same lazy-hex rule as darkTheme — see comment there.
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

    // Darker gold than the dark-theme accent so headings stay legible on a
    // light background (the #f0c674 gold washes out on white). Same warm
    // family as evot's brand accent.
    h1: style(s => chalk.hex('#b8860b').bold.italic.underline(s)),
    h2: style(s => chalk.hex('#b8860b').bold(s)),
    h3: style(s => chalk.hex('#b8860b').bold(s)),
    h4: style(s => chalk.hex('#b8860b').bold(s)),
    h5: style(s => chalk.hex('#b8860b').bold(s)),
    h6: style(s => chalk.hex('#b8860b').bold(s)),

    // See darkTheme: teal list markers. A slightly deeper teal reads better on
    // a light background than the dark-theme #8abeb7.
    bullet: style(s => chalk.hex('#5a8080')(s)),
    listNumber: style(s => chalk.hex('#5a8080')(s)),

    blockquoteBorder: style(s => chalk.hex('#6a6a6a')(s)),
    blockquoteText: style(s => chalk.italic(s)),

    tableBorder: style(s => chalk.hex('#8a8a8a')(s)),
    tableHeader: style(s => chalk.bold(s)),

    hr: style(s => chalk.hex('#6a6a6a')(s)),
    codeBlockBorder: style(s => chalk.hex('#8a8a8a')(s)),
    thinkBorder: style(s => chalk.hex('#6a6a6a')(s)),
    thinkText: style(s => chalk.hex('#8a8a8a').italic(s)),

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

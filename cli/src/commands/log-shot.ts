/**
 * /log shot — export the last assistant markdown turn as a visual snapshot
 * that matches the TUI 1:1 (wrap, tables, prefix, colors).
 *
 * Content resolution (full turn, not a single stream-flush chunk):
 *   1. in-memory history painted lines (OutputLine.text as committed) — preferred
 *   2. markdown.log `[rendered lines]` (exact TUI paint, including ANSI)
 *   3. re-render joined rawMarkdown (fallback when no painted lines exist)
 *   4. explicit messageId: the turn group containing that chunk
 *
 * Paint pipeline:
 *   preferred: history OutputLines → buildOutputBlocks → blocksToLines
 *           or markdown.log rendered lines (already final ANSI)
 *   fallback:  raw markdown → buildAssistantLines → buildOutputBlocks → blocksToLines
 *   then: ansiToHtml → self-contained HTML (+ optional Chrome PNG)
 */

import { mkdirSync, writeFileSync } from 'fs'
import { dirname, join } from 'path'
import { homedir } from 'os'
import chalk from 'chalk'
import stringWidth from 'string-width'
import stripAnsi from 'strip-ansi'
import {
  findLastAssistantPaintedTurn,
  findLastAssistantTurn,
  type AssistantMarkdownLine,
} from '../session/assistant-markdown.js'
import {
  readMarkdownTurnFile,
  type MarkdownTurn,
} from '../session/markdown-trace.js'
import { buildAssistantLines, type OutputLine } from '../render/output.js'
import { buildOutputBlocks } from '../term/viewmodel/output.js'
import { blocksToLines } from '../term/viewmodel/types.js'

/** Default terminal width used for wrap/table layout in shots. */
export const SHOT_COLUMNS = 100

/**
 * Terminal canvas colours sampled from the live TUI (slate pane, not pure black).
 * Theme text colours still come from the shared chalk/theme pipeline.
 */
export const SHOT_BG = '#3f404e'
export const SHOT_FG = '#e8e8ec'
export const SHOT_MUTED = '#91929b'
export const SHOT_BORDER = '#2e2f3a'
/** Match common macOS / Cursor integrated-terminal mono stack. */
export const SHOT_FONT =
  'Menlo, Monaco, "SF Mono", ui-monospace, "Cascadia Mono", "Segoe UI Mono", monospace'
export const SHOT_FONT_SIZE_PX = 12
export const SHOT_LINE_HEIGHT = 1.25

const graphemeSegmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

export type ShotSource =
  | { kind: 'turn'; turn: MarkdownTurn; logPath?: string }
  | {
      kind: 'history'
      rawMarkdown: string
      id?: string
      chunkCount: number
      /** Committed TUI lines (preferred 1:1 path). */
      paintedLines?: AssistantMarkdownLine[]
    }

export interface ResolveShotSourceOptions {
  markdownLogPath?: string | null
  historyLines?: readonly AssistantMarkdownLine[]
  messageId?: string
}

export interface WriteMarkdownShotOptions extends ResolveShotSourceOptions {
  outDir?: string
  /** Terminal columns for wrap/table layout. Default SHOT_COLUMNS. */
  columns?: number
  /** Try Chrome headless PNG. Default true when Chrome is found. */
  png?: boolean
  /** Open the HTML in the default browser after write. Default false. */
  open?: boolean
}

export interface MarkdownShotResult {
  source: ShotSource
  htmlPath: string
  pngPath?: string
  messageId: string
  chunkCount: number
}

/**
 * Resolve the full last assistant turn.
 *
 * Priority:
 *   - explicit messageId → that single markdown.log chunk
 *   - history painted lines → full turn after last user (preferred live path)
 *   - markdown.log → trailing stream flushes grouped by time gap
 */
export function resolveShotSource(opts: ResolveShotSourceOptions): ShotSource | null {
  const logPath = opts.markdownLogPath?.trim() || null
  const messageId = opts.messageId?.trim() || undefined

  // Explicit id always reads from the log (single chunk / turn group).
  if (messageId && logPath) {
    const turn = readMarkdownTurnFile(logPath, messageId)
    if (turn) return { kind: 'turn', turn, logPath }
  }

  // Live history: prefer committed painted lines (exact TUI wrap/layout).
  if (opts.historyLines && opts.historyLines.length > 0 && !messageId) {
    const painted = findLastAssistantPaintedTurn(opts.historyLines)
    if (painted) {
      return {
        kind: 'history',
        rawMarkdown: painted.rawMarkdown,
        id: painted.id,
        chunkCount: painted.chunkCount,
        paintedLines: painted.lines,
      }
    }
    const turn = findLastAssistantTurn(opts.historyLines)
    if (turn) {
      return {
        kind: 'history',
        rawMarkdown: turn.rawMarkdown,
        id: turn.id,
        chunkCount: turn.chunkCount,
      }
    }
  }

  // Offline / no history: group trailing log chunks into one turn.
  if (logPath) {
    const turn = readMarkdownTurnFile(logPath, messageId)
    if (turn) return { kind: 'turn', turn, logPath }
  }

  // History fallback even when messageId was requested but missing from log.
  if (opts.historyLines && opts.historyLines.length > 0) {
    const painted = findLastAssistantPaintedTurn(opts.historyLines)
    if (painted) {
      return {
        kind: 'history',
        rawMarkdown: painted.rawMarkdown,
        id: painted.id,
        chunkCount: painted.chunkCount,
        paintedLines: painted.lines,
      }
    }
    const turn = findLastAssistantTurn(opts.historyLines)
    if (turn) {
      return {
        kind: 'history',
        rawMarkdown: turn.rawMarkdown,
        id: turn.id,
        chunkCount: turn.chunkCount,
      }
    }
  }

  return null
}

function sourceRaw(source: ShotSource): string {
  return source.kind === 'turn' ? source.turn.rawMarkdown : source.rawMarkdown
}

function sourceMeta(source: ShotSource): {
  messageId: string
  lastMessageId?: string
  ts?: string
  rendererVersion?: string
  chunkCount: number
} {
  if (source.kind === 'turn') {
    return {
      messageId: source.turn.messageId,
      lastMessageId: source.turn.lastMessageId,
      ts: source.turn.ts,
      rendererVersion: source.turn.rendererVersion,
      chunkCount: source.turn.traces.length,
    }
  }
  return {
    messageId: source.id ?? 'history-last',
    chunkCount: source.chunkCount,
  }
}

/** True when the string still carries SGR color/attr codes (not just OSC zones). */
function hasSgr(s: string): boolean {
  return /\x1b\[[0-9;]*m/.test(s)
}

/** Collect markdown.log `[rendered lines]` for a turn (may be color-stripped). */
function collectRenderedLines(turn: MarkdownTurn): string[] {
  const lines = turn.traces.flatMap(t => t.renderedLines ?? [])
  while (lines.length > 0 && lines[0] === '') lines.shift()
  while (lines.length > 0 && lines[lines.length - 1] === '') lines.pop()
  return lines
}

/**
 * Join markdown.log `[rendered lines]` only when they still carry SGR colors.
 * Older logs kept OSC zone markers but stripped colors — those fall through so
 * we re-render with theme colors at the original content width.
 */
function renderedLinesAnsi(turn: MarkdownTurn): string | null {
  const lines = collectRenderedLines(turn)
  if (lines.length === 0) return null
  const joined = lines.join('\n')
  if (!hasSgr(joined)) return null
  return joined
}

/** Infer the live TUI column budget from layout-only rendered lines. */
function layoutHintColumns(turn: MarkdownTurn, fallback: number): number {
  const lines = collectRenderedLines(turn)
  if (lines.length === 0) return fallback
  return Math.max(fallback, ansiMaxColumns(lines.join('\n')))
}

/**
 * Paint committed history OutputLines through the same viewmodel path the
 * REPL history cache uses. Does NOT re-run renderMarkdown — wrap/table layout
 * already lives inside each line's `text`.
 */
export function paintHistoryLines(
  lines: readonly AssistantMarkdownLine[],
  columns: number = SHOT_COLUMNS,
): string {
  const prevLevel = chalk.level
  chalk.level = 3
  try {
    const outputLines: OutputLine[] = lines.map((l, i) => ({
      id: l.id ?? `shot-${i}`,
      kind: 'assistant',
      text: l.text ?? '',
      rawMarkdown: l.rawMarkdown,
      isContinuationSpacer: l.isContinuationSpacer,
      zoneStart: l.zoneStart,
      zoneEnd: l.zoneEnd,
    }))
    // Slim fixtures without text, or text with no SGR colors: re-render raw
    // so theme colors still appear (older sessions / chalk.level 0 commits).
    // Multi-chunk: each distinct rawMarkdown is its own flush (continuation spacer).
    const hasPaintedColor = outputLines.some(l => hasSgr(l.text))
    if (!hasPaintedColor || !outputLines.some(l => l.text.length > 0 || l.isContinuationSpacer)) {
      const raws: string[] = []
      for (const l of lines) {
        const r = l.rawMarkdown
        if (!r || !r.trim()) continue
        if (raws[raws.length - 1] !== r) raws.push(r)
      }
      if (raws.length === 0) return ''
      const rebuilt: OutputLine[] = []
      for (let i = 0; i < raws.length; i++) {
        if (i > 0) {
          rebuilt.push({
            id: `sep-${i}`,
            kind: 'assistant',
            text: '',
            isContinuationSpacer: true,
          })
        }
        rebuilt.push(...buildAssistantLines(raws[i]!))
      }
      const blocks = buildOutputBlocks(rebuilt, { columns })
      return blocksToLines(blocks).join('\n')
    }
    const blocks = buildOutputBlocks(outputLines, { columns })
    return blocksToLines(blocks).join('\n')
  } finally {
    chalk.level = prevLevel
  }
}

/**
 * Re-render each stream-flush chunk separately (with continuation spacers),
 * matching how the TUI commits multi-chunk answers — not one joined document.
 */
export function renderTurnChunksAnsi(turn: MarkdownTurn, columns: number = SHOT_COLUMNS): string {
  const prevLevel = chalk.level
  const prevColumns = process.stdout.columns
  chalk.level = 3
  try {
    Object.defineProperty(process.stdout, 'columns', {
      value: columns,
      configurable: true,
      writable: true,
      enumerable: true,
    })
    const outputLines: OutputLine[] = []
    let chunkIndex = 0
    for (const trace of turn.traces) {
      const raw = trace.rawMarkdown
      if (!raw || !raw.trim()) continue
      if (chunkIndex > 0) {
        outputLines.push({
          id: `sep-${chunkIndex}`,
          kind: 'assistant',
          text: '',
          isContinuationSpacer: true,
        })
      }
      const lines = buildAssistantLines(raw)
      outputLines.push(...lines)
      chunkIndex++
    }
    if (outputLines.length === 0) return ''
    const blocks = buildOutputBlocks(outputLines, { columns })
    return blocksToLines(blocks).join('\n')
  } finally {
    chalk.level = prevLevel
    try {
      Object.defineProperty(process.stdout, 'columns', {
        value: prevColumns,
        configurable: true,
        writable: true,
        enumerable: true,
      })
    } catch {
      /* ignore */
    }
  }
}

/**
 * Render a shot source to final TUI ANSI.
 *
 * Order:
 *   1. history painted lines (live /log shot)
 *   2. markdown.log rendered lines with SGR colors (exact prior paint)
 *   3. per-chunk re-render at layout-hint width (color-stripped logs)
 *   4. raw re-render at `columns`
 */
export function renderShotAnsi(source: ShotSource, columns: number = SHOT_COLUMNS): string {
  if (source.kind === 'history' && source.paintedLines && source.paintedLines.length > 0) {
    return paintHistoryLines(source.paintedLines, columns)
  }
  if (source.kind === 'turn') {
    const painted = renderedLinesAnsi(source.turn)
    if (painted) return painted
    // Color-stripped log: re-render each flush chunk with theme colors at the
    // original content width. Per-chunk (not joined) matches TUI multi-⏺ layout.
    const width = layoutHintColumns(source.turn, columns)
    return renderTurnChunksAnsi(source.turn, width)
  }
  return renderAssistantAnsi(sourceRaw(source), columns)
}

/**
 * Render raw markdown through the TUI paint path:
 *   renderMarkdown (shared) → buildAssistantLines → buildOutputBlocks → chalk ANSI
 *
 * Also pins `process.stdout.columns` so table/wrap math inside the markdown
 * renderer (which reads stdout width) matches the shot column budget.
 */
export function renderAssistantAnsi(rawMarkdown: string, columns: number = SHOT_COLUMNS): string {
  const prevLevel = chalk.level
  const prevColumns = process.stdout.columns
  // chalk.Level: 0 none · 1 basic · 2 256 · 3 truecolor
  chalk.level = 3
  try {
    Object.defineProperty(process.stdout, 'columns', {
      value: columns,
      configurable: true,
      writable: true,
      enumerable: true,
    })
    const lines = buildAssistantLines(rawMarkdown)
    if (lines.length === 0) return ''
    const blocks = buildOutputBlocks(lines, { columns })
    return blocksToLines(blocks).join('\n')
  } finally {
    chalk.level = prevLevel
    try {
      Object.defineProperty(process.stdout, 'columns', {
        value: prevColumns,
        configurable: true,
        writable: true,
        enumerable: true,
      })
    } catch {
      /* ignore restore failure */
    }
  }
}

/**
 * Wrap the TUI ANSI paint in a terminal-like HTML canvas.
 * Markdown/table layout is NOT reimplemented — only ANSI → HTML with terminal metrics.
 */
/** Max visible terminal columns in an ANSI paint (for canvas sizing). */
export function ansiMaxColumns(ansi: string): number {
  let max = 0
  for (const line of ansi.split('\n')) {
    const w = stringWidth(stripAnsi(line))
    if (w > max) max = w
  }
  return max
}

export function buildShotHtml(source: ShotSource, opts?: { columns?: number }): string {
  const meta = sourceMeta(source)
  const columns = opts?.columns ?? SHOT_COLUMNS
  const ansi = renderShotAnsi(source, columns)
  // Painted log lines may be wider than the current column budget (they were
  // laid out at the live terminal width). Size the canvas to the content.
  const contentCols = Math.max(columns, ansiMaxColumns(ansi))
  const bodyHtml = `<pre class="term">${ansiToHtml(ansi)}</pre>`
  const idLabel = meta.lastMessageId && meta.lastMessageId !== meta.messageId
    ? `${meta.messageId}…${meta.lastMessageId}`
    : meta.messageId

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>evot shot · ${escapeHtml(idLabel)}</title>
<style>
  :root {
    color-scheme: dark;
    --bg: ${SHOT_BG};
    --fg: ${SHOT_FG};
    --muted: ${SHOT_MUTED};
    --border: ${SHOT_BORDER};
    --accent: #f0c674;
    --cell: 1ch;
  }
  * { box-sizing: border-box; }
  html, body {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
  }
  body {
    /*
     * Same mono family terminals commonly use (Menlo on macOS).
     * Do NOT put dual-width CJK fonts first — that swaps the whole
     * document into a different glyph set from the live TUI.
     */
    font-family: ${SHOT_FONT};
    font-size: ${SHOT_FONT_SIZE_PX}px;
    line-height: ${SHOT_LINE_HEIGHT};
    -webkit-font-smoothing: antialiased;
    -moz-osx-font-smoothing: grayscale;
    font-variant-ligatures: none;
    font-feature-settings: "liga" 0, "calt" 0;
    font-kerning: none;
    text-rendering: optimizeLegibility;
  }
  header {
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--muted);
    font-size: 11px;
    display: flex;
    gap: 12px;
    flex-wrap: wrap;
  }
  header strong { color: var(--accent); font-weight: 600; }
  main {
    padding: 12px 14px 24px;
    /* Column budget the TUI used for tables/wrap (+ small pad). */
    width: ${Math.max(48, contentCols + 4)}ch;
    max-width: none;
  }
  pre.term {
    margin: 0;
    /* Never soft-wrap: wrapping a box-drawing row shatters table grids. */
    white-space: pre;
    tab-size: 4;
    font: inherit;
    line-height: inherit;
    overflow-x: auto;
    /*
     * ASCII / box-drawing advance comes from the mono font itself (same as
     * the terminal). Only wide (CJK/emoji) glyphs are width-forced below.
     */
  }
  /*
   * Wide graphemes: TUI measures them as 2 columns via string-width.
   * Force a 2-cell advance so table borders stay aligned even when the
   * browser's CJK fallback is not dual-width mono.
   */
  pre.term .w2 {
    display: inline-block;
    width: calc(var(--cell) * 2);
    text-align: center;
    vertical-align: baseline;
    white-space: pre;
  }
</style>
</head>
<body>
<header>
  <span><strong>evot shot</strong></span>
  <span>id: ${escapeHtml(idLabel)}</span>
  <span>chunks: ${meta.chunkCount}</span>
  <span>cols: ${columns}</span>
  ${meta.rendererVersion ? `<span>renderer: ${escapeHtml(meta.rendererVersion)}</span>` : ''}
  ${meta.ts ? `<span>ts: ${escapeHtml(meta.ts)}</span>` : ''}
</header>
<main>
${bodyHtml}
</main>
</body>
</html>
`
}

export async function writeMarkdownShot(opts: WriteMarkdownShotOptions): Promise<MarkdownShotResult> {
  const source = resolveShotSource(opts)
  if (!source) {
    throw new Error('No assistant markdown found to shoot (no markdown.log entry and no history).')
  }

  const meta = sourceMeta(source)
  const outDir = opts.outDir ?? join(homedir(), '.evotai', 'shots')
  mkdirSync(outDir, { recursive: true })

  const stamp = timestampSlug()
  const safeId = meta.messageId.replace(/[^a-zA-Z0-9._-]+/g, '_').slice(0, 48)
  const base = `shot-${stamp}-${safeId}`
  const htmlPath = join(outDir, `${base}.html`)
  const columns = opts.columns ?? SHOT_COLUMNS
  const html = buildShotHtml(source, { columns })
  writeFileSync(htmlPath, html, { mode: 0o600 })

  let pngPath: string | undefined
  if (opts.png !== false) {
    // Estimate size from the painted ANSI so wide TUI turns are not clipped.
    const ansi = renderShotAnsi(source, columns)
    const contentCols = Math.max(columns, ansiMaxColumns(ansi))
    const lineCount = Math.max(1, ansi.split('\n').length)
    const height = Math.min(16000, Math.max(900, 120 + lineCount * 18))
    const shot = await tryChromeScreenshot(htmlPath, join(outDir, `${base}.png`), {
      width: Math.max(720, contentCols * 9 + 48),
      height,
    })
    if (shot) pngPath = shot
  }

  if (opts.open) {
    await tryOpen(htmlPath)
  }

  return {
    source,
    htmlPath,
    pngPath,
    messageId: meta.messageId,
    chunkCount: meta.chunkCount,
  }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
}

function hex2(n: number): string {
  return Math.max(0, Math.min(255, n | 0)).toString(16).padStart(2, '0')
}

/** xterm 256-color palette → #rrggbb (standard cube + grayscale). */
function xterm256ToHex(n: number): string {
  const i = Math.max(0, Math.min(255, n | 0))
  if (i < 16) {
    const basic = [
      '#000000', '#800000', '#008000', '#808000', '#000080', '#800080', '#008080', '#c0c0c0',
      '#808080', '#ff0000', '#00ff00', '#ffff00', '#0000ff', '#ff00ff', '#00ffff', '#ffffff',
    ]
    return basic[i]!
  }
  if (i < 232) {
    const v = i - 16
    const r = Math.floor(v / 36)
    const g = Math.floor((v % 36) / 6)
    const b = v % 6
    const steps = [0, 95, 135, 175, 215, 255]
    return `#${hex2(steps[r]!)}${hex2(steps[g]!)}${hex2(steps[b]!)}`
  }
  const gray = 8 + (i - 232) * 10
  return `#${hex2(gray)}${hex2(gray)}${hex2(gray)}`
}

interface SgrState {
  bold: boolean
  dim: boolean
  italic: boolean
  underline: boolean
  strike: boolean
  inverse: boolean
  fg?: string
  bg?: string
}

function emptySgr(): SgrState {
  return {
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    strike: false,
    inverse: false,
  }
}

function sgrToStyle(s: SgrState): string {
  const parts: string[] = []
  if (s.bold) parts.push('font-weight:700')
  if (s.dim) parts.push('opacity:0.72')
  if (s.italic) parts.push('font-style:italic')
  if (s.underline) parts.push('text-decoration:underline')
  if (s.strike) parts.push('text-decoration:line-through')
  if (s.inverse) {
    parts.push(`color:${s.bg ?? SHOT_BG}`)
    parts.push(`background:${s.fg ?? SHOT_FG}`)
  } else {
    if (s.fg) parts.push(`color:${s.fg}`)
    if (s.bg) parts.push(`background:${s.bg}`)
  }
  return parts.join(';')
}

function applySgrCodes(state: SgrState, codes: number[]): void {
  let i = 0
  while (i < codes.length) {
    const code = codes[i]!
    i++
    if (code === 0) {
      Object.assign(state, emptySgr())
      state.fg = undefined
      state.bg = undefined
    } else if (code === 1) {
      state.bold = true
    } else if (code === 2) {
      state.dim = true
    } else if (code === 3) {
      state.italic = true
    } else if (code === 4) {
      state.underline = true
    } else if (code === 7) {
      state.inverse = true
    } else if (code === 9) {
      state.strike = true
    } else if (code === 22) {
      state.bold = false
      state.dim = false
    } else if (code === 23) {
      state.italic = false
    } else if (code === 24) {
      state.underline = false
    } else if (code === 27) {
      state.inverse = false
    } else if (code === 29) {
      state.strike = false
    } else if (code === 39) {
      state.fg = undefined
    } else if (code === 49) {
      state.bg = undefined
    } else if (code === 38 || code === 48) {
      const isFg = code === 38
      const mode = codes[i]
      if (mode === 2 && i + 3 < codes.length) {
        const r = codes[i + 1]!
        const g = codes[i + 2]!
        const b = codes[i + 3]!
        i += 4
        const hex = `#${hex2(r)}${hex2(g)}${hex2(b)}`
        if (isFg) state.fg = hex
        else state.bg = hex
      } else if (mode === 5 && i + 1 < codes.length) {
        const n = codes[i + 1]!
        i += 2
        const hex = xterm256ToHex(n)
        if (isFg) state.fg = hex
        else state.bg = hex
      } else if (mode !== undefined) {
        i++
      }
    } else if (code >= 30 && code <= 37) {
      // Basic ANSI palette tuned for the slate TUI canvas (not pure black).
      const map = ['#4e4e4e', '#ff6b6b', '#69c36c', '#e5c07b', '#61afef', '#c678dd', '#56b6c2', '#e8e8ec']
      state.fg = map[code - 30]
    } else if (code >= 90 && code <= 97) {
      const map = ['#91929b', '#ff8787', '#89d185', '#f0c674', '#79c0ff', '#d2a8ff', '#56d4dd', '#ffffff']
      state.fg = map[code - 90]
    } else if (code >= 40 && code <= 47) {
      const map = ['#4e4e4e', '#ff6b6b', '#69c36c', '#e5c07b', '#61afef', '#c678dd', '#56b6c2', '#e8e8ec']
      state.bg = map[code - 40]
    } else if (code >= 100 && code <= 107) {
      const map = ['#91929b', '#ff8787', '#89d185', '#f0c674', '#79c0ff', '#d2a8ff', '#56d4dd', '#ffffff']
      state.bg = map[code - 100]
    }
  }
}

/**
 * CSI SGR → HTML with truecolor / 256 / 16-color support.
 *
 * Width-1 glyphs (ASCII, box-drawing, spaces) keep the mono font's natural
 * advance — same as the terminal — so borders stay continuous and Latin
 * text matches Menlo metrics. Only wide graphemes (CJK/emoji, string-width
 * ≥ 2) get a forced 2ch cell so table columns still line up when the
 * browser's CJK fallback is not dual-width.
 *
 * Markdown/table layout is NOT reimplemented here — only terminal metrics.
 * OSC (e.g. 133 zone markers) and other non-SGR escapes are dropped.
 */
export function ansiToHtml(input: string): string {
  let out = ''
  let i = 0
  const state = emptySgr()
  let pendingStyle = ''

  // Batched width-1 run under the current style (natural mono advance).
  let runText = ''

  const noteStyle = () => {
    pendingStyle = sgrToStyle(state)
  }

  const flushRun = () => {
    if (!runText) return
    if (pendingStyle) {
      out += `<span style="${pendingStyle}">${escapeHtml(runText)}</span>`
    } else {
      out += escapeHtml(runText)
    }
    runText = ''
  }

  const writeGrapheme = (segment: string) => {
    if (!segment) return
    if (segment === '\n') {
      flushRun()
      out += '\n'
      return
    }
    if (segment === '\r') return
    const w = stringWidth(segment)
    if (w <= 0) return
    if (w >= 2) {
      flushRun()
      const styleAttr = pendingStyle ? ` style="${pendingStyle}"` : ''
      out += `<span class="w2"${styleAttr}>${escapeHtml(segment)}</span>`
      return
    }
    runText += segment
  }

  const writeVisibleRun = (text: string) => {
    if (!text) return
    for (const { segment } of graphemeSegmenter.segment(text)) {
      writeGrapheme(segment)
    }
  }

  noteStyle()

  while (i < input.length) {
    const ch = input[i]!

    // OSC: ESC ] ... BEL or ESC ] ... ESC \
    if (ch === '\x1b' && input[i + 1] === ']') {
      flushRun()
      i += 2
      while (i < input.length) {
        if (input[i] === '\x07') { i++; break }
        if (input[i] === '\x1b' && input[i + 1] === '\\') { i += 2; break }
        i++
      }
      continue
    }

    // CSI: ESC [ ... final-byte
    if (ch === '\x1b' && input[i + 1] === '[') {
      i += 2
      let params = ''
      while (i < input.length) {
        const c = input[i]!
        if (c >= '@' && c <= '~') {
          const final = c
          i++
          if (final === 'm') {
            flushRun() // style change ends the current run
            const codes = params === ''
              ? [0]
              : params.split(';').map(p => {
                const n = Number(p)
                return Number.isFinite(n) ? n : 0
              })
            applySgrCodes(state, codes)
            noteStyle()
          }
          break
        }
        params += c
        i++
      }
      continue
    }

    if (ch === '\x1b') {
      flushRun()
      i += 2
      continue
    }

    let j = i + 1
    while (j < input.length && input[j] !== '\x1b') j++
    writeVisibleRun(input.slice(i, j))
    i = j
  }
  flushRun()
  return out
}

function timestampSlug(): string {
  const d = new Date()
  const p = (n: number, w = 2) => n.toString().padStart(w, '0')
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
}

async function tryChromeScreenshot(
  htmlPath: string,
  pngPath: string,
  size: { width: number; height: number },
): Promise<string | undefined> {
  const chrome = resolveChromeBinary()
  if (!chrome) return undefined
  try {
    mkdirSync(dirname(pngPath), { recursive: true })
    const fileUrl = pathToFileUrl(htmlPath)
    const proc = Bun.spawn(
      [
        chrome,
        '--headless=new',
        '--disable-gpu',
        '--hide-scrollbars',
        '--force-device-scale-factor=2',
        `--screenshot=${pngPath}`,
        `--window-size=${size.width},${size.height}`,
        fileUrl,
      ],
      { stdout: 'ignore', stderr: 'ignore' },
    )
    const code = await proc.exited
    if (code !== 0) return undefined
    return pngPath
  } catch {
    return undefined
  }
}

function resolveChromeBinary(): string | null {
  const candidates = [
    process.env.EVOT_CHROME,
    process.env.CHROME_PATH,
    '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
    '/Applications/Chromium.app/Contents/MacOS/Chromium',
    'google-chrome',
    'google-chrome-stable',
    'chromium',
    'chromium-browser',
  ]
  for (const c of candidates) {
    if (!c) continue
    return c
  }
  return null
}

function pathToFileUrl(p: string): string {
  const abs = p.startsWith('/') ? p : join(process.cwd(), p)
  return 'file://' + abs.split('/').map(encodeURIComponent).join('/').replace(/%3A/g, ':')
}

async function tryOpen(path: string): Promise<void> {
  try {
    const opener =
      process.platform === 'darwin' ? 'open'
        : process.platform === 'win32' ? 'cmd'
          : 'xdg-open'
    const args = process.platform === 'win32' ? ['/c', 'start', '', path] : [path]
    const proc = Bun.spawn([opener, ...args], { stdout: 'ignore', stderr: 'ignore' })
    await proc.exited
  } catch {
    /* ignore */
  }
}

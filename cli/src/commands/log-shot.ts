/**
 * /log shot — export the last assistant markdown turn as a visual snapshot
 * that matches the TUI 1:1 (wrap, tables, prefix, colors).
 *
 * Content resolution (full turn, not a single stream-flush chunk):
 *   1. in-memory history painted lines (OutputLine.text as committed) — preferred
 *   2. re-render joined rawMarkdown (fallback when no painted lines exist,
 *      e.g. slim resume fixtures without painted text)
 *
 * Paint pipeline:
 *   preferred: history OutputLines → buildOutputBlocks → blocksToLines
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
/** Header block + main top padding used when sizing the PNG viewport. */
export const SHOT_CHROME_PX = 120
/** Approximate CSS px per terminal row (font-size × line-height + fudge). */
export const SHOT_ROW_PX = Math.ceil(SHOT_FONT_SIZE_PX * SHOT_LINE_HEIGHT) + 2
/** Extra CSS px under the last content line in the PNG (tight crop + breathing room). */
export const SHOT_BOTTOM_PAD_PX = 20

const graphemeSegmenter = new Intl.Segmenter(undefined, { granularity: 'grapheme' })

export interface ShotSource {
  rawMarkdown: string
  id?: string
  chunkCount: number
  /** Committed TUI lines (preferred 1:1 path). */
  paintedLines?: AssistantMarkdownLine[]
}

export interface ResolveShotSourceOptions {
  historyLines?: readonly AssistantMarkdownLine[]
}

/** Optional session/context fields shown in the shot HTML header. */
export interface ShotHeaderMeta {
  model?: string
  thinkingLevel?: string
  sessionId?: string
  cwd?: string
  /** Optional git branch, shown next to cwd when present. */
  branch?: string
}

export interface WriteMarkdownShotOptions extends ResolveShotSourceOptions {
  outDir?: string
  /** Terminal columns for wrap/table layout. Default SHOT_COLUMNS. */
  columns?: number
  /** Try Chrome headless PNG. Default true when Chrome is found. */
  png?: boolean
  /** Open the HTML in the default browser after write. Default false. */
  open?: boolean
  /** Extra header fields (model, thinking, session, cwd). */
  header?: ShotHeaderMeta
}

export interface MarkdownShotResult {
  source: ShotSource
  htmlPath: string
  pngPath?: string
  messageId: string
  chunkCount: number
}

/**
 * Resolve the full last assistant turn from committed history: painted lines
 * (exact TUI wrap/layout) preferred, joined raw markdown as fallback.
 */
export function resolveShotSource(opts: ResolveShotSourceOptions): ShotSource | null {
  if (!opts.historyLines || opts.historyLines.length === 0) return null

  const painted = findLastAssistantPaintedTurn(opts.historyLines)
  if (painted) {
    return {
      rawMarkdown: painted.rawMarkdown,
      id: painted.id,
      chunkCount: painted.chunkCount,
      paintedLines: painted.lines,
    }
  }
  const turn = findLastAssistantTurn(opts.historyLines)
  if (turn) {
    return {
      rawMarkdown: turn.rawMarkdown,
      id: turn.id,
      chunkCount: turn.chunkCount,
    }
  }
  return null
}

function sourceMeta(source: ShotSource): { messageId: string; chunkCount: number } {
  return {
    messageId: source.id ?? 'history-last',
    chunkCount: source.chunkCount,
  }
}

/** True when the string still carries SGR color/attr codes (not just OSC zones). */
function hasSgr(s: string): boolean {
  return /\x1b\[[0-9;]*m/.test(s)
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
 * Render a shot source to final TUI ANSI: history painted lines when present
 * (live /log shot), otherwise a raw re-render at `columns`.
 */
export function renderShotAnsi(source: ShotSource, columns: number = SHOT_COLUMNS): string {
  if (source.paintedLines && source.paintedLines.length > 0) {
    return paintHistoryLines(source.paintedLines, columns)
  }
  return renderAssistantAnsi(source.rawMarkdown, columns)
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

/** Format model · thinking for shot headers (provider intentionally omitted). */
export function formatShotModelLabel(header?: ShotHeaderMeta): string {
  if (!header) return ''
  const model = header.model?.trim() ?? ''
  const thinking = header.thinkingLevel?.trim() ?? ''
  if (!model && !thinking) return ''
  if (!thinking) return model
  const level = thinking === 'off' ? 'thinking off' : thinking
  return model ? `${model} · ${level}` : level
}

function shortenHomePath(path: string): string {
  const home = process.env.HOME || process.env.USERPROFILE || ''
  if (home && path.startsWith(home)) return '~' + path.slice(home.length)
  return path
}

/** Compact local time for the header (e.g. 2026-07-14 11:15). */
export function formatShotTimestamp(isoOrLogTs?: string, now = new Date()): string {
  const d = isoOrLogTs ? new Date(isoOrLogTs.replace(' ', 'T')) : now
  if (Number.isNaN(d.getTime())) {
    // Log timestamps may already be "YYYY-MM-DD HH:mm:ss.sss"
    const m = isoOrLogTs?.match(/^(\d{4}-\d{2}-\d{2})[ T](\d{2}:\d{2})/)
    if (m) return `${m[1]} ${m[2]}`
    return ''
  }
  const p = (n: number) => n.toString().padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
}

/**
 * Optional second header line (only when needed).
 * Time lives on the hero row; cwd/workspace is intentionally omitted from shares.
 */
export function buildShotMetaLine(args: {
  header?: ShotHeaderMeta
  chunkCount: number
  ts?: string
}): string {
  if (args.chunkCount > 1) return `${args.chunkCount} chunks`
  return ''
}

/** Timestamp shown on the hero row next to the model. */
export function buildShotHeroTime(ts?: string): string {
  return formatShotTimestamp(ts)
}

/** One-line footer: how this image was produced. */
export function buildShotFooterLine(): string {
  return 'Generated with evot  ·  type /log shot in a session'
}

/** @deprecated use buildShotMetaLine — kept for older tests. */
export function buildShotMetaChips(args: {
  header?: ShotHeaderMeta
  idLabel: string
  chunkCount: number
  columns: number
  rendererVersion?: string
  ts?: string
}): string[] {
  const line = buildShotMetaLine({
    header: args.header,
    chunkCount: args.chunkCount,
    ts: args.ts,
  })
  return line ? [escapeHtml(line)] : []
}

/** @deprecated use formatShotModelLabel — kept for tests that only need model chip text. */
export function buildShotHeaderSpans(header?: ShotHeaderMeta): string[] {
  const modelLabel = formatShotModelLabel(header)
  if (!modelLabel) return []
  return [`<span class="model">${escapeHtml(modelLabel)}</span>`]
}

export function buildShotHtml(
  source: ShotSource,
  opts?: { columns?: number; header?: ShotHeaderMeta },
): string {
  const meta = sourceMeta(source)
  const columns = opts?.columns ?? SHOT_COLUMNS
  const ansi = renderShotAnsi(source, columns)
  // Painted log lines may be wider than the current column budget (they were
  // laid out at the live terminal width). Size the canvas to the content.
  const contentCols = Math.max(columns, ansiMaxColumns(ansi))
  const bodyHtml = `<pre class="term">${ansiToHtml(ansi)}</pre>`
  const idLabel = meta.messageId
  const modelLabel = formatShotModelLabel(opts?.header)
  const titleBits = ['evot shot', modelLabel, idLabel].filter(Boolean)
  const metaLine = buildShotMetaLine({
    header: opts?.header,
    chunkCount: meta.chunkCount,
  })
  // The shot renders the live turn, so "now" is the honest capture time.
  const heroTime = buildShotHeroTime(new Date().toISOString())
  const canvasCh = Math.max(48, contentCols + 4)

  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>${escapeHtml(titleBits.join(' · '))}</title>
<style>
  :root {
    color-scheme: dark;
    --bg: ${SHOT_BG};
    --fg: ${SHOT_FG};
    --muted: ${SHOT_MUTED};
    --border: ${SHOT_BORDER};
    --accent: #f0c674;
    --accent-soft: rgba(240, 198, 116, 0.12);
    --rule: rgba(240, 198, 116, 0.35);
    --header-bg: #353644;
    --cell: 1ch;
  }
  * { box-sizing: border-box; }
  html, body {
    margin: 0;
    background: var(--bg);
    color: var(--fg);
    /* Shrink-wrap to the frame so PNG width tracks content, not the Chrome window. */
    width: fit-content;
    min-width: min(100%, ${canvasCh}ch);
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
  .shot-frame {
    width: ${canvasCh}ch;
    max-width: none;
  }
  header.shot-header {
    padding: 14px 16px 12px;
    background:
      linear-gradient(180deg, var(--header-bg) 0%, var(--bg) 100%);
    border-left: 3px solid var(--accent);
  }
  .shot-hero {
    display: flex;
    align-items: baseline;
    flex-wrap: nowrap;
    gap: 10px 14px;
  }
  .shot-brand {
    color: var(--accent);
    font-weight: 700;
    font-size: 12px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }
  .shot-model {
    color: var(--fg);
    font-weight: 600;
    font-size: 13px;
    background: var(--accent-soft);
    border: 1px solid rgba(240, 198, 116, 0.28);
    border-radius: 999px;
    padding: 2px 10px;
    white-space: nowrap;
  }
  .shot-time {
    color: var(--muted);
    font-size: 11px;
    white-space: nowrap;
  }
  .shot-meta {
    margin-top: 8px;
    color: var(--muted);
    font-size: 11px;
  }
  /* Hairline rule — single soft edge, inset to the content padding. */
  .shot-rule {
    height: 1px;
    border: 0;
    margin: 0 16px;
    background: linear-gradient(
      90deg,
      transparent 0%,
      rgba(255, 255, 255, 0.04) 6%,
      rgba(232, 232, 236, 0.14) 50%,
      rgba(255, 255, 255, 0.04) 94%,
      transparent 100%
    );
  }
  main {
    padding: 14px 16px 18px;
  }
  pre.term {
    margin: 0;
    /* Never soft-wrap: wrapping a box-drawing row shatters table grids. */
    white-space: pre;
    tab-size: 4;
    font: inherit;
    line-height: inherit;
    overflow-x: auto;
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
  footer.shot-footer {
    padding: 10px 16px 14px;
    color: var(--muted);
    font-size: 11px;
    letter-spacing: 0.01em;
  }
  footer.shot-footer .cmd {
    color: var(--accent);
    font-weight: 600;
  }
</style>
</head>
<body>
<div class="shot-frame">
<header class="shot-header">
  <div class="shot-hero">
    <span class="shot-brand">evot shot</span>
    ${modelLabel ? `<span class="shot-model">${escapeHtml(modelLabel)}</span>` : ''}
    ${heroTime ? `<span class="shot-time">${escapeHtml(heroTime)}</span>` : ''}
  </div>
  ${metaLine ? `<div class="shot-meta">${escapeHtml(metaLine)}</div>` : ''}
</header>
<hr class="shot-rule" />
<main>
${bodyHtml}
</main>
<hr class="shot-rule" />
<footer class="shot-footer">
  Generated with <span class="cmd">evot</span>
  · type <span class="cmd">/log shot</span> in a session
</footer>
</div>
</body>
</html>
`
}

export async function writeMarkdownShot(opts: WriteMarkdownShotOptions): Promise<MarkdownShotResult> {
  const source = resolveShotSource(opts)
  if (!source) {
    throw new Error('No assistant markdown found to shoot in committed history.')
  }

  const meta = sourceMeta(source)
  const outDir = opts.outDir ?? join(homedir(), '.evotai', 'shots')
  mkdirSync(outDir, { recursive: true })

  const stamp = timestampSlug()
  const safeId = meta.messageId.replace(/[^a-zA-Z0-9._-]+/g, '_').slice(0, 48)
  const base = `shot-${stamp}-${safeId}`
  const htmlPath = join(outDir, `${base}.html`)
  const columns = opts.columns ?? SHOT_COLUMNS
  const html = buildShotHtml(source, { columns, header: opts.header })
  writeFileSync(htmlPath, html, { mode: 0o600 })

  let pngPath: string | undefined
  if (opts.png !== false) {
    // Width is estimated from content; height is measured from the live DOM via
    // CDP so the PNG ends at the last painted pixel (no empty bottom band).
    const ansi = renderShotAnsi(source, columns)
    const contentCols = Math.max(columns, ansiMaxColumns(ansi))
    const lineCount = Math.max(1, ansi.replace(/\n+$/, '').split('\n').length)
    const size = shotWindowSize(contentCols, lineCount)
    const shot = await tryChromeScreenshot(htmlPath, join(outDir, `${base}.png`), size)
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

/**
 * Initial Chrome window size before CDP measures the real document height.
 * Width is final; height is only a bootstrap (must be tall enough to avoid
 * early layout collapse, but the PNG clip uses measured scrollHeight).
 */
export function shotWindowSize(
  contentCols: number,
  lineCount: number,
): { width: number; height: number } {
  const cols = Number.isFinite(contentCols) ? Math.max(1, Math.floor(contentCols)) : SHOT_COLUMNS
  const lines = Number.isFinite(lineCount) ? Math.max(1, Math.floor(lineCount)) : 1
  const width = Math.max(480, cols * 9 + 48)
  // Bootstrap viewport: slightly above content estimate so fonts/layout settle,
  // but never a fixed 900px floor that leaves empty PNG bands.
  const height = Math.min(16000, Math.max(200, SHOT_CHROME_PX + lines * SHOT_ROW_PX))
  return { width, height }
}

function timestampSlug(): string {
  const d = new Date()
  const p = (n: number, w = 2) => n.toString().padStart(w, '0')
  return `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}-${p(d.getHours())}${p(d.getMinutes())}${p(d.getSeconds())}`
}

/**
 * Capture a content-tight PNG via Chrome DevTools Protocol.
 * `--screenshot` always fills the whole window; CDP clips to scrollHeight so
 * short documents do not keep a blank band under the last line.
 */
async function tryChromeScreenshot(
  htmlPath: string,
  pngPath: string,
  size: { width: number; height: number },
): Promise<string | undefined> {
  const chrome = resolveChromeBinary()
  if (!chrome) return undefined
  mkdirSync(dirname(pngPath), { recursive: true })
  const fileUrl = pathToFileUrl(htmlPath)
  const port = 9200 + Math.floor(Math.random() * 1000)
  const userDataDir = join(dirname(pngPath), `.chrome-shot-${process.pid}-${port}`)
  mkdirSync(userDataDir, { recursive: true })

  const proc = Bun.spawn(
    [
      chrome,
      '--headless=new',
      '--disable-gpu',
      '--hide-scrollbars',
      '--no-first-run',
      '--no-default-browser-check',
      `--user-data-dir=${userDataDir}`,
      `--remote-debugging-port=${port}`,
      `--window-size=${size.width},${Math.max(size.height, 600)}`,
      '--force-device-scale-factor=1',
      'about:blank',
    ],
    { stdout: 'ignore', stderr: 'pipe' },
  )

  try {
    const wsUrl = await waitForChromeWs(port, 8000)
    if (!wsUrl) return undefined
    const ok = await captureViaCdp(wsUrl, fileUrl, pngPath, size.width)
    return ok ? pngPath : undefined
  } catch {
    return undefined
  } finally {
    proc.kill()
    try { await proc.exited } catch { /* ignore */ }
    try {
      const { rmSync } = await import('fs')
      rmSync(userDataDir, { recursive: true, force: true })
    } catch { /* ignore cleanup */ }
  }
}

async function waitForChromeWs(port: number, timeoutMs: number): Promise<string | null> {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const res = await fetch(`http://127.0.0.1:${port}/json/version`)
      if (res.ok) {
        const body = await res.json() as { webSocketDebuggerUrl?: string }
        if (body.webSocketDebuggerUrl) return body.webSocketDebuggerUrl
      }
    } catch { /* chrome still starting */ }
    await Bun.sleep(50)
  }
  return null
}

async function captureViaCdp(
  browserWsUrl: string,
  fileUrl: string,
  pngPath: string,
  cssWidth: number,
): Promise<boolean> {
  // Open a dedicated page target so we do not fight the about:blank default.
  const listRes = await fetch(browserWsUrl.replace('ws://', 'http://').replace(/\/devtools\/browser.*/, '') + '/json/new?' + encodeURIComponent(fileUrl), {
    method: 'PUT',
  }).catch(() => null)

  let pageWsUrl: string | null = null
  if (listRes && listRes.ok) {
    const page = await listRes.json() as { webSocketDebuggerUrl?: string }
    pageWsUrl = page.webSocketDebuggerUrl ?? null
  }
  if (!pageWsUrl) {
    // Fallback: list pages and use the first page target.
    const base = browserWsUrl.replace('ws://', 'http://').replace(/\/devtools\/browser.*/, '')
    const pagesRes = await fetch(`${base}/json/list`)
    const pages = await pagesRes.json() as Array<{ type?: string; webSocketDebuggerUrl?: string; url?: string }>
    const page = pages.find(p => p.type === 'page' && p.webSocketDebuggerUrl)
    pageWsUrl = page?.webSocketDebuggerUrl ?? null
  }
  if (!pageWsUrl) return false

  const ws = new WebSocket(pageWsUrl)
  await new Promise<void>((resolve, reject) => {
    const t = setTimeout(() => reject(new Error('cdp connect timeout')), 5000)
    ws.onopen = () => { clearTimeout(t); resolve() }
    ws.onerror = () => { clearTimeout(t); reject(new Error('cdp connect error')) }
  })

  let nextId = 1
  const pending = new Map<number, { resolve: (v: unknown) => void; reject: (e: Error) => void }>()
  ws.onmessage = (ev) => {
    try {
      const msg = JSON.parse(String(ev.data)) as { id?: number; result?: unknown; error?: { message?: string } }
      if (msg.id == null) return
      const p = pending.get(msg.id)
      if (!p) return
      pending.delete(msg.id)
      if (msg.error) p.reject(new Error(msg.error.message ?? 'cdp error'))
      else p.resolve(msg.result)
    } catch { /* ignore */ }
  }

  const send = (method: string, params?: Record<string, unknown>) =>
    new Promise<unknown>((resolve, reject) => {
      const id = nextId++
      pending.set(id, { resolve, reject })
      ws.send(JSON.stringify({ id, method, params }))
      setTimeout(() => {
        if (pending.has(id)) {
          pending.delete(id)
          reject(new Error(`cdp timeout: ${method}`))
        }
      }, 10000)
    })

  try {
    await send('Page.enable')
    await send('Runtime.enable')
    // Navigate (in case /json/new did not load file://)
    await send('Page.navigate', { url: fileUrl })
    // Wait for load + fonts
    await Bun.sleep(200)
    await send('Runtime.evaluate', {
      expression: 'document.fonts && document.fonts.ready ? document.fonts.ready.then(() => true) : true',
      awaitPromise: true,
    }).catch(() => undefined)

    const metrics = await send('Runtime.evaluate', {
      expression: `(() => {
        const frame = document.querySelector('.shot-frame') || document.body;
        const rect = frame.getBoundingClientRect();
        // Width/height from the content frame only — never the Chrome window.
        const w = Math.max(1, Math.ceil(rect.width));
        const h = Math.max(1, Math.ceil(rect.height));
        return { width: w, height: h + ${SHOT_BOTTOM_PAD_PX} };
      })()`,
      returnByValue: true,
    }) as { result?: { value?: { width: number; height: number } } }

    const box = metrics.result?.value
    const width = Math.max(1, Math.ceil(box?.width ?? cssWidth))
    const height = Math.max(1, Math.ceil(box?.height ?? 200))

    // Device metrics: 1 CSS px = 1 device px for stable clip; scale=2 in capture.
    await send('Emulation.setDeviceMetricsOverride', {
      width,
      height,
      deviceScaleFactor: 2,
      mobile: false,
    })

    const shot = await send('Page.captureScreenshot', {
      format: 'png',
      fromSurface: true,
      captureBeyondViewport: true,
      clip: { x: 0, y: 0, width, height, scale: 1 },
    }) as { data?: string }

    if (!shot.data) return false
    const buf = Buffer.from(shot.data, 'base64')
    writeFileSync(pngPath, buf)
    return true
  } finally {
    try { ws.close() } catch { /* ignore */ }
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

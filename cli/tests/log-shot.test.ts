import { describe, expect, test } from 'bun:test'
import {
  findLastAssistantMarkdown,
  findLastAssistantTurn,
} from '../src/session/assistant-markdown.js'
import {
  parseMarkdownTraces,
  lastMarkdownTrace,
  findMarkdownTrace,
  lastMarkdownTurn,
  markdownTurnContaining,
  parseMarkdownTraceTs,
} from '../src/session/markdown-trace.js'
import {
  resolveShotSource,
  buildShotHtml,
  ansiToHtml,
  renderAssistantAnsi,
  renderShotAnsi,
  writeMarkdownShot,
  formatShotModelLabel,
  buildShotHeaderSpans,
  shotWindowSize,
} from '../src/commands/log-shot.js'
import { mkdtempSync, writeFileSync, readFileSync, rmSync, existsSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'

describe('findLastAssistantMarkdown', () => {
  test('returns null for empty history', () => {
    expect(findLastAssistantMarkdown([])).toBeNull()
  })

  test('returns the last assistant rawMarkdown chunk', () => {
    const last = findLastAssistantMarkdown([
      { kind: 'user', id: 'u1' },
      { kind: 'assistant', id: 'a1', rawMarkdown: 'first' },
      { kind: 'tool', id: 't1' },
      { kind: 'assistant', id: 'a2', rawMarkdown: 'second' },
      { kind: 'system', id: 's1' },
    ])
    expect(last).toEqual({ id: 'a2', rawMarkdown: 'second' })
  })

  test('skips empty rawMarkdown', () => {
    const last = findLastAssistantMarkdown([
      { kind: 'assistant', id: 'a1', rawMarkdown: 'keep' },
      { kind: 'assistant', id: 'a2', rawMarkdown: '   ' },
    ])
    expect(last).toEqual({ id: 'a1', rawMarkdown: 'keep' })
  })
})

describe('findLastAssistantTurn', () => {
  test('joins all assistant chunks after the last user message', () => {
    const turn = findLastAssistantTurn([
      { kind: 'user', id: 'u0' },
      { kind: 'assistant', id: 'old', rawMarkdown: 'previous answer' },
      { kind: 'user', id: 'u1' },
      { kind: 'assistant', id: 'a1', rawMarkdown: '# Title\n\n' },
      { kind: 'assistant', id: 'a1b', rawMarkdown: '# Title\n\n' }, // same chunk, multi-line
      { kind: 'tool', id: 't1' },
      { kind: 'assistant', id: 'a2', rawMarkdown: '## Section\n\nbody' },
    ])
    expect(turn).not.toBeNull()
    expect(turn!.chunkCount).toBe(2)
    expect(turn!.id).toBe('a1')
    expect(turn!.rawMarkdown).toBe('# Title\n\n## Section\n\nbody')
  })

  test('returns null when no assistant after last user', () => {
    expect(findLastAssistantTurn([
      { kind: 'assistant', id: 'a0', rawMarkdown: 'old' },
      { kind: 'user', id: 'u1' },
      { kind: 'tool', id: 't1' },
    ])).toBeNull()
  })
})

const SAMPLE_TRACE = `--- markdown trace asst-1 ---
ts: 2026-07-09 12:00:00.000
schema_version: 1
renderer_version: test-renderer

[raw markdown]
## Hello

| A | B |
|---|---|
| 1 | 2 |

[rendered lines]
Hello
┌───┬───┐
--- end markdown trace asst-1 ---

--- markdown trace asst-2 ---
ts: 2026-07-09 12:01:00.000
schema_version: 1
renderer_version: test-renderer

[raw markdown]
second message

[rendered lines]
second message
--- end markdown trace asst-2 ---
`

/** Two stream-flush chunks of one turn, then a later turn. */
const STREAMED_TURN_TRACE = `--- markdown trace asst-10 ---
ts: 2026-07-09 14:05:54.000
schema_version: 1
renderer_version: test-renderer

[raw markdown]
# Snowflake MV

intro

[rendered lines]
Snowflake MV
--- end markdown trace asst-10 ---

--- markdown trace asst-11 ---
ts: 2026-07-09 14:05:58.000
schema_version: 1
renderer_version: test-renderer

[raw markdown]
## Conclusion

done

[rendered lines]
Conclusion
--- end markdown trace asst-11 ---

--- markdown trace asst-20 ---
ts: 2026-07-09 14:10:00.000
schema_version: 1
renderer_version: test-renderer

[raw markdown]
unrelated later answer

[rendered lines]
unrelated later answer
--- end markdown trace asst-20 ---
`

describe('parseMarkdownTraces', () => {
  test('parses multiple complete traces', () => {
    const all = parseMarkdownTraces(SAMPLE_TRACE)
    expect(all).toHaveLength(2)
    expect(all[0]!.messageId).toBe('asst-1')
    expect(all[0]!.rawMarkdown).toContain('## Hello')
    expect(all[0]!.renderedLines[0]).toBe('Hello')
    expect(all[0]!.rendererVersion).toBe('test-renderer')
    expect(all[1]!.messageId).toBe('asst-2')
    expect(all[1]!.rawMarkdown).toBe('second message')
  })

  test('lastMarkdownTrace returns the final entry', () => {
    const last = lastMarkdownTrace(SAMPLE_TRACE)
    expect(last?.messageId).toBe('asst-2')
  })

  test('findMarkdownTrace by id', () => {
    expect(findMarkdownTrace(SAMPLE_TRACE, 'asst-1')?.rawMarkdown).toContain('## Hello')
    expect(findMarkdownTrace(SAMPLE_TRACE, 'missing')).toBeNull()
  })

  test('skips incomplete trailing block', () => {
    const incomplete = SAMPLE_TRACE + `\n--- markdown trace asst-3 ---\n[raw markdown]\nno end\n`
    expect(parseMarkdownTraces(incomplete)).toHaveLength(2)
  })
})

describe('lastMarkdownTurn', () => {
  test('parseMarkdownTraceTs handles log timestamps', () => {
    const t = parseMarkdownTraceTs('2026-07-09 14:05:54.842')
    expect(t).not.toBeNull()
    expect(new Date(t!).getFullYear()).toBe(2026)
  })

  test('groups trailing stream flushes within the gap, not older turns', () => {
    const all = parseMarkdownTraces(STREAMED_TURN_TRACE)
    // last alone is asst-20; turn group of last is just asst-20 (gap from 11 is 4+ min)
    const turn = lastMarkdownTurn(all)
    expect(turn).not.toBeNull()
    expect(turn!.messageId).toBe('asst-20')
    expect(turn!.traces).toHaveLength(1)
    expect(turn!.rawMarkdown).toBe('unrelated later answer')
  })

  test('joins consecutive flushes of the same turn', () => {
    const all = parseMarkdownTraces(STREAMED_TURN_TRACE).slice(0, 2)
    const turn = lastMarkdownTurn(all)
    expect(turn).not.toBeNull()
    expect(turn!.traces).toHaveLength(2)
    expect(turn!.messageId).toBe('asst-10')
    expect(turn!.lastMessageId).toBe('asst-11')
    expect(turn!.rawMarkdown).toBe('# Snowflake MV\n\nintro\n## Conclusion\n\ndone')
  })
})

describe('log-shot ansi + render', () => {
  test('ansiToHtml preserves truecolor fg (chalk.hex style)', () => {
    const html = ansiToHtml('\x1b[38;2;106;106;106m```\x1b[39m plain')
    expect(html).toContain('color:#6a6a6a')
    expect(html).toContain('```')
    expect(html).toContain('plain')
    expect(html).not.toContain('opacity')
    expect(html).not.toContain('\x1b')
  })

  test('ansiToHtml uses double-width cells for CJK (TUI string-width)', () => {
    const html = ansiToHtml('A中B')
    // Width-1 Latin keeps natural mono advance (no forced cell class).
    expect(html).toContain('A')
    expect(html).toContain('class="w2">中</span>')
    expect(html).toContain('B')
    expect(html).not.toContain('class="c"')
  })

  test('ansiToHtml handles bold/italic and 256-color', () => {
    const html = ansiToHtml('\x1b[1mbold\x1b[22m \x1b[3mitalic\x1b[23m \x1b[38;5;141mx\x1b[39m')
    expect(html).toContain('font-weight:700')
    expect(html).toContain('font-style:italic')
    expect(html).toContain('color:#af87ff')
  })

  test('ansiToHtml strips OSC 133 zone markers', () => {
    const html = ansiToHtml('\x1b]133;A\x07hello\x1b]133;B\x07\x1b]133;C\x07')
    expect(html).toContain('hello')
    expect(html).not.toContain('\x1b')
    expect(html).not.toContain('133')
  })

  test('renderAssistantAnsi uses TUI prefix and theme colors', () => {
    const ansi = renderAssistantAnsi('## Title\n\n**bold** and `code`\n\n```\nfoo\n```')
    expect(ansi).toContain('⏺')
    expect(ansi).toContain('\x1b[38;2;')
    expect(ansi).toContain('240;198;116')
    expect(ansi).toContain('177;185;249')
  })

  test('resolveShotSource prefers history full turn over single log chunk', () => {
    const dir = mkdtempSync(join(tmpdir(), 'evot-shot-'))
    try {
      const logPath = join(dir, 's.markdown.log')
      writeFileSync(logPath, STREAMED_TURN_TRACE)
      const source = resolveShotSource({
        markdownLogPath: logPath,
        historyLines: [
          { kind: 'user', id: 'u1' },
          { kind: 'assistant', id: 'h1', rawMarkdown: '# Full\n\n' },
          { kind: 'assistant', id: 'h2', rawMarkdown: 'body from history' },
        ],
      })
      expect(source?.kind).toBe('history')
      if (source?.kind === 'history') {
        expect(source.rawMarkdown).toBe('# Full\n\nbody from history')
        expect(source.chunkCount).toBe(2)
      }
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('resolveShotSource falls back to log turn grouping', () => {
    const dir = mkdtempSync(join(tmpdir(), 'evot-shot-'))
    try {
      const logPath = join(dir, 's.markdown.log')
      // only the two-chunk turn
      const onlyTurn = STREAMED_TURN_TRACE.split('--- markdown trace asst-20 ---')[0]!
      writeFileSync(logPath, onlyTurn)
      const source = resolveShotSource({
        markdownLogPath: logPath,
        historyLines: [],
      })
      expect(source?.kind).toBe('turn')
      if (source?.kind === 'turn') {
        expect(source.turn.traces).toHaveLength(2)
        expect(source.turn.rawMarkdown).toContain('# Snowflake MV')
        expect(source.turn.rawMarkdown).toContain('## Conclusion')
      }
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('resolveShotSource falls back to history when no log', () => {
    const source = resolveShotSource({
      markdownLogPath: null,
      historyLines: [{ kind: 'assistant', id: 'mem', rawMarkdown: 'from memory' }],
    })
    expect(source?.kind).toBe('history')
    if (source?.kind === 'history') {
      expect(source.rawMarkdown).toBe('from memory')
      expect(source.id).toBe('mem')
      expect(source.chunkCount).toBe(1)
      expect(source.paintedLines?.length).toBe(1)
    }
  })

  test('buildShotHtml matches TUI: ⏺, gold heading, slate canvas, full turn content', () => {
    const source = resolveShotSource({
      historyLines: [
        { kind: 'user', id: 'u' },
        { kind: 'assistant', id: 'a1', rawMarkdown: '## Hello\n\n' },
        { kind: 'assistant', id: 'a2', rawMarkdown: '```\ncode\n```\n\n`inline`' },
      ],
    })
    expect(source).not.toBeNull()
    const html = buildShotHtml(source!)
    expect(html).toContain('2 chunks')
    expect(html).toContain('⏺')
    expect(html).toContain('Hello')
    expect(html).toContain('code')
    expect(html).toContain('color:#f0c674')
    expect(html).toContain('color:#b1b9f9')
    expect(html).toContain('color:#6a6a6a')
    expect(html).toContain('--bg: #3f404e')
    expect(html).toContain('Menlo') // terminal mono stack
    expect(html).toContain('.w2') // CJK cell metric present in CSS
    expect(html).not.toContain('background:#39c5cf')
    expect(html).not.toContain('background:#56b6c2')
  })

  test('buildShotHtml header shows model badge and slim meta with rules and footer', () => {
    const source = resolveShotSource({
      historyLines: [
        { kind: 'assistant', id: 'a1', rawMarkdown: 'hi' },
      ],
    })
    expect(source).not.toBeNull()
    const html = buildShotHtml(source!, {
      header: {
        model: 'claude-opus-4-8',
        thinkingLevel: 'high',
        sessionId: '019f5621-a16e-7453-83e0-d649dd632c14',
        cwd: `${process.env.HOME}/github/evotai/evot`,
        branch: 'main',
      },
    })
    expect(html).toContain('class="shot-header"')
    expect(html).toContain('class="shot-model">claude-opus-4-8 · high</span>')
    expect(html).not.toContain('@anthropic')
    expect(html).not.toContain('provider')
    // No workspace path on the share image; time sits on the hero row.
    expect(html).not.toContain('~/github/evotai/evot')
    expect(html).not.toContain('session 019f5621')
    expect(html).not.toContain('100 cols')
    expect(html).not.toContain('class="chip"')
    expect(html).toContain('class="shot-time"')
    // Clear rules separate chrome from content
    expect(html).toContain('class="shot-rule"')
    // Footer explains how to generate
    expect(html).toContain('class="shot-footer"')
    expect(html).toContain('/log shot')
    expect(html).toContain('Generated with')
    expect(html).toContain('<title>evot shot · claude-opus-4-8 · high ·')
  })

  test('formatShotModelLabel omits provider and empty parts', () => {
    expect(formatShotModelLabel({ model: 'gpt-5.5' })).toBe('gpt-5.5')
    expect(formatShotModelLabel({ model: 'gpt-5.5', thinkingLevel: 'off' })).toBe('gpt-5.5 · thinking off')
    expect(formatShotModelLabel({})).toBe('')
    expect(buildShotHeaderSpans({ model: 'm' }).join('')).toContain('class="model">m</span>')
  })

  test('painted history lines keep TUI wrap 1:1 (no raw reflow)', () => {
    // SGR-tagged text forces the painted path (colorless text would re-render).
    const long =
      'Snowflake 的 Incremental MV 不是整表全量重算，而是维护一个可合并的中间态，并在 refresh 时做增量 merge。'
    const painted = `\x1b[37m${long}\x1b[39m`
    const source = resolveShotSource({
      historyLines: [{
        kind: 'assistant',
        id: 'p1',
        rawMarkdown: long,
        text: painted,
      }],
    })
    expect(source).not.toBeNull()
    const ansi = renderShotAnsi(source!, 40)
    // Soft-wrap may split display, but the original long token sequence remains
    // (no markdown re-lex into headings/tables).
    expect(ansi).toContain('Incremental MV')
    expect(ansi).toContain('⏺')
    expect(source!.kind).toBe('history')
    if (source!.kind === 'history') {
      expect(source!.paintedLines?.[0]?.text).toBe(painted)
    }
  })

  test('color-stripped log re-renders with theme colors at layout width', () => {
    // OSC only, no SGR — mirrors pre-ANSI-preserve markdown.log entries.
    const log = `--- markdown trace asst-c1 ---
ts: 2026-07-09 14:00:00.000
schema_version: 1
renderer_version: test

[raw markdown]
## Title

plain body with enough width xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx

[rendered lines]
\x1b]133;A\x07⏺ Title
  plain body with enough width xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
--- end markdown trace asst-c1 ---
`
    const dir = mkdtempSync(join(tmpdir(), 'evot-shot-color-'))
    try {
      const logPath = join(dir, 'c.markdown.log')
      writeFileSync(logPath, log)
      const source = resolveShotSource({ markdownLogPath: logPath })
      expect(source?.kind).toBe('turn')
      const html = buildShotHtml(source!, { columns: 40 })
      expect(html).toContain('color:#f0c674') // heading gold
      expect(html).toContain('⏺')
      expect(html).toContain('Title')
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('messageId expands to the full turn group, not a single chunk', () => {
    const dir = mkdtempSync(join(tmpdir(), 'evot-shot-id-'))
    try {
      const logPath = join(dir, 's.markdown.log')
      writeFileSync(logPath, STREAMED_TURN_TRACE)
      const source = resolveShotSource({
        markdownLogPath: logPath,
        messageId: 'asst-10',
      })
      expect(source?.kind).toBe('turn')
      if (source?.kind === 'turn') {
        expect(source.turn.traces.length).toBeGreaterThan(1)
        expect(source.turn.messageId).toBe('asst-10')
        expect(source.turn.rawMarkdown).toContain('Conclusion')
      }
      // Direct helper
      const all = parseMarkdownTraces(STREAMED_TURN_TRACE)
      const group = markdownTurnContaining(all, 'asst-11')
      expect(group?.traces.map(t => t.messageId)).toEqual(['asst-10', 'asst-11'])
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('table cells keep TUI alignment metrics in HTML', () => {
    const source = resolveShotSource({
      historyLines: [{
        kind: 'assistant',
        id: 't1',
        rawMarkdown: '| 类型 | 分片内容 |\n|---|---|\n| 投影/过滤 MV | 源行子集 |\n',
      }],
    })
    expect(source).not.toBeNull()
    const html = buildShotHtml(source!, { columns: 100 })
    // Box borders from the shared TUI table renderer, painted with tableBorder.
    expect(html).toContain('┌')
    expect(html).toContain('│')
    expect(html).toContain('color:#8a8a8a')
    // CJK forced to 2-cell advance so borders stay aligned in the browser.
    expect(html).toMatch(/class="w2"[^>]*>类</)
    expect(html).toContain('--bg: #3f404e')
    expect(html).toContain('Menlo')
    // No soft-wrap that would shatter the grid.
    expect(html).toContain('white-space: pre;')
  })

  test('writeMarkdownShot writes full-turn HTML from log', async () => {
    const dir = mkdtempSync(join(tmpdir(), 'evot-shot-out-'))
    try {
      const logPath = join(dir, 's.markdown.log')
      const onlyTurn = STREAMED_TURN_TRACE.split('--- markdown trace asst-20 ---')[0]!
      writeFileSync(logPath, onlyTurn)
      const result = await writeMarkdownShot({
        markdownLogPath: logPath,
        outDir: join(dir, 'shots'),
        png: false,
        open: false,
      })
      expect(result.messageId).toBe('asst-10')
      expect(result.chunkCount).toBe(2)
      expect(existsSync(result.htmlPath)).toBe(true)
      const body = readFileSync(result.htmlPath, 'utf8')
      expect(body).toContain('Snowflake MV')
      expect(body).toContain('Conclusion')
      expect(body).toContain('⏺')
      expect(body).toContain('padding: 14px 16px 18px')
      expect(body).toContain('class="shot-footer"')
      expect(body).toContain('/log shot')
      expect(body).toContain('class="shot-rule"')
      expect(result.pngPath).toBeUndefined()
    } finally {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('shotWindowSize tracks content height without a 900px floor', () => {
    const short = shotWindowSize(80, 20)
    expect(short.height).toBeLessThan(900)
    expect(short.height).toBeGreaterThan(200)
    const long = shotWindowSize(80, 200)
    expect(long.height).toBeGreaterThan(short.height)
    expect(long.height).toBeLessThanOrEqual(16000)
    // Wide content still gets a usable width, but not a tall empty frame.
    expect(shotWindowSize(40, 5).width).toBeGreaterThanOrEqual(480)
  })
})

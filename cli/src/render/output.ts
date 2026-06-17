/**
 * OutputLine — a single line of REPL output.
 *
 * All REPL output (user messages, assistant text, tool results, verbose events)
 * is modeled as an append-only list of OutputLines. These are rendered by
 * Ink's <Static> component, which writes them once and never re-renders.
 *
 * This module is pure logic — no React, no stdout. Easy to test.
 */

import { renderMarkdown } from './markdown.js'
import { colorizeUnifiedDiff } from './diff.js'
import { truncate, humanTokens, formatDuration, renderBar, toolResultLines, padRight } from './format.js'
import type { RunStats, SlimStats, UIMessage } from '../term/app/types.js'

// ---------------------------------------------------------------------------
// Tool presentation — icon + primary-arg per tool, in the spirit of
// pi-thinking-steps' semantic glyphs. The status (✓ / ✗ + duration) is
// rendered inline on the same line at finish time, so a tool reads as a
// single “card” line followed only by its real output.
// ---------------------------------------------------------------------------

interface ToolGlyph { icon: string }

/** Map an engine tool name to a compact glyph. Unknown tools fall back to `·`. */
function toolGlyph(name: string): ToolGlyph {
  switch (name.toLowerCase()) {
    case 'bash': return { icon: '⌘' }
    case 'read': case 'read_code': return { icon: '◫' }
    case 'grep': case 'glob': case 'find': case 'search': return { icon: '⌕' }
    case 'web_fetch': case 'webfetch': return { icon: '⊕' }
    case 'edit': case 'file_edit': case 'write': case 'file_write': return { icon: '✎' }
    default: return { icon: '·' }
  }
}

/** The single most useful argument to show beside the tool name. */
function toolPrimaryArg(name: string, args: Record<string, unknown>, previewCommand?: string): string {
  const n = name.toLowerCase()
  if (n === 'bash') {
    // Show the full command — the viewmodel wraps it to terminal width so the
    // tail is never lost. Newlines collapse to spaces for a single logical line.
    return (previewCommand ?? (args?.command as string) ?? '').replace(/\r?\n/g, ' ').trim()
  }
  const path = (args?.path ?? args?.file ?? args?.file_path) as string | undefined
  if (path) return path
  const pattern = (args?.pattern ?? args?.query ?? args?.url) as string | undefined
  // Show the full value — the viewmodel wraps the card arg to terminal width,
  // so the tail is never lost. Newlines collapse to a single logical line.
  if (pattern) return String(pattern).replace(/\r?\n/g, ' ').trim()
  return ''
}

/** Tool call line text: `<glyph> <name>  <primary-arg>`. The viewmodel paints
 *  the glyph and parts; status (✓/✗) lives on the subordinate result line. */
function toolCallText(name: string, args: Record<string, unknown>, previewCommand?: string): string {
  const glyph = toolGlyph(name).icon
  const primary = toolPrimaryArg(name, args, previewCommand)
  return primary ? `${glyph} ${name}  ${primary}` : `${glyph} ${name}`
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface OutputLine {
  id: string
  kind: 'user' | 'assistant' | 'thinking' | 'thinking_summary' | 'tool' | 'tool_result' | 'verbose' | 'error' | 'system' | 'run_summary'
  text: string
  rawMarkdown?: string
  codeBlockId?: string
  codeLanguage?: string
  /** Visual spacer inserted between streamed markdown chunks. It creates a
   *  blank line but must not start a new assistant message marker. */
  isContinuationSpacer?: boolean
}

// ---------------------------------------------------------------------------
// ID generator
// ---------------------------------------------------------------------------

let nextId = 0

function genId(prefix: string): string {
  return `${prefix}-${nextId++}`
}

/** Reset ID counter (for tests). */
export function resetIdCounter(): void {
  nextId = 0
}

// ---------------------------------------------------------------------------
// Builders — pure functions that create OutputLines from events
// ---------------------------------------------------------------------------

export function buildUserMessage(text: string): OutputLine[] {
  if (!text) return []
  return [{ id: genId('user'), kind: 'user', text }]
}

export function buildAssistantLines(markdownText: string): OutputLine[] {
  if (!markdownText.trim()) return []
  const rendered = renderMarkdown(markdownText)
  if (!rendered || !rendered.trim()) return []
  const cleaned = rendered.replace(/^\n+/, '').replace(/\n+$/, '')
  return cleaned.split('\n').map((line) => ({
    id: genId('asst'),
    kind: 'assistant' as const,
    text: line,
    rawMarkdown: markdownText,
  }))
}

export function buildThinkingLines(text: string): OutputLine[] {
  if (!text.trim()) return []
  const cleaned = text.replace(/^\n+/, '').replace(/\n+$/, '')
  return cleaned.split('\n').map((line) => ({
    id: genId('think'),
    kind: 'thinking' as const,
    text: line,
  }))
}

export function buildThinkingSummary(text: string, durationMs: number, expanded?: boolean): OutputLine[] {
  if (!text.trim()) return []
  const cleaned = text.replace(/^\n+/, '').replace(/\n+$/, '')
  const allLines = cleaned.split('\n')
  const lineCount = allLines.length
  const duration = formatDuration(durationMs)
  const TAIL_LINES = 5
  const MAX_LINE_WIDTH = 256

  const lines: OutputLine[] = []
  lines.push({
    id: genId('think-summary'),
    kind: 'thinking_summary' as const,
    text: `${lineCount} lines · ${duration}`,
  })

  const capLine = (l: string) => l.length <= MAX_LINE_WIDTH ? l.slice(0, MAX_LINE_WIDTH) : l.slice(0, MAX_LINE_WIDTH - 1) + '…'

  if (expanded) {
    for (const l of allLines) {
      lines.push({ id: genId('think'), kind: 'thinking' as const, text: `  ${capLine(l)}` })
    }
    lines.push({ id: genId('think-hint'), kind: 'thinking' as const, text: '  \x1b[2m(ctrl+o to collapse)\x1b[0m' })
  } else {
    if (allLines.length > TAIL_LINES) {
      const tail = allLines.slice(0, TAIL_LINES)
      for (const l of tail) {
        lines.push({ id: genId('think'), kind: 'thinking' as const, text: `  ${capLine(l)}` })
      }
      const omitted = allLines.length - TAIL_LINES
      lines.push({ id: genId('think-hint'), kind: 'thinking' as const, text: `  ... (+${omitted} lines, ctrl+o to expand)` })
    } else {
      for (const l of allLines) {
        lines.push({ id: genId('think'), kind: 'thinking' as const, text: `  ${capLine(l)}` })
      }
    }
  }

  return lines
}

export function buildToolCall(
  name: string,
  args: Record<string, unknown>,
  previewCommand?: string,
): OutputLine[] {
  if (name === 'update_goal_tasks' || name === 'TodoWrite') return buildGoalTaskCall(name, args)

  // Reason fields surface the model's justification up-front.
  const lines: OutputLine[] = []
  for (const line of formatReasonLines(args)) {
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${line}` })
  }
  // Call line: `<glyph> <name>  <primary-arg>` — shown the moment the tool
  // starts so the running command is visible. The result block (status mark +
  // duration + output) is appended below by buildToolResult on finish.
  lines.push({
    id: genId('tool'),
    kind: 'tool',
    text: toolCallText(name, args, previewCommand),
  })
  return lines
}

export function buildToolResult(
  name: string,
  args: Record<string, unknown>,
  status: 'done' | 'error',
  result?: string,
  durationMs?: number,
  expanded?: boolean,
  slim?: SlimStats,
): OutputLine[] {
  const lines: OutputLine[] = []
  const isError = status === 'error'

  if ((name === 'update_goal_tasks' || name === 'TodoWrite') && !isError) {
    return buildGoalTaskResult(name, args, result)
  }

  const resultInfo = result ? formatToolResultInfo(result) : ''
  const slimSuffix = formatSlimSuffix(slim)
  // The status line is appended at the END (after diff/output) so a tool reads
  // top-to-bottom: command → output → closing status. Built here, pushed last.
  const mark = isError ? '✗' : '✓'
  const dur = durationMs !== undefined ? ` · ${formatDuration(durationMs)}` : ''
  const statusLine: OutputLine = {
    id: genId('tool'),
    kind: 'tool',
    text: `  ${mark}${dur}${resultInfo}${slimSuffix}`,
  }

  // Diff (for write/edit tools)
  const diff = args?.diff as string | undefined
  if (diff && typeof diff === 'string' && diff.length > 0) {
    lines.push({
      id: genId('tool-diff'),
      kind: 'tool',
      text: colorizeUnifiedDiff(diff),
    })
  }

  // Tool result content (head/tail truncated)
  if (result) {
    if (name === 'Read' || name === 'read_code') {
      if (isError) {
        // Show error content for failed reads.
        const resultLines = toolResultLines(result, isError, name, expanded)
        for (const rl of resultLines) {
          lines.push({
            id: genId('tool-res'),
            kind: 'error',
            text: `  ${rl}`,
          })
        }
      }
      // Successful reads show no body: the status line already carries the
      // size (e.g. `✓ · 12ms · 1.2 KB`), so a separate size line would repeat it.
    } else {
      const formattedResult = formatToolResultContent(result)
      const resultLines = toolResultLines(formattedResult, isError, name, expanded)
      for (const rl of resultLines) {
        lines.push({
          id: genId('tool-res'),
          kind: isError ? 'error' : 'tool_result',
          text: `  ${rl}`,
        })
      }
      // Show expand/collapse hint for multiline results.
      if (expanded && resultLines.length > 1) {
        lines.push({
          id: genId('tool-hint'),
          kind: 'tool_result',
          text: '  \x1b[2m(ctrl+o to collapse)\x1b[0m',
        })
      }
      if (!expanded) {
        const allLines = formattedResult.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n')
        if (allLines.length > 5 && !resultLines.some(l => l.includes('ctrl+o to expand'))) {
          lines.push({
            id: genId('tool-hint'),
            kind: 'tool_result',
            text: '  \x1b[2m(ctrl+o to expand)\x1b[0m',
          })
        }
      }
    }
  }

  // Closing status line, after the output.
  lines.push(statusLine)
  return lines
}

export function buildToolProgress(name: string, text: string, expanded?: boolean): OutputLine[] {
  const progressLines = text.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n')
  const total = progressLines.length
  const visible = expanded ? progressLines : progressLines.slice(-5)
  const hidden = expanded ? 0 : Math.max(0, total - visible.length)
  const header = `${toolGlyph(name).icon} ${name}  · ${total} ${total === 1 ? 'line' : 'lines'}`
  const lines: OutputLine[] = [{ id: genId('tool'), kind: 'tool', text: header }]
  for (const l of visible) {
    lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${l}` })
  }
  if (expanded && visible.length > 1) {
    lines.push({ id: genId('tool-hint'), kind: 'tool_result', text: '  \x1b[2m(ctrl+o to collapse)\x1b[0m' })
  }
  if (hidden > 0) {
    lines.push({ id: genId('tool-hint'), kind: 'tool_result', text: `  \x1b[2m... (+${hidden} lines, ctrl+o to expand)\x1b[0m` })
  }
  return lines
}

export function buildVerboseEvent(eventText: string): OutputLine[] {
  if (!eventText) return []
  return eventText.split('\n').map((line) => ({
    id: genId('verb'),
    kind: 'verbose' as const,
    text: line,
  }))
}

/** True for LLM events that must always reach the TUI (errors and retries),
 *  as opposed to per-call stats that only belong in screen.log. */
export function isVisibleLlmEvent(text: string): boolean {
  return /^\[LLM\]\s+[↻✗]/u.test(text)
}

/**
 * Render a visible LLM event (error / retry) as a tool-style card so it reads
 * like any other tool in the stream:
 *   ✦ llm  <model|retry>
 *     ✗|↻ · <meta>
 *     <error message>
 * Falls back to plain verbose lines if the text isn't in the expected shape.
 */
export function buildLlmCard(text: string): OutputLine[] {
  const rawLines = text.split('\n')
  const head = (rawLines[0] ?? '').match(/^\[LLM\]\s+([↻✗])\s*·?\s*(.*)$/u)
  if (!head) return buildVerboseEvent(text)
  const mark = head[1]!
  const rest = (head[2] ?? '').trim()
  const isRetry = mark === '↻'
  // Body: drop the `    error     ` label, keep the message text.
  const body = rawLines.slice(1)
    .map((l) => l.replace(/^\s*error\s+/u, '').trim())
    .filter((l) => l.length > 0)

  const lines: OutputLine[] = []
  if (isRetry) {
    lines.push({ id: genId('tool'), kind: 'tool', text: '✦ llm  retry' })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark} · ${rest}` })
  } else {
    const parts = rest.split(' · ')
    const model = parts[0] ?? 'unknown'
    const meta = parts.slice(1).join(' · ')
    lines.push({ id: genId('tool'), kind: 'tool', text: `✦ llm  ${model}` })
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${mark}${meta ? ` · ${meta}` : ''}` })
  }
  for (const b of body) {
    lines.push({ id: genId('tool-res'), kind: 'error', text: `  ${b}` })
  }
  return lines
}


export function buildRunSummary(stats: RunStats): OutputLine[] {
  const lines: OutputLine[] = []
  const dur = formatDuration(stats.durationMs)
  const totalTokens = stats.inputTokens + stats.outputTokens
  const pl = (n: number, s: string) => n === 1 ? s : `${s}s`
  const line = (text: string) => lines.push({ id: genId('summary'), kind: 'run_summary' as const, text })
  const barWidth = 20

  // Header
  line('')
  line('── run summary ─────────────────────────────────────')
  line(`overview    ${dur} · ${stats.turnCount} ${pl(stats.turnCount, 'turn')} · ${stats.llmCalls} llm · ${stats.toolCallCount} tools · ${humanTokens(totalTokens)} tokens`)

  // Context budget bar
  if (stats.contextWindow > 0 && stats.contextTokens > 0) {
    const budget = Math.max(0, stats.contextWindow - stats.systemPromptTokens)
    const pct = budget > 0 ? (stats.contextTokens / budget) * 100 : 0
    if (pct > 0) {
      const bar = renderBar(stats.contextTokens, budget, barWidth)
      line(`context     ${bar}   ${humanTokens(stats.contextTokens)} / ${humanTokens(budget)} · ${pct.toFixed(0)}%`)
    }
  }

  // --- tokens block ---
  const totalInput = stats.inputTokens
  const totalStreamMs = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0)
  const overallTps = totalStreamMs > 0 ? (stats.outputTokens / (totalStreamMs / 1000)).toFixed(0) : '0'
  let tokLine = `tokens      ${humanTokens(totalInput)} in → ${humanTokens(stats.outputTokens)} out · ${overallTps} tok/s`
  if (stats.cacheReadTokens > 0 || stats.cacheWriteTokens > 0) {
    const cacheTotalInput = stats.inputTokens + stats.cacheReadTokens + stats.cacheWriteTokens
    const hitRate = cacheTotalInput > 0
      ? (stats.cacheReadTokens / cacheTotalInput * 100).toFixed(0)
      : '0'
    tokLine += ` · cache ${hitRate}%`
  }
  line(tokLine)

  // Token breakdown by role (last LLM call's context snapshot)
  const ms = stats.lastMessageStats ?? stats.cumulativeStats
  const hasBreakdown = ms.userTokens > 0 || ms.assistantTokens > 0 || ms.toolResultTokens > 0 || ms.imageTokens > 0
  if (hasBreakdown) {
    const sysTok = stats.systemPromptTokens
    const breakdownTotal = sysTok + ms.userTokens + ms.assistantTokens + ms.toolResultTokens + ms.imageTokens
    if (breakdownTotal > 0) {
      const maxLabelWidth = 12
      const maxValWidth = 6
      const roles: [string, number][] = [
        ['system', sysTok],
        ['user', ms.userTokens],
        ['assistant', ms.assistantTokens],
        ['tool_result', ms.toolResultTokens],
        ['image', ms.imageTokens],
      ]
      for (const [label, tokens] of roles) {
        if (tokens === 0) continue
        const pct = (tokens / breakdownTotal * 100).toFixed(0)
        const bar = renderBar(tokens, breakdownTotal, barWidth)
        line(`  ${padRight(label, maxLabelWidth)} ${humanTokens(tokens).padStart(maxValWidth)}   ${bar} ${pct.padStart(3)}%`)
      }

      // Per-tool breakdown under tool_result (only when >= 3 distinct tools)
      if (ms.toolDetails.length >= 3) {
        const agg = new Map<string, { calls: number; tokens: number }>()
        for (const [name, tokens] of ms.toolDetails) {
          const existing = agg.get(name)
          if (existing) {
            existing.calls++
            existing.tokens += tokens
          } else {
            agg.set(name, { calls: 1, tokens })
          }
        }
        const sorted = [...agg.entries()].sort((a, b) => b[1].tokens - a[1].tokens)
        const shown = sorted.slice(0, 5)
        const omitted = sorted.slice(5)
        const toolNameWidth = Math.max(12, ...shown.map(([name]) => name.length))
        for (const [name, a] of shown) {
          const pct = breakdownTotal > 0 ? (a.tokens / breakdownTotal * 100).toFixed(0) : '0'
          const bar = renderBar(a.tokens, breakdownTotal, barWidth)
          const callWord = a.calls === 1 ? 'call' : 'calls'
          line(`    ${padRight(name, toolNameWidth)} ${humanTokens(a.tokens).padStart(maxValWidth)}   ${bar} ${pct.padStart(3)}%   ${a.calls} ${callWord}`)
        }
        if (omitted.length > 0) {
          const omittedTokens = omitted.reduce((s, [, a]) => s + a.tokens, 0)
          const omittedCalls = omitted.reduce((s, [, a]) => s + a.calls, 0)
          const pct = breakdownTotal > 0 ? (omittedTokens / breakdownTotal * 100).toFixed(0) : '0'
          const bar = renderBar(omittedTokens, breakdownTotal, barWidth)
          line(`    ${padRight(`… ${omitted.length} more`, toolNameWidth)} ${humanTokens(omittedTokens).padStart(maxValWidth)}   ${bar} ${pct.padStart(3)}%   ${omittedCalls} calls`)
        }
      }
    }
  }

  // --- compact block ---
  if (stats.compactHistory.length > 0) {
    const compactTokens = (c: typeof stats.compactHistory[number]) => ({
      before: c.beforeTokens ?? c.fromTokens ?? c.from_tokens ?? 0,
      after: c.afterTokens ?? c.toTokens ?? c.to_tokens ?? 0,
    })
    const totalBefore = stats.compactHistory.reduce((s, c) => s + compactTokens(c).before, 0)
    const totalSaved = stats.compactHistory.reduce((s, c) => {
      const { before, after } = compactTokens(c)
      return s + (before - after)
    }, 0)
    const savedPct = totalBefore > 0 ? (totalSaved / totalBefore * 100).toFixed(0) : '0'
    line(`compact     ${stats.compactHistory.length} ${pl(stats.compactHistory.length, 'run')} · saved ${humanTokens(totalSaved)} (${savedPct}%)`)

    const formatCompact = (c: typeof stats.compactHistory[number], idx: number) => {
      const { before, after } = compactTokens(c)
      const saved = before - after
      const bar = renderBar(saved, before || 1, 12)
      return `  #${String(idx + 1).padEnd(2)} L${String(c.level).padEnd(2)} ${humanTokens(before).padStart(5)} → ${humanTokens(after).padStart(5)}   −${humanTokens(saved).padEnd(5)} ${bar}`
    }
    const shownIndexes = compactDisplayIndexes(stats.compactHistory.length)
    let insertedOmitted = false
    for (const idx of shownIndexes) {
      if (!insertedOmitted && idx > 2) {
        const omitted = stats.compactHistory.slice(3, idx)
        const omittedSaved = omitted.reduce((s, c) => {
          const { before, after } = compactTokens(c)
          return s + (before - after)
        }, 0)
        line(`       … ${omitted.length} more   −${humanTokens(omittedSaved)}`)
        insertedOmitted = true
      }
      line(formatCompact(stats.compactHistory[idx]!, idx))
    }
  }

  // --- llm block ---
  if (stats.llmCallDetails.length > 0) {
    const totalLlmMs = stats.llmCallDetails.reduce((s, c) => s + c.durationMs, 0)
    const llmPct = stats.durationMs > 0 ? (totalLlmMs / stats.durationMs * 100).toFixed(0) : '0'
    const totalOutputTok = stats.llmCallDetails.reduce((s, c) => s + c.outputTokens, 0)
    const totalLlmStreamMs = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0)
    const avgTps = totalLlmStreamMs > 0 ? (totalOutputTok / (totalLlmStreamMs / 1000)).toFixed(0) : '0'
    const avgTtft = stats.llmCallDetails.reduce((s, c) => s + c.ttftMs, 0) / stats.llmCallDetails.length
    const avgStream = stats.llmCallDetails.reduce((s, c) => s + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
    line(`llm         ${stats.llmCallDetails.length} ${pl(stats.llmCallDetails.length, 'call')} · ${formatDuration(totalLlmMs)} · ${llmPct}% of run · ${avgTps} tok/s avg · ttft ${formatDuration(Math.round(avgTtft))} · stream ${formatDuration(Math.round(avgStream))}`)

    // Top 3 LLM calls by duration
    const sorted = [...stats.llmCallDetails].sort((a, b) => b.durationMs - a.durationMs)
    const show = Math.min(sorted.length, 3)
    const maxDur = sorted[0]?.durationMs ?? 1
    const maxDurWidth = Math.max(...sorted.slice(0, show).map(c => formatDuration(c.durationMs).length))
    for (let i = 0; i < show; i++) {
      const c = sorted[i]!
      const bar = renderBar(c.durationMs, maxDur, barWidth)
      const pct = totalLlmMs > 0 ? (c.durationMs / totalLlmMs * 100).toFixed(0) : '0'
      const durStr = formatDuration(c.durationMs).padStart(maxDurWidth)
      line(`  #${i + 1}        ${durStr}   ${bar} ${pct.padStart(3)}%`)
    }
    if (sorted.length > 3) {
      const restMs = sorted.slice(3).reduce((s, c) => s + c.durationMs, 0)
      line(`       … ${sorted.length - 3} more · ${formatDuration(restMs)} total`)
    }
  }

  return lines
}

function compactDisplayIndexes(count: number): number[] {
  if (count <= 4) return Array.from({ length: count }, (_, i) => i)
  return [0, 1, 2, count - 1]
}

export function buildError(message: string): OutputLine[] {
  return [{ id: genId('err'), kind: 'error', text: `Error: ${message}` }]
}

export function buildSystem(text: string): OutputLine[] {
  return [{ id: genId('sys'), kind: 'system', text }]
}

// ---------------------------------------------------------------------------
// Convert UIMessages to OutputLines (for resume)
// ---------------------------------------------------------------------------

export function messagesToOutputLines(messages: UIMessage[]): OutputLine[] {
  const lines: OutputLine[] = []
  for (const msg of messages) {
    if (msg.role === 'user') {
      lines.push(...buildUserMessage(msg.text))
    } else if (msg.role === 'assistant') {
      // Replay only the LLM errors/retries (as cards), matching live behavior.
      // Per-call stats live in screen.log, not the TUI.
      if (msg.verboseEvents) {
        for (const evt of msg.verboseEvents) {
          if (isVisibleLlmEvent(evt.text)) lines.push(...buildLlmCard(evt.text))
        }
      }
      // Tool calls: show call + result
      if (msg.toolCalls) {
        for (const tc of msg.toolCalls) {
          lines.push(...buildToolCall(tc.name, tc.args, tc.previewCommand))
          lines.push(...buildToolResult(
            tc.name,
            tc.args,
            tc.status === 'error' ? 'error' : 'done',
            tc.result,
            tc.durationMs,
            undefined,
            tc.slim,
          ))
        }
      }
      // Assistant text
      if (msg.text.trim()) {
        lines.push(...buildAssistantLines(msg.text))
      }
      // Run summary
      if (msg.runStats) {
        lines.push(...buildRunSummary(msg.runStats))
      }
    }
  }
  return lines
}

// ---------------------------------------------------------------------------
// Code-block-aware split (inspired by qwen-code's markdownUtilities)
// ---------------------------------------------------------------------------

/**
 * Check if a character index falls inside an unclosed fenced code block.
 */
function isInsideCodeBlock(content: string, index: number): boolean {
  let fenceCount = 0
  let pos = 0
  while (pos < content.length) {
    const next = content.indexOf('```', pos)
    if (next === -1 || next >= index) break
    fenceCount++
    pos = next + 3
  }
  return fenceCount % 2 === 1
}

/**
 * Find the last safe split point in `content` — a position where we can
 * cut without breaking a code block.  Prefers `\n\n` (paragraph boundary),
 * falls back to `\n`.  Returns `content.length` when no safe split exists.
 */
export function findSafeSplitPoint(content: string): number {
  // If the tail is inside an unclosed code block, don't split at all.
  if (isInsideCodeBlock(content, content.length)) return content.length

  // Prefer paragraph boundary (\n\n) not inside a code block.
  let search = content.length
  while (search >= 0) {
    const idx = content.lastIndexOf('\n\n', search)
    if (idx === -1) break
    const splitAt = idx + 2
    if (!isInsideCodeBlock(content, splitAt)) return splitAt
    search = idx - 1
  }

  // Fall back to last single newline not inside a code block.
  const nlPos = content.lastIndexOf('\n')
  if (nlPos > 0 && !isInsideCodeBlock(content, nlPos + 1)) return nlPos + 1

  return content.length
}

// ---------------------------------------------------------------------------
// AssistantStreamBuffer — accumulates streaming tokens, emits lines
// ---------------------------------------------------------------------------

export class AssistantStreamBuffer {
  private buffer = ''
  private started = false

  /** Push a token. Returns OutputLines to append (may be empty). */
  push(token: string): OutputLine[] {
    if (!token) return []
    this.buffer += token

    if (!this.started) {
      this.buffer = this.buffer.replace(/^[\n\r]+/, '')
      if (this.buffer.length === 0) return []
      this.started = true
    }

    return this.flushSafe()
  }

  /** Flush remaining buffer. Returns OutputLines to append. */
  finish(): OutputLine[] {
    if (!this.started) return []
    const result: OutputLine[] = []
    if (this.buffer.trim().length > 0) {
      result.push(...buildAssistantLines(this.buffer))
    }
    this.buffer = ''
    this.started = false
    return result
  }

  /** The current incomplete text (for display in dynamic zone). */
  get pendingText(): string {
    return this.started ? this.buffer : ''
  }

  get isStarted(): boolean {
    return this.started
  }

  /**
   * Flush completed content using code-block-aware splitting.
   * Only the portion before the safe split point is rendered and emitted;
   * the rest stays in the buffer for the dynamic zone.
   */
  private flushSafe(): OutputLine[] {
    if (!this.buffer.includes('\n')) return []

    const splitAt = findSafeSplitPoint(this.buffer)
    if (splitAt === this.buffer.length || splitAt === 0) return []

    const completeText = this.buffer.slice(0, splitAt)
    this.buffer = this.buffer.slice(splitAt)

    return buildAssistantLines(completeText)
  }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function buildGoalTaskCall(name: string, args: Record<string, unknown>): OutputLine[] {
  const label = name === 'TodoWrite' ? 'todo' : 'plan'
  const tasks = readGoalTasks(args)
  const summary = summarizeGoalTasks(tasks)
  const lines: OutputLine[] = [{ id: genId('tool'), kind: 'tool', text: `◇ ${label}  · ${summary}` }]
  for (const task of tasks) {
    lines.push({ id: genId('tool'), kind: 'tool', text: `  ${goalTaskSymbol(task.status)} #${task.id} ${task.title}` })
  }
  return lines
}

function buildGoalTaskResult(name: string, args: Record<string, unknown>, result?: string): OutputLine[] {
  const label = name === 'TodoWrite' ? 'todo' : 'plan'
  const tasks = readGoalTasks(args)
  const summary = summarizeGoalTasks(tasks, result)
  const lines: OutputLine[] = [{ id: genId('tool'), kind: 'tool', text: `◇ ${label}  · ${summary}` }]
  for (const task of tasks) {
    lines.push({ id: genId('tool-res'), kind: 'tool_result', text: `  ${goalTaskSymbol(task.status)} #${task.id} ${task.title}${goalTaskDuration(task)}` })
  }
  return lines
}

interface GoalTaskView {
  id: number
  title: string
  status: string
  startedAt?: string
  completedAt?: string
}

function readGoalTasks(args: Record<string, unknown>): GoalTaskView[] {
  const tasks = args?.tasks
  if (!Array.isArray(tasks)) return []
  return tasks.flatMap((task): GoalTaskView[] => {
    if (!task || typeof task !== 'object') return []
    const input = task as Record<string, unknown>
    const id = typeof input.id === 'number' ? input.id : Number(input.id)
    const title = typeof input.title === 'string' ? input.title.trim()
      : typeof input.content === 'string' ? input.content.trim() : ''
    const status = typeof input.status === 'string' ? input.status : ''
    const startedAt = typeof input.started_at === 'string' ? input.started_at : undefined
    const completedAt = typeof input.completed_at === 'string' ? input.completed_at : undefined
    if (!Number.isFinite(id) || title.length === 0) return []
    return [{ id, title, status, startedAt, completedAt }]
  })
}

function summarizeGoalTasks(tasks: GoalTaskView[], fallback?: string): string {
  if (tasks.length === 0) return fallback?.trim() || 'tasks updated'
  const completed = tasks.filter(task => task.status === 'completed').length
  return `${completed}/${tasks.length} completed`
}

function goalTaskDuration(task: GoalTaskView): string {
  const started = parseGoalTaskTime(task.startedAt)
  if (started === undefined) return ''
  if (task.completedAt) {
    const completed = parseGoalTaskTime(task.completedAt)
    if (completed === undefined || completed < started) return ''
    return ` · done in ${formatDuration(completed - started)}`
  }
  if (task.status === 'in_progress') {
    const elapsed = Date.now() - started
    return elapsed >= 0 ? ` · running ${formatDuration(elapsed)}` : ''
  }
  return ''
}

function parseGoalTaskTime(value?: string): number | undefined {
  if (!value) return undefined
  const parsed = Date.parse(value)
  return Number.isFinite(parsed) ? parsed : undefined
}

function goalTaskSymbol(status: string): string {
  if (status === 'completed') return '☑'
  if (status === 'in_progress') return '▷'
  return '·'
}

function humanBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / (1024 * 1024)).toFixed(1)} MB`
}

function parseJsonResult(content: string): unknown | undefined {
  const trimmed = content.trim()
  if (!trimmed) return undefined
  const first = trimmed[0]
  if (first !== '{' && first !== '[') return undefined
  try {
    return JSON.parse(trimmed)
  } catch {
    return undefined
  }
}

function formatToolResultInfo(content: string): string {
  const bytes = Buffer.byteLength(content, 'utf-8')
  // The result body (or its head/tail) is rendered right below this line, so
  // we don't restate its shape ("JSON · N keys"). Keep only what the body
  // doesn't already convey: how many lines and how big.
  const lineCount = content.replace(/\r\n/g, '\n').replace(/\n+$/, '').split('\n').length
  return lineCount > 1 ? ` · ${lineCount} lines · ${humanBytes(bytes)}` : ` · ${humanBytes(bytes)}`
}

/**
 * Render a compact slim indicator for the tool result header.
 * Kept terse so the status line still fits on narrow terminals.
 */
function formatSlimSuffix(slim?: SlimStats): string {
  if (!slim) return ''
  const { filter, original, slimmed } = slim
  // No-ops: don't render badges for these.
  if (filter === 'off' || filter === 'raw_error' || filter === 'none' || !filter) {
    return ''
  }
  if (filter === 'cache_hit') return formatSlimTokenRange('cache hit', original, slimmed, 0)
  if (original <= 0) return ''
  const saved = original - slimmed
  if (saved <= 0) return ''
  const pct = Math.round((saved / original) * 100)
  return pct >= 10 ? formatSlimTokenRange(`slim(${filter})`, original, slimmed, pct) : ''
}

function formatSlimTokenRange(label: string, originalBytes: number, slimmedBytes: number, pct: number): string {
  const originalTokens = estimatedTokens(originalBytes)
  const slimmedTokens = estimatedTokens(slimmedBytes)
  const pctSuffix = pct > 0 ? ` −${pct}%` : ''
  return ` · ${label} ~${formatTokenCount(originalTokens)}→~${formatTokenCount(slimmedTokens)} tok${pctSuffix}`
}

function estimatedTokens(bytes: number): number {
  return Math.max(1, Math.round(bytes / 4))
}

function formatTokenCount(tokens: number): string {
  if (tokens >= 1000) {
    const value = tokens / 1000
    return `${value >= 10 ? Math.round(value) : value.toFixed(1)}k`
  }
  return String(tokens)
}

function formatToolResultContent(content: string): string {
  const parsed = parseJsonResult(content)
  if (parsed === undefined) return content
  return JSON.stringify(parsed, null, 2)
}

/** Reason-style fields the model fills to justify a call. Rendered separately
 *  as ↳ lines and excluded from the generic arg list. */
function isReasonKey(key: string): boolean {
  return key === 'reason' || key.startsWith('reason_to_')
}

/** Human label for a reason field key. */
function reasonLabel(key: string): string {
  switch (key) {
    case 'reason':
      return 'reason'
    case 'reason_to_increase_timeout':
      return 'why longer timeout'
    case 'reason_to_use_instead_of_read_file_tool':
      return 'why not read'
    case 'reason_to_use_instead_of_edit_file_tool':
      return 'why not edit'
    case 'reason_to_use_instead_of_glob_files_tool':
      return 'why not glob'
    default:
      return key.replace(/^reason_to_/, 'why ').replace(/_/g, ' ')
  }
}

/** Build ↳ lines for any reason fields present, skipping empty or 'N/A'. */
function formatReasonLines(args: Record<string, unknown>): string[] {
  if (!args || typeof args !== 'object') return []
  const lines: string[] = []
  for (const [k, v] of Object.entries(args)) {
    if (!isReasonKey(k)) continue
    if (typeof v !== 'string') continue
    const val = v.trim()
    if (val === '' || val === 'N/A') continue
    lines.push(`↳ ${reasonLabel(k)}: ${truncate(val, 120)}`)
  }
  return lines
}

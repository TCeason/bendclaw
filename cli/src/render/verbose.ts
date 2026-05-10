/**
 * Shared verbose event text formatters.
 *
 * Used by both reducer.ts (real-time streaming) and transcript.ts (history replay)
 * to produce identical output from stats event data.
 */
import { formatDuration, humanTokens, renderBar, renderPositionBar } from './format.js'

function msgBreakdown(ms: Record<string, any> | undefined): string {
  if (!ms) return ''
  const parts: string[] = []
  if (ms.user_count > 0) parts.push(`user ${ms.user_count}`)
  if (ms.assistant_count > 0) parts.push(`asst ${ms.assistant_count}`)
  if (ms.tool_result_count > 0) parts.push(`tool ${ms.tool_result_count}`)
  if ((ms.image_count as number) > 0) {
    const pathCount = (ms.image_path_count as number) ?? 0
    const base64Count = (ms.image_base64_count as number) ?? 0
    const imgParts: string[] = []
    if (pathCount > 0) imgParts.push(`path ${pathCount}`)
    if (base64Count > 0) imgParts.push(`b64 ${base64Count}`)
    parts.push(imgParts.length > 0 ? `img ${ms.image_count} (${imgParts.join(' / ')})` : `img ${ms.image_count}`)
  }
  return parts.length > 0 ? ` · ${parts.join(' / ')}` : ''
}

function contextLine(used: number, contextWindow: number, saved?: number): string | undefined {
  if (contextWindow <= 0 || used <= 0) return undefined
  const pct = ((used / contextWindow) * 100).toFixed(0)
  const suffix = saved && saved > 0 ? `  (−${humanTokens(saved)})` : ''
  return `    context   ${renderBar(used, contextWindow, 20)}   ${humanTokens(used)} / ${humanTokens(contextWindow)} · ${pct}%${suffix}`
}

function roleTokensLine(parts: string[]): string | undefined {
  return parts.length > 0 ? `    tokens    ${parts.join(' · ')}` : undefined
}

function roleTokenParts(ms: Record<string, any>, sysTok: number, toolDefTok: number): string[] {
  const parts: string[] = []
  if (sysTok > 0) parts.push(`sys ${humanTokens(sysTok)}`)
  if (toolDefTok > 0) parts.push(`tools ${humanTokens(toolDefTok)}`)
  if ((ms.user_tokens as number) > 0) parts.push(`user ${humanTokens(ms.user_tokens)}`)
  if ((ms.assistant_tokens as number) > 0) parts.push(`asst ${humanTokens(ms.assistant_tokens)}`)
  if ((ms.tool_result_tokens as number) > 0) parts.push(`tool ${humanTokens(ms.tool_result_tokens)}`)
  const totalTokens = sysTok + toolDefTok + (ms.user_tokens ?? 0) + (ms.assistant_tokens ?? 0) + (ms.tool_result_tokens ?? 0) + (ms.image_tokens ?? 0)
  if ((ms.image_tokens as number) > 0) {
    const pct = totalTokens > 0 ? ` (${(((ms.image_tokens as number) / totalTokens) * 100).toFixed(0)}%)` : ''
    parts.push(`img ${humanTokens(ms.image_tokens)}${pct}`)
  }
  return parts
}

function toolTokensLine(ms: Record<string, any>): string | undefined {
  const rawDetails = ms.tool_details as [string, number][] | undefined
  if (!rawDetails || rawDetails.length < 2) return undefined

  const agg = new Map<string, number>()
  for (const [name, tokens] of rawDetails) {
    agg.set(name, (agg.get(name) ?? 0) + tokens)
  }

  const sorted = [...agg.entries()].sort((a, b) => b[1] - a[1])
  const total = (ms.tool_result_tokens as number) || sorted.reduce((sum, [, tokens]) => sum + tokens, 0) || 1
  const TOP = 3
  const shown = sorted.length <= TOP + 2 ? sorted : sorted.slice(0, TOP)
  const parts = shown.map(([name, tokens]) => {
    const pct = total > 0 ? ((tokens / total) * 100).toFixed(0) : '0'
    return `${name} ${humanTokens(tokens)} (${pct}%)`
  })

  if (shown.length < sorted.length) {
    const omitted = sorted.slice(shown.length)
    const omittedTokens = omitted.reduce((sum, [, tokens]) => sum + tokens, 0)
    const pct = total > 0 ? ((omittedTokens / total) * 100).toFixed(0) : '0'
    parts.push(`+${omitted.length} more ${humanTokens(omittedTokens)} (${pct}%)`)
  }

  return `    by tool   ${parts.join(' · ')}`
}

function compactRoleTokenParts(cms: Record<string, any>, sysTok: number, toolDefTok: number): string[] {
  const uTok = ((cms.user_tokens as number) ?? (cms.user as number)) ?? 0
  const aTok = ((cms.assistant_tokens as number) ?? (cms.assistant as number)) ?? 0
  const trTok = ((cms.tool_result_tokens as number) ?? (cms.tool as number)) ?? 0
  const imgTok = ((cms.image_tokens as number) ?? (cms.image as number)) ?? 0
  const effectiveSysTok = sysTok || ((cms.system_tokens as number) ?? (cms.system as number) ?? 0)
  const parts: string[] = []
  if (effectiveSysTok > 0) parts.push(`sys ${humanTokens(effectiveSysTok)}`)
  if (toolDefTok > 0) parts.push(`tools ${humanTokens(toolDefTok)}`)
  if (uTok > 0) parts.push(`user ${humanTokens(uTok)}`)
  if (aTok > 0) parts.push(`asst ${humanTokens(aTok)}`)
  if (trTok > 0) parts.push(`tool ${humanTokens(trTok)}`)
  const totalTokens = effectiveSysTok + toolDefTok + uTok + aTok + trTok + imgTok
  if (imgTok > 0) {
    const pct = totalTokens > 0 ? ` (${((imgTok / totalTokens) * 100).toFixed(0)}%)` : ''
    parts.push(`img ${humanTokens(imgTok)}${pct}`)
  }
  return parts
}

function compactMsgBreakdown(cms: Record<string, any> | undefined): string {
  if (!cms) return ''
  const normalized = {
    user_count: ((cms.user_count as number) ?? 0),
    assistant_count: ((cms.assistant_count as number) ?? 0),
    tool_result_count: ((cms.tool_result_count as number) ?? 0),
    image_count: ((cms.image_count as number) ?? 0),
    image_path_count: ((cms.image_path_count as number) ?? 0),
    image_base64_count: ((cms.image_base64_count as number) ?? 0),
  }
  return msgBreakdown(normalized)
}

function normalizeSummary(summary: string): string {
  return summary.replace(/^↓\s*/, '').replace(/,\s*/g, ' · ')
}

function compactLegend(legend: string): string {
  return legend
    .replace(/·=unchanged\/kept/g, '· kept')
    .replace(/·=kept/g, '· kept')
    .replace(/([A-Z])=([A-Za-z]+)/g, '$1 $2')
    .replace(/\s{2,}/g, '   ')
}

function formatAction(a: any, prefix: string): string {
  const idx = (a.index as number) ?? 0
  const endIdx = a.end_index as number | undefined
  const idxStr = endIdx != null ? `#${idx}..#${endIdx}` : `#${idx}`
  const toolName = (a.tool_name as string) ?? ''
  const method = (a.method as string) ?? 'unknown'
  const bTok = (a.before_tokens as number) ?? 0
  const aTok = (a.after_tokens as number) ?? 0
  const saved = bTok - aTok
  const name = method === 'Summarized' ? `turn(${1 + ((a.related_count as number) ?? 0)} msgs)` : toolName
  return `${prefix}${idxStr.padEnd(8)} ${name.padEnd(11)} ${method.padEnd(12)} ${humanTokens(bTok).padStart(5)} → ${humanTokens(aTok).padStart(5)}   −${humanTokens(saved)}`
}

// ---------------------------------------------------------------------------
// LLM call started
// ---------------------------------------------------------------------------

export function formatLlmCallStarted(data: Record<string, unknown>): string {
  const model = (data.model as string) ?? '?'
  const turn = (data.turn as number) ?? 0
  const attempt = (data.attempt as number) ?? 0
  const msgCount = (data.message_count as number) ?? 0
  const injectedCount = (data.injected_count as number) ?? 0
  const sysTok = (data.system_prompt_tokens as number) ?? 0
  const toolDefTok = (data.tool_definition_tokens as number) ?? 0
  const retryStr = attempt > 0 ? ` · retry ${attempt}` : ''
  const injectedStr = injectedCount > 0 ? ` · ${injectedCount} injected` : ''

  const ms = data.message_stats as Record<string, any> | undefined
  const lines: string[] = [`● LLM  ${model} · turn ${turn} · ${msgCount} msgs${msgBreakdown(ms)}${retryStr}${injectedStr}`]

  const contextWindow = (data.context_window as number) ?? 0
  const estimatedContextTokens = (data.estimated_context_tokens as number) ?? 0
  if (contextWindow > 0) {
    const total = estimatedContextTokens > 0
      ? estimatedContextTokens
      : ms
        ? sysTok + toolDefTok + (ms.user_tokens ?? 0) + (ms.assistant_tokens ?? 0) + (ms.tool_result_tokens ?? 0) + (ms.image_tokens ?? 0)
        : 0
    const line = contextLine(total, contextWindow)
    if (line) lines.push(line)
  }

  if (ms) {
    const line = roleTokensLine(roleTokenParts(ms, sysTok, toolDefTok))
    if (line) lines.push(line)
    const tools = toolTokensLine(ms)
    if (tools) lines.push(tools)
  } else {
    const bytes = (data.message_bytes as number) ?? 0
    const kb = bytes >= 1024 ? `${(bytes / 1024).toFixed(0)} KB` : `${bytes} B`
    lines.push(`    tokens    ${msgCount} msgs · ${kb} · sys ${humanTokens(sysTok)} · tools ${humanTokens(toolDefTok)}`)
  }

  return lines.join('\n')
}

export function formatLlmCallRetry(data: Record<string, unknown>): string {
  const attempt = (data.attempt as number) ?? 0
  const maxRetries = (data.max_retries as number) ?? 0
  const delayMs = (data.retry_delay_ms as number) ?? (data.delay_ms as number) ?? 0
  const error = (data.error as string) ?? ''
  const seconds = Math.max(0, Math.round(delayMs / 1000))
  const unit = seconds === 1 ? 'second' : 'seconds'
  const attemptStr = maxRetries > 0 ? ` · attempt ${attempt}/${maxRetries}` : ` · attempt ${attempt}`
  const lines = [`↻ LLM  retrying in ${seconds} ${unit}${attemptStr}`]
  if (error) lines.push(`    error     ${error}`)
  return lines.join('\n')
}

// ---------------------------------------------------------------------------
// LLM call completed
// ---------------------------------------------------------------------------

export function formatLlmCallCompleted(data: Record<string, unknown>): { text: string; expandedText?: string } {
  const model = data.model as string | undefined
  const turn = data.turn as number | undefined
  const error = data.error as string | undefined
  const usage = data.usage as Record<string, number> | undefined
  const metrics = data.metrics as Record<string, number> | undefined
  const durationMs = (data.duration_ms as number) ?? metrics?.duration_ms ?? 0

  if (error) {
    return { text: `✗ LLM  ${model ?? 'unknown'}${turn != null ? ` · turn ${turn}` : ''} · ${formatDuration(durationMs)}\n    error     ${error}` }
  }

  const inputTok = usage?.input ?? (data.input_tokens as number) ?? 0
  const outputTok = usage?.output ?? (data.output_tokens as number) ?? 0
  const tokPerSec = durationMs > 0 ? (outputTok / (durationMs / 1000)).toFixed(0) : '0'
  const ttfbMs = (data.time_to_first_byte_ms as number) ?? metrics?.ttfb_ms ?? 0
  const streamingMs = metrics?.streaming_ms ?? Math.max(0, durationMs - ttfbMs)
  const dur = durationMs || 1
  const ttfbPct = ((ttfbMs / dur) * 100).toFixed(0)
  const streamPct = ((streamingMs / dur) * 100).toFixed(0)

  const lines: string[] = []
  lines.push(`✓ LLM  ${model ?? 'unknown'}${turn != null ? ` · turn ${turn}` : ''} · ${formatDuration(durationMs)} · ${tokPerSec} tok/s`)
  lines.push(`    tokens    ${humanTokens(inputTok)} in → ${humanTokens(outputTok)} out`)
  lines.push(`    timing    ttfb ${(ttfbMs / 1000).toFixed(1)}s (${ttfbPct}%) · stream ${(streamingMs / 1000).toFixed(1)}s (${streamPct}%)`)

  const toolCalls = data.tool_calls as { id: string; name: string; arguments: Record<string, unknown> }[] | undefined
  let expandedCallsLine: string | undefined
  if (toolCalls && toolCalls.length > 0) {
    const json = JSON.stringify(toolCalls)
    const maxLen = 200
    if (json.length > maxLen) {
      lines.push(`    output    ${json.slice(0, maxLen)}…`)
      expandedCallsLine = `    output    ${json}`
    } else {
      lines.push(`    output    ${json}`)
    }
  }

  const compact = lines.join('\n')
  if (expandedCallsLine) {
    const expandedLines = [...lines.slice(0, -1), expandedCallsLine]
    return { text: compact, expandedText: expandedLines.join('\n') }
  }
  return { text: compact }
}

// ---------------------------------------------------------------------------
// Context compaction started
// ---------------------------------------------------------------------------

export function formatCompactionStarted(data: Record<string, unknown>): string {
  const msgCount = ((data.message_count as number) ?? (data.messages_count as number)) ?? 0
  const estTokens = (data.estimated_tokens as number) ?? 0
  const contextWindow = (data.context_window as number) ?? 0
  const sysTok = (data.system_prompt_tokens as number) ?? 0
  const toolDefTok = (data.tool_definition_tokens as number) ?? 0
  const cms = (data.message_stats as Record<string, any> | undefined) ?? (data.token_breakdown as Record<string, any> | undefined)
  const level = (data.level as string | undefined) ?? (data.level_name as string | undefined)
  const header = level ? `${level} · ${msgCount} msgs${compactMsgBreakdown(cms)}` : `${msgCount} msgs${compactMsgBreakdown(cms)}`
  const lines: string[] = [`● COMPACT  ${header}`]

  const ctx = contextLine(estTokens, contextWindow)
  if (ctx) lines.push(ctx)

  if (cms) {
    const line = roleTokensLine(compactRoleTokenParts(cms, sysTok, toolDefTok))
    if (line) lines.push(line)
  }

  return lines.join('\n')
}

// ---------------------------------------------------------------------------
// Context compaction completed
// ---------------------------------------------------------------------------

export function formatCompactionCompleted(data: Record<string, unknown>): string {
  const result = data.result as Record<string, any> | undefined

  if (!result) return '✓ COMPACT  done'

  const type = (result.type as string) ?? 'done'

  switch (type) {
    case 'no_op': {
      const contextWindow = (data.context_window as number) ?? 0
      const estTokens = (data.estimated_tokens as number) ?? 0
      if (contextWindow > 0 && estTokens > 0) {
        const pct = ((estTokens / contextWindow) * 100).toFixed(0)
        return `✓ COMPACT  skipped · within budget · ${humanTokens(estTokens)} / ${humanTokens(contextWindow)} · ${pct}%`
      }
      return '✓ COMPACT  skipped · within budget'
    }

    case 'run_once_cleared': {
      const saved = (result.saved_tokens as number) ?? 0
      const before = (result.before_estimated_tokens as number) ?? 0
      const after = (result.after_estimated_tokens as number) ?? 0
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      const contextWindow = (data.context_window as number) ?? 0

      const lines: string[] = []
      lines.push(`✓ COMPACT  cleared · ${humanTokens(before)} → ${humanTokens(after)} · saved ${humanTokens(saved)} (${savedPct}%)`)

      const ctx = contextLine(after, contextWindow, saved)
      if (ctx) lines.push(ctx)

      return lines.join('\n')
    }

    case 'level_done':
    case 'level_compacted': {
      const level = (result.level as number) ?? 0
      const beforeMsgs = ((result.before_message_count as number) ?? (result.messages_before as number)) ?? 0
      const afterMsgs = ((result.after_message_count as number) ?? (result.messages_after as number)) ?? 0
      const before = ((result.before_estimated_tokens as number) ?? (result.tokens_before as number)) ?? 0
      const after = ((result.after_estimated_tokens as number) ?? (result.tokens_after as number)) ?? 0
      const saved = before - after
      const savedPct = before > 0 ? ((saved / before) * 100).toFixed(0) : '0'
      const msgsDropped = (result.messages_dropped as number) ?? 0
      const deltaMsgs = beforeMsgs - afterMsgs

      const allActions = result.actions as any[] | undefined
      const sorted = allActions
        ? [...allActions]
            .filter((a: any) => a.method !== 'Skipped')
            .sort((a: any, b: any) => {
              const sa = (a.before_tokens ?? 0) - (a.after_tokens ?? 0)
              const sb = (b.before_tokens ?? 0) - (b.after_tokens ?? 0)
              return sb - sa
            })
        : []

      const { bar: generatedPosBar, legend: generatedLegend } = renderPositionBar(beforeMsgs, sorted, level)
      const posBar = (result.map as string | undefined)?.trim() || generatedPosBar
      const legend = (result.legend as string | undefined) || generatedLegend

      let summary: string
      if (level === 1) {
        const summarized = sorted.filter((a: any) => a.method === 'Summarized')
        if (summarized.length > 0) {
          const totalMsgs = summarized.reduce((s: number, a: any) => s + 1 + ((a.related_count as number) ?? 0), 0)
          summary = `summarized ${summarized.length} turns (${totalMsgs} msgs → ${summarized.length} summaries)`
        } else {
          const explicitSummary = result.result as string | undefined
          if (explicitSummary) {
            summary = normalizeSummary(explicitSummary)
          } else {
            const outlineCount = sorted.filter((a: any) => a.method === 'Outline').length
            const headtailCount = sorted.filter((a: any) => a.method === 'HeadTail').length
            const parts: string[] = []
            if (outlineCount > 0) parts.push(`outlined ${outlineCount}`)
            if (headtailCount > 0) parts.push(`head-tail ${headtailCount}`)
            summary = parts.length > 0 ? parts.join(' · ') : 'no changes'
          }
        }
      } else if (level === 2) {
        const kept = Math.max(afterMsgs - 1, 0)
        summary = `dropped ${msgsDropped} msgs · kept ${kept} + 1 marker`
      } else {
        summary = deltaMsgs > 0 ? `dropped ${deltaMsgs} msgs` : 'no changes'
      }

      const lines: string[] = []
      lines.push(`✓ COMPACT  L${level} · ${beforeMsgs} → ${afterMsgs} msgs · saved ${humanTokens(saved)} (${savedPct}%)`)

      const contextWindow = ((data.context_window as number) ?? (result.context_window as number)) ?? 0
      const ctx = contextLine(after, contextWindow, saved)
      if (ctx) lines.push(ctx)

      lines.push(`    summary   ${summary}`)
      lines.push(`    map       ${legend ? `${posBar}   ${compactLegend(legend)}` : posBar}`)

      const explicitDetails = result.details as string[] | undefined
      if (explicitDetails && explicitDetails.length > 0) {
        const [, ...rest] = explicitDetails
        const details = rest.length > 0 ? rest : explicitDetails
        const TOP = 3
        const TAIL = 2
        const shown = details.length <= TOP + TAIL ? details : [...details.slice(0, TOP), `… ${details.length - TOP - TAIL} more`, ...details.slice(details.length - TAIL)]
        const [first, ...tail] = shown
        lines.push(`    actions   ${(first ?? '').replace(/~/g, '')}`)
        for (const line of tail) lines.push(`              ${line.replace(/~/g, '')}`)
      } else if (sorted.length > 0) {
        const TOP = 3
        const TAIL = 2
        const shown = sorted.length <= TOP + TAIL ? sorted : [...sorted.slice(0, TOP), ...sorted.slice(sorted.length - TAIL)]
        const omitted = sorted.length - shown.length
        const omittedTokens = sorted.length > shown.length
          ? sorted.slice(TOP, sorted.length - TAIL).reduce((sum: number, a: any) => sum + ((a.before_tokens ?? 0) - (a.after_tokens ?? 0)), 0)
          : 0

        for (let i = 0; i < shown.length; i++) {
          if (i === TOP && omitted > 0) {
            lines.push(`              … ${omitted} more   −${humanTokens(omittedTokens)}`)
          }
          lines.push(formatAction(shown[i], i === 0 ? '    actions   ' : '              '))
        }
      }

      return lines.join('\n')
    }

    default:
      return `✓ COMPACT  ${type}`
  }
}

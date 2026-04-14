/**
 * RunSummary — detailed stats after a run completes (verbose mode).
 * Matches the Rust REPL's "This Run Summary" format with bars and breakdowns.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { RunStats } from '../state/AppState.js'
import { humanTokens, formatDuration, renderBar } from '../utils/format.js'

interface RunSummaryProps {
  stats: RunStats
}

export function RunSummary({ stats }: RunSummaryProps) {
  const totalTokens = stats.inputTokens + stats.outputTokens
  const durationSec = stats.durationMs / 1000
  const tokPerSec = durationSec > 0 ? (totalTokens / durationSec).toFixed(1) : '—'

  // Context budget
  const budget = stats.contextWindow > 0 ? stats.contextWindow - 4000 : 0 // approx sys prompt
  const ctxPct = budget > 0 ? ((stats.contextTokens / budget) * 100).toFixed(0) : '0'
  const ctxBar = renderBar(stats.contextTokens, budget, 20)

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>─── This Run Summary ──────────────────────────────────</Text>

      {/* Top line */}
      <Text dimColor>
        {formatDuration(stats.durationMs)} · {stats.turnCount} turns · {stats.llmCalls} llm calls · {stats.toolCallCount} tool calls · {humanTokens(totalTokens)} tokens
      </Text>

      {/* Context budget */}
      {budget > 0 && (
        <Text dimColor>
          {'  context   '}{ctxBar}  {ctxPct}%({humanTokens(stats.contextTokens)}) of budget({humanTokens(budget)})
        </Text>
      )}

      {/* Token breakdown */}
      <Text dimColor>{''}</Text>
      <Text dimColor>
        {'  tokens    '}{humanTokens(stats.inputTokens)} input · {humanTokens(stats.outputTokens)} output · {tokPerSec} tok/s
      </Text>

      {stats.cacheReadTokens > 0 && (
        <Text dimColor>
          {'            cache read '}{humanTokens(stats.cacheReadTokens)} · cache write {humanTokens(stats.cacheWriteTokens)}
        </Text>
      )}

      {/* LLM call breakdown */}
      {stats.llmCallDetails.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          <Text dimColor>
            {'  llm       '}{stats.llmCalls} calls · {formatDuration(stats.durationMs)} ({pct(llmTotalMs(stats), stats.durationMs)} of run) · {tokPerSec} tok/s avg
          </Text>
          {stats.llmCallDetails.length > 1 && (
            <Text dimColor>
              {'            ttft avg '}{formatDuration(avgTtft(stats))} · stream avg {formatDuration(avgStream(stats))}
            </Text>
          )}
          {stats.llmCallDetails.map((call, i) => (
            <Text key={i} dimColor>
              {'            #'}{i + 1}  {formatDuration(call.durationMs)} {renderBar(call.durationMs, stats.durationMs, 20)} {pct(call.durationMs, stats.durationMs)}
            </Text>
          ))}
        </>
      )}

      {/* Tool breakdown */}
      {stats.toolBreakdown.length > 0 && (
        <>
          <Text dimColor>{''}</Text>
          {stats.toolBreakdown
            .sort((a, b) => b.count - a.count)
            .map((tc, i) => (
              <Text key={i} dimColor>
                {'              '}{tc.name}  {tc.count} call{tc.count > 1 ? 's' : ''}  {formatDuration(tc.totalDurationMs)}
                {tc.errors > 0 ? `  (${tc.errors} error${tc.errors > 1 ? 's' : ''})` : ''}
              </Text>
            ))}
        </>
      )}

      <Text dimColor>────────────────────────────────────────────────────────</Text>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function pct(value: number, total: number): string {
  if (total <= 0) return '0%'
  return `${((value / total) * 100).toFixed(1)}%`
}

function llmTotalMs(stats: RunStats): number {
  return stats.llmCallDetails.reduce((sum, c) => sum + c.durationMs, 0)
}

function avgTtft(stats: RunStats): number {
  if (stats.llmCallDetails.length === 0) return 0
  return stats.llmCallDetails.reduce((sum, c) => sum + c.ttftMs, 0) / stats.llmCallDetails.length
}

function avgStream(stats: RunStats): number {
  if (stats.llmCallDetails.length === 0) return 0
  return stats.llmCallDetails.reduce((sum, c) => sum + (c.durationMs - c.ttftMs), 0) / stats.llmCallDetails.length
}
/**
 * RunSummary — displays detailed stats after a run completes.
 * Only shown when verbose mode is enabled (Ctrl+L toggle).
 * Mirrors the Rust REPL's "This Run Summary" format.
 */

import React from 'react'
import { Text, Box } from 'ink'
import type { RunStats } from '../state/AppState.js'

interface RunSummaryProps {
  stats: RunStats
}

export function RunSummary({ stats }: RunSummaryProps) {
  const totalTokens = stats.inputTokens + stats.outputTokens
  const durationSec = stats.durationMs / 1000
  const tokPerSec = durationSec > 0 ? (totalTokens / durationSec).toFixed(1) : '—'

  return (
    <Box flexDirection="column" marginBottom={1}>
      <Text dimColor>─── This Run Summary ──────────────────────────────────</Text>

      {/* Top line */}
      <Text dimColor>
        {formatDuration(stats.durationMs)} · {stats.turnCount} turns · {stats.llmCalls} llm calls · {stats.toolCallCount} tool calls · {humanTokens(totalTokens)} tokens
      </Text>

      {/* Token breakdown */}
      <Text dimColor>{''}</Text>
      <Text dimColor>
        {'  tokens    '}{humanTokens(stats.inputTokens)} total input · {stats.outputTokens} output · {tokPerSec} tok/s
      </Text>

      {stats.cacheReadTokens > 0 && (
        <Text dimColor>
          {'            cache read '}{humanTokens(stats.cacheReadTokens)} · cache write {humanTokens(stats.cacheWriteTokens)}
        </Text>
      )}

      {/* LLM calls */}
      {stats.llmCalls > 0 && (
        <>
          <Text dimColor>{''}</Text>
          <Text dimColor>
            {'  llm       '}{stats.llmCalls} calls · {formatDuration(stats.durationMs)} · {tokPerSec} tok/s avg
          </Text>
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

function humanTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}k`
  return `${n}`
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`
  return `${(ms / 1000).toFixed(1)}s`
}

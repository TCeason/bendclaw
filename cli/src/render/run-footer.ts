/**
 * The closing line of a run: how long the whole thing took, and when it landed.
 *
 * Committed once, after the run settles. The spinner already reports a live
 * clock, but that line is transient — it is erased when the run ends, so
 * scrollback kept no record of what a turn cost. This is that record, and it
 * measures the full run (every model call and tool round-trip), not the last
 * streaming segment.
 *
 * A run too short to have a meaningful duration produces no line at all rather
 * than a misleading `0s`.
 */

import chalk from 'chalk'

import { formatElapsed, formatWallClock } from './format.js'
import type { OutputLine } from './output.js'
import { SECTION_MUTED } from './section.js'

/** Below this, `formatElapsed` rounds to `0s` and the line says nothing. */
const MIN_FOOTER_MS = 1000

/** Matches the spinner's settled glyph, so the run closes on the mark it ran under. */
const FOOTER_GLYPH = '✳'

export function runFooterText(startedAt: number, finishedAt: number): string | null {
  const elapsed = finishedAt - startedAt
  if (!Number.isFinite(elapsed) || elapsed < MIN_FOOTER_MS) return null
  return `${FOOTER_GLYPH} Ran for ${formatElapsed(elapsed)} · done ${formatWallClock(finishedAt)}`
}

/**
 * The committed footer, or `null` when the run was too short to report.
 *
 * Pre-styled: the line carries its own muted paint, so it must be committed
 * with `preStyled` set or the system treatment would flatten the glyph and the
 * text into one flat gray anyway — same reasoning as `/skill`.
 */
export function buildRunFooterLine(startedAt: number, finishedAt: number): OutputLine | null {
  const text = runFooterText(startedAt, finishedAt)
  if (!text) return null
  return {
    id: 'run-footer',
    kind: 'system',
    text: `  ${chalk.hex(SECTION_MUTED)(text)}`,
    preStyled: true,
  }
}

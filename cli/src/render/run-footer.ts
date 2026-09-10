import chalk from 'chalk'

import { formatElapsed, formatWallClock } from './format.js'
import type { OutputLine } from './output.js'
import { SECTION_MUTED } from './section.js'

const MIN_FOOTER_MS = 1000

const FOOTER_GLYPH = '✳'

function turnSegment(turns: number): string {
  if (!Number.isFinite(turns) || turns < 1) return ''
  const whole = Math.floor(turns)
  return ` · ${whole} ${whole === 1 ? 'turn' : 'turns'}`
}

export function runFooterText(startedAt: number, finishedAt: number, turns = 0): string | null {
  const elapsed = finishedAt - startedAt
  if (!Number.isFinite(elapsed) || elapsed < MIN_FOOTER_MS) return null
  return `${FOOTER_GLYPH} Ran for ${formatElapsed(elapsed)}${turnSegment(turns)} · done ${formatWallClock(finishedAt)}`
}

export function buildRunFooterLine(
  startedAt: number,
  finishedAt: number,
  turns = 0,
): OutputLine | null {
  const text = runFooterText(startedAt, finishedAt, turns)
  if (!text) return null
  return {
    id: 'run-footer',
    kind: 'system',
    text: `  ${chalk.hex(SECTION_MUTED)(text)}`,
    preStyled: true,
  }
}

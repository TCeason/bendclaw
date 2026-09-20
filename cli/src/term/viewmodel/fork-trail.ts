/**
 * Breadcrumb for a forked session's ancestry.
 *
 * The trail is the chain from the root session to the current one. It is a
 * position indicator, not navigation: the footer shows where you are, while
 * `/back` and `/resume` move. Roots (trail length ≤ 1) render nothing, so a
 * session that never forked looks exactly as before.
 */

import stringWidth from 'string-width'
import { truncateToWidth } from './width.js'

/** Same edge the `/sessions` graph draws, so the trail reads as "this row
 * hangs off that one" wherever it appears. Box drawing renders everywhere. */
export const FORK_GLYPH = '└─'

/** Session titles root → current, ready for the footer. Empty = not a fork. */
export function forkTrailTitles(
  lineage: ReadonlyArray<{ session_id: string; title?: string | null; custom_title?: string | null }>,
): string[] {
  if (lineage.length <= 1) return []
  return lineage.map(meta => sessionLabel(meta))
}

export function sessionLabel(meta: { session_id: string; title?: string | null; custom_title?: string | null }): string {
  const title = (meta.custom_title ?? meta.title ?? '').trim()
  return title || meta.session_id.slice(0, 8)
}

/** Longest parent title shown in the fork row before it is cut. */
const PARENT_TITLE_MAX_WIDTH = 40

/**
 * The fork row's text: `fork of <parent>`. It only marks the session as a
 * fork and names what it hangs off; the full chain lives in `/sessions`,
 * where there is room to draw it.
 */
export function formatForkTrail(trail: readonly string[], maxWidth: number): string {
  if (trail.length <= 1 || maxWidth <= 0) return ''
  const parent = trail[trail.length - 2] ?? ''
  const label = 'fork of '
  const room = Math.min(PARENT_TITLE_MAX_WIDTH, maxWidth - stringWidth(label))
  if (room < 2) return truncateToWidth('fork', maxWidth)
  return `${label}${truncateToWidth(parent, room)}`
}

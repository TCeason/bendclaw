export interface LiveHeightUpdate {
  maxHeight: number
  padding: number
}

/**
 * Keep a live region's trailing edge monotonic while its content is being
 * reparsed. Markdown can legitimately shrink when an incomplete prefix becomes
 * a list, table, or fence; padding absorbs that shrink so the footer below does
 * not move up and then back down.
 */
export function updateLiveHeight(
  maxHeight: number,
  currentHeight: number,
  active: boolean,
  maxPadding = 8,
): LiveHeightUpdate {
  if (!active) return { maxHeight: 0, padding: 0 }
  const safeMax = Number.isFinite(maxHeight) ? Math.max(0, Math.floor(maxHeight)) : 0
  const safeCurrent = Number.isFinite(currentHeight) ? Math.max(0, Math.floor(currentHeight)) : 0
  const safePadding = Number.isFinite(maxPadding) ? Math.max(0, Math.floor(maxPadding)) : 0
  const retainedMax = Math.min(safeMax, safeCurrent + safePadding)
  const nextMax = Math.max(retainedMax, safeCurrent)
  return { maxHeight: nextMax, padding: nextMax - safeCurrent }
}

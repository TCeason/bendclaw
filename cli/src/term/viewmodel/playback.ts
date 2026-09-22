/** One playback rule, shared by every kind of slot item.
 *
 * Items play once each, highest priority first, and then nothing plays.
 * There is no loop: when every item has been played, `nextToPlay` returns
 * null and the caller goes quiet. What makes an item "the same item" is its
 * `key`, which the caller chooses — a notice keys on its copy, an ad on its
 * id — so this module knows nothing about either.
 */

export interface Playable {
  id: string
  priority?: number
  /** Identity for "already played". Stable for as long as the item is unchanged. */
  key: string
}

/** Highest priority first. Equal priorities keep their input order. */
export function byPriority<T extends Playable>(items: readonly T[]): T[] {
  return items
    .map((item, index) => ({ item, index }))
    .sort((a, b) => (b.item.priority ?? 0) - (a.item.priority ?? 0) || a.index - b.index)
    .map(entry => entry.item)
}

/** The next item to play: not yet played, not `exceptId`, highest priority.
 * Null when nothing qualifies — the caller stops. */
export function nextToPlay<T extends Playable>(
  items: readonly T[],
  played: ReadonlySet<string>,
  exceptId?: string,
): T | null {
  return byPriority(items).find(item => item.id !== exceptId && !played.has(item.key)) ?? null
}

/**
 * Terminal size tiers for the prompt region.
 *
 * Layout choices used to sit as independent thresholds — one for rails, one for
 * the placeholder, one for the blank-row floor, one for the completion height.
 * Nothing related them, so adding a tier meant guessing which constant it
 * belonged beside. These tiers are that missing spine: a layout choice names
 * the tier it reacts to instead of carrying its own number.
 *
 * Boundaries are the ones the layout already used, so tiering is a refactor,
 * not a redesign. The gutter keeps its own step table: it scales smoothly with
 * width rather than switching on a semantic tier.
 */

/**
 * `xs` <30 — too narrow for rails, which cost 4 of very few columns.
 * `sm` 30-64 — rails fit; only the short placeholder does.
 * `md` 65+ — room for the full placeholder.
 */
export type WidthTier = 'xs' | 'sm' | 'md'

/**
 * `xs` <10 — a framed composer plus footer would claim the whole screen.
 * `sm` 10-19 — borders fit, decorative blank rows do not.
 * `md` 20-35 — room for the blank-row floor that centres a short draft.
 * `lg` 36+ — room for a taller completion viewport.
 */
export type HeightTier = 'xs' | 'sm' | 'md' | 'lg'

const WIDTH_TIERS: readonly { min: number; tier: WidthTier }[] = [
  { min: 65, tier: 'md' },
  { min: 30, tier: 'sm' },
  { min: 0, tier: 'xs' },
]

const HEIGHT_TIERS: readonly { min: number; tier: HeightTier }[] = [
  { min: 36, tier: 'lg' },
  { min: 20, tier: 'md' },
  { min: 10, tier: 'sm' },
  { min: 0, tier: 'xs' },
]

const WIDTH_ORDER: readonly WidthTier[] = ['xs', 'sm', 'md']
const HEIGHT_ORDER: readonly HeightTier[] = ['xs', 'sm', 'md', 'lg']

export function widthTier(columns: number): WidthTier {
  return WIDTH_TIERS.find(step => columns >= step.min)?.tier ?? 'xs'
}

export function heightTier(rows: number): HeightTier {
  return HEIGHT_TIERS.find(step => rows >= step.min)?.tier ?? 'xs'
}

/** True when `tier` is `min` or roomier. Reads as "at least this much space". */
export function atLeastWidth(tier: WidthTier, min: WidthTier): boolean {
  return WIDTH_ORDER.indexOf(tier) >= WIDTH_ORDER.indexOf(min)
}

export function atLeastHeight(tier: HeightTier, min: HeightTier): boolean {
  return HEIGHT_ORDER.indexOf(tier) >= HEIGHT_ORDER.indexOf(min)
}

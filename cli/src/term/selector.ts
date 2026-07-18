export interface SelectorItem {
  label: string
  detail?: string
  /** Renders as a non-focusable group divider (e.g. a provider name). */
  header?: boolean
  /** Marks the active choice without mixing status into detail text. */
  selected?: boolean
  /** Opaque identifier (e.g. full session id) — not displayed. */
  id?: string
  /** Extra text searched but not displayed (e.g. full session id, cwd). */
  searchText?: string
  /** When false, up/down navigation skips this item. Defaults to true. */
  focusable?: boolean
}

export interface SelectorState {
  items: SelectorItem[]
  allItems: SelectorItem[]
  focusIndex: number
  /** First visible row. Moves only when focus walks past a window edge, so
   *  the list slides one row at a time instead of recentering (droid-style). */
  scrollOffset: number
  title: string
  /** Optional secondary context displayed below the title. */
  subtitle?: string
  /** Model selection uses pi's editor-replacement component instead of the
   * generic titled selector. */
  presentation?: 'model'
  /** Wraps up/down navigation between the first and last focusable items. */
  circularNavigation?: boolean
  query: string
}

/** Rows visible at once in the selector window. Shared by movement logic and
 *  the renderer so scroll behavior and display stay in sync. */
export const SELECTOR_VIEWPORT = 10

/** Slide the window minimally so `focus` is visible: no jumps, no recentering. */
function ensureVisible(offset: number, focus: number, total: number): number {
  const maxOffset = Math.max(0, total - SELECTOR_VIEWPORT)
  let next = Math.min(Math.max(offset, 0), maxOffset)
  if (focus < next) next = focus
  else if (focus >= next + SELECTOR_VIEWPORT) next = focus - SELECTOR_VIEWPORT + 1
  return Math.min(next, maxOffset)
}

/** Find the first focusable index in items, defaulting to 0. */
function firstFocusable(items: SelectorItem[]): number {
  const idx = items.findIndex(i => i.focusable !== false)
  return idx >= 0 ? idx : 0
}

function lastFocusable(items: SelectorItem[]): number {
  for (let i = items.length - 1; i >= 0; i--) {
    if (items[i]?.focusable !== false) return i
  }
  return 0
}

export function createSelectorState(title: string, items: SelectorItem[], allItems?: SelectorItem[], initialQuery?: string): SelectorState {
  const all = allItems ?? items
  if (initialQuery) {
    return applyFilter({ items: all, allItems: all, focusIndex: 0, scrollOffset: 0, title, query: '' }, initialQuery)
  }
  const focusIndex = firstFocusable(items)
  return { items, allItems: all, focusIndex, scrollOffset: ensureVisible(0, focusIndex, items.length), title, query: '' }
}

/** Move focus to the first item matching `predicate`, keeping it visible. */
export function selectorFocusOn(state: SelectorState, predicate: (item: SelectorItem) => boolean): SelectorState {
  const idx = state.items.findIndex(i => i.focusable !== false && predicate(i))
  if (idx < 0) return state
  return { ...state, focusIndex: idx, scrollOffset: ensureVisible(state.scrollOffset, idx, state.items.length) }
}

export function selectorUp(state: SelectorState): SelectorState {
  let next = state.focusIndex - 1
  while (next >= 0 && state.items[next]?.focusable === false) next--
  if (next < 0) {
    if (!state.circularNavigation || state.items.length === 0) return state
    next = lastFocusable(state.items)
    if (state.items[next]?.focusable === false) return state
    return {
      ...state,
      focusIndex: next,
      scrollOffset: ensureVisible(Math.max(0, state.items.length - SELECTOR_VIEWPORT), next, state.items.length),
    }
  }
  // When only headers remain above, slide the window to the very top so the
  // leading group divider scrolls into view with its first model.
  const anyFocusableAbove = state.items.slice(0, next).some(i => i.focusable !== false)
  const target = anyFocusableAbove ? next : 0
  return { ...state, focusIndex: next, scrollOffset: ensureVisible(state.scrollOffset, target, state.items.length) }
}

export function selectorDown(state: SelectorState): SelectorState {
  let next = state.focusIndex + 1
  while (next < state.items.length && state.items[next]?.focusable === false) next++
  if (next >= state.items.length) {
    if (!state.circularNavigation || state.items.length === 0) return state
    next = firstFocusable(state.items)
    if (state.items[next]?.focusable === false) return state
    // Include a leading group header when wrapping back to its first model.
    const target = state.items.slice(0, next).some(i => i.focusable !== false) ? next : 0
    return { ...state, focusIndex: next, scrollOffset: ensureVisible(0, target, state.items.length) }
  }
  return { ...state, focusIndex: next, scrollOffset: ensureVisible(state.scrollOffset, next, state.items.length) }
}

export function selectorSelect(state: SelectorState): SelectorItem | null {
  return state.items[state.focusIndex] ?? null
}

export function selectorType(state: SelectorState, char: string): SelectorState {
  const query = state.query + char
  return applyFilter(state, query)
}

export function selectorBackspace(state: SelectorState): SelectorState {
  if (state.query.length === 0) return state
  const query = state.query.slice(0, -1)
  return applyFilter(state, query)
}

export function selectorExpandItems(state: SelectorState, allItems: SelectorItem[]): SelectorState {
  const updated = { ...state, allItems }
  return state.query ? applyFilter(updated, state.query) : { ...updated, items: allItems }
}

export function selectorClearQuery(state: SelectorState): SelectorState {
  if (!state.query) return state
  return applyFilter(state, '')
}

export function selectorRemoveItem(state: SelectorState, index: number): SelectorState {
  const label = state.items[index]?.label
  if (!label) return state
  const items = state.items.filter((_, i) => i !== index)
  const allItems = state.allItems.filter(i => i.label !== label)
  const focusIndex = Math.min(state.focusIndex, Math.max(0, items.length - 1))
  return { ...state, items, allItems, focusIndex, scrollOffset: ensureVisible(state.scrollOffset, focusIndex, items.length) }
}

function searchableText(item: SelectorItem): string {
  if (item.searchText) return item.searchText.toLowerCase()
  return `${item.label} ${item.detail ?? ''}`.toLowerCase()
}

function isSubsequence(text: string, query: string): boolean {
  let j = 0
  for (let i = 0; i < text.length && j < query.length; i++) {
    if (text[i] === query[j]) j++
  }
  return j === query.length
}

function extractContext(source: string, query: string, width: number): string | null {
  const lower = source.toLowerCase()
  const idx = lower.indexOf(query.toLowerCase())
  if (idx === -1) return null
  const half = Math.floor((width - query.length) / 2)
  const start = Math.max(0, idx - half)
  const end = Math.min(source.length, idx + query.length + half)
  let snippet = source.slice(start, end).replace(/\n/g, ' ')
  if (start > 0) snippet = '…' + snippet
  if (end < source.length) snippet = snippet + '…'
  return snippet
}

function fuzzyMatchScore(query: string, text: string): number | null {
  const normalizedQuery = query.toLowerCase()
  const normalizedText = text.toLowerCase()
  if (normalizedQuery.length === 0) return 0
  if (normalizedQuery.length > normalizedText.length) return null

  let queryIndex = 0
  let score = 0
  let lastMatchIndex = -1
  let consecutiveMatches = 0
  for (let index = 0; index < normalizedText.length && queryIndex < normalizedQuery.length; index++) {
    if (normalizedText[index] !== normalizedQuery[queryIndex]) continue
    const atWordBoundary = index === 0 || /[\s\-_./:]/.test(normalizedText[index - 1]!)
    if (lastMatchIndex === index - 1) {
      consecutiveMatches++
      score -= consecutiveMatches * 5
    } else {
      consecutiveMatches = 0
      if (lastMatchIndex >= 0) score += (index - lastMatchIndex - 1) * 2
    }
    if (atWordBoundary) score -= 10
    score += index * 0.1
    lastMatchIndex = index
    queryIndex++
  }

  if (queryIndex < normalizedQuery.length) return null
  if (normalizedQuery === normalizedText) score -= 100
  return score
}

function modelFuzzyScore(query: string, item: SelectorItem): number | null {
  const tokens = query.trim().split(/[\s/]+/).filter(Boolean)
  if (tokens.length === 0) return 0
  const text = item.searchText ?? `${item.label} ${item.detail ?? ''}`
  let total = 0
  for (const token of tokens) {
    const primary = fuzzyMatchScore(token, text)
    if (primary !== null) {
      total += primary
      continue
    }
    const alphaNumeric = /^(?<letters>[a-z]+)(?<digits>[0-9]+)$/i.exec(token)
    const numericAlpha = /^(?<digits>[0-9]+)(?<letters>[a-z]+)$/i.exec(token)
    const swapped = alphaNumeric
      ? `${alphaNumeric.groups?.digits ?? ''}${alphaNumeric.groups?.letters ?? ''}`
      : numericAlpha
        ? `${numericAlpha.groups?.letters ?? ''}${numericAlpha.groups?.digits ?? ''}`
        : ''
    const swappedScore = swapped ? fuzzyMatchScore(swapped, text) : null
    if (swappedScore === null) return null
    total += swappedScore + 5
  }
  return total
}

function applyFilter(state: SelectorState, query: string): SelectorState {
  if (!query) {
    const focusIndex = firstFocusable(state.allItems)
    return { ...state, query, items: state.allItems, focusIndex, scrollOffset: ensureVisible(0, focusIndex, state.allItems.length) }
  }

  if (state.presentation === 'model') {
    const filtered = state.allItems
      .filter(item => !item.header)
      .map((item, index) => ({ item, index, score: modelFuzzyScore(query, item) }))
      .filter((entry): entry is { item: SelectorItem; index: number; score: number } => entry.score !== null)
      .sort((left, right) => left.score - right.score || left.index - right.index)
      .map(entry => entry.item)
    const focusIndex = firstFocusable(filtered)
    return { ...state, query, items: filtered, focusIndex, scrollOffset: ensureVisible(0, focusIndex, filtered.length) }
  }

  const lower = query.toLowerCase()
  const exact: SelectorItem[] = []
  const fuzzy: SelectorItem[] = []
  for (const item of state.allItems) {
    // Group dividers are dropped while filtering: results are ranked flat.
    if (item.header) continue
    const text = searchableText(item)
    if (text.includes(lower)) {
      exact.push(withContext(item, query))
    } else if (!item.searchText && isSubsequence(text, lower)) {
      fuzzy.push(item)
    }
  }
  const filtered = exact.concat(fuzzy)
  const focusIndex = firstFocusable(filtered)
  return { ...state, query, items: filtered, focusIndex, scrollOffset: ensureVisible(0, focusIndex, filtered.length) }
}

function withContext(item: SelectorItem, query: string): SelectorItem {
  if (!item.searchText) return item
  const ctx = extractContext(item.searchText, query, 80)
  if (!ctx) return item
  return { ...item, detail: ctx }
}

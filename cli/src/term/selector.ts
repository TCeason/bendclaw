export interface SelectorItem {
  label: string
  detail?: string
}

export interface SelectorState {
  items: SelectorItem[]
  focusIndex: number
  title: string
}

export function createSelectorState(title: string, items: SelectorItem[]): SelectorState {
  return { items, focusIndex: 0, title }
}

export function selectorUp(state: SelectorState): SelectorState {
  if (state.focusIndex <= 0) return state
  return { ...state, focusIndex: state.focusIndex - 1 }
}

export function selectorDown(state: SelectorState): SelectorState {
  if (state.focusIndex >= state.items.length - 1) return state
  return { ...state, focusIndex: state.focusIndex + 1 }
}

export function selectorSelect(state: SelectorState): SelectorItem | null {
  return state.items[state.focusIndex] ?? null
}

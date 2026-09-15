import { createSelectorState, type SelectorItem, type SelectorState } from '../selector.js'

/** In-memory feature ownership, never inferred from copy or visual styling. */
export const SELECTOR_OWNER = {
  model: Symbol('model'),
  taskModel: Symbol('task-model'),
  resume: Symbol('resume'),
  shares: Symbol('shares'),
  skill: Symbol('skill'),
  queue: Symbol('queue'),
  background: Symbol('background'),
  backgroundOutput: Symbol('background-output'),
  task: Symbol('task'),
} as const

export function createAppSelectorState(
  owner: keyof typeof SELECTOR_OWNER,
  title: string,
  items: SelectorItem[],
  allItems?: SelectorItem[],
  initialQuery?: string,
): SelectorState {
  return { ...createSelectorState(title, items, allItems, initialQuery), owner: SELECTOR_OWNER[owner] }
}

export function isBackgroundSelector(state: SelectorState): boolean {
  return state.owner === SELECTOR_OWNER.background || state.owner === SELECTOR_OWNER.backgroundOutput
}

export function isCommandSelector(state: SelectorState): boolean {
  return state.owner === SELECTOR_OWNER.model
    || state.owner === SELECTOR_OWNER.resume
    || state.owner === SELECTOR_OWNER.skill
}

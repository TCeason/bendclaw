import type { ConfigInfo } from '../../native/contracts/config-info.js'
import { selectorFocusOn, type SelectorEffort, type SelectorItem, type SelectorState } from '../selector.js'
import { createAppSelectorState } from './selector-identity.js'
import { currentModelSpec, modelOptions, modelSelectorItems } from './provider.js'
import { RESUME_SELECTOR_TITLE } from './resume.js'

/** One factory for preview and explicitly opened model windows. */
export function createModelWindow(config: ConfigInfo | undefined, model: string, listFocused = false): SelectorState {
  const models = modelOptions(config, model)
  const activeSpec = currentModelSpec(config, model)
  return selectorFocusOn({
    ...createAppSelectorState('model', 'Models', modelSelectorItems(models, activeSpec, config?.thinkingLevel)),
    presentation: 'model',
    circularNavigation: true,
    listFocused,
  }, item => item.id === activeSpec)
}

/**
 * Carry already-adjusted tiers onto a freshly built row set.
 *
 * The catalog refreshes on a timer while the picker is open. Rebuilding rows
 * from the addon would silently reset a tier the user just moved, so an
 * adjusted row keeps its choice — but only when the incoming ladder is
 * identical. A changed ladder means the model's own capabilities moved, and the
 * server's ordering then wins over a stale index.
 */
export function carryModelEfforts(previous: SelectorItem[], next: SelectorItem[]): SelectorItem[] {
  const adjusted = new Map<string, SelectorEffort>()
  for (const item of previous) {
    if (item.effort && item.id !== undefined) adjusted.set(item.id, item.effort)
  }
  if (adjusted.size === 0) return next
  return next.map(item => {
    const carried = item.id === undefined ? undefined : adjusted.get(item.id)
    if (!carried || !item.effort) return item
    const sameLadder = carried.levels.length === item.effort.levels.length
      && carried.levels.every((level, at) => level === item.effort!.levels[at])
    return sameLadder ? { ...item, effort: carried } : item
  })
}

export function createResumeWindow(items: SelectorItem[], initialQuery?: string): SelectorState {
  const state = createAppSelectorState('resume', RESUME_SELECTOR_TITLE, items, items, initialQuery)
  return {
    ...state,
    listFocused: false,
    lowercaseHints: true,
    ...(state.query.length === 0 && state.items.length === 0 && state.allItems.some(item => !item.header)
      ? { emptyMessage: 'No sessions in current cwd · type to search all sessions' }
      : {}),
  }
}

import type { KeyEvent } from './input.js'
import type { SelectorState } from './selector.js'
import type { Hint } from './design/key-hints.js'
import { previewGeometry, previewScrollLimit } from './preview-scroll.js'
import { SELECTOR_OWNER } from './app/selector-identity.js'

export type PaneAction = { kind: 'update'; state: SelectorState } | { kind: 'none' }

/** Shared focus/scroll ownership. null delegates to the feature's list actions;
 * 'none' consumes a key in the detail pane so it cannot mutate the list. */
export function handleSplitPaneKey(state: SelectorState, event: KeyEvent, columns = 80, rows = 24): PaneAction | null {
  const pane = state.previewPane
  if (!pane) return null
  const selected = state.items[state.focusIndex]
  const disarm = (): SelectorState => state.pendingDeleteId
    ? { ...state, pendingDeleteId: undefined, subtitle: undefined } : state
  if (event.type === 'escape' && state.pendingDeleteId) return { kind: 'update', state: disarm() }
  if (event.type === 'tab' || event.type === 'shift-tab') {
    if (!selected?.preview?.length) return { kind: 'none' }
    return { kind: 'update', state: { ...disarm(), listFocused: true, previewPane: { ...pane, focused: !pane.focused } } }
  }
  if (pane.focused && event.type === 'escape') {
    return { kind: 'update', state: { ...state, listFocused: true, previewPane: { ...pane, focused: false } } }
  }
  const down = event.type === 'down' || (state.noFilter && event.type === 'char' && event.char === 'j')
  const up = event.type === 'up' || (state.noFilter && event.type === 'char' && event.char === 'k')
  if (event.type === 'page-up' || event.type === 'page-down' || (pane.focused && (up || down))) {
    if (!selected?.preview?.length) return { kind: 'none' }
    const geometry = previewGeometry(columns, rows, pane.fraction)
    const max = previewScrollLimit(selected.preview, geometry.width, geometry.height)
    const offset = Math.min(pane.offset, max)
    const delta = event.type === 'page-down' ? geometry.height - 1
      : event.type === 'page-up' ? 1 - geometry.height : down ? 1 : -1
    return { kind: 'update', state: { ...disarm(), previewPane: {
      ...pane, offset: Math.max(0, Math.min(max, offset + delta)),
    } } }
  }
  return pane.focused ? { kind: 'none' } : null
}

/** Reset detail scrolling only when selection/filter changes, not on repaint. */
export function resetPaneForSelection(previous: SelectorState, next: SelectorState): SelectorState {
  if (!next.previewPane || (previous.query === next.query
    && previous.items[previous.focusIndex]?.id === next.items[next.focusIndex]?.id)) return next
  return { ...next, previewPane: { ...next.previewPane, offset: 0, focused: false } }
}

/** Contextual hints: the shell owns navigation, features supply domain actions. */
export function splitPaneHints(state: SelectorState): Hint[] {
  const selected = state.items[state.focusIndex]
  if (state.previewPane?.focused) return [
    { keys: ['up', 'down'], action: 'scroll' },
    { keys: 'tab', action: 'list' },
    { keys: 'escape', action: 'back' },
  ]
  if (selected?.id && state.pendingDeleteId === selected.id && state.previewPane?.confirmDeleteKey) return [
    { keys: state.previewPane.confirmDeleteKey, action: 'confirm delete', confirmationPending: true },
    { keys: 'escape', action: 'cancel' },
  ]
  // A resume or shares list whose filter owns the input — the `/resume` command
  // preview, or after pressing `/` — takes letters as search text, so only the
  // gestures that work there are offered. `↑/↓` selects in both cases: on the
  // preview it also promotes the window.
  if ((state.owner === SELECTOR_OWNER.resume || state.owner === SELECTOR_OWNER.shares)
    && state.listFocused !== true) return [
    { keys: 'type', action: 'search' },
    { keys: ['up', 'down'], action: 'select' },
    { keys: 'escape', action: state.query ? 'clear search' : 'close' },
  ]
  if (selected?.pendingAction) return [
    { keys: ['up', 'down'], action: 'select' },
    ...(selected.preview?.length ? [{ keys: 'tab', action: 'details' }] : []),
    { keys: 'escape', action: 'close' },
  ]
  return selected?.hints ?? state.hints ?? [{ keys: 'escape', action: 'close' }]
}

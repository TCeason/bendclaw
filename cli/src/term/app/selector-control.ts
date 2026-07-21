import type { KeyEvent } from '../input.js'
import {
  selectorBackspace,
  selectorDown,
  selectorRemoveItem,
  selectorSelect,
  selectorType,
  selectorUp,
  type SelectorState,
} from '../selector.js'
import { decideQueueSelectorAction, isQueueSelectorTitle, type ManagedQueuedPrompt } from './queue-manage.js'
import { isResumeSelectorTitle } from './resume.js'

export type SelectorControlAction =
  | { kind: 'update'; state: SelectorState }
  | { kind: 'close' }
  | { kind: 'resume'; sessionId: string }
  | { kind: 'select-model'; spec: string }
  | { kind: 'delete-session'; sessionId: string; label: string; state: SelectorState }
  | { kind: 'queue-edit'; entry: ManagedQueuedPrompt }
  | { kind: 'queue-remove'; entry: ManagedQueuedPrompt; state: SelectorState }
  | { kind: 'none' }

export function handleSelectorControl(state: SelectorState, event: KeyEvent): SelectorControlAction {
  switch (event.type) {
    case 'up':
      return { kind: 'update', state: selectorUp(state) }
    case 'down':
      return { kind: 'update', state: selectorDown(state) }
    case 'char':
      if (isQueueSelectorTitle(state.title)) return { kind: 'none' }
      return { kind: 'update', state: selectorType(state, event.char) }
    case 'backspace':
      return { kind: 'update', state: selectorBackspace(state) }
    case 'enter':
      return selectAction(state)
    case 'escape':
      return { kind: 'close' }
    case 'delete':
      return deleteAction(state)
    case 'ctrl':
      return event.key === 'd' ? deleteAction(state) : { kind: 'none' }
    default:
      return { kind: 'none' }
  }
}

function selectAction(state: SelectorState): SelectorControlAction {
  const selected = selectorSelect(state)
  if (!selected) return { kind: 'close' }

  if (isResumeSelectorTitle(state.title)) return { kind: 'resume', sessionId: selected.id ?? selected.label }

  if (isQueueSelectorTitle(state.title)) {
    const action = decideQueueSelectorAction(selected, 'enter')
    if (action.kind === 'edit') return { kind: 'queue-edit', entry: action.entry }
    return { kind: 'none' }
  }

  return { kind: 'select-model', spec: selected.id ?? selected.label }
}

function deleteAction(state: SelectorState): SelectorControlAction {
  const target = selectorSelect(state)
  if (!target?.id) return { kind: 'none' }

  if (isQueueSelectorTitle(state.title)) {
    const action = decideQueueSelectorAction(target, 'delete')
    if (action.kind !== 'remove') return { kind: 'none' }
    return {
      kind: 'queue-remove',
      entry: action.entry,
      state: selectorRemoveItem(state, state.focusIndex),
    }
  }

  if (!isResumeSelectorTitle(state.title)) return { kind: 'none' }
  return {
    kind: 'delete-session',
    sessionId: target.id,
    label: target.label,
    state: selectorRemoveItem(state, state.focusIndex),
  }
}

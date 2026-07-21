import { describe, expect, test } from 'bun:test'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import { RESUME_SELECTOR_TITLE } from '../src/term/app/resume.js'
import { createSelectorState, type SelectorItem } from '../src/term/selector.js'

const char = (value: string) => ({ type: 'char' as const, char: value })
const key = (type: 'up' | 'down' | 'backspace' | 'enter' | 'escape' | 'delete') => ({ type })

describe('repl selector control', () => {
  const items: SelectorItem[] = [
    { label: 'one', id: 'session-one', detail: 'first' },
    { label: 'two', id: 'session-two', detail: 'second' },
  ]

  test('updates focus on down/up', () => {
    let state = createSelectorState('Select model', items)
    const down = handleSelectorControl(state, key('down'))
    expect(down.kind).toBe('update')
    if (down.kind === 'update') {
      expect(down.state.focusIndex).toBe(1)
      state = down.state
    }

    const up = handleSelectorControl(state, key('up'))
    expect(up.kind).toBe('update')
    if (up.kind === 'update') expect(up.state.focusIndex).toBe(0)
  })

  test('updates query on char and backspace', () => {
    let action = handleSelectorControl(createSelectorState('Select model', items), char('w'))
    expect(action.kind).toBe('update')
    if (action.kind !== 'update') return
    expect(action.state.query).toBe('w')
    expect(action.state.items.map(i => i.label)).toEqual(['two'])

    action = handleSelectorControl(action.state, key('backspace'))
    expect(action.kind).toBe('update')
    if (action.kind === 'update') {
      expect(action.state.query).toBe('')
      expect(action.state.items.length).toBe(2)
    }
  })

  test('escape closes selector', () => {
    expect(handleSelectorControl(createSelectorState('T', items), key('escape')).kind).toBe('close')
  })

  test('resume enter returns selected session id', () => {
    const action = handleSelectorControl(createSelectorState(RESUME_SELECTOR_TITLE, items), key('enter'))
    expect(action).toEqual({ kind: 'resume', sessionId: 'session-one' })
  })

  test('model enter returns provider-qualified select-model action', () => {
    const state = createSelectorState('Select model', [{ label: 'claude', id: 'anthropic:claude', detail: 'anthropic' }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({ kind: 'select-model', spec: 'anthropic:claude' })
  })

  test('delete removes resume session item', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const action = handleSelectorControl(state, key('delete'))
    expect(action.kind).toBe('delete-session')
    if (action.kind === 'delete-session') {
      expect(action.sessionId).toBe('session-one')
      expect(action.label).toBe('one')
      expect(action.state.items.map(i => i.label)).toEqual(['two'])
    }
  })

  test('ctrl-d removes resume session item', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    const action = handleSelectorControl(state, { type: 'ctrl', key: 'd' })
    expect(action.kind).toBe('delete-session')
  })

  test('non resume delete is ignored', () => {
    const state = createSelectorState('Select model', items)
    expect(handleSelectorControl(state, key('delete')).kind).toBe('none')
  })

  test('queue selector supports selection, edit, and remove only', () => {
    const state = createSelectorState('Prompt queue', [{
      label: '1. later',
      id: 'follow_up|q1|3',
      searchText: 'later',
    }])
    expect(handleSelectorControl(state, key('enter'))).toEqual({
      kind: 'queue-edit',
      entry: { queue: 'follow_up', id: 'q1', version: 3, text: 'later' },
    })
    expect(handleSelectorControl(state, key('delete')).kind).toBe('queue-remove')
    expect(handleSelectorControl(state, { type: 'ctrl', key: 'd' }).kind).toBe('queue-remove')
    expect(handleSelectorControl(state, { type: 'shift-char', char: 'j' }).kind).toBe('none')
    expect(handleSelectorControl(state, { type: 'ctrl-enter' }).kind).toBe('none')
  })

  test('queue character keys do not define alternate edit or remove shortcuts', () => {
    const state = createSelectorState('Prompt queue', [{
      label: '1. queued',
      id: 'steering|q2|0',
      searchText: 'queued',
    }])
    expect(handleSelectorControl(state, char('e')).kind).toBe('none')
    expect(handleSelectorControl(state, char('x')).kind).toBe('none')
  })

  test('other ctrl key is ignored', () => {
    const state = createSelectorState(RESUME_SELECTOR_TITLE, items)
    expect(handleSelectorControl(state, { type: 'ctrl', key: 'c' }).kind).toBe('none')
  })
})

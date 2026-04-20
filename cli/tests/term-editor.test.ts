import { describe, expect, test } from 'bun:test'
import {
  applyCompletion,
  backspace,
  clearEditor,
  createEditorState,
  createHistoryState,
  getEditorText,
  historyNext,
  historyPrev,
  insertText,
  isEditorEmpty,
  moveEnd,
  moveHome,
  moveLeft,
  moveRight,
  pushHistory,
  refreshGhostHint,
} from '../src/term/input/editor.js'

describe('term input editor', () => {
  test('insertText inserts chars and newlines', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    expect(getEditorText(state)).toBe('hello')
    state = insertText(state, '\nworld')
    expect(state.lines).toEqual(['hello', 'world'])
    expect(state.cursorLine).toBe(1)
    expect(state.cursorCol).toBe(5)
  })

  test('backspace joins lines', () => {
    let state = createEditorState()
    state = insertText(state, 'a\nb')
    state = moveHome(state)
    state = backspace(state)
    expect(state.lines).toEqual(['ab'])
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(1)
  })

  test('move left/right across lines', () => {
    let state = createEditorState()
    state = insertText(state, 'a\nb')
    state = moveLeft(state)
    expect(state.cursorLine).toBe(1)
    expect(state.cursorCol).toBe(0)
    state = moveLeft(state)
    expect(state.cursorLine).toBe(0)
    expect(state.cursorCol).toBe(1)
    state = moveRight(state)
    expect(state.cursorLine).toBe(1)
    expect(state.cursorCol).toBe(0)
  })

  test('clearEditor resets to empty', () => {
    let state = createEditorState()
    state = insertText(state, 'abc')
    state = clearEditor(state)
    expect(isEditorEmpty(state)).toBe(true)
  })

  test('applyCompletion updates candidates', () => {
    let state = createEditorState()
    state = insertText(state, '/he')
    const result = applyCompletion(state)
    expect(result.applied).toBe(true)
    expect(getEditorText(result.state).startsWith('/help')).toBe(true)
  })

  test('refreshGhostHint does not crash', () => {
    let state = createEditorState()
    state = insertText(state, '/he')
    state = refreshGhostHint(state)
    expect(typeof state.ghostHint).toBe('string')
  })

  test('history prev/next restore input', () => {
    let editor = createEditorState()
    let history = createHistoryState([])
    history = pushHistory(history, 'one')
    history = pushHistory(history, 'two')
    editor = insertText(editor, 'draft')

    let prev = historyPrev(history, editor)
    expect(prev.changed).toBe(true)
    expect(getEditorText(prev.editor)).toBe('two')

    prev = historyPrev(prev.history, prev.editor)
    expect(getEditorText(prev.editor)).toBe('one')

    let next = historyNext(prev.history, prev.editor)
    expect(getEditorText(next.editor)).toBe('two')

    next = historyNext(next.history, next.editor)
    expect(getEditorText(next.editor)).toBe('draft')
  })

  test('move home/end update cursor', () => {
    let state = createEditorState()
    state = insertText(state, 'hello')
    state = moveHome(state)
    expect(state.cursorCol).toBe(0)
    state = moveEnd(state)
    expect(state.cursorCol).toBe(5)
  })
})

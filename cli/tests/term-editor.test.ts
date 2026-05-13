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

  test('history prev/next handles multi-line entries', () => {
    let editor = createEditorState()
    let history = createHistoryState([])
    history = pushHistory(history, 'single line')
    history = pushHistory(history, 'line one\nline two\nline three')
    editor = insertText(editor, 'current')

    // Navigate to multi-line entry
    let prev = historyPrev(history, editor)
    expect(prev.changed).toBe(true)
    expect(prev.editor.lines).toEqual(['line one', 'line two', 'line three'])
    expect(prev.editor.cursorLine).toBe(2)
    expect(prev.editor.cursorCol).toBe(10)

    // Navigate further to single-line entry
    prev = historyPrev(prev.history, prev.editor)
    expect(prev.editor.lines).toEqual(['single line'])
    expect(prev.editor.cursorLine).toBe(0)

    // Navigate forward back to multi-line
    let next = historyNext(prev.history, prev.editor)
    expect(next.editor.lines).toEqual(['line one', 'line two', 'line three'])

    // Navigate forward to saved input
    next = historyNext(next.history, next.editor)
    expect(getEditorText(next.editor)).toBe('current')
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

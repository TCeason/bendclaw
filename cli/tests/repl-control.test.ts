import { describe, expect, test } from 'bun:test'
import { decideReplControl } from '../src/term/app/repl-control.js'
import { createEditorState, insertText } from '../src/term/input/editor.js'
import { createSelectorState } from '../src/term/selector.js'

const editor = createEditorState()
const textEditor = insertText(createEditorState(), 'hello')
const none = { kind: 'none' as const }
const help = { kind: 'help' as const }
const selector = { kind: 'selector' as const, state: createSelectorState('T', [{ label: 'one' }]) }
const selectorWithQuery = { kind: 'selector' as const, state: createSelectorState('T', [{ label: 'one' }], undefined, 'o') }
const askUser = { kind: 'ask-user' as const, state: { questions: [], currentIndex: 0, answers: {} } as any }

const kinds = (input: Parameters<typeof decideReplControl>[0]) => decideReplControl(input).map(a => a.kind)
const base = { overlay: none, isLoading: false, hasStream: false, editor, exitHint: false, logMode: false }

describe('repl control', () => {
  test('ctrl-c interrupts loading stream', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, isLoading: true, hasStream: true })).toEqual(['interrupt'])
  })

  test('ctrl-c shows exit hint on empty editor', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' } })).toEqual(['show-exit-hint'])
  })

  test('ctrl-c exits when exit hint is already visible', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, exitHint: true })).toEqual(['exit'])
  })

  test('ctrl-c clears non-empty editor', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'c' }, editor: textEditor })).toEqual(['clear-editor'])
  })

  test('non ctrl-c clears exit hint then continues', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, exitHint: true })).toEqual(['clear-exit-hint', 'normal-key'])
  })

  test('escape cancels ask overlay with stream', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: askUser, hasStream: true })).toEqual(['cancel-ask'])
  })

  test('escape clears selector query before closing overlay', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: selectorWithQuery })).toEqual(['clear-selector-query'])
  })

  test('escape closes overlay without query', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, overlay: selector })).toEqual(['close-overlay'])
  })

  test('escape interrupts loading stream without overlay', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, isLoading: true, hasStream: true })).toEqual(['interrupt'])
  })

  test('escape clears editor before exiting log mode', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, editor: textEditor, logMode: true })).toEqual(['clear-editor'])
  })

  test('escape exits log mode when editor is empty', () => {
    expect(kinds({ ...base, event: { type: 'escape' }, logMode: true })).toEqual(['exit-log-mode'])
  })

  test('help overlay closes on any key', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, overlay: help })).toEqual(['close-overlay'])
  })

  test('selector overlay delegates key', () => {
    expect(kinds({ ...base, event: { type: 'down' }, overlay: selector })).toEqual(['selector-key'])
  })

  test('ask overlay delegates key', () => {
    expect(kinds({ ...base, event: { type: 'char', char: 'y' }, overlay: askUser })).toEqual(['ask-key'])
  })

  test('loading enter/char/paste have loading actions', () => {
    expect(kinds({ ...base, event: { type: 'enter' }, isLoading: true })).toEqual(['loading-enter'])
    expect(kinds({ ...base, event: { type: 'char', char: 'x' }, isLoading: true })).toEqual(['loading-char'])
    expect(kinds({ ...base, event: { type: 'paste', text: 'x' }, isLoading: true })).toEqual(['loading-paste'])
  })

  test('ctrl-o toggles expanded view while loading', () => {
    expect(kinds({ ...base, event: { type: 'ctrl', key: 'o' }, isLoading: true })).toEqual(['toggle-expanded'])
  })

  test('loading movement falls through to normal key', () => {
    expect(kinds({ ...base, event: { type: 'left' }, isLoading: true })).toEqual(['normal-key'])
  })
})

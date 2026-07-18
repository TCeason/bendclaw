import { complete, getGhostHint } from '../../commands/completion.js'
import { COMMANDS, HIDDEN_COMMANDS } from '../../commands/index.js'
import { needsContinuation } from './continuation.js'

export interface CompletionCandidate {
  label: string
  value: string
  description?: string
}

export interface CompletionMenu {
  items: CompletionCandidate[]
  selectedIndex: number
  replaceStart: number
  replaceEnd: number
}

export interface EditorState {
  lines: string[]
  cursorLine: number
  cursorCol: number
  ghostHint: string
  completion: CompletionMenu | null
}

/** Check if current editor content needs continuation (unclosed fence, trailing backslash). */
export function editorNeedsContinuation(state: EditorState): boolean {
  return needsContinuation(getEditorText(state))
}

export interface HistoryState {
  entries: string[]
  index: number
  savedInput: string
}

export interface CompletionApplyResult {
  applied: boolean
  state: EditorState
}

export function createEditorState(): EditorState {
  return {
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    ghostHint: '',
    completion: null,
  }
}

export function getEditorText(state: EditorState): string {
  return state.lines.join('\n')
}

export function isEditorEmpty(state: EditorState): boolean {
  return state.lines.length === 1 && state.lines[0] === ''
}

export function clearEditor(state: EditorState): EditorState {
  return {
    ...state,
    lines: [''],
    cursorLine: 0,
    cursorCol: 0,
    ghostHint: '',
    completion: null,
  }
}

export function insertText(state: EditorState, text: string): EditorState {
  const insertedLines = text.split('\n')
  const newLines = [...state.lines]
  const currentLine = newLines[state.cursorLine]!
  const before = currentLine.slice(0, state.cursorCol)
  const after = currentLine.slice(state.cursorCol)

  if (insertedLines.length === 1) {
    newLines[state.cursorLine] = before + insertedLines[0] + after
    return withCompletionsCleared({
      ...state,
      lines: newLines,
      cursorCol: state.cursorCol + insertedLines[0]!.length,
    })
  }

  const first = before + insertedLines[0]!
  const last = insertedLines[insertedLines.length - 1]! + after
  const middle = insertedLines.slice(1, -1)
  newLines.splice(state.cursorLine, 1, first, ...middle, last)
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine + insertedLines.length - 1,
    cursorCol: insertedLines[insertedLines.length - 1]!.length,
  })
}

export function backspace(state: EditorState): EditorState {
  if (state.cursorCol > 0) {
    const newLines = [...state.lines]
    const currentLine = newLines[state.cursorLine]!
    newLines[state.cursorLine] = currentLine.slice(0, state.cursorCol - 1) + currentLine.slice(state.cursorCol)
    return withCompletionsCleared({
      ...state,
      lines: newLines,
      cursorCol: state.cursorCol - 1,
    })
  }

  if (state.cursorLine === 0) return state

  const newLines = [...state.lines]
  const prevLine = newLines[state.cursorLine - 1]!
  const currentLine = newLines[state.cursorLine]!
  newLines.splice(state.cursorLine - 1, 2, prevLine + currentLine)
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine - 1,
    cursorCol: prevLine.length,
  })
}

export function moveLeft(state: EditorState): EditorState {
  if (state.cursorCol > 0) {
    return withoutGhost({ ...state, cursorCol: state.cursorCol - 1 })
  }
  if (state.cursorLine > 0) {
    return withoutGhost({
      ...state,
      cursorLine: state.cursorLine - 1,
      cursorCol: state.lines[state.cursorLine - 1]!.length,
    })
  }
  return state
}

export function moveRight(state: EditorState): EditorState {
  const lineLen = state.lines[state.cursorLine]!.length
  if (state.cursorCol < lineLen) {
    return withoutGhost({ ...state, cursorCol: state.cursorCol + 1 })
  }
  if (state.cursorLine < state.lines.length - 1) {
    return withoutGhost({ ...state, cursorLine: state.cursorLine + 1, cursorCol: 0 })
  }
  return state
}

export function moveHome(state: EditorState): EditorState {
  return withoutGhost({ ...state, cursorCol: 0 })
}

export function moveEnd(state: EditorState): EditorState {
  return withoutGhost({ ...state, cursorCol: state.lines[state.cursorLine]!.length })
}

export function applyCompletion(state: EditorState): CompletionApplyResult {
  if (state.completion) return { state: acceptCompletion(state), applied: true }

  const currentLine = state.lines[state.cursorLine]!
  const result = complete(currentLine, state.cursorCol)
  if (!result) return { state, applied: false }

  const before = currentLine.slice(0, result.wordStart)
  const after = currentLine.slice(state.cursorCol)
  const newLines = [...state.lines]
  newLines[state.cursorLine] = before + result.replacement + after
  const cursorCol = before.length + result.replacement.length
  const items = result.candidates.map(completionCandidate)

  return {
    state: {
      ...state,
      lines: newLines,
      cursorCol,
      ghostHint: '',
      completion: items.length > 1
        ? { items, selectedIndex: 0, replaceStart: result.wordStart, replaceEnd: cursorCol }
        : null,
    },
    applied: true,
  }
}

export function showCompletions(
  state: EditorState,
  items: CompletionCandidate[],
  replaceStart: number,
  replaceEnd = state.cursorCol,
): EditorState {
  return {
    ...state,
    ghostHint: '',
    completion: items.length > 0
      ? { items, selectedIndex: 0, replaceStart, replaceEnd }
      : null,
  }
}

export function moveCompletion(state: EditorState, delta: number): EditorState {
  const menu = state.completion
  if (!menu || menu.items.length === 0) return state
  const selectedIndex = (menu.selectedIndex + delta + menu.items.length) % menu.items.length
  return { ...state, completion: { ...menu, selectedIndex } }
}

export function acceptCompletion(state: EditorState): EditorState {
  const menu = state.completion
  const item = menu?.items[menu.selectedIndex]
  if (!menu || !item) return state
  const currentLine = state.lines[state.cursorLine]!
  const newLines = [...state.lines]
  newLines[state.cursorLine] = currentLine.slice(0, menu.replaceStart) + item.value + currentLine.slice(menu.replaceEnd)
  return {
    ...state,
    lines: newLines,
    cursorCol: menu.replaceStart + item.value.length,
    ghostHint: '',
    completion: null,
  }
}

export function closeCompletion(state: EditorState): EditorState {
  return state.completion ? { ...state, completion: null } : state
}

function completionCandidate(value: string): CompletionCandidate {
  const command = [...COMMANDS, ...HIDDEN_COMMANDS].find(
    item => item.name === value || item.aliases?.includes(value),
  )
  return {
    label: value,
    value: command ? `${value} ` : value,
    description: command?.description,
  }
}

export function refreshGhostHint(state: EditorState): EditorState {
  const currentLine = state.lines[state.cursorLine]!
  return {
    ...state,
    ghostHint: getGhostHint(currentLine, state.cursorCol) ?? '',
  }
}

export function createHistoryState(entries: string[]): HistoryState {
  return {
    entries,
    index: entries.length,
    savedInput: '',
  }
}

export function pushHistory(state: HistoryState, entry: string): HistoryState {
  return {
    entries: [...state.entries, entry],
    index: state.entries.length + 1,
    savedInput: '',
  }
}

export function historyPrev(history: HistoryState, editor: EditorState): { history: HistoryState; editor: EditorState; changed: boolean } {
  if (history.index <= 0) return { history, editor, changed: false }

  const savedInput = history.index === history.entries.length ? getEditorText(editor) : history.savedInput
  const index = history.index - 1
  return {
    history: { ...history, index, savedInput },
    editor: editorFromText(editor, history.entries[index]!),
    changed: true,
  }
}

export function historyNext(history: HistoryState, editor: EditorState): { history: HistoryState; editor: EditorState; changed: boolean } {
  if (history.index >= history.entries.length) return { history, editor, changed: false }

  const index = history.index + 1
  const text = index === history.entries.length ? history.savedInput : history.entries[index]!
  return {
    history: { ...history, index },
    editor: editorFromText(editor, text),
    changed: true,
  }
}


// ---------------------------------------------------------------------------
// Ctrl+U — clear line before cursor
// ---------------------------------------------------------------------------

export function clearLineBefore(state: EditorState): EditorState {
  const newLines = [...state.lines]
  newLines[state.cursorLine] = newLines[state.cursorLine]!.slice(state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines, cursorCol: 0 })
}

// ---------------------------------------------------------------------------
// Ctrl+K — clear line after cursor
// ---------------------------------------------------------------------------

export function clearLineAfter(state: EditorState): EditorState {
  const newLines = [...state.lines]
  newLines[state.cursorLine] = newLines[state.cursorLine]!.slice(0, state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines })
}

// ---------------------------------------------------------------------------
// Ctrl+D — delete char at cursor (or signal exit if empty)
// ---------------------------------------------------------------------------

export function deleteForward(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  if (state.cursorCol < line.length) {
    const newLines = [...state.lines]
    newLines[state.cursorLine] = line.slice(0, state.cursorCol) + line.slice(state.cursorCol + 1)
    return withCompletionsCleared({ ...state, lines: newLines })
  }
  // Join with next line
  if (state.cursorLine < state.lines.length - 1) {
    const newLines = [...state.lines]
    newLines[state.cursorLine] = newLines[state.cursorLine]! + newLines[state.cursorLine + 1]!
    newLines.splice(state.cursorLine + 1, 1)
    return withCompletionsCleared({ ...state, lines: newLines })
  }
  return state
}

// ---------------------------------------------------------------------------
// Ctrl+W — delete word before cursor
// ---------------------------------------------------------------------------

export function deleteWordBefore(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  let i = state.cursorCol
  // skip trailing whitespace backward
  while (i > 0 && line[i - 1] === ' ') i--
  // skip word backward
  while (i > 0 && line[i - 1] !== ' ') i--
  const newLines = [...state.lines]
  newLines[state.cursorLine] = line.slice(0, i) + line.slice(state.cursorCol)
  return withCompletionsCleared({ ...state, lines: newLines, cursorCol: i })
}

// ---------------------------------------------------------------------------
// Insert newline (Alt+Enter / continuation)
// ---------------------------------------------------------------------------

export function insertNewline(state: EditorState): EditorState {
  const line = state.lines[state.cursorLine]!
  const newLines = [...state.lines]
  newLines.splice(state.cursorLine, 1, line.slice(0, state.cursorCol), line.slice(state.cursorCol))
  return withCompletionsCleared({
    ...state,
    lines: newLines,
    cursorLine: state.cursorLine + 1,
    cursorCol: 0,
  })
}

// ---------------------------------------------------------------------------
// Multi-line cursor movement (up/down within multi-line editor)
// ---------------------------------------------------------------------------

export function moveUp(state: EditorState): EditorState {
  if (state.cursorLine <= 0) return state
  const newLine = state.cursorLine - 1
  const newCol = Math.min(state.cursorCol, state.lines[newLine]!.length)
  return withoutGhost({ ...state, cursorLine: newLine, cursorCol: newCol })
}

export function moveDown(state: EditorState): EditorState {
  if (state.cursorLine >= state.lines.length - 1) return state
  const newLine = state.cursorLine + 1
  const newCol = Math.min(state.cursorCol, state.lines[newLine]!.length)
  return withoutGhost({ ...state, cursorLine: newLine, cursorCol: newCol })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function editorFromText(state: EditorState, text: string): EditorState {
  const lines = text.split('\n')
  return {
    ...state,
    lines,
    cursorLine: lines.length - 1,
    cursorCol: lines[lines.length - 1]!.length,
    ghostHint: '',
    completion: null,
  }
}

function withCompletionsCleared(state: EditorState): EditorState {
  return refreshGhostHint({ ...state, completion: null })
}

function withoutGhost(state: EditorState): EditorState {
  return { ...state, ghostHint: '', completion: null }
}

import { line, block, plain, dim, bold, colored, inverse, type ViewBlock, type StyledSpan, type StyledLine } from './types.js'
import type { SelectorState } from '../selector.js'
import type { AskState } from '../ask.js'

export type OverlayState =
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'selector'; state: SelectorState }
  | { kind: 'ask-user'; state: AskState }

export function buildOverlayBlocks(overlay: OverlayState, columns: number): ViewBlock[] {
  switch (overlay.kind) {
    case 'none':
      return []
    case 'help':
      return buildHelpBlocks(columns)
    case 'selector':
      return buildSelectorBlocks(overlay.state)
    case 'ask-user':
      return buildAskBlocks(overlay.state, columns)
  }
}

function buildHelpBlocks(columns: number): ViewBlock[] {
  const entries = [
    ['Enter', 'Submit message'],
    ['Alt+Enter', 'Insert newline'],
    ['Ctrl+C', 'Clear / Exit (×2)'],
    ['Esc', 'Clear input / Dismiss / Interrupt'],
    ['↑ / ↓', 'History navigation / multi-line'],
    ['Tab', 'Complete command / path'],
    ['Ctrl+U', 'Clear line before cursor'],
    ['Ctrl+K', 'Clear line after cursor'],
    ['Ctrl+W', 'Delete word before cursor'],
    ['Ctrl+D', 'Delete char / Exit if empty'],
    ['Ctrl+A/E', 'Move to start/end of line'],
    ['Ctrl+L', 'Clear all input'],
    ['Ctrl+O', 'Expand/collapse output'],
    ['Shift+Tab', 'Cycle thinking level'],
    ['/help', 'Show this help'],
    ['/model <name>', 'Switch model'],
    ['/resume [id|query]', 'Resume session'],
    ['/new', 'Start new session'],
    ['/goto <n>', 'Go to message'],
    ['/history [n]', 'Show recent messages'],
    ['/goal [...]', 'Manage long-task goal'],
    ['/plan', 'Toggle planning mode'],
    ['/env', 'Manage variables'],
    ['/skill', 'Manage skills'],
    ['/copy', 'Copy last agent message (Markdown)'],
    ['/update', 'Update evot'],
    ['/clear', 'Clear session context'],
    ['/exit', 'Exit'],
  ]

  const maxKeyLen = Math.max(...entries.map(e => e[0]!.length))
  const lines = [
    line(bold('  Keyboard Shortcuts & Commands')),
    line(plain('')),
    ...entries.map(([key, desc]) =>
      line(colored(`  ${key!.padEnd(maxKeyLen + 2)}`, 'cyan'), dim(desc!))
    ),
    line(plain('')),
    line(dim('  Press Esc to dismiss')),
  ]

  return [block(lines, 1)]
}

function highlightSpans(text: string, query: string, base: Partial<StyledSpan>): StyledSpan[] {
  if (!query) return [{ text, ...base }]
  const lower = text.toLowerCase()
  const lowerQuery = query.toLowerCase()
  const idx = lower.indexOf(lowerQuery)
  if (idx === -1) return [{ text, ...base }]
  const spans: StyledSpan[] = []
  if (idx > 0) spans.push({ text: text.slice(0, idx), ...base })
  spans.push({ text: text.slice(idx, idx + lowerQuery.length), fg: 'yellow', bold: true })
  if (idx + lowerQuery.length < text.length) spans.push({ text: text.slice(idx + lowerQuery.length), ...base })
  return spans
}

function buildSelectorBlocks(state: SelectorState): ViewBlock[] {
  const lines = [
    line(bold(state.title)),
  ]

  if (state.query) {
    lines.push(line(dim('  search: '), plain(state.query)))
  }

  lines.push(line(plain('')))

  if (state.items.length === 0) {
    lines.push(line(dim('  No matches')))
  } else {
    const maxVisible = 10
    // Keep focused item visible within the window
    let start = 0
    if (state.items.length > maxVisible) {
      start = Math.min(
        Math.max(0, state.focusIndex - Math.floor(maxVisible / 2)),
        state.items.length - maxVisible
      )
    }
    const end = Math.min(start + maxVisible, state.items.length)

    if (start > 0) {
      lines.push(line(dim(`  ↑ ${start} more`)))
    }
    for (let i = start; i < end; i++) {
      const item = state.items[i]!
      const focused = i === state.focusIndex
      const prefix: StyledSpan = focused ? colored('❯ ', 'cyan') : plain('  ')
      const labelSpans = state.query
        ? highlightSpans(item.label, state.query, focused ? { bold: true } : {})
        : [focused ? bold(item.label) : plain(item.label)]
      const detailSpans = item.detail && state.query
        ? highlightSpans(` ${item.detail}`, state.query, { dim: true })
        : [item.detail ? dim(` ${item.detail}`) : plain('')]
      lines.push(line(prefix, ...labelSpans, ...detailSpans))
    }
    if (end < state.items.length) {
      lines.push(line(dim(`  ↓ ${state.items.length - end} more`)))
    }
  }

  lines.push(line(plain('')))
  lines.push(line(dim('↑↓ navigate · type to filter · enter select · esc cancel')))
  return [block(lines, 1)]
}

const CHECKBOX_ON = '☒'
const CHECKBOX_OFF = '☐'
const TICK = '✓'
const POINTER = '❯'
const BULLET = '•'
const ARROW_RIGHT = '→'

function optionCountStringWidth(count: number): number {
  return count.toString().length
}

function optionIndexText(index: number, maxIndexWidth: number): string {
  return `${index}.`.padEnd(maxIndexWidth + 2)
}

function appendTick(spans: StyledSpan[]): StyledSpan[] {
  return [...spans, colored(TICK, 'green')]
}

function selectedAnswerKind(state: AskState, questionIndex: number): 'option' | 'other' | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  if (answer.customText !== null) return 'other'
  if (answer.selectedOption !== null) return 'option'
  return null
}

function selectedOptionIndex(state: AskState, questionIndex: number): number | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  return answer.selectedOption
}

function selectedAnswerText(state: AskState, questionIndex: number): string | null {
  const answer = state.answers[questionIndex]
  if (!answer) return null
  if (answer.customText !== null) return answer.customText
  if (answer.selectedOption !== null) return state.questions[questionIndex]?.options[answer.selectedOption]?.label ?? null
  return null
}

function isAnswered(state: AskState, index: number): boolean {
  const a = state.answers[index]
  return a !== undefined && (a.selectedOption !== null || a.customText !== null)
}

export function buildAskBlocks(state: AskState, columns: number): ViewBlock[] {
  const result: StyledLine[] = []
  const isMulti = state.questions.length > 1

  // ── Tab bar (multi-question only) ──────────────────────────────
  if (isMulti) {
    const tabLine: StyledSpan[] = []

    // Left arrow
    const canGoLeft = state.currentTab > 0 || state.onSubmitTab
    tabLine.push(canGoLeft ? plain('← ') : dim('← '))

    // Tabs with checkboxes
    for (let i = 0; i < state.questions.length; i++) {
      if (i > 0) tabLine.push(plain('  '))
      const qq = state.questions[i]!
      const active = !state.onSubmitTab && i === state.currentTab
      const answered = isAnswered(state, i)
      const checkbox = answered ? CHECKBOX_ON : CHECKBOX_OFF
      if (active) {
        tabLine.push(inverse(` ${checkbox} ${qq.header} `))
      } else {
        tabLine.push(plain(` ${checkbox} ${qq.header} `))
      }
    }

    // Submit tab
    tabLine.push(plain('  '))
    if (state.onSubmitTab) {
      tabLine.push(inverse(` ${TICK} Submit `))
    } else {
      tabLine.push(plain(` ${TICK} Submit `))
    }

    // Right arrow
    const canGoRight = !state.onSubmitTab
    tabLine.push(canGoRight ? plain(' →') : dim(' →'))

    result.push(line(...tabLine))
    result.push(line(plain('')))
  }

  // ── Submit review page ─────────────────────────────────────────
  if (state.onSubmitTab) {
    const allAnswered = state.questions.every((_, i) => isAnswered(state, i))

    result.push(line(bold('Review your answers')))
    result.push(line(plain('')))

    if (!allAnswered) {
      result.push(line(colored('⚠ You have not answered all questions', 'yellow')))
      result.push(line(plain('')))
    }

    for (let i = 0; i < state.questions.length; i++) {
      const qq = state.questions[i]!
      const answerText = selectedAnswerText(state, i)
      if (!answerText) continue
      result.push(line(plain(`  ${BULLET} ${qq.question}`)))
      result.push(line(colored(`    ${ARROW_RIGHT} ${answerText}`, 'green')))
    }

    result.push(line(plain('')))
    result.push(line(dim('Ready to submit your answers?')))
    result.push(line(plain('')))

    // Submit / Cancel options
    const submitFocused = state.submitFocus === 0
    const cancelFocused = state.submitFocus === 1
    result.push(line(
      submitFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      submitFocused ? bold('Submit answers') : plain('Submit answers')
    ))
    result.push(line(
      cancelFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      cancelFocused ? bold('Cancel') : plain('Cancel')
    ))

    result.push(line(plain('')))
    result.push(line(dim('↑↓ navigate · enter select · ← back · esc cancel')))

    return [block(result, 1)]
  }

  // ── Question view ──────────────────────────────────────────────
  const q = state.questions[state.currentTab]!

  // ── Question text ──────────────────────────────────────────────
  result.push(line(bold(q.question)))
  result.push(line(plain('')))

  const ui = state.uiStates.get(state.currentTab) ?? { focusIndex: 0, inOtherMode: false, otherText: '', otherCursor: 0 }
  const selectedKind = selectedAnswerKind(state, state.currentTab)
  const selectedIndex = selectedOptionIndex(state, state.currentTab)
  const selectedText = selectedAnswerText(state, state.currentTab)
  const maxIndexWidth = optionCountStringWidth(q.options.length + 1)

  // ── Options ────────────────────────────────────────────────────
  for (let i = 0; i < q.options.length; i++) {
    const opt = q.options[i]!
    const focused = !ui.inOtherMode && i === state.focusIndex
    const selected = selectedKind === 'option' && selectedIndex === i
    const spans: StyledSpan[] = [
      focused ? colored(`${POINTER} `, 'cyan') : plain('  '),
      dim(optionIndexText(i + 1, maxIndexWidth)),
      selected
        ? colored(opt.label, 'green')
        : focused
          ? colored(opt.label, 'cyan')
          : plain(opt.label),
    ]
    if (opt.description) spans.push(dim(` — ${opt.description}`))
    result.push(line(...(selected ? appendTick(spans) : spans)))
  }

  // ── Other ──────────────────────────────────────────────────────
  const otherSelected = selectedKind === 'other'
  const otherFocused = ui.inOtherMode
  const otherText = otherFocused ? ui.otherText : otherSelected ? selectedText ?? '' : ui.otherText
  const otherSpans: StyledSpan[] = [
    otherFocused ? colored(`${POINTER} `, 'cyan') : plain('  '),
    dim(optionIndexText(q.options.length + 1, maxIndexWidth)),
  ]
  if (otherFocused) {
    if (otherText) {
      const cursor = ui.otherCursor ?? otherText.length
      const before = otherText.slice(0, cursor)
      const atCursor = otherText[cursor] ?? ' '
      const after = otherText.slice(cursor + 1)
      if (before) otherSpans.push(plain(before))
      otherSpans.push(inverse(atCursor))
      if (after) otherSpans.push(plain(after))
    } else {
      otherSpans.push(inverse(' '), dim('Type something.'))
    }
  } else {
    otherSpans.push(otherSelected ? colored(otherText || 'Type something.', 'green') : dim(otherText || 'Type something.'))
  }
  result.push(line(...(otherSelected ? appendTick(otherSpans) : otherSpans)))

  result.push(line(plain('')))

  // ── Footer hint ────────────────────────────────────────────────
  if (isMulti) {
    result.push(line(dim('↑↓ navigate · ←→ switch tab · enter select · esc cancel')))
  } else {
    result.push(line(dim('↑↓ navigate · enter select · esc cancel')))
  }

  return [block(result, 1)]
}

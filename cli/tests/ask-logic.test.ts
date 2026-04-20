import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createAskState,
  askUp,
  askDown,
  askNextTab,
  askPrevTab,
  askTypeChar,
  askBackspace,
  askClearOther,
  askSelect,
} from '../src/term/ask.js'
import { buildOverlayBlocks } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

beforeAll(() => { chalk.level = 3 })

const singleQuestion = [
  {
    header: 'Language',
    question: 'Which language?',
    options: [
      { label: 'Rust', description: 'Systems programming' },
      { label: 'TypeScript', description: 'Web development' },
    ],
  },
]

const multiQuestion = [
  {
    header: 'Language',
    question: 'Which language?',
    options: [
      { label: 'Rust', description: 'Systems programming' },
      { label: 'TypeScript', description: 'Web development' },
    ],
  },
  {
    header: 'Style',
    question: 'Which style?',
    options: [
      { label: 'Functional' },
      { label: 'Imperative' },
      { label: 'Mixed' },
    ],
  },
]

function renderAskVM(state: ReturnType<typeof createAskState>): string {
  const lines = blocksToLines(buildOverlayBlocks({ kind: 'ask-user', state }, 80))
  return lines.map(l => stripAnsi(l)).join('\n')
}

describe('createAskState', () => {
  test('creates initial state', () => {
    const state = createAskState(singleQuestion)
    expect(state.currentTab).toBe(0)
    expect(state.focusIndex).toBe(0)
    expect(state.inOtherMode).toBe(false)
    expect(state.otherText).toBe('')
    expect(state.submitted).toBe(false)
    expect(state.answers).toHaveLength(1)
  })

  test('multi-question creates answers for each', () => {
    const state = createAskState(multiQuestion)
    expect(state.answers).toHaveLength(2)
  })
})

describe('askUp / askDown', () => {
  test('down moves focus', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    expect(state.focusIndex).toBe(1)
  })

  test('down past last option enters other mode', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    expect(state.inOtherMode).toBe(true)
    expect(state.focusIndex).toBe(2)
  })

  test('up from other mode goes to last option', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askUp(state)
    expect(state.inOtherMode).toBe(false)
    expect(state.focusIndex).toBe(1)
  })

  test('up at top does nothing', () => {
    const state = createAskState(singleQuestion)
    const next = askUp(state)
    expect(next.focusIndex).toBe(0)
    expect(next).toBe(state)
  })
})

describe('askNextTab / askPrevTab', () => {
  test('next tab advances', () => {
    let state = createAskState(multiQuestion)
    state = askNextTab(state)
    expect(state.currentTab).toBe(1)
    expect(state.focusIndex).toBe(0)
  })

  test('next tab at last does nothing', () => {
    let state = createAskState(multiQuestion)
    state = askNextTab(state)
    const next = askNextTab(state)
    expect(next.currentTab).toBe(1)
    expect(next).toBe(state)
  })

  test('prev tab goes back', () => {
    let state = createAskState(multiQuestion)
    state = askNextTab(state)
    state = askPrevTab(state)
    expect(state.currentTab).toBe(0)
  })

  test('prev tab at first does nothing', () => {
    const state = createAskState(multiQuestion)
    const next = askPrevTab(state)
    expect(next.currentTab).toBe(0)
    expect(next).toBe(state)
  })
})

describe('askTypeChar / askBackspace / askClearOther', () => {
  test('typing in other mode appends text', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askTypeChar(state, 'h')
    state = askTypeChar(state, 'i')
    expect(state.otherText).toBe('hi')
  })

  test('typing outside other mode does nothing', () => {
    let state = createAskState(singleQuestion)
    state = askTypeChar(state, 'x')
    expect(state.otherText).toBe('')
  })

  test('backspace removes last char', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askTypeChar(state, 'a')
    state = askTypeChar(state, 'b')
    state = askBackspace(state)
    expect(state.otherText).toBe('a')
  })

  test('clearOther empties text', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askTypeChar(state, 'x')
    state = askTypeChar(state, 'y')
    state = askClearOther(state)
    expect(state.otherText).toBe('')
  })
})

describe('askSelect', () => {
  test('single question: selecting option auto-submits', () => {
    const state = createAskState(singleQuestion)
    const { state: next, done } = askSelect(state)
    expect(done).toBe(true)
    expect(next.submitted).toBe(true)
    expect(next.answers[0]!.selectedOption).toBe(0)
    expect(next.answers[0]!.customText).toBeNull()
  })

  test('single question: down + select picks second option', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    const { state: next, done } = askSelect(state)
    expect(done).toBe(true)
    expect(next.answers[0]!.selectedOption).toBe(1)
  })

  test('single question: other text submits custom', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askTypeChar(state, 't')
    state = askTypeChar(state, 'e')
    state = askTypeChar(state, 's')
    state = askTypeChar(state, 't')
    const { state: next, done } = askSelect(state)
    expect(done).toBe(true)
    expect(next.answers[0]!.selectedOption).toBeNull()
    expect(next.answers[0]!.customText).toBe('test')
  })

  test('single question: empty other text does not submit', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    const { state: next, done } = askSelect(state)
    expect(done).toBe(false)
    expect(next.submitted).toBe(false)
  })

  test('multi question: first select advances to next tab', () => {
    const state = createAskState(multiQuestion)
    const { state: next, done } = askSelect(state)
    expect(done).toBe(false)
    expect(next.currentTab).toBe(1)
    expect(next.answers[0]!.selectedOption).toBe(0)
  })

  test('multi question: answering all submits', () => {
    let state = createAskState(multiQuestion)
    let result = askSelect(state)
    state = result.state
    result = askSelect(state)
    expect(result.done).toBe(true)
    expect(result.state.submitted).toBe(true)
    expect(result.state.answers[0]!.selectedOption).toBe(0)
    expect(result.state.answers[1]!.selectedOption).toBe(0)
  })
})

describe('renderAsk via viewmodel', () => {
  test('single question shows question text', () => {
    const state = createAskState(singleQuestion)
    expect(renderAskVM(state)).toContain('Which language?')
  })

  test('shows all options', () => {
    const state = createAskState(singleQuestion)
    const text = renderAskVM(state)
    expect(text).toContain('Rust')
    expect(text).toContain('TypeScript')
  })

  test('shows descriptions', () => {
    const state = createAskState(singleQuestion)
    const text = renderAskVM(state)
    expect(text).toContain('Systems programming')
    expect(text).toContain('Web development')
  })

  test('shows focus indicator', () => {
    const state = createAskState(singleQuestion)
    expect(renderAskVM(state)).toContain('❯')
  })

  test('multi question shows tab bar', () => {
    const state = createAskState(multiQuestion)
    const text = renderAskVM(state)
    expect(text).toContain('Language')
    expect(text).toContain('Style')
  })

  test('other mode shows typed text', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    state = askTypeChar(state, 'h')
    state = askTypeChar(state, 'i')
    expect(renderAskVM(state)).toContain('hi')
  })

  test('other mode shows placeholder when empty', () => {
    let state = createAskState(singleQuestion)
    state = askDown(state)
    state = askDown(state)
    expect(renderAskVM(state)).toContain('Type something.')
  })
})

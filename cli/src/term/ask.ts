export interface AskOption {
  label: string
  description?: string
}

export interface AskQuestion {
  header: string
  question: string
  options: AskOption[]
}

export interface AskAnswer {
  questionIndex: number
  selectedOption: number | null
  customText: string | null
}

/** Per-question UI state (preserved when switching tabs) */
export interface QuestionUIState {
  focusIndex: number
  inOtherMode: boolean
  otherText: string
}

export interface AskState {
  questions: AskQuestion[]
  currentTab: number
  focusIndex: number
  answers: AskAnswer[]
  /** Per-tab UI state keyed by question index */
  uiStates: Map<number, QuestionUIState>
  /** True when viewing the submit review page (multi-question only) */
  onSubmitTab: boolean
  /** Focus index within submit/cancel options (0=submit, 1=cancel) */
  submitFocus: number
  submitted: boolean
}

function getUIState(state: AskState, tab: number): QuestionUIState {
  return (
    state.uiStates.get(tab) ?? {
      focusIndex: 0,
      inOtherMode: false,
      otherText: '',
    }
  )
}

function setUIState(
  state: AskState,
  tab: number,
  patch: Partial<QuestionUIState>,
): AskState {
  const existing = getUIState(state, tab)
  const next: QuestionUIState = {
    focusIndex: patch.focusIndex ?? existing.focusIndex,
    inOtherMode: patch.inOtherMode ?? existing.inOtherMode,
    otherText: patch.otherText ?? existing.otherText,
  }
  const uiStates = new Map(state.uiStates)
  uiStates.set(tab, next)
  return {
    ...state,
    uiStates,
    focusIndex: next.focusIndex,
  }
}

export function createAskState(questions: AskQuestion[]): AskState {
  return {
    questions,
    currentTab: 0,
    focusIndex: 0,
    answers: questions.map((_, i) => ({
      questionIndex: i,
      selectedOption: null,
      customText: null,
    })),
    uiStates: new Map(),
    onSubmitTab: false,
    submitFocus: 0,
    submitted: false,
  }
}

/** Total options including "Other" */
function optionCount(state: AskState): number {
  return state.questions[state.currentTab]!.options.length + 1
}

/** Current tab's inOtherMode */
function inOther(state: AskState): boolean {
  return getUIState(state, state.currentTab).inOtherMode
}

/** Current tab's otherText */
function otherText(state: AskState): string {
  return getUIState(state, state.currentTab).otherText
}

export function askUp(state: AskState): AskState {
  const tab = state.currentTab
  const ui = getUIState(state, tab)
  if (ui.inOtherMode) {
    return setUIState(state, tab, {
      inOtherMode: false,
      focusIndex: optionCount(state) - 2,
    })
  }
  if (ui.focusIndex <= 0) return state
  return setUIState(state, tab, { focusIndex: ui.focusIndex - 1 })
}

export function askDown(state: AskState): AskState {
  const tab = state.currentTab
  const ui = getUIState(state, tab)
  const max = optionCount(state) - 1
  if (ui.focusIndex >= max) {
    return setUIState(state, tab, { inOtherMode: true, focusIndex: max })
  }
  const next = ui.focusIndex + 1
  return setUIState(state, tab, {
    focusIndex: next,
    inOtherMode: next === max,
  })
}

export function askNextTab(state: AskState): AskState {
  // If on submit tab, can't go further
  if (state.onSubmitTab) return state

  if (state.currentTab >= state.questions.length - 1) {
    // At last question: check if all answered to enter submit tab
    const allAnswered = state.answers.every(a => a.selectedOption !== null || a.customText !== null)
    if (allAnswered && state.questions.length > 1) {
      return { ...state, onSubmitTab: true, submitFocus: 0 }
    }
    return state
  }

  const next = state.currentTab + 1
  const ui = getUIState(state, next)
  return {
    ...state,
    currentTab: next,
    focusIndex: ui.focusIndex,
  }
}

export function askPrevTab(state: AskState): AskState {
  if (state.onSubmitTab) {
    // From submit tab, go back to last question
    const last = state.questions.length - 1
    const ui = getUIState(state, last)
    return {
      ...state,
      currentTab: last,
      focusIndex: ui.focusIndex,
      onSubmitTab: false,
    }
  }

  if (state.currentTab <= 0) return state
  const prev = state.currentTab - 1
  const ui = getUIState(state, prev)
  return {
    ...state,
    currentTab: prev,
    focusIndex: ui.focusIndex,
  }
}

/** Switch to a specific tab — used by left/right arrows */
export function askSwitchTab(state: AskState, targetTab: number): AskState {
  if (targetTab < 0 || targetTab >= state.questions.length) return state
  const ui = getUIState(state, targetTab)
  return {
    ...state,
    currentTab: targetTab,
    focusIndex: ui.focusIndex,
    onSubmitTab: false,
  }
}

export function askTypeChar(state: AskState, char: string): AskState {
  if (!inOther(state)) return state
  const tab = state.currentTab
  const ui = getUIState(state, tab)
  return setUIState(state, tab, { otherText: ui.otherText + char })
}

export function askBackspace(state: AskState): AskState {
  if (!inOther(state)) return state
  const tab = state.currentTab
  const ui = getUIState(state, tab)
  return setUIState(state, tab, { otherText: ui.otherText.slice(0, -1) })
}

export function askClearOther(state: AskState): AskState {
  return setUIState(state, state.currentTab, { otherText: '' })
}

export type AskKeyResult =
  | { action: 'update'; state: AskState }
  | { action: 'cancel' }
  | { action: 'submit'; state: AskState }

export function handleAskKeyEvent(
  state: AskState,
  eventType: string,
  char?: string,
): AskKeyResult {
  // On submit tab, special handling
  if (state.onSubmitTab) {
    switch (eventType) {
      case 'escape':
        return { action: 'cancel' }
      case 'up':
      case 'down':
        return {
          action: 'update',
          state: { ...state, submitFocus: state.submitFocus === 0 ? 1 : 0 },
        }
      case 'left':
        return { action: 'update', state: askPrevTab(state) }
      case 'enter': {
        if (state.submitFocus === 0) {
          // Submit
          return { action: 'submit', state: { ...state, submitted: true } }
        }
        // Cancel
        return { action: 'cancel' }
      }
      default:
        return { action: 'update', state }
    }
  }

  switch (eventType) {
    case 'escape':
      return { action: 'cancel' }
    case 'up':
      return { action: 'update', state: askUp(state) }
    case 'down':
      return { action: 'update', state: askDown(state) }
    case 'tab':
      return { action: 'update', state: askNextTab(state) }
    case 'right':
      return { action: 'update', state: askNextTab(state) }
    case 'left':
      return { action: 'update', state: askPrevTab(state) }
    case 'char':
      if (inOther(state) && char)
        return { action: 'update', state: askTypeChar(state, char) }
      return { action: 'update', state }
    case 'backspace':
      if (inOther(state))
        return { action: 'update', state: askBackspace(state) }
      return { action: 'update', state }
    case 'enter': {
      const result = askSelect(state)
      if (result.done) return { action: 'submit', state: result.state }
      return { action: 'update', state: result.state }
    }
    default:
      return { action: 'update', state }
  }
}

export function askSelect(state: AskState): { state: AskState; done: boolean } {
  const tab = state.currentTab
  const ui = getUIState(state, tab)
  const answers = [...state.answers]

  if (ui.inOtherMode && ui.otherText.trim()) {
    answers[tab] = {
      questionIndex: tab,
      selectedOption: null,
      customText: ui.otherText.trim(),
    }
  } else if (!ui.inOtherMode) {
    answers[tab] = {
      questionIndex: tab,
      selectedOption: ui.focusIndex,
      customText: null,
    }
  } else {
    return { state, done: false } // empty other text, do nothing
  }

  // After selecting, preserve the UI state so returning to this tab
  // shows the previously selected option (like Claude Code does)
  const newState = { ...state, answers }

  // Single question: auto-submit
  if (state.questions.length === 1) {
    return { state: { ...newState, submitted: true }, done: true }
  }

  // Multi question: advance to next tab or enter submit review if last
  if (tab < state.questions.length - 1) {
    const next = tab + 1
    const nextUI = getUIState(newState, next)
    return {
      state: {
        ...newState,
        currentTab: next,
        focusIndex: nextUI.focusIndex,
      },
      done: false,
    }
  }

  // Last question answered: enter submit review page
  return {
    state: {
      ...newState,
      onSubmitTab: true,
      submitFocus: 0,
    },
    done: false,
  }
}

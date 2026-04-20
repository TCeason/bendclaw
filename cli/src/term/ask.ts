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

export interface AskState {
  questions: AskQuestion[]
  currentTab: number
  focusIndex: number  // within current question's options (last = "Other")
  answers: AskAnswer[]
  otherText: string   // text typed in "Other" mode
  inOtherMode: boolean
  submitted: boolean
}

export function createAskState(questions: AskQuestion[]): AskState {
  return {
    questions,
    currentTab: 0,
    focusIndex: 0,
    answers: questions.map((_, i) => ({ questionIndex: i, selectedOption: null, customText: null })),
    otherText: '',
    inOtherMode: false,
    submitted: false,
  }
}

/** Total options including "Other" */
function optionCount(state: AskState): number {
  return state.questions[state.currentTab]!.options.length + 1
}

export function askUp(state: AskState): AskState {
  if (state.inOtherMode) {
    // Exit other mode, go to last real option
    return { ...state, inOtherMode: false, focusIndex: optionCount(state) - 2 }
  }
  if (state.focusIndex <= 0) return state
  return { ...state, focusIndex: state.focusIndex - 1 }
}

export function askDown(state: AskState): AskState {
  const max = optionCount(state) - 1
  if (state.focusIndex >= max) {
    // Enter other mode
    return { ...state, inOtherMode: true, focusIndex: max }
  }
  return { ...state, focusIndex: state.focusIndex + 1, inOtherMode: state.focusIndex + 1 === max }
}

export function askNextTab(state: AskState): AskState {
  if (state.currentTab >= state.questions.length - 1) return state
  return { ...state, currentTab: state.currentTab + 1, focusIndex: 0, inOtherMode: false }
}

export function askPrevTab(state: AskState): AskState {
  if (state.currentTab <= 0) return state
  return { ...state, currentTab: state.currentTab - 1, focusIndex: 0, inOtherMode: false }
}

export function askTypeChar(state: AskState, char: string): AskState {
  if (!state.inOtherMode) return state
  return { ...state, otherText: state.otherText + char }
}

export function askBackspace(state: AskState): AskState {
  if (!state.inOtherMode) return state
  return { ...state, otherText: state.otherText.slice(0, -1) }
}

export function askClearOther(state: AskState): AskState {
  return { ...state, otherText: '' }
}

export function askSelect(state: AskState): { state: AskState; done: boolean } {
  const answers = [...state.answers]
  if (state.inOtherMode && state.otherText.trim()) {
    answers[state.currentTab] = {
      questionIndex: state.currentTab,
      selectedOption: null,
      customText: state.otherText.trim(),
    }
  } else if (!state.inOtherMode) {
    answers[state.currentTab] = {
      questionIndex: state.currentTab,
      selectedOption: state.focusIndex,
      customText: null,
    }
  } else {
    return { state, done: false } // empty other text, do nothing
  }

  const newState = { ...state, answers }

  // Single question: auto-submit
  if (state.questions.length === 1) {
    return { state: { ...newState, submitted: true }, done: true }
  }

  // Multi question: advance to next tab or submit if last
  if (state.currentTab < state.questions.length - 1) {
    return { state: { ...newState, currentTab: state.currentTab + 1, focusIndex: 0, inOtherMode: false }, done: false }
  }

  // Last question answered
  return { state: { ...newState, submitted: true }, done: true }
}

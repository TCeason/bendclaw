import type { AskState } from '../ask.js'
import type { AskUserAnswer } from './types.js'

export function askStateToResponse(state: AskState): AskUserAnswer[] {
  return state.questions.map((question, index) => {
    const answer = state.answers[index]
    let text = 'Skipped'
    if (answer) {
      if (answer.customText !== null) {
        text = answer.customText
      } else if (answer.selectedOption !== null) {
        text = question.options[answer.selectedOption]?.label ?? 'Skipped'
      }
    }
    return {
      header: question.header,
      question: question.question,
      answer: text,
    }
  })
}

/**
 * AskUser — structured multiple-choice question UI.
 *
 * Inspired by Claude Code's AskUserQuestionPermissionRequest:
 * - Tab bar with ← ☑/☐ Header → navigation (multi-question)
 * - Compact-vertical option layout with descriptions
 * - "Other" inline text input (auto-activates on focus)
 * - Submit review page (multi-question)
 * - Single question auto-submits on selection
 * - Per-question state persists across tab switches
 */

import React, { useState, useRef } from 'react'
import { Text, Box, useInput } from 'ink'

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface AskUserQuestion {
  header: string
  question: string
  options: { label: string; description?: string }[]
}

export interface AskUserRequest {
  questions: AskUserQuestion[]
}

export interface AskUserAnswer {
  questionIndex: number
  selectedOption: number | null  // null = custom text
  customText?: string
}

interface AskUserProps {
  request: AskUserRequest
  onSubmit: (answers: AskUserAnswer[]) => void
  onCancel: () => void
}

/** Per-question UI state — persists across tab switches. */
interface QuestionState {
  focusIndex: number
  textInputValue: string
  answer: AskUserAnswer | null
}

// ---------------------------------------------------------------------------
// Tab Bar
// ---------------------------------------------------------------------------

function TabBar({ questions, activeIndex, states, showSubmit }: {
  questions: AskUserQuestion[]
  activeIndex: number
  states: QuestionState[]
  showSubmit: boolean
}) {
  const isMulti = questions.length > 1 || showSubmit
  if (!isMulti) return null

  const totalTabs = questions.length + (showSubmit ? 1 : 0)
  const atStart = activeIndex === 0
  const atEnd = activeIndex === totalTabs - 1

  return (
    <Box flexDirection="row" marginBottom={1}>
      <Text color={atStart ? 'gray' : undefined}>{'← '}</Text>
      {questions.map((q, i) => {
        const isActive = i === activeIndex
        const isAnswered = states[i]?.answer !== null
        const check = isAnswered ? '☑' : '☐'
        const label = ` ${check} ${q.header} `
        return (
          <Box key={i}>
            {isActive
              ? <Text backgroundColor="cyan" color="black">{label}</Text>
              : <Text>{label}</Text>
            }
          </Box>
        )
      })}
      {showSubmit && (
        <Box>
          {activeIndex === questions.length
            ? <Text backgroundColor="cyan" color="black">{' ✓ Submit '}</Text>
            : <Text>{' ✓ Submit '}</Text>
          }
        </Box>
      )}
      <Text color={atEnd ? 'gray' : undefined}>{' →'}</Text>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Question View
// ---------------------------------------------------------------------------

function QuestionView({ question, state }: {
  question: AskUserQuestion
  state: QuestionState
}) {
  const isOnOther = state.focusIndex === question.options.length
  const isTyping = isOnOther

  return (
    <Box flexDirection="column">
      <Box marginBottom={1}>
        <Text bold>{question.question}</Text>
      </Box>

      {question.options.map((opt, i) => {
        const isFocused = !isTyping && i === state.focusIndex
        return (
          <Box key={i} flexDirection="column">
            <Box>
              <Text color={isFocused ? 'cyan' : undefined} bold={isFocused}>
                {isFocused ? '❯ ' : '  '}
              </Text>
              <Text color={isFocused ? 'cyan' : undefined} bold={isFocused}>
                {opt.label}
              </Text>
            </Box>
            {opt.description && (
              <Box marginLeft={4}>
                <Text dimColor>{opt.description}</Text>
              </Box>
            )}
          </Box>
        )
      })}

      {/* "Other" — inline text input, auto-activates on focus */}
      <Box flexDirection="column">
        <Box>
          <Text color={isOnOther ? 'cyan' : undefined} bold={isOnOther}>
            {isOnOther ? '❯ ' : '  '}
          </Text>
          {isOnOther ? (
            <Box>
              <Text color="cyan">{state.textInputValue || ''}</Text>
              <Text inverse>{' '}</Text>
              {!state.textInputValue && (
                <Text dimColor> Type something.</Text>
              )}
            </Box>
          ) : (
            <Text dimColor>
              {state.textInputValue ? state.textInputValue : 'Type something.'}
            </Text>
          )}
        </Box>
      </Box>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Submit Review View
// ---------------------------------------------------------------------------

function SubmitView({ questions, states, selected }: {
  questions: AskUserQuestion[]
  states: QuestionState[]
  selected: number
}) {
  const allAnswered = states.every(s => s.answer !== null)

  return (
    <Box flexDirection="column">
      <Box marginBottom={1}>
        <Text bold>Review your answers</Text>
      </Box>

      {!allAnswered && (
        <Box marginBottom={1}>
          <Text color="yellow">⚠ You have not answered all questions</Text>
        </Box>
      )}

      <Box flexDirection="column" marginBottom={1}>
        {questions.map((q, i) => {
          const a = states[i]?.answer
          if (!a) return null
          const answerText = a.customText ?? q.options[a.selectedOption ?? 0]?.label ?? ''
          return (
            <Box key={i} flexDirection="column" marginLeft={1}>
              <Text>• {q.question}</Text>
              <Box marginLeft={2}>
                <Text color="green">→ {answerText}</Text>
              </Box>
            </Box>
          )
        })}
      </Box>

      <Text dimColor>Ready to submit your answers?</Text>

      <Box flexDirection="column" marginTop={1}>
        {['Submit answers', 'Cancel'].map((label, i) => {
          const isFocused = i === selected
          return (
            <Box key={i}>
              <Text color={isFocused ? 'cyan' : undefined} bold={isFocused}>
                {isFocused ? '❯ ' : '  '}{label}
              </Text>
            </Box>
          )
        })}
      </Box>
    </Box>
  )
}

// ---------------------------------------------------------------------------
// Main Component
// ---------------------------------------------------------------------------

export function AskUser({ request, onSubmit, onCancel }: AskUserProps) {
  const { questions } = request
  const isSingle = questions.length === 1
  const totalTabs = isSingle ? 1 : questions.length + 1

  const [activeTab, setActiveTab] = useState(0)
  const [submitSelected, setSubmitSelected] = useState(0)
  const [states, setStates] = useState<QuestionState[]>(
    () => questions.map(() => ({ focusIndex: 0, textInputValue: '', answer: null }))
  )

  const isSubmitTab = !isSingle && activeTab === questions.length
  const currentQuestion = isSubmitTab ? null : questions[activeTab]
  const currentState = isSubmitTab ? null : states[activeTab]
  const isOnOther = currentState ? currentState.focusIndex === currentQuestion!.options.length : false
  const optionCount = currentQuestion ? currentQuestion.options.length + 1 : 2

  function updateState(tabIndex: number, patch: Partial<QuestionState>) {
    setStates(prev => {
      const next = [...prev]
      next[tabIndex] = { ...next[tabIndex]!, ...patch }
      return next
    })
  }

  function confirmAnswer(tabIndex: number, answer: AskUserAnswer) {
    setStates(prev => {
      const next = [...prev]
      next[tabIndex] = { ...next[tabIndex]!, answer }

      if (isSingle) {
        // Single question — auto-submit (use setTimeout to avoid setState-in-setState)
        setTimeout(() => onSubmit([answer]), 0)
      } else {
        const nextUnanswered = next.findIndex(s => s.answer === null)
        if (nextUnanswered === -1) {
          setActiveTab(questions.length)
          setSubmitSelected(0)
        } else {
          setActiveTab(nextUnanswered)
        }
      }

      return next
    })
  }

  useInput((ch, key) => {
    // --- Typing mode (on "Other" option) ---
    if (isOnOther && !isSubmitTab) {
      if (key.escape) {
        if (currentState!.textInputValue) {
          updateState(activeTab, { textInputValue: '' })
        } else {
          updateState(activeTab, { focusIndex: 0 })
        }
        return
      }
      if (key.return) {
        const text = currentState!.textInputValue.trim()
        if (text) {
          confirmAnswer(activeTab, { questionIndex: activeTab, selectedOption: null, customText: text })
        }
        return
      }
      if (key.upArrow) {
        updateState(activeTab, { focusIndex: currentQuestion!.options.length - 1 })
        return
      }
      // Tab / Shift+Tab to switch tabs even while typing
      if (key.tab) {
        if (key.shift) {
          if (activeTab > 0) setActiveTab(prev => prev - 1)
        } else {
          if (activeTab < totalTabs - 1) {
            setActiveTab(prev => prev + 1)
            if (activeTab + 1 === questions.length) setSubmitSelected(0)
          }
        }
        return
      }
      if (key.backspace || key.delete) {
        updateState(activeTab, {
          textInputValue: currentState!.textInputValue.slice(0, -1),
        })
        return
      }
      // Left/right arrows: switch tabs even while typing
      if (key.leftArrow) {
        if (activeTab > 0) setActiveTab(prev => prev - 1)
        return
      }
      if (key.rightArrow) {
        if (activeTab < totalTabs - 1) {
          setActiveTab(prev => prev + 1)
          if (activeTab + 1 === questions.length) setSubmitSelected(0)
        }
        return
      }
      if (ch && !key.ctrl && !key.meta) {
        updateState(activeTab, {
          textInputValue: currentState!.textInputValue + ch,
        })
      }
      return
    }

    // --- Submit tab ---
    if (isSubmitTab) {
      if (key.upArrow) {
        setSubmitSelected(prev => (prev - 1 + 2) % 2)
        return
      }
      if (key.downArrow) {
        setSubmitSelected(prev => (prev + 1) % 2)
        return
      }
      if (key.return) {
        if (submitSelected === 0) {
          onSubmit(states.filter(s => s.answer !== null).map(s => s.answer!))
        } else {
          onCancel()
        }
        return
      }
      if (key.leftArrow) {
        if (activeTab > 0) {
          setActiveTab(prev => prev - 1)
        }
        return
      }
      if (key.escape || (key.ctrl && ch === 'c')) {
        onCancel()
        return
      }
      return
    }

    // --- Question view navigation ---
    if (key.upArrow) {
      updateState(activeTab, {
        focusIndex: (currentState!.focusIndex - 1 + optionCount) % optionCount,
      })
      return
    }
    if (key.downArrow) {
      updateState(activeTab, {
        focusIndex: (currentState!.focusIndex + 1) % optionCount,
      })
      return
    }
    if (key.leftArrow) {
      if (activeTab > 0) {
        setActiveTab(prev => prev - 1)
      }
      return
    }
    if (key.rightArrow || key.tab) {
      if (activeTab < totalTabs - 1) {
        setActiveTab(prev => prev + 1)
        if (activeTab + 1 === questions.length) {
          setSubmitSelected(0)
        }
      }
      return
    }
    if (key.return) {
      const idx = currentState!.focusIndex
      if (idx < currentQuestion!.options.length) {
        confirmAnswer(activeTab, { questionIndex: activeTab, selectedOption: idx })
      }
      // If on "Other", isOnOther handles it above
      return
    }

    // Digit shortcuts (1-9)
    const digit = parseInt(ch, 10)
    if (digit >= 1 && digit <= currentQuestion!.options.length) {
      confirmAnswer(activeTab, { questionIndex: activeTab, selectedOption: digit - 1 })
      return
    }

    if (key.escape || (key.ctrl && ch === 'c')) {
      onCancel()
      return
    }
  })

  return (
    <Box flexDirection="column" marginTop={1} marginBottom={1}>
      <TabBar
        questions={questions}
        activeIndex={activeTab}
        states={states}
        showSubmit={!isSingle}
      />

      {isSubmitTab ? (
        <SubmitView
          questions={questions}
          states={states}
          selected={submitSelected}
        />
      ) : (
        <QuestionView
          question={currentQuestion!}
          state={currentState!}
        />
      )}

      <Box marginTop={1}>
        <Text dimColor>
          {isOnOther
            ? 'Type your answer · Enter confirm · Esc clear · ↑ back'
            : isSubmitTab
              ? '↑↓ navigate · Enter select · ← tabs · Esc cancel'
              : `↑↓ navigate · Enter select · 1-${currentQuestion!.options.length} shortcut · ←→ tabs · Esc cancel`}
        </Text>
      </Box>
    </Box>
  )
}

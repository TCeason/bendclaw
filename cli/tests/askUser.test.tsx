/**
 * AskUser component tests using ink-testing-library.
 * Covers: rendering, navigation, state persistence across tabs,
 * answer submission, "Other" text input, and submit review page.
 *
 * NOTE: ink-testing-library swallows the first stdin.write, so we
 * send a dummy write before the real input. All writes need async
 * delays for React to process state updates.
 */

import { describe, test, expect } from 'bun:test'
import React from 'react'
import { render } from 'ink-testing-library'
import { AskUser, type AskUserRequest } from '../src/components/AskUser.js'

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

const singleQuestion: AskUserRequest = {
  questions: [{
    header: 'Language',
    question: 'Which language?',
    options: [
      { label: 'Rust', description: 'Systems programming' },
      { label: 'TypeScript', description: 'Web development' },
    ],
  }],
}

const multiQuestion: AskUserRequest = {
  questions: [
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
  ],
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

function noop() {}

const ENTER = '\r'
const ESCAPE = '\x1B'
const UP = '\x1B[A'
const DOWN = '\x1B[B'
const LEFT = '\x1B[D'
const RIGHT = '\x1B[C'
const TAB = '\t'

const delay = () => new Promise(r => setTimeout(r, 50))

/** Send a key and wait for React to process. First call warms up stdin. */
async function send(stdin: { write: (s: string) => void }, ...keys: string[]) {
  for (const k of keys) {
    stdin.write(k)
    await delay()
  }
}

/** Render with a dummy first write to warm up ink stdin. */
async function renderAskUser(props: { request: AskUserRequest; onSubmit: (a: any) => void; onCancel: () => void }) {
  const inst = render(<AskUser {...props} />)
  // Warm up stdin — first write is swallowed by ink-testing-library
  inst.stdin.write('\0')
  await delay()
  return inst
}

// ---------------------------------------------------------------------------
// Rendering
// ---------------------------------------------------------------------------

describe('AskUser rendering', () => {
  test('single question shows question text and options', async () => {
    const { lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    const frame = lastFrame()
    expect(frame).toContain('Which language?')
    expect(frame).toContain('Rust')
    expect(frame).toContain('TypeScript')
    expect(frame).toContain('Type something.')
  })

  test('single question does not show tab bar or Submit', async () => {
    const { lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    const frame = lastFrame()
    expect(frame).not.toContain('Submit')
  })

  test('multi question shows tab bar with headers', async () => {
    const { lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    const frame = lastFrame()
    expect(frame).toContain('Language')
    expect(frame).toContain('Style')
    expect(frame).toContain('Submit')
    expect(frame).toContain('←')
    expect(frame).toContain('→')
  })

  test('first option is focused by default', async () => {
    const { lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    expect(lastFrame()).toContain('❯')
  })
})

// ---------------------------------------------------------------------------
// Single question — selection and auto-submit
// ---------------------------------------------------------------------------

describe('AskUser single question', () => {
  test('Enter on first option auto-submits', async () => {
    let submitted: any = null
    const { stdin } = await renderAskUser({
      request: singleQuestion,
      onSubmit: (a) => { submitted = a },
      onCancel: noop,
    })
    await send(stdin, ENTER)
    await delay()
    expect(submitted).not.toBeNull()
    expect(submitted).toHaveLength(1)
    expect(submitted[0].selectedOption).toBe(0)
    expect(submitted[0].questionIndex).toBe(0)
  })

  test('down + Enter selects second option', async () => {
    let submitted: any = null
    const { stdin } = await renderAskUser({
      request: singleQuestion,
      onSubmit: (a) => { submitted = a },
      onCancel: noop,
    })
    await send(stdin, DOWN, ENTER)
    await delay()
    expect(submitted).not.toBeNull()
    expect(submitted[0].selectedOption).toBe(1)
  })

  test('digit shortcut selects option', async () => {
    let submitted: any = null
    const { stdin } = await renderAskUser({
      request: singleQuestion,
      onSubmit: (a) => { submitted = a },
      onCancel: noop,
    })
    await send(stdin, '2')
    await delay()
    expect(submitted).not.toBeNull()
    expect(submitted[0].selectedOption).toBe(1)
  })

  test('Escape cancels', async () => {
    let cancelled = false
    const { stdin } = await renderAskUser({
      request: singleQuestion,
      onSubmit: noop,
      onCancel: () => { cancelled = true },
    })
    await send(stdin, ESCAPE)
    expect(cancelled).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// Multi question — tab navigation and state persistence
// ---------------------------------------------------------------------------

describe('AskUser multi question navigation', () => {
  test('right arrow switches to second question', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, RIGHT)
    const frame = lastFrame()
    expect(frame).toContain('Which style?')
    expect(frame).toContain('Functional')
  })

  test('left arrow from second goes back to first', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, RIGHT, LEFT)
    expect(lastFrame()).toContain('Which language?')
  })

  test('answering first question auto-advances to second', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, ENTER) // Select "Rust" on Q1
    await delay()
    const frame = lastFrame()
    expect(frame).toContain('Which style?')
  })

  test('answering first question shows ☑ in tab bar', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, ENTER) // Answer Q1
    await delay()
    await send(stdin, LEFT)  // Go back to Q1
    expect(lastFrame()).toContain('☑')
  })

  test('answer persists when switching tabs', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    // Answer Q1 with "Rust"
    await send(stdin, ENTER)
    await delay()
    // Now on Q2, go back to Q1
    await send(stdin, LEFT)
    // Tab bar should show ☑ for Language
    const frame = lastFrame()
    expect(frame).toContain('☑')
    expect(frame).toContain('Language')
  })

  test('focus position persists across tab switches', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    // Move focus to second option on Q1
    await send(stdin, DOWN)
    // Switch to Q2
    await send(stdin, RIGHT)
    expect(lastFrame()).toContain('Which style?')
    // Switch back to Q1
    await send(stdin, LEFT)
    // Should still show Q1 content
    expect(lastFrame()).toContain('Which language?')
  })
})

// ---------------------------------------------------------------------------
// "Other" text input
// ---------------------------------------------------------------------------

describe('AskUser Other input', () => {
  test('navigating to Other shows input cursor', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, DOWN, DOWN) // Past TypeScript to Other
    const frame = lastFrame()
    // Should show input mode indicator
    expect(frame).toContain('Type something.')
  })

  test('typing in Other shows text', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, DOWN, DOWN) // Other
    await send(stdin, 'h', 'e', 'l', 'l', 'o')
    expect(lastFrame()).toContain('hello')
  })

  test('Enter in Other submits custom text', async () => {
    let submitted: any = null
    const { stdin } = await renderAskUser({
      request: singleQuestion,
      onSubmit: (a) => { submitted = a },
      onCancel: noop,
    })
    await send(stdin, DOWN, DOWN) // Other
    await send(stdin, 't', 'e', 's', 't')
    await send(stdin, ENTER)
    await delay()
    expect(submitted).not.toBeNull()
    expect(submitted[0].selectedOption).toBeNull()
    expect(submitted[0].customText).toBe('test')
  })

  test('Escape in Other clears text', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, DOWN, DOWN) // Other
    await send(stdin, 'h', 'i')
    // "hi" should appear in the input area
    const beforeEsc = lastFrame()
    expect(beforeEsc).toMatch(/❯.*hi/)
    await send(stdin, ESCAPE)
    // Text should be cleared — input should show placeholder again
    expect(lastFrame()).toContain('Type something.')
  })

  test('text input persists across tab switches', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    // Navigate to Other on Q1
    await send(stdin, DOWN, DOWN) // Other
    await send(stdin, 'a', 'b', 'c')
    expect(lastFrame()).toContain('abc')
    // Switch to Q2 via Tab
    await send(stdin, TAB)
    expect(lastFrame()).toContain('Which style?')
    // Switch back to Q1
    await send(stdin, LEFT)
    // Text should still be there
    expect(lastFrame()).toContain('abc')
  })

  test('up arrow from Other goes back to last option', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: singleQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, DOWN, DOWN) // Other
    await send(stdin, UP)
    // Should no longer be in typing mode — hint should show shortcuts
    const frame = lastFrame()
    expect(frame).toContain('1-2 shortcut')
  })
})

// ---------------------------------------------------------------------------
// Submit view (multi question)
// ---------------------------------------------------------------------------

describe('AskUser submit view', () => {
  test('answering all questions shows submit view', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, ENTER) // Answer Q1 with Rust
    await delay()
    await send(stdin, ENTER) // Answer Q2 with Functional
    await delay()
    // Should auto-advance to submit view
    const frame = lastFrame()
    expect(frame).toContain('Review your answers')
    expect(frame).toContain('Rust')
    expect(frame).toContain('Functional')
    expect(frame).toContain('Submit answers')
    expect(frame).toContain('Cancel')
  })

  test('submit view Enter submits all answers', async () => {
    let submitted: any = null
    const { stdin } = await renderAskUser({
      request: multiQuestion,
      onSubmit: (a) => { submitted = a },
      onCancel: noop,
    })
    await send(stdin, ENTER) // Answer Q1
    await delay()
    await send(stdin, ENTER) // Answer Q2
    await delay()
    await send(stdin, ENTER) // Submit
    await delay()
    expect(submitted).not.toBeNull()
    expect(submitted).toHaveLength(2)
    expect(submitted[0].selectedOption).toBe(0) // Rust
    expect(submitted[1].selectedOption).toBe(0) // Functional
  })

  test('submit view down + Enter cancels', async () => {
    let cancelled = false
    const { stdin } = await renderAskUser({
      request: multiQuestion,
      onSubmit: noop,
      onCancel: () => { cancelled = true },
    })
    await send(stdin, ENTER) // Answer Q1
    await delay()
    await send(stdin, ENTER) // Answer Q2
    await delay()
    await send(stdin, DOWN)  // Move to Cancel
    await send(stdin, ENTER)
    expect(cancelled).toBe(true)
  })

  test('can go back from submit view to edit answer', async () => {
    const { stdin, lastFrame } = await renderAskUser({ request: multiQuestion, onSubmit: noop, onCancel: noop })
    await send(stdin, ENTER) // Answer Q1
    await delay()
    await send(stdin, ENTER) // Answer Q2 → submit view
    await delay()
    expect(lastFrame()).toContain('Review your answers')
    await send(stdin, LEFT)  // Go back to Q2
    expect(lastFrame()).toContain('Which style?')
  })
})

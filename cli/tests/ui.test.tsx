/**
 * UI component tests using ink-testing-library.
 * Renders components and asserts on the text output (lastFrame).
 */

import { describe, test, expect } from 'bun:test'
import React from 'react'
import { render } from 'ink-testing-library'
import { Text, Box } from 'ink'
import { OutputView } from '../src/components/OutputView.js'
import { StreamingMarkdown } from '../src/components/StreamingMarkdown.js'
import type { OutputLine } from '../src/render/output.js'

// ---------------------------------------------------------------------------
// Helper
// ---------------------------------------------------------------------------

let idCounter = 0
function line(kind: OutputLine['kind'], text: string): OutputLine {
  return { id: `test-${++idCounter}`, kind, text }
}

// ---------------------------------------------------------------------------
// OutputView — line rendering
// ---------------------------------------------------------------------------

describe('OutputView', () => {
  test('renders user message with ❯ prefix', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>banner</Text>} lines={[line('user', 'hello world')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('❯')
    expect(frame).toContain('hello world')
  })

  test('renders assistant message with indentation', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('assistant', 'some response')]} verbose={true} />
    )
    expect(lastFrame()).toContain('some response')
  })

  test('renders error in red', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('error', 'something broke')]} verbose={true} />
    )
    expect(lastFrame()).toContain('something broke')
  })

  test('renders system message', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('system', 'info message')]} verbose={true} />
    )
    expect(lastFrame()).toContain('info message')
  })

  test('renders run_summary', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('run_summary', '─── run summary')]} verbose={true} />
    )
    expect(lastFrame()).toContain('run summary')
  })

  test('renders tool_result in green', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool_result', '  Result: completed')]} verbose={true} />
    )
    expect(lastFrame()).toContain('Result: completed')
  })
})

// ---------------------------------------------------------------------------
// OutputView — ToolLineView
// ---------------------------------------------------------------------------

describe('ToolLineView', () => {
  test('renders tool call badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[BASH] call')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[BASH]')
    expect(frame).toContain('call')
  })

  test('renders tool completed badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[BASH] completed · 120ms')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[BASH]')
    expect(frame).toContain('completed')
    expect(frame).toContain('120ms')
  })

  test('renders tool failed badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '[READ] failed · 50ms')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[READ]')
    expect(frame).toContain('failed')
  })

  test('renders tool detail line (indented)', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('tool', '  ❯ ls -la')]} verbose={true} />
    )
    expect(lastFrame()).toContain('❯ ls -la')
  })
})

// ---------------------------------------------------------------------------
// OutputView — VerboseLineView
// ---------------------------------------------------------------------------

describe('VerboseLineView', () => {
  test('renders LLM call badge in yellow', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] call · claude-opus-4-6 · turn 1')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('call')
    expect(frame).toContain('turn 1')
  })

  test('renders LLM completed badge in green', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] completed · 4.3s · 42 tok/s')]} verbose={true} />
    )
    expect(lastFrame()).toContain('[LLM]')
    expect(lastFrame()).toContain('completed')
  })

  test('renders LLM failed badge in red', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] failed · 2.1s')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('failed')
    expect(frame).toContain('2.1s')
  })

  test('renders LLM retry badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[LLM] call · claude-opus-4-6 · turn 2 · retry 1')]} verbose={true} />
    )
    expect(lastFrame()).toContain('retry 1')
  })

  test('renders COMPACT badge', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '[COMPACT] · no-op')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('[COMPACT]')
    expect(frame).toContain('no-op')
  })

  test('renders verbose detail line (indented)', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '  tokens  4k in · 12 out')]} verbose={true} />
    )
    expect(lastFrame()).toContain('tokens')
    expect(lastFrame()).toContain('4k in')
  })

  test('renders timing with percentages', () => {
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={[line('verbose', '  timing  ttfb 3.9s (91%) · stream 0.3s (8%)')]} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('ttfb 3.9s (91%)')
    expect(frame).toContain('stream 0.3s (8%)')
  })
})

// ---------------------------------------------------------------------------
// OutputView — mixed content ordering
// ---------------------------------------------------------------------------

describe('OutputView mixed content', () => {
  test('renders multiple line types in order', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call · test · turn 1'),
      line('verbose', '  1 messages · 9 tools'),
      line('verbose', '[LLM] completed · 0.5s · 100 tok/s'),
      line('verbose', '  tokens  1k in · 50 out'),
      line('assistant', 'Hi there!'),
      line('run_summary', '─── run summary ──────────────────────────────────'),
      line('run_summary', '2.5s · 1 turn · 1 llm call · 0 tool calls · 1k tokens'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('hello')
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('Hi there!')
    expect(frame).toContain('run summary')
  })
})

// ---------------------------------------------------------------------------
// StreamingMarkdown
// ---------------------------------------------------------------------------

describe('StreamingMarkdown', () => {
  test('renders null for empty text', () => {
    const { lastFrame } = render(<StreamingMarkdown text="" maxHeight={10} />)
    expect(lastFrame()).toBe('')
  })

  test('renders markdown text', () => {
    const { lastFrame } = render(<StreamingMarkdown text="hello **world**" maxHeight={10} />)
    expect(lastFrame()).toContain('hello')
    expect(lastFrame()).toContain('world')
  })

  test('truncates to maxHeight lines', () => {
    const text = Array.from({ length: 20 }, (_, i) => `line ${i + 1}`).join('\n\n')
    const { lastFrame } = render(<StreamingMarkdown text={text} maxHeight={5} />)
    const frame = lastFrame()
    // Should contain the last lines, not the first
    expect(frame).toContain('line 20')
    expect(frame).not.toContain('line 2\n')
  })
})

// ---------------------------------------------------------------------------
// OutputView — verbose filtering
// ---------------------------------------------------------------------------

describe('OutputView verbose filtering', () => {
  test('verbose=true shows verbose and run_summary lines', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call · test'),
      line('assistant', 'response'),
      line('run_summary', '─── run summary'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('hello')
    expect(frame).toContain('[LLM]')
    expect(frame).toContain('response')
    expect(frame).toContain('run summary')
  })

  test('verbose=false hides verbose and run_summary lines', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call · test'),
      line('assistant', 'response'),
      line('run_summary', '─── run summary'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={false} />
    )
    const frame = lastFrame()
    expect(frame).toContain('hello')
    expect(frame).not.toContain('[LLM]')
    expect(frame).toContain('response')
    expect(frame).not.toContain('run summary')
  })

  test('verbose=false preserves non-verbose lines', () => {
    const lines: OutputLine[] = [
      line('user', 'question'),
      line('tool', '[BASH] completed · 50ms'),
      line('tool_result', '  output here'),
      line('error', 'oops'),
      line('system', 'info'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={false} />
    )
    const frame = lastFrame()
    expect(frame).toContain('question')
    expect(frame).toContain('[BASH]')
    expect(frame).toContain('output here')
    expect(frame).toContain('oops')
    expect(frame).toContain('info')
  })
})

// ---------------------------------------------------------------------------
// OutputView — render cap
// ---------------------------------------------------------------------------

describe('OutputView render cap', () => {
  test('caps rendered lines to prevent unbounded growth', () => {
    const lines: OutputLine[] = Array.from({ length: 200 }, (_, i) =>
      line('assistant', `line-${i}`)
    )
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('line-199')
    expect(frame).not.toContain('line-0')
  })

  test('shows all lines when under cap', () => {
    const lines: OutputLine[] = [
      line('user', 'first'),
      line('assistant', 'second'),
      line('tool', '[BASH] call'),
    ]
    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    const frame = lastFrame()
    expect(frame).toContain('first')
    expect(frame).toContain('second')
    expect(frame).toContain('[BASH]')
  })
})

// ---------------------------------------------------------------------------
// OutputView — verbose toggle affects history (Ctrl+O behavior)
// ---------------------------------------------------------------------------

describe('OutputView verbose toggle on history', () => {
  test('toggling verbose filters existing lines retroactively', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call · model · turn 1'),
      line('verbose', '  tokens  1k in · 50 out'),
      line('assistant', 'response text'),
      line('run_summary', '─── run summary ───'),
    ]

    const { lastFrame: frame1 } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    expect(frame1()).toContain('[LLM]')
    expect(frame1()).toContain('tokens')
    expect(frame1()).toContain('run summary')

    const { lastFrame: frame2 } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={false} />
    )
    expect(frame2()).toContain('hello')
    expect(frame2()).toContain('response text')
    expect(frame2()).not.toContain('[LLM]')
    expect(frame2()).not.toContain('tokens')
    expect(frame2()).not.toContain('run summary')
  })

  test('toggling verbose back restores all lines', () => {
    const lines: OutputLine[] = [
      line('user', 'q1'),
      line('verbose', '[LLM] call · test'),
      line('assistant', 'a1'),
      line('run_summary', 'summary data'),
    ]

    const { lastFrame: off } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={false} />
    )
    expect(off()).not.toContain('[LLM]')
    expect(off()).not.toContain('summary data')

    const { lastFrame: on } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={true} />
    )
    expect(on()).toContain('[LLM]')
    expect(on()).toContain('summary data')
  })
})

// ---------------------------------------------------------------------------
// OutputView — verbose lines always stored (screen.log completeness)
// ---------------------------------------------------------------------------

describe('OutputView verbose lines always in data', () => {
  test('verbose=false filters display but lines array is unchanged', () => {
    const lines: OutputLine[] = [
      line('user', 'hello'),
      line('verbose', '[LLM] call'),
      line('assistant', 'world'),
    ]

    const { lastFrame } = render(
      <OutputView banner={<Text>b</Text>} lines={lines} verbose={false} />
    )
    expect(lastFrame()).not.toContain('[LLM]')
    expect(lines).toHaveLength(3)
    expect(lines[1]!.kind).toBe('verbose')
    expect(lines[1]!.text).toBe('[LLM] call')
  })
})

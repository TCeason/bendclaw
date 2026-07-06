import { describe, expect, test } from 'bun:test'
import { transcriptToMessages } from '../src/session/transcript.js'

describe('transcript conversion', () => {
  test('repairs legacy text/thinking split at paired backticks on resume', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        text: '每条都停在 `',
        thinking: '` 里的推理中途:\n- 第 1 题',
      },
    ])

    expect(messages).toHaveLength(1)
    expect(messages[0]?.text).toContain('每条都停在 `')
    expect(messages[0]?.text).toContain('` 里的推理中途')
  })

  test('does not expose ordinary thinking on resume', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        text: 'final answer',
        thinking: 'internal reasoning',
      },
    ])

    expect(messages).toHaveLength(1)
    expect(messages[0]?.text).toBe('final answer')
  })

  test('restores plan tool-result details onto the tool call for resume', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        text: 'proposing a plan',
        tool_calls: [{ id: 'call-1', name: 'plan', input: { action: 'propose' } }],
      },
      {
        type: 'tool_result',
        tool_call_id: 'call-1',
        tool_name: 'plan',
        content: 'Plan approved (2 tasks).',
        is_error: false,
        details: {
          action: 'propose',
          approved: true,
          goal: {
            tasks: [
              { id: 1, title: 'Load data', status: 'completed' },
              { id: 2, title: 'Transform', status: 'in_progress', deps: [1] },
            ],
          },
        },
      },
    ])

    const toolCalls = messages[0]?.toolCalls
    expect(toolCalls).toHaveLength(1)
    const details = toolCalls?.[0]?.details as { goal?: { tasks?: unknown[] } } | undefined
    expect(Array.isArray(details?.goal?.tasks)).toBe(true)
    expect(details?.goal?.tasks).toHaveLength(2)
  })

  test('tool call without details leaves details undefined', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        text: 'running bash',
        tool_calls: [{ id: 'c2', name: 'bash', input: { command: 'ls' } }],
      },
      {
        type: 'tool_result',
        tool_call_id: 'c2',
        tool_name: 'bash',
        content: 'file.txt',
        is_error: false,
      },
    ])

    expect(messages[0]?.toolCalls?.[0]?.details).toBeUndefined()
  })
})

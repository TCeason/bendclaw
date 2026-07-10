import { describe, expect, test } from 'bun:test'
import { messagesToOutputLines } from '../src/render/output.js'
import { transcriptToMessages } from '../src/session/transcript.js'

describe('transcript conversion', () => {
  test('restores canonical assistant content in provider order', () => {
    const messages = transcriptToMessages([{
      type: 'assistant',
      content: [
        { type: 'thinking', text: 'plan' },
        { type: 'tool_call', id: 'c1', name: 'read', input: { path: 'a' } },
        { type: 'text', text: 'answer' },
      ],
    }])

    expect(messages[0]?.text).toBe('answer')
    expect(messages[0]?.content?.map(block => block.type)).toEqual(['thinking', 'tool_call', 'text'])
    expect(messagesToOutputLines(messages).map(line => line.kind)).toContain('thinking')
  })

  test('restores tool-result details onto the canonical tool block', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        content: [
          { type: 'text', text: 'running a tool' },
          { type: 'tool_call', id: 'call-1', name: 'bash', input: { command: 'ls' } },
        ],
      },
      {
        type: 'tool_result',
        tool_call_id: 'call-1',
        tool_name: 'bash',
        content: 'done',
        is_error: false,
        details: { diff: 'a\nb' },
      },
    ])

    const block = messages[0]?.content?.find(block => block.type === 'tool_call')
    expect(block?.type === 'tool_call' ? block.toolCall.details : undefined).toEqual({ diff: 'a\nb' })
    expect(block?.type === 'tool_call' ? block.toolCall.status : undefined).toBe('done')
  })

  test('tool call without result remains queued', () => {
    const messages = transcriptToMessages([{
      type: 'assistant',
      content: [
        { type: 'tool_call', id: 'c2', name: 'bash', input: { command: 'ls' } },
      ],
    }])

    const block = messages[0]?.content?.find(block => block.type === 'tool_call')
    expect(block?.type === 'tool_call' ? block.toolCall.status : undefined).toBe('queued')
    expect(block?.type === 'tool_call' ? block.toolCall.details : undefined).toBeUndefined()
  })

  test('reads pre-migration content_blocks in mixed existing sessions', () => {
    const messages = transcriptToMessages([{
      type: 'assistant',
      content_blocks: [{ type: 'text', text: 'old answer' }],
    }])

    expect(messages[0]?.text).toBe('old answer')
  })

  test('does not replay historical runtime errors on resume', () => {
    const messages = transcriptToMessages([
      {
        type: 'stats',
        kind: 'llm_call_completed',
        data: { error: 'Auth error: HTTP 403', turn: 4 },
      },
      {
        type: 'assistant',
        content: [{ type: 'text', text: 'answer after error' }],
      },
    ])

    expect(messages[0]?.verboseEvents).toBeUndefined()
    const rendered = messagesToOutputLines(messages).map(line => line.text).join('\n')
    expect(rendered).toContain('answer after error')
    expect(rendered).not.toContain('403')
  })
})

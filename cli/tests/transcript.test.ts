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

  test('restored tool details render the same semantic status as live cards', () => {
    const messages = transcriptToMessages([
      {
        type: 'assistant',
        content: [
          { type: 'tool_call', id: 'call-edit', name: 'edit', input: { path: 'src/a.ts' } },
        ],
      },
      {
        type: 'tool_result',
        tool_call_id: 'call-edit',
        tool_name: 'edit',
        content: 'Updated src/a.ts.',
        is_error: false,
        details: {
          diff: '@@ -1 +1 @@\n-old\n+new',
          replacement_count: 1,
          added_lines: 1,
          removed_lines: 1,
        },
      },
    ])

    const rendered = messagesToOutputLines(messages).map(line => line.text)
    expect(rendered[0]).toBe('✎ edit  src/a.ts')
    expect(rendered[1]).toBe('  ✓ · 1 replacement · +1 −1')
    expect(rendered.join('\n')).toContain('+new')
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

  test('restores compact transcript markers as expandable resume cards', () => {
    const messages = transcriptToMessages([{
      type: 'compact',
      reason: 'manual',
      summary: 'summary text',
      tokens_before: 12345,
      tokens_after: 4567,
      messages_before: 20,
      messages_after: 4,
      details: { method: 'remote', remote_blob_bytes: 2048 },
    } as any])

    expect(messages).toHaveLength(1)
    expect(messages[0]?.compaction).toEqual({
      reason: 'manual',
      summary: 'summary text',
      tokensBefore: 12345,
      tokensAfter: 4567,
      messagesBefore: 20,
      messagesAfter: 4,
      method: 'remote',
      remoteBlobBytes: 2048,
    })

    const collapsed = messagesToOutputLines(messages).map(line => line.text).join('\n')
    expect(collapsed).toContain('✦ compact')
    expect(collapsed).toContain('summary hidden (ctrl+o to expand)')
    expect(collapsed).not.toContain('summary text')

    const expanded = messagesToOutputLines(messages, true).map(line => line.text).join('\n')
    expect(expanded).toContain('✦ compact')
    expect(expanded).toContain('summary text')
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

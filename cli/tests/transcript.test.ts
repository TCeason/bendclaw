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
})

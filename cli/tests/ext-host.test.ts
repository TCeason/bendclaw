import { describe, expect, test } from 'bun:test'
import {
  dispatchHostToolCall,
  HOST_TOOL_SPECS_JSON,
  type AskUserAnswer,
  type AskUserParams,
} from '../src/term/host-tools.js'

const answers: AskUserAnswer[] = [
  { header: 'Choice', question: 'Which option?', answer: 'First' },
]

async function collect(_params: AskUserParams): Promise<AskUserAnswer[]> {
  return answers
}

describe('host tools', () => {
  test('advertises the ask_user spec', () => {
    const specs = JSON.parse(HOST_TOOL_SPECS_JSON)
    expect(specs).toHaveLength(1)
    expect(specs[0].name).toBe('ask_user')
  })

  test('dispatches ask_user and formats answers', async () => {
    const response = await dispatchHostToolCall({
      tool_name: 'ask_user',
      tool_call_id: 'c1',
      arguments: { questions: [] },
    }, collect)

    expect(response.tool_call_id).toBe('c1')
    expect(response.is_error).toBe(false)
    expect(response.content[0].text).toContain('Which option? → First')
  })

  test('resolves the model alias case-insensitively', async () => {
    const response = await dispatchHostToolCall({
      tool_name: 'AskUser',
      tool_call_id: 'c2',
      arguments: { questions: [] },
    }, collect)

    expect(response.is_error).toBe(false)
  })

  test('returns an error for unknown tools', async () => {
    const response = await dispatchHostToolCall({
      tool_name: 'nope',
      tool_call_id: 'c3',
      arguments: {},
    }, collect)

    expect(response.is_error).toBe(true)
    expect(response.content[0].text).toContain('Unknown host tool')
  })

  test('returns an error when the user cancels', async () => {
    const response = await dispatchHostToolCall({
      tool_name: 'ask_user',
      tool_call_id: 'c4',
      arguments: { questions: [] },
    }, async () => null)

    expect(response.is_error).toBe(true)
    expect(response.content[0].text).toContain('cancelled')
  })

  test('catches collection errors instead of throwing', async () => {
    const response = await dispatchHostToolCall({
      tool_name: 'ask_user',
      tool_call_id: 'c5',
      arguments: { questions: [] },
    }, async () => {
      throw new Error('kaboom')
    })

    expect(response.is_error).toBe(true)
    expect(response.content[0].text).toBe('kaboom')
  })
})

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
  test('advertises the ask_user spec with explicit object-array guidance', () => {
    const specs = JSON.parse(HOST_TOOL_SPECS_JSON)
    expect(specs).toHaveLength(1)

    const spec = specs[0]
    expect(spec.name).toBe('ask_user')
    expect(spec.description).toContain('Every item in "questions" and "options" is an object')

    const example = spec.description.split('Example: ')[1]?.split('\n\nUsers')[0]
    expect(example).toBeDefined()
    expect(JSON.parse(example!)).toEqual({
      questions: [{
        header: 'Scope',
        question: 'Which scope?',
        options: [
          { label: 'Minimal (Recommended)', description: 'Make the smallest change.' },
          { label: 'Complete', description: 'Cover the broader change.' },
        ],
      }],
    })

    const schema = spec.parameters_schema
    expect(schema.additionalProperties).toBe(false)
    expect(schema.properties.questions.description).toContain('complete JSON object')
    expect(schema.properties.questions.items.additionalProperties).toBe(false)
    expect(schema.properties.questions.items.properties.options.description).toContain('Each item must be enclosed in { }')
    expect(schema.properties.questions.items.properties.options.items.additionalProperties).toBe(false)
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

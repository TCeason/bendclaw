import { describe, expect, test } from 'bun:test'
import { sortModelOptionsForSelector } from '../src/term/app/provider.js'

const options = [
  { provider: 'anthropic', model: 'claude-opus-4-8', spec: 'anthropic:claude-opus-4-8' },
  { provider: 'openai', model: 'grok-4.5', spec: 'openai:grok-4.5' },
  { provider: 'droid', model: 'gpt-5.6-sol', spec: 'droid:gpt-5.6-sol' },
  { provider: 'openai', model: 'gpt-5.6-sol', spec: 'openai:gpt-5.6-sol' },
  { provider: 'anthropic', model: 'claude-sonnet-5', spec: 'anthropic:claude-sonnet-5' },
]

describe('sortModelOptionsForSelector', () => {
  test('keeps providers contiguous and puts the active provider first', () => {
    const sorted = sortModelOptionsForSelector(options, 'openai:gpt-5.6-sol')

    expect(sorted.map(option => option.spec)).toEqual([
      'openai:gpt-5.6-sol',
      'openai:grok-4.5',
      'anthropic:claude-opus-4-8',
      'anthropic:claude-sonnet-5',
      'droid:gpt-5.6-sol',
    ])
  })

  test('preserves configured order within inactive provider groups', () => {
    const sorted = sortModelOptionsForSelector(options, 'droid:gpt-5.6-sol')

    expect(sorted.map(option => option.spec)).toEqual([
      'droid:gpt-5.6-sol',
      'anthropic:claude-opus-4-8',
      'anthropic:claude-sonnet-5',
      'openai:grok-4.5',
      'openai:gpt-5.6-sol',
    ])
  })
})

import { describe, expect, test } from 'bun:test'
import { formatModelOptionDetail, sortModelOptionsForSelector } from '../src/term/app/provider.js'

const options = [
  { provider: 'anthropic', protocol: 'anthropic' as const, model: 'claude-opus-4-8', spec: 'anthropic:claude-opus-4-8' },
  { provider: 'openai', protocol: 'openai_responses' as const, model: 'grok-4.5', spec: 'openai:grok-4.5' },
  { provider: 'droid', protocol: 'openai' as const, model: 'gpt-5.6-sol', spec: 'droid:gpt-5.6-sol' },
  { provider: 'openai', protocol: 'openai_responses' as const, model: 'gpt-5.6-sol', spec: 'openai:gpt-5.6-sol' },
  { provider: 'anthropic', protocol: 'anthropic' as const, model: 'claude-sonnet-5', spec: 'anthropic:claude-sonnet-5' },
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

  test('labels each provider group with its wire protocol', () => {
    expect(formatModelOptionDetail(options[0]!)).toBe('anthropic · Anthropic Messages')
    expect(formatModelOptionDetail(options[1]!)).toBe('openai · OpenAI Responses')
    expect(formatModelOptionDetail(options[2]!)).toBe('droid · OpenAI Chat Completions')
  })
})

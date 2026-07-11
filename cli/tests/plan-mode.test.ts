import { describe, expect, test } from 'bun:test'
import { extractPlanItems } from '../src/term/plan-mode.js'

describe('extractPlanItems', () => {
  test('pulls numbered steps under a Plan: header', () => {
    const md = [
      'Some context about the change.',
      '',
      'Plan:',
      '1. Inspect the prompt-mode plumbing',
      '2. Update the CLI renderer',
      '3. Run the targeted tests',
    ].join('\n')

    const items = extractPlanItems(md)
    expect(items).toHaveLength(3)
    expect(items[0]).toEqual({ step: 1, text: 'Inspect the prompt-mode plumbing' })
    expect(items[2]!.text).toBe('Run the targeted tests')
  })

  test('handles bold header and strips markdown emphasis', () => {
    const md = '**Plan:**\n1. **Bold** step with `code`\n2. Second step'
    const items = extractPlanItems(md)
    expect(items).toHaveLength(2)
    expect(items[0]!.text).toBe('Bold step with code')
  })

  test('returns empty when no Plan: header is present', () => {
    expect(extractPlanItems('Just a normal reply with 1. a list')).toEqual([])
  })
})

import { describe, expect, test } from 'bun:test'
import {
  extractPlanItems,
  markCompletedPlanItems,
  planItemsToTasks,
  footerLabel,
} from '../src/term/plan-mode.js'

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
    expect(items[0]).toEqual({ step: 1, text: 'Inspect the prompt-mode plumbing', completed: false })
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

describe('markCompletedPlanItems', () => {
  test('marks steps referenced by [DONE:n] tags', () => {
    const items = extractPlanItems('Plan:\n1. First\n2. Second\n3. Third')
    const changed = markCompletedPlanItems('Finished step [DONE:1] and [DONE:3].', items)
    expect(changed).toBe(2)
    expect(items[0]!.completed).toBe(true)
    expect(items[1]!.completed).toBe(false)
    expect(items[2]!.completed).toBe(true)
  })

  test('does not recount already-completed steps', () => {
    const items = extractPlanItems('Plan:\n1. First\n2. Second')
    markCompletedPlanItems('[DONE:1]', items)
    const changed = markCompletedPlanItems('[DONE:1] [DONE:2]', items)
    expect(changed).toBe(1)
  })
})

describe('planItemsToTasks and footerLabel', () => {
  test('maps items to tasks with pending/completed status', () => {
    const items = extractPlanItems('Plan:\n1. First\n2. Second')
    items[0]!.completed = true
    const tasks = planItemsToTasks(items)
    expect(tasks).toEqual([
      { id: 1, title: 'First', status: 'completed' },
      { id: 2, title: 'Second', status: 'pending' },
    ])
  })

  test('footerLabel summarizes completion, null when empty', () => {
    const items = extractPlanItems('Plan:\n1. First\n2. Second')
    items[0]!.completed = true
    expect(footerLabel(planItemsToTasks(items))).toBe('📋 1/2')
    expect(footerLabel(null)).toBeNull()
    expect(footerLabel([])).toBeNull()
  })
})

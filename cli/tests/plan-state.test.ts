import { describe, expect, test } from 'bun:test'
import { footerLabel, normalizeTasks, summarize, tasksFromDetails, validatePlan } from '../src/ext/builtin/plan-artifact/state.js'
import type { PlanTask } from '../src/ext/types.js'

function task(id: number, over: Partial<PlanTask> = {}): PlanTask {
  return { id, title: `task ${id}`, status: 'pending', ...over }
}

describe('validatePlan', () => {
  test('rejects an empty plan', () => {
    expect(validatePlan([])).toContain('at least one task')
  })

  test('rejects duplicate ids', () => {
    expect(validatePlan([task(1), task(1)])).toContain('duplicate task id')
  })

  test('rejects empty titles', () => {
    expect(validatePlan([task(1, { title: '  ' })])).toContain('non-empty title')
  })

  test('rejects unknown dependency', () => {
    expect(validatePlan([task(1, { deps: [9] })])).toContain('unknown task #9')
  })

  test('rejects self dependency', () => {
    expect(validatePlan([task(1, { deps: [1] })])).toContain('cannot depend on itself')
  })

  test('detects a dependency cycle', () => {
    const plan = [task(1, { deps: [2] }), task(2, { deps: [3] }), task(3, { deps: [1] })]
    expect(validatePlan(plan)).toContain('cycle')
  })

  test('accepts a valid DAG', () => {
    const plan = [task(1), task(2, { deps: [1] }), task(3, { deps: [1, 2] })]
    expect(validatePlan(plan)).toBeNull()
  })
})

describe('normalizeTasks', () => {
  test('defaults missing status to pending and trims titles', () => {
    const out = normalizeTasks([{ id: 1, title: '  x  ', status: 'bogus' as never }])
    expect(out[0].status).toBe('pending')
    expect(out[0].title).toBe('x')
  })

  test('stamps started_at when a task goes in_progress', () => {
    const out = normalizeTasks([task(1, { status: 'in_progress' })])
    expect(out[0].started_at).toBeDefined()
    expect(out[0].completed_at).toBeUndefined()
  })

  test('stamps completed_at for terminal states', () => {
    const done = normalizeTasks([task(1, { status: 'completed' })])
    expect(done[0].completed_at).toBeDefined()
    const failed = normalizeTasks([task(1, { status: 'failed' })])
    expect(failed[0].completed_at).toBeDefined()
  })

  test('preserves existing timestamps', () => {
    const out = normalizeTasks([
      task(1, { status: 'completed', started_at: 'S', completed_at: 'C' }),
    ])
    expect(out[0].started_at).toBe('S')
    expect(out[0].completed_at).toBe('C')
  })

  test('drops empty deps arrays', () => {
    const out = normalizeTasks([task(1, { deps: [] })])
    expect(out[0].deps).toBeUndefined()
  })
})

describe('summarize', () => {
  test('reports completed count and current task', () => {
    const plan = [
      task(1, { status: 'completed' }),
      task(2, { status: 'in_progress' }),
      task(3),
    ]
    expect(summarize(plan)).toBe('1/3 completed · current #2 task 2')
  })

  test('reports failures', () => {
    const plan = [task(1, { status: 'failed' }), task(2, { status: 'completed' })]
    expect(summarize(plan)).toBe('1/2 completed · 1 failed')
  })
})

describe('tasksFromDetails', () => {
  test('extracts tasks from a plan details payload', () => {
    const tasks = tasksFromDetails({
      goal: { tasks: [{ id: 1, title: 'a', status: 'completed' }, { id: 2, title: 'b', status: 'pending' }] },
    })
    expect(tasks).toHaveLength(2)
    expect(tasks?.[0]?.status).toBe('completed')
  })

  test('returns null for non-plan payloads', () => {
    expect(tasksFromDetails(undefined)).toBeNull()
    expect(tasksFromDetails({})).toBeNull()
    expect(tasksFromDetails({ goal: {} })).toBeNull()
    expect(tasksFromDetails({ goal: { tasks: [] } })).toBeNull()
  })

  test('skips malformed task entries', () => {
    const tasks = tasksFromDetails({
      goal: { tasks: [{ id: 1, title: 'ok', status: 'pending' }, { title: 'no id' }, null] },
    })
    expect(tasks).toHaveLength(1)
  })
})

describe('footerLabel', () => {
  test('renders completed/total', () => {
    const plan = [task(1, { status: 'completed' }), task(2), task(3)]
    expect(footerLabel(plan)).toBe('📋 1/3')
  })

  test('appends failure count when tasks failed', () => {
    const plan = [task(1, { status: 'completed' }), task(2, { status: 'failed' })]
    expect(footerLabel(plan)).toBe('📋 1/2 · 1✗')
  })

  test('returns null for an empty or missing plan', () => {
    expect(footerLabel(null)).toBeNull()
    expect(footerLabel([])).toBeNull()
  })
})

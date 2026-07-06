/**
 * Pure helpers for plan validation, normalization, and summarizing.
 * Extracted for testability — no I/O, no UI.
 */

import type { PlanTask, PlanTaskStatus } from '../../types.js'

const VALID_STATUS: ReadonlySet<string> = new Set([
  'pending',
  'in_progress',
  'completed',
  'failed',
])

const NOW_TIMESTAMP = () => new Date().toISOString()

/**
 * Validate a proposed/updated task list. Returns an error string, or null when
 * the plan is well-formed. Catches the failure modes that would otherwise
 * surface as a confusing artifact: empty plans, duplicate ids, unknown deps,
 * self-deps, and dependency cycles.
 */
export function validatePlan(tasks: PlanTask[]): string | null {
  if (!Array.isArray(tasks) || tasks.length === 0) {
    return 'plan must contain at least one task'
  }

  const ids = new Set<number>()
  for (const task of tasks) {
    if (typeof task.id !== 'number' || !Number.isInteger(task.id)) {
      return `task id must be an integer (got ${JSON.stringify(task.id)})`
    }
    if (ids.has(task.id)) {
      return `duplicate task id: ${task.id}`
    }
    ids.add(task.id)
    if (typeof task.title !== 'string' || task.title.trim().length === 0) {
      return `task #${task.id} must have a non-empty title`
    }
    if (task.status !== undefined && !VALID_STATUS.has(task.status)) {
      return `task #${task.id} has invalid status: ${task.status}`
    }
  }

  for (const task of tasks) {
    for (const dep of task.deps ?? []) {
      if (dep === task.id) return `task #${task.id} cannot depend on itself`
      if (!ids.has(dep)) return `task #${task.id} depends on unknown task #${dep}`
    }
  }

  const cycle = findCycle(tasks)
  if (cycle) return `dependency cycle detected: ${cycle.join(' → ')}`

  return null
}

/** Detect a dependency cycle via DFS. Returns the cycle path, or null. */
function findCycle(tasks: PlanTask[]): number[] | null {
  const byId = new Map(tasks.map(t => [t.id, t]))
  const state = new Map<number, 'visiting' | 'done'>()
  const stack: number[] = []

  const visit = (id: number): number[] | null => {
    const marked = state.get(id)
    if (marked === 'done') return null
    if (marked === 'visiting') {
      const start = stack.indexOf(id)
      return [...stack.slice(start), id]
    }
    state.set(id, 'visiting')
    stack.push(id)
    for (const dep of byId.get(id)?.deps ?? []) {
      const found = visit(dep)
      if (found) return found
    }
    stack.pop()
    state.set(id, 'done')
    return null
  }

  for (const task of tasks) {
    const found = visit(task.id)
    if (found) return found
  }
  return null
}

/**
 * Normalize tasks for storage: default missing status to pending, and stamp
 * timing so the renderer can show durations. `started_at` is set when a task
 * first goes in_progress; `completed_at` when it reaches a terminal state.
 */
export function normalizeTasks(tasks: PlanTask[]): PlanTask[] {
  return tasks.map(task => {
    const status: PlanTaskStatus = VALID_STATUS.has(task.status) ? task.status : 'pending'
    const out: PlanTask = {
      id: task.id,
      title: task.title.trim(),
      status,
      deps: task.deps && task.deps.length > 0 ? [...task.deps] : undefined,
      started_at: task.started_at,
      completed_at: task.completed_at,
    }
    if (status === 'in_progress' && !out.started_at) {
      out.started_at = NOW_TIMESTAMP()
    }
    if ((status === 'completed' || status === 'failed') && !out.completed_at) {
      out.completed_at = out.completed_at ?? NOW_TIMESTAMP()
    }
    return out
  })
}

/** One-line progress summary, e.g. "2/5 completed · current #3 Load data". */
export function summarize(tasks: PlanTask[]): string {
  const completed = tasks.filter(t => t.status === 'completed').length
  const failed = tasks.filter(t => t.status === 'failed').length
  const current = tasks.find(t => t.status === 'in_progress')
  const parts = [`${completed}/${tasks.length} completed`]
  if (failed > 0) parts.push(`${failed} failed`)
  if (current) parts.push(`current #${current.id} ${current.title}`)
  return parts.join(' · ')
}

/**
 * Extract the plan task list from a `plan` tool-result `details` payload
 * (shape: `{ goal: { tasks: [...] } }`), or null when the payload is not a
 * recognizable plan. Used to derive the persistent footer indicator from both
 * the live dispatch result and a restored transcript entry.
 */
export function tasksFromDetails(details: unknown): PlanTask[] | null {
  if (!details || typeof details !== 'object') return null
  const goal = (details as { goal?: unknown }).goal
  if (!goal || typeof goal !== 'object') return null
  const tasks = (goal as { tasks?: unknown }).tasks
  if (!Array.isArray(tasks)) return null
  const parsed = tasks.flatMap((t): PlanTask[] => {
    if (!t || typeof t !== 'object') return []
    const input = t as Record<string, unknown>
    const id = typeof input.id === 'number' ? input.id : Number(input.id)
    const title = typeof input.title === 'string' ? input.title : ''
    if (!Number.isFinite(id) || title.length === 0) return []
    const status = (typeof input.status === 'string' ? input.status : 'pending') as PlanTaskStatus
    return [{ id, title, status }]
  })
  return parsed.length > 0 ? parsed : null
}

/**
 * Compact footer label for the active plan, e.g. "📋 2/5" (or "📋 2/5 · 1✗"
 * when tasks failed). Returns null when there is no plan to show.
 */
export function footerLabel(tasks: PlanTask[] | null): string | null {
  if (!tasks || tasks.length === 0) return null
  const completed = tasks.filter(t => t.status === 'completed').length
  const failed = tasks.filter(t => t.status === 'failed').length
  const base = `📋 ${completed}/${tasks.length}`
  return failed > 0 ? `${base} · ${failed}✗` : base
}

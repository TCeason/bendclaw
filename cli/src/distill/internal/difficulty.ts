/**
 * difficulty — coarse task-difficulty labels.
 *
 * Difficulty is a complexity *label* only: distill imposes no runtime limits
 * based on it. The proposer authors more or less complex tasks per tier (more
 * endpoints, edge cases, and moving parts as the tier rises), and the label is
 * carried into the dataset metadata so downstream training can filter, balance,
 * or schedule by difficulty. The number in each name is an ordinal complexity
 * level, not a turn budget.
 */

import type { Difficulty } from './types.js'

/** Ordered difficulty tiers, from least to most complex. */
export const DIFFICULTIES: readonly Difficulty[] = ['L2', 'L4', 'L6', 'L8', 'L16']

/** Human-facing complexity guidance for the proposer, per tier. */
export const DIFFICULTY_GUIDANCE: Record<Difficulty, string> = {
  L2: 'trivial: a single endpoint or function with no edge cases',
  L4: 'easy: one small feature with minimal validation',
  L6: 'moderate: a small CRUD surface with basic validation and error handling',
  L8: 'involved: multiple endpoints with validation, error handling, and state',
  L16: 'complex: multiple interacting modules, rich edge cases, and careful error handling',
}

/** Whether a string is a known difficulty tier. */
export function isDifficulty(value: string): value is Difficulty {
  return (DIFFICULTIES as readonly string[]).includes(value)
}

/** Build a per-task difficulty plan of length n.
 *  - a fixed tier repeats that tier n times
 *  - 'mixed' round-robins the tiers for an even spread */
export function difficultyPlan(n: number, difficulty: Difficulty | 'mixed'): Difficulty[] {
  if (n <= 0) return []
  if (difficulty !== 'mixed') return Array.from({ length: n }, () => difficulty)
  return Array.from({ length: n }, (_, i) => DIFFICULTIES[i % DIFFICULTIES.length])
}

/** Compact human-readable summary of a plan, e.g. "L2×2 L4×1 L8×3". */
export function summarizePlan(plan: Difficulty[]): string {
  const counts: Partial<Record<Difficulty, number>> = {}
  for (const d of plan) counts[d] = (counts[d] ?? 0) + 1
  return DIFFICULTIES.filter((d) => counts[d]).map((d) => `${d}×${counts[d]}`).join(' ')
}

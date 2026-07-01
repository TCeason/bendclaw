/**
 * Tests for the distill difficulty helper: tiers are pure complexity labels
 * (no runtime limits), and `mixed` spreads tasks evenly across tiers.
 */

import { test, expect } from 'bun:test'
import {
  DIFFICULTIES,
  isDifficulty,
  difficultyPlan,
  summarizePlan,
} from '../src/distill/internal/difficulty.js'

test('DIFFICULTIES are ordered tiers and isDifficulty guards membership', () => {
  expect(DIFFICULTIES).toEqual(['L2', 'L4', 'L6', 'L8', 'L16'])
  expect(isDifficulty('L4')).toBe(true)
  expect(isDifficulty('L16')).toBe(true)
  expect(isDifficulty('mixed')).toBe(false)
  expect(isDifficulty('L3')).toBe(false)
  expect(isDifficulty('')).toBe(false)
})

test('difficultyPlan repeats a fixed tier n times', () => {
  expect(difficultyPlan(3, 'L2')).toEqual(['L2', 'L2', 'L2'])
  expect(difficultyPlan(0, 'L8')).toEqual([])
  expect(difficultyPlan(-2, 'L8')).toEqual([])
})

test('difficultyPlan mixed round-robins tiers for an even spread', () => {
  expect(difficultyPlan(5, 'mixed')).toEqual(['L2', 'L4', 'L6', 'L8', 'L16'])
  // Wraps past the tier count.
  expect(difficultyPlan(7, 'mixed')).toEqual(['L2', 'L4', 'L6', 'L8', 'L16', 'L2', 'L4'])
})

test('summarizePlan reports counts in tier order', () => {
  expect(summarizePlan(['L2', 'L4', 'L2', 'L8'])).toBe('L2×2 L4×1 L8×1')
  expect(summarizePlan([])).toBe('')
})

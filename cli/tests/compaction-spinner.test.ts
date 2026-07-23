import { describe, expect, test } from 'bun:test'
import { toolActionLabel } from '../src/term/spinner.js'

describe('compaction spinner labels', () => {
  test('shows the active compaction method', () => {
    expect(toolActionLabel('compact')).toBe('Compacting')
    expect(toolActionLabel('compact_remote')).toBe('Compacting remote')
    expect(toolActionLabel('compact_local')).toBe('Compacting local')
    expect(toolActionLabel('compact_local_fallback')).toBe('Compacting local fallback')
  })
})

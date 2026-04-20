import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createSelectorState,
  selectorUp,
  selectorDown,
  selectorSelect,
} from '../src/term/selector.js'
import { buildOverlayBlocks } from '../src/term/viewmodel/overlays.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import stripAnsi from 'strip-ansi'
import chalk from 'chalk'

beforeAll(() => { chalk.level = 3 })

const items = [
  { label: 'claude-opus', detail: 'Anthropic' },
  { label: 'gpt-4o', detail: 'OpenAI' },
  { label: 'gemini-pro', detail: 'Google' },
]

describe('createSelectorState', () => {
  test('creates state with focus at 0', () => {
    const state = createSelectorState('Pick model', items)
    expect(state.focusIndex).toBe(0)
    expect(state.title).toBe('Pick model')
    expect(state.items).toBe(items)
  })
})

describe('selectorUp', () => {
  test('moves focus up', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 2 }
    state = selectorUp(state)
    expect(state.focusIndex).toBe(1)
  })

  test('does not go below 0', () => {
    const state = createSelectorState('T', items)
    const next = selectorUp(state)
    expect(next.focusIndex).toBe(0)
    expect(next).toBe(state)
  })
})

describe('selectorDown', () => {
  test('moves focus down', () => {
    const state = createSelectorState('T', items)
    const next = selectorDown(state)
    expect(next.focusIndex).toBe(1)
  })

  test('does not exceed last item', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 2 }
    const next = selectorDown(state)
    expect(next.focusIndex).toBe(2)
    expect(next).toBe(state)
  })
})

describe('selectorSelect', () => {
  test('returns focused item', () => {
    let state = createSelectorState('T', items)
    state = { ...state, focusIndex: 1 }
    const selected = selectorSelect(state)
    expect(selected).toEqual({ label: 'gpt-4o', detail: 'OpenAI' })
  })

  test('returns first item by default', () => {
    const state = createSelectorState('T', items)
    const selected = selectorSelect(state)
    expect(selected).toEqual({ label: 'claude-opus', detail: 'Anthropic' })
  })

  test('returns null for empty items', () => {
    const state = createSelectorState('T', [])
    const selected = selectorSelect(state)
    expect(selected).toBeNull()
  })
})

describe('renderSelector via viewmodel', () => {
  test('contains title', () => {
    const state = createSelectorState('Pick model', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('Pick model')
  })

  test('contains all item labels', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('claude-opus')
    expect(text).toContain('gpt-4o')
    expect(text).toContain('gemini-pro')
  })

  test('contains detail text', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('Anthropic')
    expect(text).toContain('OpenAI')
  })

  test('shows focus indicator on current item', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('❯ claude-opus')
  })

  test('shows navigation hint', () => {
    const state = createSelectorState('T', items)
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('navigate')
    expect(text).toContain('enter select')
    expect(text).toContain('esc cancel')
  })
})

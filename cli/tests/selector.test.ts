import { describe, test, expect, beforeAll } from 'bun:test'
import {
  createSelectorState,
  selectorUp,
  selectorDown,
  selectorSelect,
  selectorType,
  selectorBackspace,
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

  test('shows search query when filtering', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('search:')
    expect(text).toContain('g')
  })

  test('shows "No matches" when filter yields nothing', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    state = selectorType(state, 'z')
    const lines = blocksToLines(buildOverlayBlocks({ kind: 'selector', state }, 80))
    const text = lines.map(l => stripAnsi(l)).join('\n')
    expect(text).toContain('No matches')
  })
})

describe('selectorType', () => {
  test('filters items by label', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    expect(state.query).toBe('g')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
    expect(state.focusIndex).toBe(0)
  })

  test('filters items by detail', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'o')
    state = selectorType(state, 'p')
    state = selectorType(state, 'e')
    state = selectorType(state, 'n')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o'])
  })

  test('is case insensitive', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'G')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
  })

  test('resets focus on filter change', () => {
    let state = createSelectorState('T', items)
    state = selectorDown(state)
    expect(state.focusIndex).toBe(1)
    state = selectorType(state, 'g')
    expect(state.focusIndex).toBe(0)
  })
})

describe('selectorBackspace', () => {
  test('removes last char and widens filter', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    state = selectorType(state, 'p')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o'])
    state = selectorBackspace(state)
    expect(state.query).toBe('g')
    expect(state.items.map(i => i.label)).toEqual(['gpt-4o', 'gemini-pro'])
  })

  test('clears filter restores all items', () => {
    let state = createSelectorState('T', items)
    state = selectorType(state, 'g')
    state = selectorBackspace(state)
    expect(state.query).toBe('')
    expect(state.items).toEqual(items)
  })

  test('noop when query is empty', () => {
    const state = createSelectorState('T', items)
    const next = selectorBackspace(state)
    expect(next).toBe(state)
  })
})

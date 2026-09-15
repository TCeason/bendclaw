import { describe, expect, test } from 'bun:test'
import { createModelWindow, createResumeWindow } from '../src/term/app/selector-windows.js'
import { createTaskModelWindow, refreshTaskModelWindow } from '../src/task/model-picker.js'
import { SELECTOR_OWNER } from '../src/term/app/selector-identity.js'
import { buildCommandSelectorRegion } from '../src/term/viewmodel/command-selector.js'
import { buildSelectorRegionLines } from '../src/term/viewmodel/selector.js'
import { selectorAdjustEffort, selectorType } from '../src/term/selector.js'
import type { ConfigInfo } from '../src/native/contracts/config-info.js'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import { TASK_RUNTIME_DEFAULT_MODEL } from '../src/task/types.js'

const config: ConfigInfo = {
  provider: 'test', protocol: 'openai', envPath: '', hasApiKey: true, baseUrl: null, thinkingLevel: '',
  availableModels: [
    { provider: 'test', model: 'a', spec: 'test:a' },
    { provider: 'test', model: 'b', spec: 'test:b' },
  ],
}

describe('selector window composition', () => {
  test('preview and explicit model entry share rows and selection, differing only in focus', () => {
    const preview = createModelWindow(config, 'b')
    const explicit = createModelWindow(config, 'b', true)
    expect(explicit).toEqual({ ...preview, listFocused: true })
    expect(preview.owner).toBe(SELECTOR_OWNER.model)
    expect(preview.items[preview.focusIndex]?.id).toBe('test:b')
    expect(preview.listFocused).toBe(false)
  })

  test('task model picker starts on the Task model and submits the row the user moves to', () => {
    // The live evot model is a, while the persisted Task model is b.
    const task = createTaskModelWindow(config, 'a', 'test:b')
    expect(task.owner).toBe(SELECTOR_OWNER.taskModel)
    expect(task.title).toBe('Task model')
    expect(task.items[task.focusIndex]?.id).toBe('test:b')
    expect(task.items.some(item => item.id === TASK_RUNTIME_DEFAULT_MODEL)).toBe(true)

    const moved = handleSelectorControl(task, { type: 'down' })
    expect(moved.kind).toBe('update')
    if (moved.kind !== 'update') throw new Error('expected model picker movement')
    expect(moved.state.items[moved.state.focusIndex]?.id).toBe('test:a')
    expect(handleSelectorControl(moved.state, { type: 'enter' })).toEqual({
      kind: 'select-task-model',
      spec: 'test:a',
    })
  })

  test('task model picker does not fall back to the evot model when the saved model is unavailable', () => {
    const task = createTaskModelWindow(config, 'a', 'retired:saved-task-model', 'high')
    expect(task.items[task.focusIndex]?.id).toBe('retired:saved-task-model')
    expect(task.items[task.focusIndex]?.selected).toBe(true)
    expect(task.items[task.focusIndex]?.detail).toBe('(currently unavailable)')
    expect(task.items[task.focusIndex]?.id).not.toBe('test:a')

    const moved = handleSelectorControl(task, { type: 'down' })
    expect(moved.kind).toBe('update')
    if (moved.kind !== 'update') throw new Error('expected model picker movement')
    expect(moved.state.items[moved.state.focusIndex]?.id).toBe('test:a')
  })

  test('task model picker can preselect the runtime-default policy', () => {
    const task = createTaskModelWindow(config, 'a', TASK_RUNTIME_DEFAULT_MODEL)
    expect(task.items[task.focusIndex]?.id).toBe(TASK_RUNTIME_DEFAULT_MODEL)
    expect(handleSelectorControl(task, { type: 'enter' })).toEqual({
      kind: 'select-task-model',
      spec: TASK_RUNTIME_DEFAULT_MODEL,
    })
  })

  test('task catalog refresh preserves query, focus, policy, and adjusted thinking level', () => {
    const initial: ConfigInfo = {
      ...config,
      availableModels: [
        { provider: 'test', model: 'a', spec: 'test:a' },
        {
          provider: 'test', model: 'b', spec: 'test:b',
          thinking_level: 'medium', thinking_levels: ['low', 'medium', 'high'],
        },
      ],
    }
    let task = createTaskModelWindow(initial, 'a', 'test:b', 'medium')
    task = selectorAdjustEffort(task, 1)
    task = selectorType(task, 'b')

    const refreshed = refreshTaskModelWindow(task, {
      ...initial,
      availableModels: [
        ...initial.availableModels,
        { provider: 'test', model: 'c', spec: 'test:c' },
      ],
    }, 'a')

    expect(refreshed.owner).toBe(SELECTOR_OWNER.taskModel)
    expect(refreshed.query).toBe('b')
    expect(refreshed.items[refreshed.focusIndex]?.id).toBe('test:b')
    expect(refreshed.items[refreshed.focusIndex]?.effort).toEqual({
      levels: ['low', 'medium', 'high'], index: 2,
    })
    expect(refreshed.allItems.find(item => item.id === 'test:b')?.selected).toBe(true)
    expect(refreshed.allItems.some(item => item.id === 'test:c')).toBe(true)
    expect(refreshed.subtitle).toContain('Choose for this task')
  })

  test('resume keeps cross-workspace items searchable without exposing them initially', () => {
    const items = [{ id: 's1', label: 'Other workspace', searchOnly: true }]
    const state = createResumeWindow(items)
    expect(state.items).toEqual([])
    expect(state.emptyMessage).toContain('No sessions in current cwd')
    expect(createResumeWindow(items, 'Other').items).toHaveLength(1)
  })

  test('slot keeps original content as a suffix and does not shrink on focus', () => {
    const state = createModelWindow(config, 'a')
    for (const [columns, rows] of [[30, 10], [80, 24], [160, 40]]) {
      const preview = buildCommandSelectorRegion(state, columns, rows, false)
      const focused = buildCommandSelectorRegion(state, columns, rows, true)
      expect(preview.length).toBe(focused.length)
      const raw = buildSelectorRegionLines(state, columns, rows, false)
      expect(preview.slice(-raw.length)).toEqual(raw)
      expect(preview.slice(0, -raw.length).every(line => line === '')).toBe(true)
    }
  })
})

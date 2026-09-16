import { expect, test } from 'bun:test'
import { shareSelectorState, ShareSelector, type SharedSession } from '../src/term/app/share-selector.js'
import { handleSelectorControl } from '../src/term/app/selector-control.js'
import { selectorType, type SelectorState } from '../src/term/selector.js'

const shares: SharedSession[] = ['Alpha', 'Beta'].map((title, index) => ({
  id: `share-${index}`, title, url: `https://evot.ai/share/share-${index}`,
  created_at: 1_700_000_000_000, size_bytes: 2048,
}))

test('shares use the same gestures as sessions and tasks: d d deletes, / searches, esc closes', () => {
  const state = shareSelectorState(shares)
  expect(state.listFocused).toBe(true)
  expect(state.previewPane?.confirmDeleteKey).toBe('d')
  expect(handleSelectorControl(state, { type: 'enter' })).toEqual({ kind: 'open-share', shareId: 'share-0' })
  // While the list owns the input, letters are actions rather than a filter...
  const first = handleSelectorControl(state, { type: 'char', char: 'd' })
  if (first.kind !== 'update') throw new Error('expected armed delete')
  expect(first.state.pendingDeleteId).toBe('share-0')
  expect(handleSelectorControl(first.state, { type: 'char', char: 'd' })).toMatchObject({ kind: 'delete-share', shareId: 'share-0' })
  expect(handleSelectorControl(first.state, { type: 'escape' }).kind).toBe('update')
  // ...and `/` hands the input to the filter, where typing searches.
  const searching = handleSelectorControl(state, { type: 'char', char: '/' })
  if (searching.kind !== 'update') throw new Error('expected filter focus')
  expect(searching.state.listFocused).toBe(false)
  const typed = handleSelectorControl(searching.state, { type: 'char', char: 'B' })
  if (typed.kind !== 'update') throw new Error('expected filter update')
  expect(typed.state.items.map(item => item.id)).toEqual(['share-1'])
  expect(selectorType(state, 'Beta').items.map(item => item.id)).toEqual(['share-1'])
  const armed = handleSelectorControl(state, { type: 'ctrl', key: 'd' })
  if (armed.kind !== 'update') throw new Error('expected armed delete')
  expect(armed.state.pendingDeleteId).toBe('share-0')
  expect(handleSelectorControl(armed.state, { type: 'delete' }).kind).toBe('delete-share')
  const moved = handleSelectorControl(armed.state, { type: 'down' })
  if (moved.kind !== 'update') throw new Error('expected movement')
  expect(moved.state.pendingDeleteId).toBeUndefined()
  expect(handleSelectorControl(moved.state, { type: 'ctrl', key: 'd' }).kind).toBe('update')
  expect(handleSelectorControl(state, { type: 'escape' }).kind).toBe('close')
})

function setup(remove: (id: string) => Promise<void> = async () => {}) {
  let state: SelectorState | undefined
  const opened: string[] = []
  const controller = new ShareSelector({
    list: async () => shares,
    delete: remove,
    open: async url => { opened.push(url) },
    current: () => state,
    publish: next => { state = next },
  })
  return { controller, opened, current: () => state, close: () => { state = undefined }, set: (next: SelectorState) => { state = next } }
}

test('delete success removes row only after server success; open uses selected URL', async () => {
  let finish: () => void = () => {}
  const ctx = setup(() => new Promise<void>(resolve => { finish = resolve }))
  await ctx.controller.load()
  await ctx.controller.open('share-1')
  expect(ctx.opened).toEqual([shares[1]!.url])
  const pending = ctx.controller.delete('share-0')
  expect(ctx.current()?.items.length).toBe(2)
  finish()
  await pending
  expect(ctx.current()?.items.map(item => item.id)).toEqual(['share-1'])
})

test('failed deletion retains row with visible error and can retry', async () => {
  const ctx = setup(async () => { throw new Error('offline') })
  await ctx.controller.load()
  await ctx.controller.delete('share-0')
  expect(ctx.current()?.items.length).toBe(2)
  expect(ctx.current()?.subtitle).toBe('offline')
})

test('delete completion respects filtering and does not reopen a closed selector', async () => {
  let finish: () => void = () => {}
  const ctx = setup(() => new Promise<void>(resolve => { finish = resolve }))
  await ctx.controller.load()
  const pending = ctx.controller.delete('share-0')
  const state = ctx.current()
  if (!state) throw new Error('missing state')
  ctx.set(selectorType(state, 'Beta'))
  finish()
  await pending
  expect(ctx.current()?.items.map(item => item.id)).toEqual(['share-1'])
  expect(ctx.current()?.allItems.map(item => item.id)).toEqual(['share-1'])
  const next = ctx.controller.delete('share-1')
  ctx.close()
  finish()
  await next
  expect(ctx.current()).toBeUndefined()
})

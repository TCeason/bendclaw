import { expect, test } from 'bun:test'
import { mkdtempSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { AuthIdentityTracker } from '../src/term/app/auth-identity.js'
import { TaskSession, type TaskSessionHost, type TaskSessionApi } from '../src/task/session.js'
import type { SelectorState } from '../src/term/selector.js'
import type { ScheduledTask, TaskListResponse } from '../src/task/types.js'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: Error) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

const task = (id: string): ScheduledTask => ({
  id, revision: 1, name: id, cron: '* * * * *', timezone: 'UTC', instruction: 'hi', executor_id: 'exec',
  model_policy: 'default', model_spec: '', thinking_level: '', workspace_ref: '',
  delivery_channel: 'feishu', delivery_target: 'oc_me', timeout_seconds: 60,
  max_lateness_seconds: 60, enabled: true, next_run_at: 0,
})
const list = (stale = false): TaskListResponse => ({ tasks: [task('a'), task('b')], cache: { ready: true, synced_at: 1, stale } })
const flush = async () => { for (let i = 0; i < 8; i++) await Promise.resolve() }
const d = { type: 'char', char: 'd' } as const

function harness(api: Partial<TaskSessionApi>, hostOverrides: Partial<TaskSessionHost> = {}) {
  let overlay: SelectorState | null = null
  const errors: string[] = []
  const host: TaskSessionHost = {
    configInfo: () => undefined, activeModel: () => '', activeModelSpec: () => '',
    modelOptionLabel: model => model.model, ensureDelivery: async () => false,
    isTaskOverlay: () => overlay !== null, taskOverlayState: () => overlay,
    showSelector: state => { overlay = state }, closeOverlay: () => { overlay = null },
    requestRender: () => {}, notifyError: error => { errors.push(error) },
    collectAnswers: async () => null, presentModelPicker: async () => null,
    runTaskTurn: () => {}, primeInput: () => {}, destroyed: () => false,
    ...hostOverrides,
  }
  const session = new TaskSession(host, {
    list: async () => list(), get: async id => ({ ...task(id), runs: [] }),
    delete: async () => {}, update: async id => ({ task: task(id), next_runs: [] }),
    run: async () => {}, ...api,
  })
  return { session, errors, view: () => overlay }
}

test('cold open is immediate, shares one request, and Esc does not wait for I/O', async () => {
  const pending = deferred<TaskListResponse>()
  let requests = 0
  const h = harness({ list: () => { requests++; return pending.promise } })
  expect(h.session.open()).toBeUndefined()
  expect(h.view()?.emptyMessage).toBe('Loading tasks…')
  h.session.open()
  await flush()
  expect(requests).toBe(1)
  await h.session.handleKey({ type: 'escape' })
  pending.resolve(list())
  await flush()
  expect(h.view()).toBeNull()
  h.session.open()
  expect(h.view()?.items.map(row => row.id)).toEqual(['a', 'b'])
  await flush()
  expect(requests).toBe(1)
  h.session.dispose()
})

test('delete gives immediate feedback, deduplicates presses, and removes without a second list call', async () => {
  const deletion = deferred<void>()
  let deletes = 0
  let lists = 0
  const h = harness({ list: async () => { lists++; return list() }, delete: () => { deletes++; return deletion.promise } })
  h.session.open()
  await flush()
  await h.session.handleKey(d)
  const action = h.session.handleKey(d)
  expect(h.view()?.subtitle).toContain('Deleting')
  expect(h.view()?.items[0]?.detail).toBe('Deleting…')
  await h.session.handleKey(d)
  await h.session.handleKey(d)
  expect(deletes).toBe(1)
  deletion.resolve()
  await action
  expect(h.view()?.items.map(row => row.id)).toEqual(['b'])
  expect(h.view()?.focusIndex).toBe(0)
  expect(lists).toBe(1)
  h.session.dispose()
})

test('failed delete preserves the row and releases busy state', async () => {
  const deletion = deferred<void>()
  const h = harness({ delete: () => deletion.promise })
  h.session.open()
  await flush()
  await h.session.handleKey(d)
  const action = h.session.handleKey(d)
  deletion.reject(new Error('timeout'))
  await action
  expect(h.view()?.items.map(row => row.id)).toEqual(['a', 'b'])
  expect(h.view()?.items[0]?.detail).not.toBe('Deleting…')
  expect(h.errors[0]).toContain('timeout')
  h.session.dispose()
})

test('a refresh started before deletion cannot resurrect the deleted row', async () => {
  const refresh = deferred<TaskListResponse>()
  let lists = 0
  const h = harness({ list: () => ++lists === 1 ? Promise.resolve(list(true)) : refresh.promise })
  h.session.open()
  await flush()
  h.session.open()
  await flush()
  await h.session.handleKey(d)
  await h.session.handleKey(d)
  refresh.resolve(list())
  await flush()
  expect(h.view()?.items.map(row => row.id)).toEqual(['b'])
  h.session.dispose()
})

test('deleting while navigating does not steal focus, and completion cannot reopen a closed list', async () => {
  const deletion = deferred<void>()
  const h = harness({ delete: () => deletion.promise })
  h.session.open()
  await flush()
  await h.session.handleKey(d)
  const action = h.session.handleKey(d)
  await h.session.handleKey({ type: 'down' })
  expect(h.view()?.items[h.view()!.focusIndex]?.id).toBe('b')
  await h.session.handleKey({ type: 'escape' })
  deletion.resolve()
  await action
  expect(h.view()).toBeNull()
  h.session.open()
  expect(h.view()?.items.map(row => row.id)).toEqual(['b'])
  h.session.dispose()
})

test('run now while a run is in flight reports queued, not a failure', async () => {
  const h = harness(
    { run: async () => { throw new Error('/v1/tasks/x/run: task already has an active run') } },
    { collectAnswers: async questions => questions.map(q => ({ header: q.header, question: q.question, answer: 'Run now' })) },
  )
  h.session.open()
  await flush()
  await h.session.handleKey({ type: 'char', char: 'r' })
  await flush()
  expect(h.errors.some(e => e.includes('already has a run in flight'))).toBe(true)
  expect(h.errors.some(e => e.includes('Task operation failed'))).toBe(false)
  h.session.dispose()
})

test('background refresh preserves navigation and armed deletion; errors keep cached rows', async () => {
  const refresh = deferred<TaskListResponse>()
  let calls = 0
  const h = harness({ list: () => ++calls === 1 ? Promise.resolve(list(true)) : refresh.promise })
  h.session.open()
  await flush()
  h.session.open()
  expect(h.view()?.items.map(row => row.id)).toEqual(['a', 'b'])
  expect(h.view()?.subtitle).toBe('Refreshing…')
  await h.session.handleKey({ type: 'down' })
  await h.session.handleKey(d)
  refresh.resolve(list(true))
  await flush()
  expect(h.view()?.items[h.view()!.focusIndex]?.id).toBe('b')
  expect(h.view()?.pendingDeleteId).toBe('b')
  h.session.dispose()

  const broken = harness({ list: async () => { throw new Error('offline') } })
  broken.session.open()
  await flush()
  expect(broken.view()?.emptyMessage).toBe('Could not load tasks')
  expect(broken.errors[0]).toContain('offline')
  broken.session.dispose()
})

test('task list stays open during same-account auth/catalog refresh and closes on real account switch', async () => {
  const root = mkdtempSync(join(tmpdir(), 'evot-task-auth-'))
  const h = harness({})
  try {
    const path = join(root, 'auth.json')
    const auth = { user: { id: 'a' }, server_base_url: 'https://cloud.example', cli_token: 'test-token' }
    writeFileSync(path, JSON.stringify(auth))
    const identity = new AuthIdentityTracker(() => h.session.resetIdentity(), root)
    h.session.open()
    await flush()
    await h.session.handleKey({ type: 'down' })
    await h.session.handleKey(d)
    const before = h.view()
    writeFileSync(path, JSON.stringify({ ...auth, cli_token: 'rotated-token', models_synced_at: 123 }))
    writeFileSync(join(root, 'models.cache.json'), '{"revision":2}')
    identity.refresh()
    expect(h.view()).toBe(before)
    expect(h.view()?.pendingDeleteId).toBe('b')
    writeFileSync(path, JSON.stringify({ ...auth, user: { id: 'b' } }))
    identity.refresh()
    expect(h.view()).toBeNull()
  } finally {
    h.session.dispose()
    rmSync(root, { recursive: true, force: true })
  }
})

test('identity change discards cached rows and in-flight responses', async () => {
  const old = deferred<TaskListResponse>()
  let calls = 0
  const h = harness({ list: () => ++calls === 1 ? old.promise : Promise.resolve({ ...list(), tasks: [task('new-account')] }) })
  h.session.open()
  await flush()
  h.session.resetIdentity()
  h.session.open()
  await flush()
  old.resolve(list())
  await flush()
  expect(h.view()?.items.map(row => row.id)).toEqual(['new-account'])
  h.session.dispose()
})

test('list refresh invalidates detail data even when task revision is unchanged', async () => {
  let calls = 0
  const h = harness({
    list: async () => { calls++; return { ...list(true), tasks: [{ ...task('a'), name: calls > 1 ? 'Fresh' : 'Old' }] } },
    get: async () => ({ ...task('a'), name: 'Old detail', runs: [] }),
  })
  h.session.open()
  await flush()
  await h.session.handleKey({ type: 'enter' })
  h.session.open()
  await flush()
  expect(h.view()?.items[0]?.label).toContain('Fresh')
  expect(h.view()?.items[0]?.label).not.toContain('Old')
  h.session.dispose()
})

test('late task details cannot replace another overlay', async () => {
  const detail = deferred<ScheduledTask & { runs: [] }>()
  const h = harness({ get: () => detail.promise })
  h.session.open()
  await flush()
  const action = h.session.handleKey({ type: 'enter' })
  await h.session.handleKey({ type: 'escape' })
  detail.resolve({ ...task('a'), runs: [] })
  await action
  expect(h.view()).toBeNull()
  h.session.dispose()
})

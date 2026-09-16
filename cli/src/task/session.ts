/** `/task` orchestration: list window lifecycle, key handling, and the agent
 *  turns that create or edit a task.
 *
 *  The REPL owns the terminal; this owns Task behaviour. Everything the REPL
 *  must provide is named in `TaskSessionHost`, so no Task state leaks into it.
 */

import type { ConfigInfo, ModelOption } from '../native/contracts/config-info.js'
import { selectorFocusOn, type SelectorState } from '../term/selector.js'
import type { KeyEvent } from '../term/input.js'
import type { AskUserAnswer, AskUserQuestion, HostToolExtension } from '../term/host-tools.js'
/** Signatures of the real RPCs. Type-only, so importing this module still
 *  loads no native addon — the calls below resolve it on first use. */
import type { deleteTask, getTask, listTasks, runTask, updateTask } from './client.js'
import { handleTaskKey } from './control.js'
import { createTaskExtension } from './host-tool.js'
import type { TaskModelDefaults, TaskModelPickerRequest, TaskModelSelection } from './model-picker.js'
import { createTaskFlowState, createTaskPrompt, updateTaskPrompt, type TaskPromptContext } from './prompt.js'
import { createTaskWindow } from './window.js'
import type { ScheduledTask, TaskListResponse } from './types.js'

/** How often an open Task list re-reads cloud state. */
const REFRESH_INTERVAL_MS = 10_000

/** Narrow transport seam for deterministic deferred-response tests. */
export interface TaskSessionApi {
  list: typeof listTasks
  get: typeof getTask
  delete: typeof deleteTask
  update: typeof updateTask
  run: typeof runTask
}

const taskApi: TaskSessionApi = {
  list: async () => (await import('./client.js')).listTasks(),
  get: async id => (await import('./client.js')).getTask(id),
  delete: async id => (await import('./client.js')).deleteTask(id),
  update: async (id, input, envFile) => (await import('./client.js')).updateTask(id, input, envFile),
  run: async id => (await import('./client.js')).runTask(id),
}

export interface TaskSessionHost {
  dimensions?: () => { columns: number; rows: number }
  envFile?: string
  ensureDelivery: (signal: AbortSignal) => Promise<boolean>
  configInfo: () => ConfigInfo | undefined
  activeModelSpec: () => string
  activeModel: () => string
  modelOptionLabel: (option: ModelOption) => string
  /** True while the Task list itself is the visible overlay. */
  isTaskOverlay: () => boolean
  taskOverlayState: () => SelectorState | null
  showSelector: (state: SelectorState) => void
  closeOverlay: () => void
  requestRender: () => void
  notifyError: (text: string) => void
  collectAnswers: (questions: AskUserQuestion[]) => Promise<AskUserAnswer[] | null>
  presentModelPicker: (request: TaskModelPickerRequest) => Promise<TaskModelSelection | null>
  /** Show `userLine` as the user's turn, then run `prompt` with Task tools. */
  runTaskTurn: (userLine: string, prompt: string, extension: HostToolExtension) => void
  /** Put text in the editor instead of running it, for `n` → `/task `. */
  primeInput: (text: string) => void
  destroyed: () => boolean
}

export class TaskSession {
  #host: TaskSessionHost
  #response: TaskListResponse | null = null
  #api: TaskSessionApi
  #listRequest: Promise<void> | null = null
  #revision = 0
  #identityGeneration = 0
  #loadedAt = 0
  #pending = new Map<string, string>()
  #detail: ScheduledTask | undefined
  #detailRequest = 0
  #loadError = false
  #disposed = false
  /** Bumped whenever the window is closed or reopened, so a slow in-flight
   *  refresh can tell that its result is no longer wanted. */
  #generation = 0
  #setup = new AbortController()

  cancelSetup(): void { this.#setup.abort() }

  resetIdentity(): void {
    this.cancelSetup()
    this.invalidate()
    this.#identityGeneration++
    this.#revision++
    this.#response = null
    this.#detail = undefined
    this.#loadedAt = 0
    this.#pending.clear()
    this.#listRequest = null
    if (this.#host.isTaskOverlay()) this.#host.closeOverlay()
  }

  constructor(host: TaskSessionHost, api: TaskSessionApi = taskApi) {
    this.#host = host
    this.#api = api
  }

  /** Scheduling belongs to the host background scheduler, not the feature. */
  refreshIfVisible(): Promise<void> {
    return this.#host.isTaskOverlay() ? this.#refresh() : Promise.resolve()
  }

  dispose(): void {
    this.#disposed = true
    this.invalidate()
    this.cancelSetup()
  }

  /** `/task` with no argument, and every return to the list after an action. */
  open(focusId?: string): void {
    if (this.#disposed || this.#host.destroyed()) return
    this.invalidate()
    this.#loadError = false
    // Open before starting I/O. The first load has a placeholder; subsequent
    // opens render cached rows immediately, even if the server is unavailable.
    this.#paint(focusId, true)
    if (!this.#response || this.#response.cache.stale || Date.now() - this.#loadedAt >= REFRESH_INTERVAL_MS) {
      void this.#refresh()
    }
  }

  /** `/task <prompt>`: create via an agent turn. */
  create(userLine: string, request: string): void {
    const flow = createTaskFlowState('create')
    this.#host.runTaskTurn(
      userLine,
      createTaskPrompt(request, this.#promptContext()),
      this.#extension(flow),
    )
  }

  /** Called when the Task list stops being the visible overlay. */
  invalidate(): void {
    this.#generation++
    this.#detailRequest++
  }

  async handleKey(event: KeyEvent): Promise<void> {
    const state = this.#host.taskOverlayState()
    if (!state) return
    const size = this.#host.dimensions?.()
    const action = handleTaskKey(state, event, size?.columns, size?.rows)
    const focused = state.items[state.focusIndex]?.id
    if (focused && this.#pending.has(focused) && event.type === 'char' && event.char === 'd') return
    switch (action.kind) {
      case 'none':
        return
      case 'update':
        this.#detailRequest++
        this.#host.showSelector(action.state)
        this.#host.requestRender()
        return
      case 'close':
        this.#close()
        this.#host.requestRender()
        return
      case 'create':
        this.#close()
        this.#host.primeInput('/task ')
        this.#host.requestRender()
        return
      case 'detail':
      case 'history':
        await this.#showDetail(action.id)
        return
      default:
        await this.#mutate(action.kind, action.id)
    }
  }

  async #showDetail(id: string): Promise<void> {
    if (this.#pending.has(id)) return
    const generation = this.#generation
    const revision = this.#revision
    const request = ++this.#detailRequest
    const current = () => !this.#disposed && !this.#host.destroyed()
      && this.#host.isTaskOverlay() && generation === this.#generation
      && request === this.#detailRequest && revision === this.#revision
    try {
      const task = await this.#api.get(id)
      if (!current()) return
      this.#detail = task
      this.#paint()
    } catch (error) {
      if (current()) this.#host.notifyError(`Failed to load task: ${message(error)}`)
    }
  }

  async #mutate(kind: 'edit' | 'toggle' | 'run' | 'delete', id: string): Promise<void> {
    const task = this.#response?.tasks.find(item => item.id === id)
    if (!task || this.#pending.has(id)) return
    if (kind === 'edit') {
      this.#close()
      const flow = createTaskFlowState('update', task)
      this.#host.runTaskTurn(
        `/task edit ${task.name}`,
        updateTaskPrompt(task, this.#promptContext(task)),
        this.#extension(flow),
      )
      return
    }
    const generation = this.#generation
    const identity = this.#identityGeneration
    let queued = false
    this.#pending.set(id, kind === 'delete' ? 'Deleting…' : kind === 'toggle' ? 'Updating…' : 'Starting…')
    // Invalidate any list snapshot or detail started before this mutation.
    this.#revision++
    this.#detailRequest++
    this.#paint()
    let nextFocus: string | undefined
    try {
      if (kind === 'toggle') {
        const updated = await this.#api.update(task.id, { revision: task.revision, enabled: !task.enabled }, this.#host.envFile)
        if (identity !== this.#identityGeneration) return
        if (this.#response) this.#response = {
          ...this.#response, tasks: this.#response.tasks.map(row => row.id === id ? updated.task : row),
        }
      } else if (kind === 'run') {
        if (!(await this.#confirmRun(task))) return
        if (this.#disposed || this.#host.destroyed() || generation !== this.#generation) return
        await this.#api.run(task.id)
        // A run lives only on the server; refetch so the row turns Queued
        // instead of sitting on the stale last run until the next poll.
        this.#loadedAt = 0
        queued = true
      } else {
        await this.#api.delete(task.id)
        if (identity !== this.#identityGeneration) return
        if (this.#response) {
          const rows = this.#response.tasks
          const at = rows.findIndex(row => row.id === id)
          const state = this.#host.taskOverlayState()
          if (state?.items[state.focusIndex]?.id === id) nextFocus = rows[at + 1]?.id ?? rows[at - 1]?.id
          this.#response = { ...this.#response, tasks: rows.filter(row => row.id !== id) }
        }
      }
      if (this.#detail?.id === id) this.#detail = undefined
    } catch (error) {
      this.#loadedAt = 0
      const text = message(error)
      if (identity === this.#identityGeneration && !this.#disposed && !this.#host.destroyed()) {
        // The server allows one active run per task; pressing run while it is
        // queued or in flight is not a failure, it is just already committed.
        this.#host.notifyError(
          kind === 'run' && /busy|active run/i.test(text)
            ? `“${task.name}” already has a run in flight — wait for it to finish.`
            : `Task operation failed: ${text}. Check task status before retrying.`,
        )
      }
    } finally {
      if (identity !== this.#identityGeneration) return
      this.#revision++
      this.#pending.delete(id)
      if (queued) void this.#refresh()
      // Update any currently visible list from cache, but only the original
      // run confirmation may restore its temporarily hidden window. That
      // confirm overlay replaces the list, so the selector can no longer
      // report the selection — restore the acted-on row by id.
      this.#paint(kind === 'run' ? id : nextFocus, kind === 'run' && generation === this.#generation)
    }
  }

  async #confirmRun(task: ScheduledTask): Promise<boolean> {
    const destination = task.delivery_channel
      ? `\nResult: ${task.delivery_channel} · ${task.delivery_target}`
      : ''
    const answers = await this.#host.collectAnswers([{
      header: 'Run task',
      question: `Run “${task.name}” now?${destination}`,
      options: [
        { label: 'Run now', description: 'Start one manual run with the saved configuration.' },
        { label: 'Cancel', description: 'Return without starting a run.' },
      ],
    }])
    return answers?.[0]?.answer === 'Run now'
  }

  /** One list request at a time. Mutations fence older snapshots so a late
   * pre-delete response cannot resurrect a removed row. No persistent cache. */
  #refresh(): Promise<void> {
    if (this.#listRequest) return this.#listRequest
    if (this.#pending.size || this.#disposed) return Promise.resolve()
    const revision = this.#revision
    this.#loadError = false
    const request = Promise.resolve().then(async () => {
      try {
        const response = await this.#api.list()
        if (this.#disposed || this.#host.destroyed() || revision !== this.#revision) return
        this.#response = response
        this.#loadedAt = Date.now()
        this.#detail = undefined
      } catch (error) {
        if (this.#disposed || this.#host.destroyed() || revision !== this.#revision) return
        this.#loadError = true
        if (this.#host.isTaskOverlay()) this.#host.notifyError(`Failed to load tasks: ${message(error)}`)
      } finally {
        if (this.#listRequest === request) {
          this.#listRequest = null
          this.#paint()
        }
      }
    })
    this.#listRequest = request
    this.#paint()
    return request
  }

  #paint(focusId?: string, open = false): void {
    if (this.#disposed || this.#host.destroyed() || (!open && !this.#host.isTaskOverlay())) return
    const current = this.#host.taskOverlayState()
    const focus = focusId ?? (current ? current.items[current.focusIndex]?.id : undefined)
    const response = this.#response ?? { tasks: [], cache: { ready: false, synced_at: 0, stale: false } }
    const state = createTaskWindow(response, focus, this.#detail, this.#modelLabels())
    if (this.#pending.size) state.subtitle = [...this.#pending.values()].join(' · ')
    else if (this.#listRequest) state.subtitle = this.#response ? 'Refreshing…' : 'Loading tasks…'
    else if (this.#loadError) state.subtitle = 'Could not refresh · reopen /task to retry'
    if (!this.#response) state.emptyMessage = this.#loadError ? 'Could not load tasks' : 'Loading tasks…'
    // Refreshing should not disarm a deliberate first `d` or reset scrolling.
    if (current) {
      state.scrollOffset = current.scrollOffset
      if (current.items[current.focusIndex]?.id === focus && current.previewPane) {
        state.previewPane = { ...current.previewPane }
      }
      const armed = current.pendingDeleteId
      if (armed && !this.#pending.has(armed) && state.items.some(row => row.id === armed)) {
        state.pendingDeleteId = armed
        state.subtitle = current.subtitle
      }
    }
    for (const row of state.allItems) {
      if (row.id && this.#pending.has(row.id)) {
        row.detail = this.#pending.get(row.id)
        row.pendingAction = true
      }
    }
    const focusedId = state.items[state.focusIndex]?.id
    this.#host.showSelector(focusedId ? selectorFocusOn(state, row => row.id === focusedId) : state)
    this.#host.requestRender()
  }

  #close(): void {
    this.invalidate()
    this.#host.closeOverlay()
  }

  #extension(flow: ReturnType<typeof createTaskFlowState>): HostToolExtension {
    this.#loadedAt = 0
    this.#revision++
    this.cancelSetup()
    this.#setup = new AbortController()
    const signal = this.#setup.signal
    return createTaskExtension({
      flow,
      ensureDelivery: () => this.#host.ensureDelivery(signal),
      defaults: () => this.#modelDefaults(),
      pickModel: request => this.#host.presentModelPicker(request),
      collectAnswers: params => this.#host.collectAnswers(params.questions),
    })
  }

  /** Model catalog plus device delivery target, read when a Task tool actually
   *  fires rather than on every streamed event. */
  async #modelDefaults(): Promise<TaskModelDefaults> {
    const config = this.#host.configInfo()
    const activeSpec = this.#host.activeModelSpec()
    const { taskDeliveryDefaults } = await import('./client.js')
    const delivery = await taskDeliveryDefaults(this.#host.envFile)
    return {
      model_spec: activeSpec,
      thinking_level: config?.thinkingLevel ?? '',
      available_models: (config?.availableModels ?? []).map(model => ({
        spec: model.spec,
        model: model.model,
        label: this.#host.modelOptionLabel(model),
        group: model.group_label ?? model.provider,
        thinking_level: model.spec === activeSpec
          ? config?.thinkingLevel ?? model.thinking_level ?? ''
          : model.thinking_level ?? '',
      })),
      feishu_ready: delivery.feishu_ready,
      feishu_target: delivery.feishu_target,
      env_file: this.#host.envFile,
    }
  }

  #modelLabels(): Record<string, string> {
    return Object.fromEntries(
      (this.#host.configInfo()?.availableModels ?? [])
        .map(option => [option.spec, this.#host.modelOptionLabel(option)]),
    )
  }

  #promptContext(task?: ScheduledTask): TaskPromptContext {
    const config = this.#host.configInfo()
    const activeSpec = this.#host.activeModelSpec()
    const active = config?.availableModels.find(model => model.spec === activeSpec)
    const saved = task?.model_policy === 'fixed'
      ? config?.availableModels.find(model => model.spec === task.model_spec)
      : undefined
    return {
      localTimezone: Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC',
      currentModel: active ? this.#host.modelOptionLabel(active) : this.#host.activeModel(),
      thinkingLevel: config?.thinkingLevel ?? '',
      availableModels: (config?.availableModels ?? []).map(this.#host.modelOptionLabel),
      savedModel: saved
        ? this.#host.modelOptionLabel(saved)
        : task?.model_spec
          ? task.model_spec.slice(task.model_spec.indexOf(':') + 1)
          : undefined,
    }
  }
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

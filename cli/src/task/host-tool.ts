/** Host-side handling of the `automation_task_*` tools.
 *
 *  The agent proposes; the host decides. Two things are never taken from tool
 *  arguments: the model (only the visible picker sets it) and the delivery
 *  target (only device config sets it). Everything else is confirmed by the user
 *  before it reaches the cloud.
 */

import {
  ASK_USER_SPEC,
  errorResponse,
  textResponse,
  type AskUserAnswer,
  type AskUserParams,
  type HostToolCall,
  type HostToolExtension,
  type HostToolResponse,
} from '../term/host-tools.js'
import { createTask, updateTask } from './client.js'
import {
  applySelection,
  pickerRequest,
  type TaskModelDefaults,
  type TaskModelPicker,
} from './model-picker.js'
import type { TaskFlowState } from './prompt.js'
import { BROADCAST_TARGET } from './types.js'

const MIN_TIMEOUT_SECONDS = 30
const MAX_TIMEOUT_SECONDS = 3600
const DEFAULT_TIMEOUT_SECONDS = 900
const DEFAULT_MAX_LATENESS_SECONDS = 14_400

const properties = {
  name: { type: 'string' },
  cron: { type: 'string', description: 'Five-field cron expression.' },
  timezone: { type: 'string', description: 'IANA timezone.' },
  instruction: { type: 'string' },
  model: {
    type: 'string',
    description: 'Optional human model name used only to preselect the host model picker. The user makes the final choice from the current model catalog.',
  },
  thinking_level: {
    type: 'string',
    enum: ['', 'off', 'minimal', 'low', 'medium', 'high', 'xhigh', 'max'],
    description: 'Optional preferred thinking level. The user makes the final choice in the host model picker.',
  },
  workspace_ref: { type: 'string' },
  delivery_channel: { type: 'string', enum: ['', 'feishu'] },
  delivery_target: {
    type: 'string',
    description: `Optional explicit Feishu chat ID. Usually omit it: the host uses the device's default notification chat. Use "${BROADCAST_TARGET}" only when the user explicitly asks to notify every 1:1 conversation the bot knows.`,
  },
  max_lateness_seconds: { type: 'integer', minimum: 0 },
  timeout_seconds: {
    type: 'integer',
    minimum: MIN_TIMEOUT_SECONDS,
    maximum: MAX_TIMEOUT_SECONDS,
    description: `Run timeout in seconds (${MIN_TIMEOUT_SECONDS}-${MAX_TIMEOUT_SECONDS}). Values outside this range are clamped by the runner.`,
  },
}

export const TASK_CREATE_SPEC = {
  name: 'automation_task_create',
  label: 'Create Task',
  description: "Create a recurring task with push delivery after final confirmation. The host guides Feishu setup when needed; never silently choose local-only delivery.",
  parameters_schema: {
    type: 'object',
    additionalProperties: false,
    properties,
    required: ['name', 'cron', 'timezone', 'instruction'],
  },
}

export const TASK_UPDATE_SPEC = {
  name: 'automation_task_update',
  label: 'Update Task',
  description: 'Update a recurring task after the user has confirmed the changes.',
  parameters_schema: {
    type: 'object',
    additionalProperties: false,
    properties: {
      task_id: { type: 'string' },
      revision: { type: 'integer' },
      ...properties,
      enabled: { type: 'boolean' },
    },
    required: ['task_id', 'revision'],
  },
}

export const TASK_HOST_TOOL_SPECS_JSON = JSON.stringify([
  ASK_USER_SPEC,
  TASK_CREATE_SPEC,
  TASK_UPDATE_SPEC,
])

export interface TaskToolContext {
  flow: TaskFlowState
  /** Resolved lazily so config/auth reads stay off the stream hot path. */
  defaults: () => Promise<TaskModelDefaults>
  pickModel: TaskModelPicker
  ensureDelivery?: () => Promise<boolean>
  collectAnswers: (params: AskUserParams) => Promise<AskUserAnswer[] | null>
}

function has(source: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(source, key)
}

function text(value: unknown): string {
  return String(value ?? '').trim()
}

/** Resolve where a task's result goes.
 *
 *  `feishu_ready` and `feishu_target` both come from device config via the
 *  addon, so this reads values rather than inventing a policy. "Not connected"
 *  and "connected but no default chat" are separate problems with separate
 *  fixes, and saying the wrong one sends the user to the wrong screen. */
function resolveDelivery(
  arguments_: Record<string, unknown>,
  flow: TaskFlowState,
  defaults?: TaskModelDefaults,
): Record<string, unknown> {
  const channel = text(arguments_.delivery_channel)
  const target = text(arguments_.delivery_target)
  const deviceTarget = text(defaults?.feishu_target)

  if (channel === 'feishu' || flow.mode === 'create') {
    if (channel && channel !== 'feishu') throw new Error(`Unsupported delivery channel: ${channel}`)
    const resolved = target || deviceTarget
    if (!resolved) {
      throw new Error(
        defaults?.feishu_ready
          ? 'Feishu is connected but has no default notification chat ID. Set one in settings, or give this task an explicit chat ID.'
          : 'Feishu is not connected on this device, so this task cannot deliver there. Connect it in settings, complete setup before creating the task.',
      )
    }
    return { delivery_channel: 'feishu', delivery_target: resolved }
  }
  if (has(arguments_, 'delivery_channel')) {
    if (channel) throw new Error(`Unsupported delivery channel: ${channel}`)
    return { delivery_channel: '', delivery_target: '' }
  }
  return {}
}

/** Strip everything the agent may not decide, then resolve delivery.
 *
 *  Model fields are always dropped: on a model-choosing turn the picker writes
 *  them afterwards, and otherwise the stored task keeps its own model. */
export function normalizeTaskArguments(
  arguments_: Record<string, unknown>,
  flow: TaskFlowState,
  defaults?: TaskModelDefaults,
): Record<string, unknown> {
  const allowed = new Set(Object.keys(flow.mode === 'create'
    ? TASK_CREATE_SPEC.parameters_schema.properties
    : TASK_UPDATE_SPEC.parameters_schema.properties))
  const patch = Object.fromEntries(Object.entries(arguments_).filter(([key]) => allowed.has(key)))
  if (flow.mode === 'update') {
    const current = flow.currentTask
    if (!current) throw new Error('No task is bound to this edit flow.')
    if ((has(arguments_, 'task_id') && arguments_.task_id !== current.id)
      || (has(arguments_, 'revision') && arguments_.revision !== current.revision)) {
      throw new Error('Task identity or revision does not match the confirmed edit flow.')
    }
    patch.task_id = current.id
    patch.revision = current.revision
  }
  delete patch.model
  delete patch.thinking_level
  return { ...patch, ...resolveDelivery(arguments_, flow, defaults) }
}

function modelLabel(spec: unknown, defaults?: TaskModelDefaults): string {
  const value = String(spec ?? '')
  const configured = defaults?.available_models?.find(model => model.spec === value)
  if (configured) return configured.label
  const model = value.includes(':') ? value.slice(value.indexOf(':') + 1) : value
  return model || 'Unavailable model'
}

function modelSetting(
  policy: unknown,
  spec: unknown,
  thinkingLevel: unknown,
  defaults?: TaskModelDefaults,
): string {
  if (policy === 'default') return 'Device default at run time'
  const label = modelLabel(spec || defaults?.model_spec, defaults)
  const thinking = text(thinkingLevel || defaults?.thinking_level)
  return thinking ? `${label} · ${thinking}` : label
}

/** Name the destination concretely. A user confirming delivery should see the
 *  chat it lands in, not a vague "Feishu". */
function deliveryLabel(channel: unknown, target: unknown): string {
  if (channel !== 'feishu') return 'Local result only'
  const chat = text(target)
  if (chat === BROADCAST_TARGET) return 'Feishu · all bot direct conversations'
  return chat ? `Feishu · ${chat}` : 'Feishu · unavailable'
}

/** Same as `deliveryLabel`, but a new task that silently lands local-only says
 *  why. The user is approving a schedule they will not hear from otherwise. */
function createDeliveryLabel(
  patch: Record<string, unknown>,
  defaults?: TaskModelDefaults,
): string {
  if (patch.delivery_channel === 'feishu') {
    return deliveryLabel(patch.delivery_channel, patch.delivery_target)
  }
  if (!defaults?.feishu_ready) return 'Local result only · Feishu not connected'
  if (!text(defaults.feishu_target)) {
    return 'Local result only · no default Feishu notification chat'
  }
  return 'Local result only'
}

function createConfirmationFields(
  patch: Record<string, unknown>,
  defaults?: TaskModelDefaults,
): string[] {
  const fields: Record<string, unknown> = {
    name: patch.name,
    schedule: `${String(patch.cron ?? '')} · ${String(patch.timezone ?? '')}`,
    instruction: patch.instruction,
    model: modelSetting(patch.model_policy ?? 'fixed', patch.model_spec, patch.thinking_level, defaults),
    workspace_ref: patch.workspace_ref || 'Default workspace',
    delivery: createDeliveryLabel(patch, defaults),
    enabled: patch.enabled ?? true,
    timeout_seconds: patch.timeout_seconds ?? DEFAULT_TIMEOUT_SECONDS,
    max_lateness_seconds: patch.max_lateness_seconds ?? DEFAULT_MAX_LATENESS_SECONDS,
  }
  return Object.entries(fields)
    .filter(([, value]) => value !== undefined && value !== '')
    .map(([key, value]) => `${key}: ${String(value)}`)
}

/** Old → new for the fields this request actually touches, so a confirmation
 *  never restates configuration the user did not ask to change. */
function taskUpdateChanges(
  patch: Record<string, unknown>,
  flow: TaskFlowState,
  defaults?: TaskModelDefaults,
): string[] {
  const current = flow.currentTask
  if (!current) return []
  const merged = { ...current, ...patch }
  const changes: string[] = []
  const touched = (...keys: string[]) => keys.some(key => has(patch, key))
  const add = (label: string, before: unknown, after: unknown) => {
    const left = String(before ?? '')
    const right = String(after ?? '')
    if (left !== right) changes.push(`${label}: ${left || '—'} → ${right || '—'}`)
  }

  if (touched('name')) add('name', current.name, merged.name)
  if (touched('cron', 'timezone')) {
    add('schedule', `${current.cron} · ${current.timezone}`, `${merged.cron} · ${merged.timezone}`)
  }
  if (touched('instruction')) add('instruction', current.instruction, merged.instruction)
  if (touched('model_policy', 'model_spec', 'thinking_level')) {
    add(
      'model',
      modelSetting(current.model_policy, current.model_spec, current.thinking_level, defaults),
      modelSetting(merged.model_policy, merged.model_spec, merged.thinking_level, defaults),
    )
  }
  if (touched('workspace_ref')) {
    add(
      'workspace',
      current.workspace_ref || 'Default workspace',
      merged.workspace_ref || 'Default workspace',
    )
  }
  if (touched('delivery_channel', 'delivery_target')) {
    add(
      'delivery',
      deliveryLabel(current.delivery_channel, current.delivery_target),
      deliveryLabel(merged.delivery_channel, merged.delivery_target),
    )
  }
  if (touched('enabled')) {
    add('status', current.enabled ? 'Enabled' : 'Paused', merged.enabled ? 'Enabled' : 'Paused')
  }
  if (touched('timeout_seconds')) add('timeout', `${current.timeout_seconds}s`, `${merged.timeout_seconds}s`)
  if (touched('max_lateness_seconds')) {
    add('lateness', `${current.max_lateness_seconds}s`, `${merged.max_lateness_seconds}s`)
  }
  return changes
}

/** Register the task tools with the generic host-tool dispatcher. */
export function createTaskExtension(context: TaskToolContext): HostToolExtension {
  return {
    specsJson: TASK_HOST_TOOL_SPECS_JSON,
    handles: name => name === TASK_CREATE_SPEC.name || name === TASK_UPDATE_SPEC.name,
    dispatch: call => dispatchTaskToolCall(call, context),
  }
}

async function dispatchTaskToolCall(
  call: HostToolCall,
  context: TaskToolContext,
): Promise<HostToolResponse> {
  const { flow } = context
  try {
    const creating = call.tool_name === TASK_CREATE_SPEC.name
    if ((creating ? 'create' : 'update') !== flow.mode) {
      return errorResponse(call, `This Task flow only allows ${flow.mode}.`)
    }
    // One confirmed mutation per flow. Retrying a create the user already saw
    // would silently duplicate a schedule.
    if (flow.mutationAttempted) {
      const detail = flow.mutationError ? `: ${flow.mutationError}` : ''
      return textResponse(
        call,
        `This Task flow already attempted its confirmed change${detail}. Do not retry automatically; ask the user to start a new /task action.`,
      )
    }

    let defaults: TaskModelDefaults
    try {
      defaults = await context.defaults()
    } catch (error) {
      if (!context.ensureDelivery) throw error
      if (!(await context.ensureDelivery())) {
        flow.mutationAttempted = true
        flow.mutationError = 'delivery setup cancelled'
        return textResponse(call, 'Task change cancelled during setup. Nothing was submitted. Do not retry automatically.')
      }
      defaults = await context.defaults()
    }
    const needsDelivery = creating || call.arguments.delivery_channel === 'feishu'
    if (needsDelivery && (!defaults.feishu_ready
      || !(text(call.arguments.delivery_target) || text(defaults.feishu_target)))) {
      if (!context.ensureDelivery) throw new Error('Feishu setup is required before creating this task.')
      if (!(await context.ensureDelivery())) {
        flow.mutationAttempted = true
        flow.mutationError = 'delivery setup cancelled'
        return textResponse(call, 'Task creation cancelled during delivery setup. Nothing was created. Do not retry automatically.')
      }
      defaults = await context.defaults()
      if (!defaults.feishu_ready) throw new Error('Feishu setup is not complete. Nothing was created.')
    }
    let patch = normalizeTaskArguments(call.arguments, flow, defaults)
    let effective = defaults
    // Creating always chooses a model. Editing does only when the user's request
    // mentioned one, so an unrelated edit never reopens the picker.
    const choosesModel = creating || has(call.arguments, 'model') || has(call.arguments, 'thinking_level')
    if (choosesModel) {
      const selection = await context.pickModel(pickerRequest(flow, defaults, call.arguments.model))
      if (!selection) {
        flow.mutationAttempted = true
        flow.mutationError = 'model selection cancelled by user'
        return textResponse(
          call,
          'Task change cancelled during model selection. Do not retry unless asked.',
        )
      }
      const applied = applySelection(patch, selection, defaults)
      patch = applied.patch
      effective = applied.taskModel ?? defaults
    }

    const changes = creating ? [] : taskUpdateChanges(patch, flow, effective)
    const fields = creating ? createConfirmationFields(patch, effective) : changes
    const answers = await context.collectAnswers({
      questions: [{
        header: 'Task',
        question: creating
          ? `Create this scheduled task?\n${fields.join('\n')}`
          : `Apply these changes to “${flow.currentTask?.name ?? 'Task'}”?\n${fields.join('\n') || 'No effective changes'}`,
        options: [
          {
            label: 'Confirm',
            description: `${creating ? 'Create' : 'Update'} the task with these settings.`,
          },
          { label: 'Cancel', description: 'Keep the current task configuration.' },
        ],
      }],
    })
    if (answers?.[0]?.answer !== 'Confirm') {
      flow.mutationAttempted = true
      flow.mutationError = 'cancelled by user'
      return textResponse(call, 'Task change cancelled by the user. Do not retry unless asked.')
    }

    flow.mutationAttempted = true
    try {
      return textResponse(
        call,
        creating
          ? await create(patch, flow.requestId ??= crypto.randomUUID(), effective)
          : await update(patch, changes, effective),
      )
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error)
      flow.mutationError = message
      return errorResponse(
        call,
        `${message}. The save outcome is unknown. Check /task before starting another change; do not retry automatically.`,
      )
    }
  } catch (error) {
    flow.mutationAttempted = true
    flow.mutationError = error instanceof Error ? error.message : String(error)
    return errorResponse(call, `${flow.mutationError} Do not retry automatically.`)
  }
}

async function create(
  patch: Record<string, unknown>,
  idempotencyKey: string,
  defaults: TaskModelDefaults,
): Promise<string> {
  const created = await createTask(
    { ...patch, idempotency_key: idempotencyKey },
    defaults.env_file,
  )
  const next = created.next_runs[0] ? new Date(created.next_runs[0]).toLocaleString() : 'paused'
  return `Created ${created.task.name}. Next run: ${next}.`
}

async function update(
  patch: Record<string, unknown>,
  changes: string[],
  defaults: TaskModelDefaults,
): Promise<string> {
  const { task_id, ...body } = patch
  const updated = await updateTask(String(task_id), body, defaults.env_file)
  return changes.length > 0
    ? `Updated ${updated.task.name}: ${changes.join('; ')}.`
    : `Updated ${updated.task.name}: no effective changes.`
}

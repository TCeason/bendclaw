import { createAppSelectorState } from '../term/app/selector-identity.js'
import type { SelectorItem, SelectorState } from '../term/selector.js'
import type { ScheduledTask, TaskListResponse, TaskRunSummary, TaskStats } from './types.js'

const hints = [
  { keys: ['up', 'down'], action: 'select' },
  { keys: 'tab', action: 'details' },
  { keys: 'e', action: 'edit' },
  { keys: 'r', action: 'run now' },
  { keys: 's', action: 'share' },
  { keys: 'd', action: 'delete' },
  { keys: 'escape', action: 'close' },
]

const emptyStats: TaskStats = {
  window_days: 30,
  runs: 0,
  completed: 0,
  succeeded: 0,
  execution_success_rate: null,
  delivery_attempted: 0,
  delivery_sent: 0,
  delivery_success_rate: null,
}

function dateTime(value: number): string {
  if (!value) return '—'
  return new Intl.DateTimeFormat(undefined, {
    month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit',
  }).format(new Date(value))
}

function relativeTime(value: number): string {
  if (!value) return '—'
  const seconds = Math.max(0, Math.floor((Date.now() - value) / 1000))
  if (seconds < 60) return 'just now'
  const minutes = Math.floor(seconds / 60)
  if (minutes < 60) return `${minutes}m ago`
  const hours = Math.floor(minutes / 60)
  if (hours < 24) return `${hours}h ago`
  return `${Math.floor(hours / 24)}d ago`
}

function schedule(task: ScheduledTask): string {
  const [minute, hour, day, month, weekday] = task.cron.split(' ')
  const clock = minute !== undefined && hour !== undefined
    && /^\d+$/.test(minute) && /^\d+$/.test(hour)
    ? `${hour.padStart(2, '0')}:${minute.padStart(2, '0')}`
    : ''
  if (day === '*' && month === '*' && weekday === '1-5' && clock) return `Weekdays ${clock}`
  if (day === '*' && month === '*' && weekday === '*' && clock) return `Daily ${clock}`
  if (minute === '0' && hour === '*' && day === '*' && month === '*' && weekday === '*') return 'Hourly'
  if (minute === '*' && hour === '*' && day === '*' && month === '*' && weekday === '*') return 'Every minute'
  const interval = minute?.match(/^\*\/(\d+)$/)?.[1]
  if (interval && hour === '*' && day === '*' && month === '*' && weekday === '*') return `Every ${interval}m`
  return task.cron
}

function status(run: TaskRunSummary | null | undefined): string {
  if (!run) return 'Never run'
  switch (run.status) {
    case 'succeeded': return run.delivery_status === 'failed' ? 'Delivery failed' : 'Succeeded'
    case 'failed': return 'Failed'
    case 'needs_attention': return 'Needs attention'
    case 'running':
    case 'claimed': return 'Running'
    case 'pending': return 'Queued'
    case 'unknown': return 'Unknown'
    case 'expired': return 'Expired'
    case 'cancelled': return 'Cancelled'
    default: return run.status
  }
}

function statusIcon(run: TaskRunSummary): string {
  if (run.status === 'succeeded' && run.delivery_status !== 'failed') return '✓'
  if (run.status === 'running' || run.status === 'claimed' || run.status === 'pending') return '◷'
  if (run.status === 'cancelled') return '–'
  return '✗'
}

/** Minutes a run has occupied its current state, when that is worth saying. */
function stateAge(run: TaskRunSummary): string {
  const since = run.updated_at || run.scheduled_for
  if (!since) return ''
  const minutes = Math.floor((Date.now() - since) / 60_000)
  return minutes >= 1 ? ` ${minutes}m` : ''
}

function taskState(task: ScheduledTask): string {
  if (!task.enabled) return 'Paused'
  const latest = task.last_run
  if (!latest) return 'Ready'
  if (latest.status === 'running' || latest.status === 'claimed') return `Running${stateAge(latest)}`
  if (latest.status === 'pending') return `Queued${stateAge(latest)}`
  if (latest.status === 'failed' || latest.status === 'needs_attention') return 'Attention'
  return 'On'
}

export type TaskModelLabels = Readonly<Record<string, string>>

function model(task: ScheduledTask, labels: TaskModelLabels = {}): string {
  if (task.model_policy === 'default') return 'Device default at run time'
  const fallback = task.model_spec.includes(':')
    ? task.model_spec.slice(task.model_spec.indexOf(':') + 1)
    : task.model_spec
  const name = labels[task.model_spec]?.trim() || fallback || 'Unavailable model'
  return `${name}${task.thinking_level ? ` · ${task.thinking_level}` : ''}`
}

function recentRun(run: TaskRunSummary): string {
  const at = run.updated_at || run.scheduled_for
  const details = [
    run.source === 'manual' ? 'manual' : '',
    // Dispatchers poll every few seconds, so a run still queued a minute later
    // has no live owner: name that instead of looking merely busy.
    run.status === 'pending' && run.scheduled_for && Date.now() - run.scheduled_for > 60_000
      ? 'awaiting an executor'
      : '',
    run.delivery_status === 'sent'
      ? 'sent'
      : run.delivery_status === 'failed'
        ? 'delivery failed'
        : '',
    run.error ? run.error.replace(/\s+/g, ' ').slice(0, 48) : '',
  ].filter(Boolean)
  return `${statusIcon(run)} ${relativeTime(at)}  ${status(run)}${details.length ? ` · ${details.join(' · ')}` : ''}`
}

function preview(
  task: ScheduledTask,
  allRuns?: TaskRunSummary[],
  modelLabels: TaskModelLabels = {},
): string[] {
  const stats = task.stats ?? emptyStats
  const runs = allRuns ?? task.recent_runs ?? []
  const history = runs.length > 0 ? runs.map(recentRun) : ['No runs yet']
  const delivery = !task.delivery_channel ? 'Not configured'
    : `${task.delivery_channel} · ${task.delivery_target === 'p2p:*' ? 'All bot direct conversations' : task.delivery_target}`
  return [
    task.name,
    `Model  ${model(task, modelLabels)}`,
    `Schedule  ${task.cron} · ${task.timezone}`,
    `Next  ${task.enabled ? dateTime(task.next_run_at) : 'Paused'}`,
    `Delivery  ${delivery}`,
    '',
    '# Instructions',
    ...task.instruction.split('\n'),
    '',
    '# Activity',
    `${stats.runs} runs · ${stats.succeeded} succeeded · last ${stats.window_days || 30} days`,
    `Workspace  ${task.workspace_ref || 'Default workspace'}`,
    `Timeout  ${task.timeout_seconds}s · Max lateness ${task.max_lateness_seconds}s`,
    '',
    '# Recent runs',
    ...history,
  ]
}

function item(
  task: ScheduledTask,
  allRuns?: TaskRunSummary[],
  modelLabels: TaskModelLabels = {},
): SelectorItem {
  const stats = task.stats ?? emptyStats
  const taskModel = model(task, modelLabels)
  return {
    id: task.id,
    label: task.name,
    detail: [
      schedule(task),
      taskState(task),
      `${stats.runs} runs · ${stats.succeeded} succeeded`,
    ].join('  ·  '),
    searchText: `${task.name} ${task.instruction} ${task.cron} ${task.timezone} ${taskModel}`,
    preview: preview(task, allRuns, modelLabels),
    hints,
  }
}

export function createTaskWindow(
  response: TaskListResponse,
  focusId?: string,
  detailTask?: ScheduledTask,
  modelLabels: TaskModelLabels = {},
): SelectorState {
  const items = response.tasks.map(task => {
    // List fields remain authoritative; only same-revision history is cached.
    const detailed = detailTask?.id === task.id && detailTask.revision === task.revision
      ? { ...task, runs: detailTask.runs } : task
    return item(detailed, detailed.runs, modelLabels)
  })
  const index = focusId ? items.findIndex(row => row.id === focusId) : 0
  return {
    ...createAppSelectorState('task', 'Tasks', items),
    focusIndex: index >= 0 ? index : 0,
    noFilter: true,
    previewPane: { fraction: 0.55, offset: 0, confirmDeleteKey: 'd' },
    listFocused: true,
    lowercaseHints: true,
    hints: items.length ? hints : [{ keys: 'n', action: 'new' }, { keys: 'escape', action: 'close' }],
    subtitle: response.cache.stale
      ? 'Data may be stale'
      : `${items.length} task${items.length === 1 ? '' : 's'}`,
    emptyMessage: 'No scheduled tasks · n to create',
  }
}

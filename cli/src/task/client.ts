import {
  taskCreate as nativeCreate,
  taskDelete as nativeDelete,
  taskDeliveryDefaults as nativeDeliveryDefaults,
  taskGet as nativeGet,
  taskList as nativeList,
  taskRun as nativeRun,
  taskUpdate as nativeUpdate,
} from '../native/index.js'
import type {
  CreatedTaskResponse,
  ScheduledTask,
  TaskDeliveryDefaults,
  TaskListResponse,
  TaskRunSummary,
} from './types.js'

export function decodeTaskNativeResult<T>(raw: unknown, operation: string): T {
  if (raw instanceof Error) throw raw
  if (typeof raw !== 'string') {
    const message = raw && typeof raw === 'object' && 'message' in raw
      ? String((raw as { message: unknown }).message)
      : ''
    throw new Error(message || `${operation}: invalid native result`)
  }
  try {
    return JSON.parse(raw) as T
  } catch (error) {
    const text = raw.trim()
    if (/^(?:Error|NapiError|GenericFailure)\b/i.test(text)) throw new Error(text)
    throw new Error(`${operation}: invalid native result`, { cause: error })
  }
}

async function nativeJson<T>(operation: string, result: Promise<unknown>): Promise<T> {
  return decodeTaskNativeResult<T>(await result, operation)
}

export async function taskDeliveryDefaults(envFile?: string): Promise<TaskDeliveryDefaults> {
  return nativeJson('Resolve task delivery', nativeDeliveryDefaults(envFile ?? null))
}

export async function listTasks(): Promise<TaskListResponse> {
  return nativeJson('List tasks', nativeList())
}

export async function getTask(id: string): Promise<ScheduledTask & { runs: TaskRunSummary[] }> {
  return nativeJson('Get task', nativeGet(id))
}

export async function createTask(
  input: Record<string, unknown>,
  envFile?: string,
): Promise<CreatedTaskResponse> {
  return nativeJson('Create task', nativeCreate(JSON.stringify(input), envFile ?? null))
}

export async function updateTask(
  id: string,
  input: Record<string, unknown>,
  envFile?: string,
): Promise<CreatedTaskResponse> {
  return nativeJson('Update task', nativeUpdate(id, JSON.stringify(input), envFile ?? null))
}

export async function deleteTask(id: string): Promise<void> {
  await nativeDelete(id)
}

export async function runTask(id: string): Promise<void> {
  await nativeRun(id, crypto.randomUUID())
}

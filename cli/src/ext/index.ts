/**
 * Extension system entry point.
 *
 * {@link createExtensionHost} assembles the built-in extensions (ask_user,
 * plan-artifact) into a ready {@link ExtensionHost}. User extensions would be
 * loaded and registered here too — the host treats built-ins and user
 * extensions identically.
 */

import { createAskUserTool, type AskUserAnswer, type AskUserParams } from './builtin/ask-user/index.js'
import { createPlanTool } from './builtin/plan-artifact/index.js'
import { ExtensionHost } from './host.js'

export { ExtensionHost } from './host.js'
export type {
  ExtensionUI,
  HostTool,
  HostToolCallEvent,
  HostToolResponsePayload,
  HostToolResult,
  HostToolSpec,
  PlanReviewRequest,
  PlanReviewResult,
  PlanTask,
  PlanTaskStatus,
} from './types.js'
export type { AskUserAnswer, AskUserParams } from './builtin/ask-user/index.js'
export type { PlanDetails } from './builtin/plan-artifact/index.js'
export { footerLabel, tasksFromDetails } from './builtin/plan-artifact/state.js'

/** Hooks the REPL provides so built-in tools can drive interactive UI. */
export interface ExtensionHooks {
  /** Present ask_user questions and collect answers (null = cancelled). */
  collectAnswers: (params: AskUserParams) => Promise<AskUserAnswer[] | null>
}

/** Build the extension host with all built-in tools registered. */
export function createExtensionHost(hooks: ExtensionHooks): ExtensionHost {
  const host = new ExtensionHost()
  host.register(createAskUserTool(hooks.collectAnswers))
  host.register(createPlanTool())
  return host
}

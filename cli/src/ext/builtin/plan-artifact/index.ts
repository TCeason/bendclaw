/**
 * plan-artifact builtin extension — implements issue #39.
 *
 * Provides a `plan` host tool the agent uses to:
 *  1. propose a structured execution plan (tasks with dependencies),
 *  2. get user review/approval before expensive execution,
 *  3. report live status (pending / in_progress / completed / failed) as it
 *     executes each task.
 *
 * State lives entirely in the tool result `details` (see {@link PlanDetails}),
 * so it reconstructs correctly across branching and resume — the CLI rebuilds
 * the artifact view by replaying the latest `plan` tool result. The engine
 * core holds none of this; it only delegates execution back here.
 */

import type { HostTool, HostToolResult, PlanTask, PlanTaskStatus } from '../../types.js'
import { PARAMETERS_SCHEMA, PLAN_DESCRIPTION } from './schema.js'
import { normalizeTasks, summarize, validatePlan } from './state.js'

export type PlanAction = 'propose' | 'update'

export interface PlanParams {
  action: PlanAction
  tasks: PlanTask[]
}

/** Structured plan snapshot emitted in the tool result details. The CLI's
 *  renderer reads `goal.tasks` (matching the existing goal-task renderer). */
export interface PlanDetails {
  action: PlanAction
  goal: { tasks: PlanTask[] }
  approved: boolean
}

/**
 * Build the plan host tool.
 *
 * `reviewPlan` is supplied via the tool context UI: on a `propose`, the plan is
 * presented for approval before execution proceeds.
 */
export function createPlanTool(): HostTool<PlanParams> {
  return {
    spec: {
      name: 'plan',
      label: 'Plan',
      description: PLAN_DESCRIPTION,
      parameters_schema: PARAMETERS_SCHEMA,
      name_aliases: [['claude', 'Plan']],
    },

    async execute(params, ctx): Promise<HostToolResult> {
      const validationError = validatePlan(params.tasks)
      if (validationError) {
        return {
          content: [{ type: 'text', text: `Invalid plan: ${validationError}` }],
          isError: true,
        }
      }

      const tasks = normalizeTasks(params.tasks)

      if (params.action === 'propose') {
        const review = await ctx.ui.reviewPlan({ tasks })
        if (review.kind === 'rejected') {
          const suffix = review.feedback ? `: ${review.feedback}` : '.'
          return {
            content: [
              {
                type: 'text',
                text: `User did not approve the plan${suffix} Revise it and propose again.`,
              },
            ],
            details: planDetails('propose', tasks, false),
            isError: true,
          }
        }
        return {
          content: [
            {
              type: 'text',
              text: `Plan approved (${tasks.length} tasks). Execute the tasks in dependency order, calling plan with action "update" to report status as you go.`,
            },
          ],
          details: planDetails('propose', tasks, true),
        }
      }

      // action === 'update': status report during execution.
      return {
        content: [{ type: 'text', text: summarize(tasks) }],
        details: planDetails('update', tasks, true),
      }
    },
  }
}

function planDetails(action: PlanAction, tasks: PlanTask[], approved: boolean): PlanDetails {
  return { action, goal: { tasks }, approved }
}

export type { PlanTask, PlanTaskStatus }

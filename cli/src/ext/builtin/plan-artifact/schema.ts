/** Static schema and description for the `plan` host tool. */

export const PLAN_DESCRIPTION = `Manage a structured execution plan for a multi-step task.

Use this tool for long-running work that involves several dependent steps
(data analysis, ETL, migrations, refactors). It gives the user a chance to
review the strategy before expensive execution, and shows live progress.

Actions:
- "propose": present the full plan for the user to review and approve BEFORE
  you start executing. Include every task with a stable id, a short title, and
  its dependencies. Execution should not begin until the plan is approved.
- "update": report status as you execute. Resend the full task list with each
  task's current status (pending, in_progress, completed, failed). Mark a task
  in_progress when you start it and completed/failed when it finishes.

Guidelines:
- Propose once, then update as you make progress. Keep task ids stable across
  calls so the artifact tracks correctly.
- Keep titles short and action-oriented. Put detail in your normal replies.`

export const PARAMETERS_SCHEMA = {
  type: 'object',
  properties: {
    action: {
      type: 'string',
      enum: ['propose', 'update'],
      description: 'propose = present for approval; update = report live status.',
    },
    tasks: {
      type: 'array',
      minItems: 1,
      description: 'The full task list. Always send every task, not just changes.',
      items: {
        type: 'object',
        properties: {
          id: { type: 'integer', description: 'Stable task id, unique within the plan.' },
          title: { type: 'string', description: 'Short, action-oriented task title.' },
          status: {
            type: 'string',
            enum: ['pending', 'in_progress', 'completed', 'failed'],
            description: 'Current task status.',
          },
          deps: {
            type: 'array',
            items: { type: 'integer' },
            description: 'Ids of tasks that must complete before this one.',
          },
        },
        required: ['id', 'title', 'status'],
      },
    },
  },
  required: ['action', 'tasks'],
}

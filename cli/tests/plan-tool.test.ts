import { describe, expect, test } from 'bun:test'
import { createPlanTool, type PlanDetails, type PlanParams } from '../src/ext/builtin/plan-artifact/index.js'
import type { ExtensionUI, HostToolContext, PlanReviewResult } from '../src/ext/types.js'

function ctx(review: PlanReviewResult): HostToolContext {
  const ui: ExtensionUI = {
    async reviewPlan() {
      return review
    },
  }
  return { toolCallId: 'c1', ui }
}

const tasks = [
  { id: 1, title: 'Load data', status: 'pending' as const },
  { id: 2, title: 'Transform', status: 'pending' as const, deps: [1] },
]

describe('plan tool', () => {
  test('propose returns approval and marks the plan approved', async () => {
    const tool = createPlanTool()
    const params: PlanParams = { action: 'propose', tasks }
    const result = await tool.execute(params, ctx({ kind: 'approved' }))
    expect(result.isError).toBeFalsy()
    const details = result.details as PlanDetails
    expect(details.approved).toBe(true)
    expect(details.goal.tasks).toHaveLength(2)
    expect(result.content[0].text).toContain('Plan approved')
  })

  test('rejected propose surfaces feedback as a tool error', async () => {
    const tool = createPlanTool()
    const params: PlanParams = { action: 'propose', tasks }
    const result = await tool.execute(
      params,
      ctx({ kind: 'rejected', feedback: 'split step 2' }),
    )
    expect(result.isError).toBe(true)
    expect(result.content[0].text).toContain('split step 2')
    const details = result.details as PlanDetails
    expect(details.approved).toBe(false)
  })

  test('invalid plan is rejected before review', async () => {
    const tool = createPlanTool()
    let reviewed = false
    const ui: ExtensionUI = {
      async reviewPlan() {
        reviewed = true
        return { kind: 'approved' }
      },
    }
    const params: PlanParams = {
      action: 'propose',
      tasks: [{ id: 1, title: 'a', status: 'pending', deps: [1] }],
    }
    const result = await tool.execute(params, { toolCallId: 'c1', ui })
    expect(result.isError).toBe(true)
    expect(result.content[0].text).toContain('Invalid plan')
    expect(reviewed).toBe(false)
  })

  test('update reports live status without review', async () => {
    const tool = createPlanTool()
    let reviewed = false
    const ui: ExtensionUI = {
      async reviewPlan() {
        reviewed = true
        return { kind: 'approved' }
      },
    }
    const params: PlanParams = {
      action: 'update',
      tasks: [
        { id: 1, title: 'Load data', status: 'completed' },
        { id: 2, title: 'Transform', status: 'in_progress', deps: [1] },
      ],
    }
    const result = await tool.execute(params, { toolCallId: 'c1', ui })
    expect(reviewed).toBe(false)
    expect(result.isError).toBeFalsy()
    const details = result.details as PlanDetails
    expect(details.action).toBe('update')
    expect(result.content[0].text).toContain('1/2 completed')
  })
})

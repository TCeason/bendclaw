import { describe, expect, test } from 'bun:test'
import { ExtensionHost } from '../src/ext/host.js'
import type { ExtensionUI, HostTool } from '../src/ext/types.js'

const noopUI: ExtensionUI = {
  async reviewPlan() {
    return { kind: 'approved' }
  },
}

function echoTool(name: string): HostTool {
  return {
    spec: {
      name,
      label: name,
      description: 'echo',
      parameters_schema: { type: 'object' },
      name_aliases: [['claude', name.charAt(0).toUpperCase() + name.slice(1)]],
    },
    async execute(params) {
      return { content: [{ type: 'text', text: JSON.stringify(params) }] }
    },
  }
}

describe('ExtensionHost', () => {
  test('empty host advertises no specs', () => {
    const host = new ExtensionHost()
    expect(host.isEmpty).toBe(true)
    expect(host.specsJson()).toBeUndefined()
  })

  test('specsJson serializes registered specs', () => {
    const host = new ExtensionHost()
    host.register(echoTool('plan'))
    const specs = JSON.parse(host.specsJson()!)
    expect(specs).toHaveLength(1)
    expect(specs[0].name).toBe('plan')
  })

  test('dispatch routes to the matching tool and returns its result', async () => {
    const host = new ExtensionHost()
    host.register(echoTool('plan'))
    const resp = await host.dispatch(
      { tool_name: 'plan', tool_call_id: 'c1', arguments: { a: 1 } },
      noopUI,
    )
    expect(resp.tool_call_id).toBe('c1')
    expect(resp.is_error).toBe(false)
    expect(resp.content[0]).toEqual({ type: 'text', text: '{"a":1}' })
  })

  test('dispatch resolves aliased tool names', async () => {
    const host = new ExtensionHost()
    host.register(echoTool('plan'))
    const resp = await host.dispatch(
      { tool_name: 'Plan', tool_call_id: 'c2', arguments: {} },
      noopUI,
    )
    expect(resp.is_error).toBe(false)
  })

  test('unknown tool yields an error result, never throws', async () => {
    const host = new ExtensionHost()
    const resp = await host.dispatch(
      { tool_name: 'nope', tool_call_id: 'c3', arguments: {} },
      noopUI,
    )
    expect(resp.is_error).toBe(true)
    expect(resp.content[0].text).toContain('Unknown host tool')
  })

  test('a throwing tool is caught and surfaced as an error result', async () => {
    const host = new ExtensionHost()
    host.register({
      spec: { name: 'boom', label: 'boom', description: '', parameters_schema: {} },
      async execute() {
        throw new Error('kaboom')
      },
    })
    const resp = await host.dispatch(
      { tool_name: 'boom', tool_call_id: 'c4', arguments: {} },
      noopUI,
    )
    expect(resp.is_error).toBe(true)
    expect(resp.content[0].text).toBe('kaboom')
  })

  test('later registration with same name wins', () => {
    const host = new ExtensionHost()
    host.register(echoTool('plan'))
    host.register({ ...echoTool('plan'), spec: { ...echoTool('plan').spec, label: 'v2' } })
    expect(host.specs()).toHaveLength(1)
    expect(host.specs()[0].label).toBe('v2')
  })
})

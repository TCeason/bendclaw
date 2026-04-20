import { describe, expect, test } from 'bun:test'
import { handleSlashCommand } from '../src/term/app/commands.js'
import { createInitialState } from '../src/term/app/state.js'

describe('term commands', () => {
  const mkCtx = () => ({
    agent: { model: 'claude-3-5-sonnet' } as any,
    appState: createInitialState('claude-3-5-sonnet', '/tmp'),
    configInfo: { availableModels: ['m1', 'm2'] } as any,
    preloadedSessions: [
      { session_id: 'abc12345', title: 'session one', source: 'local' },
      { session_id: 'def67890', title: 'session two' },
    ] as any,
    planning: false,
  })

  test('/help opens overlay', () => {
    const result = handleSlashCommand('/help', mkCtx())
    expect(result.overlay?.kind).toBe('help')
  })

  test('/verbose toggles flag', () => {
    const result = handleSlashCommand('/verbose', mkCtx())
    expect(result.appState.verbose).toBe(false)
  })

  test('/plan toggles planning', () => {
    const result = handleSlashCommand('/plan', mkCtx())
    expect(result.planning).toBe(true)
  })

  test('/model updates model', () => {
    const ctx = mkCtx()
    const result = handleSlashCommand('/model m2', ctx)
    expect(result.appState.model).toBe('m2')
    expect(ctx.agent.model).toBe('m2')
  })

  test('/model without arg returns empty result for selector', () => {
    const result = handleSlashCommand('/model', mkCtx())
    expect(result.systemLines.length).toBe(0)
  })

  test('/resume resolves session by prefix', () => {
    const result = handleSlashCommand('/resume abc', mkCtx())
    expect(result.resumeSession?.session_id).toBe('abc12345')
  })

  test('/resume with no arg returns empty result for selector', () => {
    const ctx = mkCtx()
    const result = handleSlashCommand('/resume', ctx)
    expect(result.systemLines.length).toBe(0)
  })

  test('unknown command returns system message', () => {
    const result = handleSlashCommand('/wat', mkCtx())
    expect(result.systemLines[0]?.text).toContain('Unknown command')
  })
})

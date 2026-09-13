import { test, expect } from 'bun:test'
import { runShareCommand, type ShareContext } from '../src/commands/share.js'

function setup() {
  const output: string[] = []
  const calls: string[] = []
  const ctx: ShareContext = {
    agent: {
      listSessions: async () => [],
      shareSession: async sid => { calls.push(sid); return { id: 'abcdefghijklmnopqrstuv', url: 'https://evot.ai/share/abcdefghijklmnopqrstuv' } },

    },
    openShareList: async () => { calls.push('open-list') },
    getSessionId: () => 'session',
    commitSystem: (_, text) => output.push(text),
    flushShareNotices: async () => { calls.push('flush') },
    requestRender() {},
  }
  return { ctx, output, calls }
}

test('one command flushes notices and uploads immediately; returns a viewer URL', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, '')
  expect(calls).toEqual(['flush', 'session'])
  expect(output.at(-1)).toContain('https://evot.ai/share/')
  expect(output.join('')).not.toContain('password')
})

test('does not upload partial runs or import remote archives', async () => {
  const { ctx, output, calls } = setup()
  ctx.isBusy = () => true
  await runShareCommand(ctx, '')
  expect(output.at(-1)).toContain('Wait for')
  ctx.isBusy = () => false
  await runShareCommand(ctx, 'https://tmpfiles.org/old#key')
  expect(output.at(-1)).toContain('Usage:')
  expect(calls).toEqual([])
})

test('list opens the shared selector; textual rm is not supported', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'list')
  expect(calls).toEqual(['open-list'])
  expect(output).toEqual([])
  await runShareCommand(ctx, 'rm abcdefghijklmnopqrstuv')
  expect(output.at(-1)).toContain('Usage: /share [session-id | list]')
  expect(calls).toEqual(['open-list'])
})

test('failed notice persistence prevents incomplete upload', async () => {
  const { ctx, output, calls } = setup()
  ctx.flushShareNotices = async () => { throw new Error('notices unavailable') }
  await runShareCommand(ctx, '')
  expect(calls).toEqual([])
  expect(output.at(-1)).toContain('notices unavailable')
})

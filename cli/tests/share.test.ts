import { test, expect } from 'bun:test'
import { runShareCommand, type ShareContext } from '../src/commands/share.js'
import type { CloudPushResult, SessionMeta } from '../src/native/index.js'

function synced(visibility: 'private' | 'public', seq = 3): CloudPushResult {
  return {
    kind: 'synced', pushed: seq,
    cloud: { visibility, synced_seq: seq, synced_at: 't', origin_host: 'laptop',
      public_url: visibility === 'public' ? 'https://evot.ai/live/abcdefghijklmnopqrstuv' : null },
  }
}

function setup(result: (visibility: string) => CloudPushResult = v => synced(v === 'public' ? 'public' : 'private')) {
  const output: string[] = []
  const calls: string[] = []
  const ctx: ShareContext = {
    agent: {
      listSessions: async () => [{ session_id: 'abc12345-0000', cwd: '/w', model: 'm', turns: 1, created_at: '', updated_at: '' } as SessionMeta],
      cloudShareSession: async (sid, visibility) => { calls.push(`share:${sid}:${visibility}`); return result(visibility) },
      cloudUnshareSession: async sid => { calls.push(`off:${sid}`) },
      cloudPushSession: async (sid, force) => { calls.push(`push:${sid}:${force}`); return synced('private') },
      cloudForkRemoteSession: async sid => { calls.push(`fork:${sid}`); return { session_id: 'fork0000-1111', cwd: '/w', model: 'm', turns: 1, created_at: '', updated_at: '' } as SessionMeta },
    },
    openShareList: async () => { calls.push('open-list') },
    getSessionId: () => 'session',
    commitSystem: (_, text) => output.push(text),
    flushShareNotices: async () => { calls.push('flush') },
    requestRender() {},
    cloudAcknowledged: sid => { calls.push(`ack:${sid}`) },
    cloudForgotten: sid => { calls.push(`forget:${sid}`) },
    resumeSession: async session => { calls.push(`resume:${session.session_id}`) },
  }
  return { ctx, output, calls }
}

test('bare /share opens the list without mutating or requiring a current session', async () => {
  const { ctx, output, calls } = setup()
  ctx.getSessionId = () => null
  ctx.isBusy = () => true
  await runShareCommand(ctx, '')
  expect(calls).toEqual(['open-list'])
  expect(output).toEqual([])
})

test('/share private explicitly syncs privately', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'private')
  expect(calls).toEqual(['flush', 'share:session:private', 'ack:session'])
  expect(output[0]).toContain('☁ Shared to cloud')
  expect(output[0]).toContain('any machine')
})

test('/share public warns before publishing and returns the live page link', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'public')
  expect(calls).toEqual(['flush', 'share:session:public', 'ack:session'])
  expect(output[0]).toContain('anyone with the link can read it')
  expect(output.at(-1)).toContain('https://evot.ai/live/')
  expect(output.at(-1)).toContain('🌐')
})

test('/share off removes the cloud copy and keeps the local one', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'off')
  expect(calls).toEqual(['flush', 'off:session', 'forget:session'])
  expect(output.at(-1)).toContain('local copy kept')
})

test('a session id prefix targets another session, with or without a visibility word', async () => {
  const { ctx, calls } = setup()
  await runShareCommand(ctx, 'abc1')
  await runShareCommand(ctx, 'public abc1')
  expect(calls.filter(c => c.startsWith('share:'))).toEqual(['share:abc12345-0000:keep', 'share:abc12345-0000:public'])
})

test('divergence is reported as a choice, and each side has a word', async () => {
  const { ctx, output, calls } = setup(() => ({ kind: 'diverged', local_seq: 4, remote_seq: 6 }))
  await runShareCommand(ctx, 'private')
  expect(output.at(-1)).toContain('/share cloud')
  expect(output.at(-1)).toContain('/share local')
  await runShareCommand(ctx, 'cloud')
  expect(calls).toContain('fork:session')
  expect(calls).toContain('resume:fork0000-1111')
  await runShareCommand(ctx, 'local')
  expect(calls).toContain('push:session:true')
  expect(output.at(-1)).toContain('replaced')
})

test('does not sync partial runs, and rejects anything that is not a word or id', async () => {
  const { ctx, output, calls } = setup()
  ctx.isBusy = () => true
  await runShareCommand(ctx, 'public')
  expect(output.at(-1)).toContain('Wait for')
  ctx.isBusy = () => false
  await runShareCommand(ctx, 'https://tmpfiles.org/old#key')
  expect(output.at(-1)).toContain('Usage:')
  await runShareCommand(ctx, 'rm abcdefghijklmnopqrstuv')
  expect(output.at(-1)).toContain('Usage: /share [public | private | off | list] [session-id]')
  expect(calls).toEqual([])
})

test('list opens the cloud-filtered sessions list', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'list')
  expect(calls).toEqual(['open-list'])
  expect(output).toEqual([])
})

test('failed notice persistence prevents an incomplete push', async () => {
  const { ctx, output, calls } = setup()
  ctx.flushShareNotices = async () => { throw new Error('notices unavailable') }
  await runShareCommand(ctx, 'private')
  expect(calls).toEqual([])
  expect(output.at(-1)).toContain('notices unavailable')
})

test('bare /share on a public session opens the list without changing visibility', async () => {
  const { ctx, output, calls } = setup(() => synced('public'))
  await runShareCommand(ctx, '')
  expect(calls).toEqual(['open-list'])
  expect(output).toEqual([])
})

test('list rejects extra arguments instead of silently ignoring them', async () => {
  const { ctx, output, calls } = setup()
  await runShareCommand(ctx, 'list abc1')
  expect(calls).toEqual([])
  expect(output.at(-1)).toContain('Usage:')
})

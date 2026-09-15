import { expect, test } from 'bun:test'
import { inspectConsole } from '../src/channels/console-client.js'
import { observeFeishu, bindFeishuChat } from '../src/channels/feishu/client.js'
import { setupFeishu } from '../src/channels/feishu/onboarding.js'

const snapshot = { env_file_path: '/tmp/console.env', feishu: { app_id: 'test_app', app_secret_set: true, default_chat_id: '' } }

test('an existing console exposes settings and setup from its owning process', async () => {
  let bound: unknown
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, async fetch(request) {
    if (new URL(request.url).pathname === '/api/channels/feishu') return Response.json(snapshot)
    if (request.method === 'POST') {
      bound = await request.json()
      return Response.json({ ok: true })
    }
    return Response.json({ configured: true, revision: 'v1', default_chat_id: '', chats: ['oc_owner'], connection: { state: 'connected', message: '' } })
  } })
  try {
    const address = `http://127.0.0.1:${server.port}`
    expect((await inspectConsole(address)).env_file_path).toBe('/tmp/console.env')
    expect((await observeFeishu(address)).chats).toEqual(['oc_owner'])
    await bindFeishuChat(address, 'v1', 'oc_owner')
    expect(bound).toEqual({ revision: 'v1', chat_id: 'oc_owner' })
  } finally { await server.stop(true) }
})

test('missing setup endpoint fails once without legacy fallback or a fake connecting state', async () => {
  const paths: string[] = []
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch(request) {
    paths.push(new URL(request.url).pathname)
    return new Response('', { status: 404 })
  } })
  try {
    const address = `http://127.0.0.1:${server.port}`
    await expect(observeFeishu(address)).rejects.toThrow('needs the current evot build')
    expect(paths).toEqual(['/api/channels/feishu/setup'])
    let waits = 0
    await expect(setupFeishu({
      consoleUrl: `${address}/feishu`, signal: new AbortController().signal,
      observe: () => observeFeishu(address),
      bind: async () => { throw new Error('must not bind') },
      ask: async () => { throw new Error('must not ask') },
      wait: async () => { waits++; return false },
    })).rejects.toThrow('needs the current evot build')
    expect(waits).toBe(0)
    expect(paths).toEqual(['/api/channels/feishu/setup', '/api/channels/feishu/setup'])
  } finally { await server.stop(true) }
})

test('an unrelated service or redirect is not treated as an evot console', async () => {
  let redirect = false
  const server = Bun.serve({ hostname: '127.0.0.1', port: 0, fetch() {
    return redirect ? Response.redirect('http://127.0.0.1:1', 302) : Response.json({ ok: true })
  } })
  try {
    const address = `http://127.0.0.1:${server.port}`
    await expect(inspectConsole(address)).rejects.toThrow('different service')
    redirect = true
    await expect(inspectConsole(address)).rejects.toThrow()
  } finally { await server.stop(true) }
})

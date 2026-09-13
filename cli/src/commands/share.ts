import type { Agent } from '../native/index.js'
import { resolveSessionByPrefix } from '../term/app/resume.js'
import { createHyperlink } from '../render/hyperlink.js'

export interface ShareContext {
  agent: Pick<Agent, 'listSessions' | 'shareSession'>
  openShareList(): Promise<void>
  getSessionId(): string | null
  flushShareNotices?(): Promise<void>
  isBusy?(): boolean
  commitSystem(id: string, text: string): void
  requestRender(): void
}

/** Commands orchestrate only; persistence, auth and transport remain native. */
export async function runShareCommand(ctx: ShareContext, args: string): Promise<void> {
  const target = args.trim()
  const show = (text: string) => ctx.commitSystem('sys-share', text)
  try {
    if (target === 'list') {
      await ctx.openShareList()
      return
    }
    if (ctx.isBusy?.()) throw new Error('Wait for the current run to finish before sharing.')
    let sid = ctx.getSessionId()
    if (target) {
      if (!/^[0-9a-f-]{1,36}$/i.test(target)) throw new Error('Usage: /share [session-id | list]')
      const resolved = resolveSessionByPrefix(await ctx.agent.listSessions(0), target)
      if (resolved.kind !== 'matched') throw new Error(`Session ${resolved.kind === 'none' ? 'not found' : 'id is ambiguous'}: ${target}`)
      sid = resolved.session.session_id
    }
    if (!sid) throw new Error('No active session to share.')
    await ctx.flushShareNotices?.()
    // Say what leaves the machine before it does: the upload is public-by-link,
    // and deleting the share revokes the link, not what was already fetched.
    show('Uploading session… (transcript, system prompt, tool output — anyone with the link can read it)')
    ctx.requestRender()
    const result = await ctx.agent.shareSession(sid)
    show(createHyperlink(result.url))
  } catch (error) {
    ctx.commitSystem('sys-share-error', `Share failed: ${error instanceof Error ? error.message : String(error)}`)
  }
}

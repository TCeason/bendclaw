import type { Agent, CloudPushResult, SessionMeta } from '../native/index.js'
import { resolveSessionByPrefix } from '../term/app/resume.js'
import { createHyperlink } from '../render/hyperlink.js'
import { describeShareResult } from '../session/cloud-sessions.js'

export interface ShareContext {
  agent: Pick<Agent, 'listSessions' | 'cloudShareSession' | 'cloudUnshareSession' | 'cloudPushSession' | 'cloudForkRemoteSession'>
  openShareList(): Promise<void>
  /** Legacy one-shot share links, kept so they can still be revoked. */
  openShareLinks?(): Promise<void>
  getSessionId(): string | null
  flushShareNotices?(): Promise<void>
  isBusy?(): boolean
  commitSystem(id: string, text: string): void
  requestRender(): void
  /** The list caches remote state; tell it what just happened. */
  cloudAcknowledged?(sessionId: string, result: CloudPushResult): void
  cloudForgotten?(sessionId: string): void
  /** `/share cloud` lands in the fetched copy. */
  resumeSession?(session: SessionMeta): Promise<void>
}

const USAGE = 'Usage: /share [public | private | off | list] [session-id]'
const WORDS = new Set(['public', 'private', 'off', 'list', 'links', 'cloud', 'local'])

/**
 * `/share` opens the cloud session list without changing the current session.
 * `private` enables private sync; `public` also publishes a read-only page.
 * Commands orchestrate only; persistence, auth and transport remain native.
 */
export async function runShareCommand(ctx: ShareContext, args: string): Promise<void> {
  const parts = args.trim().split(/\s+/).filter(Boolean)
  // A bare command is navigation, like /sessions. Keep the historical
  // /share <id> form for explicit targets, without changing their visibility.
  const word = parts.length === 0 ? 'list' : parts[0] && WORDS.has(parts[0]) ? parts.shift()! : 'keep'
  const target = parts.shift()
  const show = (text: string) => ctx.commitSystem('sys-share', text)
  try {
    if (parts.length > 0) throw new Error(USAGE)
    if (word === 'list') {
      if (target) throw new Error(USAGE)
      await ctx.openShareList()
      return
    }
    if (word === 'links') {
      await ctx.openShareLinks?.()
      return
    }
    if (ctx.isBusy?.()) throw new Error('Wait for the current run to finish before sharing.')
    let sid = ctx.getSessionId()
    if (target) {
      if (!/^[0-9a-f-]{1,36}$/i.test(target)) throw new Error(USAGE)
      const resolved = resolveSessionByPrefix(await ctx.agent.listSessions(0), target)
      if (resolved.kind !== 'matched') throw new Error(`Session ${resolved.kind === 'none' ? 'not found' : 'id is ambiguous'}: ${target}`)
      sid = resolved.session.session_id
    }
    if (!sid) throw new Error('No active session to share.')
    await ctx.flushShareNotices?.()

    if (word === 'off') {
      await ctx.agent.cloudUnshareSession(sid)
      ctx.cloudForgotten?.(sid)
      show('Removed from cloud · local copy kept')
      return
    }
    if (word === 'cloud') {
      // Divergence, cloud side: the server copy becomes a fresh local session
      // and the diverged local one stays as it is.
      show('Fetching the cloud copy…')
      ctx.requestRender()
      const fork = await ctx.agent.cloudForkRemoteSession(sid)
      show(`☁ Cloud copy saved as ${fork.session_id.slice(0, 8)} · the local version stays as it was`)
      await ctx.resumeSession?.(fork)
      return
    }
    if (word === 'local') {
      // Divergence, local side: overwrite the server copy wholesale.
      const result = await ctx.agent.cloudPushSession(sid, true)
      ctx.cloudAcknowledged?.(sid, result)
      show(result.kind === 'synced' ? '☁ Cloud copy replaced with this session' : describeShareResult(result, 'private'))
      return
    }

    const requested = word as 'public' | 'private' | 'keep'
    if (requested === 'public') {
      // Say what leaves the machine before it does: the page is public-by-link,
      // and turning it private later hides the page, not what was already read.
      show('Publishing… (transcript, system prompt, tool output — anyone with the link can read it)')
      ctx.requestRender()
    }
    const result = await ctx.agent.cloudShareSession(sid, requested)
    ctx.cloudAcknowledged?.(sid, result)
    const visibility = result.kind === 'synced' ? result.cloud.visibility : requested === 'public' ? 'public' : 'private'
    const line = describeShareResult(result, visibility)
    const url = result.kind === 'synced' ? result.cloud.public_url : null
    show(visibility === 'public' && url ? line.replace(url, createHyperlink(url)) : line)
    if (visibility === 'private' && result.kind === 'synced') show('  Public page instead? /share public')
  } catch (error) {
    ctx.commitSystem('sys-share-error', `Share failed: ${error instanceof Error ? error.message : String(error)}`)
  }
}

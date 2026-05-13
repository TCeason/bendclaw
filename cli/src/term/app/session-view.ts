import type { SessionMeta } from '../../native/index.js'
import type { OutputLine } from '../../render/output.js'

export function shouldPreloadStartupSessions(opts: { continueLatest?: boolean; resumeSessionId?: string }): boolean {
  return Boolean(opts.continueLatest || opts.resumeSessionId)
}

export function findPreviousSession(preloaded: SessionMeta[], cwd: string): SessionMeta | undefined {
  return [...preloaded]
    .sort((a, b) => (b.updated_at || '').localeCompare(a.updated_at || ''))
    .find(s => s.cwd === cwd)
}

export function previousSessionLine(session: SessionMeta): OutputLine {
  const tag = session.source ? `[${session.source}] ` : ''
  const title = session.title || '(untitled)'
  const short = title.length > 40 ? title.slice(0, 39) + '…' : title
  return {
    id: `prev-session-${session.session_id}`,
    kind: 'system',
    text: `  previous session: ${tag}${short} · /resume ${session.session_id.slice(0, 8)}`,
  }
}

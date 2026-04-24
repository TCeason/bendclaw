import type { SessionMeta } from '../../native/index.js'
import type { OutputLine } from '../../render/output.js'
import { selectSessionPool } from './resume.js'

export function chooseBannerSessions(preloaded: SessionMeta[], cwd: string): SessionMeta[] {
  return selectSessionPool(preloaded, cwd)
}

export function findPreviousSession(preloaded: SessionMeta[], cwd: string): SessionMeta | undefined {
  return preloaded.find(s => s.cwd === cwd)
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

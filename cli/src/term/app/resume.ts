import { padRight, relativeTime } from '../../render/format.js'
import type { SessionMeta, SessionWithText } from '../../native/index.js'
import type { SelectorItem } from '../selector.js'

export const RESUME_SELECTOR_TITLE = 'Resume session  (^D delete)'

export type SessionPrefixResolution =
  | { kind: 'matched'; session: SessionMeta }
  | { kind: 'none' }
  | { kind: 'ambiguous'; matches: SessionMeta[] }

export function selectSessionPool<T extends { cwd?: string }>(sessions: T[], cwd: string): T[] {
  const cwdSessions = sessions.filter(s => s.cwd === cwd)
  return cwdSessions.length > 0 ? cwdSessions : sessions
}

export function isSessionIdPrefix(value: string): boolean {
  return /^[0-9a-f]{1,36}$/i.test(value)
}

export function resolveSessionByPrefix(sessions: SessionMeta[], prefix: string): SessionPrefixResolution {
  const matches = sessions.filter(s => s.session_id === prefix || s.session_id.startsWith(prefix))
  if (matches.length === 0) return { kind: 'none' }
  if (matches.length > 1) return { kind: 'ambiguous', matches }
  return { kind: 'matched', session: matches[0]! }
}

export function isResumeSelectorTitle(title: string): boolean {
  return title.startsWith('Resume session')
}

export function formatSessionItems(sessions: SessionMeta[]): SelectorItem[] {
  return sessions.map(s => {
    const source = padRight(s.source || '', 6)
    const title = padRight(s.title || '(untitled)', 65)
    const turns = padRight(s.turns ? `[${s.turns} turns]` : '', 12)
    const time = relativeTime(s.updated_at)
    const searchText = `${s.session_id} ${s.title} ${s.cwd} ${s.source} ${s.model}`
    return { label: s.session_id.slice(0, 8), id: s.session_id, detail: `${source} ${title} ${turns} ${time}`, searchText }
  })
}

export function formatSessionWithTextItems(items: SessionWithText[]): SelectorItem[] {
  return items.map(s => {
    const source = padRight(s.source || '', 6)
    const title = padRight(s.title || '(untitled)', 65)
    const turns = padRight(s.turns ? `[${s.turns} turns]` : '', 12)
    const time = relativeTime(s.updated_at)
    return { label: s.session_id.slice(0, 8), id: s.session_id, detail: `${source} ${title} ${turns} ${time}`, searchText: s.search_text }
  })
}

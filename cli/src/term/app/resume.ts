import { padRight, relativeTime } from '../../render/format.js'
import type { SessionMeta, SessionWithText } from '../../native/index.js'
import type { SelectorItem } from '../selector.js'

export const RESUME_SELECTOR_TITLE = 'Resume session  (Ctrl+D delete · twice)'

/**
 * Stem of the synthetic user message compaction injects ahead of a summary.
 * Shorter than the 40-character head budget used by session titles, so it also
 * matches titles an earlier release derived from that message before the
 * backend stopped doing so.
 */
export const COMPACT_SUMMARY_PREFIX = 'The conversation history before this'

/**
 * Display title for a session. Sessions compacted by an earlier release carry a
 * title built from the compaction summary boilerplate, which renders the resume
 * list as a wall of identical rows. Those read as `(compacted)` until the next
 * save recomputes them from real user turns.
 */
export function sanitizeSessionTitle(title?: string | null): string {
  if (title && title.startsWith(COMPACT_SUMMARY_PREFIX)) return '(compacted)'
  return title || '(untitled)'
}

export type SessionPrefixResolution =
  | { kind: 'matched'; session: SessionMeta }
  | { kind: 'none' }
  | { kind: 'ambiguous'; matches: SessionMeta[] }

export function shortenSessionCwd(cwd: string): string {
  const home = process.env.HOME || process.env.USERPROFILE || ''
  if (!home) return cwd
  if (cwd === home) return '~'
  return cwd.startsWith(`${home}/`) ? `~${cwd.slice(home.length)}` : cwd
}

function sessionHeader(label: string, group: string): SelectorItem {
  return { label, header: true, focusable: false, group }
}

function groupedSessionItems<T extends SessionMeta>(
  sessions: T[],
  currentCwd: string,
  format: (session: T, otherCwd: boolean) => SelectorItem,
): SelectorItem[] {
  const current = sessions.filter(session => session.cwd === currentCwd)
  const other = sessions.filter(session => session.cwd !== currentCwd)
  const items: SelectorItem[] = []

  if (current.length > 0) {
    items.push(sessionHeader(`Current cwd · ${shortenSessionCwd(currentCwd)}`, 'current-cwd'))
    items.push(...current.map(session => ({ ...format(session, false), group: 'current-cwd' })))
  }
  if (other.length > 0) {
    items.push(sessionHeader('Other cwd', 'other-cwd'))
    items.push(...other.map(session => ({ ...format(session, true), group: 'other-cwd' })))
  }
  return items
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

function formatSessionItem(s: SessionMeta, otherCwd: boolean, searchText: string): SelectorItem {
  const source = padRight(s.source || '', 6)
  const title = padRight(sanitizeSessionTitle(s.title), 65)
  const turns = padRight(s.turns ? `[${s.turns} turns]` : '', 12)
  const time = relativeTime(s.updated_at)
  const cwd = otherCwd ? `  ${shortenSessionCwd(s.cwd)}` : ''
  return {
    label: s.session_id.slice(0, 8),
    id: s.session_id,
    detail: `${source} ${title} ${turns} ${time}${cwd}`,
    searchText,
    contextPrefix: otherCwd ? `${shortenSessionCwd(s.cwd)} · ` : undefined,
  }
}

export function formatSessionItems(sessions: SessionMeta[], currentCwd: string): SelectorItem[] {
  return groupedSessionItems(sessions, currentCwd, (session, otherCwd) =>
    formatSessionItem(
      session,
      otherCwd,
      `${session.session_id} ${session.title ?? ''} ${session.cwd} ${session.source} ${session.provider ?? ''} ${session.model}`,
    ),
  )
}

export function formatSessionWithTextItems(items: SessionWithText[], currentCwd: string): SelectorItem[] {
  return groupedSessionItems(items, currentCwd, (session, otherCwd) =>
    formatSessionItem(session, otherCwd, session.search_text),
  )
}

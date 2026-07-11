import type { SessionMeta } from '../../native/index.js'
import type { OutputLine } from '../../render/output.js'
import type { UIMessage } from './types.js'

/** Default number of most-recent messages painted to scrollback on resume. */
export const RESUME_DISPLAY_LIMIT = 80

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

/**
 * Select which messages to paint on resume. Rendering the whole transcript
 * re-runs markdown per message (O(total), ~500ms on very long sessions), so we
 * keep only the most recent `limit`. Hidden messages stay in the model's
 * context — the backend restores it by session_id, independent of this display
 * transcript — so this trims what's painted, not what the model remembers.
 */
export function selectResumeMessages(
  messages: UIMessage[],
  limit: number = RESUME_DISPLAY_LIMIT,
): { shown: UIMessage[]; hidden: number } {
  const hidden = Math.max(0, messages.length - limit)
  return {
    shown: hidden > 0 ? messages.slice(-limit) : messages,
    hidden,
  }
}

/** System notice shown above the resumed transcript when older messages are hidden. */
export function resumeElidedLine(hidden: number, limit: number = RESUME_DISPLAY_LIMIT): OutputLine {
  const plural = hidden === 1 ? '' : 's'
  return {
    id: 'sys-resumed-elided',
    kind: 'system',
    text: `  … ${hidden} earlier message${plural} hidden (still in context) · showing the latest ${limit}`,
  }
}

/**
 * Notice when a session's saved provider/model cannot be restored (e.g. the
 * channel was removed from config). Resume still loads the transcript and keeps
 * the live model so the user can switch with /model.
 */
export function resumeModelUnavailableNote(opts: {
  provider?: string
  model?: string
  keptModel: string
}): string {
  if (opts.provider) {
    return `  provider '${opts.provider}' unavailable · kept ${opts.keptModel} · /model to switch`
  }
  return `  model '${opts.model ?? ''}' unavailable · kept ${opts.keptModel} · /model to switch`
}

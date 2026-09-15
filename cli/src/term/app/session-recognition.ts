import type { SessionWithText, TranscriptItem } from '../../native/index.js'
import { replayUserText } from '../../session/task-notification.js'

/** UI-only enrichment, never serialized into the native/search wire contract. */
export interface SessionRecognition extends SessionWithText {
  recognition?: {
    /** First user turn, unclipped: the echo test compares against it. */
    opening?: string
    /** Newest-first user-visible turns, excerpted for the pane. */
    recent: { role: 'user' | 'assistant'; text: string }[]
  }
}

const RECENT_LIMIT = 6
const EXCERPT_CHARS = 2400

function excerpt(text: string): string {
  const chars = Array.from(text.trim())
  return chars.length > EXCERPT_CHARS
    ? `${chars.slice(0, EXCERPT_CHARS).join('')}\n[Excerpt — resume session to read more]`
    : chars.join('')
}

function visibleText(item: TranscriptItem, clip = true): { role: 'user' | 'assistant'; text: string } | undefined {
  if (item.type === 'user' && typeof item.text === 'string') {
    if (item.text.startsWith('The conversation history before this')) return undefined
    const text = replayUserText(item.text).filter(part => part.kind === 'user').map(part => part.text).join('\n').trim()
    return text ? { role: 'user', text: clip ? excerpt(text) : text } : undefined
  }
  if (item.type !== 'assistant') return undefined
  const blocks = Array.isArray(item.content) ? item.content : item.content_blocks
  if (!Array.isArray(blocks)) return undefined
  // Only user-visible answer text. Thinking, tool inputs/results and system
  // messages are not a summary and must not leak into the recognition pane.
  const text = blocks.filter(block => block?.type === 'text' && typeof block.text === 'string')
    .map(block => block.text).join('').trim()
  return text ? { role: 'assistant', text: excerpt(text) } : undefined
}

export function enrichSessionRecognition(session: SessionWithText, transcript: TranscriptItem[]): SessionRecognition {
  let opening: string | undefined
  for (const item of transcript) {
    if (item.type !== 'user') continue
    const entry = visibleText(item, false)
    if (entry) {
      opening = entry.text
      break
    }
  }
  const recent: NonNullable<SessionRecognition['recognition']>['recent'] = []
  for (let index = transcript.length - 1; index >= 0 && recent.length < RECENT_LIMIT; index--) {
    const entry = visibleText(transcript[index]!)
    if (entry) recent.push(entry)
  }
  return { ...session, recognition: { opening, recent } }
}

/**
 * Recognition order: where the session left off, then what it was asked first.
 * Each turn appears once — a single-turn session shows its only request rather
 * than the same sentence twice under two headings.
 */
export function recognitionSections(session: SessionRecognition): string[] {
  const detail = session.recognition
  if (!detail) return []
  const latestUser = detail.recent.find(entry => entry.role === 'user')
  const answer = detail.recent.find(entry => entry.role === 'assistant')
  const request = latestUser ?? (detail.opening ? { role: 'user' as const, text: detail.opening } : undefined)
  // In a one-turn session the opening *is* the latest request, and in a long one
  // the request is a clipped prefix of it. Prefix-inclusion settles both without
  // tracking whether either side was cut.
  const openingPrefix = detail.opening ? Array.from(detail.opening).slice(0, EXCERPT_CHARS).join('') : ''
  const openingIsRequest = Boolean(openingPrefix) && Boolean(request?.text.includes(openingPrefix))

  const lines: string[] = []
  if (answer) lines.push('# Latest assistant response', ...answer.text.split('\n'), '')
  if (request) lines.push('# Latest request', ...request.text.split('\n'), '')
  if (detail.opening && !openingIsRequest) lines.push('# Original goal', ...detail.opening.split('\n'), '')
  if (lines.length === 0) lines.push('# Conversation', 'No user requests or assistant responses yet.')
  return lines
}

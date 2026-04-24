import { describe, expect, test } from 'bun:test'
import { chooseBannerSessions, findPreviousSession, previousSessionLine } from '../src/term/app/session-view.js'
import type { SessionMeta } from '../src/native/index.js'

describe('repl session view helpers', () => {
  const sessions: SessionMeta[] = [
    { session_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', title: 'cwd session', cwd: '/work', source: 'local' } as any,
    { session_id: 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb', title: 'other session', cwd: '/other' } as any,
  ]

  test('chooseBannerSessions prefers cwd sessions', () => {
    expect(chooseBannerSessions(sessions, '/work')).toEqual([sessions[0]])
  })

  test('chooseBannerSessions falls back to all sessions', () => {
    expect(chooseBannerSessions(sessions, '/missing')).toEqual(sessions)
  })

  test('findPreviousSession returns first cwd session', () => {
    expect(findPreviousSession(sessions, '/work')).toBe(sessions[0])
  })

  test('previousSessionLine formats source, title, and resume prefix', () => {
    const line = previousSessionLine(sessions[0]!)
    expect(line.id).toBe(`prev-session-${sessions[0]!.session_id}`)
    expect(line.kind).toBe('system')
    expect(line.text).toBe('  previous session: [local] cwd session · /resume aaaaaaaa')
  })

  test('previousSessionLine truncates long title', () => {
    const line = previousSessionLine({ session_id: sessions[0]!.session_id, title: 'x'.repeat(41) } as any)
    expect(line.text).toContain(`${'x'.repeat(39)}…`)
  })
})

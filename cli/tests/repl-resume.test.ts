import { describe, expect, test } from 'bun:test'
import {
  RESUME_SELECTOR_TITLE,
  formatSessionItems,
  formatSessionWithTextItems,
  isResumeSelectorTitle,
  isSessionIdPrefix,
  resolveSessionByPrefix,
} from '../src/term/app/resume.js'
import type { SessionMeta, SessionWithText } from '../src/native/index.js'

describe('repl resume helpers', () => {
  const sessions: SessionMeta[] = [
    { session_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', title: 'cwd session', cwd: '/work', source: 'local', model: 'm1', turns: 3, updated_at: Date.now() } as any,
    { session_id: 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb', title: 'other session', cwd: '/other', model: 'm2', updated_at: Date.now() } as any,
  ]

  test('isSessionIdPrefix accepts hex prefix only', () => {
    expect(isSessionIdPrefix('abc123')).toBe(true)
    expect(isSessionIdPrefix('ABCDEF')).toBe(true)
    expect(isSessionIdPrefix('not-hex')).toBe(false)
    expect(isSessionIdPrefix('')).toBe(false)
  })

  test('resolveSessionByPrefix matches unique prefix', () => {
    const resolved = resolveSessionByPrefix(sessions, 'aaaaaaaa')
    expect(resolved.kind).toBe('matched')
    if (resolved.kind === 'matched') expect(resolved.session).toBe(sessions[0])
  })

  test('resolveSessionByPrefix reports none', () => {
    expect(resolveSessionByPrefix(sessions, 'cccc').kind).toBe('none')
  })

  test('resolveSessionByPrefix reports ambiguous prefixes', () => {
    const ambiguous = [
      { session_id: 'abc11111-1111-4111-8111-aaaaaaaaaaaa' },
      { session_id: 'abc22222-2222-4222-8222-bbbbbbbbbbbb' },
    ] as SessionMeta[]
    const resolved = resolveSessionByPrefix(ambiguous, 'abc')
    expect(resolved.kind).toBe('ambiguous')
  })

  test('formatSessionItems groups current cwd before other cwd', () => {
    const items = formatSessionItems(sessions, '/work')
    expect(items.map(item => item.label)).toEqual([
      'Current cwd · /work',
      'aaaaaaaa',
      'Other cwd',
      'bbbbbbbb',
    ])
    expect(items[0]).toMatchObject({ header: true, focusable: false, group: 'current-cwd' })
    expect(items[1]).toMatchObject({ id: sessions[0]!.session_id, group: 'current-cwd' })
    expect(items[1]!.detail).toContain('local ')
    expect(items[1]!.detail).toContain('cwd session')
    expect(items[1]!.detail).toContain('[3 turns]')
    expect(items[1]!.searchText).toContain('/work')
    expect(items[3]).toMatchObject({ id: sessions[1]!.session_id, group: 'other-cwd' })
    expect(items[3]!.detail).toContain('/other')
  })

  test('formatSessionItems shows other cwd when current cwd has no sessions', () => {
    const items = formatSessionItems(sessions, '/missing')
    expect(items[0]).toMatchObject({ label: 'Other cwd', header: true })
    expect(items.filter(item => !item.header)).toHaveLength(2)
  })

  test('formatSessionWithTextItems uses full search text and matching groups', () => {
    const withText = sessions.map((session, index) => ({
      ...session,
      search_text: index === 0 ? 'current full text body' : 'other full text body',
    })) as SessionWithText[]
    const items = formatSessionWithTextItems(withText, '/work')
    expect(items[1]!.searchText).toBe('current full text body')
    expect(items[3]!.searchText).toBe('other full text body')
    expect(items[3]!.contextPrefix).toBe('/other · ')
  })

  test('resume title shows the portable Ctrl+D delete shortcut', () => {
    expect(RESUME_SELECTOR_TITLE).toBe('Resume session  (Ctrl+D delete)')
    expect(isResumeSelectorTitle(RESUME_SELECTOR_TITLE)).toBe(true)
    expect(isResumeSelectorTitle('Select model')).toBe(false)
  })
})

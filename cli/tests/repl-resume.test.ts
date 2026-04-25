import { describe, expect, test } from 'bun:test'
import {
  RESUME_SELECTOR_TITLE,
  formatSessionItems,
  formatSessionWithTextItems,
  isResumeSelectorTitle,
  isSessionIdPrefix,
  resolveSessionByPrefix,
  selectSessionPool,
} from '../src/term/app/resume.js'
import type { SessionMeta, SessionWithText } from '../src/native/index.js'

describe('repl resume helpers', () => {
  const sessions: SessionMeta[] = [
    { session_id: 'aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa', title: 'cwd session', cwd: '/work', source: 'local', model: 'm1', turns: 3, updated_at: Date.now() } as any,
    { session_id: 'bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb', title: 'other session', cwd: '/other', model: 'm2', updated_at: Date.now() } as any,
  ]

  test('selectSessionPool prefers cwd sessions', () => {
    expect(selectSessionPool(sessions, '/work')).toEqual([sessions[0]])
  })

  test('selectSessionPool falls back to all sessions', () => {
    expect(selectSessionPool(sessions, '/missing')).toEqual(sessions)
  })

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

  test('formatSessionItems builds selector items', () => {
    const item = formatSessionItems([sessions[0]!])[0]!
    expect(item.label).toBe('aaaaaaaa')
    expect(item.id).toBe(sessions[0]!.session_id)
    expect(item.detail).toContain('local ')
    expect(item.detail).toContain('cwd session')
    expect(item.detail).toContain('[3 turns]')
    expect(item.searchText).toContain('/work')
  })

  test('formatSessionWithTextItems uses full search text', () => {
    const withText = [{ ...sessions[0], search_text: 'full text body' }] as SessionWithText[]
    const item = formatSessionWithTextItems(withText)[0]!
    expect(item.searchText).toBe('full text body')
  })

  test('isResumeSelectorTitle accepts resume title', () => {
    expect(isResumeSelectorTitle(RESUME_SELECTOR_TITLE)).toBe(true)
    expect(isResumeSelectorTitle('Select model')).toBe(false)
  })
})

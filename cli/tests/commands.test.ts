import { describe, test, expect } from 'bun:test'
import { mkdirSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { resolveCommand, isSlashCommand } from '../src/commands/index.js'
import { skillListFromDirs } from '../src/commands/skill.js'

describe('isSlashCommand', () => {
  test('recognizes slash commands', () => {
    expect(isSlashCommand('/help')).toBe(true)
    expect(isSlashCommand('/h')).toBe(true)
    expect(isSlashCommand('/model gpt-4')).toBe(true)
  })

  test('rejects non-commands', () => {
    expect(isSlashCommand('hello')).toBe(false)
    expect(isSlashCommand('')).toBe(false)
    expect(isSlashCommand('/')).toBe(false)
  })

  test('rejects double-slash paths', () => {
    expect(isSlashCommand('//some/path')).toBe(false)
  })

  test('rejects pasted file paths', () => {
    expect(isSlashCommand('/some/path.rs')).toBe(false)
    expect(isSlashCommand('/usr/local/bin')).toBe(false)
  })
})

describe('resolveCommand', () => {
  test('resolves exact command names', () => {
    const result = resolveCommand('/help')
    expect(result).toEqual({ kind: 'resolved', name: '/help', args: '' })
  })

  test('resolves command with args', () => {
    const result = resolveCommand('/model gpt-4o')
    expect(result).toEqual({ kind: 'resolved', name: '/model', args: 'gpt-4o' })
  })

  test('resolves aliases', () => {
    const result = resolveCommand('/q')
    expect(result).toEqual({ kind: 'resolved', name: '/exit', args: '' })
  })

  test('resolves by prefix when unambiguous', () => {
    const result = resolveCommand('/he')
    expect(result).toEqual({ kind: 'resolved', name: '/help', args: '' })
  })

  test('returns ambiguous for multiple prefix matches', () => {
    // /plan and /act both exist, but /p could match /plan only
    const result = resolveCommand('/p')
    expect(result.kind).toBe('resolved')
  })

  test('returns unknown for unrecognized commands', () => {
    const result = resolveCommand('/foobar')
    expect(result).toEqual({ kind: 'unknown' })
  })

  test('is case insensitive', () => {
    const result = resolveCommand('/HELP')
    expect(result).toEqual({ kind: 'resolved', name: '/help', args: '' })
  })

  test('resolves /v alias to /verbose', () => {
    const result = resolveCommand('/v')
    expect(result).toEqual({ kind: 'resolved', name: '/verbose', args: '' })
  })

  test('handles extra whitespace in args', () => {
    const result = resolveCommand('/resume   abc123')
    expect(result).toEqual({ kind: 'resolved', name: '/resume', args: 'abc123' })
  })
})

describe('skillListFromDirs', () => {
  test('lists skills from evotai and claude directories', () => {
    const home = join(tmpdir(), `evot-skill-list-${Date.now()}`)
    const evotai = join(home, '.evotai', 'skills')
    const claude = join(home, '.claude', 'skills')

    try {
      mkdirSync(join(evotai, 'evot-skill'), { recursive: true })
      mkdirSync(join(claude, 'claude-skill'), { recursive: true })
      writeFileSync(join(evotai, 'evot-skill', 'SKILL.md'), '---\ndescription: evot\n---\n')
      writeFileSync(join(claude, 'claude-skill', 'SKILL.md'), '---\ndescription: claude\n---\n')

      expect(skillListFromDirs([evotai, claude])).toBe([
        '',
        '  Skills:',
        `  • [claude-skill] ${join(claude, 'claude-skill')}`,
        `  • [evot-skill] ${join(evotai, 'evot-skill')}`,
      ].join('\n'))
    } finally {
      rmSync(home, { recursive: true, force: true })
    }
  })
})

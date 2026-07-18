import { describe, test, expect } from 'bun:test'
import { mkdirSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { resolveCommand, isSlashCommand, buildHardenPrompt } from '../src/commands/index.js'
import { skillListFromDirs, resolveSkillsDirs, skillList } from '../src/commands/skill.js'
import { getSkillNames } from '../src/term/banner-skills.js'
import { homedir } from 'os'

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

  test('resolves /harden command', () => {
    const result = resolveCommand('/harden plan')
    expect(result).toEqual({ kind: 'resolved', name: '/harden', args: 'plan' })
    expect(isSlashCommand('/harden')).toBe(true)
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

  test('routes removed /history command to the unknown-command handler', () => {
    expect(resolveCommand('/history')).toEqual({ kind: 'unknown' })
    expect(isSlashCommand('/history')).toBe(true)
    expect(isSlashCommand('/history 10')).toBe(true)
  })

  test('returns unknown for unrecognized commands', () => {
    const result = resolveCommand('/foobar')
    expect(result).toEqual({ kind: 'unknown' })
  })

  test('resolves /copy command', () => {
    const result = resolveCommand('/copy')
    expect(result).toEqual({ kind: 'resolved', name: '/copy', args: '' })
    expect(isSlashCommand('/copy')).toBe(true)
  })

  test('/c is ambiguous between /copy and /clear', () => {
    const result = resolveCommand('/c')
    expect(result.kind).toBe('ambiguous')
    if (result.kind === 'ambiguous') {
      expect(result.candidates).toContain('/copy')
      expect(result.candidates).toContain('/clear')
    }
  })

  test('is case insensitive', () => {
    const result = resolveCommand('/HELP')
    expect(result).toEqual({ kind: 'resolved', name: '/help', args: '' })
  })

  test('handles extra whitespace in args', () => {
    const result = resolveCommand('/resume   abc123')
    expect(result).toEqual({ kind: 'resolved', name: '/resume', args: 'abc123' })
  })
})

describe('buildHardenPrompt', () => {
  test('defaults to previous plan or conclusion with git diff as supporting context', () => {
    const prompt = buildHardenPrompt('')

    expect(prompt).toContain('immediately preceding conversation context')
    expect(prompt).toContain('If local git changes exist')
    expect(prompt).toContain('supporting context')
    expect(prompt).toContain('do not default to hardening the diff')
    expect(prompt).not.toBe('harden current git changes')
  })

  test('keeps explicit changes subject focused on git changes', () => {
    expect(buildHardenPrompt('changes')).toBe('harden current git changes')
  })

  test('keeps explicit plan subject focused on previous context', () => {
    expect(buildHardenPrompt('plan')).toContain('immediately preceding conversation context')
  })

  test('keeps explicit arch subject focused on architecture', () => {
    const prompt = buildHardenPrompt('arch')
    expect(prompt).toContain('architecture')
    expect(prompt).toContain('simplicity')
    expect(prompt).toContain('annotated file tree')
  })

  test('passes custom subject through as strategy', () => {
    expect(buildHardenPrompt('retry rollout')).toBe('harden this strategy: retry rollout')
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

describe('resolveSkillsDirs', () => {
  const evotaiDir = join(homedir(), '.evotai', 'skills')
  const claudeDir = join(homedir(), '.claude', 'skills')

  test('defaults to global + claude dirs when EVOT_SKILLS_DIRS is unset', () => {
    expect(resolveSkillsDirs({})).toEqual([evotaiDir, claudeDir])
  })

  test('inserts EVOT_SKILLS_DIRS entries between global and claude, in order', () => {
    expect(resolveSkillsDirs({ EVOT_SKILLS_DIRS: '/abs/one:/abs/two' })).toEqual([
      evotaiDir,
      '/abs/one',
      '/abs/two',
      claudeDir,
    ])
  })

  test('expands a leading ~ in EVOT_SKILLS_DIRS entries', () => {
    expect(resolveSkillsDirs({ EVOT_SKILLS_DIRS: '~/work/skills' })).toEqual([
      evotaiDir,
      join(homedir(), 'work', 'skills'),
      claudeDir,
    ])
  })

  test('trims whitespace and skips empty segments', () => {
    expect(resolveSkillsDirs({ EVOT_SKILLS_DIRS: ' /a : : /b ' })).toEqual([
      evotaiDir,
      '/a',
      '/b',
      claudeDir,
    ])
  })

  test('de-duplicates while preserving order', () => {
    // Repeating the global dir must not produce a duplicate entry.
    expect(resolveSkillsDirs({ EVOT_SKILLS_DIRS: evotaiDir })).toEqual([evotaiDir, claudeDir])
  })
})

describe('skillList / getSkillNames honor an explicit dirs override (issue #38)', () => {
  // The agent resolves EVOT_SKILLS_DIRS from ~/.evotai/evot.env, which
  // resolveSkillsDirs() (process.env only) can't see. Both display helpers must
  // scan the caller-provided dirs verbatim so `/skill list` and the banner match
  // what the agent actually loaded.
  test('skillList scans provided dirs, not process.env', () => {
    const home = join(tmpdir(), `evot-skill-override-${Date.now()}`)
    const envFileDir = join(home, 'from-env-file', 'skills')
    try {
      mkdirSync(join(envFileDir, 'env-skill'), { recursive: true })
      writeFileSync(join(envFileDir, 'env-skill', 'SKILL.md'), '---\ndescription: x\n---\n')
      const out = skillList([envFileDir])
      expect(out).toContain('[env-skill]')
      expect(out).toContain(join(envFileDir, 'env-skill'))
    } finally {
      rmSync(home, { recursive: true, force: true })
    }
  })

  test('getSkillNames scans provided dirs, not process.env', () => {
    const home = join(tmpdir(), `evot-skill-names-${Date.now()}`)
    const envFileDir = join(home, 'from-env-file', 'skills')
    try {
      mkdirSync(join(envFileDir, 'alpha'), { recursive: true })
      mkdirSync(join(envFileDir, 'beta'), { recursive: true })
      writeFileSync(join(envFileDir, 'alpha', 'SKILL.md'), '---\n---\n')
      writeFileSync(join(envFileDir, 'beta', 'SKILL.md'), '---\n---\n')
      expect(getSkillNames([envFileDir])).toEqual(['alpha', 'beta'])
    } finally {
      rmSync(home, { recursive: true, force: true })
    }
  })
})

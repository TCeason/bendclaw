import { describe, test, expect } from 'bun:test'
import { complete, getGhostHint } from '../src/commands/completion.js'

describe('slash command completion', () => {
  test('completes single match with trailing space', () => {
    const result = complete('/he', 3)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/help ')
    expect(result!.candidates).toEqual(['/help'])
    expect(result!.wordStart).toBe(0)
  })

  test('completes common prefix for multiple matches', () => {
    // /e matches /exit and /env — should show both candidates
    const result = complete('/e', 2)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/exit')
    expect(result!.candidates).toContain('/env')
  })

  test('returns null for no matches', () => {
    const result = complete('/zzz', 4)
    expect(result).toBeNull()
  })

  test('completes /ha to /harden', () => {
    const result = complete('/ha', 3)
    expect(result).not.toBeNull()
    expect(result!.replacement).toBe('/harden ')
    expect(result!.candidates).toEqual(['/harden'])
  })

  test('returns null when past the command word', () => {
    const result = complete('/model gpt', 10)
    expect(result).toBeNull()
  })

  test('completes /q to /quit alias', () => {
    const result = complete('/qu', 3)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/quit')
  })

  test('shows multiple candidates for ambiguous prefix', () => {
    // /cl could match /clear
    const result = complete('/cl', 3)
    expect(result).not.toBeNull()
    expect(result!.candidates).toContain('/clear')
  })
})

describe('file path completion', () => {
  test('completes paths starting with ./', () => {
    const result = complete('./', 2)
    expect(result).not.toBeNull()
    expect(result!.candidates.length).toBeGreaterThan(0)
  })

  test('returns null for plain text', () => {
    const result = complete('hello world', 11)
    expect(result).toBeNull()
  })
})

describe('ghost hints', () => {
  test('shows goal subcommands and options', () => {
    const hint = getGhostHint('/goal ', 6)
    expect(hint).toContain('show')
    expect(hint).toContain('pause')
    expect(hint).toContain('resume')
    expect(hint).toContain('clear')
    expect(hint).toContain('<objective>')
    expect(hint).toContain('--budget=<tokens>')
    expect(hint).toContain('--max-iter=<n>')
    expect(hint).toContain('--timeout=<secs>')
  })
})

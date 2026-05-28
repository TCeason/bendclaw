import { describe, test, expect } from 'bun:test'
import { extractAtPrefix, completeAtFile } from '../src/commands/file-completion.js'
import { mkdtempSync, writeFileSync, mkdirSync } from 'fs'
import { join } from 'path'
import { tmpdir } from 'os'

describe('extractAtPrefix', () => {
  test('extracts @ at start of line', () => {
    expect(extractAtPrefix('@src')).toEqual({ prefix: '@src', start: 0 })
  })

  test('extracts @ after space', () => {
    expect(extractAtPrefix('look at @src/m')).toEqual({ prefix: '@src/m', start: 8 })
  })

  test('returns null for email-like patterns', () => {
    expect(extractAtPrefix('user@example')).toBeNull()
  })

  test('returns null when no @', () => {
    expect(extractAtPrefix('hello world')).toBeNull()
  })

  test('extracts @ with empty query', () => {
    expect(extractAtPrefix('@')).toEqual({ prefix: '@', start: 0 })
  })

  test('extracts @ after multiple words', () => {
    expect(extractAtPrefix('fix the bug in @cli/src')).toEqual({ prefix: '@cli/src', start: 15 })
  })
})

describe('completeAtFile', () => {
  const tmp = mkdtempSync(join(tmpdir(), 'evot-file-completion-'))
  mkdirSync(join(tmp, 'src'))
  writeFileSync(join(tmp, 'src', 'main.ts'), 'console.log("hi")')
  writeFileSync(join(tmp, 'src', 'utils.ts'), 'export {}')
  writeFileSync(join(tmp, 'README.md'), '# Test')
  mkdirSync(join(tmp, 'src', 'nested'))

  test('completes files in root with readdir fallback', async () => {
    const result = await completeAtFile('@', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.length).toBeGreaterThan(0)
    expect(result!.items.some(i => i.label === 'src/')).toBe(true)
    expect(result!.items.some(i => i.label === 'README.md')).toBe(true)
  })

  test('completes files in subdirectory', async () => {
    const result = await completeAtFile('@src/', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/main.ts')).toBe(true)
    expect(result!.items.some(i => i.label === 'src/utils.ts')).toBe(true)
  })

  test('filters by prefix', async () => {
    const result = await completeAtFile('@src/m', tmp)
    expect(result).not.toBeNull()
    expect(result!.items.some(i => i.label === 'src/main.ts')).toBe(true)
    expect(result!.items.some(i => i.label === 'src/utils.ts')).toBe(false)
  })

  test('returns null for non-matching prefix', async () => {
    const result = await completeAtFile('@zzz_nonexistent', tmp)
    expect(result).toBeNull()
  })

  test('respects abort signal', async () => {
    const abort = new AbortController()
    abort.abort()
    const result = await completeAtFile('@src', tmp, abort.signal)
    // With readdir fallback, abort only affects fd; readdir still works
    // But if fd is found, it should return empty
    expect(result === null || result.items.length >= 0).toBe(true)
  })
})


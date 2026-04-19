import { describe, test, expect } from 'bun:test'
import { parseArgs } from '../src/cli.js'

describe('parseArgs', () => {
  test('-f / --file collects files', async () => {
    const opts = await parseArgs(['-p', 'hello', '-f', 'a.ts', '--file', 'b.ts'])
    expect(opts.command).toBe('prompt')
    expect(opts.files).toEqual(['a.ts', 'b.ts'])
  })

  test('-r is short alias for --resume', async () => {
    const opts = await parseArgs(['-p', 'hello', '-r', 'my-session'])
    expect(opts.resume).toBe('my-session')
  })

  test('--resume still works', async () => {
    const opts = await parseArgs(['-p', 'hello', '--resume', 'sid-123'])
    expect(opts.resume).toBe('sid-123')
  })

  test('files defaults to empty array', async () => {
    const opts = await parseArgs(['-p', 'hello'])
    expect(opts.files).toEqual([])
  })

  test('-p -f -r together', async () => {
    const opts = await parseArgs(['-p', 'review', '-f', 'src/cli.ts', '-f', 'src/prompt.ts', '-r', 'task-1'])
    expect(opts.command).toBe('prompt')
    expect(opts.prompt).toBe('review')
    expect(opts.files).toEqual(['src/cli.ts', 'src/prompt.ts'])
    expect(opts.resume).toBe('task-1')
  })
})

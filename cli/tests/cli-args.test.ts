import { describe, test, expect } from 'bun:test'
import { parseArgs, applyCliOpts } from '../src/cli.js'

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

  test('-c / --continue resumes the latest session', async () => {
    const shortOpts = await parseArgs(['-p', 'hello', '-c'])
    const longOpts = await parseArgs(['-p', 'hello', '--continue'])
    expect(shortOpts.continueLatest).toBe(true)
    expect(longOpts.continueLatest).toBe(true)
  })

  test('files defaults to empty array', async () => {
    const opts = await parseArgs(['-p', 'hello'])
    expect(opts.files).toEqual([])
    expect(opts.skillNames).toEqual([])
  })

  test('--skill selects skills by name', async () => {
    const opts = await parseArgs(['-p', 'hello', '--skill', 'review', '--skill', 'custom'])
    expect(opts.skillNames).toEqual(['review', 'custom'])
  })

  test('one-shot skill filtering does not affect interactive TUI', async () => {
    const selected: string[][] = []
    const addedDirs: string[][] = []
    const agent = {
      setLimits() {},
      appendSystemPrompt() {},
      addSkillsDirs(dirs: string[]) { addedDirs.push(dirs) },
      setSkillNames(names: string[]) { selected.push(names) },
    }

    const replOpts = await parseArgs(['--skill', 'review', '--skills', '/repl/skills'])
    applyCliOpts(agent as any, replOpts)
    expect(selected).toEqual([])
    expect(addedDirs).toEqual([['/repl/skills']])

    const promptOpts = await parseArgs(['-p', 'hello', '--skill', 'review', '--skills', '/prompt/skills'])
    applyCliOpts(agent as any, promptOpts)
    expect(selected).toEqual([['review']])
    expect(addedDirs).toEqual([['/repl/skills'], ['/prompt/skills']])

    const noSkillPrompt = await parseArgs(['-p', 'hello'])
    applyCliOpts(agent as any, noSkillPrompt)
    expect(selected).toEqual([['review'], []])
  })

  test('-p -f -r together', async () => {
    const opts = await parseArgs(['-p', 'review', '-f', 'src/cli.ts', '-f', 'src/prompt.ts', '-r', 'task-1'])
    expect(opts.command).toBe('prompt')
    expect(opts.prompt).toBe('review')
    expect(opts.files).toEqual(['src/cli.ts', 'src/prompt.ts'])
    expect(opts.resume).toBe('task-1')
  })
})

import {
  appendFileSync,
  existsSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { describe, expect, test } from 'bun:test'
import { RendererTrace } from '../src/session/renderer-trace.js'
import { RollingLogWriter } from '../src/session/rolling-log.js'
import { TermRenderer, type RendererTraceEntry } from '../src/term/renderer.js'
import { Writable } from 'node:stream'

class TraceStdout extends Writable {
  rows = 6
  columns = 40

  _write(_chunk: Buffer | string, _encoding: string, callback: () => void): void {
    callback()
  }
}

function segments(runDirectory: string): string[] {
  return readdirSync(runDirectory)
    .filter(name => /^\d{6}\.jsonl$/.test(name))
    .sort()
}

function frameEntries(path: string): RendererTraceEntry[] {
  return readFileSync(path, 'utf8')
    .trim()
    .split('\n')
    .filter(Boolean)
    .map(line => JSON.parse(line))
    .filter(entry => entry.kind === 'frame')
}

function analyzeRun(runDirectory: string): { exitCode: number; stdout: string; stderr: string } {
  const result = Bun.spawnSync({
    cmd: ['bun', 'scripts/analyze-renderer-trace.ts', runDirectory],
    cwd: join(import.meta.dir, '..'),
    stdout: 'pipe',
    stderr: 'pipe',
  })
  return {
    exitCode: result.exitCode,
    stdout: result.stdout.toString(),
    stderr: result.stderr.toString(),
  }
}

describe('RollingLogWriter', () => {
  test('rolls segments and retains only the configured newest segments', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-rolling-log-'))
    try {
      const writer = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 40,
        retainSegments: 2,
        retainRuns: 3,
      })
      for (let index = 0; index < 6; index++) {
        writer.append(JSON.stringify({ index, payload: 'x'.repeat(30) }))
      }

      const names = segments(writer.runDirectory)
      expect(names).toEqual(['000005.jsonl', '000006.jsonl'])
      expect(statSync(join(writer.runDirectory, names[0]!)).mode & 0o777).toBe(0o600)
      expect(statSync(writer.runDirectory).mode & 0o777).toBe(0o700)
      expect(existsSync(join(writer.runDirectory, '.active'))).toBe(true)
      writer.close()
      expect(existsSync(join(writer.runDirectory, '.active'))).toBe(false)
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  test('retains closed managed runs but never deletes active or unrelated directories', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-rolling-runs-'))
    try {
      const unrelated = join(root, 'business-data')
      writeFileSync(unrelated, 'keep me')

      const first = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 2,
      })
      first.close()
      const second = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 2,
      })
      second.close()
      const third = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 2,
      })
      third.close()
      const active = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 2,
      })

      const retainedClosed = [first.runDirectory, second.runDirectory, third.runDirectory]
        .filter(directory => existsSync(directory))
      expect(retainedClosed).toHaveLength(2)
      expect(existsSync(active.runDirectory)).toBe(true)
      expect(readFileSync(unrelated, 'utf8')).toBe('keep me')
      active.close()
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  test('reclaims a dead active marker into closed-run retention', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-rolling-stale-'))
    try {
      const stale = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 1,
      })
      // Simulate an abnormal exit: leave .active behind, but point it at a PID
      // that cannot exist on supported platforms.
      writeFileSync(join(stale.runDirectory, '.active'), '999999999\n')

      const current = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 1,
      })
      expect(existsSync(stale.runDirectory)).toBe(true)
      expect(existsSync(join(stale.runDirectory, '.active'))).toBe(false)
      expect(existsSync(current.runDirectory)).toBe(true)
      current.close()
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  test('does not inspect or alter stale markers owned by another namespace', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-rolling-namespace-'))
    try {
      const other = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'other-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 1,
      })
      const marker = join(other.runDirectory, '.active')
      writeFileSync(marker, '999999999\n')

      const current = new RollingLogWriter({
        rootDirectory: root,
        namespace: 'test-log',
        segmentMaxBytes: 100,
        retainSegments: 2,
        retainRuns: 1,
      })
      expect(readFileSync(marker, 'utf8')).toBe('999999999\n')
      expect(existsSync(other.runDirectory)).toBe(true)
      current.close()
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})

describe('RendererTrace rolling storage', () => {
  test('is enabled by default and can be explicitly disabled', () => {
    const previous = process.env.EVOT_TUI_TRACE
    delete process.env.EVOT_TUI_TRACE
    try {
      expect(new RendererTrace().isEnabled).toBe(true)
      process.env.EVOT_TUI_TRACE = '0'
      const disabled = new RendererTrace()
      expect(disabled.isEnabled).toBe(false)
      disabled.bind('disabled')
      expect(disabled.filePath).toBeNull()
    } finally {
      if (previous === undefined) delete process.env.EVOT_TUI_TRACE
      else process.env.EVOT_TUI_TRACE = previous
    }
  })

  test('buffers before bind and writes compact frame patches to a managed run', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-renderer-run-'))
    try {
      const trace = new RendererTrace({
        rootDirectory: root,
        segmentMaxBytes: 1_000_000,
        retainSegments: 3,
        retainRuns: 3,
      })
      const stdout = new TraceStdout() as unknown as NodeJS.WriteStream
      const renderer = new TermRenderer({ stdout, trace: entry => trace.log(entry) })
      let lines = ['history', 'prompt']
      renderer.setRenderCallback(() => lines)
      renderer.init()
      ;(renderer as any).doRender()
      lines = ['history', 'answer', 'prompt']
      ;(renderer as any).doRender()
      trace.bind('00000000-0000-0000-0000-000000000001')

      const run = trace.filePath
      expect(run).not.toBeNull()
      const names = segments(run ?? '')
      expect(names).toEqual(['000001.jsonl'])
      const entries = frameEntries(join(run ?? '', names[0]!))
      expect(entries).toHaveLength(2)
      expect(entries[0].branch).toBe('first_render')
      expect(entries[0].viewportTail).toEqual(['history', 'prompt'])
      expect(entries[1].viewportTail).toBeUndefined()
      expect(entries[1].viewportPatch).toEqual({ start: 1, lines: ['answer', 'prompt'] })
      expect(entries[0].terminal).toMatchObject({ columns: 40, rows: 6 })
      renderer.destroy()
      trace.close()
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  test('retained segments replay independently and tolerate a truncated final record', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-renderer-replay-'))
    try {
      const trace = new RendererTrace({
        rootDirectory: root,
        segmentMaxBytes: 2_000,
        retainSegments: 2,
        retainRuns: 2,
      })
      trace.bind('00000000-0000-0000-0000-000000000003')
      const stdout = new TraceStdout() as unknown as NodeJS.WriteStream
      const renderer = new TermRenderer({
        stdout,
        trace: entry => trace.log(entry),
        synchronizedOutput: true,
      })
      let lines = ['history 0', 'history 1', 'body', '────────', '❯ ', '────────']
      renderer.setRenderCallback(() => lines)
      renderer.init()
      ;(renderer as any).doRender()
      for (let frame = 1; frame <= 20; frame++) {
        lines = [
          'history 0',
          'history 1',
          ...Array.from({ length: frame }, (_, index) => `stream ${index}`),
          '────────',
          '❯ ',
          '────────',
        ]
        ;(renderer as any).doRender()
      }
      renderer.destroy()
      trace.close()

      const run = trace.filePath ?? ''
      const names = segments(run)
      expect(names).toHaveLength(2)
      expect(names).not.toContain('000001.jsonl')

      const replay = analyzeRun(run)
      expect(replay.exitCode).toBe(0)
      expect(replay.stderr).not.toContain('Invalid JSONL')
      expect(JSON.parse(replay.stdout).firstReplayMismatch).toBeNull()

      appendFileSync(join(run, names.at(-1)!), '{"kind":')
      const crashReplay = analyzeRun(run)
      expect(crashReplay.exitCode).toBe(0)
      expect(JSON.parse(crashReplay.stdout).firstReplayMismatch).toBeNull()
      expect(JSON.parse(crashReplay.stdout).truncatedTailRecords).toBe(1)

      appendFileSync(join(run, names.at(-1)!), '\n')
      const corruptReplay = analyzeRun(run)
      expect(corruptReplay.exitCode).not.toBe(0)
      expect(corruptReplay.stderr).toContain('Invalid JSONL')
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })

  test('pre-bind overflow and session switch both start from replayable checkpoints', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-renderer-checkpoints-'))
    try {
      const trace = new RendererTrace({
        rootDirectory: root,
        segmentMaxBytes: 1_000_000,
        retainSegments: 2,
        retainRuns: 2,
      })
      const stdout = new TraceStdout() as unknown as NodeJS.WriteStream
      const renderer = new TermRenderer({ stdout, trace: entry => trace.log(entry) })
      let lines = ['frame 0', '────────', '❯ ', '────────']
      renderer.setRenderCallback(() => lines)
      renderer.init()
      for (let frame = 0; frame < 300; frame++) {
        lines = [`frame ${frame}`, '────────', '❯ ', '────────']
        ;(renderer as any).doRender()
      }

      trace.bind('00000000-0000-0000-0000-000000000004')
      const overflowRun = trace.filePath ?? ''
      const overflowRecords = readFileSync(join(overflowRun, segments(overflowRun)[0]!), 'utf8')
        .trim().split('\n').map(line => JSON.parse(line))
      expect(overflowRecords[0]?.branch).toBe('segment_checkpoint')
      expect(overflowRecords.some(entry => entry.kind === 'buffer_overflow')).toBe(true)
      expect(analyzeRun(overflowRun).exitCode).toBe(0)

      lines = ['after bind', '────────', '❯ ', '────────']
      ;(renderer as any).doRender()
      trace.bind('00000000-0000-0000-0000-000000000005')
      const switchedRun = trace.filePath ?? ''
      expect(switchedRun).not.toBe(overflowRun)
      const switchedEntries = frameEntries(join(switchedRun, segments(switchedRun)[0]!))
      expect(switchedEntries[0]?.branch).toBe('segment_checkpoint')
      expect(analyzeRun(switchedRun).exitCode).toBe(0)

      renderer.destroy()
      trace.close()
    } finally {
      rmSync(root, { recursive: true, force: true })
    }
  })
})

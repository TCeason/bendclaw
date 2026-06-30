/**
 * Tests for the distill progress reporter — verbosity gating and event lines.
 */

import { test, expect, spyOn } from 'bun:test'
import { mkdtemp, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Reporter } from '../src/distill/internal/progress.js'

function capture(fn: () => void): string[] {
  const lines: string[] = []
  const spy = spyOn(console, 'log').mockImplementation((...a: unknown[]) => {
    lines.push(a.join(' '))
  })
  try {
    fn()
  } finally {
    spy.mockRestore()
  }
  return lines
}

test('normal: one line per result, no stage/event noise', () => {
  const lines = capture(() => {
    const r = new Reporter('normal', 2)
    r.taskStart('t1')
    r.stage('t1', 'solver')
    r.agentEvent('t1', 'tool_started', { tool_name: 'bash', preview_command: 'ls' })
    r.taskOk('t1', 0)
    r.taskStart('t2')
    r.taskDrop('t2', 'verifier failed after solve')
  })
  expect(lines.length).toBe(2)
  expect(lines[0]).toContain('[ok]')
  expect(lines[0]).toContain('(1/2) t1')
  expect(lines[1]).toContain('[drop]')
  expect(lines[1]).toContain('verifier failed after solve')
})

test('quiet: no per-task lines', () => {
  const lines = capture(() => {
    const r = new Reporter('quiet', 1)
    r.taskStart('t1')
    r.stage('t1', 'solver')
    r.taskOk('t1', 0)
  })
  expect(lines.length).toBe(0)
})

test('verbose: emits stage and agent-event lines', () => {
  const lines = capture(() => {
    const r = new Reporter('verbose', 1)
    r.taskStart('t1')
    r.stage('t1', 'builder')
    r.agentEvent('t1', 'tool_started', { tool_name: 'bash', preview_command: 'pytest -q' })
    r.agentEvent('t1', 'tool_finished', { tool_name: 'bash', is_error: false })
    r.agentEvent('t1', 'assistant_completed', { content: [{ type: 'tool_call' }] })
    r.taskOk('t1', 0)
  })
  const blob = lines.join('\n')
  expect(blob).toContain('task t1')
  expect(blob).toContain('[builder]')
  expect(blob).toContain('→ bash: pytest -q')
  expect(blob).toContain('✓ bash')
  expect(blob).toContain('assistant (1 tool call)')
  expect(blob).toContain('[ok] t1')
})

test('verbose: skips uninteresting events', () => {
  const lines = capture(() => {
    const r = new Reporter('verbose', 1)
    r.agentEvent('t1', 'assistant_delta', { delta: 'x' })
    r.agentEvent('t1', 'llm_call_started', {})
  })
  expect(lines.length).toBe(0)
})

test('mirrors diagnostics to log file even when terminal is quiet', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'distill-log-'))
  const logPath = join(dir, 'logs', 'distill-test.log')
  try {
    const lines = capture(() => {
      const r = new Reporter('quiet', 1, logPath)
      r.phase('log file: ' + logPath)
      r.taskStart('t1')
      r.stage('t1', 'solver')
      r.agentEvent('t1', 'tool_started', { tool_name: 'bash', preview_command: 'pytest -q' })
      r.taskDrop('t1', 'failed')
      r.summary('done')
    })
    expect(lines).toEqual(['done'])
    const body = await readFile(logPath, 'utf8')
    expect(body).toContain('# evot distill log')
    expect(body).toContain('==> log file: ' + logPath)
    expect(body).toContain('┌─ task t1')
    expect(body).toContain('[solver]')
    expect(body).toContain('→ bash: pytest -q')
    expect(body).toContain('[drop]')
    expect(body).toContain('done')
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

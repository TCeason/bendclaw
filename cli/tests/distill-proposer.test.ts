/**
 * Tests for task normalization at the distill boundary: the proposer/loader may
 * receive snake_case fields from an LLM or curated file, but the pipeline uses
 * a canonical camelCase TaskSpec internally.
 */

import { test, expect } from 'bun:test'
import { mkdtemp, writeFile, rm, chmod } from 'node:fs/promises'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { loadTasks, proposeTasks } from '../src/distill/internal/proposer.js'

test('loadTasks accepts snake_case builder_prompt and protected_paths', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'prop-'))
  const file = join(dir, 'tasks.jsonl')
  try {
    await writeFile(file, JSON.stringify({
      id: 't1',
      prompt: 'solve',
      answer: 'done',
      workspace: {
        source: 'agent_scaffold',
        builder_prompt: 'create files',
        setup: ['mkdir -p /workspace/tests', 'touch /workspace/tests/test_app.py'],
      },
      verifier: { check_command: 'python /workspace/tests/test_app.py', expected_exit_code: 0 },
      protected_paths: ['tests/**'],
    }) + '\n')

    const [t] = await loadTasks(file)
    expect(t.workspace.source).toBe('agent_scaffold')
    if (t.workspace.source !== 'agent_scaffold') throw new Error('wrong source')
    expect(t.workspace.builderPrompt).toBe('create files')
    expect(t.workspace.setup).toEqual(['mkdir -p tests', 'touch tests/test_app.py'])
    expect(t.verifier).toEqual({ checkCommand: 'python tests/test_app.py', expectedExitCode: 0 })
    expect(t.protectedPaths).toEqual(['tests/**'])
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

test('proposeTasks retries failures and falls back instead of returning empty', async () => {
  const dir = await mkdtemp(join(tmpdir(), 'prop-'))
  const fake = join(dir, 'fake-evot')
  try {
    await writeFile(fake, `#!/usr/bin/env bash
printf '%s\n' '{"kind":"error","payload":{"message":"Empty response from model"}}'
printf '%s\n' '{"kind":"run_finished","payload":{}}'
`, 'utf8')
    await chmod(fake, 0o755)
    const seen: string[] = []
    const tasks = await proposeTasks(
      { domain: 'python flask backend', n: 2 },
      fake,
      dir,
      8,
      undefined,
      undefined,
      { phaseEvent: () => {}, phase: (m) => seen.push(m) },
    )
    expect(tasks.length).toBe(2)
    expect(tasks.every((t) => t.source === 'evot_fallback')).toBe(true)
    expect(seen.filter((m) => m.includes('proposer attempt')).length).toBeGreaterThanOrEqual(3)
    expect(seen.some((m) => m.includes('proposer fallback'))).toBe(true)
  } finally {
    await rm(dir, { recursive: true, force: true })
  }
})

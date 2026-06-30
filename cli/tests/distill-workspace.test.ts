/**
 * Tests for workspace materialization + the self-check gate (git patch
 * apply/revert). These touch the filesystem and git but need no model.
 */

import { test, expect } from 'bun:test'
import { mkdtemp, mkdir, readFile, writeFile, rm, chmod } from 'node:fs/promises'
import { spawnSync } from 'node:child_process'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { materialize, runSetup } from '../src/distill/internal/workspace.js'
import { gitInit, selfCheck, runVerifier, runVerifierDetailed, changedPaths } from '../src/distill/internal/verifier.js'
import type { TaskSpec } from '../src/distill/internal/types.js'

test('materialize inline writes nested files', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'mat-'))
  try {
    const task: TaskSpec = {
      id: 'm1',
      prompt: '',
      answer: '',
      workspace: { source: 'inline', files: { 'pkg/a.py': 'x=1\n', 'b.txt': 'hi' } },
      verifier: { checkCommand: 'true' },
    }
    await materialize(task, dst)
    expect(await readFile(join(dst, 'pkg', 'a.py'), 'utf8')).toBe('x=1\n')
    expect(await readFile(join(dst, 'b.txt'), 'utf8')).toBe('hi')
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('runSetup runs commands in workspace', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'mat-'))
  try {
    const task: TaskSpec = {
      id: 'm2',
      prompt: '',
      answer: '',
      workspace: { source: 'inline', files: {}, setup: ['echo ready > marker.txt'] },
      verifier: { checkCommand: 'true' },
    }
    await materialize(task, dst)
    runSetup(task, dst)
    expect((await readFile(join(dst, 'marker.txt'), 'utf8')).trim()).toBe('ready')
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('selfCheck passes when reference patch toggles the verifier', async () => {
  if (!hasGit()) return
  const dst = await mkdtemp(join(tmpdir(), 'sc-'))
  try {
    // Base: a test that fails until expected.txt says PASS.
    await writeFile(join(dst, 'expected.txt'), 'FAIL\n', 'utf8')
    await writeFile(join(dst, 'check.sh'), 'grep -q PASS expected.txt\n', 'utf8')
    gitInit(dst)

    // Reference patch flips FAIL -> PASS. Use a clean unified diff (defeating
    // any user global diff driver) so `git apply` can consume it.
    await writeFile(join(dst, 'expected.txt'), 'PASS\n', 'utf8')
    const patch = spawnSync(
      'git',
      ['-C', dst, 'diff', '--no-ext-diff', '--no-color', '-U3'],
      { encoding: 'utf8' },
    ).stdout
    // restore base working tree
    spawnSync('git', ['-C', dst, 'checkout', '--', '.'], { encoding: 'utf8' })

    const task: TaskSpec = {
      id: 'sc1',
      prompt: '',
      answer: '',
      workspace: { source: 'inline', files: {} },
      verifier: { checkCommand: 'bash check.sh', expectedExitCode: 0 },
      referencePatch: patch,
    }

    // Base must fail before the patch.
    expect(runVerifier(task.verifier, dst)).toBe(false)

    const res = await selfCheck(task, dst)
    expect(res.baseFails).toBe(true)
    expect(res.referencePasses).toBe(true)
    expect(res.ok).toBe(true)

    // selfCheck must leave the workspace back at base state.
    expect((await readFile(join(dst, 'expected.txt'), 'utf8')).trim()).toBe('FAIL')
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('selfCheck without reference only requires base to fail', async () => {
  if (!hasGit()) return
  const dst = await mkdtemp(join(tmpdir(), 'sc-'))
  try {
    await writeFile(join(dst, 'expected.txt'), 'FAIL\n', 'utf8')
    gitInit(dst)
    const task: TaskSpec = {
      id: 'sc2',
      prompt: '',
      answer: '',
      workspace: { source: 'inline', files: {} },
      verifier: { checkCommand: 'grep -q PASS expected.txt', expectedExitCode: 0 },
    }
    const res = await selfCheck(task, dst)
    expect(res.baseFails).toBe(true)
    expect(res.ok).toBe(true)
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('runVerifierDetailed returns exit code and output', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'vd-'))
  try {
    await writeFile(join(dst, 'out.txt'), 'nope\n', 'utf8')
    const fail = runVerifierDetailed(
      { checkCommand: 'grep PASS out.txt; echo "checked out.txt"', expectedExitCode: 0 },
      dst,
    )
    // grep finds nothing -> exit 1 from the pipeline's last? No: echo runs last
    // so exit is 0; assert we still capture output either way.
    expect(fail.output).toContain('checked out.txt')

    const real = runVerifierDetailed(
      { checkCommand: 'grep -q PASS out.txt', expectedExitCode: 0 },
      dst,
    )
    expect(real.passed).toBe(false)
    expect(real.exitCode).toBe(1)
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('setup and verifier prefer workspace .venv/bin on PATH', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'venv-'))
  try {
    const task: TaskSpec = {
      id: 'venv1',
      prompt: '',
      answer: '',
      workspace: { source: 'inline', files: {}, setup: ['python -m pip --version'] },
      verifier: { checkCommand: 'python -m pytest tests -q', expectedExitCode: 0 },
    }
    await materialize(task, dst)

    await mkdir(join(dst, '.venv', 'bin'), { recursive: true })
    const fakePython = join(dst, '.venv', 'bin', 'python')
    await writeFile(fakePython, '#!/usr/bin/env bash\necho venv-python "$@" > used_python.txt\n', 'utf8')
    await chmod(fakePython, 0o755)

    runSetup(task, dst)
    expect((await readFile(join(dst, 'used_python.txt'), 'utf8')).trim()).toBe('venv-python -m pip --version')

    await writeFile(join(dst, 'used_python.txt'), '', 'utf8')
    const res = runVerifierDetailed(task.verifier, dst)
    expect(res.passed).toBe(true)
    expect((await readFile(join(dst, 'used_python.txt'), 'utf8')).trim()).toBe('venv-python -m pytest tests -q')
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('runSetup prepares a standard Python runtime before task setup', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'runtime-'))
  try {
    const task: TaskSpec = {
      id: 'rt1',
      prompt: '',
      answer: '',
      workspace: {
        source: 'inline',
        files: { 'requirements.txt': '' },
        setup: ['python -c "import sys; open(\'runtime.txt\', \'w\').write(sys.executable)"'],
      },
      verifier: { checkCommand: 'python -c "import sys; assert \'.venv\' in sys.executable"' },
    }
    await materialize(task, dst)
    runSetup(task, dst)
    expect((await readFile(join(dst, 'runtime.txt'), 'utf8')).includes('.venv')).toBe(true)
    expect(runVerifier(task.verifier, dst)).toBe(true)
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('runSetup installs pytest when the verifier uses it, even without requirements.txt', async () => {
  const dst = await mkdtemp(join(tmpdir(), 'pytest-rt-'))
  try {
    const task: TaskSpec = {
      id: 'rt2',
      prompt: '',
      answer: '',
      workspace: {
        source: 'inline',
        // Standard-library task: no requirements.txt at all.
        files: {
          'app.py': 'def add(a, b):\n    return a + b\n',
          'verify/test_app.py': 'from app import add\n\n\ndef test_add():\n    assert add(2, 3) == 5\n',
        },
      },
      verifier: { checkCommand: 'python -m pytest verify -q', expectedExitCode: 0 },
    }
    await materialize(task, dst)
    runSetup(task, dst)
    // pytest is part of the verification harness, so the runtime must provide it.
    expect(runVerifier(task.verifier, dst)).toBe(true)
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

function hasGit(): boolean {
  return spawnSync('git', ['--version'], { encoding: 'utf8' }).status === 0
}

test('changedPaths reports files the solver edited since the base commit', async () => {
  if (!hasGit()) return
  const dst = await mkdtemp(join(tmpdir(), 'changed-'))
  try {
    const task: TaskSpec = {
      id: 'cp1',
      prompt: '',
      answer: '',
      workspace: {
        source: 'inline',
        files: { 'app.py': 'x = 1\n', 'verify/test_app.py': 'def test():\n    assert True\n' },
      },
      verifier: { checkCommand: 'true' },
    }
    await materialize(task, dst)
    gitInit(dst)
    // No edits yet.
    expect(changedPaths(dst)).toEqual([])
    // Solver edits both a source file and a protected test file.
    await writeFile(join(dst, 'app.py'), 'x = 2\n')
    await writeFile(join(dst, 'verify/test_app.py'), 'def test():\n    assert 1 == 1\n')
    const changed = changedPaths(dst)
    expect(changed).toContain('app.py')
    expect(changed).toContain('verify/test_app.py')
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

test('changedPaths ignores transient artifacts generated by running the verifier', async () => {
  if (!hasGit()) return
  const dst = await mkdtemp(join(tmpdir(), 'transient-'))
  try {
    const task: TaskSpec = {
      id: 'cp2',
      prompt: '',
      answer: '',
      workspace: {
        source: 'inline',
        files: { 'app.py': 'x = 1\n', 'verify/test_app.py': 'def test():\n    assert True\n' },
      },
      verifier: { checkCommand: 'true' },
    }
    await materialize(task, dst)
    gitInit(dst)
    // Simulate what running pytest does: drop bytecode caches + a venv into the
    // workspace. None of these are solver edits, so the integrity gate must not
    // see them — the same way a real project's .gitignore hides build output.
    await mkdir(join(dst, 'verify', '__pycache__'), { recursive: true })
    await writeFile(join(dst, 'verify', '__pycache__', 'test_app.cpython-314.pyc'), 'bytecode')
    await mkdir(join(dst, '__pycache__'), { recursive: true })
    await writeFile(join(dst, '__pycache__', 'app.cpython-314.pyc'), 'bytecode')
    await mkdir(join(dst, '.venv', 'bin'), { recursive: true })
    await writeFile(join(dst, '.venv', 'bin', 'python'), '#!/bin/sh\n')
    expect(changedPaths(dst)).toEqual([])
  } finally {
    await rm(dst, { recursive: true, force: true })
  }
})

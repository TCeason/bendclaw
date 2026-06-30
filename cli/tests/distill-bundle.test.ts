/**
 * Tests for the distill bundle writer — the delivery contract: bundle-relative
 * workspace paths, manifest with counts + integrity, downstream-ready rows.
 */

import { test, expect } from 'bun:test'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { Bundle } from '../src/distill/internal/bundle.js'
import type { TaskSpec } from '../src/distill/internal/types.js'

function task(id: string): TaskSpec {
  return {
    id,
    prompt: 'add pagination',
    answer: 'paginated',
    workspace: { source: 'inline', files: {}, setup: ['pip install -e .'] },
    verifier: { checkCommand: 'pytest -q', expectedExitCode: 0 },
    source: 'evot_auto',
  }
}

test('writes bundle with relative workspace paths and manifest', async () => {
  const out = await mkdtemp(join(tmpdir(), 'bundle-'))
  const wsSrc = await mkdtemp(join(tmpdir(), 'ws-'))
  await mkdir(join(wsSrc, 'app'), { recursive: true })
  await writeFile(join(wsSrc, 'app', 'main.py'), 'print(1)\n', 'utf8')

  try {
    const b = new Bundle(out, { evotVersion: '9.9.9', teacherModel: 'test-model' })
    b.addSft([{ messages: [{ role: 'user', content: 'hi' }], tools: [], metadata: {} }])
    await b.addRl(task('t1'), wsSrc)
    const summary = await b.write(['sft', 'rl'])

    expect(summary).toContain('sft=1')
    expect(summary).toContain('rl=1')

    // RL row references a bundle-relative workspace path.
    const rlBody = await readFile(join(out, 'teacher_rl.jsonl'), 'utf8')
    const rlRow = JSON.parse(rlBody.trim())
    expect(rlRow.metadata.workspace).toBe('workspaces/teacher_t1')
    expect(rlRow.metadata.verifier.check_command).toBe('bash .evot/verify.sh && echo VERIFY_PASS')
    expect(rlRow.metadata.verifier.check_contains).toEqual(['VERIFY_PASS'])
    expect(rlRow.metadata.verifier.type).toBe('bash')
    expect(rlRow.metadata.setup).toEqual([])
    expect(rlRow.metadata.prepare).toBe('scripts/prepare.sh')
    expect(rlRow.metadata.execution).toMatchObject({
      contract: 'evot.workspace.v1',
      verify_script: '.evot/verify.sh',
      shared_runtime: '.venv',
      original_setup_count: 1,
      setup_effects_snapshotted: true,
      original_verifier: 'pytest -q',
    })
    expect(rlRow.id).toBe('teacher_t1')

    // Workspace files copied into the bundle.
    const copied = await readFile(join(out, 'workspaces', 'teacher_t1', 'app', 'main.py'), 'utf8')
    expect(copied).toBe('print(1)\n')

    // No per-workspace setup script: the shared dataset env handles install.
    const verifyScript = await readFile(join(out, 'workspaces', 'teacher_t1', '.evot', 'verify.sh'), 'utf8')
    expect(verifyScript).toContain('pytest -q')

    // One shared dataset env, built once, symlinked into every workspace.
    const prepareScript = await readFile(join(out, 'scripts', 'prepare.sh'), 'utf8')
    expect(prepareScript).toContain('python3 -m venv .venv')
    expect(prepareScript).toContain('ln -s ../../.venv "$ws/.venv"')
    // pytest is the shared verification harness (verifier uses pytest here).
    expect(prepareScript).toContain('.venv/bin/python -m pip install -q pytest')

    // Manifest records counts, sources, and file hashes.
    const manifest = JSON.parse(await readFile(join(out, 'manifest.json'), 'utf8'))
    expect(manifest.schema).toBe('evot.distill.bundle.v1')
    expect(manifest.counts).toEqual({ sft_rows: 1, rl_rows: 1, workspaces: 1 })
    expect(manifest.sources).toEqual({ evot_auto: 1 })
    expect(manifest.scripts).toEqual({ prepare: 'scripts/prepare.sh' })
    expect(manifest.teacher_model).toBe('test-model')
    expect(manifest.files['teacher_rl.jsonl'].sha256).toMatch(/^[0-9a-f]{64}$/)
    // app/main.py + .evot/verify.sh (no per-workspace setup.sh under shared env).
    expect(manifest.workspaces['teacher_t1'].files).toBe(2)
  } finally {
    await rm(out, { recursive: true, force: true })
    await rm(wsSrc, { recursive: true, force: true })
  }
})

test('excludes transient artifacts from copied workspace', async () => {
  const out = await mkdtemp(join(tmpdir(), 'bundle-'))
  const wsSrc = await mkdtemp(join(tmpdir(), 'ws-'))
  await mkdir(join(wsSrc, '.git'), { recursive: true })
  await writeFile(join(wsSrc, '.git', 'HEAD'), 'ref\n', 'utf8')
  await mkdir(join(wsSrc, 'node_modules'), { recursive: true })
  await writeFile(join(wsSrc, 'node_modules', 'x.js'), '1\n', 'utf8')
  await mkdir(join(wsSrc, '.venv', 'bin'), { recursive: true })
  await writeFile(join(wsSrc, '.venv', 'bin', 'python'), 'x\n', 'utf8')
  await mkdir(join(wsSrc, '__pycache__'), { recursive: true })
  await writeFile(join(wsSrc, '__pycache__', 'app.pyc'), 'x\n', 'utf8')
  await mkdir(join(wsSrc, '.pytest_cache'), { recursive: true })
  await writeFile(join(wsSrc, '.pytest_cache', 'README.md'), 'x\n', 'utf8')
  await writeFile(join(wsSrc, 'keep.py'), 'ok\n', 'utf8')

  try {
    const b = new Bundle(out, { evotVersion: '1', teacherModel: 'm' })
    await b.addRl(task('t2'), wsSrc)
    await b.write(['rl'])

    const wsDir = join(out, 'workspaces', 'teacher_t2')
    expect(await readFile(join(wsDir, 'keep.py'), 'utf8')).toBe('ok\n')
    expect(await fileExists(join(wsDir, '.git', 'HEAD'))).toBe(false)
    expect(await fileExists(join(wsDir, 'node_modules', 'x.js'))).toBe(false)
    expect(await fileExists(join(wsDir, '.venv', 'bin', 'python'))).toBe(false)
    expect(await fileExists(join(wsDir, '__pycache__', 'app.pyc'))).toBe(false)
    expect(await fileExists(join(wsDir, '.pytest_cache', 'README.md'))).toBe(false)
  } finally {
    await rm(out, { recursive: true, force: true })
    await rm(wsSrc, { recursive: true, force: true })
  }
})

test('writes a stable execution contract for Python workspaces', async () => {
  const out = await mkdtemp(join(tmpdir(), 'bundle-'))
  const wsSrc = await mkdtemp(join(tmpdir(), 'ws-'))
  await writeFile(join(wsSrc, 'requirements.txt'), 'pytest\n', 'utf8')
  try {
    const t = task('py1')
    t.workspace.setup = ['mkdir -p tests']
    t.verifier.checkCommand = '.venv/bin/python -m pytest tests -q'
    const b = new Bundle(out, { evotVersion: '1', teacherModel: 'm' })
    await b.addRl(t, wsSrc)
    await b.write(['rl'])
    const row = JSON.parse((await readFile(join(out, 'teacher_rl.jsonl'), 'utf8')).trim())
    expect(row.metadata.setup).toEqual([])
    expect(row.metadata.prepare).toBe('scripts/prepare.sh')
    expect(row.metadata.verifier.check_command).toBe('bash .evot/verify.sh && echo VERIFY_PASS')
    expect(row.metadata.verifier.check_contains).toEqual(['VERIFY_PASS'])
    expect(row.metadata.execution.original_verifier).toBe('.venv/bin/python -m pytest tests -q')
    expect(row.metadata.execution.shared_runtime).toBe('.venv')
    // The shared dataset env installs the union of workspace requirements once.
    const prepareScript = await readFile(join(out, 'scripts', 'prepare.sh'), 'utf8')
    expect(prepareScript).toContain('python3 -m venv .venv')
    expect(prepareScript).toContain('for req in workspaces/*/requirements.txt; do')
    expect(prepareScript).toContain('ln -s ../../.venv "$ws/.venv"')
    // No per-workspace setup script exists anymore.
    expect(await fileExists(join(out, 'workspaces', 'teacher_py1', '.evot', 'setup.sh'))).toBe(false)
    const verifyScript = await readFile(join(out, 'workspaces', 'teacher_py1', '.evot', 'verify.sh'), 'utf8')
    expect(verifyScript).toContain('export PATH="$PWD/.venv/bin:$PWD/node_modules/.bin:$PATH"')
    expect(verifyScript).toContain('.venv/bin/python -m pytest tests -q')
  } finally {
    await rm(out, { recursive: true, force: true })
    await rm(wsSrc, { recursive: true, force: true })
  }
})

async function fileExists(p: string): Promise<boolean> {
  return readFile(p).then(() => true).catch(() => false)
}

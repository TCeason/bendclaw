/**
 * bundle — the delivery format. A DatasetBundle is a self-contained directory
 * the downstream trainer consumes with zero post-processing:
 *
 *   <out>/teacher_sft.jsonl     SFT rows
 *   <out>/teacher_rl.jsonl      RL rows (metadata.workspace is bundle-relative)
 *   <out>/workspaces/<id>/      each RL task's initial workspace
 *   <out>/patches/<id>.patch    reference solutions (optional, not trained on)
 *   <out>/manifest.json         counts + sha256 integrity + provenance
 *
 * Invariants enforced here:
 *  - workspace paths are RELATIVE to the bundle root (portable across machines)
 *  - workspaces hold only initial state; deps are rebuilt downstream via `setup`
 *  - manifest records sha256 so the loader can verify integrity
 */

import { createHash } from 'node:crypto'
import { cp, mkdir, readFile, readdir, rm, writeFile, stat, chmod } from 'node:fs/promises'
import { join } from 'node:path'
import type { RlRow, SftRow, TaskSpec } from './types.js'
import { isTransientPath } from './artifacts.js'

export class Bundle {
  private sftRows: SftRow[] = []
  private rlRows: RlRow[] = []
  private sources: Record<string, number> = {}
  private workspaceRels: string[] = []
  // Whether any task's verifier uses pytest, so the one shared dataset env
  // installs the harness exactly once (not per workspace).
  private needsPytest = false

  constructor(
    private root: string,
    private meta: { evotVersion: string; teacherModel: string },
  ) {}

  addSft(rows: SftRow[]): void {
    this.sftRows.push(...rows)
  }

  /**
   * Register an RL task: copy its frozen workspace into the bundle and emit a
   * row whose `metadata.workspace` is a bundle-relative path.
   */
  async addRl(task: TaskSpec, workspaceDir: string): Promise<void> {
    const wsRel = join('workspaces', `teacher_${task.id}`)
    this.workspaceRels.push(wsRel)
    const wsAbs = join(this.root, wsRel)
    await rm(wsAbs, { recursive: true, force: true })
    await mkdir(wsAbs, { recursive: true })
    await cp(workspaceDir, wsAbs, {
      recursive: true,
      filter: (src) => !isTransientPath(src),
    })

    if (task.referencePatch) {
      const pAbs = join(this.root, 'patches', `${task.id}.patch`)
      await mkdir(join(this.root, 'patches'), { recursive: true })
      await writeFile(pAbs, task.referencePatch, 'utf8')
    }

    const src = task.source ?? 'evot_auto'
    this.sources[src] = (this.sources[src] ?? 0) + 1
    if (/\bpytest\b/.test(task.verifier.checkCommand)) this.needsPytest = true
    const execution = await writeExecutionContract(task, wsAbs)

    this.rlRows.push({
      id: `teacher_${task.id}`,
      prompt: [{ role: 'user', content: task.prompt }],
      label: { answer: task.answer },
      metadata: {
        task_type: 'teacher_rl',
        tool_policy: 'required',
        source: src,
        target_turns: task.targetTurns,
        workspace: wsRel,
        setup: [],
        prepare: 'scripts/prepare.sh',
        verifier: {
          type: 'bash',
          // LocalRL judges success by stdout-contains, not raw exit code. The
          // verify script echoes VERIFY_PASS only after the underlying command
          // exits 0, so `&&` makes a partial failure (e.g. "1 failed, 2 passed")
          // correctly score as not-passing without LocalRL needing exit codes.
          check_command: 'bash .evot/verify.sh && echo VERIFY_PASS',
          expected_exit_code: task.verifier.expectedExitCode ?? 0,
          check_contains: ['VERIFY_PASS'],
          timeout: 60,
        },
        execution,
      },
    })
  }

  /** Write all jsonl files + manifest. Returns a short summary string. */
  async write(emit: ('sft' | 'rl')[]): Promise<string> {
    await mkdir(this.root, { recursive: true })
    const files: Record<string, { rows: number; sha256: string }> = {}
    const scripts = emit.includes('rl') ? await this.writePrepareScript() : undefined

    if (emit.includes('sft')) {
      const sftPath = join(this.root, 'teacher_sft.jsonl')
      await writeJsonl(sftPath, this.sftRows)
      files['teacher_sft.jsonl'] = { rows: this.sftRows.length, sha256: await sha256(sftPath) }
    }
    if (emit.includes('rl')) {
      const rlPath = join(this.root, 'teacher_rl.jsonl')
      await writeJsonl(rlPath, this.rlRows)
      files['teacher_rl.jsonl'] = { rows: this.rlRows.length, sha256: await sha256(rlPath) }
    }

    const workspaces = await this.hashWorkspaces()
    const manifest = {
      schema: 'evot.distill.bundle.v1',
      created_at: new Date().toISOString(),
      evot_version: this.meta.evotVersion,
      teacher_model: this.meta.teacherModel,
      counts: {
        sft_rows: this.sftRows.length,
        rl_rows: this.rlRows.length,
        workspaces: Object.keys(workspaces).length,
      },
      sources: this.sources,
      ...(scripts ? { scripts } : {}),
      files,
      workspaces,
    }
    await writeFile(join(this.root, 'manifest.json'), JSON.stringify(manifest, null, 2), 'utf8')

    return `sft=${this.sftRows.length} rl=${this.rlRows.length} workspaces=${Object.keys(workspaces).length}`
  }

  private async writePrepareScript(): Promise<{ prepare: string }> {
    const scriptsDir = join(this.root, 'scripts')
    await mkdir(scriptsDir, { recursive: true })
    const scriptPath = join(scriptsDir, 'prepare.sh')
    // One shared dataset environment, built once. Every workspace points its
    // .venv at this single env via a relative symlink, so preparing the dataset
    // installs dependencies exactly once instead of per workspace.
    const lines = [
      '#!/usr/bin/env bash',
      'set -euo pipefail',
      'ROOT="$(cd "$(dirname "$0")/.." && pwd)"',
      'cd "$ROOT"',
      '',
      '# Build the single shared dataset runtime (one venv for all workspaces).',
      'python3 -m venv .venv',
      '.venv/bin/python -m pip install -q --upgrade pip >/dev/null 2>&1 || true',
    ]
    if (this.needsPytest) {
      lines.push('# pytest is the verification harness, shared by every task.',
        '.venv/bin/python -m pip install -q pytest')
    }
    lines.push(
      '# Install the union of all workspace requirements into the shared env.',
      'for req in workspaces/*/requirements.txt; do',
      '  [ -f "$req" ] && .venv/bin/python -m pip install -q -r "$req"',
      'done',
      '',
      '# Point every workspace at the one shared env via a relative symlink.',
      'for ws in workspaces/*/; do',
      '  ws="${ws%/}"',
      '  rm -rf "$ws/.venv"',
      '  ln -s ../../.venv "$ws/.venv"',
      'done',
      '',
      'echo "==> prepared shared dataset runtime for all workspaces"',
      '',
    )
    await writeFile(scriptPath, lines.join('\n'), 'utf8')
    await chmod(scriptPath, 0o755)
    return { prepare: 'scripts/prepare.sh' }
  }

  private async hashWorkspaces(): Promise<Record<string, { files: number; bytes: number }>> {
    const dir = join(this.root, 'workspaces')
    const out: Record<string, { files: number; bytes: number }> = {}
    let entries: string[] = []
    try {
      entries = await readdir(dir)
    } catch {
      return out
    }
    for (const name of entries) {
      const { files, bytes } = await dirStats(join(dir, name))
      out[name] = { files, bytes }
    }
    return out
  }
}

async function writeExecutionContract(task: TaskSpec, bundledWorkspace: string): Promise<Record<string, unknown>> {
  const evotDir = join(bundledWorkspace, '.evot')
  await mkdir(evotDir, { recursive: true })
  const verifyScript = join(evotDir, 'verify.sh')
  const setupCommands = task.workspace.setup ?? []

  // No per-workspace setup script: the shared dataset env (scripts/prepare.sh)
  // installs dependencies once and symlinks each workspace's .venv to it.
  await writeFile(verifyScript, buildVerifyScript(task.verifier.checkCommand), 'utf8')
  await chmod(verifyScript, 0o755)

  return {
    contract: 'evot.workspace.v1',
    verify_script: '.evot/verify.sh',
    shared_runtime: '.venv',
    original_setup_count: setupCommands.length,
    setup_effects_snapshotted: true,
    original_verifier: task.verifier.checkCommand,
  }
}

function buildVerifyScript(command: string): string {
  return [
    '#!/usr/bin/env bash',
    'set -euo pipefail',
    'cd "$(dirname "$0")/.."',
    'export PATH="$PWD/.venv/bin:$PWD/node_modules/.bin:$PATH"',
    'if [ -d "$PWD/.venv" ]; then export VIRTUAL_ENV="$PWD/.venv"; fi',
    '',
    '# Original verifier command, executed inside the standardized workspace runtime.',
    command,
    '',
  ].join('\n')
}

async function writeJsonl(path: string, rows: unknown[]): Promise<void> {
  const body = rows.map((r) => JSON.stringify(r)).join('\n') + (rows.length ? '\n' : '')
  await writeFile(path, body, 'utf8')
}

async function sha256(path: string): Promise<string> {
  const data = await readFile(path)
  return createHash('sha256').update(data).digest('hex')
}

async function dirStats(dir: string): Promise<{ files: number; bytes: number }> {
  let files = 0
  let bytes = 0
  const walk = async (d: string) => {
    for (const e of await readdir(d, { withFileTypes: true })) {
      const p = join(d, e.name)
      if (e.isDirectory()) await walk(p)
      else {
        files++
        bytes += (await stat(p)).size
      }
    }
  }
  await walk(dir).catch(() => {})
  return { files, bytes }
}

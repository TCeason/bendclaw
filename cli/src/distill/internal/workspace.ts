/**
 * workspace — materialize a TaskSpec's WorkspaceSource into a real directory.
 *
 * Every source resolves to the same thing: a freshly populated directory the
 * agent can operate in. The directory holds only the *initial* state (the task
 * starting point); the Solver's mutations are captured separately as a diff.
 *
 * Dependency installs (`setup`) are run once here so the agent starts from a
 * ready workspace, but installed artifacts (node_modules, .venv) are NOT part
 * of the delivered bundle — the bundle stores the `setup` commands instead.
 */

import { cp, mkdir, rm, writeFile } from 'node:fs/promises'
import { existsSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { spawnSync } from 'node:child_process'
import type { TaskSpec } from './types.js'
import { workspaceEnv } from './env.js'

/** Populate `dst` with the task's initial files. Returns nothing; throws on failure. */
export async function materialize(task: TaskSpec, dst: string): Promise<void> {
  await rm(dst, { recursive: true, force: true })
  await mkdir(dst, { recursive: true })

  const ws = task.workspace
  switch (ws.source) {
    case 'inline':
      await writeFiles(dst, ws.files)
      break
    case 'dir':
      await cp(ws.path, dst, { recursive: true })
      break
    case 'git_local':
    case 'git':
      checkoutGit(ws.repo, ws.ref, dst)
      break
    case 'agent_scaffold':
      // The Builder agent populates this directory in a separate step
      // (see builder.ts). Nothing to copy here — start empty.
      break
  }
}

/** Run the task's `setup` commands in `dst`. Throws if any command fails. */
export function runSetup(task: TaskSpec, dst: string): void {
  prepareWorkspaceRuntime(dst, { needsPytest: verifierUsesPytest(task) })
  const setup = task.workspace.setup ?? []
  for (const cmd of setup) {
    const r = spawnSync('bash', ['-c', cmd], { cwd: dst, env: workspaceEnv(dst), encoding: 'utf8' })
    if (r.status !== 0) {
      throw new Error(`setup failed (${cmd}): ${r.stderr || r.stdout}`)
    }
  }
}

/** Whether a task's verifier runs pytest (so the runtime must provide it). */
export function verifierUsesPytest(task: TaskSpec): boolean {
  return /\bpytest\b/.test(task.verifier.checkCommand)
}

/** Create a local workspace runtime before model-authored setup/verifier commands run.
 *
 * pytest is part of the verification harness, not a project dependency, so the
 * runtime installs it whenever the verifier uses it — even for standard-library
 * tasks that ship no requirements.txt. Otherwise `python -m pytest` resolves to
 * a venv without pytest and every such task fails its own verifier.
 */
export function prepareWorkspaceRuntime(dst: string, opts: { needsPytest?: boolean } = {}): void {
  const hasRequirements = existsSync(join(dst, 'requirements.txt'))
  const needsPytest = opts.needsPytest ?? false
  if (!hasRequirements && !needsPytest) return
  const steps = ['python3 -m venv .venv']
  if (hasRequirements) steps.push('.venv/bin/python -m pip install -q -r requirements.txt')
  if (needsPytest) steps.push('.venv/bin/python -m pip install -q pytest')
  const r = spawnSync('bash', ['-c', steps.join(' && ')], {
    cwd: dst,
    env: workspaceEnv(dst),
    encoding: 'utf8',
  })
  if (r.status !== 0) {
    throw new Error(`runtime setup failed: ${r.stderr || r.stdout}`)
  }
}

async function writeFiles(dst: string, files: Record<string, string>): Promise<void> {
  for (const [rel, content] of Object.entries(files)) {
    const p = join(dst, rel)
    await mkdir(dirname(p), { recursive: true })
    await writeFile(p, content, 'utf8')
  }
}

/**
 * Check out `repo` at `ref` into `dst` without the .git directory, using a
 * worktree-free shallow approach: clone to a temp, archive the ref, extract.
 * We never mutate the source repo's working tree.
 */
function checkoutGit(repo: string, ref: string | undefined, dst: string): void {
  const at = ref || 'HEAD'
  // `git archive` produces a clean tree at the ref with no history or .git.
  const archive = spawnSync(
    'bash',
    ['-c', `git -C ${shellQuote(repo)} archive ${shellQuote(at)} | tar -x -C ${shellQuote(dst)}`],
    { encoding: 'utf8' },
  )
  if (archive.status !== 0) {
    throw new Error(`git archive ${repo}@${at} failed: ${archive.stderr}`)
  }
}

function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`
}

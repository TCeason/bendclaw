/**
 * verifier — the objective quality gate. No LLM judging here; correctness is
 * decided by running the task's check command and observing the exit code.
 *
 * Two gates:
 *  - runVerifier: did the Solver's final workspace pass? (decides SFT/RL keep)
 *  - selfCheck:   is the task itself valid? Applying the reference patch must
 *                 make the verifier pass, and reverting it must make it fail.
 *                 This catches Proposer/Builder mistakes (unsolvable tasks, or
 *                 tasks that pass without any work) with zero human review.
 */

import { spawnSync } from 'node:child_process'
import { writeFile, rm } from 'node:fs/promises'
import { join } from 'node:path'
import type { TaskSpec, Verifier } from './types.js'
import { workspaceEnv } from './env.js'

/** Run the verifier command in `cwd`. Returns whether it passed. */
export function runVerifier(v: Verifier, cwd: string, timeoutSec = 300): boolean {
  return runVerifierDetailed(v, cwd, timeoutSec).passed
}

export interface VerifierRun {
  passed: boolean
  exitCode: number
  output: string
}

/** Run the verifier and return exit code + combined output for diagnostics. */
export function runVerifierDetailed(v: Verifier, cwd: string, timeoutSec = 300): VerifierRun {
  const r = spawnSync('bash', ['-c', v.checkCommand], {
    cwd,
    env: workspaceEnv(cwd),
    encoding: 'utf8',
    timeout: timeoutSec * 1000,
  })
  const exitCode = r.status ?? 1
  const output = `${r.stdout ?? ''}${r.stderr ?? ''}`.trim()
  return { passed: exitCode === (v.expectedExitCode ?? 0), exitCode, output }
}

export interface SelfCheckResult {
  baseFails: boolean
  referencePasses: boolean
  ok: boolean
}

/**
 * Validate a task by toggling its reference patch.
 * Assumes `cwd` is the initial (unsolved) workspace and is a git repo so the
 * patch can be applied and reverted cleanly.
 */
export async function selfCheck(
  task: TaskSpec,
  cwd: string,
  timeoutSec = 300,
): Promise<SelfCheckResult> {
  // Without a reference solution we can only assert the weaker invariant:
  // the task isn't already done. (base must fail)
  if (!task.referencePatch) {
    const baseFails = !runVerifier(task.verifier, cwd, timeoutSec)
    return { baseFails, referencePasses: false, ok: baseFails }
  }

  const baseFails = !runVerifier(task.verifier, cwd, timeoutSec)

  const patchFile = join(cwd, '.evot_reference.patch')
  await writeFile(patchFile, task.referencePatch, 'utf8')
  const applied = git(cwd, ['apply', '.evot_reference.patch'])
  const referencePasses = applied && runVerifier(task.verifier, cwd, timeoutSec)
  if (applied) git(cwd, ['apply', '-R', '.evot_reference.patch'])
  await rm(patchFile, { force: true })

  return { baseFails, referencePasses, ok: baseFails && referencePasses }
}

/** Initialize a git repo in `cwd` so patches can be applied/reverted. */
export function gitInit(cwd: string): void {
  git(cwd, ['init', '-q'])
  git(cwd, ['add', '-A'])
  git(cwd, ['-c', 'user.email=d@e.f', '-c', 'user.name=distill', 'commit', '-qm', 'base'])
}

/** Capture the Solver's changes as a clean unified diff against the base commit. */
export function captureDiff(cwd: string): string {
  const r = spawnSync('git', ['-C', cwd, 'add', '-A'], { encoding: 'utf8' })
  if (r.status !== 0) return ''
  // --no-ext-diff/--no-color defeat any user global diff driver or colorizer,
  // so the output is a plain unified diff `git apply` can consume.
  const diff = spawnSync(
    'git',
    ['-C', cwd, 'diff', '--no-ext-diff', '--no-color', '-U3', '--cached', 'HEAD'],
    { encoding: 'utf8' },
  )
  return diff.stdout ?? ''
}

function git(cwd: string, args: string[]): boolean {
  const r = spawnSync('git', ['-C', cwd, ...args], { encoding: 'utf8' })
  return r.status === 0
}

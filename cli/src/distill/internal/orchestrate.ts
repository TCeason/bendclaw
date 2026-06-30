/**
 * orchestrate — run the full distill pipeline for a batch of tasks.
 *
 * Per task (Builder → self-check → Solver → verify → emit):
 *   1. materialize the initial workspace
 *   2. agent_scaffold: drive a Builder run to create the base project
 *   3. git-init + run setup, then self-check the task is valid
 *   4. drive a Solver run on the frozen workspace
 *   5. run the verifier; on pass, emit SFT rows + register the RL task
 *
 * Failures (build error, bad self-check, verifier fail, timeout) drop the task.
 * Concurrency is bounded so we don't exhaust API rate limits or local CPU.
 */

import { mkdtemp, rm, cp, mkdir, realpath } from 'node:fs/promises'
import { join } from 'node:path'
import type { DistillOptions, TaskSpec } from './types.js'
import { Bundle } from './bundle.js'
import { importEvents } from './importer.js'
import { proposeTasks, loadTasks } from './proposer.js'
import { runAgent } from './runner.js'
import { materialize, runSetup } from './workspace.js'
import { captureDiff, gitInit, runVerifierDetailed, selfCheck } from './verifier.js'
import { Reporter } from './progress.js'
// captureDiff is reserved for future feedback/gold-diff emission.
void captureDiff

const WORKSPACE_RULE = `The current working directory is the workspace root. Use workspace-relative paths only, e.g. app.py, tests/test_app.py, mkdir -p tests. Never use absolute paths, home-directory paths, or temporary-directory paths.`

const BUILDER_SYSTEM = `You build the BASE project for a dataset task. ${WORKSPACE_RULE} Create files in the current directory only. Do not solve the later task unless the builder prompt explicitly asks for base functionality. Keep the project runnable offline.`

const SOLVER_SYSTEM = `You are an autonomous coding agent. Complete the task by editing files and running commands in the workspace. ${WORKSPACE_RULE} Work only within the workspace. Do not modify files matching the task's protected paths (e.g. test files the grader relies on). When done, ensure the project satisfies the stated goal.`

export interface OrchestrateDeps {
  evotBin: string
  evotVersion: string
}

export async function orchestrate(opts: DistillOptions, deps: OrchestrateDeps): Promise<string> {
  const workspaceRoot = effectiveWorkspaceRoot(opts)
  // Create the reporter first so the (potentially slow) task-collection phase
  // — especially `--auto` proposing — is visible instead of silent. All lines
  // are also mirrored to <out>/logs/distill-*.log for post-mortem debugging.
  const logPath = makeLogPath(opts.out)
  const reporter = new Reporter(opts.verbosity, 0, logPath)
  reporter.phase(`log file: ${logPath}`)
  reporter.phase(
    `distill starting (model=${opts.model ?? 'default'}, out=${opts.out}, workspaceRoot=${workspaceRoot}, concurrency=${opts.maxConcurrency})`,
  )

  const tasks = await collectTasks(opts, deps.evotBin, reporter, workspaceRoot)
  if (!tasks.length) {
    await cleanupDefaultWorkspaceRoot(opts, workspaceRoot)
    const summary = `no tasks to run | log=${logPath}`
    reporter.summary(summary)
    return summary
  }
  reporter.setTotal(tasks.length)
  reporter.phase(`running ${tasks.length} task(s)`)

  const bundle = new Bundle(opts.out, {
    evotVersion: deps.evotVersion,
    teacherModel: opts.model ?? 'default',
  })

  let ok = 0
  let dropped = 0
  // In verbose mode, run serially so each task's block stays contiguous;
  // interleaved box-drawing from parallel tasks would be unreadable.
  const concurrency = opts.verbosity === 'verbose' ? 1 : opts.maxConcurrency
  await pool(tasks, concurrency, async (task) => {
    reporter.taskStart(task.id)
    for (let attempt = 0; attempt < opts.repeats; attempt++) {
      const res = await runOne(task, opts, deps, bundle, reporter, workspaceRoot)
      if (res.ok) {
        ok++
        reporter.taskOk(task.id, attempt)
        return
      }
      if (attempt === opts.repeats - 1) {
        dropped++
        reporter.taskDrop(task.id, res.reason)
      }
    }
  })

  await cleanupDefaultWorkspaceRoot(opts, workspaceRoot)
  reporter.phase('writing bundle')
  const summary = `${await bundle.write(opts.emit)} | kept ${ok} dropped ${dropped} -> ${opts.out} | log=${logPath}`
  reporter.summary(summary)
  return summary
}

async function collectTasks(
  opts: DistillOptions,
  evotBin: string,
  reporter: Reporter,
  workspaceRoot: string,
): Promise<TaskSpec[]> {
  const tasks: TaskSpec[] = []
  if (opts.tasksFile) {
    reporter.phase(`loading tasks from ${opts.tasksFile}`)
    const loaded = (await loadTasks(opts.tasksFile)).map((t) => ({
      ...t,
      targetTurns: t.targetTurns ?? opts.targetTurns,
    }))
    reporter.phase(`loaded ${loaded.length} task(s)`)
    tasks.push(...loaded)
  }
  if (opts.auto) {
    const domains = await resolveDomains(opts)
    for (const d of domains) {
      reporter.phase(`proposing ${d.n} task(s) for domain: ${d.domain}`)
      await mkdir(workspaceRoot, { recursive: true })
      const proposerDir = join(workspaceRoot, '_proposer')
      await mkdir(proposerDir, { recursive: true })
      const proposerCwd = await realpath(proposerDir)
      const proposed = (await proposeTasks(d, evotBin, proposerCwd, opts.targetTurns, opts.model, opts.envFile, reporter)).map((t) => ({
        ...t,
        targetTurns: t.targetTurns ?? opts.targetTurns,
      }))
      reporter.phase(`proposed ${proposed.length} task(s)`)
      tasks.push(...proposed)
    }
  }
  return tasks
}

async function resolveDomains(opts: DistillOptions) {
  if (opts.domainsFile) {
    const { readFile } = await import('node:fs/promises')
    const body = await readFile(opts.domainsFile, 'utf8')
    return body
      .split('\n')
      .map((l) => l.trim())
      .filter(Boolean)
      .map((domain) => ({ domain, n: opts.n ?? 20 }))
  }
  if (opts.domain) return [{ domain: opts.domain, n: opts.n ?? 20 }]
  return []
}

function effectiveWorkspaceRoot(opts: DistillOptions): string {
  return opts.workspaceRoot ?? join(opts.out, '.distill-work')
}

function makeLogPath(out: string): string {
  const stamp = new Date().toISOString().replace(/[:.]/g, '-').replace('T', '_').replace('Z', '')
  return join(out, 'logs', `distill-${stamp}.log`)
}

async function cleanupDefaultWorkspaceRoot(opts: DistillOptions, workspaceRoot: string): Promise<void> {
  if (opts.workspaceRoot) return
  await rm(workspaceRoot, { recursive: true, force: true }).catch(() => {})
}

/** Run one task end-to-end. Returns whether it produced data, plus a drop reason. */
async function runOne(
  task: TaskSpec,
  opts: DistillOptions,
  deps: OrchestrateDeps,
  bundle: Bundle,
  reporter: Reporter,
  workspaceRoot: string,
): Promise<{ ok: boolean; reason: string }> {
  const parent = workspaceRoot
  await mkdir(parent, { recursive: true })
  // mkdtemp under $TMPDIR can return a symlinked path (on macOS /var ->
  // /private/var). The agent subprocess resolves its cwd to the real path, so
  // its tool args use /private/var/... — which wouldn't match the cwd the
  // importer scrubs against, leaking host paths into the data. Resolve up front
  // so the agent and the importer agree on one canonical path.
  const ws = await realpath(await mkdtemp(join(parent, `evot-distill-${task.id}-`)))
  const onEvent = (ev: { kind: string; payload: Record<string, unknown> }) =>
    reporter.agentEvent(task.id, ev.kind, ev.payload)
  const onDebug = (msg: string) => reporter.debug(task.id, msg)
  try {
    reporter.stage(task.id, 'materialize', task.workspace.source)
    await materialize(task, ws)

    // Builder: build the base project for agent_scaffold tasks.
    if (task.workspace.source === 'agent_scaffold') {
      if (!task.workspace.builderPrompt.trim()) return drop('empty builder prompt')
      reporter.stage(task.id, 'builder')
      const build = await runAgent({
        cwd: ws,
        prompt: task.workspace.builderPrompt,
        model: opts.model,
        envFile: opts.envFile,
        systemPrompt: BUILDER_SYSTEM,
        timeoutSec: opts.perTaskTimeout,
        evotBin: deps.evotBin,
        limits: { maxTurns: 40 },
        onEvent,
        onDebug,
      })
      if (!build.finished) return drop('builder did not finish')
    }

    try {
      reporter.stage(task.id, 'setup')
      runSetup(task, ws)
    } catch (e) {
      return drop(`setup failed: ${String(e).slice(0, 80)}`)
    }

    // Freeze the initial state and self-check the task is valid.
    reporter.stage(task.id, 'self_check')
    gitInit(ws)
    const check = await selfCheck(task, ws)
    if (!check.ok) {
      return drop(
        check.baseFails ? 'reference does not pass verifier' : 'base already passes verifier',
      )
    }

    // Snapshot the frozen initial workspace NOW, before the Solver mutates it.
    // This is the RL task's starting state (no solver edits, no .git).
    const frozen = await realpath(await mkdtemp(join(parent, `evot-frozen-${task.id}-`)))
    await rm(frozen, { recursive: true, force: true })
    await mkdir(frozen, { recursive: true })
    await cp(ws, frozen, { recursive: true, filter: (s) => !s.includes('/.git') })

    // Solver: solve on the frozen workspace.
    reporter.stage(task.id, 'solver')
    const solverSystem = task.protectedPaths?.length
      ? `${SOLVER_SYSTEM}\nProtected paths (do not edit): ${task.protectedPaths.join(', ')}`
      : SOLVER_SYSTEM
    const solve = await runAgent({
      cwd: ws,
      prompt: task.prompt,
      model: opts.model,
      envFile: opts.envFile,
      systemPrompt: solverSystem,
      limits: task.limits ?? { maxTurns: 25 },
      timeoutSec: opts.perTaskTimeout,
      evotBin: deps.evotBin,
      onEvent,
      onDebug,
    })
    if (!solve.finished) {
      await rm(frozen, { recursive: true, force: true }).catch(() => {})
      return drop(solve.error || 'solver did not finish')
    }

    // Objective gate: did the solver's workspace pass?
    reporter.stage(task.id, 'verify')
    const verify = runVerifierDetailed(task.verifier, ws)
    if (!verify.passed) {
      reporter.debug(task.id, `verifier exit=${verify.exitCode}: ${verify.output.slice(0, 300)}`)
      await rm(frozen, { recursive: true, force: true }).catch(() => {})
      const tail = verify.output.replace(/\s+/g, ' ').trim().slice(-120)
      return drop(`verifier failed (exit ${verify.exitCode})${tail ? `: …${tail}` : ''}`)
    }

    // Emit SFT rows from the solver trajectory.
    if (opts.emit.includes('sft')) {
      const rows = importEvents(solve.events, {
        systemPrompt: solverSystem,
        userPrompt: task.prompt,
        cwd: ws,
        metadata: {
          source: task.source ?? 'evot_auto',
          teacher_model: opts.model,
          verified: true,
          task_id: task.id,
          target_turns: task.targetTurns ?? opts.targetTurns,
        },
      })
      reporter.stage(task.id, 'emit', `sft=${rows.length}`)
      bundle.addSft(rows)
    }

    // Register the RL task with its frozen initial workspace (snapshotted above).
    if (opts.emit.includes('rl')) {
      await bundle.addRl(task, frozen)
    }
    await rm(frozen, { recursive: true, force: true }).catch(() => {})

    return { ok: true, reason: '' }
  } finally {
    await rm(ws, { recursive: true, force: true }).catch(() => {})
  }
}

/** A dropped task result with its reason. */
function drop(reason: string): { ok: false; reason: string } {
  return { ok: false, reason }
}

/** Run `fn` over `items` with at most `limit` in flight. */
async function pool<T>(items: T[], limit: number, fn: (item: T) => Promise<void>): Promise<void> {
  const queue = [...items]
  const workers = Array.from({ length: Math.max(1, limit) }, async () => {
    let item: T | undefined
    while ((item = queue.shift()) !== undefined) {
      await fn(item)
    }
  })
  await Promise.all(workers)
}

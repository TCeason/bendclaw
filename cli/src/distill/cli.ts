/**
 * distill — CLI entry for dataset generation.
 *
 * `evot distill` drives the agent itself as a teacher to produce a training
 * bundle (SFT trajectories + RL task pool) that downstream trainers consume
 * with zero post-processing. Tasks come from a curated JSONL file, from the
 * agent authoring them (`--auto`), or both.
 */

import { version } from '../native/index.js'
import { orchestrate } from './internal/orchestrate.js'
import { isDifficulty } from './internal/difficulty.js'
import type { DistillOptions, Difficulty } from './internal/types.js'

export async function runDistill(argv: string[]): Promise<void> {
  if (argv.includes('--help') || argv.includes('-h')) {
    printDistillHelp()
    process.exit(0)
  }
  const opts = parseDistillArgs(argv)
  if (!opts.tasksFile && !opts.auto) {
    console.error('distill: need --tasks <file> and/or --auto --domain <text>')
    process.exit(1)
  }
  const evotBin = process.execPath.endsWith('evot') ? process.execPath : 'evot'
  const summary = await orchestrate(opts, { evotBin, evotVersion: version() })
  void summary
  process.exit(0)
}

function printDistillHelp(): void {
  console.log(`evot distill — generate an SFT/RL dataset by driving evot as a teacher

Usage: evot distill [--tasks <file>] [--auto --domain <text>] [options]

Task sources (at least one):
  --tasks <file>         curated tasks JSONL
  --auto                 author tasks with the agent
  --domain <text>        direction for --auto
  --domains <file>       newline-separated domains for --auto
  --n <n>                tasks per domain (--auto)

Options:
  --out <dir>            bundle output dir (default: data)
  --model <spec>         teacher model
  --env-file <path>      evot.env path
  --emit <sft,rl>        what to produce (default: sft,rl)
  --rl-only              skip the Solver: prove solvability via a bounded
                         reference solve and emit RL rows only (faster; no SFT)
  --repeats <n>          attempts per task (default: 1)
  --difficulty <tier>    task difficulty: L2|L4|L6|L8|L16|mixed (default: L2).
                         A complexity label (higher = more complex); 'mixed'
                         spreads tasks evenly across all tiers.
  --max-concurrency <n>  parallel tasks (default: 2)
  --per-task-timeout <s> per-task wall-clock cap (default: 600)
  --workspace-root <dir> parent dir for temporary Builder/Solver workspaces
                         (default: <out>/.distill-work)
  --keep-fail            keep failed trajectories in a side file
  -v, --verbose          live per-stage and agent-event progress
  -q, --quiet            only the final summary
  -h, --help             show this help`)
}

function parseDistillArgs(argv: string[]): DistillOptions {
  const opts: DistillOptions = {
    out: 'data',
    tools: ['read', 'write', 'edit', 'bash'],
    emit: ['sft', 'rl'],
    repeats: 1,
    keepFail: false,
    maxConcurrency: 2,
    perTaskTimeout: 600,
    difficulty: 'L2',
    verbosity: 'normal',
  }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--tasks' && argv[i + 1]) opts.tasksFile = argv[++i]
    else if (a === '--auto') opts.auto = true
    else if (a === '--domain' && argv[i + 1]) opts.domain = argv[++i]
    else if (a === '--domains' && argv[i + 1]) opts.domainsFile = argv[++i]
    else if (a === '--n' && argv[i + 1]) opts.n = parsePositiveInt(argv[++i], '--n')
    else if (a === '--out' && argv[i + 1]) opts.out = argv[++i]
    else if (a === '--model' && argv[i + 1]) opts.model = argv[++i]
    else if (a === '--env-file' && argv[i + 1]) opts.envFile = argv[++i]
    else if (a === '--tools' && argv[i + 1]) opts.tools = argv[++i].split(',').map((s) => s.trim())
    else if (a === '--emit' && argv[i + 1]) opts.emit = argv[++i].split(',').map((s) => s.trim()) as ('sft' | 'rl')[]
    else if (a === '--rl-only') { opts.rlOnly = true; opts.emit = ['rl'] }
    else if (a === '--repeats' && argv[i + 1]) opts.repeats = parsePositiveInt(argv[++i], '--repeats')
    else if (a === '--keep-fail') opts.keepFail = true
    else if (a === '--difficulty' && argv[i + 1]) opts.difficulty = parseDifficulty(argv[++i])
    else if (a === '--max-concurrency' && argv[i + 1]) opts.maxConcurrency = parsePositiveInt(argv[++i], '--max-concurrency')
    else if (a === '--per-task-timeout' && argv[i + 1]) opts.perTaskTimeout = parsePositiveInt(argv[++i], '--per-task-timeout')
    else if (a === '--workspace-root' && argv[i + 1]) opts.workspaceRoot = argv[++i]
    else if (a === '--verbose' || a === '-v') opts.verbosity = 'verbose'
    else if (a === '--quiet' || a === '-q') opts.verbosity = 'quiet'
  }
  return opts
}

/** Parse a positive-integer flag value, exiting with a clear error on garbage.
 *  Guards against NaN (e.g. `--max-concurrency abc`) silently degrading the run:
 *  NaN concurrency yields zero workers (whole batch no-ops), NaN repeats skips
 *  the attempt loop, and NaN timeout fires setTimeout immediately. */
function parsePositiveInt(raw: string, flag: string): number {
  const n = Number(raw)
  if (!Number.isInteger(n) || n <= 0) {
    console.error(`distill: ${flag} must be a positive integer, got "${raw}"`)
    process.exit(1)
  }
  return n
}

/** Parse the --difficulty flag: a known tier or 'mixed'. */
function parseDifficulty(raw: string): Difficulty | 'mixed' {
  if (raw === 'mixed' || isDifficulty(raw)) return raw
  console.error(`distill: --difficulty must be one of L2|L4|L6|L8|L16|mixed, got "${raw}"`)
  process.exit(1)
}

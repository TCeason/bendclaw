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
import type { DistillOptions } from './internal/types.js'

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
  --repeats <n>          attempts per task (default: 1)
  --target-turns <n>     target solver turns for task difficulty (default: 8;
                         not a hard runtime limit)
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
    targetTurns: 8,
    verbosity: 'normal',
  }
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i]
    if (a === '--tasks' && argv[i + 1]) opts.tasksFile = argv[++i]
    else if (a === '--auto') opts.auto = true
    else if (a === '--domain' && argv[i + 1]) opts.domain = argv[++i]
    else if (a === '--domains' && argv[i + 1]) opts.domainsFile = argv[++i]
    else if (a === '--n' && argv[i + 1]) opts.n = parseInt(argv[++i], 10)
    else if (a === '--out' && argv[i + 1]) opts.out = argv[++i]
    else if (a === '--model' && argv[i + 1]) opts.model = argv[++i]
    else if (a === '--env-file' && argv[i + 1]) opts.envFile = argv[++i]
    else if (a === '--tools' && argv[i + 1]) opts.tools = argv[++i].split(',').map((s) => s.trim())
    else if (a === '--emit' && argv[i + 1]) opts.emit = argv[++i].split(',').map((s) => s.trim()) as ('sft' | 'rl')[]
    else if (a === '--repeats' && argv[i + 1]) opts.repeats = parseInt(argv[++i], 10)
    else if (a === '--keep-fail') opts.keepFail = true
    else if (a === '--target-turns' && argv[i + 1]) opts.targetTurns = parseInt(argv[++i], 10)
    else if (a === '--max-concurrency' && argv[i + 1]) opts.maxConcurrency = parseInt(argv[++i], 10)
    else if (a === '--per-task-timeout' && argv[i + 1]) opts.perTaskTimeout = parseInt(argv[++i], 10)
    else if (a === '--workspace-root' && argv[i + 1]) opts.workspaceRoot = argv[++i]
    else if (a === '--verbose' || a === '-v') opts.verbosity = 'verbose'
    else if (a === '--quiet' || a === '-q') opts.verbosity = 'quiet'
  }
  return opts
}

/**
 * runner — drive one evot headless run in a given workspace and collect events.
 *
 * We spawn the `evot` binary as a child process rather than calling the napi
 * Agent in-process for one decisive reason: the agent binds its working
 * directory from `std::env::current_dir()` at creation, so per-task isolation
 * under concurrency is only safe via separate processes each with their own
 * `cwd`. This also gives us hard timeouts and clean cancellation for free.
 *
 * The child runs with `--output-format stream-json`, emitting one JSON event
 * per line. We keep the structured events the importer needs and ignore deltas.
 */

import { spawn } from 'node:child_process'
import type { RunLimits } from './types.js'

export interface RunEvent {
  kind: string
  payload: Record<string, unknown>
}

export interface RunResult {
  events: RunEvent[]
  finished: boolean
  error?: string
}

export interface RunParams {
  cwd: string
  prompt: string
  model?: string
  envFile?: string
  systemPrompt?: string
  limits?: RunLimits
  timeoutSec: number
  evotBin: string
  /** Called for each event as it streams in (live progress). */
  onEvent?: (ev: RunEvent) => void
  /** Called with diagnostic lines (spawn command, stderr) for verbose tracing. */
  onDebug?: (msg: string) => void
}

/** Run evot headless in `cwd`, returning the collected event stream. Never throws. */
export async function runAgent(p: RunParams): Promise<RunResult> {
  const args = ['-p', p.prompt, '--output-format', 'stream-json']
  if (p.model) args.push('--model', p.model)
  if (p.envFile) args.push('--env-file', p.envFile)
  if (p.systemPrompt) args.push('--append-system-prompt', p.systemPrompt)
  if (p.limits?.maxTurns) args.push('--max-turns', String(p.limits.maxTurns))
  if (p.limits?.maxTokens) args.push('--max-tokens', String(p.limits.maxTokens))
  if (p.limits?.maxDuration) args.push('--max-duration', String(p.limits.maxDuration))

  return new Promise<RunResult>((resolve) => {
    p.onDebug?.(`spawn: ${p.evotBin} ${args.join(' ')} (cwd=${p.cwd})`)
    const child = spawn(p.evotBin, args, {
      cwd: p.cwd,
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    const events: RunEvent[] = []
    let finished = false
    let stderr = ''
    let buf = ''

    const timer = setTimeout(() => {
      child.kill('SIGKILL')
    }, p.timeoutSec * 1000)

    child.stdout.on('data', (chunk: Buffer) => {
      buf += chunk.toString()
      let nl: number
      while ((nl = buf.indexOf('\n')) !== -1) {
        const line = buf.slice(0, nl).trim()
        buf = buf.slice(nl + 1)
        if (!line) continue
        try {
          const ev = JSON.parse(line) as RunEvent
          events.push(ev)
          p.onEvent?.(ev)
          if (ev.kind === 'run_finished') finished = true
        } catch {
          // ignore non-JSON noise
        }
      }
    })

    child.stderr.on('data', (chunk: Buffer) => {
      const text = chunk.toString()
      stderr += text
      p.onDebug?.(`stderr: ${text.trim().slice(0, 200)}`)
    })

    child.on('close', () => {
      clearTimeout(timer)
      resolve({
        events,
        finished,
        error: finished ? undefined : stderr.trim().slice(0, 300) || 'run did not finish',
      })
    })

    child.on('error', (err) => {
      clearTimeout(timer)
      resolve({ events, finished: false, error: String(err) })
    })
  })
}

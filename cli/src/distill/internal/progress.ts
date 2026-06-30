/**
 * progress — a small reporter so the pipeline doesn't litter console.log calls.
 *
 * Three terminal levels:
 *   quiet    only the final summary
 *   normal   one line per task result + summary (default)
 *   verbose  per-stage progress and live agent events (turns, tool calls)
 *
 * Regardless of terminal verbosity, the reporter can also mirror all diagnostic
 * lines to a per-run log file under <out>/logs for post-mortem debugging.
 */

export type { Verbosity } from './types.js'
import type { Verbosity } from './types.js'
import { appendFileSync, mkdirSync, writeFileSync } from 'node:fs'
import { dirname } from 'node:path'

export class Reporter {
  private done = 0
  private startedAt = new Map<string, number>()

  constructor(
    private level: Verbosity,
    private total: number,
    public readonly logPath?: string,
  ) {
    if (logPath) {
      mkdirSync(dirname(logPath), { recursive: true })
      writeFileSync(logPath, `# evot distill log\ncreated_at=${new Date().toISOString()}\n\n`, 'utf8')
    }
  }

  /** Total task count is known only after proposing; allow updating it. */
  setTotal(total: number): void {
    this.total = total
  }

  /** A top-level pipeline phase began (collect / propose / run / write). */
  phase(msg: string): void {
    this.line(`==> ${msg}`, this.level !== 'quiet')
  }

  /** A live event from a non-task agent run (e.g. the proposer). */
  phaseEvent(label: string, kind: string, payload: Record<string, unknown>): void {
    const text = describeEvent(kind, payload)
    if (text) this.emit(label, kind, text, this.level === 'verbose')
  }

  /** A task entered the pipeline. */
  taskStart(id: string): void {
    this.startedAt.set(id, Date.now())
    this.line('', this.level === 'verbose')
    this.line(`┌─ task ${id}`, this.level === 'verbose')
  }

  /** A pipeline stage began (materialize / builder / self_check / solver / verify / emit). */
  stage(id: string, stage: string, detail = ''): void {
    this.emit(id, `[${stage}]`, detail, this.level === 'verbose')
  }

  /** A live event from an agent run (assistant turn, tool call, tool result). */
  agentEvent(id: string, kind: string, payload: Record<string, unknown>): void {
    const text = describeEvent(kind, payload)
    if (text) this.emit(id, '', text, this.level === 'verbose')
  }

  /** Low-level diagnostic (spawn command, stderr) — terminal verbose, file always. */
  debug(id: string, msg: string): void {
    this.emit(id, '', `· ${msg}`, this.level === 'verbose')
  }

  /** A task produced data. */
  taskOk(id: string, attempt: number): void {
    this.done++
    const verboseLine = `└─ [ok] ${id} attempt=${attempt}${this.elapsed(id)}`
    const normalLine = `[ok]   (${this.done}/${this.total}) ${id} attempt=${attempt}${this.elapsed(id)}`
    this.line(this.level === 'verbose' ? verboseLine : normalLine, this.level !== 'quiet')
  }

  /** A task was dropped. */
  taskDrop(id: string, reason: string): void {
    this.done++
    const verboseLine = `└─ [drop] ${id} — ${reason}${this.elapsed(id)}`
    const normalLine = `[drop] (${this.done}/${this.total}) ${id} — ${reason}${this.elapsed(id)}`
    this.line(this.level === 'verbose' ? verboseLine : normalLine, this.level !== 'quiet')
  }

  /** Final one-line summary (always shown and always logged). */
  summary(msg: string): void {
    this.line(msg, true)
  }

  private elapsed(id: string): string {
    const t0 = this.startedAt.get(id)
    if (!t0) return ''
    return ` (${((Date.now() - t0) / 1000).toFixed(1)}s)`
  }

  private emit(id: string, tag: string, detail = '', terminal = true): void {
    const parts = [tag, detail].filter(Boolean).join(' ')
    this.line(`│  ${parts}`, terminal)
  }

  private line(text: string, terminal: boolean): void {
    if (terminal) console.log(text)
    if (this.logPath) appendFileSync(this.logPath, `${text}\n`, 'utf8')
  }
}

/** Turn a raw run event into a short human line, or '' to skip it. */
function describeEvent(kind: string, payload: Record<string, unknown>): string {
  switch (kind) {
    case 'tool_started':
      return `→ ${payload.tool_name}${preview(payload.preview_command)}`
    case 'tool_finished':
      return `✓ ${payload.tool_name}${payload.is_error ? ' (error)' : ''}`
    case 'assistant_completed': {
      const blocks = (payload.content as { type: string }[]) ?? []
      const tools = blocks.filter((b) => b.type === 'tool_call' || b.type === 'toolCall').length
      return tools ? `assistant (${tools} tool call${tools > 1 ? 's' : ''})` : 'assistant (text)'
    }
    case 'run_finished':
      return 'run finished'
    case 'error':
      return `error: ${String(payload.message ?? '').slice(0, 120)}`
    default:
      return ''
  }
}

function preview(cmd: unknown): string {
  if (typeof cmd !== 'string' || !cmd) return ''
  const oneLine = cmd.replace(/\s+/g, ' ').trim()
  return `: ${oneLine.slice(0, 80)}`
}

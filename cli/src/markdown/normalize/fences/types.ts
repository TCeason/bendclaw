/**
 * Shared types for the fence-repair case catalog + engine.
 */

export type FenceOpenState = {
  marker: string
  length: number
  close: string
  lang: string | null
  codeLines: string[]
}

export type FenceCtx = {
  lines: string[]
  index: number
  line: string
  newline: string
  open: FenceOpenState | null
  finalClose: boolean
}

/**
 * What a case wants the engine to do.
 *
 * Cases never mutate the output buffer directly — they return an action and
 * the thin engine applies it. This keeps heuristics out of the scan loop.
 */
export type FenceAction =
  | { type: 'skip' }
  | { type: 'emit'; lines: string[], closeOpen?: boolean, open?: FenceOpenState | null }
  | { type: 'close-then-emit'; line: string }
  | { type: 'passthrough' }

export type FenceCase = {
  id: string
  /** When to fire. Read-only over ctx; no side effects. */
  match: (ctx: FenceCtx) => boolean
  /** How to repair. Returns the action the engine should apply. */
  apply: (ctx: FenceCtx) => FenceAction
}

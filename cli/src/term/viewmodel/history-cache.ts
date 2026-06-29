/**
 * HistoryRenderCache — incremental flattening cache for committed REPL history.
 *
 * The REPL re-renders a full frame on every spinner tick, streaming delta, and
 * keystroke. Re-flattening the entire committed transcript (buildOutputBlocks +
 * blocksToLines) each frame is O(total history) and grows to 5–14 ms in long
 * sessions, which visibly stalls the just-sent message and keystroke echo.
 *
 * Committed history is only ever appended to or fully cleared/replaced — never
 * mutated in place. So the flattened ANSI lines are cached and extended in
 * place: each new commit flattens only its own lines and appends them, keeping
 * per-commit cost at O(new lines). A full rebuild happens only when the prefix
 * can change: an explicit reset (clear/replace), a view-mode or width change
 * (wrapping differs), or the history shrinking below what's cached.
 *
 * The incremental path carries the trailing `prevKind` forward so assistant
 * block-start dots and marginTop spacing match a full rebuild byte-for-byte.
 */

import type { OutputLine } from '../../render/output.js'
import { buildOutputBlocks } from './output.js'
import { blocksToLines } from './types.js'

export class HistoryRenderCache {
  private lines: string[] = []
  private count = 0
  private prevKind: string | undefined
  private dirty = true
  private columns = -1

  /** Mark the cache stale so the next sync does a full rebuild. Call after a
   *  clear or in-place replacement of the source history. */
  reset(): void {
    this.dirty = true
  }

  /**
   * Reconcile the cache against the current committed history and return the
   * flattened ANSI lines. `columns` is the terminal width; a change forces a
   * full rebuild because wrapping differs.
   */
  sync(history: OutputLine[], columns: number): string[] {
    const needFullRebuild =
      this.dirty ||
      this.columns !== columns ||
      this.count > history.length

    if (needFullRebuild) {
      this.lines = history.length > 0
        ? blocksToLines(buildOutputBlocks(history, { columns }))
        : []
      this.count = history.length
      this.prevKind = advancePrevKind(undefined, history)
      this.dirty = false
      this.columns = columns
    } else if (this.count < history.length) {
      const newLines = history.slice(this.count)
      const appended = blocksToLines(buildOutputBlocks(newLines, { prevKind: this.prevKind, columns }))
      if (appended.length > 0) this.lines = this.lines.concat(appended)
      this.prevKind = advancePrevKind(this.prevKind, newLines)
      this.count = history.length
    }

    return this.lines
  }
}

/**
 * Advance the `prevKind` tracking state across a run of OutputLines, applying
 * the exact rule buildOutputBlocks uses: an empty assistant line that is not a
 * continuation spacer carries the prior kind forward instead of becoming
 * 'assistant'. Keeps the incremental cache's block-start detection in lockstep
 * with a full rebuild.
 */
export function advancePrevKind(prevKind: string | undefined, lines: OutputLine[]): string | undefined {
  let kind = prevKind
  for (const ol of lines) {
    if (ol.kind === 'assistant' && !ol.text) {
      kind = ol.isContinuationSpacer ? 'assistant' : kind
      continue
    }
    kind = ol.kind
  }
  return kind
}

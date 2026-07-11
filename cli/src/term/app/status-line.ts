/**
 * Pure helpers for collapsing transient model/thinking status lines in place.
 *
 * Model and thinking share one status slot so rapid alternating switches stay
 * single-line. Only a trailing status line is eligible for replacement; any
 * later non-status output freezes the prior status into history.
 */

import type { OutputLine } from '../../render/output.js'

const STATUS_IDS = new Set(['sys-model', 'sys-think'])

export function isStatusLineId(id: string): boolean {
  return STATUS_IDS.has(id)
}

/** Mutate `lines` by replacing the trailing status slot or appending.
 *  Returns true when an existing line was replaced. */
export function replaceOrPushStatusLine(lines: OutputLine[], line: OutputLine): boolean {
  const last = lines.length > 0 ? lines[lines.length - 1] : undefined
  if (last && isStatusLineId(last.id) && isStatusLineId(line.id)) {
    lines[lines.length - 1] = line
    return true
  }
  lines.push(line)
  return false
}

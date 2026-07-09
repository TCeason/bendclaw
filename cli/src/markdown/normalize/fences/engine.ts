/**
 * Thin fence-repair state machine.
 *
 * Scans lines, asks the case catalog what to do, applies the action.
 * No heuristics live here — only scheduling + buffer mutation.
 */

import { CODE_FENCE_RE } from '../../primitives.js'
import { FENCE_OPEN_GLUED_RE } from './signals.js'
import { matchFenceCase } from './cases.js'
import type { FenceCtx, FenceOpenState } from './types.js'

function splitSingleLineFence(line: string): string[] | null {
  const match = /^( {0,3})(`{3,}|~{3,})([^`~\n]*?)(`{3,}|~{3,})[ \t]*$/.exec(line)
  if (!match) return null
  const indent = match[1]!
  const open = match[2]!
  const content = match[3]!.trim()
  const close = match[4]!
  if (!content) return null
  if (close[0] !== open[0] || close.length < open.length) return null
  // Language-only tags like ```json``` are not single-line code.
  if (/^[A-Za-z0-9_+.#-]+$/.test(content)) return null
  return [`${indent}${open}`, content, close]
}

/**
 * Split opening fences that models glued onto the end of a heading/prose line,
 * and single-line ```code``` fences into open/body/close.
 *
 * This is a structural pre-pass (line shape), not a named repair case — it
 * runs before the case catalog so cases always see well-formed fence lines.
 */
export function splitGluedFenceOpens(text: string): string {
  const lines = text.split('\n')
  let inFence = false
  let fenceMarker = ''
  const out: string[] = []

  for (const line of lines) {
    if (!inFence) {
      const singleLineFence = splitSingleLineFence(line)
      if (singleLineFence) {
        out.push(...singleLineFence)
        continue
      }
    }

    const fenceMatch = CODE_FENCE_RE.exec(line)
    if (fenceMatch) {
      const marker = fenceMatch[2]!
      if (!inFence) {
        inFence = true
        fenceMarker = marker
      } else if (marker[0] === fenceMarker[0] && marker.length >= fenceMarker.length) {
        inFence = false
        fenceMarker = ''
      }
      out.push(line)
      continue
    }

    if (inFence) {
      // Closing fence glued to end of a content line.
      const escChar = fenceMarker[0]!.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
      const closeGlued = new RegExp(
        `^(.+[^\\s${escChar}])(${escChar}{${fenceMarker.length},})[ \\t]*$`,
      )
      const cm = closeGlued.exec(line)
      if (cm) {
        out.push(cm[1]!)
        out.push(cm[2]!)
        inFence = false
        fenceMarker = ''
        continue
      }
      out.push(line)
      continue
    }

    const m = FENCE_OPEN_GLUED_RE.exec(line)
    if (m) {
      const lead = m[1]!
      const marker = m[2]!
      const info = m[3] ?? ''
      const infoTrim = info.trim()
      if (infoTrim === '' || /^[A-Za-z0-9_+.#-]+$/.test(infoTrim)) {
        out.push(lead.trimEnd(), `${marker}${infoTrim}`)
        inFence = true
        fenceMarker = marker
        continue
      }
    }
    out.push(line)
  }

  return out.join('\n')
}

function appendLines(out: string, lines: string[], newline: string): string {
  if (lines.length === 0) return out
  let next = out
  for (let i = 0; i < lines.length; i++) {
    const isLast = i === lines.length - 1
    next += lines[i]! + (isLast ? newline : '\n')
  }
  return next
}

/**
 * Drop stray bare fences, split glued closes, and auto-close completed
 * fences before following markdown prose — via the named case catalog.
 */
export function repairUnclosedFences(content: string, finalClose: boolean): string {
  const lines = content.split('\n')
  let out = ''
  let open: FenceOpenState | null = null

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i]!
    const newline = i < lines.length - 1 ? '\n' : ''
    const ctx: FenceCtx = { lines, index: i, line, newline, open, finalClose }

    const matched = matchFenceCase(ctx)
    if (!matched) {
      out += line + newline
      if (open) open.codeLines.push(line)
      continue
    }

    const action = matched.apply(ctx)
    switch (action.type) {
      case 'skip':
        break
      case 'passthrough':
        out += line + newline
        if (open) open.codeLines.push(line)
        break
      case 'emit': {
        out = appendLines(out, action.lines, newline)
        if (action.closeOpen) {
          open = null
        } else if (action.open !== undefined) {
          open = action.open
        } else if (open && action.lines.length > 0) {
          // Body content emitted while still open (should be rare for emit).
          for (const emitted of action.lines) {
            if (!CODE_FENCE_RE.test(emitted)) open.codeLines.push(emitted)
          }
        }
        break
      }
      case 'close-then-emit': {
        if (open) {
          out += `${open.close}\n`
          open = null
        }
        out += action.line + newline
        break
      }
    }
  }

  if (finalClose && open) {
    out += out.endsWith('\n') ? open.close : `\n${open.close}`
  }
  return out
}

/**
 * Prepare markdown for lexing by repairing only fence-boundary failures.
 */
export function prepareMarkdownFences(text: string): string {
  return repairUnclosedFences(splitGluedFenceOpens(text), true)
}

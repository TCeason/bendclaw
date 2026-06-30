/**
 * importer — convert an evot event stream into normalized SFT rows.
 *
 * This is the single place that enforces the downstream training contract, so
 * the delivered data needs zero post-processing. The rules below are not
 * cosmetic; each one prevents a concrete training failure:
 *
 *  - tool whitelist (read/write/edit/bash): keep data on-distribution; a
 *    segment using any other tool is dropped whole.
 *  - call_N renumber: evot's random tool ids ("tooluse_AbC123…") have no
 *    language structure; training on them teaches repetition loops. We remap
 *    to call_1/2/3… and keep tool_use ↔ tool_result paired.
 *  - path relativization: file-tool paths and free text are rewritten relative
 *    to the workspace so host paths never leak into training.
 *  - edit shape: evot emits {path, edits:[{oldText,newText}]}; we train the
 *    single-replacement {path, old, new} form (first edit wins).
 *  - split at user turns: each user→(assistant/tool_result)* span is one row,
 *    kept only if it contains at least one tool call.
 */

import type { RunEvent } from './runner.js'
import type { SftBlock, SftMessage, SftRow } from './types.js'

const MAX_TOOL_CONTENT = 6000

function toolKey(name: unknown): string {
  return String(name ?? '').trim().toLowerCase()
}

export interface ImportParams {
  systemPrompt: string
  /** The task prompt — the headless event stream carries no user event. */
  userPrompt: string
  cwd: string
  metadata: Record<string, unknown>
}

/** Build SFT rows from one run's events. Returns [] if nothing trainable. */
export function importEvents(events: RunEvent[], p: ImportParams): SftRow[] {
  const rows: SftRow[] = []
  // Seed the single user turn: `evot -p <prompt>` takes the prompt as CLI
  // input, so the event stream contains only assistant/tool events.
  let current: SftMessage[] = [{ role: 'user', content: p.userPrompt }]
  let hadTool = false
  const idMap = new Map<string, string>()
  let callN = 0

  const flush = () => {
    if (hadTool && current.length) {
      const tally: Record<string, number> = {}
      for (const m of current) {
        if (Array.isArray(m.content)) {
          for (const b of m.content) {
            if (b.type === 'tool_use') tally[b.name] = (tally[b.name] ?? 0) + 1
          }
        }
      }
      rows.push({
        messages: [{ role: 'system', content: p.systemPrompt }, ...current],
        tools: [],
        metadata: {
          ...p.metadata,
          tool_calls: Object.values(tally).reduce((a, b) => a + b, 0),
          tools_used: tally,
        },
      })
    }
    current = []
    hadTool = false
  }

  for (const ev of events) {
    if (ev.kind === 'assistant_completed') {
      const blocks = assistantBlocks(ev, p.cwd, idMap, () => `call_${++callN}`)
      if (blocks.hadTool) hadTool = true
      if (blocks.blocks.length) current.push({ role: 'assistant', content: blocks.blocks })
    } else if (ev.kind === 'tool_finished') {
      const origId = String(ev.payload.tool_call_id ?? '')
      current.push({
        role: 'user',
        content: [
          {
            type: 'tool_result',
            tool_use_id: idMap.get(origId) ?? origId ?? 'call_1',
            content: scrubCwd(truncate(String(ev.payload.content ?? '')), p.cwd),
            ...(ev.payload.is_error ? { is_error: true } : {}),
          },
        ],
      })
    }
  }
  flush()
  return rows
}

interface BlocksOut {
  blocks: SftBlock[]
  hadTool: boolean
}

function assistantBlocks(
  ev: RunEvent,
  cwd: string,
  idMap: Map<string, string>,
  nextId: () => string,
): BlocksOut {
  const out: SftBlock[] = []
  let hadTool = false
  const content = (ev.payload.content as unknown[]) ?? []

  for (const raw of content) {
    const b = raw as Record<string, unknown>
    const type = b.type as string
    if (type === 'thinking' && b.text) {
      out.push({ type: 'thinking', thinking: String(b.text) })
    } else if (type === 'text' && b.text) {
      out.push({ type: 'text', text: String(b.text) })
    } else if (type === 'tool_call' || type === 'toolCall' || type === 'tool_use') {
      const rawName = String(b.name ?? '').trim()
      const input = normalizeInput(rawName, (b.input ?? b.arguments) as Record<string, unknown>, cwd)
      if (input === null) {
        continue
      }
      // Opus/claude-code emit capitalized tool names (Read/Write/Edit/Bash),
      // but the trained harness uses lowercase. Lowercase every tool name so the
      // SFT protocol matches what RL/inference accept.
      const name = toolKey(rawName)
      hadTool = true
      const simpleId = nextId()
      const orig = b.id ? String(b.id) : ''
      if (orig) idMap.set(orig, simpleId)
      out.push({ type: 'tool_use', id: simpleId, name, input })
    }
  }
  return { blocks: out, hadTool }
}

/** Map an evot tool input onto the trained schema. Returns null to drop the segment. */
function normalizeInput(
  name: string,
  args: Record<string, unknown>,
  cwd: string,
): Record<string, unknown> | null {
  if (!args || typeof args !== 'object') return {}
  const key = toolKey(name)
  if (key === 'bash') {
    return { command: scrubCwd(String(args.command ?? args.cmd ?? ''), cwd) }
  }
  if (key === 'read') {
    const path = relPath(args.path, cwd)
    if (path === null) return null
    const out: Record<string, unknown> = { path }
    if (args.offset != null) out.offset = args.offset
    if (args.limit != null) out.limit = args.limit
    return out
  }
  if (key === 'write') {
    const path = relPath(args.path, cwd)
    if (path === null) return null
    return { path, content: String(args.content ?? '') }
  }
  if (key === 'edit') {
    const path = relPath(args.path, cwd)
    if (path === null) return null
    let old = args.oldText as string | undefined
    let neu = args.newText as string | undefined
    const edits = args.edits as { oldText?: string; newText?: string }[] | undefined
    if (old == null && Array.isArray(edits) && edits.length) {
      old = edits[0].oldText
      neu = edits[0].newText
    }
    return { path, old: old ?? '', new: neu ?? '' }
  }
  return sanitizeValue(args, cwd) as Record<string, unknown>
}

function sanitizeValue(value: unknown, cwd: string): unknown {
  if (typeof value === 'string') return scrubCwd(value, cwd)
  if (Array.isArray(value)) return value.map((v) => sanitizeValue(v, cwd))
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {}
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) out[k] = sanitizeValue(v, cwd)
    return out
  }
  return value
}

/** Rewrite an absolute path under cwd to workspace-relative; null if outside. */
function relPath(value: unknown, cwd: string): string | null {
  if (typeof value !== 'string' || !cwd) return typeof value === 'string' ? value : null
  const root = cwd.replace(/\/$/, '')
  if (value === root || value === cwd) return '.'
  if (value.startsWith(root + '/')) return value.slice(root.length + 1)
  // A file-tool path still absolute or under home points outside the workspace.
  if (value.startsWith('/') || value.startsWith('~')) return null
  return value
}

function scrubCwd(text: string, cwd: string): string {
  if (!text || !cwd) return text
  const root = cwd.replace(/\/$/, '')
  return scrubHostArtifacts(text.split(root + '/').join('').split(root).join('.'))
}

function scrubHostArtifacts(text: string): string {
  return text
    // Long-format directory listings include host-specific user/group names.
    .replace(/^([bcdlps-][rwxXsStT-]{9}@?\s+\d+\s+)\S+\s+\S+(\s+\d+\s+)/gm, '$1owner group$2')
    .replace(/\/Users\/[^\s'"`]+/g, '<home>')
    .replace(/\/(?:private\/)?var\/folders\/[^\s'"`]+/g, '<tmp>')
}

function truncate(text: string): string {
  if (text.length <= MAX_TOOL_CONTENT) return text
  return text.slice(0, MAX_TOOL_CONTENT) + `\n...[truncated ${text.length - MAX_TOOL_CONTENT} chars]`
}

import type { OutputLine } from '../render/output.js'

/** Structured settings snapshots, independent of translated/styled status text. */
export function modelShareEvents(provider: string, model: string, thinkingLevel?: string): NonNullable<OutputLine['shareEvents']> {
  const events: NonNullable<OutputLine['shareEvents']> = [
    { kind: 'model_change', data: { provider, model } },
  ]
  if (thinkingLevel) events.push({ kind: 'thinking_level_change', data: { thinking_level: thinkingLevel } })
  return events
}

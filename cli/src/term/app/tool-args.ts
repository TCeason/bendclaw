import { parse as partialParse } from 'partial-json'

/** Parse an in-progress tool-call JSON object, matching pi's partial-json path. */
export function parseStreamingToolArgs(raw: string): Record<string, unknown> {
  if (!raw.trim()) return {}
  try {
    const parsed = JSON.parse(raw)
    return isRecord(parsed) ? parsed : {}
  } catch {
    try {
      const parsed = partialParse(raw)
      return isRecord(parsed) ? parsed : {}
    } catch {
      return {}
    }
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

/**
 * Visible copy for mid-stream queued user messages.
 *
 * Placed above the spinner/prompt (pi-style) so a message that vanished from
 * the input box still has a clear home, plus a one-line hint that esc pulls
 * it back for editing.
 */
export function formatQueuedMessageLines(
  messages: string[],
  options: { maxChars?: number } = {},
): string[] {
  const maxChars = Number.isFinite(options.maxChars)
    ? Math.max(16, Math.floor(options.maxChars!))
    : 120

  const lines: string[] = []
  for (const raw of messages) {
    const one = raw.replace(/\s+/g, ' ').trim()
    if (!one) continue
    const text = one.length > maxChars ? `${one.slice(0, maxChars - 1)}…` : one
    lines.push(`Queued: ${text}`)
  }
  if (lines.length === 0) return []
  lines.push('↳ esc to pull back into input')
  return lines
}

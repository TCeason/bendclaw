/**
 * Parse release body markdown to extract changelog bullet points.
 */

const MAX_NOTES = 5
const CHANGELOG_HEADER = '### Changelog'

/**
 * Extract bullet points from a release body under the `### Changelog` section.
 * Returns up to MAX_NOTES items, stripped and cleaned.
 */
export function parseReleaseNotes(body: string | undefined): string[] {
  if (!body) return []

  const idx = body.indexOf(CHANGELOG_HEADER)
  if (idx === -1) return []

  const section = body.slice(idx + CHANGELOG_HEADER.length)
  const lines = section.split('\n')

  const bullets: string[] = []
  for (const line of lines) {
    const trimmed = line.trim()
    if (!trimmed) continue
    // Stop at next markdown header or section boundary
    if (trimmed.startsWith('##') || trimmed.startsWith('---')) break
    // Collect bullet points (support `* ` and `- `)
    if (trimmed.startsWith('* ') || trimmed.startsWith('- ')) {
      const note = trimmed.slice(2).trim()
      if (note) bullets.push(note)
    }
    if (bullets.length >= MAX_NOTES) break
  }

  return bullets
}

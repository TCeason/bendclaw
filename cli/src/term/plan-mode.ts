export interface PlanModeItem {
  step: number
  text: string
}

export function cleanPlanStepText(text: string): string {
  let cleaned = text
    .replace(/\*{1,2}([^*]+)\*{1,2}/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\s+/g, ' ')
    .trim()

  if (cleaned.length > 0) {
    cleaned = cleaned.charAt(0).toUpperCase() + cleaned.slice(1)
  }
  if (cleaned.length > 80) {
    cleaned = `${cleaned.slice(0, 77)}...`
  }
  return cleaned
}

export function extractPlanItems(message: string): PlanModeItem[] {
  const headerMatch = message.match(/\*{0,2}Plan:\*{0,2}\s*\n/i)
  if (!headerMatch || headerMatch.index === undefined) return []

  const planSection = message.slice(headerMatch.index + headerMatch[0].length)
  const items: PlanModeItem[] = []
  const numberedPattern = /^\s*(\d+)[.)]\s+(.+)$/gm

  for (const match of planSection.matchAll(numberedPattern)) {
    const raw = match[2]
    if (!raw) continue
    const withoutCheckbox = raw.replace(/^\[[ xX-]\]\s*/, '').trim()
    const text = cleanPlanStepText(withoutCheckbox.replace(/\*{1,2}$/, '').trim())
    if (text.length > 3 && !text.startsWith('/') && !text.startsWith('`')) {
      items.push({ step: items.length + 1, text })
    }
  }

  return items
}

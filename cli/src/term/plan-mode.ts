export interface PlanModeItem {
  step: number
  text: string
  completed: boolean
}

export interface PlanModeTask {
  id: number
  title: string
  status: 'pending' | 'completed'
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
      items.push({ step: items.length + 1, text, completed: false })
    }
  }

  return items
}

export function extractDoneSteps(message: string): number[] {
  const steps: number[] = []
  for (const match of message.matchAll(/\[DONE:(\d+)]/gi)) {
    const step = Number(match[1])
    if (Number.isInteger(step) && step > 0) steps.push(step)
  }
  return steps
}

export function markCompletedPlanItems(message: string, items: PlanModeItem[]): number {
  let changed = 0
  for (const step of extractDoneSteps(message)) {
    const item = items.find(candidate => candidate.step === step)
    if (item && !item.completed) {
      item.completed = true
      changed += 1
    }
  }
  return changed
}

export function planItemsToTasks(items: PlanModeItem[]): PlanModeTask[] {
  return items.map(item => ({
    id: item.step,
    title: item.text,
    status: item.completed ? 'completed' : 'pending',
  }))
}

export function footerLabel(tasks: PlanModeTask[] | null): string | null {
  if (!tasks || tasks.length === 0) return null
  const completed = tasks.filter(task => task.status === 'completed').length
  return `📋 ${completed}/${tasks.length}`
}

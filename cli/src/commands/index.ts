/**
 * Slash commands for the REPL.
 */

export interface SlashCommand {
  name: string
  aliases?: string[]
  description: string
  usage?: string
  handler: 'builtin'
}

export const COMMANDS: SlashCommand[] = [
  { name: '/help', description: 'Show help information', usage: '/help [command]', handler: 'builtin' },
  { name: '/resume', description: 'Resume a session', usage: '/resume [id | query]', handler: 'builtin' },
  { name: '/new', description: 'Start a new session', handler: 'builtin' },
  { name: '/goto', description: 'Go to a message', usage: '/goto <message_number>', handler: 'builtin' },
  { name: '/history', description: 'Show recent messages with seq numbers', usage: '/history [count]', handler: 'builtin' },
  { name: '/model', description: 'Show or change model', usage: '/model [name]', handler: 'builtin' },
  { name: '/plan', description: 'Enter planning mode', handler: 'builtin' },
  { name: '/harden', description: 'Stress-test the previous plan or current changes', usage: '/harden [plan | changes | arch | subject]', handler: 'builtin' },
  { name: '/skill', description: 'Manage skills', usage: '/skill [list | install <source> | remove <name>]', handler: 'builtin' },
  { name: '/copy', description: 'Copy last agent message (Markdown source) to clipboard', handler: 'builtin' },
  { name: '/clear', description: 'Clear session context', handler: 'builtin' },
]

/** Hidden commands — recognised but not shown in /help or ghost hints */
export const HIDDEN_COMMANDS: SlashCommand[] = [
  { name: '/exit', aliases: ['/quit', '/q'], description: 'Exit the REPL', handler: 'builtin' },
  { name: '/act', description: 'Return to normal action mode', handler: 'builtin' },
  { name: '/done', description: 'Exit log/plan mode', handler: 'builtin' },
  { name: '/env', description: 'Manage variables', usage: '/env [set K=V | del K | load FILE]', handler: 'builtin' },
  { name: '/log', description: 'Analyze session log / share / shot last markdown', usage: '/log [up [id] | dl <url> | shot [id] | query]', handler: 'builtin' },
  { name: '/update', description: 'Update evot to latest version', handler: 'builtin' },
  { name: '/_dump', description: 'Dump system prompt + tools + skills as JSON', usage: '/_dump [path]', handler: 'builtin' },
]

/** All commands (visible + hidden) for resolution */
const ALL_COMMANDS: SlashCommand[] = [...COMMANDS, ...HIDDEN_COMMANDS]

export type ResolvedCommand =
  | { kind: 'resolved'; name: string; args: string }
  | { kind: 'ambiguous'; candidates: string[] }
  | { kind: 'unknown' }

export function buildHardenPrompt(args: string): string {
  const subject = args.trim()
  if (!subject || subject === 'plan') {
    return [
      'harden the plan, strategy, or conclusion from the immediately preceding conversation context.',
      'If local git changes exist, inspect them only as supporting context and combine any relevant findings with the hardening pass; do not default to hardening the diff as the primary subject.',
    ].join(' ')
  }
  if (subject === 'changes') {
    return 'harden current git changes'
  }
  if (subject === 'arch') {
    return [
      'harden the architecture of the current git changes or the immediately preceding plan.',
      'Evaluate: simplicity, decoupling, clarity of responsibility, and cohesion.',
      'In the final output, include an annotated file tree showing the proposed directory structure with short comments explaining each module\'s role.',
    ].join(' ')
  }
  return `harden this strategy: ${subject}`
}

/**
 * Resolve a slash command input to a known command.
 * Supports prefix matching (e.g. "/h" → "/help").
 */
export function resolveCommand(input: string): ResolvedCommand {
  const parts = input.trim().split(/\s+/)
  const cmd = parts[0]!.toLowerCase()
  const args = parts.slice(1).join(' ')

  // Exact match first (visible + hidden)
  for (const c of ALL_COMMANDS) {
    if (c.name === cmd) return { kind: 'resolved', name: c.name, args }
    if (c.aliases?.includes(cmd)) return { kind: 'resolved', name: c.name, args }
  }

  // Prefix match (visible + hidden)
  const matches = ALL_COMMANDS.filter(
    (c) => c.name.startsWith(cmd) || (c.aliases?.some((a) => a.startsWith(cmd)) ?? false)
  )

  if (matches.length === 1) {
    return { kind: 'resolved', name: matches[0]!.name, args }
  }
  if (matches.length > 1) {
    return { kind: 'ambiguous', candidates: matches.map((c) => c.name) }
  }

  return { kind: 'unknown' }
}

/**
 * Returns true when text looks like a hand-typed slash command prefix:
 * `/` followed by zero or more ASCII lowercase letters.
 * Pasted paths like `/some/path.rs` are rejected.
 */
function isSlashPrefix(text: string): boolean {
  if (!text.startsWith('/')) return false
  const rest = text.slice(1)
  const cmdPart = rest.split(/\s/)[0] ?? ''
  // Allow lowercase letters plus `_` so hidden commands like `/_dump` work.
  return /^[a-z_]*$/.test(cmdPart)
}

/**
 * Check if input looks like a slash command.
 * Only triggers when the first word is a valid slash prefix
 * AND matches a known command (visible + hidden) by exact or prefix match.
 */
export function isSlashCommand(input: string): boolean {
  const trimmed = input.trim()
  if (!isSlashPrefix(trimmed)) return false
  const firstWord = trimmed.split(/\s+/)[0]!.toLowerCase()
  if (firstWord === '/') return false
  const allCmds = [...COMMANDS, ...HIDDEN_COMMANDS]
  return allCmds.some(c => c.name === firstWord || c.aliases?.includes(firstWord))
    || allCmds.some(c => c.name.startsWith(firstWord) || (c.aliases?.some(a => a.startsWith(firstWord)) ?? false))
}

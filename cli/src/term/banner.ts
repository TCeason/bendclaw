import chalk from 'chalk'
import stringWidth from 'string-width'
import { wrapTextWithAnsi } from '../render/wrap.js'
import { getSkillNames, getContextFiles } from './banner-skills.js'
import type { ConfigInfo } from '../native/index.js'

function truncate(text: string, maxWidth: number): string {
  if (stringWidth(text) <= maxWidth) return text
  let w = 0
  let i = 0
  for (const ch of text) {
    const cw = stringWidth(ch)
    if (w + cw + 1 > maxWidth) break // +1 for '…'
    w += cw
    i += ch.length
  }
  return text.slice(0, i) + '…'
}

function wrapBannerLine(text: string, columns: number): string[] {
  return wrapTextWithAnsi(text, Math.max(1, columns))
}

export interface BannerOptions {
  version: string
  model: string
  cwd: string
  configInfo: ConfigInfo | undefined
  columns: number
  serverState?: { port: number; address: string; channels: string[] } | null
  quiet?: boolean
  /** Release notes to show after an update (What's New) */
  releaseNotes?: string[] | null
  /** Update available info */
  updateAvailable?: { version: string } | null
  /**
   * Skills directories resolved by the agent (global + EVOT_SKILLS_DIRS from
   * evot.env/config + claude). When provided, the [Skills] section scans these
   * so it matches what the agent loads (issue #38); otherwise it falls back to
   * process.env-only resolution.
   */
  skillsDirs?: string[]
}

export function renderBanner(opts: BannerOptions): string {
  if (opts.quiet) return ''

  const { version: ver, model, cwd, configInfo, columns, serverState, releaseNotes, updateAvailable, skillsDirs } = opts

  const lines: string[] = []

  // Line 1: name + version
  lines.push(`  ${chalk.bold('evot')} ${chalk.dim(`v${ver}`)}`)

  // Line 2: compact keyboard hints
  lines.push(chalk.dim('  escape interrupt · ctrl+c/ctrl+d clear/exit · / commands · ctrl+o expand output'))

  // [Context] section
  const contextFiles = getContextFiles(cwd)
  if (contextFiles.length > 0) {
    lines.push('')
    lines.push(chalk.hex('#f0c674')('  [Context]'))
    lines.push(chalk.hex('#666666')(`    ${contextFiles.join(', ')}`))
  }

  // [Skills] section
  const skills = getSkillNames(skillsDirs)
  if (skills.length > 0) {
    lines.push('')
    lines.push(chalk.hex('#f0c674')('  [Skills]'))
    lines.push(chalk.hex('#666666')(`    ${skills.join(', ')}`))
  }

  // Update Available (yellow bordered section)
  if (updateAvailable) {
    lines.push('')
    const border = chalk.hex('#ffff00')('  ' + '─'.repeat(Math.max(1, Math.min(columns - 4, 72))))
    lines.push(border)
    lines.push(chalk.bold.hex('#ffff00')('  Update Available'))
    lines.push(
      chalk.hex('#808080')(`  New version ${updateAvailable.version} is available. Run `) +
        chalk.hex('#8abeb7')('evot update')
    )
    lines.push(
      chalk.hex('#808080')('  Changelog: ') +
        chalk.hex('#8abeb7')('https://github.com/evotai/evot/releases')
    )
    lines.push(border)
  }

  // What's New (shown once after update)
  if (releaseNotes && releaseNotes.length > 0) {
    lines.push('')
    lines.push(chalk.bold.hex('#8abeb7')("  What's New:"))
    for (const note of releaseNotes) {
      lines.push(chalk.hex('#808080')(`    • ${note}`))
    }
  }

  // Model line
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  lines.push('')
  lines.push(chalk.hex('#808080')(`  Model: ${truncate(modelLine, Math.max(8, columns - 12))}`))

  // Server info
  if (serverState) {
    lines.push(chalk.hex('#808080')(`  Server: ${serverState.address}`))
  }

  // API key warning
  if (configInfo && !configInfo.hasApiKey) {
    const envPath = configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'
    lines.push(chalk.hex('#ffff00')(`  ⚠ No API key — edit ${envPath}`))
  }

  lines.push('')
  return lines.flatMap(line => wrapBannerLine(line, columns)).join('\n')
}

import chalk from 'chalk'
import stringWidth from 'string-width'
import { version, type ConfigInfo } from '../native/index.js'
import { getSkillNames, getContextFiles } from './banner-skills.js'

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

export interface BannerOptions {
  model: string
  cwd: string
  configInfo: ConfigInfo | undefined
  columns: number
  serverState?: { port: number; address: string; channels: string[] } | null
  quiet?: boolean
  /** Release notes to show after an update (What's New) */
  releaseNotes?: string[] | null
}

export function renderBanner(opts: BannerOptions): string {
  if (opts.quiet) return ''

  const { model, cwd, configInfo, columns, serverState, releaseNotes } = opts
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const ver = version()
  const maxRight = columns - 14 // logo width + gap

  const lines: string[] = []

  const logo = [
    ' ▗██████▖ ',
    '▐████████▌',
    ' ▀██▀▀██▀ ',
  ]

  // Line 1: logo + name + version
  lines.push(chalk.hex('#3b82f6')(logo[0]!) + '  ' + chalk.bold('evot') + chalk.dim(` v${ver}`))

  // Line 2: logo + model · provider
  lines.push(chalk.hex('#3b82f6')(logo[1]!) + '  ' + chalk.dim(truncate(modelLine, maxRight)))

  // Line 3: logo + context/skills summary
  const skills = getSkillNames()
  const contextFiles = getContextFiles(cwd)
  const infoParts: string[] = []
  if (contextFiles.length > 0) {
    infoParts.push(contextFiles.join(', '))
  }
  if (skills.length > 0) {
    infoParts.push(`skills: ${skills.join(', ')}`)
  }
  const infoLine = infoParts.join('  ·  ')
  lines.push(chalk.hex('#3b82f6')(logo[2]!) + '  ' + chalk.dim(truncate(infoLine, maxRight)))

  // Server info
  if (serverState) {
    lines.push(chalk.dim(`  server: ${serverState.address}`))
  }

  // API key warning
  if (configInfo && !configInfo.hasApiKey) {
    const envPath = configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'
    lines.push(chalk.yellow(`  ⚠ No API key — edit ${envPath}`))
  }

  // Help hints
  lines.push(chalk.dim('  /help · Tab · ↑↓ history · Ctrl+C×2 exit'))

  // What's New (shown once after update)
  if (releaseNotes && releaseNotes.length > 0) {
    lines.push('')
    lines.push(chalk.bold.hex('#3b82f6')("  What's New:"))
    for (const note of releaseNotes) {
      lines.push(chalk.dim(`    • ${note}`))
    }
  }

  lines.push('')
  return lines.join('\n')
}

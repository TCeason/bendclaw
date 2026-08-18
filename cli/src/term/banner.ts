import { existsSync } from 'fs'
import { homedir } from 'os'
import { join } from 'path'
import chalk from 'chalk'
import { getSkillEntries } from '../commands/skill.js'
import type { ConfigInfo } from '../native/index.js'
import { getTheme } from '../render/theme.js'
import { wrapTextWithAnsi } from '../render/wrap.js'

const MUTED = '#808080'
const LOGO_MIN_COLUMNS = 50
const PROMPT_RESERVED_ROWS = 5
const PROJECT_CONTEXT_FILES = ['EVOT.md', 'CLAUDE.md', 'AGENTS.md']
const EVOT_LOGO = [
  ' ███████╗██╗   ██╗ ██████╗ ████████╗',
  ' ██╔════╝██║   ██║██╔═══██╗╚══██╔══╝',
  ' █████╗  ██║   ██║██║   ██║   ██║   ',
  ' ██╔══╝  ╚██╗ ██╔╝██║   ██║   ██║   ',
  ' ███████╗ ╚████╔╝ ╚██████╔╝   ██║   ',
  ' ╚══════╝  ╚═══╝   ╚═════╝    ╚═╝   ',
]

function formatPath(path: string): string {
  const home = homedir()
  if (path === home) return '~'
  return path.startsWith(`${home}/`) ? `~${path.slice(home.length)}` : path
}

function getContextFiles(cwd: string): string[] {
  return PROJECT_CONTEXT_FILES.filter(name => existsSync(join(cwd, name)))
}

function renderLogo(version: string): string[] {
  const theme = getTheme()
  return [
    ...EVOT_LOGO.map(line => theme.brandBold.paint(line.trimEnd())),
    `  ${chalk.dim(`v${version}`)}`,
  ]
}

function renderSection(title: string, values: string[], columns: number): string[] {
  if (values.length === 0) return []

  const valueWidth = Math.max(1, columns - 4)
  const valueLines = wrapTextWithAnsi(values.join(', '), valueWidth)
  return [
    getTheme().accent.paint(`  [${title}]`),
    ...valueLines.map(line => chalk.hex(MUTED)(`    ${line}`)),
  ]
}

function appendBlock(lines: string[], block: string[]): void {
  if (block.length === 0) return
  if (lines.length > 0) lines.push('')
  lines.push(...block)
}

function wrapBannerLines(lines: string[], columns: number): string[] {
  const width = Math.max(1, columns)
  return lines.flatMap(line => wrapTextWithAnsi(line, width))
}

export interface BannerOptions {
  version: string
  model: string
  cwd: string
  configInfo: ConfigInfo | undefined
  columns: number
  rows?: number
  serverState?: { port: number; address: string; channels: string[] } | null
  quiet?: boolean
  /** Release notes to show after an update (What's New) */
  releaseNotes?: string[] | null
  /** Update available info */
  updateAvailable?: { version: string } | null
  /** Fully resolved, ordered skill directories from the agent. */
  skillsDirs?: string[]
}

export function renderBanner(opts: BannerOptions): string {
  if (opts.quiet) return ''

  const {
    version,
    cwd,
    configInfo,
    columns,
    rows = Number.POSITIVE_INFINITY,
    serverState,
    releaseNotes,
    updateAvailable,
    skillsDirs,
  } = opts

  const detailLines: string[] = []
  appendBlock(detailLines, renderSection('Context', getContextFiles(cwd), columns))
  appendBlock(
    detailLines,
    renderSection('Skills', getSkillEntries(skillsDirs).map(entry => entry.name), columns),
  )
  if (serverState) {
    appendBlock(detailLines, renderSection('Server', [serverState.address], columns))
  }

  if (detailLines.length > 0) detailLines.push('')
  detailLines.push(chalk.dim('  Esc interrupt  ·  / commands  ·  Ctrl+O expand  ·  Ctrl+D exit'))

  if (updateAvailable) {
    detailLines.push('')
    const border = chalk.hex('#ffff00')('  ' + '─'.repeat(Math.max(1, Math.min(columns - 4, 72))))
    detailLines.push(border)
    detailLines.push(chalk.bold.hex('#ffff00')('  Update Available'))
    detailLines.push(
      chalk.hex(MUTED)(`  New version ${updateAvailable.version} is available. Run `) +
        chalk.hex('#8abeb7')('evot update'),
    )
    detailLines.push(
      chalk.hex(MUTED)('  Changelog: ') +
        chalk.hex('#8abeb7')('https://github.com/evotai/evot/releases'),
    )
    detailLines.push(border)
  }

  if (releaseNotes && releaseNotes.length > 0) {
    detailLines.push('')
    detailLines.push(chalk.bold.hex('#8abeb7')("  What's New:"))
    for (const note of releaseNotes) {
      detailLines.push(chalk.hex(MUTED)(`    • ${note}`))
    }
  }

  if (configInfo && !configInfo.hasApiKey) {
    const envPath = configInfo.envPath ? formatPath(configInfo.envPath) : '.env'
    detailLines.push('')
    detailLines.push(chalk.hex('#ffff00')(`  ⚠ No API key — edit ${envPath}`))
  }

  const fullBannerLines = wrapBannerLines(
    [...renderLogo(version), '', ...detailLines, ''],
    columns,
  )
  const showLogo = columns >= LOGO_MIN_COLUMNS &&
    fullBannerLines.length + PROMPT_RESERVED_ROWS <= rows
  const brandLines = showLogo
    ? renderLogo(version)
    : [`  ${getTheme().brandBold.paint('evot')} ${chalk.dim(`v${version}`)}`]

  return wrapBannerLines([...brandLines, '', ...detailLines, ''], columns).join('\n')
}

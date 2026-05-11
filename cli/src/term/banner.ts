import chalk from 'chalk'
import stringWidth from 'string-width'
import { version, type ConfigInfo } from '../native/index.js'

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

function getGitBranch(cwd: string): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--abbrev-ref', 'HEAD'], {
      cwd,
      stdout: 'pipe',
      stderr: 'pipe',
    })
    if (result.exitCode === 0) {
      return result.stdout.toString().trim()
    }
  } catch {}
  return null
}


export function renderBanner(
  model: string,
  cwd: string,
  configInfo: ConfigInfo | undefined,
  columns: number,
  serverState?: { port: number; address: string; channels: string[] } | null,
): string {
  const shortCwd = cwd.replace(process.env.HOME ?? '', '~')
  const gitBranch = getGitBranch(cwd)
  const provider = configInfo?.provider ?? ''
  const modelLine = provider ? `${model} · ${provider}` : model
  const cwdLine = gitBranch ? `${gitBranch} · ${shortCwd}` : shortCwd
  const ver = version()

  const lines: string[] = []

  const logo = [
    ' ▗██████▖ ',
    '▐████████▌',
    ' ▀██▀▀██▀ ',
  ]

  lines.push(chalk.hex('#3b82f6')(logo[0]!) + '  ' + chalk.bold('evot') + chalk.dim(` v${ver}`))
  lines.push(chalk.hex('#3b82f6')(logo[1]!) + '  ' + chalk.dim(truncate(modelLine, columns - 14)))
  lines.push(chalk.hex('#3b82f6')(logo[2]!) + '  ' + chalk.dim(truncate(cwdLine, columns - 14)))

  if (serverState) {
    lines.push(chalk.dim(`  server: ${serverState.address}`))
  }

  if (configInfo && !configInfo.hasApiKey) {
    const envPath = configInfo.envPath?.replace(process.env.HOME ?? '', '~') ?? '.env'
    lines.push(chalk.yellow(`  ⚠ No API key configured — edit ${envPath}`))
  }
  lines.push(chalk.dim('  /help commands  ·  Tab complete  ·  ↑↓ history  ·  Ctrl+C×2 exit'))
  lines.push('')

  return lines.join('\n')
}

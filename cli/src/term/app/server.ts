import { startServerBackground, type ServerInfo } from '../../native/index.js'
import { spawn } from 'child_process'

export interface ServerState {
  port: number
  address: string
  channels: string[]
  startedAt: number
}

let activePort: number | null = null

/**
 * Decide how to open a URL in the default browser for the given platform, or
 * return null when auto-open should be skipped (EVOT_NO_OPEN set). Pure so it
 * can be unit-tested without spawning anything.
 */
export function browserOpenCommand(
  url: string,
  platform: NodeJS.Platform = process.platform,
  env: NodeJS.ProcessEnv = process.env,
): { cmd: string; args: string[] } | null {
  if (env.EVOT_NO_OPEN) return null
  if (platform === 'darwin') return { cmd: 'open', args: [url] }
  if (platform === 'win32') return { cmd: 'cmd', args: ['/C', 'start', '', url] }
  return { cmd: 'xdg-open', args: [url] }
}

/**
 * Open the dashboard URL in the default browser. Best-effort and non-blocking:
 * the child is detached and any failure (headless/SSH/CI) is swallowed, mirroring
 * the native `evot serve` auto-open.
 */
function openDashboard(url: string): void {
  const plan = browserOpenCommand(url)
  if (plan === null) return
  try {
    const child = spawn(plan.cmd, plan.args, { stdio: 'ignore', detached: true })
    child.on('error', () => { /* no browser available — ignore */ })
    child.unref()
  } catch { /* spawn failed — ignore */ }
}

export async function tryStartServer(port?: number, envFile?: string): Promise<ServerState | null> {
  const info = await startServerBackground(port, undefined, envFile)
  if (info === null) return null
  activePort = info.port
  // Only reached when this process actually bound the port and started a fresh
  // server (null means another instance already owns it), so this is the right
  // moment to surface the dashboard.
  openDashboard(info.address)
  return {
    port: info.port,
    address: info.address,
    channels: info.channels,
    startedAt: Date.now(),
  }
}

export function formatUptime(startedAt: number): string {
  const elapsed = Math.floor((Date.now() - startedAt) / 1000)
  if (elapsed < 60) return `${elapsed}s`
  const minutes = Math.floor(elapsed / 60)
  const seconds = elapsed % 60
  if (minutes < 60) return `${minutes}m${seconds.toString().padStart(2, '0')}s`
  const hours = Math.floor(minutes / 60)
  const remainMinutes = minutes % 60
  return `${hours}h${remainMinutes.toString().padStart(2, '0')}m`
}

export function terminalTitle(prefix?: string): string {
  const suffix = activePort ? ` · :${activePort}` : ''
  return prefix ? `${prefix} Evot${suffix}` : `Evot${suffix}`
}

export function setTerminalTitle(prefix?: string): void {
  process.stdout.write(`\x1b]0;${terminalTitle(prefix)}\x07`)
}

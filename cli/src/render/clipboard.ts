/**
 * Clipboard write with graceful fallbacks, aligned with pi's approach.
 *
 * Order: platform tool (pbcopy/clip/wl-copy/xclip/xsel) → OSC 52 escape.
 * OSC 52 is also emitted for remote sessions (SSH/mosh) so the copy lands on
 * the local machine's clipboard, not the remote host. evot ships no native
 * clipboard addon, so we rely entirely on these mechanisms.
 */

import { execSync, spawn } from 'child_process'
import { platform } from 'os'

const MAX_OSC52_ENCODED_LENGTH = 100_000

type ExecOptions = { input: string; timeout: number; stdio: ['pipe', 'ignore', 'ignore'] }

function isRemoteSession(env: NodeJS.ProcessEnv = process.env): boolean {
  return Boolean(env.SSH_CONNECTION || env.SSH_CLIENT || env.MOSH_CONNECTION)
}

function emitOsc52(text: string): boolean {
  const encoded = Buffer.from(text).toString('base64')
  if (encoded.length > MAX_OSC52_ENCODED_LENGTH) return false
  process.stdout.write(`\x1b]52;c;${encoded}\x07`)
  return true
}

function copyViaX11(options: ExecOptions): void {
  try {
    execSync('xclip -selection clipboard', options)
  } catch {
    execSync('xsel --clipboard --input', options)
  }
}

function copyViaWayland(text: string): boolean {
  try {
    execSync('which wl-copy', { stdio: 'ignore' })
    // wl-copy hangs under execSync due to fork behavior; use spawn + unref.
    const proc = spawn('wl-copy', [], { stdio: ['pipe', 'ignore', 'ignore'] })
    proc.stdin.on('error', () => { /* ignore EPIPE if wl-copy exits early */ })
    proc.stdin.write(text)
    proc.stdin.end()
    proc.unref()
    return true
  } catch {
    return false
  }
}

/**
 * Copy `text` to the system clipboard. Throws if every mechanism fails.
 */
export async function copyToClipboard(text: string): Promise<void> {
  const p = platform()
  const remote = isRemoteSession()
  const options: ExecOptions = { input: text, timeout: 5000, stdio: ['pipe', 'ignore', 'ignore'] }
  let copied = false

  if (!remote) {
    try {
      if (p === 'darwin') {
        execSync('pbcopy', options)
        copied = true
      } else if (p === 'win32') {
        execSync('clip', options)
        copied = true
      } else {
        // Linux: Termux, then Wayland, then X11.
        if (process.env.TERMUX_VERSION) {
          try {
            execSync('termux-clipboard-set', options)
            copied = true
          } catch { /* fall through */ }
        }
        if (!copied) {
          const hasWayland = Boolean(process.env.WAYLAND_DISPLAY)
          const hasX11 = Boolean(process.env.DISPLAY)
          if (hasWayland && copyViaWayland(text)) {
            copied = true
          } else if (hasX11) {
            copyViaX11(options)
            copied = true
          }
        }
      }
    } catch {
      // Fall through to OSC 52.
    }
  }

  // OSC 52 for remote sessions or when no platform tool worked.
  if (remote || !copied) {
    if (emitOsc52(text)) copied = true
  }

  if (!copied) throw new Error('Failed to copy to clipboard')
}

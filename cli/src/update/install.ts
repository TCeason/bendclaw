/**
 * Execute the install script to update evot.
 */

import { homedir } from 'os'
import { basename, dirname, join } from 'path'

const INSTALL_SCRIPT = 'https://raw.githubusercontent.com/evotai/evot/main/install.sh'

function errorMessage(err: unknown): string {
  return err instanceof Error ? err.message : String(err)
}

function installRoot(env: Record<string, string>): string {
  if (env.EVOT_INSTALL_DIR) {
    return basename(env.EVOT_INSTALL_DIR) === 'bin'
      ? dirname(env.EVOT_INSTALL_DIR)
      : env.EVOT_INSTALL_DIR
  }
  return env.EVOT_HOME || join(homedir(), '.evotai')
}

async function verifyInstalledVersion(
  expectedVersion: string,
  env: Record<string, string>,
): Promise<{ success: boolean; output: string }> {
  const root = installRoot(env)
  const installDir = env.EVOT_INSTALL_DIR || join(root, 'bin')
  const binary = join(installDir, 'evot')
  const proc = Bun.spawn([binary, '--version'], {
    stdout: 'pipe',
    stderr: 'pipe',
    env: { ...env, EVOT_HOME: root },
  })
  const [stdout, stderr, exitCode] = await Promise.all([
    new Response(proc.stdout).text(),
    new Response(proc.stderr).text(),
    proc.exited,
  ])
  const output = (stderr || stdout).trim()
  if (exitCode !== 0) {
    return {
      success: false,
      output: `installed evot failed verification (exit code ${exitCode})${output ? `: ${output}` : ''}`,
    }
  }

  const actual = stdout.trim()
  const expected = `evot v${expectedVersion}`
  if (actual !== expected) {
    return {
      success: false,
      output: `installed version mismatch: expected ${expected}, got ${actual || '(empty output)'}`,
    }
  }
  return { success: true, output: actual }
}

export async function executeInstall(tag?: string): Promise<{ success: boolean; output: string }> {
  try {
    const env: Record<string, string> = { ...process.env as Record<string, string> }
    if (tag) {
      env.EVOT_INSTALL_VERSION = tag
    }

    // Fetch first, then pass the complete script to sh. A `curl | sh` pipeline
    // can return success when curl fails because POSIX sh has no pipefail.
    const response = await fetch(INSTALL_SCRIPT)
    if (!response.ok) {
      return { success: false, output: `failed to download install script: HTTP ${response.status}` }
    }
    const script = await response.text()
    if (!script.trim()) {
      return { success: false, output: 'failed to download install script: empty response' }
    }

    const proc = Bun.spawn(['sh'], {
      stdin: new Blob([script]),
      stdout: 'pipe',
      stderr: 'pipe',
      env,
    })

    const [stdout, stderr, exitCode] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
      proc.exited,
    ])

    if (exitCode !== 0) {
      return { success: false, output: stderr || stdout || `exit code ${exitCode}` }
    }

    if (tag) {
      const verification = await verifyInstalledVersion(tag.replace(/^v/, ''), env)
      if (!verification.success) return verification
    }
    return { success: true, output: stdout }
  } catch (err: unknown) {
    return { success: false, output: errorMessage(err) || 'failed to run install script' }
  }
}

/**
 * Execute the install script to update evot.
 */

const INSTALL_SCRIPT = 'https://raw.githubusercontent.com/evotai/evot/main/install.sh'

export async function executeInstall(tag?: string): Promise<{ success: boolean; output: string }> {
  try {
    const env: Record<string, string> = { ...process.env as Record<string, string> }
    if (tag) {
      env.EVOT_INSTALL_VERSION = tag
    }

    const proc = Bun.spawn(['bash', '-c', `curl -fsSL ${INSTALL_SCRIPT} | bash`], {
      stdout: 'pipe',
      stderr: 'pipe',
      env,
    })

    const [stdout, stderr] = await Promise.all([
      new Response(proc.stdout).text(),
      new Response(proc.stderr).text(),
    ])
    const exitCode = await proc.exited

    if (exitCode === 0) {
      return { success: true, output: stdout }
    }
    return { success: false, output: stderr || stdout || `exit code ${exitCode}` }
  } catch (err: any) {
    return { success: false, output: err?.message ?? 'failed to run install script' }
  }
}

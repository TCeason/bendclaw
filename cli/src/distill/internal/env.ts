/** Workspace process environment helpers.
 *
 * Setup and verifier commands should run as if the workspace's local tooling is
 * active. In Python projects the Builder often creates `.venv`; putting
 * `.venv/bin` first makes LLM-generated `python`, `pytest`, etc. resolve to
 * that workspace instead of the host (or fail on macOS where `python` may not
 * exist).
 */

import { existsSync } from 'node:fs'
import { join } from 'node:path'

export function workspaceEnv(cwd: string): NodeJS.ProcessEnv {
  const pathParts: string[] = []
  const venvBin = join(cwd, '.venv', 'bin')
  const nodeBin = join(cwd, 'node_modules', '.bin')
  if (existsSync(venvBin)) pathParts.push(venvBin)
  if (existsSync(nodeBin)) pathParts.push(nodeBin)
  pathParts.push(process.env.PATH ?? '')
  return {
    ...process.env,
    PATH: pathParts.filter(Boolean).join(':'),
    VIRTUAL_ENV: existsSync(venvBin) ? join(cwd, '.venv') : process.env.VIRTUAL_ENV,
  }
}

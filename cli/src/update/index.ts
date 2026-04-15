/**
 * Update module — public API.
 */

export type { CheckResult, RunResult, ReleaseInfo } from './types.js'
export { checkForUpdate, fetchLatestStable, isNewer } from './check.js'
export { executeInstall } from './install.js'
export { UpdateManager } from './manager.js'

import type { RunResult } from './types.js'
import { checkForUpdate } from './check.js'
import { executeInstall } from './install.js'

/**
 * Force-check for updates and install if available.
 * Used by `/update` and `evot update`.
 */
export async function runUpdate(currentVersion: string): Promise<RunResult> {
  const result = await checkForUpdate(currentVersion, { force: true })

  if (result.kind === 'error') {
    return { kind: 'error', message: result.message }
  }
  if (result.kind === 'up_to_date') {
    return { kind: 'up_to_date' }
  }

  const installResult = await executeInstall(result.latest.tag)
  if (installResult.success) {
    return { kind: 'updated', from: currentVersion, to: result.latest.version }
  }
  return { kind: 'error', message: installResult.output }
}

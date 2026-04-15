/**
 * UpdateManager — automatic update check scheduler with event emission.
 * Only responsible for checking; does not install.
 *
 * Events:
 *   'update-available' → ReleaseInfo
 */

import { EventEmitter } from 'events'
import type { ReleaseInfo } from './types.js'
import { checkForUpdate } from './check.js'

const INITIAL_DELAY = 10_000       // 10s after start
const CHECK_THROTTLE = 5 * 60_000  // 5 min between checks
const PERIODIC_CHECK = 10 * 60_000 // 10 min interval

export class UpdateManager extends EventEmitter {
  private currentVersion: string
  private initialTimer: ReturnType<typeof setTimeout> | null = null
  private periodicTimer: ReturnType<typeof setInterval> | null = null
  private lastCheckTime = 0
  private lastNotifiedVersion: string | null = null

  constructor(currentVersion: string) {
    super()
    this.currentVersion = currentVersion
  }

  /** Start the scheduler: delayed first check + periodic checks. */
  start(): void {
    this.initialTimer = setTimeout(() => {
      this.checkForUpdate()
    }, INITIAL_DELAY)

    this.periodicTimer = setInterval(() => {
      this.checkForUpdate()
    }, PERIODIC_CHECK)
  }

  /** Trigger a check (throttled to avoid excessive requests). */
  checkForUpdate(): void {
    const now = Date.now()
    if (now - this.lastCheckTime < CHECK_THROTTLE) return
    this.lastCheckTime = now

    checkForUpdate(this.currentVersion, { force: true })
      .then((result) => {
        if (result.kind === 'available' && result.latest.version !== this.lastNotifiedVersion) {
          this.lastNotifiedVersion = result.latest.version
          this.emit('update-available', result.latest)
        }
      })
      .catch(() => { /* silent */ })
  }

  /** Clean up timers. */
  cleanup(): void {
    if (this.initialTimer) {
      clearTimeout(this.initialTimer)
      this.initialTimer = null
    }
    if (this.periodicTimer) {
      clearInterval(this.periodicTimer)
      this.periodicTimer = null
    }
  }
}

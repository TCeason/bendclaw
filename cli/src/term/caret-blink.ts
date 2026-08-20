/** Idle blink for the composer caret: solid while typing, blinks once quiet. */

export const CARET_IDLE_DELAY_MS = 500
export const CARET_BLINK_INTERVAL_MS = 500

export interface CaretBlinkOptions {
  onChange: () => void
  idleDelayMs?: number
  intervalMs?: number
}

function unref(timer: ReturnType<typeof setTimeout>): void {
  const handle = timer as unknown as { unref?: () => void }
  handle.unref?.()
}

export class CaretBlink {
  private readonly onChange: () => void
  private readonly idleDelayMs: number
  private readonly intervalMs: number
  private idleTimer: ReturnType<typeof setTimeout> | null = null
  private blinkTimer: ReturnType<typeof setInterval> | null = null
  private on = true
  private enabled = true
  private disposed = false

  constructor(options: CaretBlinkOptions) {
    this.onChange = options.onChange
    this.idleDelayMs = options.idleDelayMs ?? CARET_IDLE_DELAY_MS
    this.intervalMs = options.intervalMs ?? CARET_BLINK_INTERVAL_MS
    this.scheduleIdle()
  }

  get visible(): boolean {
    return this.on
  }

  /** Editor activity: back to solid, restart the countdown. */
  bump(): void {
    if (this.disposed) return
    const wasOff = !this.on
    this.on = true
    this.clearTimers()
    this.scheduleIdle()
    // The caller already repaints; only ask for a frame when the caret changed.
    if (wasOff) this.onChange()
  }

  /**
   * Hold solid without blinking, for when an overlay owns the screen. Silent by
   * design: called from the frame builder, so this frame paints the new phase.
   */
  setEnabled(enabled: boolean): void {
    if (this.disposed || this.enabled === enabled) return
    this.enabled = enabled
    this.on = true
    this.clearTimers()
    if (enabled) this.scheduleIdle()
  }

  dispose(): void {
    this.disposed = true
    this.clearTimers()
    this.on = true
  }

  private scheduleIdle(): void {
    if (this.disposed || !this.enabled) return
    this.idleTimer = setTimeout(() => {
      this.idleTimer = null
      this.blinkTimer = setInterval(() => {
        this.on = !this.on
        this.onChange()
      }, this.intervalMs)
      unref(this.blinkTimer)
    }, this.idleDelayMs)
    unref(this.idleTimer)
  }

  private clearTimers(): void {
    if (this.idleTimer) {
      clearTimeout(this.idleTimer)
      this.idleTimer = null
    }
    if (this.blinkTimer) {
      clearInterval(this.blinkTimer)
      this.blinkTimer = null
    }
  }
}

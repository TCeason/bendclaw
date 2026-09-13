import stripAnsi from 'strip-ansi'
import type { OutputLine } from '../render/output.js'
import type { ShareNotice } from '../native/contracts/share.js'

/** Ordered display-only persistence. Failures are surfaced by flush before sharing. */
export class ShareNotices {
  private tail: Promise<void> = Promise.resolve()
  private failure: Error | null = null

  constructor(private readonly write: (sessionId: string, notices: ShareNotice[]) => Promise<void>) {}

  record(sessionId: string | null, lines: OutputLine[]): void {
    if (!sessionId) return // Startup notices have no unambiguous session owner.
    const notices: ShareNotice[] = []
    for (const line of lines) {
      if (line.id.startsWith('sys-share')) continue
      const text = stripAnsi(line.text)
      if (line.shareEvents?.length) {
        for (const event of line.shareEvents) {
          notices.push({
            schema_version: 1,
            level: 'system',
            text,
            timestamp: Date.now(),
            kind: event.kind,
            data: event.data,
          })
        }
        continue
      }
      // Model changes with no structured event are already persisted by the
      // next run's session update. Do not turn them into a duplicate notice.
      if (line.id === 'sys-model') continue
      if (line.kind !== 'error' && line.kind !== 'system' && line.kind !== 'cancelled') continue
      notices.push({ schema_version: 1, level: line.kind, text, timestamp: Date.now() })
    }
    if (!notices.length) return
    this.tail = this.tail.then(async () => {
      try { await this.write(sessionId, notices) } catch (error) {
        // Keep the first cause: later batches fail for the same reason.
        this.failure ??= error instanceof Error ? error : new Error(String(error))
      }
    })
  }

  /**
   * Wait for pending writes and report the first failure.
   *
   * The failure is cleared as it is reported, so a transient storage error
   * blocks the share that would have been incomplete, not every later one.
   */
  async flush(): Promise<void> {
    await this.tail
    const failure = this.failure
    if (!failure) return
    this.failure = null
    throw new Error(`Some client notices could not be saved (${failure.message}); refusing an incomplete share`)
  }
}

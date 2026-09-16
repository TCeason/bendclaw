import {
  appendFileSync,
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  unlinkSync,
  writeFileSync,
} from 'fs'
import { randomBytes } from 'crypto'
import { join } from 'path'

const MANIFEST_FILE = 'manifest.json'
const ACTIVE_FILE = '.active'
const SEGMENT_PATTERN = /^(\d{6})\.jsonl$/
const RUN_PATTERN = /^run-\d{8}T\d{9}Z-\d+-[0-9a-f]{8}$/

export interface RollingLogOptions {
  /** Dedicated root owned by this log stream, e.g. ~/.evotai/logs/renderer. */
  rootDirectory: string
  /** Namespace recorded in manifests; retention only touches matching runs. */
  namespace: string
  /** Maximum bytes per segment before the next append rolls to a new file. */
  segmentMaxBytes: number
  /** Number of newest segments retained in the active run. */
  retainSegments: number
  /** Number of newest closed runs retained under rootDirectory. */
  retainRuns: number
  /** Diagnostic metadata written once to the run manifest. */
  metadata?: Record<string, unknown>
}

export interface RollingAppendOptions {
  /** Record written first when this append opens a new segment. */
  segmentPrelude?: () => string | null
}

interface RunManifest {
  schemaVersion: 1
  namespace: string
  createdAt: string
  pid: number
  metadata?: Record<string, unknown>
}

function normalizedPositiveInteger(value: number, fallback: number): number {
  return Number.isFinite(value) && value > 0 ? Math.max(1, Math.floor(value)) : fallback
}

function runTimestamp(date = new Date()): string {
  return date.toISOString().replace(/[-:]/g, '').replace('.', '').replace(/Z$/, 'Z')
}

function ownerOnlyDirectory(path: string): void {
  mkdirSync(path, { recursive: true, mode: 0o700 })
  chmodSync(path, 0o700)
}

function ownerOnlyFile(path: string, content: string): void {
  writeFileSync(path, content, { mode: 0o600 })
  chmodSync(path, 0o600)
}

function jsonRecord(record: string): string {
  return record.endsWith('\n') ? record : `${record}\n`
}

function processIsRunning(pid: number): boolean {
  if (!Number.isInteger(pid) || pid <= 0) return false
  try {
    process.kill(pid, 0)
    return true
  } catch (error: any) {
    // EPERM means the process exists but cannot be signalled by this user.
    return error?.code === 'EPERM'
  }
}

function runIsActive(path: string): boolean {
  const marker = join(path, ACTIVE_FILE)
  if (!existsSync(marker)) return false
  try {
    const pid = Number(readFileSync(marker, 'utf8').trim())
    if (processIsRunning(pid)) return true
    // The owner process is gone; remove its stale marker so normal closed-run
    // retention can manage the diagnostic run.
    unlinkSync(marker)
    return false
  } catch {
    // An unreadable marker is treated as active: retaining data is safer than
    // deleting a run whose ownership cannot be established.
    return true
  }
}

/**
 * Generic owner-only rolling JSONL storage.
 *
 * The writer manages only run directories it created under its dedicated root.
 * Retention requires a strict run name and a matching manifest namespace, so it
 * cannot remove adjacent application logs. Active runs are never removed.
 */
export class RollingLogWriter {
  readonly runDirectory: string

  private readonly segmentMaxBytes: number
  private readonly retainSegments: number
  private readonly retainRuns: number
  private segmentIndex = 0
  private segmentPath: string | null = null
  private segmentBytes = 0
  private closed = false

  constructor(private readonly options: RollingLogOptions) {
    this.segmentMaxBytes = normalizedPositiveInteger(options.segmentMaxBytes, 4 * 1024 * 1024)
    this.retainSegments = normalizedPositiveInteger(options.retainSegments, 4)
    this.retainRuns = normalizedPositiveInteger(options.retainRuns, 10)

    ownerOnlyDirectory(options.rootDirectory)
    this.runDirectory = this.createRunDirectory()
    this.cleanupClosedRuns()
  }

  append(record: string, appendOptions: RollingAppendOptions = {}): void {
    if (this.closed) return
    try {
      const serialized = jsonRecord(record)
      const recordBytes = Buffer.byteLength(serialized)
      const shouldRoll = this.segmentPath === null
        || (this.segmentBytes > 0 && this.segmentBytes + recordBytes > this.segmentMaxBytes)
      if (shouldRoll) this.openNextSegment()

      const initializesSegment = shouldRoll || this.segmentBytes === 0
      const prelude = initializesSegment ? appendOptions.segmentPrelude?.() : null
      if (initializesSegment) {
        this.writeInitialCurrent((prelude ? jsonRecord(prelude) : '') + serialized)
        this.cleanupOldSegments()
      } else {
        this.appendToCurrent(serialized)
      }
    } catch {
      // Diagnostic storage must never affect the application using it.
    }
  }

  close(): void {
    if (this.closed) return
    this.closed = true
    try {
      unlinkSync(join(this.runDirectory, ACTIVE_FILE))
    } catch {
      // Best effort; a stale marker keeps the run rather than risking data loss.
    }
  }

  private createRunDirectory(): string {
    for (let attempt = 0; attempt < 10; attempt++) {
      const random = randomBytes(4).toString('hex')
      const name = `run-${runTimestamp()}-${process.pid}-${random}`
      const directory = join(this.options.rootDirectory, name)
      const temporary = join(this.options.rootDirectory, `.${name}.tmp`)
      if (existsSync(directory) || existsSync(temporary)) continue

      try {
        ownerOnlyDirectory(temporary)
        const manifest: RunManifest = {
          schemaVersion: 1,
          namespace: this.options.namespace,
          createdAt: new Date().toISOString(),
          pid: process.pid,
          metadata: this.options.metadata,
        }
        ownerOnlyFile(join(temporary, MANIFEST_FILE), `${JSON.stringify(manifest, null, 2)}\n`)
        ownerOnlyFile(join(temporary, ACTIVE_FILE), `${process.pid}\n`)
        renameSync(temporary, directory)
        return directory
      } catch (error) {
        try {
          rmSync(temporary, { recursive: true, force: true })
        } catch {
          // Best effort; hidden temporary directories are never retention targets.
        }
        if (existsSync(directory)) continue
        throw error
      }
    }
    throw new Error('Unable to allocate rolling log run directory')
  }

  private openNextSegment(): void {
    this.segmentIndex++
    this.segmentPath = join(this.runDirectory, `${this.segmentIndex.toString().padStart(6, '0')}.jsonl`)
    ownerOnlyFile(this.segmentPath, '')
    this.segmentBytes = 0
  }

  private appendToCurrent(serialized: string): void {
    if (!this.segmentPath) return
    appendFileSync(this.segmentPath, serialized, { mode: 0o600 })
    this.segmentBytes += Buffer.byteLength(serialized)
  }

  private writeInitialCurrent(serialized: string): void {
    if (!this.segmentPath) return
    writeFileSync(this.segmentPath, serialized, { mode: 0o600 })
    chmodSync(this.segmentPath, 0o600)
    this.segmentBytes = Buffer.byteLength(serialized)
  }

  private cleanupOldSegments(): void {
    let segments: { index: number; path: string }[] = []
    try {
      segments = readdirSync(this.runDirectory, { withFileTypes: true })
        .filter(entry => entry.isFile() && SEGMENT_PATTERN.test(entry.name))
        .map(entry => {
          const match = SEGMENT_PATTERN.exec(entry.name)
          return { index: Number(match?.[1] ?? 0), path: join(this.runDirectory, entry.name) }
        })
        .sort((a, b) => b.index - a.index)
    } catch {
      return
    }

    for (const segment of segments.slice(this.retainSegments)) {
      try {
        unlinkSync(segment.path)
      } catch {
        // Best effort.
      }
    }
  }

  private cleanupClosedRuns(): void {
    const runs: { name: string; path: string; createdAt: number }[] = []
    let entries: Array<import('fs').Dirent<string>>
    try {
      entries = readdirSync(this.options.rootDirectory, { withFileTypes: true })
    } catch {
      return
    }

    for (const entry of entries) {
      if (!entry.isDirectory() || !RUN_PATTERN.test(entry.name)) continue
      const path = join(this.options.rootDirectory, entry.name)
      if (path === this.runDirectory) continue
      try {
        const raw = readFileSync(join(path, MANIFEST_FILE), 'utf8')
        const manifest = JSON.parse(raw) as Partial<RunManifest>
        if (manifest.schemaVersion !== 1 || manifest.namespace !== this.options.namespace) continue
        if (runIsActive(path)) continue
        const createdAt = Date.parse(manifest.createdAt ?? '')
        if (!Number.isFinite(createdAt)) continue
        runs.push({ name: entry.name, path, createdAt })
      } catch {
        // Unknown or malformed directories are not managed by this writer.
      }
    }

    runs.sort((a, b) => b.createdAt - a.createdAt || b.name.localeCompare(a.name))
    for (const run of runs.slice(this.retainRuns)) {
      try {
        rmSync(run.path, { recursive: true, force: true })
      } catch {
        // Best effort.
      }
    }
  }
}

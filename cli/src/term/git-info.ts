import { execFile } from 'child_process'
import { dirname, join, resolve } from 'path'
import { existsSync, type FSWatcher, readFileSync, statSync, watch } from 'fs'

type GitPaths = {
  repoDir: string
  commonGitDir: string
  headPath: string
}

type BranchChangeCallback = () => void

const WATCH_DEBOUNCE_MS = 500
const WATCH_RETRY_MS = 5_000

function findGitPaths(cwd: string): GitPaths | null {
  let dir = resolve(cwd)

  while (true) {
    const gitPath = join(dir, '.git')
    if (existsSync(gitPath)) {
      try {
        const stat = statSync(gitPath)
        if (stat.isFile()) {
          const content = readFileSync(gitPath, 'utf8').trim()
          if (!content.startsWith('gitdir: ')) return null

          const gitDir = resolve(dir, content.slice('gitdir: '.length).trim())
          const headPath = join(gitDir, 'HEAD')
          if (!existsSync(headPath)) return null

          const commonDirPath = join(gitDir, 'commondir')
          const commonGitDir = existsSync(commonDirPath)
            ? resolve(gitDir, readFileSync(commonDirPath, 'utf8').trim())
            : gitDir
          return { repoDir: dir, commonGitDir, headPath }
        }

        if (stat.isDirectory()) {
          const headPath = join(gitPath, 'HEAD')
          if (!existsSync(headPath)) return null
          return { repoDir: dir, commonGitDir: gitPath, headPath }
        }
      } catch {
        return null
      }
    }

    const parent = dirname(dir)
    if (parent === dir) return null
    dir = parent
  }
}

function resolveBranchWithGitAsync(repoDir: string): Promise<string | null> {
  return new Promise(resolvePromise => {
    execFile(
      'git',
      ['--no-optional-locks', 'symbolic-ref', '--quiet', '--short', 'HEAD'],
      { cwd: repoDir, encoding: 'utf8' },
      (error, stdout) => {
        if (error) {
          resolvePromise(null)
          return
        }
        const branch = stdout.trim()
        resolvePromise(branch || null)
      }
    )
  })
}

export class GitInfoProvider {
  private cwd: string
  private gitPaths: GitPaths | null
  private cachedRepo: string | null = null
  private cachedBranch: string | null = null
  private headWatcher: FSWatcher | null = null
  private reftableWatcher: FSWatcher | null = null
  private refreshTimer: ReturnType<typeof setTimeout> | null = null
  private retryTimer: ReturnType<typeof setTimeout> | null = null
  private refreshInFlight = false
  private refreshPending = false
  private disposed = false
  private callbacks = new Set<BranchChangeCallback>()

  constructor(cwd: string) {
    this.cwd = cwd
    this.gitPaths = findGitPaths(cwd)
    this.refreshSync()
    this.setupWatchers()
  }

  getRepo(): string | null {
    return this.cachedRepo
  }

  getBranch(): string | null {
    return this.cachedBranch
  }

  onChange(callback: BranchChangeCallback): () => void {
    this.callbacks.add(callback)
    return () => this.callbacks.delete(callback)
  }

  /**
   * Re-read repository metadata immediately. Tool subprocesses can switch HEAD
   * and finish before the debounced filesystem watcher fires; callers use this
   * at tool completion so the footer never shows the previous branch.
   */
  refresh(): boolean {
    if (this.disposed) return false
    const changed = this.refreshSync()
    if (changed) this.notifyChange()
    return changed
  }

  setCwd(cwd: string): void {
    if (this.cwd === cwd) return
    this.cwd = cwd
    this.clearTimers()
    this.clearWatchers()
    this.gitPaths = findGitPaths(cwd)
    const changed = this.refreshSync()
    this.setupWatchers()
    if (changed) this.notifyChange()
  }

  dispose(): void {
    this.disposed = true
    this.clearTimers()
    this.clearWatchers()
    this.callbacks.clear()
  }

  private refreshSync(): boolean {
    const previousRepo = this.cachedRepo
    const previousBranch = this.cachedBranch
    this.cachedRepo = this.resolveRepo()
    this.cachedBranch = this.resolveBranchFromHeadSync()
    return previousRepo !== this.cachedRepo || previousBranch !== this.cachedBranch
  }

  private resolveRepo(): string | null {
    if (!this.gitPaths) return null
    return this.gitPaths.repoDir.split('/').pop() || null
  }

  private resolveBranchFromHeadSync(): string | null {
    try {
      if (!this.gitPaths) return null
      const content = readFileSync(this.gitPaths.headPath, 'utf8').trim()
      if (content.startsWith('ref: refs/heads/')) {
        const branch = content.slice('ref: refs/heads/'.length)
        if (branch === '.invalid') return this.cachedBranch ?? 'detached'
        return branch
      }
      return 'detached'
    } catch {
      return null
    }
  }

  private async resolveBranchAsync(): Promise<string | null> {
    try {
      if (!this.gitPaths) return null
      const content = readFileSync(this.gitPaths.headPath, 'utf8').trim()
      if (content.startsWith('ref: refs/heads/')) {
        const branch = content.slice('ref: refs/heads/'.length)
        if (branch === '.invalid') {
          return (await resolveBranchWithGitAsync(this.gitPaths.repoDir)) ?? 'detached'
        }
        return branch
      }
      return 'detached'
    } catch {
      return null
    }
  }

  private async refreshAsync(): Promise<void> {
    if (this.disposed) return
    if (this.refreshInFlight) {
      this.refreshPending = true
      return
    }

    this.refreshInFlight = true
    try {
      const nextRepo = this.resolveRepo()
      const nextBranch = await this.resolveBranchAsync()
      if (this.disposed) return

      const changed = this.cachedRepo !== nextRepo || this.cachedBranch !== nextBranch
      this.cachedRepo = nextRepo
      this.cachedBranch = nextBranch
      if (changed) this.notifyChange()
    } finally {
      this.refreshInFlight = false
      if (this.refreshPending && !this.disposed) {
        this.refreshPending = false
        this.scheduleRefresh()
      }
    }
  }

  private notifyChange(): void {
    for (const callback of this.callbacks) callback()
  }

  private scheduleRefresh(): void {
    if (this.disposed || this.refreshTimer) return
    if (this.refreshInFlight) {
      this.refreshPending = true
      return
    }
    this.refreshTimer = setTimeout(() => {
      this.refreshTimer = null
      void this.refreshAsync()
    }, WATCH_DEBOUNCE_MS)
  }

  private setupWatchers(): void {
    this.clearWatchers()
    if (!this.gitPaths) return

    this.headWatcher = this.watchDirectory(dirname(this.gitPaths.headPath), filename => {
      if (!filename || filename === 'HEAD') this.scheduleRefresh()
    })

    const reftableDir = join(this.gitPaths.commonGitDir, 'reftable')
    if (existsSync(reftableDir)) {
      this.reftableWatcher = this.watchDirectory(reftableDir, () => this.scheduleRefresh())
    }
  }

  private watchDirectory(path: string, onChange: (filename: string | null) => void): FSWatcher | null {
    try {
      const watcher = watch(path, (_eventType, filename) => {
        onChange(typeof filename === 'string' ? filename : null)
      })
      watcher.on('error', () => this.handleWatcherError())
      return watcher
    } catch {
      this.scheduleWatcherRetry()
      return null
    }
  }

  private handleWatcherError(): void {
    this.clearWatchers()
    this.scheduleWatcherRetry()
  }

  private scheduleWatcherRetry(): void {
    if (this.disposed || this.retryTimer) return
    this.retryTimer = setTimeout(() => {
      this.retryTimer = null
      this.setupWatchers()
    }, WATCH_RETRY_MS)
  }

  private clearWatchers(): void {
    if (this.headWatcher) {
      this.headWatcher.close()
      this.headWatcher = null
    }
    if (this.reftableWatcher) {
      this.reftableWatcher.close()
      this.reftableWatcher = null
    }
  }

  private clearTimers(): void {
    if (this.refreshTimer) {
      clearTimeout(this.refreshTimer)
      this.refreshTimer = null
    }
    if (this.retryTimer) {
      clearTimeout(this.retryTimer)
      this.retryTimer = null
    }
    this.refreshPending = false
  }
}

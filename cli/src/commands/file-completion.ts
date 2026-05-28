/**
 * @file completion — async file path completion using fd.
 * Triggered when user types `@` in the input.
 */

import { spawn } from 'child_process'
import { readdirSync, statSync } from 'fs'
import { join, basename, dirname } from 'path'

export interface FileCompletionItem {
  /** Display label (relative path, directories end with /) */
  label: string
  /** Value to insert (includes @ prefix) */
  value: string
  /** Whether this is a directory */
  isDirectory: boolean
}

export interface FileCompletionResult {
  items: FileCompletionItem[]
  /** The @ prefix being completed (e.g. "@src/") */
  prefix: string
  /** Start index of the prefix in the line */
  prefixStart: number
}

/**
 * Extract the @-prefix from text before cursor.
 * Returns null if no @ completion should trigger.
 */
export function extractAtPrefix(textBeforeCursor: string): { prefix: string; start: number } | null {
  // Walk backwards to find the @ that starts this token
  for (let i = textBeforeCursor.length - 1; i >= 0; i--) {
    const ch = textBeforeCursor[i]!
    if (ch === '@') {
      // Must be at start of line or preceded by whitespace
      if (i === 0 || /\s/.test(textBeforeCursor[i - 1]!)) {
        return { prefix: textBeforeCursor.slice(i), start: i }
      }
      return null
    }
    // Stop at whitespace (no @ found in this token)
    if (/\s/.test(ch)) return null
  }
  return null
}

/**
 * Find fd binary path.
 */
function findFd(): string | null {
  // Common locations
  const candidates = ['/opt/homebrew/bin/fd', '/usr/local/bin/fd', '/usr/bin/fd', '/usr/bin/fdfind']
  for (const p of candidates) {
    try {
      statSync(p)
      return p
    } catch { /* not found */ }
  }
  return null
}

let fdPath: string | null | undefined

function getFdPath(): string | null {
  if (fdPath === undefined) {
    fdPath = findFd()
  }
  return fdPath
}

/**
 * Async file completion using fd.
 * Falls back to readdir if fd is not available.
 */
export async function completeAtFile(
  textBeforeCursor: string,
  cwd: string,
  signal?: AbortSignal,
): Promise<FileCompletionResult | null> {
  const extracted = extractAtPrefix(textBeforeCursor)
  if (!extracted) return null

  const { prefix, start } = extracted
  const query = prefix.slice(1) // strip leading @

  // Parse query into searchDir and filePrefix
  // @cli/src/m → searchDir="cli/src", filePrefix="m"
  // @cli/ → searchDir="cli", filePrefix=""
  // @c → searchDir="", filePrefix="c"
  let searchDir = ''
  let filePrefix = query
  if (query.includes('/')) {
    const lastSlash = query.lastIndexOf('/')
    searchDir = query.slice(0, lastSlash)
    filePrefix = query.slice(lastSlash + 1)
  }

  const fd = getFdPath()
  let items: FileCompletionItem[]

  if (fd) {
    items = await searchDirWithFd(fd, searchDir, filePrefix, cwd, signal)
  } else {
    items = searchWithReaddir(query, cwd)
  }

  if (items.length === 0) return null

  return { items, prefix, prefixStart: start }
}

/**
 * Search a specific directory with fd, depth 1, then filter by prefix.
 * This gives Pi-like behavior: only direct children of the target dir.
 */
async function searchDirWithFd(
  fdPath: string,
  searchDir: string,
  filePrefix: string,
  cwd: string,
  signal?: AbortSignal,
): Promise<FileCompletionItem[]> {
  const maxResults = 50
  const searchBase = searchDir ? join(cwd, searchDir) : cwd
  const args = [
    '--base-directory', searchBase,
    '--max-depth', '1',
    '--max-results', String(maxResults),
    '--type', 'f',
    '--type', 'd',
    '--follow',
    '--exclude', '.git',
  ]

  return new Promise((resolve) => {
    if (signal?.aborted) {
      resolve([])
      return
    }

    const child = spawn(fdPath, args, {
      stdio: ['ignore', 'pipe', 'pipe'],
    })

    let stdout = ''
    let resolved = false

    const finish = (results: FileCompletionItem[]) => {
      if (resolved) return
      resolved = true
      if (onAbort) signal?.removeEventListener('abort', onAbort)
      resolve(results)
    }

    const onAbort = () => {
      if (child.exitCode === null) {
        child.kill('SIGKILL')
      }
      finish([])
    }

    if (signal) {
      signal.addEventListener('abort', onAbort, { once: true })
    }

    child.stdout.setEncoding('utf-8')
    child.stdout.on('data', (chunk: string) => {
      stdout += chunk
    })
    child.on('error', () => finish([]))
    child.on('close', (code) => {
      if (signal?.aborted || code !== 0 || !stdout) {
        finish([])
        return
      }

      const lines = stdout.trim().split('\n').filter(Boolean)
      const dirPrefix = searchDir ? searchDir + '/' : ''
      const results: FileCompletionItem[] = []

      for (const line of lines) {
        const isDir = line.endsWith('/')
        const name = isDir ? line.slice(0, -1) : line
        if (name === '.git' || name.startsWith('.git/')) continue
        // Filter by prefix (case-insensitive)
        if (filePrefix && !name.toLowerCase().startsWith(filePrefix.toLowerCase())) continue
        const label = dirPrefix + line
        results.push({
          label,
          value: `@${label}`,
          isDirectory: isDir,
        })
      }

      finish(results.slice(0, 20))
    })
  })
}

/**
 * Fallback: search using readdir (no fd available or empty query).
 */
function searchWithReaddir(query: string, cwd: string): FileCompletionItem[] {
  let dir: string
  let prefix: string

  if (query.includes('/')) {
    const lastSlash = query.lastIndexOf('/')
    dir = join(cwd, query.slice(0, lastSlash + 1))
    prefix = query.slice(lastSlash + 1)
  } else {
    dir = cwd
    prefix = query
  }

  let entries: string[]
  try {
    entries = readdirSync(dir)
  } catch {
    return []
  }

  const filtered = prefix
    ? entries.filter(e => e.toLowerCase().startsWith(prefix.toLowerCase()))
    : entries.filter(e => !e.startsWith('.'))

  const dirPrefix = query.includes('/') ? query.slice(0, query.lastIndexOf('/') + 1) : ''

  return filtered.slice(0, 20).map(entry => {
    const fullPath = join(dir, entry)
    let isDir = false
    try { isDir = statSync(fullPath).isDirectory() } catch { /* ignore */ }
    const label = dirPrefix + entry + (isDir ? '/' : '')
    return {
      label,
      value: `@${label}`,
      isDirectory: isDir,
    }
  })
}

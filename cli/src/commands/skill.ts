/**
 * /skill command — install, list, remove skills.
 * Skills live in ~/.evotai/skills/<name>/ with a SKILL.md file.
 */

import { join } from 'path'
import { homedir } from 'os'
import {
  readdirSync, existsSync, rmSync, statSync,
} from 'fs'
import { mkdtemp, cp, mkdir, rename, rm } from 'fs/promises'

// Install target: the global ~/.evotai/skills dir. `/skill install` and
// `/skill remove` always operate here; EVOT_SKILLS_DIRS entries are
// read-only externally-managed dirs (e.g. a git checkout the user upgrades
// themselves), so we never install into or delete from them.
const SKILLS_DIR = join(homedir(), '.evotai', 'skills')

/**
 * Expand a leading `~` to the user's home directory. Mirrors the Rust
 * `paths::expand_home_path` used when the engine parses EVOT_SKILLS_DIRS.
 */
function expandHome(dir: string): string {
  if (dir === '~') return homedir()
  if (dir.startsWith('~/')) return join(homedir(), dir.slice(2))
  return dir
}

/**
 * Resolve the ordered list of skill directories to scan, matching the engine's
 * precedence (see gateway/service.rs + conf/load.rs):
 *   1. managed builtin ~/.evotai/builtin-skills
 *   2. global ~/.evotai/skills
 *   3. EVOT_SKILLS_DIRS entries (colon-separated, `~` expanded)
 *   4. ~/.claude/skills
 *
 * This reads only `process.env`, so it MISSES EVOT_SKILLS_DIRS set in
 * ~/.evotai/evot.env (or a custom --env-file / TOML config). Prefer passing the
 * agent's resolved `skillsDirs()` to skillList()/getSkillNames() when a live
 * agent is available (see issue #38); this remains the fallback for contexts
 * without one.
 */
export function resolveSkillsDirs(env: NodeJS.ProcessEnv = process.env): string[] {
  const dirs = [
    join(homedir(), '.evotai', 'builtin-skills'),
    join(homedir(), '.evotai', 'skills'),
  ]
  const extra = env.EVOT_SKILLS_DIRS
  if (extra) {
    for (const part of extra.split(':')) {
      const trimmed = part.trim()
      if (trimmed) dirs.push(expandHome(trimmed))
    }
  }
  dirs.push(join(homedir(), '.claude', 'skills'))
  // De-dup while preserving order (a user may repeat the global dir).
  return [...new Set(dirs)]
}

// ---------------------------------------------------------------------------
// /skill list
// ---------------------------------------------------------------------------

export function skillListFromDirs(dirs: string[]): string {
  const entries = dirs.flatMap((dir) => {
    if (!existsSync(dir)) return []
    return readdirSync(dir)
      .filter((name) => existsSync(join(dir, name, 'SKILL.md')))
      .map((name) => ({ name, dir: join(dir, name) }))
  }).sort((a, b) => a.name.localeCompare(b.name))

  if (entries.length === 0) return '  no skills installed'

  return `\n  Skills:\n${entries
    .map(({ name, dir }) => `  • [${name}] ${dir}`)
    .join('\n')}`
}

export function skillList(dirs?: string[]): string {
  return skillListFromDirs(dirs ?? resolveSkillsDirs())
}

// ---------------------------------------------------------------------------
// /skill install <source>
// ---------------------------------------------------------------------------

export interface GitHubSource {
  repo: string
  gitRef?: string
  subpath?: string
}

export function parseGitHubSource(input: string): GitHubSource {
  const trimmed = input.trim()

  // Full URL: https://github.com/owner/repo/tree/ref/path
  const urlMatch = trimmed.match(
    /^https?:\/\/github\.com\/([^/]+\/[^/]+)(?:\/tree\/([^/]+)(?:\/(.+))?)?$/
  )
  if (urlMatch) {
    return {
      repo: urlMatch[1]!,
      gitRef: urlMatch[2],
      subpath: urlMatch[3],
    }
  }

  // Short form: owner/repo
  if (/^[a-zA-Z0-9_.-]+\/[a-zA-Z0-9_.-]+$/.test(trimmed)) {
    return { repo: trimmed }
  }

  throw new Error(`Invalid source: ${trimmed}. Use owner/repo or a GitHub URL.`)
}

export function isValidSkillName(name: string): boolean {
  return /^[a-zA-Z0-9._-]+$/.test(name) && name.length <= 64
}

export type ProgressFn = (msg: string, level: 'info' | 'warn' | 'error') => void

async function githubToken(): Promise<string> {
  try {
    const proc = Bun.spawn(['gh', 'auth', 'token'], { stdout: 'pipe', stderr: 'ignore' })
    const [exitCode, stdout] = await Promise.all([proc.exited, new Response(proc.stdout).text()])
    return exitCode === 0 ? stdout.trim() : ''
  } catch {
    return ''
  }
}

async function runCommand(command: string[], action: string): Promise<void> {
  const proc = Bun.spawn(command, { stdout: 'ignore', stderr: 'pipe' })
  const [exitCode, stderr] = await Promise.all([proc.exited, new Response(proc.stderr).text()])
  if (exitCode !== 0) {
    const detail = stderr.trim()
    throw new Error(`${action} failed${detail ? `: ${detail}` : ''}`)
  }
}

export async function skillInstall(
  source: string,
  progress?: ProgressFn,
): Promise<string> {
  const parsed = parseGitHubSource(source)
  const repoName = parsed.repo.split('/')[1] ?? parsed.repo

  // Clone to temp dir
  const { tmpdir } = await import('os')
  const tempDir = await mkdtemp(join(tmpdir(), 'evot-skill-'))

  try {
    // Download repo tarball via GitHub API (avoids git-remote-https issues)
    const gitRef = parsed.gitRef ?? 'main'
    const tarFile = join(tempDir, 'repo.tar.gz')
    progress?.(`downloading ${parsed.repo}@${gitRef}...`, 'info')
    const token = await githubToken()
    const headers: string[] = token
      ? ['-H', `Authorization: token ${token}`, '-H', 'Accept: application/vnd.github+json']
      : ['-H', 'Accept: application/vnd.github+json']
    await runCommand(
      ['curl', '-fsSL', ...headers, '-o', tarFile, `https://api.github.com/repos/${parsed.repo}/tarball/${gitRef}`],
      'download repo',
    )

    progress?.('extracting archive...', 'info')
    await runCommand(
      ['tar', 'xzf', tarFile, '--strip-components=1', '-C', tempDir],
      'extract tarball',
    )

    // Determine source dir
    let srcDir = tempDir
    if (parsed.subpath) {
      srcDir = join(tempDir, parsed.subpath)
      if (!existsSync(srcDir)) {
        throw new Error(`Subpath not found: ${parsed.subpath}`)
      }
    }

    progress?.('installing skills...', 'info')
    const installed: string[] = []

    // Check if srcDir itself is a skill (has SKILL.md)
    if (existsSync(join(srcDir, 'SKILL.md'))) {
      const name = parsed.subpath?.split('/').pop() ?? repoName
      await installSkillDir(srcDir, name)
      installed.push(name)
    } else {
      // Multi-skill repo: scan top-level subdirs
      const subdirs = readdirSync(srcDir).filter((d) => {
        const p = join(srcDir, d)
        return statSync(p).isDirectory() && existsSync(join(p, 'SKILL.md'))
      })
      if (subdirs.length === 0) {
        throw new Error('No SKILL.md found in repo or subdirectories.')
      }
      for (const d of subdirs) {
        await installSkillDir(join(srcDir, d), d)
        installed.push(d)
      }
    }

    return installed.length === 1
      ? `✓ installed skill: ${installed[0]}`
      : `✓ installed ${installed.length} skills: ${installed.join(', ')}`
  } finally {
    await rm(tempDir, { recursive: true, force: true })
  }
}

async function installSkillDir(srcDir: string, name: string): Promise<void> {
  if (!isValidSkillName(name)) {
    throw new Error(`Invalid skill name: ${name}`)
  }
  const suffix = `${process.pid}-${Date.now()}`
  const stageDir = join(SKILLS_DIR, `.${name}.install-${suffix}`)
  const backupDir = join(SKILLS_DIR, `.${name}.backup-${suffix}`)
  const destDir = join(SKILLS_DIR, name)
  await mkdir(SKILLS_DIR, { recursive: true })
  await rm(stageDir, { recursive: true, force: true })
  await mkdir(stageDir, { recursive: true })

  let backedUp = false
  try {
    for (const entry of readdirSync(srcDir)) {
      if (entry === '.git') continue
      await cp(join(srcDir, entry), join(stageDir, entry), { recursive: true })
    }
    if (existsSync(destDir)) {
      await rename(destDir, backupDir)
      backedUp = true
    }
    try {
      await rename(stageDir, destDir)
    } catch (error) {
      if (backedUp) await rename(backupDir, destDir)
      throw error
    }
    if (backedUp) await rm(backupDir, { recursive: true, force: true })
  } finally {
    await rm(stageDir, { recursive: true, force: true })
  }
}

// ---------------------------------------------------------------------------
// /skill remove
// ---------------------------------------------------------------------------

export function skillRemove(name: string): string {
  if (!isValidSkillName(name)) {
    return `  invalid skill name: ${name}`
  }
  const skillDir = join(SKILLS_DIR, name)
  if (!existsSync(skillDir)) {
    return `  skill not found: ${name}`
  }
  rmSync(skillDir, { recursive: true, force: true })
  return `  ✓ removed skill: ${name}`
}

/**
 * Discover loaded resources for banner display:
 * - User-installed skills
 * - Project context files (EVOT.md, CLAUDE.md, AGENTS.md)
 */

import { join } from 'path'
import { existsSync, readdirSync } from 'fs'
import { resolveSkillsDirs } from '../commands/skill.js'

const PROJECT_CONTEXT_FILES = ['EVOT.md', 'CLAUDE.md', 'AGENTS.md']

/**
 * Return sorted list of user-installed skill names.
 * Lightweight — only reads directory names, not SKILL.md content.
 * Scans the same dirs the agent loads (global + EVOT_SKILLS_DIRS + claude).
 *
 * Pass the agent's resolved `skillsDirs()` so the banner reflects
 * EVOT_SKILLS_DIRS from ~/.evotai/evot.env too (issue #38). Falls back to
 * resolveSkillsDirs() (process.env only) when no dirs are provided.
 */
export function getSkillNames(dirs?: string[]): string[] {
  const names = new Set<string>()

  for (const dir of dirs ?? resolveSkillsDirs()) {
    if (!existsSync(dir)) continue
    try {
      const entries = readdirSync(dir)
      for (const name of entries) {
        if (existsSync(join(dir, name, 'SKILL.md'))) {
          names.add(name)
        }
      }
    } catch { /* skip unreadable dirs */ }
  }

  return [...names].sort()
}

/**
 * Return list of project context files that exist in the given cwd.
 */
export function getContextFiles(cwd: string): string[] {
  return PROJECT_CONTEXT_FILES.filter(name => existsSync(join(cwd, name)))
}

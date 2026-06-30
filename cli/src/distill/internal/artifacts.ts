/** Transient workspace artifacts — the single source of truth.
 *
 * These are generated or installed products (dependency trees, bytecode caches,
 * VCS metadata), never authored task content. One definition is shared by:
 *  - the git baseline (written to .git/info/exclude) so a workspace's diff,
 *    status, reset, and clean never see them — the same way a real project's
 *    own .gitignore keeps build output out of `git status`;
 *  - the bundle copy filter, so they're excluded from the shipped dataset.
 *
 * Keeping one list means "what counts as a build product" can't drift between
 * "what git ignores" and "what we ship".
 */

/** Directory names that are transient anywhere in the tree. */
export const TRANSIENT_DIRS = ['.git', 'node_modules', '.venv', '__pycache__', '.pytest_cache']

/** File suffixes that are transient anywhere in the tree. */
export const TRANSIENT_FILE_SUFFIXES = ['.pyc']

/** True if an absolute or relative path points at a transient artifact. */
export function isTransientPath(p: string): boolean {
  if (TRANSIENT_FILE_SUFFIXES.some((s) => p.endsWith(s))) return true
  return TRANSIENT_DIRS.some((d) => p === d || p.includes(`/${d}/`) || p.endsWith(`/${d}`))
}

/** Lines for a git exclude/ignore file that hide every transient artifact. */
export function gitignoreLines(): string[] {
  return [
    ...TRANSIENT_DIRS.map((d) => `${d}/`),
    ...TRANSIENT_FILE_SUFFIXES.map((s) => `*${s}`),
  ]
}

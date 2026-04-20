/**
 * GitHub issue reference auto-linking.
 */

import chalk from 'chalk'

export function linkifyIssueRefs(input: string): string {
  let out = ''
  let cursor = 0
  const hashRe = /#(\d+)/g
  let m

  while ((m = hashRe.exec(input)) !== null) {
    const hashPos = m.index
    const num = m[1]!
    const before = input.slice(cursor, hashPos)
    const slashIdx = before.lastIndexOf('/')
    if (slashIdx === -1) continue
    const absSlash = cursor + slashIdx

    const name = input.slice(absSlash + 1, hashPos)
    if (!name || !/^[a-zA-Z0-9_\-.]+$/.test(name)) continue

    let ownerStart = absSlash - 1
    while (ownerStart >= cursor && /[a-zA-Z0-9_-]/.test(input[ownerStart]!)) ownerStart--
    ownerStart++
    const owner = input.slice(ownerStart, absSlash)
    if (!owner || !/^[a-zA-Z0-9_-]+$/.test(owner)) continue

    // Boundary check: char before owner must not be alphanumeric/repo-like
    if (ownerStart > 0 && /[a-zA-Z0-9_.\/-]/.test(input[ownerStart - 1]!)) continue

    out += input.slice(cursor, ownerStart)
    out += chalk.cyan(`${owner}/${name}#${num}`)
    cursor = hashPos + m[0].length
  }

  out += input.slice(cursor)
  return out
}

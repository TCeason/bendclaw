import { appendFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs'
import { homedir } from 'os'
import { join } from 'path'

const DESCRIPTION_LIMIT = 80
const SLUG_WORD_LIMIT = 6
const SLUG_LENGTH_LIMIT = 48

export interface ClipMeta {
  sessionId?: string
  cwd?: string
}

export interface ClipResult {
  path: string
  slug: string
}

/** Save the latest assistant markdown verbatim into the memory vault. */
export function clipMarkdown(
  rawMarkdown: string,
  meta: ClipMeta,
  vaultDir = join(homedir(), '.evotai', 'memory'),
  now = new Date(),
): ClipResult {
  if (!rawMarkdown.trim()) {
    throw new Error('Cannot clip an empty assistant message')
  }

  const description = deriveDescription(rawMarkdown)
  const timestamp = formatTime(now)
  const slug = slugify(description) || timestamp
  const date = formatDate(now)
  const clipsDir = join(vaultDir, 'clips')
  mkdirSync(clipsDir, { recursive: true })

  const filename = resolveFilename(clipsDir, `${date}-${slug}`)
  const path = join(clipsDir, filename)
  const frontmatter = [
    '---',
    `name: ${yamlString(slug)}`,
    `description: ${yamlString(description)}`,
    'type: clip',
    `date: ${date}`,
    ...(meta.sessionId ? [`session: ${yamlString(meta.sessionId.slice(0, 8))}`] : []),
    ...(meta.cwd ? [`cwd: ${yamlString(meta.cwd)}`] : []),
    '---',
    '',
    '',
  ].join('\n')

  writeFileSync(path, `${frontmatter}${rawMarkdown}`, { flag: 'wx' })

  try {
    updateIndex(vaultDir, filename, description)
  } catch (error) {
    // Leave the clip readable even if an existing index is temporarily
    // unwritable; recall can rebuild MEMORY.md from files on disk.
    throw new Error(`Clip was saved to ${path}, but MEMORY.md could not be updated: ${errorMessage(error)}`)
  }

  return { path, slug }
}

function deriveDescription(rawMarkdown: string): string {
  const lines = rawMarkdown.split(/\r?\n/)
  let heading: string | undefined
  let firstText: string | undefined
  let insideFence = false
  for (const line of lines) {
    const trimmed = line.trim()
    if (/^(```|~~~)/.test(trimmed)) {
      insideFence = !insideFence
      continue
    }
    if (insideFence || !trimmed) continue
    if (!heading && /^\s{0,3}#{1,6}\s+\S/.test(line)) {
      heading = line
      break
    }
    firstText ??= line
  }
  const candidate = heading ?? firstText ?? 'Clipped assistant reply'

  const plain = candidate
    .trim()
    .replace(/^#{1,6}\s+/, '')
    .replace(/^>\s?/, '')
    .replace(/^[-*+]\s+/, '')
    .replace(/!\[([^\]]*)\]\([^)]+\)/g, '$1')
    .replace(/\[([^\]]+)\]\([^)]+\)/g, '$1')
    .replace(/[*_~`]+/g, '')
    .replace(/\s+/g, ' ')
    .trim()

  return truncateCharacters(plain || 'Clipped assistant reply', DESCRIPTION_LIMIT)
}

function slugify(value: string): string {
  const words = value
    .normalize('NFKD')
    .replace(/[\u0300-\u036f]/g, '')
    .toLowerCase()
    .match(/[a-z0-9]+/g)
    ?.slice(0, SLUG_WORD_LIMIT) ?? []

  return words.join('-').slice(0, SLUG_LENGTH_LIMIT).replace(/-+$/g, '')
}

function resolveFilename(directory: string, base: string): string {
  let suffix = 1
  while (true) {
    const filename = suffix === 1 ? `${base}.md` : `${base}-${suffix}.md`
    if (!existsSync(join(directory, filename))) return filename
    suffix += 1
  }
}

function updateIndex(vaultDir: string, filename: string, description: string): void {
  mkdirSync(vaultDir, { recursive: true })
  const indexPath = join(vaultDir, 'MEMORY.md')
  const entry = `- [clips/${filename}](clips/${filename}) — ${description}\n`

  if (!existsSync(indexPath)) {
    writeFileSync(indexPath, `# Memory\n\n${entry}`)
    return
  }

  const current = readFileSync(indexPath, 'utf8')
  const separator = current.length > 0 && !current.endsWith('\n') ? '\n' : ''
  appendFileSync(indexPath, `${separator}${entry}`)
}

function formatDate(date: Date): string {
  return [date.getFullYear(), pad(date.getMonth() + 1), pad(date.getDate())].join('-')
}

function formatTime(date: Date): string {
  return `${pad(date.getHours())}${pad(date.getMinutes())}${pad(date.getSeconds())}`
}

function pad(value: number): string {
  return String(value).padStart(2, '0')
}

function truncateCharacters(value: string, limit: number): string {
  return Array.from(value).slice(0, limit).join('')
}

function yamlString(value: string): string {
  return JSON.stringify(value)
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export const _testing = { deriveDescription, slugify, resolveFilename }

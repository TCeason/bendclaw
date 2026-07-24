/**
 * share — pack, encrypt, upload / download, decrypt, and import sessions.
 *
 * Encryption format:
 *   EVOTLOG1 (8 B magic) | salt (16 B) | IV (12 B) | authTag (16 B) | AES-256-GCM ciphertext
 *   Key derived from short password via PBKDF2 (100k iterations, SHA-256).
 *
 * Upload target: tmpfiles.org (free, no auth required).
 */

import { execFileSync } from 'child_process'
import { createCipheriv, createDecipheriv, randomBytes, pbkdf2Sync } from 'crypto'
import { existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, readdirSync, renameSync, rmSync } from 'fs'
import https from 'https'
import http from 'http'
import { homedir, tmpdir } from 'os'
import { join } from 'path'

const MAGIC = 'EVOTLOG1'
const EVOTAI_DIR = join(homedir(), '.evotai')
const PBKDF2_ITERATIONS = 100_000
const PASSWORD_LENGTH = 8
const PASSWORD_CHARS = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789' // no ambiguous chars

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

export interface ShareResult {
  url: string
}

export interface ImportResult {
  sessionId: string
}

export function isSharedSessionUrl(value: string): boolean {
  try {
    const url = new URL(value)
    return url.protocol === 'https:' || url.protocol === 'http:'
  } catch {
    return false
  }
}

/**
 * Pack, encrypt, and upload a session's logs.
 */
export async function shareSession(sessionId: string): Promise<ShareResult> {
  const files = collectFiles(sessionId)
  if (files.length === 0) {
    throw new Error(`No files found for session ${sessionId}`)
  }

  const tmpDir = mkdtempSync(join(tmpdir(), 'evot-share-'))
  const tarPath = join(tmpDir, 'session.tar.gz')
  const encrypted = (() => {
    try {
      try {
        execFileSync('tar', ['czf', tarPath, ...files], { cwd: EVOTAI_DIR })
      } catch (err: any) {
        throw new Error(`tar failed: ${err?.message ?? err}`)
      }
      return encrypt(readFileSync(tarPath))
    } finally {
      rmSync(tmpDir, { recursive: true, force: true })
    }
  })()

  const rawUrl = await upload(encrypted.payload)
  return { url: `${rawUrl}#${encrypted.password}` }
}

/**
 * Download, decrypt, and import a shared session.
 */
export async function importSharedSession(urlWithKey: string): Promise<ImportResult> {
  const hashIdx = urlWithKey.lastIndexOf('#')
  if (hashIdx < 0) {
    throw new Error('URL must contain a #password fragment')
  }
  const baseUrl = urlWithKey.slice(0, hashIdx)
  const password = urlWithKey.slice(hashIdx + 1)
  if (!password) {
    throw new Error('Password is empty')
  }

  const payload = await download(toDownloadUrl(baseUrl))
  let decrypted: Buffer
  try {
    decrypted = decrypt(payload, password)
  } catch {
    throw new Error('Decryption failed — wrong password or corrupted file')
  }

  const tmpDir = mkdtempSync(join(tmpdir(), 'evot-import-'))
  const tarPath = join(tmpDir, 'export.tar.gz')
  try {
    writeFileSync(tarPath, decrypted)
    validateArchive(tarPath)
    try {
      execFileSync('tar', ['xzf', tarPath], { cwd: tmpDir })
    } catch (err: any) {
      throw new Error(`tar extract failed: ${err?.message ?? err}`)
    }
    rmSync(tarPath, { force: true })
    return { sessionId: validateAndImport(tmpDir) }
  } finally {
    rmSync(tmpDir, { recursive: true, force: true })
  }
}

// ---------------------------------------------------------------------------
// Internals
// ---------------------------------------------------------------------------

function collectFiles(sessionId: string): string[] {
  const files: string[] = []
  const sessionDir = join('sessions', sessionId)
  const candidates = [
    join(sessionDir, 'session.json'),
    join(sessionDir, 'transcript.jsonl'),
    join('logs', `${sessionId}.log`),
    join('logs', `${sessionId}.screen.log`),
    join('logs', `${sessionId}.markdown.log`),
  ]
  for (const f of candidates) {
    if (existsSync(join(EVOTAI_DIR, f))) {
      files.push(f)
    }
  }
  return files
}

const UUID_PATTERN = '[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}'
// Strict lowercase: must stay in sync with the session-id extraction regex in
// validateAndImport, or mixed-case members could pass the whitelist yet fail
// (or skew) session-id attribution.
const ALLOWED_FILE_PATTERN = new RegExp(
  `^(sessions/${UUID_PATTERN}/(session\\.json|transcript\\.jsonl)|logs/${UUID_PATTERN}\\.(log|screen\\.log|markdown\\.log))$`,
)
const ALLOWED_DIRECTORY_PATTERN = new RegExp(`^(sessions|logs|sessions/${UUID_PATTERN})/$`)

/** Validate member paths and reject links before extraction. */
function validateArchive(tarPath: string): void {
  let members: string[]
  let verbose: string[]
  try {
    members = execFileSync('tar', ['tzf', tarPath], { encoding: 'utf8' }).split(/\r?\n/).filter(Boolean)
    verbose = execFileSync('tar', ['tvzf', tarPath], { encoding: 'utf8' }).split(/\r?\n/).filter(Boolean)
  } catch (err: any) {
    throw new Error(`tar validation failed: ${err?.message ?? err}`)
  }

  if (members.length === 0) throw new Error('Archive is empty')
  for (const raw of members) {
    const member = raw.replace(/^\.\//, '')
    if (
      member.startsWith('/')
      || member.split('/').includes('..')
      || (!ALLOWED_FILE_PATTERN.test(member) && !ALLOWED_DIRECTORY_PATTERN.test(member))
    ) {
      throw new Error(`Rejected unsafe archive path: ${raw}`)
    }
  }
  if (verbose.some(line => !/^[-d]/.test(line))) {
    throw new Error('Archive contains unsupported entry types')
  }
}

/** Validate extracted files and move them into the target dir (default ~/.evotai) */
function validateAndImport(tmpDir: string, targetRoot?: string): string {
  const destRoot = targetRoot ?? EVOTAI_DIR

  // Enumerate all files
  const allFiles = listFilesRecursive(tmpDir)
  let sessionId: string | null = null

  for (const rel of allFiles) {
    // Security: reject path traversal, absolute paths, symlinks
    if (rel.includes('..') || rel.startsWith('/')) {
      throw new Error(`Rejected unsafe path: ${rel}`)
    }
    const fullPath = join(tmpDir, rel)
    const stat = lstatSync(fullPath)
    if (stat.isSymbolicLink()) {
      throw new Error(`Rejected symbolic link: ${rel}`)
    }
    if (!ALLOWED_FILE_PATTERN.test(rel)) {
      throw new Error(`Unexpected file in archive: ${rel}`)
    }

    // Extract session id
    const match = rel.match(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/)
    if (match) {
      if (sessionId && sessionId !== match[0]) {
        throw new Error('Archive contains files from multiple sessions')
      }
      sessionId = match[0]
    }
  }

  if (!sessionId) {
    throw new Error('Could not determine session id from archive')
  }

  // Move files into place
  const targetSessionDir = join(destRoot, 'sessions', sessionId)
  const targetLogsDir = join(destRoot, 'logs')
  mkdirSync(targetSessionDir, { recursive: true })
  mkdirSync(targetLogsDir, { recursive: true })

  for (const rel of allFiles) {
    const src = join(tmpDir, rel)
    const dst = join(destRoot, rel)
    mkdirSync(join(dst, '..'), { recursive: true })
    renameSync(src, dst)
  }

  return sessionId
}

function listFilesRecursive(dir: string, prefix = ''): string[] {
  const results: string[] = []
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const rel = prefix ? `${prefix}/${entry.name}` : entry.name
    if (entry.isDirectory()) {
      results.push(...listFilesRecursive(join(dir, entry.name), rel))
    } else {
      results.push(rel)
    }
  }
  return results
}

/** Convert tmpfiles.org URL to its download variant (insert /dl/) */
function toDownloadUrl(url: string): string {
  // https://tmpfiles.org/<id>/file.bin → https://tmpfiles.org/dl/<id>/file.bin
  // The id was once purely numeric but tmpfiles.org now issues alphanumeric
  // ids (e.g. /wywHXeSghysu/). A numeric-only match left the URL unchanged, so
  // /share would otherwise fetch the HTML viewer page instead of the raw
  // payload and fail decryption. Match any non-empty id segment, but don't double-insert
  // /dl/ if it is already present.
  const m = url.match(/^(https?:\/\/tmpfiles\.org)\/([^/]+\/.+)$/)
  if (m && !m[2]!.startsWith('dl/')) {
    return `${m[1]}/dl/${m[2]}`
  }
  return url
}

/** Upload a buffer to tmpfiles.org, return the raw URL. */
function upload(data: Buffer): Promise<string> {
  return new Promise((resolve, reject) => {
    const boundary = `----evot${Date.now()}`
    const header = `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="evot-log.bin"\r\nContent-Type: application/octet-stream\r\n\r\n`
    const footer = `\r\n--${boundary}--\r\n`
    const body = Buffer.concat([Buffer.from(header), data, Buffer.from(footer)])

    const req = https.request(
      {
        hostname: 'tmpfiles.org',
        path: '/api/v1/upload',
        method: 'POST',
        headers: {
          'Content-Type': `multipart/form-data; boundary=${boundary}`,
          'Content-Length': body.length,
        },
      },
      (res) => {
        let raw = ''
        res.on('data', (chunk: Buffer) => (raw += chunk))
        res.on('end', () => {
          if (res.statusCode !== 200) {
            reject(new Error(`Upload failed (HTTP ${res.statusCode}): ${raw.slice(0, 200)}`))
            return
          }
          try {
            const json = JSON.parse(raw)
            if (json.status === 'success' && json.data?.url) {
              // tmpfiles.org returns http://, normalize to https://
              const url = (json.data.url as string).replace(/^http:\/\//, 'https://')
              resolve(url)
            } else {
              reject(new Error(`Unexpected response: ${raw.slice(0, 200)}`))
            }
          } catch {
            reject(new Error(`Failed to parse response: ${raw.slice(0, 200)}`))
          }
        })
      },
    )
    req.on('error', reject)
    req.write(body)
    req.end()
  })
}

// ---------------------------------------------------------------------------
// Test helpers — exported for unit tests only
// ---------------------------------------------------------------------------

export const _testing = { isSharedSessionUrl, toDownloadUrl, encrypt, decrypt, validateArchive, validateAndImport, listFilesRecursive, generatePassword, collectFiles }

function generatePassword(): string {
  const bytes = randomBytes(PASSWORD_LENGTH)
  let result = ''
  for (let i = 0; i < PASSWORD_LENGTH; i++) {
    result += PASSWORD_CHARS[bytes[i]! % PASSWORD_CHARS.length]
  }
  return result
}

function deriveKey(password: string, salt: Buffer): Buffer {
  return pbkdf2Sync(password, salt, PBKDF2_ITERATIONS, 32, 'sha256')
}

/** Encrypt: EVOTLOG1 (8B) | salt (16B) | IV (12B) | authTag (16B) | ciphertext */
function encrypt(plaintext: Buffer): { payload: Buffer; password: string } {
  const password = generatePassword()
  const salt = randomBytes(16)
  const key = deriveKey(password, salt)
  const iv = randomBytes(12)
  const cipher = createCipheriv('aes-256-gcm', key, iv)
  const encrypted = Buffer.concat([cipher.update(plaintext), cipher.final()])
  const authTag = cipher.getAuthTag()
  const magicBuf = Buffer.from(MAGIC)
  const payload = Buffer.concat([magicBuf, salt, iv, authTag, encrypted])
  return { payload, password }
}

/** Decrypt: parse EVOTLOG1 (8B) | salt (16B) | IV (12B) | authTag (16B) | ciphertext */
function decrypt(payload: Buffer, password: string): Buffer {
  const minSize = 8 + 16 + 12 + 16 // magic + salt + iv + authTag
  if (payload.length < minSize) {
    throw new Error('File too small to be a valid export')
  }
  const magic = payload.subarray(0, 8).toString()
  if (magic !== MAGIC) {
    throw new Error('Invalid file format — not an evot log export')
  }
  const salt = payload.subarray(8, 24)
  const iv = payload.subarray(24, 36)
  const authTag = payload.subarray(36, 52)
  const ciphertext = payload.subarray(52)
  const key = deriveKey(password, salt)
  const decipher = createDecipheriv('aes-256-gcm', key, iv)
  decipher.setAuthTag(authTag)
  return Buffer.concat([decipher.update(ciphertext), decipher.final()])
}

/** Download a URL, following redirects. */
function download(url: string, redirects = 5): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    if (redirects <= 0) {
      reject(new Error('Too many redirects'))
      return
    }
    const proto = url.startsWith('https') ? https : http
    proto.get(url, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        resolve(download(res.headers.location, redirects - 1))
        return
      }
      if (res.statusCode !== 200) {
        let body = ''
        res.on('data', (chunk: Buffer) => (body += chunk))
        res.on('end', () => reject(new Error(`Download failed (HTTP ${res.statusCode}): ${body.slice(0, 200)}`)))
        return
      }
      const chunks: Buffer[] = []
      res.on('data', (chunk: Buffer) => chunks.push(chunk))
      res.on('end', () => resolve(Buffer.concat(chunks)))
    }).on('error', reject)
  })
}

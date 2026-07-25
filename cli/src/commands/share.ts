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
import { constants as fsConstants, copyFileSync, createReadStream, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, readdirSync, rmSync, statSync } from 'fs'
import https from 'https'
import http from 'http'
import { homedir, tmpdir } from 'os'
import { join } from 'path'
import { createGunzip } from 'zlib'

const MAGIC = 'EVOTLOG1'
const EVOTAI_DIR = join(homedir(), '.evotai')
const PBKDF2_ITERATIONS = 100_000
const PASSWORD_LENGTH = 8
const PASSWORD_CHARS = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789' // no ambiguous chars
const TMPFILES_ORIGIN = 'https://tmpfiles.org'
const MAX_SHARE_DOWNLOAD_BYTES = 100 * 1024 * 1024
const MAX_SHARE_ARCHIVE_BYTES = 512 * 1024 * 1024
const MAX_SERVICE_RESPONSE_BYTES = 1024 * 1024
const NETWORK_TIMEOUT_MS = 30_000

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
      const archiveBytes = statSync(tarPath).size
      const encryptionOverhead = 8 + 16 + 12 + 16
      if (archiveBytes + encryptionOverhead > MAX_SHARE_DOWNLOAD_BYTES) {
        throw new Error(`Share archive exceeds ${MAX_SHARE_DOWNLOAD_BYTES} byte limit`)
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

  const payload = await downloadSharedPayload(baseUrl)
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
    await validateArchiveSize(tarPath)
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

async function validateArchiveSize(
  tarPath: string,
  maxBytes = MAX_SHARE_ARCHIVE_BYTES,
): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    const source = createReadStream(tarPath)
    const gunzip = createGunzip()
    let total = 0
    let settled = false

    const fail = (err: Error) => {
      if (settled) return
      settled = true
      source.destroy()
      gunzip.destroy()
      reject(err)
    }
    source.on('error', fail)
    gunzip.on('error', fail)
    gunzip.on('data', (chunk: Buffer) => {
      total += chunk.length
      if (total > maxBytes) {
        fail(new Error(`Archive expands beyond ${maxBytes} byte limit`))
      }
    })
    gunzip.on('end', () => {
      if (settled) return
      settled = true
      resolve()
    })
    source.pipe(gunzip)
  })
}

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
  const allFiles = listFilesRecursive(tmpDir).sort()
  let sessionId: string | null = null
  let extractedBytes = 0

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

    extractedBytes += stat.size
    if (extractedBytes > MAX_SHARE_ARCHIVE_BYTES) {
      throw new Error(`Extracted files exceed ${MAX_SHARE_ARCHIVE_BYTES} byte limit`)
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

  const destinations = allFiles.map(rel => ({
    rel,
    src: join(tmpDir, rel),
    dst: join(destRoot, rel),
  }))
  const conflict = destinations.find(({ dst }) => existsSync(dst))
  if (conflict) {
    throw new Error(`Session import would overwrite existing file: ${conflict.rel}`)
  }

  const written: string[] = []
  try {
    for (const { src, dst } of destinations) {
      mkdirSync(join(dst, '..'), { recursive: true })
      copyFileSync(src, dst, fsConstants.COPYFILE_EXCL)
      written.push(dst)
    }
  } catch (err) {
    for (const path of written.reverse()) {
      rmSync(path, { force: true })
    }
    throw err
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

/** Download a shared payload, resolving tmpfiles.org's signed download URL. */
async function downloadSharedPayload(url: string): Promise<Buffer> {
  const parsed = new URL(url)
  if (parsed.hostname !== 'tmpfiles.org') {
    return download(url)
  }
  if (!['http:', 'https:'].includes(parsed.protocol) || parsed.username || parsed.password || parsed.port) {
    throw new Error('Invalid tmpfiles.org URL')
  }

  const viewerUrl = tmpfilesViewerUrl(parsed)
  const viewerPayload = await download(viewerUrl, 5, TMPFILES_ORIGIN)
  if (hasExportMagic(viewerPayload)) {
    return viewerPayload
  }

  const signedUrl = extractTmpfilesDownloadUrl(viewerUrl, viewerPayload.toString('utf8'))
  const payload = await download(signedUrl, 5, TMPFILES_ORIGIN)
  if (!hasExportMagic(payload)) {
    throw new Error('tmpfiles.org returned an unexpected file format')
  }
  return payload
}

function tmpfilesViewerUrl(url: URL): string {
  const parts = url.pathname.split('/').filter(Boolean)
  // Current signed URLs are /dl/<signature>/<id>/<file>. Legacy download URLs
  // are /dl/<id>/<file> and now redirect back to the viewer.
  const viewerParts = parts[0] === 'dl'
    ? (parts.length >= 4 ? parts.slice(2) : parts.slice(1))
    : parts
  const viewer = new URL(TMPFILES_ORIGIN)
  viewer.pathname = `/${viewerParts.join('/')}`
  return viewer.toString()
}

function extractTmpfilesDownloadUrl(viewerUrl: string, html: string): string {
  const viewer = new URL(viewerUrl)
  const expectedPath = viewer.pathname
  const hrefPattern = /href=["']([^"']+)["']/gi

  for (const match of html.matchAll(hrefPattern)) {
    const href = match[1]?.replaceAll('&amp;', '&')
    if (!href) continue

    let candidate: URL
    try {
      candidate = new URL(href, viewer)
    } catch {
      continue
    }
    const viewerParts = expectedPath.split('/').filter(Boolean)
    const candidateParts = candidate.pathname.split('/').filter(Boolean)
    if (
      candidate.origin === TMPFILES_ORIGIN
      && !candidate.username
      && !candidate.password
      && candidateParts.length === viewerParts.length + 2
      && candidateParts[0] === 'dl'
      && candidateParts[1]?.length > 0
      && candidateParts.slice(2).every((part, index) => part === viewerParts[index])
    ) {
      return candidate.toString()
    }
  }

  throw new Error('tmpfiles.org download link was not found or has expired')
}

function hasExportMagic(payload: Buffer): boolean {
  return payload.length >= MAGIC.length && payload.subarray(0, MAGIC.length).toString() === MAGIC
}

/** Upload a buffer to tmpfiles.org, return the viewer URL. */
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
        let responseBytes = 0
        res.on('data', (chunk: Buffer) => {
          responseBytes += chunk.length
          if (responseBytes > MAX_SERVICE_RESPONSE_BYTES) {
            res.destroy(new Error(`Upload response exceeds ${MAX_SERVICE_RESPONSE_BYTES} byte limit`))
            return
          }
          raw += chunk.toString('utf8')
        })
        res.on('error', reject)
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
    req.setTimeout(NETWORK_TIMEOUT_MS, () => {
      req.destroy(new Error(`Upload timed out after ${NETWORK_TIMEOUT_MS}ms`))
    })
    req.write(body)
    req.end()
  })
}

// ---------------------------------------------------------------------------
// Test helpers — exported for unit tests only
// ---------------------------------------------------------------------------

export const _testing = {
  isSharedSessionUrl,
  downloadSharedPayload,
  tmpfilesViewerUrl,
  extractTmpfilesDownloadUrl,
  resolveRedirectUrl,
  download,
  hasExportMagic,
  encrypt,
  decrypt,
  validateArchiveSize,
  validateArchive,
  validateAndImport,
  listFilesRecursive,
  generatePassword,
  collectFiles,
}

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

function resolveRedirectUrl(currentUrl: URL, location: string, requiredOrigin?: string): string {
  const redirect = new URL(location, currentUrl)
  if (requiredOrigin && redirect.origin !== requiredOrigin) {
    throw new Error(`Download redirect left trusted origin: ${requiredOrigin}`)
  }
  return redirect.toString()
}

/** Download a URL, following redirects within an optional trusted origin. */
function download(
  url: string,
  redirects = 5,
  requiredOrigin?: string,
  maxBytes = MAX_SHARE_DOWNLOAD_BYTES,
): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    if (redirects <= 0) {
      reject(new Error('Too many redirects'))
      return
    }

    const requestUrl = new URL(url)
    if (!['http:', 'https:'].includes(requestUrl.protocol)) {
      reject(new Error(`Unsupported download protocol: ${requestUrl.protocol}`))
      return
    }
    if (requiredOrigin && requestUrl.origin !== requiredOrigin) {
      reject(new Error(`Download redirect left trusted origin: ${requiredOrigin}`))
      return
    }

    const proto = requestUrl.protocol === 'https:' ? https : http
    const req = proto.get(requestUrl, (res) => {
      if (res.statusCode && res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        try {
          const redirectUrl = resolveRedirectUrl(requestUrl, res.headers.location, requiredOrigin)
          res.resume()
          resolve(download(redirectUrl, redirects - 1, requiredOrigin, maxBytes))
        } catch (err) {
          res.resume()
          reject(err)
        }
        return
      }
      if (res.statusCode !== 200) {
        let body = Buffer.alloc(0)
        const rejectResponse = () => {
          reject(new Error(`Download failed (HTTP ${res.statusCode}): ${body.toString('utf8')}`))
        }
        res.on('data', (chunk: Buffer) => {
          const remaining = 200 - body.length
          if (remaining > 0) body = Buffer.concat([body, chunk.subarray(0, remaining)])
          if (body.length >= 200) {
            res.destroy()
            rejectResponse()
          }
        })
        res.on('end', rejectResponse)
        res.on('error', reject)
        return
      }

      const contentLength = Number(res.headers['content-length'])
      if (Number.isFinite(contentLength) && contentLength > maxBytes) {
        res.resume()
        reject(new Error(`Download exceeds ${maxBytes} byte limit`))
        return
      }

      const chunks: Buffer[] = []
      let total = 0
      res.on('data', (chunk: Buffer) => {
        total += chunk.length
        if (total > maxBytes) {
          res.destroy(new Error(`Download exceeds ${maxBytes} byte limit`))
          return
        }
        chunks.push(chunk)
      })
      res.on('end', () => resolve(Buffer.concat(chunks)))
      res.on('error', reject)
    })
    req.on('error', reject)
    req.setTimeout(NETWORK_TIMEOUT_MS, () => {
      req.destroy(new Error(`Download timed out after ${NETWORK_TIMEOUT_MS}ms`))
    })
  })
}

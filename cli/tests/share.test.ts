import { describe, test, expect, beforeEach, afterEach } from 'bun:test'
import { execFileSync } from 'child_process'
import { mkdirSync, writeFileSync, existsSync, rmSync, symlinkSync } from 'fs'
import { join } from 'path'
import { tmpdir, homedir } from 'os'
import { _testing } from '../src/commands/share.js'

const { isSharedSessionUrl, toDownloadUrl, encrypt, decrypt, validateArchive, validateAndImport, listFilesRecursive, generatePassword, collectFiles } = _testing

// ---------------------------------------------------------------------------
// shared URL detection
// ---------------------------------------------------------------------------

describe('isSharedSessionUrl', () => {
  test('accepts HTTP and HTTPS shared-session URLs', () => {
    expect(isSharedSessionUrl('https://tmpfiles.org/abc/evot-log.bin#password')).toBe(true)
    expect(isSharedSessionUrl('http://tmpfiles.org/abc/evot-log.bin#password')).toBe(true)
  })

  test('rejects session ids and non-HTTP protocols', () => {
    expect(isSharedSessionUrl('abcdef01')).toBe(false)
    expect(isSharedSessionUrl('not-a-url')).toBe(false)
    expect(isSharedSessionUrl('file:///tmp/evot-log.bin#password')).toBe(false)
  })
})

// ---------------------------------------------------------------------------
// toDownloadUrl
// ---------------------------------------------------------------------------

describe('toDownloadUrl', () => {
  test('inserts /dl/ for tmpfiles.org URLs', () => {
    expect(toDownloadUrl('https://tmpfiles.org/12345/evot-log.bin'))
      .toBe('https://tmpfiles.org/dl/12345/evot-log.bin')
  })

  test('handles http variant', () => {
    expect(toDownloadUrl('http://tmpfiles.org/99/file.bin'))
      .toBe('http://tmpfiles.org/dl/99/file.bin')
  })

  test('inserts /dl/ for alphanumeric ids (tmpfiles.org current format)', () => {
    // tmpfiles.org switched from numeric to alphanumeric ids; the download
    // variant must still be produced or /share fetches the HTML viewer page.
    expect(toDownloadUrl('https://tmpfiles.org/wywHXeSghysu/evot-log.bin'))
      .toBe('https://tmpfiles.org/dl/wywHXeSghysu/evot-log.bin')
  })

  test('does not double-insert /dl/ when already a download URL', () => {
    expect(toDownloadUrl('https://tmpfiles.org/dl/wywHXeSghysu/evot-log.bin'))
      .toBe('https://tmpfiles.org/dl/wywHXeSghysu/evot-log.bin')
  })

  test('returns other URLs unchanged', () => {
    expect(toDownloadUrl('https://example.com/file.bin'))
      .toBe('https://example.com/file.bin')
  })
})

// ---------------------------------------------------------------------------
// generatePassword
// ---------------------------------------------------------------------------

describe('generatePassword', () => {
  test('generates 8-char password', () => {
    const pw = generatePassword()
    expect(pw.length).toBe(8)
  })

  test('only contains expected characters', () => {
    const allowed = 'ABCDEFGHJKLMNPQRSTUVWXYZabcdefghjkmnpqrstuvwxyz23456789'
    for (let i = 0; i < 20; i++) {
      const pw = generatePassword()
      for (const ch of pw) {
        expect(allowed.includes(ch)).toBe(true)
      }
    }
  })
})

// ---------------------------------------------------------------------------
// encrypt / decrypt round-trip
// ---------------------------------------------------------------------------

describe('encrypt / decrypt', () => {
  test('round-trip preserves data', () => {
    const original = Buffer.from('hello world — session log data')
    const { payload, password } = encrypt(original)
    const result = decrypt(payload, password)
    expect(result.equals(original)).toBe(true)
  })

  test('wrong password fails', () => {
    const original = Buffer.from('secret data')
    const { payload } = encrypt(original)
    expect(() => decrypt(payload, 'WrOnGpWd')).toThrow()
  })

  test('bad magic fails', () => {
    const bad = Buffer.from('BADMAGIC' + '0'.repeat(44))
    expect(() => decrypt(bad, 'whatever')).toThrow('Invalid file format')
  })

  test('too small payload fails', () => {
    const tiny = Buffer.from('short')
    expect(() => decrypt(tiny, 'whatever')).toThrow('too small')
  })
})

// ---------------------------------------------------------------------------
// archive validation before extraction
// ---------------------------------------------------------------------------

describe('validateArchive', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = join(tmpdir(), `evot-test-archive-${process.pid}-${Date.now()}-${Math.random()}`)
    mkdirSync(tmpDir, { recursive: true })
  })

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true })
  })

  test('accepts the expected session archive layout', () => {
    const sid = 'abcdef01-2345-6789-abcd-ef0123456789'
    mkdirSync(join(tmpDir, 'sessions', sid), { recursive: true })
    mkdirSync(join(tmpDir, 'logs'), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', sid, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'logs', `${sid}.log`), 'log')
    const archive = join(tmpDir, 'valid.tar.gz')
    execFileSync('tar', ['czf', archive, `sessions/${sid}/session.json`, `logs/${sid}.log`], { cwd: tmpDir })

    expect(() => validateArchive(archive)).not.toThrow()
  })

  test('rejects unexpected archive members', () => {
    writeFileSync(join(tmpDir, 'unexpected.txt'), 'bad')
    const archive = join(tmpDir, 'unexpected.tar.gz')
    execFileSync('tar', ['czf', archive, 'unexpected.txt'], { cwd: tmpDir })

    expect(() => validateArchive(archive)).toThrow('unsafe archive path')
  })

  test('rejects uppercase session ids to match extraction strictness', () => {
    const upper = 'ABCDEF01-2345-6789-ABCD-EF0123456789'
    mkdirSync(join(tmpDir, 'sessions', upper), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', upper, 'session.json'), '{}')
    const archive = join(tmpDir, 'upper.tar.gz')
    execFileSync('tar', ['czf', archive, `sessions/${upper}/session.json`], { cwd: tmpDir })

    expect(() => validateArchive(archive)).toThrow('unsafe archive path')
  })

  test('rejects symbolic links before extraction', () => {
    const sid = 'abcdef01-2345-6789-abcd-ef0123456789'
    mkdirSync(join(tmpDir, 'sessions', sid), { recursive: true })
    symlinkSync('/tmp', join(tmpDir, 'sessions', sid, 'session.json'))
    const archive = join(tmpDir, 'link.tar.gz')
    execFileSync('tar', ['czf', archive, `sessions/${sid}/session.json`], { cwd: tmpDir })

    expect(() => validateArchive(archive)).toThrow('unsupported entry types')
  })
})

// ---------------------------------------------------------------------------
// validateAndImport
// ---------------------------------------------------------------------------

describe('validateAndImport', () => {
  const SID = 'abcdef01-2345-6789-abcd-ef0123456789'
  let tmpDir: string

  beforeEach(() => {
    tmpDir = join(tmpdir(), `evot-test-validate-${Date.now()}`)
    mkdirSync(tmpDir, { recursive: true })
  })

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true })
  })

  test('imports valid session files', () => {
    // Create valid structure
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    mkdirSync(join(tmpDir, 'logs'), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID, 'transcript.jsonl'), '')
    writeFileSync(join(tmpDir, 'logs', `${SID}.log`), 'log data')
    writeFileSync(join(tmpDir, 'logs', `${SID}.screen.log`), 'screen data')
    writeFileSync(join(tmpDir, 'logs', `${SID}.markdown.log`), '--- markdown trace asst-1 ---\n')

    const targetDir = join(tmpdir(), `evot-test-target-${Date.now()}`)
    mkdirSync(targetDir, { recursive: true })

    const result = validateAndImport(tmpDir, targetDir)
    expect(result).toBe(SID)

    // Verify files were moved to target
    expect(existsSync(join(targetDir, 'sessions', SID, 'session.json'))).toBe(true)
    expect(existsSync(join(targetDir, 'sessions', SID, 'transcript.jsonl'))).toBe(true)
    expect(existsSync(join(targetDir, 'logs', `${SID}.log`))).toBe(true)
    expect(existsSync(join(targetDir, 'logs', `${SID}.screen.log`))).toBe(true)
    expect(existsSync(join(targetDir, 'logs', `${SID}.markdown.log`))).toBe(true)

    rmSync(targetDir, { recursive: true, force: true })
  })

  test('rejects path traversal', () => {
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    // Create a file that would be listed with ..
    mkdirSync(join(tmpDir, '..hack'), { recursive: true })
    writeFileSync(join(tmpDir, '..hack', 'evil.txt'), 'bad')

    // listFilesRecursive won't produce ".." paths from normal extraction,
    // but validateAndImport rejects unexpected files
    expect(() => validateAndImport(tmpDir)).toThrow('Rejected unsafe path')
  })

  test('rejects unexpected files', () => {
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID, 'extra.txt'), 'bad')

    expect(() => validateAndImport(tmpDir)).toThrow('Unexpected file')
  })

  test('rejects multiple session ids', () => {
    const SID2 = '11111111-2222-3333-4444-555555555555'
    mkdirSync(join(tmpDir, 'sessions', SID), { recursive: true })
    mkdirSync(join(tmpDir, 'sessions', SID2), { recursive: true })
    writeFileSync(join(tmpDir, 'sessions', SID, 'session.json'), '{}')
    writeFileSync(join(tmpDir, 'sessions', SID2, 'session.json'), '{}')

    expect(() => validateAndImport(tmpDir)).toThrow('multiple sessions')
  })

  test('rejects empty archive', () => {
    expect(() => validateAndImport(tmpDir)).toThrow('Could not determine session id')
  })
})

describe('collectFiles', () => {
  const SID = 'abcdef01-2345-6789-abcd-ef0123456789'
  const evotDir = join(homedir(), '.evotai')
  const created: string[] = []

  afterEach(() => {
    for (const dir of created.splice(0).reverse()) {
      rmSync(dir, { recursive: true, force: true })
    }
  })

  test('includes markdown trace log when present', () => {
    const sessionDir = join(evotDir, 'sessions', SID)
    const logsDir = join(evotDir, 'logs')
    mkdirSync(sessionDir, { recursive: true })
    mkdirSync(logsDir, { recursive: true })
    created.push(sessionDir)

    writeFileSync(join(sessionDir, 'session.json'), '{}')
    writeFileSync(join(logsDir, `${SID}.markdown.log`), '--- markdown trace asst-1 ---\n')

    try {
      const files = collectFiles(SID)
      expect(files).toContain(`sessions/${SID}/session.json`)
      expect(files).toContain(`logs/${SID}.markdown.log`)
    } finally {
      rmSync(join(logsDir, `${SID}.markdown.log`), { force: true })
    }
  })
})

// ---------------------------------------------------------------------------
// listFilesRecursive
// ---------------------------------------------------------------------------

describe('listFilesRecursive', () => {
  let tmpDir: string

  beforeEach(() => {
    tmpDir = join(tmpdir(), `evot-test-list-${Date.now()}`)
    mkdirSync(tmpDir, { recursive: true })
  })

  afterEach(() => {
    rmSync(tmpDir, { recursive: true, force: true })
  })

  test('lists nested files with relative paths', () => {
    mkdirSync(join(tmpDir, 'a', 'b'), { recursive: true })
    writeFileSync(join(tmpDir, 'a', 'b', 'c.txt'), '')
    writeFileSync(join(tmpDir, 'top.txt'), '')

    const files = listFilesRecursive(tmpDir)
    expect(files.sort()).toEqual(['a/b/c.txt', 'top.txt'])
  })

  test('returns empty for empty dir', () => {
    expect(listFilesRecursive(tmpDir)).toEqual([])
  })
})

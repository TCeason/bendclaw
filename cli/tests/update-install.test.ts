import { afterEach, beforeEach, describe, expect, test } from 'bun:test'
import { chmodSync, existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { executeInstall } from '../src/update/install.js'

const originalFetch = globalThis.fetch
let root = ''

beforeEach(() => {
  root = mkdtempSync(join(tmpdir(), 'evot-update-test-'))
  process.env.EVOT_INSTALL_DIR = join(root, 'bin')
  delete process.env.EVOT_HOME
})

afterEach(() => {
  globalThis.fetch = originalFetch
  delete process.env.EVOT_INSTALL_DIR
  delete process.env.EVOT_HOME
  rmSync(root, { recursive: true, force: true })
})

function installScript(version: string): string {
  return `#!/bin/sh
set -e
mkdir -p "$EVOT_INSTALL_DIR"
printf '%s\\n' '#!/bin/sh' 'printf "evot v${version}\\n"' > "$EVOT_INSTALL_DIR/evot"
chmod +x "$EVOT_INSTALL_DIR/evot"
`
}

describe('executeInstall', () => {
  test('reports install-script download failures', async () => {
    globalThis.fetch = async () => new Response('unavailable', { status: 503 })

    const result = await executeInstall('v2026.7.19')

    expect(result).toEqual({
      success: false,
      output: 'failed to download install script: HTTP 503',
    })
    expect(existsSync(join(root, 'bin', 'evot'))).toBe(false)
  })

  test('rejects a successful script that did not install the requested version', async () => {
    globalThis.fetch = async () => new Response(installScript('2026.7.10.2'))

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(false)
    expect(result.output).toContain('installed version mismatch')
    expect(result.output).toContain('expected evot v2026.7.19')
    expect(result.output).toContain('got evot v2026.7.10.2')
  })

  test('accepts an installed binary with the requested version', async () => {
    globalThis.fetch = async () => new Response(installScript('2026.7.19'))

    const result = await executeInstall('v2026.7.19')

    expect(result.success).toBe(true)
    expect(readFileSync(join(root, 'bin', 'evot'), 'utf8')).toContain('2026.7.19')
  })
})

describe('install.sh', () => {
  test('validates the candidate before replacing the installed binary', async () => {
    const archiveRoot = join(root, 'archive')
    const archive = join(root, 'release.tar.gz')
    const installDir = join(root, 'installed', 'bin')
    const fakeBin = join(root, 'fake-bin')
    mkdirSync(join(archiveRoot, 'bin'), { recursive: true })
    mkdirSync(installDir, { recursive: true })
    mkdirSync(fakeBin, { recursive: true })

    writeFileSync(join(archiveRoot, 'bin', 'evot'), '#!/bin/sh\nprintf "evot v2026.7.18\\n"\n')
    chmodSync(join(archiveRoot, 'bin', 'evot'), 0o755)
    writeFileSync(join(installDir, 'evot'), '#!/bin/sh\nprintf "evot vold\\n"\n')
    chmodSync(join(installDir, 'evot'), 0o755)

    const tar = Bun.spawnSync(['tar', '-C', archiveRoot, '-czf', archive, 'bin'])
    expect(tar.exitCode).toBe(0)

    writeFileSync(join(fakeBin, 'uname'), `#!/bin/sh
if [ "\${1:-}" = "-s" ]; then printf 'Linux\\n'; else printf 'x86_64\\n'; fi
`)
    writeFileSync(join(fakeBin, 'curl'), `#!/bin/sh
output=''
while [ "$#" -gt 0 ]; do
  if [ "$1" = '-o' ]; then output="$2"; shift 2; continue; fi
  shift
done
if [ -n "$output" ]; then cp "$TEST_ARCHIVE" "$output"; exit 0; fi
exit 22
`)
    chmodSync(join(fakeBin, 'uname'), 0o755)
    chmodSync(join(fakeBin, 'curl'), 0o755)

    const proc = Bun.spawn(['sh', join(import.meta.dir, '..', '..', 'install.sh')], {
      stdout: 'pipe',
      stderr: 'pipe',
      env: {
        ...process.env,
        PATH: `${fakeBin}:/usr/bin:/bin`,
        TEST_ARCHIVE: archive,
        EVOT_INSTALL_DIR: installDir,
        EVOT_INSTALL_VERSION: 'v2026.7.19',
      },
    })
    const [stderr, exitCode] = await Promise.all([
      new Response(proc.stderr).text(),
      proc.exited,
    ])

    expect(exitCode).not.toBe(0)
    expect(stderr).toContain('Downloaded version mismatch')
    const current = Bun.spawnSync([join(installDir, 'evot'), '--version'])
    expect(current.stdout.toString().trim()).toBe('evot vold')
  })
})

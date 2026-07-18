import { afterEach, beforeAll, describe, expect, test } from 'bun:test'
import chalk from 'chalk'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { renderBanner } from '../src/term/banner.js'

beforeAll(() => {
  chalk.level = 3
})

const roots: string[] = []

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true })
})

describe('renderBanner terminal width', () => {
  test('wraps long skill lists to physical lines within terminal width', () => {
    const root = mkdtempSync(join(tmpdir(), 'evot-banner-'))
    roots.push(root)
    for (let index = 0; index < 40; index++) {
      const skill = join(root, `skill-${index.toString().padStart(2, '0')}`)
      mkdirSync(skill)
      writeFileSync(join(skill, 'SKILL.md'), '# Skill\n')
    }

    const columns = 40
    const banner = renderBanner({
      model: 'model',
      cwd: root,
      configInfo: undefined,
      columns,
      skillsDirs: [root],
    })

    for (const line of banner.split('\n')) {
      expect(stringWidth(stripAnsi(line))).toBeLessThanOrEqual(columns)
    }
  })
})

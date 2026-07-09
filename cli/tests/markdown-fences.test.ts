/**
 * Fixture-driven fence repair cases.
 *
 * Each directory under tests/fixtures/fences/<case-id>/ holds:
 *   in.md   — raw model markdown
 *   out.md  — expected prepareMarkdownFences() output
 *
 * New screenshot bug workflow: drop a fixture → red → adjust one case in
 * cases.ts → green. Do not patch unrelated cases.
 */

import { describe, expect, test } from 'bun:test'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join } from 'node:path'
import { prepareMarkdownFences } from '../src/markdown/normalize/fences/index.js'
import { renderMarkdown } from '../src/render/markdown.js'
import stripAnsi from 'strip-ansi'

const FIXTURES_ROOT = join(import.meta.dir, 'fixtures/fences')

function listFixtureCases(): string[] {
  return readdirSync(FIXTURES_ROOT)
    .filter(name => statSync(join(FIXTURES_ROOT, name)).isDirectory())
    .sort()
}

describe('prepareMarkdownFences fixtures', () => {
  for (const id of listFixtureCases()) {
    test(id, () => {
      const dir = join(FIXTURES_ROOT, id)
      const input = readFileSync(join(dir, 'in.md'), 'utf8')
      const expected = readFileSync(join(dir, 'out.md'), 'utf8')
      expect(prepareMarkdownFences(input)).toBe(expected)
    })
  }
})

describe('shell-close-before-md-boundary render', () => {
  test('heading and table after unclosed bash are not swallowed as code', () => {
    const md = [
      '```bash',
      'cursor-agent --model grok',
      '## 排除的项目',
      '| 项目 | 原因 |',
      '|---|---|',
      '| foo | `bar` |',
    ].join('\n')

    const result = stripAnsi(renderMarkdown(md)).replace(/\u200b/g, '')

    expect(result).toContain('cursor-agent --model grok')
    expect(result).toContain('排除的项目')
    // Heading must not sit inside an indented code block.
    expect(result).not.toMatch(/^ {2}## 排除的项目/m)
    // Table separator must be consumed by the table renderer.
    expect(result).not.toContain('|---|---|')
    expect(result).toContain('┌')
    // Inline code inside the table cell should render, not leak backticks as
    // part of a swallowed code fence body.
    expect(result).toContain('bar')
  })
})

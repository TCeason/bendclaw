import { mkdtempSync, readFileSync, rmSync } from 'fs'
import { tmpdir } from 'os'
import { join } from 'path'
import { describe, expect, test } from 'bun:test'
import { ScreenLog } from '../src/session/screen-log.js'

describe('ScreenLog', () => {
  test('writes rendered screen log and raw markdown trace separately', () => {
    const originalHome = process.env.HOME
    const tmp = mkdtempSync(join(tmpdir(), 'evot-screen-log-'))
    process.env.HOME = tmp

    try {
      const log = new ScreenLog()
      log.bind('00000000-0000-0000-0000-000000000001')
      log.logLines(['Title', 'rendered table'])
      log.logMarkdownTrace({
        messageId: 'asst-1',
        rendererVersion: 'test-renderer',
        rawMarkdown: '## Title\n\n| A | B |\n|---|---|',
        renderedLines: ['Title', '┌───┬───┐'],
      })

      const screen = readFileSync(log.filePath ?? '', 'utf8')
      expect(screen).toContain('Title')
      expect(screen).toContain('rendered table')
      expect(screen).not.toContain('|---|---|')

      const trace = readFileSync(log.markdownTraceFilePath ?? '', 'utf8')
      expect(log.markdownTraceFilePath).toEndWith('.markdown.log')
      expect(trace).toContain('--- markdown trace asst-1 ---')
      expect(trace).toContain('renderer_version: test-renderer')
      expect(trace).toContain('[raw markdown]')
      expect(trace).toContain('## Title\n\n| A | B |\n|---|---|')
      expect(trace).toContain('[rendered lines]')
      expect(trace).toContain('┌───┬───┐')
      expect(trace).toContain('--- end markdown trace asst-1 ---')
    } finally {
      if (originalHome === undefined) delete process.env.HOME
      else process.env.HOME = originalHome
      rmSync(tmp, { recursive: true, force: true })
    }
  })
})

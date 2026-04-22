import { describe, test, expect, beforeEach, afterEach } from 'bun:test'
import stripAnsi from 'strip-ansi'

// We need to test with controlled env, so import after setting env
describe('supportsHyperlinks', () => {
  const origEnv = { ...process.env }

  beforeEach(() => {
    // Clear relevant env vars
    delete process.env.FORCE_HYPERLINK
    delete process.env.CI
    delete process.env.TERM
    delete process.env.TERM_PROGRAM
    delete process.env.WT_SESSION
    delete process.env.KITTY_PID
    delete process.env.VTE_VERSION
  })

  afterEach(() => {
    // Restore original env
    Object.assign(process.env, origEnv)
  })

  test('FORCE_HYPERLINK=1 enables', async () => {
    process.env.FORCE_HYPERLINK = '1'
    // Re-import to pick up env changes
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(true)
  })

  test('FORCE_HYPERLINK=0 disables', async () => {
    process.env.FORCE_HYPERLINK = '0'
    process.env.TERM_PROGRAM = 'iTerm.app'
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(false)
  })

  test('CI disables', async () => {
    process.env.CI = 'true'
    process.env.TERM_PROGRAM = 'iTerm.app'
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(false)
  })

  test('dumb terminal disables', async () => {
    process.env.TERM = 'dumb'
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(false)
  })

  test('iTerm.app enables', async () => {
    process.env.TERM_PROGRAM = 'iTerm.app'
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(true)
  })

  test('Apple_Terminal disables', async () => {
    process.env.TERM_PROGRAM = 'Apple_Terminal'
    const { supportsHyperlinks } = await import('../src/render/hyperlink.js')
    expect(supportsHyperlinks()).toBe(false)
  })
})

describe('createHyperlink', () => {
  test('with hyperlinks unsupported, returns plain URL', async () => {
    process.env.FORCE_HYPERLINK = '0'
    const { createHyperlink } = await import('../src/render/hyperlink.js')
    const result = createHyperlink('https://example.com', 'click me')
    expect(result).toBe('https://example.com')
  })

  test('with hyperlinks supported, returns OSC 8 sequence', async () => {
    process.env.FORCE_HYPERLINK = '1'
    const { createHyperlink } = await import('../src/render/hyperlink.js')
    const result = createHyperlink('https://example.com', 'click me')
    expect(result).toContain('\x1b]8;;https://example.com\x07')
    expect(result).toContain('\x1b]8;;\x07')
    expect(stripAnsi(result.replace(/\x1b\]8;;[^\x07]*\x07/g, ''))).toBe('click me')
  })

  test('without text, uses URL as display', async () => {
    process.env.FORCE_HYPERLINK = '1'
    const { createHyperlink } = await import('../src/render/hyperlink.js')
    const result = createHyperlink('https://example.com')
    expect(result).toContain('\x1b]8;;https://example.com\x07')
    expect(stripAnsi(result.replace(/\x1b\]8;;[^\x07]*\x07/g, ''))).toBe('https://example.com')
  })
})

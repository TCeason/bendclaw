import { describe, test, expect, beforeEach } from 'bun:test'
import { Writable } from 'node:stream'
import { TermRenderer } from '../src/term/renderer.js'

// Mock stdout that captures writes
class MockStdout extends Writable {
  chunks: string[] = []
  rows = 24
  columns = 80

  _write(chunk: Buffer | string, _encoding: string, callback: () => void) {
    this.chunks.push(chunk.toString())
    callback()
  }

  get output(): string {
    return this.chunks.join('')
  }

  clear() {
    this.chunks = []
  }

  // Simulate event emitter for resize
  private listeners: Map<string, Function[]> = new Map()
  on(event: string, fn: Function): this {
    const list = this.listeners.get(event) ?? []
    list.push(fn)
    this.listeners.set(event, list)
    return this
  }
  off(event: string, fn: Function): this {
    const list = this.listeners.get(event) ?? []
    this.listeners.set(event, list.filter(f => f !== fn))
    return this
  }
  emit(event: string, ...args: any[]): boolean {
    const list = this.listeners.get(event) ?? []
    for (const fn of list) fn(...args)
    return list.length > 0
  }
}

function createRenderer(): { renderer: TermRenderer; stdout: MockStdout } {
  const stdout = new MockStdout() as any
  const renderer = new TermRenderer({ stdout })
  return { renderer, stdout }
}

// Helper: trigger a synchronous render by calling requestRender + flushing nextTick
async function renderFrame(renderer: TermRenderer): Promise<void> {
  renderer.requestRender()
  await new Promise(resolve => process.nextTick(resolve))
  // Wait for the scheduled render (MIN_RENDER_INTERVAL_MS = 16ms)
  await Bun.sleep(20)
}

describe('TermRenderer', () => {
  describe('init / destroy', () => {
    test('init hides cursor', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      expect(stdout.output).toContain('\x1b[?25l')
      renderer.destroy()
    })

    test('destroy shows cursor', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.destroy()
      expect(stdout.output).toContain('\x1b[?25h')
    })

    test('double destroy is safe', () => {
      const { renderer } = createRenderer()
      renderer.init()
      renderer.destroy()
      renderer.destroy() // should not throw
    })
  })

  describe('differential rendering', () => {
    test('first render outputs all lines', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2', 'line3'])
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toContain('line1')
      expect(stdout.output).toContain('line2')
      expect(stdout.output).toContain('line3')
      renderer.destroy()
    })

    test('identical frames produce no output', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toBe('')
      renderer.destroy()
    })

    test('appended lines use append fast path', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1', 'line2', 'line3']
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('line3')
      // Should not redraw line1 or line2
      expect(out).not.toContain('line1')
      expect(out).not.toContain('line2')
      renderer.destroy()
    })

    test('changed middle line only redraws that line', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2', 'line3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1', 'CHANGED', 'line3']
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('CHANGED')
      expect(out).not.toContain('line1')
      expect(out).not.toContain('line3')
      renderer.destroy()
    })

    test('shrinking content clears extra lines', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['line1', 'line2', 'line3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)
      stdout.clear()
      lines = ['line1']
      await renderFrame(renderer)
      const out = stdout.output
      // Should contain clear line sequences for removed lines
      expect(out).toContain('\x1b[2K')
      renderer.destroy()
    })

    test('shrinking a visible frame clears removed rows without repainting the viewport', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      renderer.init()

      let lines = Array.from({ length: 8 }, (_, i) => `old ${i}`)
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      lines = ['history', 'Thinking...', '────────', '❯ ', '────────', 'footer']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain('\x1b[2J\x1b[H')
      expect(out).toContain('\x1b[2K')
      expect(out).toContain('history')
      expect(out).toContain('footer')
      renderer.destroy()
    })

    test('reanchors a viewport-up shrink instead of leaving a blank band', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 8
      renderer.init()

      let lines = Array.from({ length: 20 }, (_, i) => `old ${i}`)
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      // Mirrors the observed transition: a completed live region disappears,
      // moving the logical viewport up while spinner + prompt remain at the tail.
      lines = [
        ...Array.from({ length: 8 }, (_, i) => `history ${i}`),
        'Thinking...',
        '────────',
        '❯ ',
        '────────',
        'footer',
        '',
      ]
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain('\x1b[2J\x1b[H')
      expect(out).not.toContain('\x1b[3J')
      expect(out).toContain('Thinking...')
      expect(out).toContain('footer')
      renderer.destroy()
    })

    test('uses synchronized output wrapping', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['hello'])
      stdout.clear()
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('\x1b[?2026h') // sync start
      expect(out).toContain('\x1b[?2026l') // sync end
      renderer.destroy()
    })
  })

  describe('off-viewport change (streaming markdown reflow)', () => {
    // When streaming, markdown re-renders the whole accumulated text each frame
    // and reflows earlier lines (table realign, list renumber). When a reflowed
    // line has scrolled above the visible viewport, no escape sequence can
    // address it, so the renderer falls back to a full redraw (matching pi's
    // renderer). A prior attempt to repaint in place instead desynced the
    // on-screen window from the terminal's real scrollback and made text
    // selections jump on scroll.
    const CLEAR_VIEWPORT = '\x1b[2J\x1b[H'
    const CLEAR_SCROLLBACK = '\x1b[3J'

    test('changing a line above the viewport triggers a full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 30 }, (_, i) => `hist ${i}`)
      let lines = [...history, 's0', 's1', 's2', 's3']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      // Append more streamed lines so the viewport scrolls well past history.
      lines = [...history, 's0', 's1', 's2', 's3', 's4', 's5']
      await renderFrame(renderer)

      // Now reflow an early line that is above the viewport, while appending one.
      // A differential update can't address rows already in scrollback, so the
      // renderer falls back to a full redraw (matching pi's renderer).
      stdout.clear()
      const reflowed = [...history]
      reflowed[5] = 'hist 5 REFLOWED'
      lines = [...reflowed, 's0', 's1', 's2', 's3', 's4', 's5', 's6']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_VIEWPORT)
      expect(out).not.toContain(CLEAR_SCROLLBACK)
      // Only the addressable viewport is repainted. The historical line stays
      // in terminal scrollback rather than being erased and reconstructed.
      expect(out).not.toContain('hist 5 REFLOWED')
      expect(out).toContain('s6')
      renderer.destroy()
    })

    test('full redraw keeps the newest content visible', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 6
      renderer.init()
      const history = Array.from({ length: 20 }, (_, i) => `h${i}`)
      let lines = [...history, 'a', 'b', 'c']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      stdout.clear()
      const reflowed = [...history]
      reflowed[0] = 'h0-changed'
      lines = [...reflowed, 'a', 'b', 'c', 'd']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).toContain(CLEAR_VIEWPORT)
      expect(out).not.toContain(CLEAR_SCROLLBACK)
      expect(out).not.toContain('h0-changed')
      expect(out).toContain('d')
      renderer.destroy()
    })

    // A change confined entirely above the viewport, with the frame's total line
    // count unchanged, leaves the visible region byte-for-byte identical. The
    // only differences are in scrollback, which no escape sequence can address
    // without a destructive clear+reprint. Emitting that clear is what made the
    // screen "jump" to the top on an off-screen banner update or early markdown
    // reflow. The renderer must adopt the new lines silently and NOT redraw.
    test('off-viewport change with unchanged line count does not redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = 'banner: main'
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [banner, ...history])
      await renderFrame(renderer)

      // The banner sits at row 0, far above the viewport. Changing it (e.g. a
      // git branch switch or update notice) must not clear the screen.
      stdout.clear()
      banner = 'banner: feature-branch'
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_VIEWPORT)
      // Nothing visible changed, so no line content is reprinted either.
      expect(out).not.toContain('feature-branch')
      renderer.destroy()
    })

    test('off-viewport early reflow (count unchanged) does not redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 8 }, (_, i) => `H${i}`)
      let pending = Array.from({ length: 12 }, (_, i) => `P${i}`)
      renderer.setRenderCallback(() => [...history, ...pending])
      await renderFrame(renderer)

      // Reflow an early pending line that has scrolled above the viewport, with
      // no change to the total line count (in-place table realign / renumber).
      stdout.clear()
      pending = [...pending]
      pending[1] = 'P1-REFLOWED'
      await renderFrame(renderer)

      expect(stdout.output).not.toContain(CLEAR_VIEWPORT)
      renderer.destroy()
    })

    // The real-world jump: an async banner GROWS a line mid-session (update
    // notice / release notes arrive). The banner sits at row 0, long scrolled
    // above the viewport, so the visible region is byte-identical — only its
    // buffer indices shifted by +1. A common-suffix match proves the viewport
    // is unchanged, so the renderer adopts silently instead of clearing.
    test('off-viewport line-count GROWTH with identical viewport does not redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = ['banner']
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [...banner, ...history])
      await renderFrame(renderer)

      // Banner grows from 1 line to 2 (async update notice). Visible rows are
      // the tail of history, unchanged.
      stdout.clear()
      banner = ['banner', 'New version available']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_VIEWPORT)
      // The grown banner line is off-screen, so it is never printed.
      expect(out).not.toContain('New version available')
      renderer.destroy()
    })

    // Same for a SHRINK: an off-screen banner line disappears. Buffer indices
    // shift by -1 but the viewport is identical, so no clear.
    test('off-viewport line-count SHRINK with identical viewport does not redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let banner = ['banner', 'transient notice']
      const history = Array.from({ length: 200 }, (_, i) => `hist ${i}`)
      renderer.setRenderCallback(() => [...banner, ...history])
      await renderFrame(renderer)

      stdout.clear()
      banner = ['banner']
      await renderFrame(renderer)

      expect(stdout.output).not.toContain(CLEAR_VIEWPORT)
      renderer.destroy()
    })

    // A change that only reaches PART of the viewport, with the line count
    // unchanged, repaints just the visible rows — the unaddressable scrollback
    // above is left stale (invisible) rather than triggering a full clear.
    test('partial-viewport reach (count unchanged) repaints visible rows without clearing', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      const history = Array.from({ length: 8 }, (_, i) => `H${i}`)
      // 12 pending: with rows=10 and 20 total lines, viewportTop = 10.
      let pending = Array.from({ length: 12 }, (_, i) => `P${i}`)
      renderer.setRenderCallback(() => [...history, ...pending])
      await renderFrame(renderer)

      // Change a line that spans from above the viewport (index 9, off-screen)
      // to inside it (index 11, visible). Count unchanged.
      stdout.clear()
      pending = [...pending]
      pending[1] = 'P1-off'   // buffer index 9 — above viewport
      pending[3] = 'P3-vis'   // buffer index 11 — inside viewport
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_VIEWPORT)
      // The visible changed row is repainted; the off-screen one is not.
      expect(out).toContain('P3-vis')
      expect(out).not.toContain('P1-off')
      renderer.destroy()
    })

    // Ctrl+O expand: a tool block in the viewport grows from compact to
    // expanded with the prompt below it. The change starts inside the viewport,
    // so the renderer repaints in place from the first changed row down and
    // scrolls the prompt naturally — no viewport clear, no jump to the top.
    test('expanding in-viewport content grows in place without clearing', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 12
      renderer.init()
      // Small transcript that fits entirely on screen: a couple of history
      // lines, a compact tool card, then the prompt.
      let lines = ['h0', 'h1', 'tool ✓ 2 lines', 'prompt']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      // Ctrl+O expands the tool card into its full output. The card and prompt
      // are all visible, so this must NOT clear the screen.
      stdout.clear()
      lines = ['h0', 'h1', 'tool ✓ 2 lines', '  out line 1', '  out line 2', '  out line 3', 'prompt']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_VIEWPORT)
      expect(out).toContain('out line 3')
      expect(out).toContain('prompt')
      renderer.destroy()
    })
  })

  describe('freezeLines', () => {
    test('frozen lines are not redrawn on next frame', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let lines = ['frozen1', 'frozen2', 'active1']
      renderer.setRenderCallback(() => lines)
      await renderFrame(renderer)

      // Freeze the first 2 lines
      renderer.freezeLines(2)
      stdout.clear()

      // Change active content
      lines = ['frozen1', 'frozen2', 'active-changed']
      // After freeze, renderer only tracks ['active1'] as previous
      // New callback returns 3 lines but renderer compares against ['active1']
      renderer.setRenderCallback(() => ['active-changed'])
      await renderFrame(renderer)
      const out = stdout.output
      expect(out).toContain('active-changed')
      expect(out).not.toContain('frozen1')
      expect(out).not.toContain('frozen2')
      renderer.destroy()
    })

    test('freeze with count 0 does nothing', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1'])
      await renderFrame(renderer)
      stdout.clear()
      renderer.freezeLines(0)
      renderer.setRenderCallback(() => ['line1-changed'])
      await renderFrame(renderer)
      expect(stdout.output).toContain('line1-changed')
      renderer.destroy()
    })
  })

  describe('clearScreen', () => {
    test('clears screen and resets state', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)
      stdout.clear()
      renderer.clearScreen()
      const out = stdout.output
      expect(out).toContain('\x1b[2J') // clear screen
      expect(out).toContain('\x1b[H')  // cursor home
      expect(out).toContain('\x1b[3J') // clear scrollback only on explicit clear
      renderer.destroy()
    })
  })

  describe('fullRedraw', () => {
    test('force redraws all lines', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      renderer.setRenderCallback(() => ['line1', 'line2'])
      await renderFrame(renderer)
      stdout.clear()
      renderer.fullRedraw()
      await new Promise(resolve => process.nextTick(resolve))
      await Bun.sleep(5)
      const out = stdout.output
      expect(out).toContain('line1')
      expect(out).toContain('line2')
      renderer.destroy()
    })
  })

  describe('resize handling', () => {
    test('updates dimensions on resize', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.rows = 40
      stdout.columns = 120
      stdout.emit('resize')
      expect(renderer.termRows).toBe(40)
      expect(renderer.termCols).toBe(120)
      renderer.destroy()
    })

    test('falls back when resize dimensions are non-finite', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.rows = Infinity
      stdout.columns = NaN
      stdout.emit('resize')
      expect(renderer.termRows).toBe(24)
      expect(renderer.termCols).toBe(80)
      renderer.destroy()
    })
  })

  describe('render throttling', () => {
    test('multiple requestRender calls coalesce into one render', async () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      let callCount = 0
      renderer.setRenderCallback(() => {
        callCount++
        return ['frame ' + callCount]
      })
      stdout.clear()
      renderer.requestRender()
      renderer.requestRender()
      renderer.requestRender()
      await new Promise(resolve => process.nextTick(resolve))
      await Bun.sleep(20)
      // Should only have rendered once
      expect(callCount).toBe(1)
      renderer.destroy()
    })
  })

  describe('line truncation', () => {
    test('lines wider than terminal are clipped by DECAWM off', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.columns = 20
      renderer.init()
      // Verify DECAWM off (no-wrap) is sent on init
      expect(stdout.output).toContain('\x1b[?7l')
      const longLine = 'A'.repeat(50)
      renderer.setRenderCallback(() => [longLine])
      stdout.clear()
      await renderFrame(renderer)
      // The renderer outputs the full line — the terminal clips it
      expect(stdout.output).toContain(longLine)
      renderer.destroy()
    })

    test('destroy re-enables auto-wrap', () => {
      const { renderer, stdout } = createRenderer()
      renderer.init()
      stdout.clear()
      renderer.destroy()
      expect(stdout.output).toContain('\x1b[?7h')
    })

    test('lines within terminal width render normally', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.columns = 80
      renderer.init()
      const shortLine = 'Hello world'
      renderer.setRenderCallback(() => [shortLine])
      stdout.clear()
      await renderFrame(renderer)
      expect(stdout.output).toContain(shortLine)
      renderer.destroy()
    })
  })
})

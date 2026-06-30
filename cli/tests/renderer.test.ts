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
    // Root cause of the "jump to previous conversation" bug: while streaming,
    // markdown re-renders the whole accumulated text each frame and reflows
    // earlier lines (table realign, list renumber). When a reflowed line has
    // scrolled above the visible viewport, the old code emitted CLEAR_SCREEN
    // and repainted from the top of the frame, visibly jumping the view.
    const CLEAR_SCREEN = '\x1b[2J\x1b[H\x1b[3J'

    test('changing a line above the viewport does not clear the screen', async () => {
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
      stdout.clear()
      const reflowed = [...history]
      reflowed[5] = 'hist 5 REFLOWED'
      lines = [...reflowed, 's0', 's1', 's2', 's3', 's4', 's5', 's6']
      await renderFrame(renderer)

      const out = stdout.output
      expect(out).not.toContain(CLEAR_SCREEN)
      // Latest streamed line stays visible.
      expect(out).toContain('s6')
      renderer.destroy()
    })

    test('in-place repaint keeps the newest content visible', async () => {
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
      expect(out).not.toContain(CLEAR_SCREEN)
      expect(out).toContain('d')
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

    test('stable frame prefix is frozen and not redrawn on active updates', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 1
      renderer.init()
      let prompt = 'prompt-a'
      renderer.setRenderCallback(() => ({ lines: ['history', prompt], stablePrefixLines: 1 }))
      await renderFrame(renderer)

      stdout.clear()
      prompt = 'prompt-b'
      await renderFrame(renderer)

      expect(stdout.output).toContain('prompt-b')
      expect(stdout.output).not.toContain('history')
      renderer.destroy()
    })

    test('changed frozen prefix triggers a full clear redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 1
      renderer.init()
      let history = 'history-a'
      renderer.setRenderCallback(() => ({ lines: [history, 'prompt'], stablePrefixLines: 1 }))
      await renderFrame(renderer)

      stdout.clear()
      history = 'history-b'
      await renderFrame(renderer)

      expect(stdout.output).toContain('\x1b[2J')
      expect(stdout.output).toContain('history-b')
      renderer.destroy()
    })

    test('frozen lines reset on forced full redraw', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 1
      renderer.init()
      let history = 'history-a'
      renderer.setRenderCallback(() => ({ lines: [history, 'prompt'], stablePrefixLines: 1 }))
      await renderFrame(renderer)

      stdout.clear()
      history = 'history-b'
      renderer.fullRedraw()
      await new Promise(resolve => process.nextTick(resolve))
      await Bun.sleep(5)

      expect(stdout.output).toContain('\x1b[2J')
      expect(stdout.output).toContain('history-b')
      renderer.destroy()
    })

    test('visible stable prefix stays in active viewport to keep cursor coordinates stable', async () => {
      const { renderer, stdout } = createRenderer()
      stdout.rows = 10
      renderer.init()
      let history = 'history-a'
      renderer.setRenderCallback(() => ({ lines: [history, 'prompt'], stablePrefixLines: 1 }))
      await renderFrame(renderer)

      stdout.clear()
      history = 'history-b'
      await renderFrame(renderer)

      expect(stdout.output).not.toContain('\x1b[2J')
      expect(stdout.output).toContain('history-b')
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

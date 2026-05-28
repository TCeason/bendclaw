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
})

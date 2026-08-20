import { describe, expect, test } from 'bun:test'
import { CaretBlink } from '../src/term/caret-blink.js'

/** First flip lands at idleDelay + interval (80ms), so samples sit at 20/100/140ms. */
function createBlink() {
  let renders = 0
  const blink = new CaretBlink({
    onChange: () => { renders++ },
    idleDelayMs: 40,
    intervalMs: 40,
  })
  return { blink, renders: () => renders }
}

describe('CaretBlink', () => {
  test('starts solid and stays solid until the idle delay elapses', async () => {
    const { blink, renders } = createBlink()
    expect(blink.visible).toBe(true)
    await Bun.sleep(20)
    expect(blink.visible).toBe(true)
    expect(renders()).toBe(0)
    blink.dispose()
  })

  test('blinks after going idle and requests a frame per flip', async () => {
    const { blink, renders } = createBlink()
    await Bun.sleep(100)
    expect(blink.visible).toBe(false)
    expect(renders()).toBeGreaterThanOrEqual(1)
    await Bun.sleep(40)
    expect(blink.visible).toBe(true)
    blink.dispose()
  })

  test('bump restores the caret and restarts the idle countdown', async () => {
    const { blink } = createBlink()
    await Bun.sleep(100)
    expect(blink.visible).toBe(false)
    blink.bump()
    expect(blink.visible).toBe(true)
    await Bun.sleep(20)
    expect(blink.visible).toBe(true)
    blink.dispose()
  })

  test('bump only asks for a frame when it actually changed the caret', async () => {
    const { blink, renders } = createBlink()
    blink.bump()
    blink.bump()
    expect(renders()).toBe(0)
    await Bun.sleep(100)
    const afterBlink = renders()
    blink.bump()
    expect(renders()).toBe(afterBlink + 1)
    blink.dispose()
  })

  test('disabling holds the caret solid and stops the timers', async () => {
    const { blink, renders } = createBlink()
    blink.setEnabled(false)
    await Bun.sleep(100)
    expect(blink.visible).toBe(true)
    expect(renders()).toBe(0)
    blink.dispose()
  })

  test('re-enabling resumes blinking after a fresh idle delay', async () => {
    const { blink } = createBlink()
    blink.setEnabled(false)
    blink.setEnabled(true)
    expect(blink.visible).toBe(true)
    await Bun.sleep(100)
    expect(blink.visible).toBe(false)
    blink.dispose()
  })

  test('dispose stops blinking and leaves the caret solid', async () => {
    const { blink, renders } = createBlink()
    await Bun.sleep(100)
    blink.dispose()
    const afterDispose = renders()
    expect(blink.visible).toBe(true)
    await Bun.sleep(100)
    expect(renders()).toBe(afterDispose)
    expect(blink.visible).toBe(true)
  })

  test('bump and setEnabled are inert after dispose', () => {
    const { blink, renders } = createBlink()
    blink.dispose()
    blink.bump()
    blink.setEnabled(false)
    expect(blink.visible).toBe(true)
    expect(renders()).toBe(0)
  })
})

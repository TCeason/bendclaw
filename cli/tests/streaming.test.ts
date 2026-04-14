import { describe, test, expect } from 'bun:test'
import { shouldAnimateTerminalTitle } from '../src/utils/streaming.js'

describe('shouldAnimateTerminalTitle', () => {
  test('returns false by default', () => {
    delete process.env.EVOT_ANIMATE_TITLE
    expect(shouldAnimateTerminalTitle()).toBe(false)
  })

  test('returns true when env is set to 1', () => {
    process.env.EVOT_ANIMATE_TITLE = '1'
    expect(shouldAnimateTerminalTitle()).toBe(true)
    delete process.env.EVOT_ANIMATE_TITLE
  })
})

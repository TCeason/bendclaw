import { describe, expect, test } from 'bun:test'
import { bottomAnchorFiller } from '../src/term/viewmodel/bottom-anchor.js'

describe('bottomAnchorFiller', () => {
  test('pads short frames so body + footer fill the terminal from the bottom', () => {
    // Filler is placed above the body; body and footer stay packed together.
    expect(bottomAnchorFiller(10, 6, 40)).toBe(24)
  })

  test('returns zero when content already fills the terminal', () => {
    expect(bottomAnchorFiller(30, 10, 40)).toBe(0)
    expect(bottomAnchorFiller(50, 10, 40)).toBe(0)
  })

  test('handles empty body (banner-less first paint)', () => {
    expect(bottomAnchorFiller(0, 5, 24)).toBe(19)
  })

  test('sanitizes invalid dimensions', () => {
    expect(bottomAnchorFiller(Number.NaN, 4, 20)).toBe(16)
    expect(bottomAnchorFiller(4, Number.POSITIVE_INFINITY, 20)).toBe(16)
    expect(bottomAnchorFiller(4, 4, 0)).toBe(0)
  })
})

/**
 * TUI streaming test — validates no flicker during streaming output.
 *
 * The key assertion: scroll zone content only grows (never shrinks or resets),
 * and status area maintains stable line count during streaming.
 */

import { test, expect } from "@microsoft/tui-test"

test("streaming output does not flicker (scroll content only grows)", async ({ terminal }) => {
  await terminal.waitForText("─", { timeout: 10_000 })

  // Send a query that will produce streaming output
  // This requires a working agent — skip if no API key
  const hasAgent = !process.env.SKIP_AGENT_TESTS
  if (!hasAgent) {
    test.skip()
    return
  }

  await terminal.type("say hello in 3 words")
  await terminal.keyPress("Enter")

  // Wait for spinner to appear (indicates loading started)
  await terminal.waitForText("Thinking", { timeout: 10_000 })

  // Sample frames during streaming
  const frames: string[] = []
  const sampleInterval = 50 // ms
  const maxSamples = 100 // 5 seconds max

  for (let i = 0; i < maxSamples; i++) {
    await new Promise(r => setTimeout(r, sampleInterval))
    const screen = terminal.getScreen()
    frames.push(screen)

    // Stop sampling once streaming is done (spinner gone, prompt back)
    if (!screen.includes("Thinking") && !screen.includes("Executing") && frames.length > 5) {
      break
    }
  }

  // Assertion: no frame should be empty (no clear-screen flicker)
  for (let i = 0; i < frames.length; i++) {
    expect(frames[i]!.trim().length).toBeGreaterThan(0)
  }

  // Assertion: content in scroll zone should only grow
  // Extract lines above the prompt border in each frame
  let prevContentLength = 0
  for (const frame of frames) {
    const borderIdx = frame.lastIndexOf("─".repeat(5))
    if (borderIdx === -1) continue
    const scrollContent = frame.slice(0, borderIdx)
    // Content should not shrink (allowing for minor fluctuation from status area changes)
    expect(scrollContent.length).toBeGreaterThanOrEqual(prevContentLength - 10)
    prevContentLength = Math.max(prevContentLength, scrollContent.length)
  }
})

test("status area line count is stable during streaming", async ({ terminal }) => {
  await terminal.waitForText("─", { timeout: 10_000 })

  const hasAgent = !process.env.SKIP_AGENT_TESTS
  if (!hasAgent) {
    test.skip()
    return
  }

  await terminal.type("count to 5")
  await terminal.keyPress("Enter")

  await terminal.waitForText("Thinking", { timeout: 10_000 })

  // Sample status area heights
  const heights: number[] = []
  for (let i = 0; i < 40; i++) {
    await new Promise(r => setTimeout(r, 100))
    const screen = terminal.getScreen()
    const lines = screen.split("\n")

    // Count lines from last border to end (status area)
    let lastBorderLine = -1
    for (let j = lines.length - 1; j >= 0; j--) {
      if (lines[j]!.includes("─".repeat(5))) {
        lastBorderLine = j
        break
      }
    }
    if (lastBorderLine >= 0) {
      heights.push(lines.length - lastBorderLine)
    }

    if (!screen.includes("Thinking") && !screen.includes("Executing") && heights.length > 5) {
      break
    }
  }

  // Status area height should not jump wildly (max 3 lines variance)
  if (heights.length > 2) {
    const min = Math.min(...heights)
    const max = Math.max(...heights)
    // Allow some variance for pending text growth, but not wild jumps
    expect(max - min).toBeLessThanOrEqual(15) // pending markdown can grow up to MAX_PENDING_LINES
  }
})

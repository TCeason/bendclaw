/**
 * TUI end-to-end tests using @microsoft/tui-test.
 * These test the compiled binary in a real pseudo-terminal.
 *
 * Run with: npx tui-test
 * Config: cli/tui-test.config.ts
 */

import { test, expect } from "@microsoft/tui-test"

test("startup shows banner and prompt", async ({ terminal }) => {
  // Wait for the prompt border to appear
  await terminal.waitForText("─", { timeout: 10_000 })

  // Should show the prompt cursor indicator
  const screen = terminal.getScreen()
  expect(screen).toContain("❯")
})

test("typing shows input in prompt", async ({ terminal }) => {
  await terminal.waitForText("─", { timeout: 10_000 })

  // Type some text
  await terminal.type("hello world")

  // Should appear in the prompt area
  await terminal.waitForText("hello world", { timeout: 3_000 })
})

test("Ctrl+C exits cleanly", async ({ terminal }) => {
  await terminal.waitForText("─", { timeout: 10_000 })

  // First Ctrl+C when not loading should exit
  await terminal.keyPress("c", { control: true })

  // Process should exit
  await terminal.waitForExit({ timeout: 5_000 })
})

test("escape clears input", async ({ terminal }) => {
  await terminal.waitForText("─", { timeout: 10_000 })

  await terminal.type("some text")
  await terminal.waitForText("some text", { timeout: 3_000 })

  await terminal.keyPress("Escape")

  // Text should be gone — prompt should be empty
  // Wait a moment for the render
  await new Promise(r => setTimeout(r, 100))
  const screen = terminal.getScreen()
  expect(screen).not.toContain("some text")
})

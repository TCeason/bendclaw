import { describe, expect, test } from 'bun:test'
import { buildAssistantLines } from '../src/render/output.js'
import { buildOutputBlocks } from '../src/term/viewmodel/output.js'
import { blocksToLines } from '../src/term/viewmodel/types.js'
import { updateLiveHeight } from '../src/term/viewmodel/live-height.js'

function renderedHeight(markdown: string): number {
  return blocksToLines(
    buildOutputBlocks(buildAssistantLines(markdown), { columns: 40 }),
  ).length
}

describe('streaming markdown footer stability', () => {
  test('real markdown reparses may shrink but guarded footer height never does', () => {
    const samples = [
      '1. first\n2. second\n3. third',
      'Code:\n\n```ts\nconst x = 1\n```\n\nDone.',
      'Code:\n\n~~~~ts\nconst x = 1\n~~~~\n\nDone.',
      'Heading with glued fence```ts\nconst x = 1\n```\n\nDone.',
      '| Name | Value |\n| --- | --- |\n| a | short |\n| b | a much longer value |',
    ]

    let sawParserShrink = false
    for (const sample of samples) {
      let previousRendered = 0
      let maxHeight = 0
      let previousGuarded = 0
      for (let index = 1; index <= sample.length; index++) {
        const current = renderedHeight(sample.slice(0, index))
        if (current < previousRendered) sawParserShrink = true
        previousRendered = current

        const guarded = updateLiveHeight(maxHeight, current, true)
        maxHeight = guarded.maxHeight
        const footerRow = current + guarded.padding
        expect(footerRow).toBeGreaterThanOrEqual(previousGuarded)
        previousGuarded = footerRow
      }
    }
    expect(sawParserShrink).toBe(true)
  })

  test('streaming does not hide valid numeric or fence-looking text', () => {
    expect(renderedHeight('Version\n2026')).toBeGreaterThan(renderedHeight('Version\n'))
    expect(renderedHeight('Code:\n\n```ts\nconst x = 1\n`')).toBeGreaterThan(0)
  })
})

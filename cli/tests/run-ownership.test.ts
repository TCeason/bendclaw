import { describe, expect, test } from 'bun:test'
import { RunOwnership } from '../src/term/app/run-ownership.js'
import { RunInteraction } from '../src/term/app/run-interaction.js'
import { decideReplControl } from '../src/term/app/repl-control.js'
import { createEditorState } from '../src/term/input/editor.js'
import { TerminalInputBuffer } from '../src/term/input/buffer.js'
import type { SpinnerPhase } from '../src/term/spinner.js'

describe('RunOwnership', () => {
  test('Esc owns setup and retains confirmation across streaming/retry phases', () => {
    const ownership = new RunOwnership()
    const generation = ownership.begin()
    const controller = new RunInteraction()
    const keyboard = new TerminalInputBuffer()
    const phases: SpinnerPhase[] = ['thinking', 'retrying', 'quota_waiting', 'outage_waiting', 'executing']
    for (const phase of phases) {
      controller.clear()
      const input = { active: true, owner: ownership.owner, phase }
      keyboard.write('\x1b')
      const [event] = keyboard.flushPending()
      if (!event) throw new Error('Escape was not decoded')
      expect(decideReplControl({
        event, interaction: controller.snapshot(input), overlay: { kind: 'none' },
        isLoading: true, hasStream: false, editor: createEditorState(),
        exitHint: false, logMode: false, hasQueuedPrompt: false,
      })).toEqual([{ kind: 'interrupt' }])
      expect(controller.requestInterrupt(input)).toBe('confirm')
      // The native query resolves between presses. Its stream identity must
      // not replace the operation identity or reset confirmation.
      expect(controller.requestInterrupt({ ...input, phase: 'responding' })).toBe('interrupt')
    }
    ownership.revoke()
    expect(ownership.owns(generation)).toBe(false)
    // A late query result must be aborted, never installed into a newer run.
    const replacement = ownership.begin()
    expect(ownership.owner).toBe(replacement)
    expect(ownership.owns(generation)).toBe(false)
  })

  test('revokes an interrupted run before a replacement starts', () => {
    const ownership = new RunOwnership()
    const interrupted = ownership.begin()

    ownership.revoke()
    const replacement = ownership.begin()

    expect(ownership.owns(interrupted)).toBe(false)
    expect(ownership.owns(replacement)).toBe(true)
  })

  test('starting a new run prevents an older finally block from owning state', () => {
    const ownership = new RunOwnership()
    const first = ownership.begin()
    const second = ownership.begin()

    expect(ownership.owns(first)).toBe(false)
    expect(ownership.owns(second)).toBe(true)
  })
})

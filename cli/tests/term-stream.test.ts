import { describe, expect, test } from 'bun:test'
import { createSpinnerState } from '../src/term/spinner.js'
import { createInitialState } from '../src/term/app/state.js'
import { createStreamMachineState, reduceRunEvent, flushStreaming, buildToolStartedLines, buildToolFinishedLines, buildToolProgressLines } from '../src/term/app/stream.js'
import type { OutputLine } from '../src/render/output.js'

describe('term stream machine', () => {
  test('assistant delta accumulates text without committing', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello\n\nworld' },
    }, { termRows: 24 })

    state = update.state
    // New architecture: no mid-stream commits, text accumulates
    expect(update.commitLines.length).toBe(0)
    expect(state.streamingText).toBe('hello\n\nworld')
    expect(state.pendingText).toBe('hello\n\nworld')
  })

  test('assistant delta without complete block does not commit', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello world' },
    }, { termRows: 24 })

    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.streamingText).toBe('hello world')
  })

  test('assistant_completed flushes remaining text', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello world' },
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.streamingText).toBe('hello world')

    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBeGreaterThan(0)
    expect(state.streamingText).toBe('')
    expect(state.pendingText).toBe('')
  })

  test('no duplicate commit: all text flushed once at assistant_completed', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: string[] = []

    for (const delta of ['Hello ', 'world.\n\n', 'Second paragraph.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      for (const line of update.commitLines) allCommitted.push(line.text)
    }

    // No mid-stream commits in new architecture
    expect(allCommitted.length).toBe(0)

    const update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    for (const line of update.commitLines) allCommitted.push(line.text)

    const fullText = allCommitted.join('\n')
    expect(fullText).toContain('Hello world')
    expect(fullText).toContain('Second paragraph')
    expect((fullText.match(/Hello world/g) || []).length).toBe(1)
    expect((fullText.match(/Second paragraph/g) || []).length).toBe(1)

    const final = flushStreaming(state)
    expect(final.lines.length).toBe(0)
  })

  test('pendingText tracks streamingText for viewport rendering', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const text = Array.from({ length: 9 }, (_, i) => `plain line ${i}`).join('\n')

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: text },
    }, { termRows: 18 })

    state = update.state
    // All text stays in streamingText, pendingText mirrors it
    expect(state.streamingText).toBe(text)
    expect(state.pendingText).toBe(text)
    expect(update.commitLines.length).toBe(0)
  })

  test('flushStreaming after tool_started produces clean assistant block', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    // Simulate a tool_started which flushes text
    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'Before tool.' },
    }, { termRows: 18 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: {} },
    }, { termRows: 18 })
    state = update.state
    // tool_started flushes "Before tool." into commitLines
    const beforeLines = update.commitLines.filter(l => l.kind === 'assistant')
    expect(beforeLines.length).toBeGreaterThan(0)
    expect(beforeLines.some(l => l.text.includes('Before tool'))).toBe(true)

    // Now add more text after tool
    update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'After tool.' },
    }, { termRows: 18 })
    state = update.state

    // Flush produces a clean block (no continuation spacer needed —
    // the tool call visually separates the two assistant blocks)
    const flushed = flushStreaming(state)
    expect(flushed.lines.length).toBeGreaterThan(0)
    expect(flushed.lines[0]?.kind).toBe('assistant')
    expect(flushed.lines[0]?.text).toContain('After tool')
  })

  test('line-by-line fallback keeps open math blocks pending', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const text = 'Intro\n\n$$\n' + Array.from({ length: 12 }, (_, i) => `x_${i} = ${i}`).join('\n')

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: text },
    }, { termRows: 18 })

    state = update.state
    // No mid-stream commits
    expect(update.commitLines.length).toBe(0)
    expect(state.streamingText).toBe(text)
  })

  test('verbose mode: no duplicate commits across llm_call_completed and assistant_completed', () => {
    const appState = { ...createInitialState('model', '/tmp'), verbose: true }
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: OutputLine[] = []

    // 1. llm_call_started
    let update = reduceRunEvent(state, {
      kind: 'llm_call_started',
      payload: { model: 'test', messages: [] },
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 2. assistant deltas
    for (const delta of ['Hello ', 'world.']) {
      update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      allCommitted.push(...update.commitLines)
    }

    // 3. llm_call_completed
    update = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: { model: 'test', input_tokens: 10, output_tokens: 5 },
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 4. assistant_completed
    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 5. run_finished
    update = reduceRunEvent(state, {
      kind: 'run_finished',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 6. Final flush in repl loop
    const final = flushStreaming(state)
    allCommitted.push(...final.lines)

    // Count how many times assistant text appears
    const assistantLines = allCommitted.filter(l => l.kind === 'assistant')
    const assistantText = assistantLines.map(l => l.text).join('\n')
    const helloCount = (assistantText.match(/Hello world/g) || []).length
    expect(helloCount).toBe(1)
  })

  test('tool_started flushes text once, no duplicate with assistant_completed', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: OutputLine[] = []

    // 1. assistant deltas before tool
    for (const delta of ['Before ', 'tool.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      allCommitted.push(...update.commitLines)
    }

    // 2. tool_started — should flush "Before tool."
    let update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: {} },
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // Verify text was flushed
    const afterTool = allCommitted.filter(l => l.kind === 'assistant')
    expect(afterTool.length).toBeGreaterThan(0)

    // 3. tool_finished
    update = reduceRunEvent(state, {
      kind: 'tool_finished',
      payload: { tool_name: 'bash', args: {}, is_error: false, content: 'ok' },
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 4. More deltas after tool
    for (const delta of ['After ', 'tool.']) {
      update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      allCommitted.push(...update.commitLines)
    }

    // 5. assistant_completed — should flush "After tool."
    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    allCommitted.push(...update.commitLines)

    // 6. Final flush
    const final = flushStreaming(state)
    allCommitted.push(...final.lines)

    // "Before tool." appears exactly once
    const allAssistant = allCommitted.filter(l => l.kind === 'assistant').map(l => l.text).join('\n')
    expect((allAssistant.match(/Before tool/g) || []).length).toBe(1)
    // "After tool." appears exactly once
    expect((allAssistant.match(/After tool/g) || []).length).toBe(1)
  })

  test('tool progress updates state', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: { text: 'running' },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('running')
    expect(update.rerenderStatus).toBe(true)
  })

  test('spill progress commits visible event line', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: {
        tool_name: 'bash',
        text: '__evot_spill_event__ {"kind":"write","path":"/tmp/spill.txt","size_bytes":120000,"preview_bytes":4000}',
      },
    }, { termRows: 24 })
    const text = update.commitLines.map(l => l.text).join('\n')
    expect(text).toContain('[SPILL] \u21aa 117.2 KB written \u00b7 3.9 KB preview \u00b7 bash')
    expect(text).toContain('/tmp/spill.txt')
    expect(update.state.toolProgress).toBe('')
  })

  test('tool progress builder renders spill marker as event', () => {
    const lines = buildToolProgressLines({
      kind: 'tool_progress',
      payload: {
        tool_name: 'read_file',
        text: '__evot_spill_event__ {"kind":"read","path":"/tmp/tool-results/spill.txt","size_bytes":2048,"duration_ms":12}',
      },
    } as any, true)
    const text = lines.map(l => l.text).join('\n')
    expect(text).toContain('[SPILL] \u21a9 2.0 KB read \u00b7 12ms \u00b7 read_file')
    expect(text).toContain('/tmp/tool-results/spill.txt')
    expect(text).not.toContain('__evot_spill_event__')
  })

  test('tool started suppresses ask_user', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'ask_user', args: {} },
    }, { termRows: 24 })
    expect(update.suppressToolStarted).toBe(true)
  })

  test('heartbeat progress does not replace cached output', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState(appState, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: { text: 'Running... 60s' },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('line 1\nline 2')
    expect(update.rerenderStatus).toBe(true)
  })

  test('tool started clears stale progress cache', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState(appState, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: {} },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('')
  })

  test('tool finished clears stale progress cache', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState(appState, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'tool_finished',
      payload: { tool_name: 'bash', args: {}, content: 'ok' },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('')
  })

  test('turn started clears stale progress cache', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState(appState, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'turn_started',
      payload: {},
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('')
  })

  test('llm call started clears stale progress cache', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState({ ...appState, verbose: true }, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'llm_call_started',
      payload: { model: 'model' },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('')
  })

  test('context compaction started clears stale progress cache', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState({ ...appState, verbose: true }, spinner),
      toolProgress: 'line 1\nline 2',
      lastToolProgress: 'line 1\nline 2',
    }
    const update = reduceRunEvent(state, {
      kind: 'context_compaction_started',
      payload: { estimated_tokens: 10, context_window: 100 },
    }, { termRows: 24 })
    expect(update.state.toolProgress).toBe('')
    expect(update.state.lastToolProgress).toBe('')
  })

  test('llm retry emits visible backoff line in verbose mode', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState({ ...appState, verbose: true }, spinner)
    const update = reduceRunEvent(state, {
      kind: 'llm_call_retry',
      payload: {
        attempt: 1,
        max_retries: 3,
        retry_delay_ms: 1200,
        error: 'network error',
      },
    }, { termRows: 24 })
    const text = update.commitLines.map(l => l.text).join('\n')
    expect(text).toContain('[LLM] \u21bb \u00b7 retrying in 1 second \u00b7 attempt 1/3')
    expect(text).toContain('network error')
  })

  test('verbose off: llm events route to writeLines, not commitLines', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState({ ...appState, verbose: false }, spinner)

    const started = reduceRunEvent(state, {
      kind: 'llm_call_started',
      payload: { model: 'test', messages: [] },
    }, { termRows: 24 })
    state = started.state
    const startedCommit = started.commitLines.map(l => l.text).join('\n')
    const startedWrite = started.writeLines.map(l => l.text).join('\n')
    expect(startedCommit).not.toContain('[LLM]')
    expect(startedWrite).toContain('[LLM] \u25cf \u00b7 test')

    const completed = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: { model: 'test', usage: { input: 10, output: 5, cache_read: 0, cache_write: 0 }, metrics: { duration_ms: 1000, ttfb_ms: 400, streaming_ms: 600 } },
    }, { termRows: 24 })
    state = completed.state
    const completedCommit = completed.commitLines.map(l => l.text).join('\n')
    const completedWrite = completed.writeLines.map(l => l.text).join('\n')
    expect(completedCommit).not.toContain('[LLM]')
    expect(completedWrite).toContain('[LLM] \u2713')
  })

  test('verbose off: llm retry still surfaces in commitLines', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState({ ...appState, verbose: false }, spinner)
    const update = reduceRunEvent(state, {
      kind: 'llm_call_retry',
      payload: { attempt: 1, max_retries: 3, retry_delay_ms: 500, error: 'rate limited' },
    }, { termRows: 24 })
    const text = update.commitLines.map(l => l.text).join('\n')
    expect(text).toContain('[LLM] \u21bb')
    expect(text).toContain('rate limited')
  })

  test('verbose off: run_summary is visible in commitLines', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState({ ...appState, verbose: false }, spinner)
    const update = reduceRunEvent(state, {
      kind: 'run_finished',
      payload: {},
    }, { termRows: 24 })
    const hasSummary = update.commitLines.some(l => l.kind === 'run_summary')
    expect(hasSummary).toBe(true)
  })

  test('flushStreaming emits pending assistant text', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = {
      ...createStreamMachineState(appState, spinner),
      streamingText: 'pending text',
      pendingText: 'pending text',
    }
    const flushed = flushStreaming(state)
    expect(flushed.lines.length).toBeGreaterThan(0)
    expect(flushed.state.streamingText).toBe('')
  })

  test('build tool finish lines uses goal details for complete task progress', () => {
    const finished = buildToolFinishedLines({
      kind: 'tool_finished',
      payload: {
        tool_name: 'update_goal_tasks',
        args: {},
        is_error: false,
        content: '\u2713 \u00b7 1/3 completed \u00b7 current #2 Simplify coordinator',
        details: {
          goal: {
            tasks: [
              { id: 1, title: 'Audit current code', status: 'completed', started_at: '2026-05-17T10:00:00Z', completed_at: '2026-05-17T10:02:30Z' },
              { id: 2, title: 'Simplify coordinator', status: 'in_progress' },
              { id: 3, title: 'Add tests', status: 'pending' },
            ],
          },
        },
      },
    })
    const text = finished.map(l => l.text).join('\n')
    expect(text).toContain('[GOAL] \u2611 \u00b7 1/3 completed')
    expect(text).toContain('  \u2611 #1 Audit current code \u00b7 done in 150.0s')
    expect(text).toContain('  \u25b7 #2 Simplify coordinator')
    expect(text).toContain('  \u00b7 #3 Add tests')
  })

  test('build tool start/finish lines', () => {
    const started = buildToolStartedLines({
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: { command: 'ls' } },
    })
    expect(started.length).toBeGreaterThan(0)

    const finished = buildToolFinishedLines({
      kind: 'tool_finished',
      payload: { tool_name: 'bash', args: { command: 'ls' }, is_error: false, content: 'ok', duration_ms: 10 },
    })
    expect(finished.length).toBeGreaterThan(0)
  })
})

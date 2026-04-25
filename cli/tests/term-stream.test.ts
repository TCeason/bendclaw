import { describe, expect, test } from 'bun:test'
import { createSpinnerState } from '../src/term/spinner.js'
import { createInitialState } from '../src/term/app/state.js'
import { createStreamMachineState, reduceRunEvent, flushStreaming, buildToolStartedLines, buildToolFinishedLines } from '../src/term/app/stream.js'

describe('term stream machine', () => {
  test('assistant delta commits completed markdown blocks', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    // A delta with a complete paragraph (double newline) should commit
    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello\n\nworld' },
    }, { termRows: 24 })

    state = update.state
    // "hello\n\n" is a completed block, "world" remains as pending
    expect(update.commitLines.length).toBeGreaterThan(0)
    expect(state.streamingText).toBe('world')
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

    // Simulate delta with no complete block
    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello world' },
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.streamingText).toBe('hello world')

    // Simulate assistant_completed — should flush remaining
    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBeGreaterThan(0)
    expect(state.streamingText).toBe('')
    expect(state.pendingText).toBe('')
  })

  test('no duplicate commit: completed blocks + final flush', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: string[] = []

    // Feed deltas with a complete block in the middle
    for (const delta of ['Hello ', 'world.\n\n', 'Second paragraph.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      for (const line of update.commitLines) allCommitted.push(line.text)
    }

    // "Hello world.\n\n" should have been committed
    expect(allCommitted.length).toBeGreaterThan(0)

    // assistant_completed flushes "Second paragraph."
    const update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    for (const line of update.commitLines) allCommitted.push(line.text)

    const fullText = allCommitted.join('\n')
    expect(fullText).toContain('Hello world')
    expect(fullText).toContain('Second paragraph')

    // Each appears exactly once
    expect((fullText.match(/Hello world/g) || []).length).toBe(1)
    expect((fullText.match(/Second paragraph/g) || []).length).toBe(1)

    // Final flush should be empty
    const final = flushStreaming(state)
    expect(final.lines.length).toBe(0)
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

  test('tool started keeps last progress visible until next progress update', () => {
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
    expect(update.state.lastToolProgress).toBe('line 1\nline 2')
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

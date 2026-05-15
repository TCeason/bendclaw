import { describe, expect, test } from 'bun:test'
import chalk from 'chalk'
import stringWidth from 'string-width'
import { createSpinnerState } from '../src/term/spinner.js'
import { createInitialState } from '../src/term/app/state.js'
import { createStreamMachineState, reduceRunEvent, flushStreaming, buildToolStartedLines, buildToolFinishedLines, buildToolProgressLines } from '../src/term/app/stream.js'
import type { OutputLine } from '../src/render/output.js'

describe('term stream machine', () => {
  test('assistant delta commits completed markdown blocks', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    // A delta with a complete paragraph (double newline) should commit.
    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: 'hello\n\nworld' },
    }, { termRows: 24 })

    state = update.state
    // "hello\n\n" is a completed block, "world" remains as pending.
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

  test('no duplicate commit: completed blocks + final flush', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: string[] = []

    // Feed deltas with a complete block in the middle — "Hello world.\n\n"
    // commits mid-stream, "Second paragraph." stays pending until flush.
    for (const delta of ['Hello ', 'world.\n\n', 'Second paragraph.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      for (const line of update.commitLines) allCommitted.push(line.text)
    }

    expect(allCommitted.length).toBeGreaterThan(0)

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

  test('streaming tree is committed as one block so comments align globally', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: string[] = []
    const deltas = [
      '⏺ /Users/bohu/github/evotai/evot\n',
      '  ├── Cargo.toml     # Rust workspace root (engine/app/addon)\n',
      '  ├── Cargo.lock\n',
      '  │\n',
      '  ├── src/      # Rust 核心代码\n',
      '  │   ├── engine/              # evotengine — agent 运行时\n',
      '\n要点：\n',
    ]

    for (const delta of deltas) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { delta },
      }, { termRows: 24 })
      state = update.state
      allCommitted.push(...update.commitLines.map(line => line.text))
    }

    const commentLines = allCommitted.filter(line => line.includes('#'))
    expect(commentLines.length).toBe(3)
    expect(new Set(commentLines.map(line => stringWidth(line))).size).toBe(1)
    expect(state.streamingText).toBe('要点：\n')
  })

  test('assistant delta naturally commits long plain text leading lines', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const text = Array.from({ length: 9 }, (_, i) => `plain line ${i}`).join('\n')

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: text },
    }, { termRows: 18 })

    state = update.state
    const committed = update.commitLines.map(line => line.text).join('\n')
    expect(committed).toContain('plain line 0')
    expect(committed).toContain('plain line 6')
    expect(committed).not.toContain('plain line 7')
    expect(state.streamingText).toBe('plain line 7\nplain line 8')
    expect(state.assistantCommitted).toBe(true)
  })

  test('flushStreaming marks pending tail as continuation after prior assistant commit', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const text = Array.from({ length: 9 }, (_, i) => `plain line ${i}`).join('\n')

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { delta: text },
    }, { termRows: 18 })

    state = update.state
    expect(state.assistantCommitted).toBe(true)

    const flushed = flushStreaming(state)
    expect(flushed.lines[0]?.isContinuationSpacer).toBe(true)
    expect(flushed.lines[1]?.text).toContain('plain line 7')
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
    expect(text).toContain('[SPILL] ↪ 117.2 KB written · 3.9 KB preview · bash')
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
    expect(text).toContain('[SPILL] ↩ 2.0 KB read · 12ms · read_file')
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
    expect(text).toContain('[LLM] ↻ · retrying in 1 second · attempt 1/3')
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
    expect(startedWrite).toContain('[LLM] ● · test')

    const completed = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: { model: 'test', usage: { input: 10, output: 5, cache_read: 0, cache_write: 0 }, metrics: { duration_ms: 1000, ttfb_ms: 400, streaming_ms: 600 } },
    }, { termRows: 24 })
    state = completed.state
    const completedCommit = completed.commitLines.map(l => l.text).join('\n')
    const completedWrite = completed.writeLines.map(l => l.text).join('\n')
    expect(completedCommit).not.toContain('[LLM]')
    expect(completedWrite).toContain('[LLM] ✓')
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
    expect(text).toContain('[LLM] ↻')
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

// ---------------------------------------------------------------------------
// streaming code fence: lines committed line by line
// ---------------------------------------------------------------------------

function reduceDelta(prev: ReturnType<typeof createStreamMachineState>, delta: string, termRows = 30) {
  const event = {
    kind: 'assistant_delta' as const,
    payload: { delta },
    metadata: null as any,
  }
  return reduceRunEvent(prev, event, { termRows })
}

test('commits code-fenced lines incrementally, not just at close', () => {
  let prev = createStreamMachineState(createInitialState(), createSpinnerState('responding'))
  // Simulate a single delta containing prose → code fence → code body → close → prose
  const result1 = reduceDelta(prev, 'Here is some code:\n\n```js\nconst a = 1\nconst b = 2\n```\n\nDone.\n')

  // Should have at least two code_line entries for the two body lines
  const codeLines = result1.commitLines.filter(l => l.kind === 'code_line')
  expect(codeLines.length).toBeGreaterThanOrEqual(2)
  expect(codeLines.find(l => l.text.includes('const a = 1'))).toBeTruthy()
  expect(codeLines.find(l => l.text.includes('const b = 2'))).toBeTruthy()
  expect(new Set(codeLines.map(l => l.codeBlockId)).size).toBe(1)
  expect(codeLines.every(l => l.codeLanguage === 'js')).toBe(true)

  // Should also have assistant lines for the prose before and after
  const proseLines = result1.commitLines.filter(l => l.kind === 'assistant' && l.text !== '')
  expect(proseLines.length).toBeGreaterThanOrEqual(1)
})

test('streams JSON fenced lines with syntax highlighting', () => {
  const prevLevel = chalk.level
  chalk.level = 3
  try {
    const prev = createStreamMachineState(createInitialState(), createSpinnerState('responding'))
    const result = reduceDelta(prev, '```json\n{\n  "name": "evot",\n  "enabled": true\n')
    const codeLines = result.commitLines.filter(l => l.kind === 'code_line' && l.text)

    expect(codeLines.length).toBeGreaterThanOrEqual(3)
    expect(codeLines.some(l => l.text.includes('\x1b['))).toBe(true)
    expect(codeLines.some(l => l.text.includes('"name"'))).toBe(true)
    expect(codeLines.every(l => l.codeLanguage === 'json')).toBe(true)
    expect(new Set(codeLines.map(l => l.codeBlockId)).size).toBe(1)
  } finally {
    chalk.level = prevLevel
  }
})

test('flushStreaming does not re-commit an open fence as a second-time assistant block', () => {
  let prev = createStreamMachineState(createInitialState(), createSpinnerState('responding'))
  // Incomplete fence — stream ends while the fence is still open
  const result1 = reduceDelta(prev, 'Here is code:\n\n```js\nconst a = 1\nconst b = 2\n')
  prev = result1.state

  const bodyCodeLines = result1.commitLines.filter(l => l.kind === 'code_line' && l.text)
  expect(bodyCodeLines.length).toBeGreaterThanOrEqual(2)

  // flushStreaming should NOT re-commit the fence body as assistant lines
  const flushed = flushStreaming(prev)
  const asstLines = flushed.lines.filter(l => l.kind === 'assistant' && l.text)
  expect(asstLines.find(l => l.text.includes('const a = 1'))).toBeUndefined()
  expect(asstLines.find(l => l.text.includes('const b = 2'))).toBeUndefined()
})

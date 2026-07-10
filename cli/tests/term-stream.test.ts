import { describe, expect, test } from 'bun:test'
import { createSpinnerState } from '../src/term/spinner.js'
import { createInitialState } from '../src/term/app/state.js'
import { createStreamMachineState, reduceRunEvent, flushStreaming } from '../src/term/app/stream.js'
import { buildToolCard } from '../src/render/output.js'
import { assistantToolCalls, findAssistantToolCall } from '../src/term/app/assistant-content.js'
import type { OutputLine } from '../src/render/output.js'

describe('term stream machine', () => {
  test('assistant delta keeps the whole message in the dynamic zone (no mid-stream commit)', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'hello\n\nworld' },
    }, { termRows: 24 })

    state = update.state
    // Plan A: the message streams in place. A completed paragraph is NOT drained
    // to scrollback mid-stream (that caused the dynamic zone to empty/refill and
    // the spinner below to jump). Everything stays in the pending text.
    expect(update.commitLines.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('hello\n\nworld')
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('hello\n\nworld')
  })

  test('assistant delta without complete block does not commit', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'hello world' },
    }, { termRows: 24 })

    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('hello world')
  })

  test('assistant_completed flushes remaining text', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'hello world' },
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('hello world')

    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines.length).toBeGreaterThan(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('')
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('')
  })

  test('assistant_completed with length stop appends a truncation notice', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'a partial answer that got cut off' },
    }, { termRows: 24 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: { stop_reason: 'length' },
    }, { termRows: 24 })
    state = update.state

    const committed = update.commitLines.map(l => l.text).join('\n')
    expect(committed).toContain('a partial answer that got cut off')
    expect(committed).toContain('maximum output token limit')
    expect(update.commitLines.some(l => l.kind === 'error')).toBe(true)
  })

  test('assistant_completed with normal stop appends no truncation notice', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'a complete answer' },
    }, { termRows: 24 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: { stop_reason: 'stop' },
    }, { termRows: 24 })

    const committed = update.commitLines.map(l => l.text).join('\n')
    expect(committed).toContain('a complete answer')
    expect(committed).not.toContain('maximum output token limit')
  })

  test('no mid-stream commit: whole message flushed once at assistant_completed', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: string[] = []

    for (const delta of ['Hello ', 'world.\n\n', 'Second paragraph.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { content_index: 0, content_type: 'text', delta },
      }, { termRows: 24 })
      state = update.state
      for (const line of update.commitLines) allCommitted.push(line.text)
    }

    // Plan A: nothing commits mid-stream; the full message stays pending.
    expect(allCommitted.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe('Hello world.\n\nSecond paragraph.')

    const update = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {},
    }, { termRows: 24 })
    state = update.state
    for (const line of update.commitLines) allCommitted.push(line.text)

    const fullText = allCommitted.join('\n')
    expect(fullText).toContain('Hello world')
    expect(fullText).toContain('Second paragraph')
    // Each block appears exactly once — flushed only at the turn boundary.
    expect((fullText.match(/Hello world/g) || []).length).toBe(1)
    expect((fullText.match(/Second paragraph/g) || []).length).toBe(1)

    const final = flushStreaming(state)
    expect(final.lines.length).toBe(0)
  })

  test('pendingText mirrors the whole streaming message', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    // Short multi-paragraph reply that fits the viewport: stays fully pending.
    const text = 'Para one.\n\nPara two.\n\nPara three.'

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: text },
    }, { termRows: 24 })

    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe(text)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe(text)
  })

  test('streaming a multi-paragraph reply never commits or empties the dynamic zone mid-stream', () => {
    // Regression for streaming jank: the whole message must stream in place so
    // the dynamic zone never drains-and-refills at paragraph boundaries (which
    // made the spinner/prompt below jump up and back down on every \n\n).
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const full = [
      'First paragraph explaining the setup.',
      '## Section',
      'Second paragraph with **bold** and some detail that runs on a while.',
      '- point one\n- point two',
      'Final wrap-up sentence.',
    ].join('\n\n')

    const deltas: string[] = []
    for (let i = 0; i < full.length; i += 5) deltas.push(full.slice(i, i + 5))

    let midStreamCommits = 0
    let emptyContentFrames = 0
    let prevContentLen = 0
    for (const d of deltas) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { content_index: 0, content_type: 'text', delta: d },
      }, { termRows: 40 })
      state = update.state
      midStreamCommits += update.commitLines.filter(l => l.kind === 'assistant' && l.text).length
      if (state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('').length === 0 && prevContentLen > 0) emptyContentFrames++
      prevContentLen = state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('').length
    }

    expect(midStreamCommits).toBe(0)
    expect(emptyContentFrames).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe(full)

    // Everything flushes once at the turn boundary.
    const done = reduceRunEvent(state, { kind: 'assistant_completed', payload: {} }, { termRows: 40 })
    const flushed = done.commitLines.filter(l => l.kind === 'assistant').map(l => l.text).join('\n')
    expect(flushed).toContain('First paragraph')
    expect(flushed).toContain('Final wrap-up')
  })

  test('tool_started keeps the partial assistant message stable', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    // Simulate a tool_started which flushes text
    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'Before tool.' },
    }, { termRows: 18 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: {} },
    }, { termRows: 18 })
    state = update.state
    // Execution updates the tool block in place; it must not move assistant
    // content into scrollback while the partial message is still live.
    expect(update.commitLines).toHaveLength(0)
    expect(state.appState.currentAssistantContent[0]).toMatchObject({
      type: 'text',
      text: 'Before tool.',
    })

    // Now add more text after tool
    update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 1, content_type: 'text', delta: 'After tool.' },
    }, { termRows: 18 })
    state = update.state

    // Flush produces a clean block (no continuation spacer needed —
    // the tool call visually separates the two assistant blocks)
    const flushed = flushStreaming(state)
    expect(flushed.lines.length).toBeGreaterThan(0)
    expect(flushed.lines.map(line => line.text).join('\n')).toContain('Before tool')
    expect(flushed.lines.map(line => line.text).join('\n')).toContain('After tool')
  })

  test('long open code fence stays in the partial message without scrollback migration', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    // 15 lines at termRows 18 exceeds the overflow threshold (max(8, 18-6)=12),
    // so the safety valve drains leading blocks to keep the tail on screen.
    const text = 'Intro\n\n```\n' + Array.from({ length: 12 }, (_, i) => `x_${i} = ${i}`).join('\n')

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: text },
    }, { termRows: 18 })

    state = update.state
    // The whole block remains in one dynamic AssistantMessage, matching pi.
    expect(update.commitLines).toHaveLength(0)
    const partial = state.appState.currentAssistantContent
      .filter(block => block.type === 'text')
      .map(block => block.text)
      .join('')
    expect(partial).toContain('Intro')
    expect(partial).toContain('x_11 = 11')
  })

  test('overflow drain never tears a table mid-stream (rendered whole at end)', () => {
    // Regression: a table taller than the viewport, preceded by non-pipe lines
    // (a numbered list) that used to reset the old pipe-table guard's counter.
    // With no last-resort split, the table has no internal blank line, so no
    // safe commit boundary exists inside it — it stays fully pending and renders
    // as one whole marked parse (box-drawn), never split into a committed head
    // and an orphan tail that lost its header/separator (which showed as raw
    // `| ... |` rows on screen).
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    const msg =
      '分析：\n\n1. 第一点\n2. 第二点\n3. 第三点\n\n' +
      '| 类别 | 总量 | 训练 | 覆盖 |\n|------|------|------|------|\n' +
      Array.from({ length: 10 }, (_, i) => `| item_${i} | ${i} | ${i} 步 | ok |`).join('\n')

    // Feed char-by-char through the real reducer at a small viewport (termRows
    // 10 → threshold max(8, 10-6)=8) so the overflow valve is exercised.
    const committed: OutputLine[] = []
    for (const ch of msg) {
      const update = reduceRunEvent(state, { kind: 'assistant_delta', payload: { content_index: 0, content_type: 'text', delta: ch } }, { termRows: 10 })
      state = update.state
      committed.push(...update.commitLines)
    }
    const flush = flushStreaming(state)
    committed.push(...flush.lines)

    const assistant = committed.filter(l => l.kind === 'assistant').map(l => l.text)
    // No committed assistant line is a raw pipe row (the torn-table signature).
    const rawPipeRows = assistant.filter(l => /^\s*\|.*\|\s*$/.test(l))
    expect(rawPipeRows).toEqual([])
    // The table rendered as a box-drawn grid instead.
    const boxLines = assistant.filter(l => /[┌│├└]/.test(l))
    expect(boxLines.length).toBeGreaterThan(0)
    // Every data row survived inside the rendered table.
    const joined = assistant.join('\n')
    for (let i = 0; i < 10; i++) expect(joined).toContain(`item_${i}`)
  })

  test('short message with an open code fence stays fully pending (no overflow)', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    // Well under the overflow threshold: the whole thing streams in place.
    const text = 'Intro\n\n```\nx_0 = 0\nx_1 = 1'

    const update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: text },
    }, { termRows: 24 })

    state = update.state
    expect(update.commitLines.length).toBe(0)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe(text)
    expect(state.appState.currentAssistantContent.filter(block => block.type === 'text').map(block => block.text).join('')).toBe(text)
  })

  test('assistant_completed authoritative snapshot is committed once', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    state = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'partial' },
    }, { termRows: 24 }).state

    const completed = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: { content: [{ type: 'text', text: 'authoritative' }] },
    }, { termRows: 24 })

    expect(completed.commitLines.filter(line => line.kind === 'assistant').map(line => line.text).join('\n'))
      .toContain('authoritative')
    expect(completed.commitLines.map(line => line.text).join('\n')).not.toContain('partial')
    expect(completed.state.appState.currentAssistantContent).toEqual([])
    expect(completed.state.appState.messages.at(-1)?.content).toEqual([
      { type: 'text', contentIndex: 0, text: 'authoritative' },
    ])
    expect(flushStreaming(completed.state).lines).toHaveLength(0)
  })

  test('turn_started preserves partial content as a fallback when completion is missing', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    state = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'unfinished' },
    }, { termRows: 24 }).state

    const nextTurn = reduceRunEvent(state, {
      kind: 'turn_started',
      payload: {},
    }, { termRows: 24 })

    expect(nextTurn.commitLines.filter(line => line.kind === 'assistant').map(line => line.text).join('\n'))
      .toContain('unfinished')
    expect(nextTurn.state.appState.currentAssistantContent).toEqual([])
  })

  test('llm_call_completed does not flush a tool-bearing ordered message', () => {
    const appState = createInitialState('model', '/tmp')
    appState.currentAssistantContent = [
      { type: 'thinking', contentIndex: 0, text: 'plan' },
      {
        type: 'tool_call',
        contentIndex: 1,
        toolCall: { id: 'call-1', name: 'read', args: {}, status: 'running' },
      },
      { type: 'text', contentIndex: 2, text: 'after' },
    ]
    const state = createStreamMachineState(appState, createSpinnerState())

    const completed = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: {},
    }, { termRows: 24 })

    expect(completed.commitLines.filter(line => ['thinking', 'assistant', 'tool'].includes(line.kind)))
      .toHaveLength(0)
    expect(completed.state.appState.currentAssistantContent.map(block => block.type))
      .toEqual(['thinking', 'tool_call', 'text'])
  })

  test('no duplicate commits across llm_call_completed and assistant_completed', () => {
    const appState = createInitialState('model', '/tmp')
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
        payload: { content_index: 0, content_type: 'text', delta },
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

  test('tool execution does not commit or duplicate the partial assistant message', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const allCommitted: OutputLine[] = []

    // 1. assistant deltas before tool
    for (const delta of ['Before ', 'tool.']) {
      const update = reduceRunEvent(state, {
        kind: 'assistant_delta',
        payload: { content_index: 0, content_type: 'text', delta },
      }, { termRows: 24 })
      state = update.state
      allCommitted.push(...update.commitLines)
    }

    // 2. tool_started keeps the partial message in the dynamic zone.
    let update = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: { tool_name: 'bash', args: {} },
    }, { termRows: 24 })
    state = update.state
    expect(update.commitLines).toHaveLength(0)

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
        payload: { content_index: 1, content_type: 'text', delta },
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

  test('conflicting delta type cannot replace an existing content index', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    state = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'visible text' },
    }, { termRows: 24 }).state
    state = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'thinking', delta: 'misclassified' },
    }, { termRows: 24 }).state

    expect(state.appState.currentAssistantContent).toEqual([
      { type: 'text', contentIndex: 0, text: 'visible text' },
    ])
  })

  test('thinking after text remains a distinct ordered content block', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: '每条都停在 `' },
    }, { termRows: 24 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 1, content_type: 'thinking', delta: '` 里的推理中途:\n- 第 1 题' },
    }, { termRows: 24 })
    state = update.state

    expect(state.appState.currentAssistantContent.map(block => block.type)).toEqual(['text', 'thinking'])
    const flushed = flushStreaming(state)
    const visible = flushed.lines.map(line => line.text).join('\n')
    expect(visible).toContain('每条都停在')
    expect(visible).toContain('里的推理中途')
  })

  test('thinking before visible text commits as markdown thinking content', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    let update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'thinking', delta: 'internal reasoning\nline 2' },
    }, { termRows: 24 })
    state = update.state

    update = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 1, content_type: 'text', delta: 'final answer' },
    }, { termRows: 24 })
    state = update.state

    expect(update.commitLines).toHaveLength(0)
    expect(state.appState.currentAssistantContent.map(block => block.type)).toEqual(['thinking', 'text'])
    const flushed = flushStreaming(state)
    expect(flushed.lines.filter(l => l.kind === 'thinking').map(l => l.text)).toEqual([
      'internal reasoning',
      'line 2',
    ])
    expect(flushed.lines.some(l => l.kind === 'assistant')).toBe(true)
  })

  test('tool progress updates its matching live card', () => {
    const appState = createInitialState('model', '/tmp')
    appState.currentAssistantContent = [{
      type: 'tool_call',
      contentIndex: 0,
      toolCall: {
        id: 'call-bash',
        name: 'bash',
        args: { command: 'sleep 1' },
        status: 'running',
        startedAt: Date.now(),
      },
    }]
    const state = createStreamMachineState(appState, createSpinnerState())
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: { tool_call_id: 'call-bash', tool_name: 'bash', text: 'line 1\nline 2' },
    }, { termRows: 24 })

    const progressBlock = update.state.appState.currentAssistantContent[0]
    expect(progressBlock?.type === 'tool_call' ? progressBlock.toolCall.progress : undefined).toBe('line 1\nline 2')
    expect(update.rerenderStatus).toBe(true)
  })

  test('spill progress commits visible event line', () => {
    const state = createStreamMachineState(createInitialState('model', '/tmp'), createSpinnerState())
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: {
        tool_call_id: 'call-bash',
        tool_name: 'bash',
        text: '__evot_spill_event__ {"kind":"write","path":"/tmp/spill.txt","size_bytes":120000,"preview_bytes":4000}',
      },
    }, { termRows: 24 })
    const text = update.commitLines.map(l => l.text).join('\n')
    expect(text).toContain('\u21aa 117.2 KB written \u00b7 3.9 KB preview \u00b7 bash')
    expect(text).toContain('/tmp/spill.txt')
  })

  test('heartbeat progress preserves the card output', () => {
    const appState = createInitialState('model', '/tmp')
    appState.currentAssistantContent = [{
      type: 'tool_call',
      contentIndex: 0,
      toolCall: {
        id: 'call-bash',
        name: 'bash',
        args: {},
        status: 'running',
        progress: 'line 1\nline 2',
      },
    }]
    const state = createStreamMachineState(appState, createSpinnerState())
    const update = reduceRunEvent(state, {
      kind: 'tool_progress',
      payload: { tool_call_id: 'call-bash', tool_name: 'bash', text: 'Running... 60s' },
    }, { termRows: 24 })

    const heartbeatBlock = update.state.appState.currentAssistantContent[0]
    expect(heartbeatBlock?.type === 'tool_call' ? heartbeatBlock.toolCall.progress : undefined).toBe('line 1\nline 2')
  })

  test('queued tool has no running state; execution uses the animated footer', () => {
    const queued = buildToolCard({ id: 'call-1', name: 'read', args: { path: 'src/a.rs' }, status: 'queued' })
    const queuedText = queued.map(line => line.text).join('\n')
    expect(queuedText).toContain('read')
    expect(queuedText).not.toContain('running')

    const running = buildToolCard({
      id: 'call-1',
      name: 'read',
      args: { path: 'src/a.rs' },
      status: 'running',
      startedAt: 1_000,
      progress: 'partial output',
    }, false, 2_500)
    const runningText = running.map(line => line.text).join('\n')
    expect(runningText).toContain('partial output')
    expect(runningText).not.toContain('● running')

    const completed = buildToolCard({
      id: 'call-1',
      name: 'read',
      args: { path: 'src/a.rs' },
      status: 'done',
      result: 'done',
      durationMs: 12,
    })
    expect(completed.map(line => line.text).join('\n')).toContain('✓ · 12ms')
  })

  test('llm retry renders as a visible card with backoff and error', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
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
    expect(text).toContain('✦ llm  retry')
    expect(text).toContain('\u21bb \u00b7 retrying in 1 second \u00b7 attempt 1/3')
    expect(text).toContain('network error')
  })

  test('llm stats route to writeLines (screen.log only), not commitLines', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

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

  test('llm_call_completed sets footer context tokens from real usage', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    // Pre-call estimate lands first via llm_call_started.
    state = reduceRunEvent(state, {
      kind: 'llm_call_started',
      payload: { model: 'test', messages: [], estimated_context_tokens: 5000, context_window: 160000 },
    }, { termRows: 24 }).state
    expect(state.appState.currentRunStats.contextTokens).toBe(5000)

    // On completion the footer must switch to the provider's real usage,
    // matching the compaction trigger: input + cache_read + cache_write + output.
    const completed = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: {
        model: 'test',
        usage: { input: 100000, output: 2000, cache_read: 8000, cache_write: 1000 },
        metrics: { duration_ms: 1000 },
      },
    }, { termRows: 24 })
    expect(completed.state.appState.currentRunStats.contextTokens).toBe(111000)
    expect(completed.state.appState.sessionTokens.contextTokens).toBe(111000)
  })

  test('llm_call_completed without usage keeps prior context tokens', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    state = reduceRunEvent(state, {
      kind: 'llm_call_started',
      payload: { model: 'test', messages: [], estimated_context_tokens: 7000, context_window: 160000 },
    }, { termRows: 24 }).state

    const completed = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: { model: 'test', metrics: { duration_ms: 1000 } },
    }, { termRows: 24 })
    expect(completed.state.appState.currentRunStats.contextTokens).toBe(7000)
  })

  test('llm retry surfaces in commitLines as a card', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const update = reduceRunEvent(state, {
      kind: 'llm_call_retry',
      payload: { attempt: 1, max_retries: 3, retry_delay_ms: 500, error: 'rate limited' },
    }, { termRows: 24 })
    const text = update.commitLines.map(l => l.text).join('\n')
    expect(text).toContain('✦ llm  retry')
    expect(text).toContain('rate limited')
  })

  test('llm error card and following error event do not duplicate the message', () => {
    const msg = 'API error: HTTP 520: error code: 520'
    let state = createStreamMachineState(createInitialState('claude-opus-4-6', '/tmp'), createSpinnerState())
    const u1 = reduceRunEvent(state, {
      kind: 'llm_call_completed',
      payload: { model: 'claude-opus-4-6', turn: 5, error: msg, metrics: { duration_ms: 43800 } },
    }, { termRows: 24 })
    state = u1.state
    const u2 = reduceRunEvent(state, { kind: 'error', payload: { message: msg } }, { termRows: 24 })
    const tui = [...u1.commitLines, ...u2.commitLines].map(l => l.text).join('\n')
    // Message shows exactly once in the TUI (the llm card), and the redundant
    // standalone error line is routed to screen.log instead.
    expect((tui.match(/HTTP 520: error code: 520/g) ?? []).length).toBe(1)
    expect(tui).toContain('✦ llm  claude-opus-4-6')
    expect(u2.writeLines.some(l => l.text.includes('HTTP 520'))).toBe(true)
  })

  test('run_finished preserves partial assistant content on abnormal termination', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    state = reduceRunEvent(state, {
      kind: 'assistant_delta',
      payload: { content_index: 0, content_type: 'text', delta: 'last partial line' },
    }, { termRows: 24 }).state

    const finished = reduceRunEvent(state, {
      kind: 'run_finished',
      payload: {},
    }, { termRows: 24 })

    expect(finished.commitLines.filter(line => line.kind === 'assistant').map(line => line.text).join('\n'))
      .toContain('last partial line')
    expect(finished.state.appState.currentAssistantContent).toEqual([])
  })

  test('run_finished emits no run summary', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const update = reduceRunEvent(state, {
      kind: 'run_finished',
      payload: {},
    }, { termRows: 24 })
    // The end-of-run summary block was removed; run_finished only flushes any
    // pending assistant text and never appends a summary.
    expect(update.commitLines.some(l => (l.kind as string) === 'run_summary')).toBe(false)
  })

  test('flushStreaming emits ordered assistant content', () => {
    const appState = createInitialState('model', '/tmp')
    appState.currentAssistantContent = [{ type: 'text', contentIndex: 0, text: 'pending text' }]
    const spinner = createSpinnerState()
    const state = createStreamMachineState(appState, spinner)
    const flushed = flushStreaming(state)
    expect(flushed.lines.length).toBeGreaterThan(0)
    expect(flushed.state.appState.currentAssistantContent).toEqual([])
  })

  test('streams parallel tool calls independently before execution', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    state = reduceRunEvent(state, {
      kind: 'assistant_tool_call',
      payload: {
        content_index: 0,
        tool_call_id: 'call-read',
        tool_name: 'read',
        phase: 'start',
      },
    }, { termRows: 24 }).state
    for (const delta of ['{"path":"src/', 'a.rs"}']) {
      state = reduceRunEvent(state, {
        kind: 'assistant_tool_call',
        payload: {
          content_index: 0,
          tool_call_id: 'call-read',
          tool_name: 'read',
          phase: 'delta',
          delta,
        },
      }, { termRows: 24 }).state
    }
    state = reduceRunEvent(state, {
      kind: 'assistant_tool_call',
      payload: {
        content_index: 1,
        tool_call_id: 'call-edit',
        tool_name: 'edit',
        phase: 'end',
        args: { path: 'src/b.rs', edits: [] },
      },
    }, { termRows: 24 }).state

    const calls = assistantToolCalls(state.appState.currentAssistantContent)
    expect(calls).toHaveLength(2)
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-read')?.args).toEqual({ path: 'src/a.rs' })
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')?.name).toBe('edit')

    state = reduceRunEvent(state, {
      kind: 'assistant_completed',
      payload: {
        content: [
          { type: 'tool_call', id: 'call-read', name: 'read', input: { path: 'src/a.rs' } },
          { type: 'tool_call', id: 'call-edit', name: 'edit', input: { path: 'src/b.rs', edits: [] } },
        ],
      },
    }, { termRows: 24 }).state
    expect(assistantToolCalls(state.appState.currentAssistantContent)).toHaveLength(2)
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-read')?.argsComplete).toBe(true)
    const assistantMessage = state.appState.messages[state.appState.messages.length - 1]
    const callIds = assistantMessage?.content
      ?.filter(block => block.type === 'tool_call')
      .map(block => block.type === 'tool_call' ? block.toolCall.id : '')
    expect(callIds).toEqual(['call-read', 'call-edit'])

    state = reduceRunEvent(state, {
      kind: 'tool_started',
      payload: {
        tool_call_id: 'call-edit',
        tool_name: 'edit',
        args: { path: 'src/b.rs', edits: [] },
      },
    }, { termRows: 24 }).state

    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')?.status).toBe('running')
    expect(state.spinnerState.phase).toBe('executing')
    expect(state.spinnerState.toolName).toBe('edit')
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')?.startedAt).toBeNumber()
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-read')?.startedAt).toBeUndefined()
  })

  test('large streamed tool args stay as raw fragments and finalize once', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)
    const chunk = 'x'.repeat(16 * 1024)
    const raw = JSON.stringify({ path: 'a', oldText: chunk, newText: chunk })

    state = reduceRunEvent(state, {
      kind: 'assistant_tool_call',
      payload: { content_index: 0, tool_call_id: 'call-edit', tool_name: 'edit', phase: 'start' },
    }, { termRows: 24 }).state
    for (let offset = 0; offset < raw.length; offset += 128) {
      state = reduceRunEvent(state, {
        kind: 'assistant_tool_call',
        payload: {
          content_index: 0,
          tool_call_id: 'call-edit',
          tool_name: 'edit',
          phase: 'delta',
          delta: raw.slice(offset, offset + 128),
        },
      }, { termRows: 24 }).state
    }

    const streaming = findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')
    expect(streaming?.partialArgs?.length).toBe(raw.length)
    expect(streaming?.args.oldText).toBe(chunk)

    state = reduceRunEvent(state, {
      kind: 'assistant_tool_call',
      payload: {
        content_index: 0,
        tool_call_id: 'call-edit',
        tool_name: 'edit',
        phase: 'end',
        args: { path: 'a', oldText: chunk, newText: chunk },
      },
    }, { termRows: 24 }).state

    const finalized = findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')
    expect(finalized?.partialArgs).toBeUndefined()
    expect(finalized?.argsComplete).toBe(true)
  })

  test('last tool completion flushes the ordered assistant message once', () => {
    const appState = createInitialState('model', '/tmp')
    appState.currentAssistantContent = [
      { type: 'thinking', contentIndex: 0, text: 'plan' },
      {
        type: 'tool_call',
        contentIndex: 1,
        toolCall: { id: 'call-1', name: 'read', args: {}, status: 'running' },
      },
      { type: 'text', contentIndex: 2, text: 'answer' },
    ]
    const state = createStreamMachineState(appState, createSpinnerState())

    const finished = reduceRunEvent(state, {
      kind: 'tool_finished',
      payload: { tool_call_id: 'call-1', tool_name: 'read', content: 'ok', is_error: false },
    }, { termRows: 24 })

    const visible = finished.commitLines.map(line => line.text).join('\n')
    expect(visible.indexOf('plan')).toBeLessThan(visible.indexOf('read'))
    expect(visible.indexOf('read')).toBeLessThan(visible.indexOf('answer'))
    expect(finished.state.appState.currentAssistantContent).toEqual([])
    expect(flushStreaming(finished.state).lines).toHaveLength(0)
  })

  test('live cards preserve model order while tools finish out of order', () => {
    const appState = createInitialState('model', '/tmp')
    const spinner = createSpinnerState()
    let state = createStreamMachineState(appState, spinner)

    for (const [contentIndex, id, name] of [[0, 'call-read', 'read'], [1, 'call-edit', 'edit']] as const) {
      state = reduceRunEvent(state, {
        kind: 'assistant_tool_call',
        payload: { content_index: contentIndex, tool_call_id: id, tool_name: name, phase: 'start' },
      }, { termRows: 24 }).state
      state = reduceRunEvent(state, {
        kind: 'tool_started',
        payload: { tool_call_id: id, tool_name: name, args: {} },
      }, { termRows: 24 }).state
    }

    state = reduceRunEvent(state, {
      kind: 'tool_finished',
      payload: { tool_call_id: 'call-edit', tool_name: 'edit', content: 'edited', is_error: false },
    }, { termRows: 24 }).state

    expect(assistantToolCalls(state.appState.currentAssistantContent).map(call => call.id)).toEqual(['call-read', 'call-edit'])
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-read')?.status).toBe('running')
    expect(findAssistantToolCall(state.appState.currentAssistantContent, 'call-edit')?.status).toBe('done')
  })
})

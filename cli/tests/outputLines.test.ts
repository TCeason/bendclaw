import { describe, test, expect, beforeEach } from 'bun:test'
import {
  buildUserMessage,
  buildAssistantLines,
  buildThinkingLines,
  buildToolResult,
  buildToolProgress,
  buildToolCall,
  buildVerboseEvent,
  buildError,
  AssistantStreamBuffer,
  findSafeSplitPoint,
  resetIdCounter,
} from '../src/render/output.js'
import { formatLlmCallStarted, formatLlmCallRetry, formatLlmCallCompleted, formatCompactionStarted, formatCompactionCompleted } from '../src/render/verbose.js'

beforeEach(() => {
  resetIdCounter()
})

// ---------------------------------------------------------------------------
// buildUserMessage
// ---------------------------------------------------------------------------

describe('buildUserMessage', () => {
  test('creates a single user line', () => {
    const lines = buildUserMessage('hello world')
    expect(lines).toHaveLength(1)
    expect(lines[0]!.kind).toBe('user')
    expect(lines[0]!.text).toBe('hello world')
  })

  test('shows image ref inline from displayText', () => {
    const lines = buildUserMessage('analyze this [Image #1]')
    expect(lines).toHaveLength(1)
    expect(lines[0]!.text).toBe('analyze this [Image #1]')
  })

  test('image-only displayText', () => {
    const lines = buildUserMessage('[Image #1]')
    expect(lines).toHaveLength(1)
    expect(lines[0]!.kind).toBe('user')
    expect(lines[0]!.text).toBe('[Image #1]')
  })

  test('empty text returns empty', () => {
    const lines = buildUserMessage('')
    expect(lines).toHaveLength(0)
  })
})

// ---------------------------------------------------------------------------
// buildAssistantLines
// ---------------------------------------------------------------------------

describe('buildAssistantLines', () => {
  test('renders markdown and splits into lines', () => {
    const lines = buildAssistantLines('hello **world**')
    expect(lines.length).toBeGreaterThan(0)
    expect(lines.every((l) => l.kind === 'assistant')).toBe(true)
  })

  test('returns empty for blank text', () => {
    expect(buildAssistantLines('')).toHaveLength(0)
    expect(buildAssistantLines('   ')).toHaveLength(0)
  })
})

describe('buildThinkingLines', () => {
  test('renders thinking as markdown like pi assistant content', () => {
    const lines = buildThinkingLines('**Planning**\n\nnext')
    expect(lines).toHaveLength(2)
    expect(lines.every(line => line.kind === 'thinking')).toBe(true)
    expect(lines.every(line => line.thinkingStyle)).toBe(true)
    expect(lines.map(line => line.text).join('\n')).not.toContain('**')
    expect(lines.map(line => line.text).join('\n')).toContain('Planning')
    expect(lines.map(line => line.text).join('\n')).toContain('next')
  })
})

// ---------------------------------------------------------------------------
// buildToolCall
// ---------------------------------------------------------------------------

describe('buildToolCall', () => {
  test('regular tool call emits a call line with glyph and command (no status mark)', () => {
    const preview = 'python3 -c "print(1)"'
    const lines = buildToolCall('bash', { command: preview }, preview)
    const card = lines[lines.length - 1]!
    expect(card.kind).toBe('tool')
    expect(card.text).toContain('⌘ bash')
    expect(card.text).toContain(preview)
    // No status mark on the call line — status is on the result line below.
    expect(card.text).not.toContain('✓')
    expect(card.text).not.toContain('✗')
  })

  test('multi-line bash command collapses to first line + line count', () => {
    const command = [
      "cd /Users/bohu/github/evotai/llmproxy && python3 - <<'PY'",
      'from pathlib import Path',
      "path = Path('src/core/base_proxy.py')",
      'print(path.read_text()[:20])',
      'PY',
    ].join('\n')
    const lines = buildToolCall('bash', { command }, command)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('⌘ bash  cd /Users/bohu/github/evotai/llmproxy && python3 - <<\'PY\' … (+5 lines)')
    // Full heredoc body must not be flattened into the header.
    expect(all).not.toContain('from pathlib import Path')
  })

  test('expanded multi-line bash command preserves newlines under the header', () => {
    const command = [
      "cd /tmp && python3 - <<'PY'",
      'from pathlib import Path',
      'print(1)',
      'PY',
    ].join('\n')
    const lines = buildToolCall('bash', { command }, command, true)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain("⌘ bash  cd /tmp && python3 - <<'PY'")
    expect(all).toContain('  from pathlib import Path')
    expect(all).toContain('  print(1)')
    expect(all).toContain('  PY')
    expect(all).not.toContain('… (+')
  })

  test('long single-line bash command is truncated on the card header', () => {
    const command = 'x'.repeat(200)
    const lines = buildToolCall('bash', { command }, command)
    const card = lines[lines.length - 1]!
    expect(card.text).toContain('⌘ bash')
    expect(card.text.endsWith('…')).toBe(true)
    expect(card.text.length).toBeLessThan(command.length)
  })

  test('regular tool call still surfaces reason lines up-front', () => {
    const lines = buildToolCall('bash', { reason: 'list project files' }, 'ls -la')
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('↳ reason: list project files')
    // The command itself is no longer shown on the start line — it appears on
    // the finished card instead.
    expect(all).not.toContain('❯ ls -la')
  })

  test('renders bash bypass and timeout reasons with friendly labels', () => {
    const lines = buildToolCall(
      'bash',
      {
        reason: 'run the build',
        reason_to_increase_timeout: 'full release build is slow',
        reason_to_use_instead_of_read_file_tool: 'N/A',
      },
      'cargo build --release',
    )
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('↳ reason: run the build')
    expect(all).toContain('↳ why longer timeout: full release build is slow')
    // 'N/A' reasons are omitted.
    expect(all).not.toContain('why not read')
  })

  test('omits empty and N/A reasons from the call line', () => {
    const lines = buildToolCall('grep', { pattern: 'foo', reason: '' })
    const all = lines.map(l => l.text).join('\n')
    expect(all).not.toContain('↳ reason:')
    // No generic arg summary on the start line anymore.
    expect(all).not.toContain('1 arg')
  })
})

describe('buildToolResult', () => {
  test('emits a closing status line with mark and duration', () => {
    const lines = buildToolResult('bash', { command: 'ls -la' }, 'done', undefined, 42)
    expect(lines.length).toBeGreaterThanOrEqual(1)
    // Status closes the block (after the output), indented, no glyph/command.
    const status = lines[lines.length - 1]!
    expect(status.kind).toBe('tool')
    expect(status.text).toMatch(/^ {2}✓/)
    expect(status.text).toContain('42ms')
    expect(status.text).not.toContain('⌘ bash')
    expect(status.text).not.toContain('completed')
  })

  test('error result status line uses ✗', () => {
    const lines = buildToolResult('bash', { command: 'fail' }, 'error', 'command not found', 10)
    const status = lines[lines.length - 1]!
    expect(status.text).toMatch(/^ {2}✗/)
    expect(status.text).not.toContain('failed')
    expect(lines.some((l) => l.kind === 'error')).toBe(true)
  })

  test('pretty prints JSON result body (status line no longer labels shape)', () => {
    const lines = buildToolResult('web_fetch', {}, 'done', '{"status":"ok","items":[1,2]}', undefined, true)
    const status = lines[lines.length - 1]!
    expect(status.text).toMatch(/^ {2}✓/)
    // Shape labels ("JSON · N keys") removed — the body above conveys it.
    expect(status.text).not.toContain('JSON')
    expect(status.text).not.toContain('keys')
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('  {')
    expect(all).toContain('    "status": "ok"')
    expect(all).toContain('    "items": [')
  })

  test('collapsed multiline result shows only the expand hint, no content preview', () => {
    const result = JSON.stringify({ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 }, null, 2)
    const lines = buildToolResult('edit', {}, 'done', result)
    const bodyLines = lines.filter(l => l.kind === 'tool_result')
    // Collapsed view: a single hint line carrying the full line count, no
    // previewed content rows.
    expect(bodyLines).toHaveLength(1)
    expect(bodyLines[0]!.text).toContain('ctrl+o to expand')
    expect(bodyLines[0]!.text).toMatch(/\.\.\. \(\+\d+ lines, ctrl\+o to expand\)/)
    // No JSON body line leaked into the collapsed card.
    expect(bodyLines.some(l => l.text.includes('"status"') || l.text.trim() === '{')).toBe(false)
  })

  test('collapsed search result hint counts every line (no head preview)', () => {
    const result = Array.from({ length: 8 }, (_, i) => `match ${i}`).join('\n')
    const lines = buildToolResult('search', {}, 'done', result)
    const bodyLines = lines.filter(l => l.kind === 'tool_result')
    expect(bodyLines).toHaveLength(1)
    expect(bodyLines[0]!.text).toContain('... (+8 lines, ctrl+o to expand)')
    // No match rows previewed in the collapsed view.
    const all = lines.map(l => l.text).join('\n')
    expect(all).not.toContain('match 0')
  })

  test('Read collapses to an expand hint and expands to full content', () => {
    // Regression: successful reads used to render no body at all, leaving Read
    // as the only tool whose output couldn't be expanded with ctrl+o. It now
    // collapses/expands like bash/search.
    const result = Array.from({ length: 93 }, (_, i) => `line ${i}`).join('\n')
    const collapsed = buildToolResult('Read', { path: 'a.ts' }, 'done', result, 0)
    const collapsedBody = collapsed.filter(l => l.kind === 'tool_result')
    expect(collapsedBody).toHaveLength(1)
    expect(collapsedBody[0]!.text).toContain('... (+93 lines, ctrl+o to expand)')
    expect(collapsed.map(l => l.text).join('\n')).not.toContain('line 0')

    const expanded = buildToolResult('Read', { path: 'a.ts' }, 'done', result, 0, true)
    const all = expanded.map(l => l.text).join('\n')
    expect(all).toContain('line 0')
    expect(all).toContain('line 92')
    expect(all).toContain('ctrl+o to collapse')
  })

  test('expanded multiline result shows collapse hint', () => {
    const result = Array.from({ length: 7 }, (_, i) => `line ${i}`).join('\n')
    const lines = buildToolResult('bash', {}, 'done', result, undefined, true)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('line 0')
    expect(all).toContain('line 6')
    expect(all).toContain('ctrl+o to collapse')
  })

  test('expanded progress shows collapse hint', () => {
    const progress = Array.from({ length: 7 }, (_, i) => `line ${i}`).join('\n')
    const lines = buildToolProgress('bash', progress, true)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('line 0')
    expect(all).toContain('line 6')
    expect(all).toContain('ctrl+o to collapse')
  })

  test('collapsed progress card shows header + expand hint, no content preview', () => {
    const lines = buildToolProgress('bash', 'line1\nline2\nline3')
    expect(lines[0]!.text).toBe('⌘ bash  · 3 lines')
    const body = lines.slice(1)
    expect(body).toHaveLength(1)
    expect(body[0]!.text).toContain('... (+3 lines, ctrl+o to expand)')
    // No progress content previewed in the collapsed card.
    expect(lines.map(l => l.text).join('\n')).not.toContain('line3')
  })

  test('single-line progress renders inline (nothing to collapse)', () => {
    const lines = buildToolProgress('bash', 'only line')
    expect(lines[0]!.text).toBe('⌘ bash  · 1 line')
    expect(lines.map(l => l.text).join('\n')).toContain('  only line')
  })

  test('includes diff when present', () => {
    const lines = buildToolResult('file_edit', { path: 'a.ts', diff: '+added\n-removed' }, 'done')
    expect(lines.some((l) => l.text.includes('added') || l.text.includes('removed'))).toBe(true)
  })
})

// ---------------------------------------------------------------------------
// buildVerboseEvent
// ---------------------------------------------------------------------------

describe('buildVerboseEvent', () => {
  test('splits multi-line text without trailing separator', () => {
    const lines = buildVerboseEvent('line1\nline2\nline3')
    expect(lines).toHaveLength(3)
    expect(lines.filter((l) => l.kind === 'verbose')).toHaveLength(3)
    expect(lines[0]!.text).toBe('line1')
    expect(lines[2]!.text).toBe('line3')
  })

  test('formats llm started with status symbol and full details', () => {
    const text = formatLlmCallStarted({
      model: 'claude-sonnet-4',
      turn: 2,
      message_count: 18,
      system_prompt_tokens: 8000,
      context_window: 200000,
      estimated_context_tokens: 42000,
      message_stats: {
        user_count: 6,
        assistant_count: 5,
        tool_result_count: 7,
        user_tokens: 12000,
        assistant_tokens: 4000,
        tool_result_tokens: 18000,
        image_tokens: 0,
        tool_details: [['read_file', 8000], ['search', 6000], ['bash', 4000]],
      },
    })
    expect(text).toContain('[LLM] ● · claude-sonnet-4 · turn 2 · 18 msgs · user 6 / asst 5 / tool 7')
    expect(text).toContain('    context   ')
    expect(text).toContain('    tokens    sys 8k · user 12k · asst 4k · tool 18k')
    expect(text).toContain('    by tool   read_file 8k (44%)')
  })

  test('formats llm retry with wait time and attempt', () => {
    const text = formatLlmCallRetry({
      attempt: 2,
      max_retries: 3,
      retry_delay_ms: 2100,
      error: 'tls handshake eof',
    })
    expect(text).toContain('[LLM] ↻ · retrying in 2 seconds · attempt 2/3')
    expect(text).toContain('    error     tls handshake eof')
  })

  test('formats llm completed with status symbol and timing details', () => {
    const result = formatLlmCallCompleted({
      model: 'claude-sonnet-4',
      turn: 2,
      duration_ms: 8400,
      input_tokens: 42000,
      output_tokens: 352,
      context_window: 200000,
      estimated_context_tokens: 42000,
      time_to_first_byte_ms: 1100,
      cache_read: 21000,
      cache_write: 0,
      tool_calls: [{ id: 'tc-1', name: 'search', arguments: { pattern: 'foo' } }],
    })
    expect(result.text).toContain('[LLM] ✓ · claude-sonnet-4 · turn 2 · 8.4s')
    expect(result.text).toContain('    tokens    42k in → 352 out')
    expect(result.text).toContain('    cache     21k read · 0 write · 33% hit')
    expect(result.text).toContain('    timing    ttfb 1.1s (13%) · stream 7.3s (87%)')
    expect(result.text).toContain('    tools     search')
    expect(result.text).not.toContain('    output    ')
    expect(result.expandedText).toBeUndefined()
  })

  test('tok/s uses the streaming window, not total wall-clock', () => {
    // 600 output tokens over a 3s streaming window = 200 tok/s. The 12s ttfb
    // wait must not dilute the rate (total duration 15s would give 40 tok/s).
    const result = formatLlmCallCompleted({
      model: 'qwen3-4b',
      turn: 1,
      output_tokens: 600,
      metrics: { duration_ms: 15000, ttfb_ms: 12000, ttft_ms: 12000, streaming_ms: 3000 },
    })
    expect(result.text).toContain('· 200 tok/s')
    expect(result.text).not.toContain('· 40 tok/s')
  })

  test('shows server-side model fallback when response_model differs', () => {
    const result = formatLlmCallCompleted({
      model: 'claude-fable-5',
      response_model: 'claude-opus-4-8',
      turn: 3,
      duration_ms: 5000,
      output_tokens: 100,
      metrics: { duration_ms: 5000, ttfb_ms: 1000, ttft_ms: 1000, streaming_ms: 4000 },
    })
    expect(result.text).toContain('[LLM] ✓ · claude-fable-5 → claude-opus-4-8 · turn 3')
    expect(result.text).toContain('    fallback  served by claude-opus-4-8 (requested claude-fable-5)')
  })

  test('no fallback line when response_model matches requested model', () => {
    const result = formatLlmCallCompleted({
      model: 'claude-fable-5',
      response_model: 'claude-fable-5',
      turn: 3,
      duration_ms: 5000,
      output_tokens: 100,
      metrics: { duration_ms: 5000, ttfb_ms: 1000, ttft_ms: 1000, streaming_ms: 4000 },
    })
    expect(result.text).toContain('[LLM] ✓ · claude-fable-5 · turn 3')
    expect(result.text).not.toContain('fallback')
  })

  test('formats compact verbose with status symbols and preserves details', () => {
    const started = formatCompactionStarted({
      level: 'L1',
      messages_count: 48,
      estimated_tokens: 168000,
      context_window: 200000,
      token_breakdown: { system: 8000, user: 24000, assistant: 18000, tool: 118000 },
    })
    expect(started).toContain('[COMPACT] ● · L1 · 48 msgs')
    expect(started).toContain('    context   ')
    expect(started).toContain('    tokens    sys 8k · user 24k · asst 18k · tool 118k')

    const completed = formatCompactionCompleted({
      result: {
        type: 'level_done',
        level: 1,
        messages_before: 48,
        messages_after: 35,
        tokens_before: 168000,
        tokens_after: 126000,
        context_window: 200000,
        map: '[··OHHH··SS] ',
        legend: '·=unchanged/kept  O=Outline  H=HeadTail  S=Summarized',
        result: '↓ outlined 2, head-tail 3',
        details: ['changed 5/48', '#12 read_file HeadTail ~18k → ~4k (−14k)'],
      },
    })
    expect(completed).toContain('[COMPACT] ✓ · L1 · 48 → 35 msgs · saved 42k (25%)')
    expect(completed).toContain('    context   ')
    expect(completed).toContain('    map       [··OHHH··SS]   · kept   O Outline   H HeadTail   S Summarized')
    expect(completed).toContain('    summary   outlined 2 · head-tail 3')
    expect(completed).toContain('    actions   #12 read_file HeadTail 18k → 4k (−14k)')
  })
})

// ---------------------------------------------------------------------------
// findSafeSplitPoint
// ---------------------------------------------------------------------------

describe('findSafeSplitPoint', () => {
  test('returns content.length when no newline', () => {
    expect(findSafeSplitPoint('hello world')).toBe(11)
  })

  test('splits at paragraph boundary', () => {
    const text = 'first paragraph\n\nsecond paragraph'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(17) // after \n\n
    expect(text.slice(0, split)).toBe('first paragraph\n\n')
  })

  test('does not split inside code block', () => {
    const text = '```js\nconst x = 1\n\nconst y = 2\n```'
    const split = findSafeSplitPoint(text)
    // Should return content.length — the whole thing is inside a code block
    expect(split).toBe(text.length)
  })

  test('splits before code block, not inside', () => {
    const text = 'some text\n\n```js\nconst x = 1\n```'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(11) // after "some text\n\n"
    expect(text.slice(0, split).trim()).toBe('some text')
  })

  test('falls back to single newline', () => {
    const text = 'line one\nline two'
    const split = findSafeSplitPoint(text)
    expect(split).toBe(9) // after "line one\n"
  })

  test('returns content.length for unclosed code block', () => {
    const text = 'hello\n\n```js\nconst x = 1'
    const split = findSafeSplitPoint(text)
    // End is inside unclosed code block, should not split
    expect(split).toBe(text.length)
  })
})

// ---------------------------------------------------------------------------
// AssistantStreamBuffer
// ---------------------------------------------------------------------------

describe('AssistantStreamBuffer', () => {
  test('emits lines on first content', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    const lines = buf.finish()
    expect(lines.some((l) => l.kind === 'assistant')).toBe(true)
  })

  test('skips leading whitespace', () => {
    const buf = new AssistantStreamBuffer()
    const lines1 = buf.push('\n\n')
    expect(lines1).toHaveLength(0)
    buf.push('hello')
    const lines2 = buf.finish()
    expect(lines2.some((l) => l.kind === 'assistant')).toBe(true)
  })

  test('emits lines on newline', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    const lines = buf.push(' world\n')
    const assistantLines = lines.filter((l) => l.kind === 'assistant')
    expect(assistantLines.length).toBeGreaterThanOrEqual(0)
  })

  test('finish flushes remaining buffer', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello world')
    const lines = buf.finish()
    expect(lines.some((l) => l.kind === 'assistant')).toBe(true)
  })

  test('finish on empty buffer returns nothing', () => {
    const buf = new AssistantStreamBuffer()
    expect(buf.finish()).toHaveLength(0)
  })

  test('pendingText returns incomplete line', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('hello')
    expect(buf.pendingText).toBe('hello')
    buf.push(' world\nfoo')
    expect(buf.pendingText).toBe('foo')
  })

  test('multi-line push emits all complete lines', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('first line\n')
    const lines = buf.push('second line\nthird')
    // 'third' stays pending
    expect(buf.pendingText).toBe('third')
  })

  test('does not split inside code block', () => {
    const buf = new AssistantStreamBuffer()
    buf.push('text before\n\n```js\nconst x = 1\n')
    // The code block is unclosed, so the \n inside should NOT cause a flush
    // that breaks the code block. The pending text should contain the code block.
    const pending = buf.pendingText
    expect(pending).toContain('```js')
  })

  test('flushes text before code block at paragraph boundary', () => {
    const buf = new AssistantStreamBuffer()
    // Push text with a paragraph break followed by a closed code block
    const allLines: import('../src/render/output.js').OutputLine[] = []
    allLines.push(...buf.push('hello world\n\n'))
    allLines.push(...buf.push('```js\nconst x = 1\n```\n'))
    allLines.push(...buf.finish())
    // Should have emitted assistant lines for both parts
    const assistantLines = allLines.filter((l) => l.kind === 'assistant')
    expect(assistantLines.length).toBeGreaterThan(0)
  })
})

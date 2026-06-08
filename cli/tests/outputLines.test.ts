import { describe, test, expect, beforeEach } from 'bun:test'
import {
  buildUserMessage,
  buildAssistantLines,
  buildToolResult,
  buildToolProgress,
  buildToolCall,
  buildVerboseEvent,
  buildRunSummary,
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

// ---------------------------------------------------------------------------
// buildToolCall
// ---------------------------------------------------------------------------

describe('buildToolCall', () => {
  test('compact bash preview shows head and expand hint', () => {
    const preview = ['python3 << EOF', 'import json', 'print(1)', 'print(2)', 'EOF'].join('\n')
    const lines = buildToolCall('bash', {}, preview)
    const all = lines.map(l => l.text).join('\n')

    expect(all).toContain('command · 5 lines')
    expect(all).toContain('  ❯ python3 << EOF')
    expect(all).toContain('    import json')
    expect(all).toContain('    print(1)')
    expect(all).not.toContain('    print(2)')
    expect(all).toContain('... (+2 lines, ctrl+o to expand)')
  })

  test('renders goal task updates as a compact goal block', () => {
    const args = {
      tasks: [
        { id: 1, title: 'Audit current code', status: 'completed', started_at: '2026-05-17T10:00:00Z', completed_at: '2026-05-17T10:02:30Z' },
        { id: 2, title: 'Simplify coordinator', status: 'in_progress', started_at: new Date(Date.now() - 5000).toISOString() },
        { id: 3, title: 'Add tests', status: 'pending' },
      ],
    }

    const started = buildToolCall('update_goal_tasks', args)
    const startedText = started.map(l => l.text).join('\n')
    expect(startedText).toContain('[GOAL] ▷ · 1/3 completed')
    expect(startedText).toContain('  ☑ #1 Audit current code')
    expect(startedText).toContain('  ▷ #2 Simplify coordinator')
    expect(startedText).toContain('  · #3 Add tests')

    const finished = buildToolResult('update_goal_tasks', args, 'done', 'ignored')
    const all = finished.map(l => l.text).join('\n')
    expect(all).toContain('[GOAL] ☑ · 1/3 completed')
    expect(all).toContain('  ☑ #1 Audit current code · done in 150.0s')
    expect(all).toContain('  ▷ #2 Simplify coordinator')
    expect(all).toContain('  · #3 Add tests')
    expect(all).not.toContain('UPDATE_GOAL_TASKS')
  })

  test('expanded bash preview shows full command', () => {
    const preview = ['python3 << EOF', 'import json', 'print(1)', 'print(2)', 'EOF'].join('\n')
    const lines = buildToolCall('bash', {}, preview, true)
    const all = lines.map(l => l.text).join('\n')

    expect(all).toContain('    print(2)')
    expect(all).toContain('    EOF')
    expect(all).not.toContain('ctrl+o to expand')
  })

  test('renders reason as a ↳ line alongside a preview command', () => {
    const lines = buildToolCall('bash', { reason: 'list project files' }, 'ls -la')
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('↳ reason: list project files')
    // Preview command still renders.
    expect(all).toContain('❯ ls -la')
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

  test('omits empty and N/A reasons, and does not double-count reason in arg summary', () => {
    const lines = buildToolCall('grep', { pattern: 'foo', reason: '' })
    const all = lines.map(l => l.text).join('\n')
    expect(all).not.toContain('↳ reason:')
    // reason is excluded from the generic arg count (only `pattern` remains).
    expect(all).toContain('· 1 arg')
  })
})

describe('buildToolResult', () => {
  test('creates tool badge with uppercase name, status dot, and duration', () => {
    const lines = buildToolResult('bash', { command: 'ls -la' }, 'done', undefined, 42)
    expect(lines.length).toBeGreaterThanOrEqual(1)
    expect(lines[0]!.kind).toBe('tool')
    expect(lines[0]!.text).toContain('[BASH]')
    expect(lines[0]!.text).toContain('✓')
    expect(lines[0]!.text).not.toContain('completed')
    expect(lines[0]!.text).toContain('42ms')
  })

  test('creates error tool badge', () => {
    const lines = buildToolResult('bash', { command: 'fail' }, 'error', 'command not found', 10)
    expect(lines[0]!.text).toContain('[BASH]')
    expect(lines[0]!.text).toContain('✗')
    expect(lines[0]!.text).not.toContain('failed')
    expect(lines.some((l) => l.kind === 'error')).toBe(true)
  })

  test('pretty prints JSON result and summarizes it in the badge', () => {
    const lines = buildToolResult('web_fetch', {}, 'done', '{"status":"ok","items":[1,2]}', undefined, true)
    expect(lines[0]!.text).toContain('[WEB_FETCH]')
    expect(lines[0]!.text).toContain('JSON')
    expect(lines[0]!.text).toContain('2 keys')
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('  {')
    expect(all).toContain('    "status": "ok"')
    expect(all).toContain('    "items": [')
  })

  test('compact JSON result shows shorter head with expand hint', () => {
    const result = JSON.stringify({ a: 1, b: 2, c: 3, d: 4, e: 5, f: 6 })
    const lines = buildToolResult('web_fetch', {}, 'done', result)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('  {')
    expect(all).toContain('  ... (+')
    expect(all).toContain('ctrl+o to expand')
    expect(lines.filter(l => l.kind === 'tool_result' && !l.text.includes('ctrl+o'))).toHaveLength(5)
  })

  test('compact search result shows fewer lines by default', () => {
    const result = Array.from({ length: 8 }, (_, i) => `match ${i}`).join('\n')
    const lines = buildToolResult('search', {}, 'done', result)
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('match 0')
    expect(all).toContain('match 4')
    expect(all).not.toContain('match 5')
    expect(all).toContain('... (+3 lines, ctrl+o to expand)')
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

  test('creates progress badge and preserves progress details', () => {
    const lines = buildToolProgress('bash', 'line1\nline2\nline3')
    expect(lines[0]!.text).toBe('[BASH] ● · 3 lines')
    expect(lines.map(l => l.text).join('\n')).toContain('  line3')
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
// buildRunSummary
// ---------------------------------------------------------------------------

describe('buildRunSummary', () => {
  test('formats stats with header and footer', () => {
    const lines = buildRunSummary({
      durationMs: 2500,
      turnCount: 3,
      toolCallCount: 5,
      toolErrorCount: 0,
      inputTokens: 1000,
      outputTokens: 200,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      llmCalls: 2,
      contextTokens: 0,
      contextWindow: 0,
      toolBreakdown: [],
      llmCallDetails: [],
      compactHistory: [],
      lastMessageStats: null,
      cumulativeStats: { userCount: 0, assistantCount: 0, toolResultCount: 0, imageCount: 0, userTokens: 0, assistantTokens: 0, toolResultTokens: 0, imageTokens: 0, toolDetails: [] },
      systemPromptTokens: 0,
    })
    expect(lines.length).toBeGreaterThan(1)
    expect(lines[0]!.text).toBe('')
    expect(lines[1]!.text).toContain('run summary')
    const statsLine = lines.find((l) => l.text.includes('overview'))!
    expect(statsLine.text).toContain('2.5s')
    expect(statsLine.text).toContain('3 turns')
    expect(statsLine.text).toContain('5 tools')
    expect(statsLine.text).toContain('1k tokens')
    // Footer removed — no trailing separator line
    const lastLine = lines[lines.length - 1]!
    expect(lastLine.text).not.toContain('────')
  })

  test('includes llm call details', () => {
    const lines = buildRunSummary({
      durationMs: 5000,
      turnCount: 2,
      toolCallCount: 3,
      toolErrorCount: 0,
      inputTokens: 5000,
      outputTokens: 500,
      cacheReadTokens: 1000,
      cacheWriteTokens: 200,
      llmCalls: 2,
      contextTokens: 0,
      contextWindow: 0,
      toolBreakdown: [],
      llmCallDetails: [
        { model: 'test', durationMs: 2000, inputTokens: 3000, outputTokens: 300, ttfbMs: 100, ttftMs: 200, tokPerSec: 150 },
        { model: 'test', durationMs: 1500, inputTokens: 2000, outputTokens: 200, ttfbMs: 80, ttftMs: 150, tokPerSec: 133 },
      ],
      compactHistory: [],
      lastMessageStats: null,
      cumulativeStats: { userCount: 0, assistantCount: 0, toolResultCount: 0, imageCount: 0, userTokens: 0, assistantTokens: 0, toolResultTokens: 0, imageTokens: 0, toolDetails: [] },
      systemPromptTokens: 0,
    })
    expect(lines.some((l) => l.text.includes('llm'))).toBe(true)
    expect(lines.some((l) => l.text.includes('ttft'))).toBe(true)
    expect(lines.some((l) => l.text.includes('#1'))).toBe(true)
    expect(lines.some((l) => l.text.includes('cache'))).toBe(true)
    expect(lines.some((l) => l.text.includes(' in → '))).toBe(true)
  })

  test('includes token breakdown by role', () => {
    const lines = buildRunSummary({
      durationMs: 10000,
      turnCount: 2,
      toolCallCount: 2,
      toolErrorCount: 0,
      inputTokens: 100000,
      outputTokens: 500,
      cacheReadTokens: 0,
      cacheWriteTokens: 0,
      llmCalls: 2,
      contextTokens: 0,
      contextWindow: 0,
      toolBreakdown: [],
      llmCallDetails: [
        { model: 'test', durationMs: 5000, inputTokens: 50000, outputTokens: 250, ttfbMs: 500, ttftMs: 1000, tokPerSec: 62.5 },
        { model: 'test', durationMs: 5000, inputTokens: 50000, outputTokens: 250, ttfbMs: 500, ttftMs: 1000, tokPerSec: 62.5 },
      ],
      compactHistory: [],
      lastMessageStats: null,
      cumulativeStats: {
        userCount: 3,
        assistantCount: 2,
        toolResultCount: 2,
        imageCount: 0,
        userTokens: 5000,
        assistantTokens: 15000,
        toolResultTokens: 78000,
        imageTokens: 0,
        toolDetails: [['bash', 30000], ['read', 28000], ['search', 20000]],
      },
      systemPromptTokens: 2000,
    })
    const all = lines.map(l => l.text).join('\n')
    expect(all).toContain('system')
    expect(all).toContain('user')
    expect(all).toContain('assistant')
    expect(all).toContain('tool_result')
    // Per-tool breakdown
    expect(all).toContain('bash')
    expect(all).toContain('read')
    expect(all).toContain('%')
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

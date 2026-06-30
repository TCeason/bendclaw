/**
 * Tests for the distill importer — the normalization contract that lets the
 * downstream trainer consume rows with zero post-processing.
 */

import { test, expect } from 'bun:test'
import { importEvents } from '../src/distill/internal/importer.js'
import type { RunEvent } from '../src/distill/internal/runner.js'

function ev(kind: string, payload: Record<string, unknown>): RunEvent {
  return { kind, payload }
}

const base = { systemPrompt: 'SYS', userPrompt: 'do the thing', cwd: '/tmp/ws', metadata: { task_id: 't1' } }

test('renumbers tool ids to call_N and pairs results', () => {
  const events = [
    ev('assistant_completed', {
      content: [
        { type: 'text', text: 'looking' },
        { type: 'tool_call', id: 'tooluse_RANDOM123', name: 'bash', input: { command: 'ls' } },
      ],
    }),
    ev('tool_finished', { tool_call_id: 'tooluse_RANDOM123', name: 'bash', content: 'a.py\nb.py', is_error: false }),
    ev('assistant_completed', { content: [{ type: 'text', text: 'done' }] }),
  ]
  const rows = importEvents(events, base)
  expect(rows.length).toBe(1)
  const msgs = rows[0].messages
  // system + user + assistant(tool_use) + user(tool_result) + assistant(text)
  const toolUse = (msgs[2].content as any[]).find((b) => b.type === 'tool_use')
  expect(toolUse.id).toBe('call_1')
  const toolResult = (msgs[3].content as any[])[0]
  expect(toolResult.tool_use_id).toBe('call_1')
})

test('keeps unknown tools instead of applying a distill-side whitelist', () => {
  const events = [
    ev('assistant_completed', {
      content: [{ type: 'tool_call', id: 'x', name: 'web_fetch', input: { url: 'http://x', note: '/tmp/ws/a' } }],
    }),
    ev('tool_finished', { tool_call_id: 'x', name: 'web_fetch', content: 'html' }),
  ]
  const rows = importEvents(events, base)
  expect(rows.length).toBe(1)
  const toolUse = (rows[0].messages[2].content as any[])[0]
  expect(toolUse.name).toBe('web_fetch')
  expect(toolUse.input).toEqual({ url: 'http://x', note: 'a' })
})

test('relativizes file paths and drops out-of-workspace reads', () => {
  const inWs = importEvents(
    [
      ev('assistant_completed', {
        content: [{ type: 'tool_call', id: 'r', name: 'read', input: { path: '/tmp/ws/app.py' } }],
      }),
      ev('tool_finished', { tool_call_id: 'r', name: 'read', content: 'x' }),
    ],
    base,
  )
  const toolUse = (inWs[0].messages[2].content as any[]).find((b) => b.type === 'tool_use')
  expect(toolUse.input.path).toBe('app.py')

  const outWs = importEvents(
    [
      ev('assistant_completed', {
        content: [{ type: 'tool_call', id: 'r', name: 'read', input: { path: '/etc/passwd' } }],
      }),
      ev('tool_finished', { tool_call_id: 'r', name: 'read', content: 'x' }),
    ],
    base,
  )
  expect(outWs.length).toBe(0)
})

test('converts evot edit shape to {path, old, new}', () => {
  const events = [
    ev('assistant_completed', {
      content: [
        {
          type: 'tool_call',
          id: 'e',
          name: 'edit',
          input: { path: 'app.py', edits: [{ oldText: 'a', newText: 'b' }, { oldText: 'c', newText: 'd' }] },
        },
      ],
    }),
    ev('tool_finished', { tool_call_id: 'e', name: 'edit', content: 'ok' }),
  ]
  const toolUse = (importEvents(events, base)[0].messages[2].content as any[]).find((b) => b.type === 'tool_use')
  expect(toolUse.input).toEqual({ path: 'app.py', old: 'a', new: 'b' })
})

test('scrubs workspace path from bash command and tool result', () => {
  const events = [
    ev('assistant_completed', {
      content: [{ type: 'tool_call', id: 'b', name: 'bash', input: { command: 'cat /tmp/ws/x' } }],
    }),
    ev('tool_finished', { tool_call_id: 'b', name: 'bash', content: 'see /tmp/ws/x for detail' }),
  ]
  const rows = importEvents(events, base)
  const toolUse = (rows[0].messages[2].content as any[]).find((b) => b.type === 'tool_use')
  expect(toolUse.input.command).toBe('cat x')
  const toolResult = (rows[0].messages[3].content as any[])[0]
  expect(toolResult.content).toBe('see x for detail')
})

test('scrubs generic host artifacts from tool results', () => {
  const events = [
    ev('assistant_completed', {
      content: [{ type: 'tool_call', id: 'b', name: 'bash', input: { command: 'ls -la' } }],
    }),
    ev('tool_finished', {
      tool_call_id: 'b',
      name: 'bash',
      content: 'drwxr-xr-x  3 alice  staff  96 Jun 30 10:00 .\n-rw-r--r--  1 alice  staff  12 Jun 30 10:00 app.py\nsee /Users/alice/project and /private/var/folders/abc/tmp',
    }),
  ]
  const toolResult = (importEvents(events, base)[0].messages[3].content as any[])[0]
  expect(toolResult.content).toContain('owner group')
  expect(toolResult.content).not.toContain('alice  staff')
  expect(toolResult.content).toContain('<home>')
  expect(toolResult.content).toContain('<tmp>')
})

test('always prepends system + seeds user turn', () => {
  const events = [
    ev('assistant_completed', { content: [{ type: 'tool_call', id: 'b', name: 'bash', input: { command: 'ls' } }] }),
    ev('tool_finished', { tool_call_id: 'b', name: 'bash', content: 'x' }),
  ]
  const msgs = importEvents(events, base)[0].messages
  expect(msgs[0]).toEqual({ role: 'system', content: 'SYS' })
  expect(msgs[1]).toEqual({ role: 'user', content: 'do the thing' })
})

test('lowercases tool names while normalizing known tool inputs case-insensitively', () => {
  const events = [
    ev('assistant_completed', {
      content: [
        { type: 'tool_call', id: 'r', name: 'Read', input: { path: 'app.py' } },
        { type: 'tool_call', id: 'w', name: 'Write', input: { path: 'out.txt', content: 'ok' } },
        { type: 'tool_call', id: 'b', name: 'Bash', input: { command: 'pytest -q' } },
      ],
    }),
    ev('tool_finished', { tool_call_id: 'r', name: 'Read', content: 'x' }),
    ev('tool_finished', { tool_call_id: 'w', name: 'Write', content: 'ok' }),
    ev('tool_finished', { tool_call_id: 'b', name: 'Bash', content: 'ok' }),
  ]
  const rows = importEvents(events, base)
  expect(rows.length).toBe(1)
  const blocks = rows[0].messages.flatMap((m) => Array.isArray(m.content) ? m.content : []) as any[]
  expect(blocks.filter((b) => b.type === 'tool_use').map((b) => b.name)).toEqual(['read', 'write', 'bash'])
  expect(rows[0].metadata.tools_used).toEqual({ read: 1, write: 1, bash: 1 })
})

test('lowercases unknown tool names too (no whitelist)', () => {
  const events = [
    ev('assistant_completed', {
      content: [{ type: 'tool_call', id: 'g', name: 'WebFetch', input: { url: 'http://x' } }],
    }),
    ev('tool_finished', { tool_call_id: 'g', name: 'WebFetch', content: 'html' }),
  ]
  const rows = importEvents(events, base)
  expect(rows.length).toBe(1)
  const toolUse = (rows[0].messages[2].content as any[])[0]
  expect(toolUse.name).toBe('webfetch')
})

test('accepts tool_use block type from assistant_completed', () => {
  const events = [
    ev('assistant_completed', {
      content: [{ type: 'tool_use', id: 'b', name: 'Bash', input: { command: 'ls' } }],
    }),
    ev('tool_finished', { tool_call_id: 'b', name: 'Bash', content: 'x' }),
  ]
  const rows = importEvents(events, base)
  expect(rows.length).toBe(1)
  const toolUse = (rows[0].messages[2].content as any[])[0]
  expect(toolUse.name).toBe('bash')
})

test('drops trajectory with no tool calls', () => {
  const events = [ev('assistant_completed', { content: [{ type: 'text', text: 'just talking' }] })]
  expect(importEvents(events, base).length).toBe(0)
})

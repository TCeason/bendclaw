import { expect, test } from 'bun:test'
import { formatJevDecided, formatJevTask } from '../src/render/verbose.js'
import { formatLogPaths, judgeTracePath, logAnalysisPrompt } from '../src/term/repl-commands.js'

const requests = [
  { message_index: 0, text: 'hi', probability: 0.05, in_play: false },
  { message_index: 6, text: 'analyse   whether jev\nscores persist to the transcript', probability: 0.9, in_play: true },
  { message_index: 10, text: 'what triggers a prune?', probability: null, in_play: true },
]

test('the [JEV] block names the task the verdicts were judged against', () => {
  const text = formatJevDecided({
    verdicts: [{ call_id: 'c1', tool_name: 'read', arguments: '{"path":"a.rs"}', decision: 'remove', keep_call: 0.4, keep_result: 0.1, saves_tokens: 1500 }],
    context_tokens: 26_000, pending_tokens: 1500, requests: 2, elapsed_ms: 900,
    user_requests: requests,
  }, 200_000)
  const lines = text.split('\n')
  expect(lines[0]).toBe('[JEV] · 1 judged · remove 1 · truncate 0 · keep 0 · −2k pending · 2 req · 900ms')
  const task = lines.find(l => l.trim().startsWith('task'))
  expect(task).toBe('    task  [6] 90% analyse whether jev sco… · [10] what triggers a prune? · closed [0] 5% hi')
  expect(lines.at(-1)).toContain('remove   read  {"path":"a.rs"}   call  40% · result  10% · −2k')
})

test('older rounds without user requests render as before', () => {
  expect(formatJevTask([])).toBe('')
  const text = formatJevDecided({ verdicts: [], context_tokens: 100, pending_tokens: 0, requests: 1, elapsed_ms: 5 }, 0)
  expect(text.split('\n').some(l => l.includes('task'))).toBe(false)
})

test('/log lists the judge trace once it exists', () => {
  expect(judgeTracePath(null)).toBeNull()
  expect(judgeTracePath('sid', () => false)).toBeNull()
  const path = judgeTracePath('sid', () => true)
  expect(path?.endsWith('/.evotai/sessions/sid/judge-trace.jsonl')).toBe(true)
  expect(formatLogPaths('/tmp/s.screen.log', null, null, path)).toBe(`  Log: /tmp/s.screen.log\n  Judge trace: ${path}`)
})

test('/log <query> tells the analysis fork about both files', () => {
  const without = logAnalysisPrompt('/tmp/s.screen.log', null)
  expect(without).toContain('/tmp/s.screen.log')
  expect(without).not.toContain('Judge trace')
  const withTrace = logAnalysisPrompt('/tmp/s.screen.log', '/tmp/judge-trace.jsonl')
  expect(withTrace).toContain('Judge trace (JSONL')
  expect(withTrace).toContain('/tmp/judge-trace.jsonl')
  expect(withTrace).toContain('[JEV]')
  expect(withTrace).toContain('- Do not modify any files')
})

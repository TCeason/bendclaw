import { expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { providerFailurePresentation } from '../src/provider/error-presentation.js'
import { buildLlmCard } from '../src/render/output.js'
import { createSpinnerState, formatSpinnerLine, setLongWait, setRetryWait } from '../src/term/spinner.js'

test('provider failure vocabulary is reusable without terminal-specific output', () => {
  for (const [error, label] of [
    ['Network error: tls handshake eof https://private.invalid/key', 'Connection interrupted'],
    ['request timed out', 'Request timed out'],
    ['DNS lookup failed', 'Unable to resolve service address'],
    ['HTTP 529 overloaded', 'Service temporarily overloaded. Please retry.'], ['HTTP 429', 'Rate limited'],
    ['invalid API key', 'Authentication failed'], ['insufficient_quota', 'Quota unavailable'],
  ]) {
    const copy = providerFailurePresentation({ error })
    expect(copy.label).toBe(label)
    expect(copy.label).not.toContain('https:')
    expect(copy.label).not.toContain('\x1b')
  }
  expect(providerFailurePresentation({ kind: 'busy', error: 'tls' }).label).toBe('Service temporarily unavailable.')
})

test('input validation numbers are not mistaken for HTTP status codes', () => {
  const error = 'HTTP 400: invalid_request_error: Image has 256 total pixels (16x16), which is below the minimum of 512 pixels.'
  const copy = providerFailurePresentation({ error })
  expect(copy.kind).toBe('invalid-request')
  expect(copy.label).toBe('Invalid request')
  expect(copy.guidance).toContain('attachments')
  expect(copy.guidance).toContain('Retrying unchanged will not help')
  for (const detail of ['minimum of 512 pixels', 'image width 503', 'max 429 tokens', 'size 401 bytes']) {
    expect(providerFailurePresentation({ error: detail }).kind).toBe('unknown')
  }
  for (const message of ['HTTP 503 unavailable', 'HTTP/1.1 502 Bad Gateway', 'status=504', 'status_code: 500']) {
    expect(providerFailurePresentation({ error: message }).kind).toBe('busy')
  }
  expect(providerFailurePresentation({ error: 'HTTP 400: connection parameter invalid' }).kind).toBe('invalid-request')
  expect(providerFailurePresentation({ error: 'HTTP 502: invalid_request_error' }).kind).toBe('busy')
})

test('overflow semantics take precedence over generic invalid requests', () => {
  for (const error of [
    'Context overflow: HTTP 400: invalid_request_error: prompt is too long',
    'HTTP 413: context_length_exceeded',
    'HTTP 400: the request exceeds the model context window',
    'This model has a maximum context length of 128000 tokens',
  ]) {
    const copy = providerFailurePresentation({ error })
    expect(copy.kind).toBe('context-overflow')
    expect(copy.label).toBe('Context limit exceeded')
    expect(copy.guidance).toContain('Compact')
    expect(copy.guidance).not.toContain('attachments')
  }
  expect(providerFailurePresentation({ kind: 'context-overflow', error: 'HTTP 400' }).kind).toBe('context-overflow')
})

test('model errors use protocol semantics, not gateway-specific sentences', () => {
  for (const error of ['Model not found: HTTP 404', 'HTTP 404: model_not_found', 'not_found_error: Model not found.']) {
    expect(providerFailurePresentation({ error }).kind).toBe('model-not-found')
    expect(providerFailurePresentation({ error }).label).toBe('Model not found')
  }
  expect(providerFailurePresentation({ error: 'HTTP 404: route not found' }).label).toBe('Resource not found')
  expect(providerFailurePresentation({ error: 'HTTP 400: invalid_request_error: Unsupported operation' }).kind).toBe('invalid-request')
  expect(providerFailurePresentation({ error: 'HTTP 503: temporarily unavailable' }).kind).toBe('busy')
})

test('standard overload errors have fixed copy without provider details', () => {
  for (const error of [
    'Overloaded: HTTP 503: overloaded_error: PRIVATE_BACKEND is cooling down',
    'HTTP 503: {"error":{"type":"server_error","code":"overloaded_error","message":"PRIVATE_BACKEND"}}',
    'overloaded_error: PRIVATE_BACKEND',
    'HTTP 529',
  ]) {
    const copy = providerFailurePresentation({ error })
    expect(copy.kind).toBe('overloaded')
    expect(copy.label).toBe('Service temporarily overloaded. Please retry.')
    const card = buildLlmCard(`[LLM] ✗ · model · turn 3 · 1.4s\n    error     ${error}`)
    const text = card.map(line => line.text).join('\n')
    expect(text).toContain(copy.label)
    expect(text).not.toContain('PRIVATE_BACKEND')
    expect(text).not.toContain('Service busy')
  }
  for (const error of ['HTTP 502', 'HTTP 503: temporarily unavailable', 'HTTP 504']) {
    expect(providerFailurePresentation({ error }).label).toBe('Service temporarily unavailable.')
  }
  for (const [error, kind] of [
    ['HTTP 429: overloaded_error: rate limit', 'rate-limit'],
    ['HTTP 401: overloaded_error', 'authentication'],
    ['HTTP 400: prompt is too long: overloaded_error', 'context-overflow'],
    ['HTTP 503: overloaded_error: quota exhausted', 'quota'],
  ]) {
    expect(providerFailurePresentation({ error }).kind).toBe(kind)
  }
})

test('TLS retries keep cumulative elapsed time across the long-wait transition', () => {
  let state = setRetryWait(createSpinnerState(), 2000, 1, 10, 1000, 'tls handshake eof')
  expect(stripAnsi(formatSpinnerLine(state, 1000))).toContain('Connection interrupted · retrying in 2s')
  state = setLongWait(state, 'outage_waiting', 60000, 121000, 'tls handshake eof')
  const text = stripAnsi(formatSpinnerLine(state, 121000, { inputTokens: 1234 }))
  expect(text).toContain('Unable to connect · retrying in 60s')
  expect(text).toContain('waiting 2m')
  expect(text).not.toContain('attempt')
  expect(text).not.toContain('tls')
  expect(text).not.toContain('↑')
  state = setLongWait(state, 'outage_waiting', 60000, 181000, 'tls handshake eof')
  expect(stripAnsi(formatSpinnerLine(state, 181000))).toContain('waiting 3m')
})

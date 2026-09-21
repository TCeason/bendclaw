/** Customer-facing provider failure vocabulary. No TUI, native, or DOM dependency.
 * Consumers retain the original error separately for diagnostics. Prefer a
 * structured category; string matching is only for historical error events.
 */
export type ProviderFailureKind = 'connection' | 'timeout' | 'dns' | 'busy' | 'overloaded' | 'gateway' | 'gateway-no-backend' | 'rate-limit' | 'quota' | 'authentication' | 'invalid-request' | 'context-overflow' | 'configuration' | 'backend-version' | 'model-not-found' | 'not-found' | 'unknown'

/** Who failed, when the gateway says so. Two parties only: the model
 * provider (the backend serving the model) or the gateway (the proxy in
 * between). It tells us in standard fields -- `error.code` (`provider_*` /
 * `gateway_*`), the message wording, and the HTTP status (500 is the
 * gateway's own; 502/503/504 are the provider's). */
function classifyByOrigin(text: string, status: number): ProviderFailureKind | undefined {
  if (/\bgateway_no_backend\b|the gateway has no available model backend/.test(text)) return 'gateway-no-backend'
  if (/\bgateway_(?:error|draining)\b|\bthe gateway (?:failed|is restarting)\b/.test(text)) return 'gateway'
  if (/\bprovider_overloaded\b|the model provider is temporarily overloaded/.test(text)) return 'overloaded'
  if (/\bprovider_timeout\b|the model provider did not respond in time/.test(text)) return 'timeout'
  if (/\bprovider_(?:error|unreachable|stream_interrupted)\b|\bthe model provider (?:failed|is temporarily unreachable|interrupted|ended)\b/.test(text)) return 'busy'
  if (status === 500) return 'gateway'
  return undefined
}

export function classifyProviderFailure(error: string): ProviderFailureKind {
  const text = error.toLowerCase()
  // Numeric values in error details (e.g. "minimum of 512 pixels")
  // are not HTTP statuses. Accept explicit markers or a leading bare code.
  const status = Number(text.match(/\bhttp(?:\/\d(?:\.\d)?)?\s+(\d{3})(?!\d)|\bstatus(?:_code|\s+code)?[\s:=]+(\d{3})(?!\d)|^\s*(\d{3})(?:\s|:|$)/)?.slice(1).find(Boolean))
  if (status === 401 || status === 403) return 'authentication'
  const origin = classifyByOrigin(text, status)
  if (origin) return origin
  if (/model_not_found|model not found/.test(text)) return 'model-not-found'
  if (status === 404 || /not_found_error/.test(text)) return 'not-found'
  if (/^invalid request:/.test(text)) return 'invalid-request'
  if (/configuration error:/.test(text)) return 'configuration'
  // Preserve the engine's overflow diagnosis before the generic HTTP 400 branch.
  if (/context overflow|context_length_exceeded|prompt is too long|request exceeds the model context window|maximum context length/.test(text)) return 'context-overflow'
  if (/quota|insufficient_quota|credit balance/.test(text)) return 'quota'
  if (status === 401 || status === 403 || /unauthorized|invalid.api.key|authentication/.test(text)) return 'authentication'
  if (status === 429 || /rate.limit|too many requests/.test(text)) return 'rate-limit'
  // Standard provider overload semantics, independent of gateway/vendor.
  // Do not label every transport failure or unknown 5xx as overload.
  if (status === 529 || /\boverloaded_error\b|\boverloaded\b/.test(text)) return 'overloaded'
  if (status >= 500 && status <= 599) return 'busy'
  if (status === 400 || status === 413 || status === 422 || /invalid_request_error/.test(text)) return 'invalid-request'
  if (/dns|enotfound|eai_again|failed to lookup|name resolution/.test(text)) return 'dns'
  if (/timed? ?out|timeout/.test(text)) return 'timeout'
  if (/tls|handshake|connection reset|econnreset|connection refused|econnrefused|network error|connect error/.test(text)) return 'connection'
  if (/server.error|service unavailable|bad gateway/.test(text)) return 'busy'
  return 'unknown'
}

const labels: Record<ProviderFailureKind, string> = {
  connection: 'Connection interrupted', timeout: 'Request timed out',
  dns: 'Unable to resolve service address',
  busy: 'Model provider unavailable. Retrying usually helps.',
  overloaded: 'Model provider overloaded. Please retry.',
  gateway: 'Gateway error, not the model provider. Retry; report it if it persists.',
  'gateway-no-backend': 'No backend available for this model right now. Retry later or switch model.',
  'rate-limit': 'Rate limited', quota: 'Quota unavailable',
  authentication: 'Authentication failed', 'invalid-request': 'Invalid request',
  'context-overflow': 'Context limit exceeded',
  configuration: 'Configuration error', 'backend-version': 'Backend version unsupported',
  'model-not-found': 'Model not found', 'not-found': 'Resource not found',
  unknown: 'Request failed',
}

export function providerFailurePresentation(input: {
  error?: string
  kind?: ProviderFailureKind
  sustained?: boolean
}): { kind: ProviderFailureKind; label: string; guidance?: string } {
  const kind = input.kind ?? classifyProviderFailure(input.error ?? '')
  return {
    kind,
    label: input.sustained && kind === 'connection' ? 'Unable to connect' : labels[kind],
    guidance: kind === 'connection' || kind === 'timeout' || kind === 'dns'
      ? 'Check your network or proxy settings. The service may also be temporarily unavailable.'
      : kind === 'context-overflow'
        ? 'Compact the conversation before retrying. The upstream input limit may be smaller than the advertised model window.'
      : kind === 'model-not-found'
        ? 'Check the model identifier and access permissions, or select another model.'
      : kind === 'not-found'
        ? 'Check the API endpoint and requested resource.'
      : kind === 'invalid-request'
        ? 'Check the request content and attachments against the model’s input requirements. Retrying unchanged will not help.'
        : kind === 'busy' || kind === 'overloaded'
          ? 'The model provider is having trouble; your request and the gateway are fine. Retrying usually helps, or switch model.'
        : kind === 'gateway'
          ? 'The gateway itself failed on this request. Retry; if it keeps happening, tell the gateway administrator and mention the time and model.'
        : kind === 'gateway-no-backend'
          ? 'Every backend the gateway has for this model is unavailable or cooling down. Retry in a minute or pick another model.'
        : kind === 'configuration'
          ? 'No channel is configured for this model. Pick another model or ask the proxy administrator to add one. Retrying unchanged will not help.'
          : kind === 'backend-version'
            ? 'The model backend requires a newer client version than the proxy provides. Ask the proxy administrator to update it. Retrying unchanged will not help.'
            : undefined,
  }
}

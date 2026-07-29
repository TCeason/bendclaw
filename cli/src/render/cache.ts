/**
 * Prompt-cache display helpers and miss detection (after pi's cache-stats).
 * Usage buckets are disjoint: uncached input + cache read + cache write.
 */

import { humanTokens } from './format.js'

/** Prompt-cache TTL (Anthropic default 5m); longer idle gaps explain a miss. */
export const CACHE_TTL_MS = 5 * 60 * 1000

/** Misses at or below cache-breakpoint granularity are noise. */
const NOISE_FLOOR_TOKENS = 1024

/** Only misses at least this large surface as a transcript notice. */
const NOTICE_MIN_MISSED_TOKENS = 20_000

/**
 * Cache hit share of billed prompt tokens. One decimal in [99, 100) — a
 * steady loop always has a small uncached tail and rounding would pin the
 * display at a fake "100%"; "100" only when the whole prompt was read.
 */
export function formatCacheHitPercent(
  inputTokens: number,
  cacheReadTokens: number,
  cacheWriteTokens = 0,
): string {
  const total = inputTokens + cacheReadTokens + cacheWriteTokens
  if (total <= 0 || cacheReadTokens <= 0) return '0'
  if (cacheReadTokens >= total) return '100'
  const pct = (cacheReadTokens / total) * 100
  if (pct >= 99) return Math.min(99.9, Math.floor(pct * 10) / 10).toFixed(1)
  return String(Math.round(pct))
}

/** Disjoint prompt usage buckets of one completed LLM call. */
export interface PromptUsageBuckets {
  inputTokens: number
  cacheReadTokens: number
  cacheWriteTokens: number
}

/** The last billed request; everything in its prompt should be cached next. */
export interface PromptCacheSnapshot {
  /** Total prompt tokens: input + cache read + cache write. */
  promptTokens: number
  usage: PromptUsageBuckets
  model: string
  timestamp: number
  /** Sticky: some earlier call reported cache activity. Distinguishes a total
   *  miss on a read-only provider from one that never reports caching. */
  reportedCache: boolean
}

/** A counted cache miss on a single completed LLM call. */
export interface CacheMissInfo {
  /** Tokens that were in the previous prompt but re-billed instead of read. */
  missedTokens: number
  idleMs: number
  modelChanged: boolean
}

/**
 * Detect a cache miss relative to the previous request. Null when nothing is
 * counted: first call, no cache activity ever reported, or below noise floor.
 */
export function detectCacheMiss(
  prev: PromptCacheSnapshot | null,
  usage: PromptUsageBuckets,
  model: string,
  timestamp: number,
): CacheMissInfo | null {
  const promptTokens = usage.inputTokens + usage.cacheReadTokens + usage.cacheWriteTokens
  if (!prev || promptTokens <= 0) return null
  if (usage.cacheReadTokens + usage.cacheWriteTokens === 0 && !prev.reportedCache) return null
  const missedTokens = Math.min(prev.promptTokens, promptTokens) - usage.cacheReadTokens
  if (missedTokens <= NOISE_FLOOR_TOKENS) return null
  return {
    missedTokens,
    idleMs: Math.max(0, timestamp - prev.timestamp),
    modelChanged: model !== prev.model,
  }
}

/** Roll the snapshot forward; calls without prompt usage keep the previous one. */
export function nextPromptCacheSnapshot(
  prev: PromptCacheSnapshot | null,
  usage: PromptUsageBuckets,
  model: string,
  timestamp: number,
): PromptCacheSnapshot | null {
  const promptTokens = usage.inputTokens + usage.cacheReadTokens + usage.cacheWriteTokens
  if (promptTokens <= 0) return prev
  return {
    promptTokens,
    usage,
    model,
    timestamp,
    reportedCache: (prev?.reportedCache ?? false) || usage.cacheReadTokens + usage.cacheWriteTokens > 0,
  }
}

/**
 * Transcript notice for a significant cache miss, naming the observable
 * cause (model switch / idle past TTL). Small misses stay log-only.
 */
export function formatCacheMissNotice(miss: CacheMissInfo): string | null {
  if (miss.missedTokens < NOTICE_MIN_MISSED_TOKENS) return null
  let label = 'cache miss'
  if (miss.modelChanged) label = 'cache miss after model switch'
  else if (miss.idleMs >= CACHE_TTL_MS) label = `cache miss after ${Math.round(miss.idleMs / 60_000)}m idle`
  return `[LLM] ⚠ · ${label} · ${humanTokens(miss.missedTokens)} tokens re-billed`
}

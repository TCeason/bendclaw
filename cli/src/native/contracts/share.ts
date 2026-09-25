import { array, json, nullable, object, optional, text, uint } from './schema.js'

export const shareCreated = object({ id: text, url: text })
/** `created_at` / `size_bytes` are metadata the list can render without: a
 *  server that stops sending them must not turn the selector into a schema
 *  error. Only id and url are load-bearing. */
export const shareList = object({ shares: array(object({
  id: text, url: text, title: optional(nullable(text)),
  created_at: optional(nullable(uint)), size_bytes: optional(nullable(uint)),
  /** `session` (also when absent: older servers only had sessions) or `task`. */
  kind: optional(text),
  /** The local record the link was made from — session id or task id — so
   *  the selector can match a link to what it shows. Absent on older servers. */
  source_id: optional(text),
  /** Task shares only: schedule and model, so the selector can say what the
   *  link holds without fetching it. Every field is optional. */
  summary: optional(nullable(json)),
})) })

export interface ShareNotice {
  schema_version: 1
  level: 'error' | 'system' | 'cancelled'
  text: string
  timestamp: number
  kind?: 'thinking_level_change' | 'model_change'
  data?: { thinking_level: string } | { provider: string; model: string }
}

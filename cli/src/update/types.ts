export interface ReleaseInfo {
  tag: string       // "v2026.4.13"
  version: string   // "2026.4.13"
}

export type CheckResult =
  | { kind: 'up_to_date' }
  | { kind: 'available'; latest: ReleaseInfo }
  | { kind: 'error'; message: string }

export type RunResult =
  | { kind: 'up_to_date' }
  | { kind: 'updated'; from: string; to: string }
  | { kind: 'error'; message: string }

/**
 * distill — types shared across the dataset-generation pipeline.
 *
 * Two layers of "task":
 *  - DomainSpec: the high-level direction a human writes (the only human input).
 *  - TaskSpec:   a concrete task, either authored by the Proposer agent or
 *                loaded from a curated JSONL file. Same shape for both.
 *
 * The pipeline turns TaskSpecs into a DatasetBundle that the downstream RL/SFT
 * trainer consumes with zero post-processing.
 */

// ---------------------------------------------------------------------------
// Input layer
// ---------------------------------------------------------------------------

export interface RunLimits {
  maxTurns?: number
  maxTokens?: number
  maxDuration?: number
}

/** Coarse difficulty label: a complexity tier, not a runtime limit. The number
 *  in each name is an ordinal level (higher = more complex). See ./difficulty.ts. */
export type Difficulty = 'L2' | 'L4' | 'L6' | 'L8' | 'L16'

export interface DomainSpec {
  domain: string
  taskTypes?: string[]
  n: number
  /** Per-task difficulty plan (length n) the proposer should author against. */
  difficulties?: Difficulty[]
  limits?: { solver?: RunLimits; builder?: RunLimits }
}

export type WorkspaceSource =
  | { source: 'inline'; files: Record<string, string> }
  | { source: 'dir'; path: string }
  | { source: 'git_local' | 'git'; repo: string; ref?: string }
  | { source: 'agent_scaffold'; builderPrompt: string; setup?: string[] }

export interface Verifier {
  checkCommand: string
  expectedExitCode?: number
}

export interface TaskSpec {
  id: string
  prompt: string
  answer: string
  workspace: WorkspaceSource & { setup?: string[] }
  verifier: Verifier
  /** Builder's known-good solution, used for the self-check gate. Never trained on. */
  referencePatch?: string
  /** Paths the Solver must not touch (e.g. tests the verifier depends on). */
  protectedPaths?: string[]
  limits?: RunLimits
  /** Coarse difficulty label (complexity tier, see difficulty.ts). Not a runtime limit. */
  difficulty?: Difficulty
  split?: 'train' | 'eval' | 'probe'
  /** Provenance: "evot_auto" | "curated". */
  source?: string
}

// ---------------------------------------------------------------------------
// Output layer (downstream contract — do not change field names lightly)
// ---------------------------------------------------------------------------

export type SftBlock =
  | { type: 'thinking'; thinking: string }
  | { type: 'text'; text: string }
  | { type: 'tool_use'; id: string; name: string; input: Record<string, unknown> }
  | { type: 'tool_result'; tool_use_id: string; content: string; is_error?: boolean }

export interface SftMessage {
  role: 'system' | 'user' | 'assistant'
  content: string | SftBlock[]
}

export interface SftRow {
  messages: SftMessage[]
  tools: unknown[]
  metadata: Record<string, unknown>
}

export interface RlRow {
  id: string
  prompt: { role: 'user'; content: string }[]
  label: { answer: string }
  metadata: Record<string, unknown>
}

// ---------------------------------------------------------------------------
// Orchestration options
// ---------------------------------------------------------------------------

export type Verbosity = 'quiet' | 'normal' | 'verbose'

export interface DistillOptions {
  tasksFile?: string
  auto?: boolean
  domain?: string
  domainsFile?: string
  n?: number
  out: string
  model?: string
  envFile?: string
  /** Tool whitelist for SFT normalization. Default: read/write/edit/bash. */
  tools: string[]
  emit: ('sft' | 'rl')[]
  /** Skip the Solver: validate solvability via the builder's reference patch
   *  and emit only RL rows. Much faster when SFT trajectories aren't needed. */
  rlOnly?: boolean
  repeats: number
  keepFail: boolean
  maxConcurrency: number
  perTaskTimeout: number
  /** Difficulty tier to author. 'mixed' spreads tasks evenly across all tiers.
   *  Default: 'L2'. A complexity label only; imposes no runtime limit. */
  difficulty: Difficulty | 'mixed'
  /** Parent directory for temporary Builder/Solver workspaces. Default: <out>/.distill-work. */
  workspaceRoot?: string
  /** Output detail level. Default: normal. */
  verbosity: Verbosity
}

/**
 * Extension system types.
 *
 * An extension is a module that registers host-owned tools (and, later,
 * commands and renderers) with the {@link ExtensionHost}. Host tools are
 * described by a {@link HostToolSpec} that the Rust engine advertises to the
 * LLM; when the LLM calls one, the engine delegates execution back here via a
 * `host_tool_call` event, and the tool's {@link HostTool.execute} runs in TS.
 *
 * This is the single, reusable seam behind ask_user and any future domain
 * workflow — the engine core knows none of them.
 */

/** Content block returned to the LLM as part of a tool result. */
export interface TextContent {
  type: 'text'
  text: string
}

export type ToolResultContent = TextContent

/** Result a host tool returns to the engine. */
export interface HostToolResult {
  /** Blocks shown to the LLM as the tool result. */
  content: ToolResultContent[]
  /** Structured metadata for UI rendering / state reconstruction. Never sent
   *  to the LLM. */
  details?: unknown
  /** Whether this result represents an error. */
  isError?: boolean
}

/**
 * Static description of a host tool. Serialized to the engine as a spec so it
 * can advertise the tool to the LLM and route calls back. Field names are
 * snake_case to match the Rust `HostToolSpec` wire format.
 */
export interface HostToolSpec {
  name: string
  label: string
  description: string
  parameters_schema: unknown
  prompt_snippet?: string
  /** Model-specific name aliases: [model_pattern, llm_name]. */
  name_aliases?: [string, string][]
}

/** Context passed to a host tool's execute. */
export interface HostToolContext {
  toolCallId: string
}

/** A registered host tool: its spec plus its TS execution logic. */
export interface HostTool<TParams = Record<string, unknown>> {
  spec: HostToolSpec
  execute(params: TParams, ctx: HostToolContext): Promise<HostToolResult>
}

// ---------------------------------------------------------------------------
// The event forwarded from the engine when the LLM calls a host tool.
// ---------------------------------------------------------------------------

export interface HostToolCallEvent {
  tool_name: string
  tool_call_id: string
  arguments: Record<string, unknown>
}

/** The wire response the host sends back for a host tool call. */
export interface HostToolResponsePayload {
  tool_call_id: string
  content: ToolResultContent[]
  details?: unknown
  is_error?: boolean
}

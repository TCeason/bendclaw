/**
 * ExtensionHost — registry and dispatcher for host-owned tools.
 *
 * Extensions register {@link HostTool}s here. The host exposes two things the
 * rest of the CLI needs:
 *
 * 1. {@link specsJson} — the JSON array of tool specs passed to `agent.query`
 *    so the engine can advertise them to the LLM.
 * 2. {@link dispatch} — handles a `host_tool_call` event by running the matched
 *    tool's `execute` and returning the wire response payload.
 *
 * The host holds no tool-specific logic; ask_user, plan, and any future tool
 * are just registered entries. This keeps the engine core and the CLI shell
 * free of domain concerns.
 */

import type {
  ExtensionUI,
  HostTool,
  HostToolCallEvent,
  HostToolResponsePayload,
  HostToolSpec,
} from './types.js'

export class ExtensionHost {
  // Tools are stored with their params type erased; each tool validates its
  // own params in execute. The public register() below keeps call-site typing.
  private tools = new Map<string, HostTool<never>>()

  /** Register a host tool. A later registration with the same name wins. */
  register<T>(tool: HostTool<T>): void {
    this.tools.set(tool.spec.name, tool as unknown as HostTool<never>)
  }

  /** Whether any tools are registered. */
  get isEmpty(): boolean {
    return this.tools.size === 0
  }

  /** All registered specs, for advertising to the engine. */
  specs(): HostToolSpec[] {
    return [...this.tools.values()].map(t => t.spec)
  }

  /** JSON payload for `agent.query`'s hostSpecsJson argument, or undefined
   *  when no tools are registered (headless-style runs). */
  specsJson(): string | undefined {
    if (this.isEmpty) return undefined
    return JSON.stringify(this.specs())
  }

  /** Resolve a called tool name (canonical or alias) to its registered tool. */
  private resolve(calledName: string): HostTool<never> | undefined {
    const direct = this.tools.get(calledName)
    if (direct) return direct
    const lower = calledName.toLowerCase()
    for (const tool of this.tools.values()) {
      if (tool.spec.name.toLowerCase() === lower) return tool
      if (tool.spec.name_aliases?.some(([, alias]) => alias.toLowerCase() === lower)) {
        return tool
      }
    }
    return undefined
  }

  /**
   * Execute a host tool call and produce the wire response.
   *
   * Never throws: a missing tool or a thrown execute yields an error result so
   * the engine's tool-execution path always gets a response and the run does
   * not hang.
   */
  async dispatch(call: HostToolCallEvent, ui: ExtensionUI): Promise<HostToolResponsePayload> {
    const tool = this.resolve(call.tool_name)
    if (!tool) {
      return {
        tool_call_id: call.tool_call_id,
        content: [{ type: 'text', text: `Unknown host tool: ${call.tool_name}` }],
        is_error: true,
      }
    }

    try {
      const result = await tool.execute(call.arguments as never, {
        toolCallId: call.tool_call_id,
        ui,
      })
      return {
        tool_call_id: call.tool_call_id,
        content: result.content,
        details: result.details,
        is_error: result.isError ?? false,
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      return {
        tool_call_id: call.tool_call_id,
        content: [{ type: 'text', text: message }],
        is_error: true,
      }
    }
  }
}

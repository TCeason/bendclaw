import chalk from 'chalk'
import type { Agent, SessionMeta, ConfigInfo } from '../../native/index.js'
import type { AppState } from './state.js'
import type { OutputLine } from '../../render/output.js'
import type { OverlayState } from '../viewmodel/index.js'
import { resolveCommand } from '../../commands/index.js'

export interface CommandContext {
  agent: Agent
  appState: AppState
  configInfo?: ConfigInfo
  preloadedSessions: SessionMeta[]
  planning: boolean
}

export interface CommandResult {
  appState: AppState
  planning: boolean
  overlay?: OverlayState
  clearScreen?: boolean
  clearContext?: boolean
  exit?: boolean
  resumeSession?: SessionMeta
  systemLines: OutputLine[]
}

export function handleSlashCommand(text: string, ctx: CommandContext): CommandResult {
  const resolved = resolveCommand(text)

  if (resolved.kind === 'unknown') {
    return {
      ...baseResult(ctx),
      systemLines: [{ id: 'sys-unk', kind: 'system', text: `  Unknown command: ${text.trim()}` }],
    }
  }

  if (resolved.kind === 'ambiguous') {
    return {
      ...baseResult(ctx),
      systemLines: [
        { id: 'sys-amb', kind: 'system', text: `  Ambiguous command: ${resolved.candidates.join(', ')}` },
        { id: 'sys-amb2', kind: 'system', text: '  Type more characters or /help for commands' },
      ],
    }
  }

  const { name, args } = resolved

  switch (name) {
    case '/help':
      return { ...baseResult(ctx), overlay: { kind: 'help' } }

    case '/verbose': {
      const appState = { ...ctx.appState, verbose: !ctx.appState.verbose }
      return {
        ...baseResult(ctx),
        appState,
        systemLines: [{ id: 'sys-v', kind: 'system', text: `  verbose: ${appState.verbose ? 'on' : 'off'}` }],
      }
    }

    case '/clear':
      return { ...baseResult(ctx), clearContext: true }

    case '/exit':
      return { ...baseResult(ctx), exit: true }

    case '/plan': {
      const planning = !ctx.planning
      return {
        ...baseResult(ctx),
        planning,
        systemLines: [{ id: 'sys-p', kind: 'system', text: `  planning: ${planning ? 'on' : 'off'}` }],
      }
    }

    case '/model': {
      if (args === 'n') {
        const models = ctx.configInfo?.availableModels ?? [ctx.agent.model]
        if (models.length <= 1) {
          return { ...baseResult(ctx), systemLines: [{ id: 'sys-m', kind: 'system', text: '  Only one model available.' }] }
        }
        const idx = models.indexOf(ctx.agent.model)
        const next = models[(idx + 1) % models.length]!
        ctx.agent.model = next
        const appState = { ...ctx.appState, model: next }
        return {
          ...baseResult(ctx),
          appState,
          systemLines: [{ id: 'sys-m', kind: 'system', text: `  Model → ${next}` }],
        }
      }
      if (args) {
        ctx.agent.model = args
        const appState = { ...ctx.appState, model: args }
        return {
          ...baseResult(ctx),
          appState,
          systemLines: [{ id: 'sys-m', kind: 'system', text: `  Model → ${args}` }],
        }
      }
      // No arg — return empty result; handleSlashInput will show selector overlay
      return baseResult(ctx)
    }

    case '/resume': {
      // Handled asynchronously in handleSlashInput (needs full session list)
      return baseResult(ctx)
    }

    default:
      // Commands handled asynchronously by handleSlashInput in repl.ts
      return baseResult(ctx)
  }
}

function baseResult(ctx: CommandContext): CommandResult {
  return {
    appState: ctx.appState,
    planning: ctx.planning,
    systemLines: [],
  }
}

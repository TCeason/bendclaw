import React from 'react'
import { Agent } from '../native/index.js'
import type { AppState } from '../state/app.js'
import type { UIMessage } from '../state/types.js'
import { createInitialState } from '../state/app.js'
import type { OutputLine } from '../render/output.js'
import { messagesToOutputLines } from '../render/output.js'
import { transcriptToMessages, type TranscriptItem } from '../session/transcript.js'
import { pushSystem, type SystemMsg } from './messages.js'

export function syncProvider(
  agent: Agent,
  model: string,
  configInfo?: import('../native/index.js').ConfigInfo,
): void {
  try {
    if (configInfo) {
      if (model === configInfo.anthropicModel) { agent.setProvider('anthropic'); return }
      if (model === configInfo.openaiModel) { agent.setProvider('openai'); return }
    }
    if (model.startsWith('claude-') || model.startsWith('anthropic/')) {
      agent.setProvider('anthropic')
    } else if (model.startsWith('gpt-') || model.startsWith('o1-') || model.startsWith('o3-') || model === 'o1' || model === 'o3') {
      agent.setProvider('openai')
    }
  } catch { /* ignore — provider may not support the model */ }
}

export async function resumeSession(
  agent: Agent,
  session: import('../native/index.js').SessionMeta,
  setState: React.Dispatch<React.SetStateAction<AppState>>,
  setSystem: React.Dispatch<React.SetStateAction<SystemMsg[]>>,
  setOutputLines: React.Dispatch<React.SetStateAction<OutputLine[]>>,
) {
  let messages: UIMessage[] = []
  try {
    const transcript = await agent.loadTranscript(session.session_id)
    messages = transcriptToMessages(transcript as TranscriptItem[])
  } catch { /* ignore */ }

  if (session.model) {
    agent.model = session.model
    syncProvider(agent, session.model)
  }

  const lines = messagesToOutputLines(messages)
  setOutputLines(lines)

  setState((prev) => ({
    ...createInitialState(session.model || prev.model, prev.cwd),
    verbose: prev.verbose,
    sessionId: session.session_id,
    messages,
  }))
  const tag = session.source ? `[${session.source}] ` : ''
  pushSystem(setSystem, 'info', `Resumed ${tag}${session.session_id.slice(0, 8)} — ${session.title || '(untitled)'}`)
}

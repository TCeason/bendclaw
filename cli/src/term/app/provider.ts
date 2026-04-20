import type { Agent, ConfigInfo } from '../../native/index.js'

export function syncProvider(
  agent: Agent,
  model: string,
  configInfo?: ConfigInfo,
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

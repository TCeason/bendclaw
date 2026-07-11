import type { ConfigInfo, ModelOption } from '../../native/index.js'

/** Return configured provider/model pairs, preserving duplicate model ids. */
export function modelOptions(configInfo: ConfigInfo | undefined, fallbackModel: string): ModelOption[] {
  const configured = configInfo?.availableModels ?? []
  if (configured.length > 0) return configured
  const provider = configInfo?.provider ?? ''
  return [{ provider, model: fallbackModel, spec: provider ? `${provider}:${fallbackModel}` : fallbackModel }]
}

export function currentModelSpec(configInfo: ConfigInfo | undefined, model: string): string {
  return configInfo?.provider ? `${configInfo.provider}:${model}` : model
}

export function formatModelLabel(model: string, provider: string): string {
  return provider ? `${model}@${provider}` : model
}

export function selectModelOption(configInfo: ConfigInfo | undefined, spec: string): ModelOption | undefined {
  return configInfo?.availableModels.find(option => option.spec === spec)
}

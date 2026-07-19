import type { ConfigInfo, ModelOption } from '../../native/index.js'

/** Return configured provider/model pairs, preserving duplicate model ids. */
export function modelOptions(configInfo: ConfigInfo | undefined, fallbackModel: string): ModelOption[] {
  const configured = configInfo?.availableModels ?? []
  if (configured.length > 0) return configured
  const provider = configInfo?.provider ?? ''
  return [{ provider, model: fallbackModel, spec: provider ? `${provider}:${fallbackModel}` : fallbackModel }]
}

export function sortModelOptionsForSelector(options: ModelOption[], activeSpec: string): ModelOption[] {
  const activeProvider = options.find(option => option.spec === activeSpec)?.provider
  return options
    .map((option, index) => ({ option, index }))
    .sort((left, right) => {
      const leftGroup = left.option.provider === activeProvider ? 0 : 1
      const rightGroup = right.option.provider === activeProvider ? 0 : 1
      if (leftGroup !== rightGroup) return leftGroup - rightGroup

      const providerOrder = left.option.provider.localeCompare(right.option.provider)
      if (providerOrder !== 0) return providerOrder

      const leftActive = left.option.spec === activeSpec ? 0 : 1
      const rightActive = right.option.spec === activeSpec ? 0 : 1
      return leftActive - rightActive || left.index - right.index
    })
    .map(entry => entry.option)
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

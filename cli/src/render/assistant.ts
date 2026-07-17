import { buildAssistantLines, buildThinkingLines, buildToolCard, type OutputLine } from './output.js'
import type { UIAssistantBlock } from '../term/app/types.js'

export function assistantMessageToOutputLines(
  content: UIAssistantBlock[],
  expandedTools = false,
  options: { streaming?: boolean } = {},
): OutputLine[] {
  return assistantContentToOutputLines(content, expandedTools, options)
}

/** Convert ordered assistant blocks to committed terminal output. */
export function assistantContentToOutputLines(
  content: UIAssistantBlock[],
  expandedTools = false,
  options: { streaming?: boolean } = {},
): OutputLine[] {
  return [...content]
    .sort((a, b) => a.contentIndex - b.contentIndex)
    .flatMap(block => {
      if (block.type === 'thinking') return buildThinkingLines(block.text, options)
      if (block.type === 'text') return buildAssistantLines(block.text, options)
      return buildToolCard(block.toolCall, expandedTools)
    })
}

/** Visible text blocks used by non-interactive clients and overflow handling. */
export function assistantText(content: UIAssistantBlock[]): string {
  return [...content]
    .sort((a, b) => a.contentIndex - b.contentIndex)
    .filter((block): block is Extract<UIAssistantBlock, { type: 'text' }> => block.type === 'text')
    .map(block => block.text)
    .join('')
}

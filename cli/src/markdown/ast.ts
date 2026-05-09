import type { Token } from 'marked'

export type MarkdownNode = MarkedMarkdownNode

export interface MarkedMarkdownNode {
  type: 'marked'
  token: Token
}

export function markedTokensToNodes(tokens: Token[]): MarkdownNode[] {
  return tokens.map(token => ({ type: 'marked', token }))
}

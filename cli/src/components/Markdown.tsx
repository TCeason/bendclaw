/**
 * Markdown — renders markdown content as Ink components.
 *
 * Hybrid approach: tables are rendered as React components with terminal-width
 * awareness; all other content is rendered as ANSI strings via formatToken.
 */

import React, { useMemo } from 'react'
import { Box, Text, useStdout } from 'ink'
import { marked, type Token, type Tokens } from 'marked'
import stripAnsi from 'strip-ansi'
import { configureMarked, formatToken } from '../render/markdown.js'
import { MarkdownTable } from './MarkdownTable.js'

interface Props {
  children: string
  dimColor?: boolean
}

export function Markdown({ children, dimColor }: Props) {
  const { stdout } = useStdout()
  const termWidth = stdout?.columns ?? 80

  const elements = useMemo(() => {
    if (!children || !children.trim()) return null
    configureMarked()
    const tokens = marked.lexer(children)
    const result: React.ReactNode[] = []
    let nonTableContent = ''

    function flush() {
      if (nonTableContent) {
        const trimmed = nonTableContent
          .replace(/\n{3,}/g, '\n\n')
          .trim()
        if (trimmed) {
          result.push(
            <Text key={result.length} dimColor={dimColor}>{trimmed}</Text>,
          )
        }
        nonTableContent = ''
      }
    }

    for (const token of tokens) {
      if (token.type === 'table') {
        flush()
        result.push(
          <MarkdownTable
            key={result.length}
            token={token as Tokens.Table}
            termWidth={termWidth}
          />,
        )
      } else {
        nonTableContent += formatToken(token)
      }
    }
    flush()
    return result
  }, [children, dimColor, termWidth])

  if (!elements || elements.length === 0) return null

  return (
    <Box flexDirection="column" gap={0}>
      {elements}
    </Box>
  )
}

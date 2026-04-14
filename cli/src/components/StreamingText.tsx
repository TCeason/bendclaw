/**
 * StreamingText component — renders only the active (growing) tail block.
 *
 * The parent manages the boundary via splitStableBlocks and passes
 * streamBoundary to indicate where frozen content ends. This component
 * only renders text after that boundary.
 */

import React from 'react'
import { Text, Box } from 'ink'
import { renderMarkdown } from '../utils/markdown.js'

interface StreamingTextProps {
  text: string
  thinkingText: string
  /** Character offset: text before this is already in <Static> */
  streamBoundary: number
}

export function StreamingText({ text, thinkingText, streamBoundary }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  const activeTail = text.substring(streamBoundary)
  const activeRendered = activeTail ? renderMarkdown(activeTail) : ''
  const hasFrozen = streamBoundary > 0

  return (
    <Box flexDirection="column" marginBottom={1}>
      {thinkingText.length > 0 && (
        <Box marginBottom={0}>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {activeRendered.length > 0 && (
        <Box marginTop={hasFrozen ? 0 : 1}>
          {!hasFrozen && <Text color="magenta" bold>{'⏺ '}</Text>}
          <Box flexDirection="column" flexShrink={1}>
            <Text>{activeRendered.replace(/^\n+/, '')}</Text>
            <Text color="gray">▍</Text>
          </Box>
        </Box>
      )}
      {activeRendered.length === 0 && (
        <Box>
          <Text color="gray">▍</Text>
        </Box>
      )}
    </Box>
  )
}

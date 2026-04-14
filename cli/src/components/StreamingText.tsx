/**
 * StreamingText component — shows only the current incomplete line.
 *
 * Completed lines are written directly to stdout by StreamWriter.
 * This component only renders the trailing incomplete fragment
 * and thinking text in Ink's dynamic zone.
 */

import React from 'react'
import { Text, Box } from 'ink'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

export const StreamingText = React.memo(function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (text.length === 0 && thinkingText.length === 0) {
    return null
  }

  // Only show the last incomplete line (after the last newline).
  // Everything before has already been written to stdout by StreamWriter.
  const lastNl = text.lastIndexOf('\n')
  const pendingLine = lastNl >= 0 ? text.slice(lastNl + 1) : text

  return (
    <Box flexDirection="column">
      {thinkingText.length > 0 && (
        <Box>
          <Text dimColor italic>
            {thinkingText}
          </Text>
        </Box>
      )}
      {pendingLine.length > 0 && (
        <Box>
          <Text dimColor>{pendingLine}</Text>
        </Box>
      )}
    </Box>
  )
})

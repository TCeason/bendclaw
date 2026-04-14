/**
 * StreamingText component — shows only thinking text.
 *
 * All assistant text is written directly to stdout by StreamWriter.
 * This component only renders thinking text in Ink's dynamic zone.
 */

import React from 'react'
import { Text, Box } from 'ink'

interface StreamingTextProps {
  text: string
  thinkingText: string
}

export const StreamingText = React.memo(function StreamingText({ text, thinkingText }: StreamingTextProps) {
  if (thinkingText.length === 0) {
    return null
  }

  return (
    <Box flexDirection="column">
      <Box>
        <Text dimColor italic>
          {thinkingText}
        </Text>
      </Box>
    </Box>
  )
})

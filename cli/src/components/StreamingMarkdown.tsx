/**
 * StreamingMarkdown — renders a small pending markdown tail in the dynamic zone.
 * Memoized so unchanged text doesn't trigger re-renders.
 * Limits visible output to maxHeight lines (tail truncation).
 */

import React, { useMemo } from 'react'
import { Box, Text } from 'ink'
import { renderMarkdown } from '../render/markdown.js'

interface Props {
  text: string
  maxHeight: number
}

export const StreamingMarkdown = React.memo(function StreamingMarkdown({ text, maxHeight }: Props) {
  const rendered = useMemo(() => {
    if (!text) return ''
    const full = renderMarkdown(text).replace(/\n+$/, '')
    const lines = full.split('\n')
    if (lines.length <= maxHeight) return full
    return lines.slice(-maxHeight).join('\n')
  }, [text, maxHeight])

  if (!rendered) return null

  return (
    <Box>
      <Text>{'  '}{rendered}</Text>
    </Box>
  )
})

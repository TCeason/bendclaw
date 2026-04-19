/**
 * OutputView — renders OutputLines as plain React components.
 * Only the most recent lines are kept in the Ink dynamic area (render cap).
 * Older lines scroll into the terminal's native scrollback.
 */

import React, { useMemo } from 'react'
import { Text, Box } from 'ink'
import type { OutputLine } from '../render/output.js'
import { filterLines } from '../render/output.js'

const RENDER_CAP = 100

interface Props {
  banner: React.ReactNode
  lines: OutputLine[]
  verbose?: boolean
}

export function OutputView({ banner, lines, verbose = true }: Props) {
  const visible = useMemo(() => filterLines(lines, verbose), [lines, verbose])
  const capped = visible.length > RENDER_CAP ? visible.slice(-RENDER_CAP) : visible

  return (
    <Box flexDirection="column">
      {banner}
      {capped.map((line, index) => {
        const globalIndex = visible.length - capped.length + index
        const prevKind = globalIndex > 0 ? visible[globalIndex - 1]?.kind : undefined
        return <OutputLineView key={line.id} line={line} prevKind={prevKind} />
      })}
    </Box>
  )
}

const OutputLineView = React.memo(function OutputLineView({ line, prevKind }: { line: OutputLine; prevKind?: string }) {
  switch (line.kind) {
    case 'user':
      return (
        <Box marginTop={1}>
          <Text bold color="yellow">{'❯ '}</Text>
          <Text bold>{line.text}</Text>
        </Box>
      )
    case 'assistant': {
      const isBlockStart = prevKind !== 'assistant'
      return (
        <Box marginTop={isBlockStart ? 1 : 0}>
          <Text>{'  '}{line.text}</Text>
        </Box>
      )
    }
    case 'tool':
      return <ToolLineView text={line.text} />
    case 'tool_result':
      return (
        <Box>
          <Text color="gray">{line.text}</Text>
        </Box>
      )
    case 'verbose':
      return <VerboseLineView text={line.text} />
    case 'error':
      return (
        <Box>
          <Text color="red">{line.text}</Text>
        </Box>
      )
    case 'system':
      return (
        <Box>
          <Text dimColor>{line.text}</Text>
        </Box>
      )
    case 'run_summary':
      return (
        <Box>
          <Text dimColor>{line.text}</Text>
        </Box>
      )
    default:
      return null
  }
})

function ToolLineView({ text }: { text: string }) {
  const badgeMatch = text.match(/^\[([^\]]+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCompleted = rest.startsWith('completed')
    const isFailed = rest.startsWith('failed')
    const isCall = rest.startsWith('call')
    let color: string = 'yellow'
    if (isCompleted) color = 'green'
    if (isFailed) color = 'red'
    if (isCall) color = 'yellow'
    return (
      <Box marginTop={1}>
        <Text color={color} bold>[{badge}]</Text>
        {rest ? <Text dimColor> {rest}</Text> : null}
      </Box>
    )
  }
  // Detail line (indented with spaces)
  if (text.startsWith('  ')) {
    return (
      <Box>
        <Text dimColor>{text}</Text>
      </Box>
    )
  }
  return (
    <Box>
      <Text>{text}</Text>
    </Box>
  )
}

function VerboseLineView({ text }: { text: string }) {
  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const isCompleted = rest.startsWith('completed') || rest.startsWith('·')
    const isFailed = rest.startsWith('failed')
    let color: string = 'yellow'
    if (badge === 'COMPACT') color = 'green'
    if (isCompleted) color = 'green'
    if (isFailed) color = 'red'
    return (
      <Box marginTop={1}>
        <Text color={color} bold>[{badge}]</Text>
        {rest ? <Text dimColor={!isFailed} color={isFailed ? 'red' : undefined}> {rest}</Text> : null}
      </Box>
    )
  }
  // Detail line (indented with spaces)
  return (
    <Box>
      <Text dimColor>{text}</Text>
    </Box>
  )
}

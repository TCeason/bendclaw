/**
 * MessageHistory — renders committed messages and frozen stream blocks
 * using ink's <Static>. Items are appended once and never re-rendered.
 */

import React from 'react'
import { Static, Text, Box } from 'ink'
import { Message } from './Message.js'
import { RunSummary } from './RunSummary.js'
import { VerboseEventLine } from './VerboseEventLine.js'
import type { UIMessage } from '../state/AppState.js'

type StaticItem =
  | { kind: 'banner'; id: string; node: React.ReactNode }
  | { kind: 'message'; id: string; msg: UIMessage }
  | { kind: 'stream_block'; id: string; rendered: string; first: boolean }

interface Props {
  banner: React.ReactNode
  messages: UIMessage[]
  frozenStreamBlocks: Array<{ id: string; rendered: string }>
  verbose: boolean
}

export function MessageHistory({ banner, messages, frozenStreamBlocks, verbose }: Props) {
  const items: StaticItem[] = [
    { kind: 'banner', id: '__banner__', node: banner },
    ...messages.map((msg) => ({ kind: 'message' as const, id: msg.id, msg })),
    ...frozenStreamBlocks.map((block, i) => ({
      kind: 'stream_block' as const,
      id: block.id,
      rendered: block.rendered,
      first: i === 0 && frozenStreamBlocks.length > 0,
    })),
  ]

  return (
    <Static items={items}>
      {(item) => {
        if (item.kind === 'banner') {
          return <React.Fragment key={item.id}>{item.node}</React.Fragment>
        }
        if (item.kind === 'stream_block') {
          return (
            <Box key={item.id} marginTop={item.first ? 1 : 0}>
              {item.first && <Text color="magenta" bold>{'⏺ '}</Text>}
              <Box flexDirection="column" flexShrink={1}>
                <Text>{item.rendered.replace(/^\n+/, '')}</Text>
              </Box>
            </Box>
          )
        }
        const msg = item.msg
        return (
          <React.Fragment key={item.id}>
            {verbose && msg.verboseEvents?.map((evt, i) => (
              <VerboseEventLine key={`${item.id}-evt-${i}`} event={evt} />
            ))}
            <Message message={msg} />
            {verbose && msg.runStats && (
              <RunSummary stats={msg.runStats} />
            )}
          </React.Fragment>
        )
      }}
    </Static>
  )
}

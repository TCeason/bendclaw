/**
 * Banner — startup header displayed at the top of the REPL.
 *
 * Delegates to the chalk-based `renderBanner` from `term/banner.ts`
 * which produces pixel-perfect box-drawing output for all layout modes.
 */

import React from 'react'
import { Text, useStdout } from 'ink'
import { type SessionMeta } from '../native/index.js'
import { renderBanner } from '../term/banner.js'

export interface BannerProps {
  model: string
  cwd: string
  sessionId: string | null
  configInfo?: import('../native/index.js').ConfigInfo
  recentSessions?: SessionMeta[]
  serverState?: import('../repl/server.js').ServerState | null
}

export function Banner({ model, cwd, configInfo, recentSessions, serverState }: BannerProps) {
  const { stdout } = useStdout()
  const columns = stdout?.columns ?? 80

  const server = serverState
    ? { port: serverState.port, address: serverState.address, channels: serverState.channels }
    : null

  const text = renderBanner(model, cwd, configInfo, recentSessions ?? [], columns, server)

  return <Text>{text}</Text>
}

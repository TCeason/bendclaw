#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 */

import React from 'react'
import { render } from 'ink'
import { startServer } from './native/index.js'
import { REPL } from './repl/REPL.js'
import { createAgent, parseArgs } from './cli.js'
import { runPrompt } from './prompt.js'

async function main() {
  const opts = parseArgs(process.argv.slice(2))

  switch (opts.command) {
    case 'serve':
      await startServer(opts.port, opts.model)
      break

    case 'prompt':
      await runPrompt(opts)
      break

    case 'repl':
    default: {
      const agent = createAgent(opts)
      process.on('SIGINT', () => {})

      const { waitUntilExit } = render(React.createElement(REPL, {
        agent,
        initialVerbose: opts.verbose,
        initialResume: opts.resume,
      }), {
        exitOnCtrlC: false,
      })
      waitUntilExit().then(() => {
        process.exit(0)
      })
      break
    }
  }
}

main()

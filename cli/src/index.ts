#!/usr/bin/env bun
/**
 * evot CLI — TypeScript entry point.
 */

import { startServer } from './native/index.js'
import { createAgent, parseArgs } from './cli.js'
import { runPrompt } from './prompt.js'

async function main() {
  const opts = await parseArgs(process.argv.slice(2))

  switch (opts.command) {
    case 'serve':
      await startServer(opts.port, opts.model, opts.envFile)
      break

    case 'prompt':
      await runPrompt(opts)
      break

    case 'update': {
      const { runUpdate } = await import('./update/index.js')
      const { version } = await import('./native/index.js')
      console.log('  checking for updates...')
      const result = await runUpdate(version())
      switch (result.kind) {
        case 'up_to_date': console.log('  ✓ evot is up to date.'); break
        case 'updated': {
          console.log(`  ✓ updated ${result.from} → ${result.to}`)
          if (result.notes && result.notes.length > 0) {
            console.log('')
            console.log(`  What's new in ${result.to}:`)
            for (const note of result.notes) {
              console.log(`    • ${note}`)
            }
          }
          break
        }
        case 'error': console.error(`  ✗ ${result.message}`); process.exit(1)
      }
      break
    }

    case 'repl':
    default: {
      const agent = await createAgent(opts)
      const { startRepl } = await import('./term/repl.js')
      await startRepl({
        agent,
        verbose: opts.verbose,
        resumeSessionId: opts.resume,
        serverPort: opts.port,
        envFile: opts.envFile,
      })
      break
    }
  }
}

main().catch((err: any) => {
  console.error(`Failed to initialize: ${err?.message ?? err}`)
  process.exit(1)
})

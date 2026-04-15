import { Agent } from './native/index.js'
import type { CliOptions } from './cli.js'
import { createAgent } from './cli.js'

export async function runPrompt(opts: CliOptions) {
  if (!opts.prompt) {
    console.error('No prompt provided. Use -p <text>')
    process.exit(1)
  }

  const agent: Agent = createAgent(opts)
  const stream = await agent.query(opts.prompt, opts.resume)
  for await (const event of stream) {
    if (opts.outputFormat === 'stream-json') {
      console.log(JSON.stringify(event))
    } else {
      printEventText(event)
    }
  }
  process.exit(0)
}

function printEventText(event: any) {
  switch (event.kind) {
    case 'assistant_delta':
      if (event.payload?.delta) process.stdout.write(event.payload.delta)
      break
    case 'tool_finished':
      if (event.payload?.is_error) {
        process.stderr.write(`[error: ${event.payload.tool_name}] ${event.payload.content}\n`)
      }
      break
    case 'error':
      process.stderr.write(`error: ${event.payload?.message}\n`)
      break
    case 'run_finished':
      console.log()
      break
  }
}

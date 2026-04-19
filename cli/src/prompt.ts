import { Agent } from './native/index.js'
import type { CliOptions } from './cli.js'
import { createAgent } from './cli.js'
import { loadFileBlocks } from './file-loader.js'

export async function runPrompt(opts: CliOptions) {
  if (!opts.prompt) {
    console.error('No prompt provided. Use -p <text>')
    process.exit(1)
  }

  const agent: Agent = await createAgent(opts)

  let contentJson: string | undefined
  try {
    const fileBlocks = loadFileBlocks(opts.files)
    if (fileBlocks.length > 0) {
      const blocks = [{ type: 'text', text: opts.prompt }, ...fileBlocks]
      contentJson = JSON.stringify(blocks)
    }
  } catch (err: any) {
    console.error(err.message)
    process.exit(1)
  }

  const stream = await agent.query(
    // When contentJson is present, the native layer uses it as the full input
    // and ignores the prompt parameter. We pass empty string to make this explicit.
    contentJson ? '' : opts.prompt,
    opts.resume,
    undefined,
    contentJson,
  )
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

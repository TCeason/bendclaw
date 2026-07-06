/**
 * ask_user builtin extension.
 *
 * Migrated from the former engine-side `AskUserTool`. It is now a host tool:
 * the engine advertises the spec and delegates execution here, where the
 * interactive question UI runs. The actual UI presentation is owned by the
 * REPL (via the overlay it shows on `host_tool_call`); this module supplies
 * the spec and the result formatting.
 */

import type { HostTool, HostToolResult } from '../../types.js'

export interface AskUserOption {
  label: string
  description: string
}

export interface AskUserQuestion {
  header: string
  question: string
  options: AskUserOption[]
}

export interface AskUserParams {
  questions: AskUserQuestion[]
}

/** A single answer collected from the user. */
export interface AskUserAnswer {
  header: string
  question: string
  answer: string
}

const SPEC_DESCRIPTION = `Use this tool when you need to ask the user questions during execution. This allows you to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take.

Usage notes:
- You can ask 1-4 questions in a single call; batch related questions together
- Users will always be able to select "None of the above" to provide custom text input
- If you recommend a specific option, make it the first option and add "(Recommended)" at the end of the label`

const PARAMETERS_SCHEMA = {
  type: 'object',
  properties: {
    questions: {
      type: 'array',
      minItems: 1,
      maxItems: 4,
      description: 'Questions to ask the user (1-4 questions).',
      items: {
        type: 'object',
        properties: {
          question: { type: 'string', description: "Clear, specific question ending with '?'" },
          header: { type: 'string', description: "Short tab label for this question." },
          options: {
            type: 'array',
            minItems: 2,
            maxItems: 4,
            description: "Distinct options. No 'Other' — provided automatically.",
            items: {
              type: 'object',
              properties: {
                label: { type: 'string', description: 'Concise choice (1-5 words).' },
                description: { type: 'string', description: 'Brief explanation of tradeoffs' },
              },
              required: ['label', 'description'],
            },
          },
        },
        required: ['question', 'header', 'options'],
      },
    },
  },
  required: ['questions'],
}

/**
 * Build the ask_user host tool.
 *
 * `collect` is provided by the REPL: it presents the questions interactively
 * and resolves with the answers, or `null` if the user cancelled/skipped.
 */
export function createAskUserTool(
  collect: (params: AskUserParams) => Promise<AskUserAnswer[] | null>,
): HostTool<AskUserParams> {
  return {
    spec: {
      name: 'ask_user',
      label: 'Ask User',
      description: SPEC_DESCRIPTION,
      parameters_schema: PARAMETERS_SCHEMA,
      name_aliases: [['claude', 'AskUser']],
    },
    async execute(params): Promise<HostToolResult> {
      const answers = await collect(params)
      if (!answers) {
        return {
          content: [{ type: 'text', text: 'User cancelled the question.' }],
          isError: true,
        }
      }
      const lines = ['User answered your questions:']
      for (const a of answers) lines.push(`- ${a.question} → ${a.answer}`)
      return { content: [{ type: 'text', text: lines.join('\n') }] }
    },
  }
}

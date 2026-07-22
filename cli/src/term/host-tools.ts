/** Host-owned tool wiring for the interactive REPL. */

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

export interface AskUserAnswer {
  header: string
  question: string
  answer: string
}

export interface HostToolCall {
  tool_name: string
  tool_call_id: string
  arguments: Record<string, unknown>
}

export interface HostToolResponse {
  tool_call_id: string
  content: Array<{ type: 'text'; text: string }>
  is_error: boolean
}

const ASK_USER_SPEC = {
  name: 'ask_user',
  label: 'Ask User',
  description: `Use this tool when you need to ask the user questions during execution. This allows you to:
1. Gather user preferences or requirements
2. Clarify ambiguous instructions
3. Get decisions on implementation choices as you work
4. Offer choices to the user about what direction to take.

Usage notes:
- You can ask 1-4 questions in a single call; batch related questions together
- Users will always be able to select "None of the above" to provide custom text input
- If you recommend a specific option, make it the first option and add "(Recommended)" at the end of the label`,
  parameters_schema: {
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
            header: { type: 'string', description: 'Short tab label for this question.' },
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
  },
  name_aliases: [['claude', 'AskUser']],
}

export const HOST_TOOL_SPECS_JSON = JSON.stringify([ASK_USER_SPEC])

function isAskUserName(name: string): boolean {
  const lower = name.toLowerCase()
  return lower === ASK_USER_SPEC.name || lower === 'askuser'
}

function errorResponse(call: HostToolCall, text: string): HostToolResponse {
  return {
    tool_call_id: call.tool_call_id,
    content: [{ type: 'text', text }],
    is_error: true,
  }
}

export async function dispatchHostToolCall(
  call: HostToolCall,
  collectAnswers: (params: AskUserParams) => Promise<AskUserAnswer[] | null>,
): Promise<HostToolResponse> {
  if (!isAskUserName(call.tool_name)) {
    return errorResponse(call, `Unknown host tool: ${call.tool_name}`)
  }

  try {
    const answers = await collectAnswers(call.arguments as unknown as AskUserParams)
    if (!answers) return errorResponse(call, 'User cancelled the question.')

    const lines = ['User answered your questions:']
    for (const answer of answers) lines.push(`- ${answer.question} → ${answer.answer}`)
    return {
      tool_call_id: call.tool_call_id,
      content: [{ type: 'text', text: lines.join('\n') }],
      is_error: false,
    }
  } catch (error) {
    return errorResponse(call, error instanceof Error ? error.message : String(error))
  }
}

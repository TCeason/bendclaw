import { formatSpinnerLine, type SpinnerState } from '../spinner.js'
import { line, block, plain, dim, type ViewBlock, type StyledLine } from './types.js'

const MAX_PROGRESS_LINES = 5
const MAX_PROGRESS_LINE_WIDTH = 120

export interface ActiveResponseInput {
  isLoading: boolean
  pendingText: string
  toolProgress: string
  spinner: SpinnerState
  termRows: number
}

export function buildActiveResponseBlocks(input: ActiveResponseInput): ViewBlock[] {
  if (!input.isLoading) return []

  const blocks: ViewBlock[] = []

  if (input.pendingText) {
    // Show only the last line of pending text to keep status area height stable.
    // Completed markdown blocks are committed to scroll area by the stream machine;
    // this just shows the in-progress trailing fragment.
    const lastNewline = input.pendingText.lastIndexOf('\n')
    const lastLine = lastNewline >= 0 ? input.pendingText.slice(lastNewline + 1) : input.pendingText
    if (lastLine.trim()) {
      blocks.push(block([line(plain(`  ${lastLine}`))]))
    }
  }

  if (input.toolProgress) {
    const progLines = input.toolProgress.split('\n')
    const tail = progLines
      .slice(-MAX_PROGRESS_LINES)
      .map(l => l.length > MAX_PROGRESS_LINE_WIDTH ? l.slice(0, MAX_PROGRESS_LINE_WIDTH - 1) + '…' : l)
    while (tail.length < MAX_PROGRESS_LINES) tail.unshift('')
    const styledLines: StyledLine[] = tail.map(l => line(dim(`  ${l}`)))
    blocks.push(block(styledLines, 1))
  }

  const spinnerText = formatSpinnerLine(input.spinner, Date.now())
  blocks.push(block(
    [line(plain(spinnerText))],
    input.toolProgress ? 0 : 1,
  ))

  return blocks
}

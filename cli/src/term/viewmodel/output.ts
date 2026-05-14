import type { OutputLine } from '../../render/output.js'
import { line, block, plain, dim, bold, colored, type ViewBlock, type StyledLine } from './types.js'

export interface OutputContext {
  prevKind?: string
  prevCodeBlockId?: string
}

export function buildOutputBlocks(lines: OutputLine[], context: OutputContext | string = {}): ViewBlock[] {
  const blocks: ViewBlock[] = []
  const initialContext: OutputContext = typeof context === 'string' ? { prevKind: context } : context
  let prevKind: string | undefined = initialContext.prevKind
  let prevCodeBlockId: string | undefined = initialContext.prevCodeBlockId

  for (const ol of lines) {
    let nextPrevKind: string | undefined = ol.kind
    switch (ol.kind) {
      case 'user':
        blocks.push(block([
          line(bold('❯ ', 'yellow'), bold(ol.text)),
        ], 1))
        break

      case 'assistant': {
        // Empty-text assistant lines are block-spacing separators inserted by
        // the stream machine. Continuation spacers keep the next rendered
        // assistant line in the same message, so headings in later streamed
        // chunks don't get another leading dot.
        if (!ol.text) {
          blocks.push(block([line(plain(''))]))
          nextPrevKind = ol.isContinuationSpacer ? 'assistant' : prevKind
          break   // intentionally skip normal prevKind update
        }
        const isBlockStart = prevKind !== 'assistant'
        const dot = isBlockStart ? colored('⏺ ', 'cyan') : plain('  ')
        blocks.push(block([
          line(dot, plain(ol.text)),
        ], isBlockStart ? 1 : 0))
        break
      }

      case 'code_line': {
        // Streamed code-fence line. Preserve any ANSI from syntax highlighting
        // and apply the same left padding as finalized fenced code blocks so
        // tokens don't shift when the closing fence eventually arrives. Empty
        // code_line entries are used as block separators around the fence.
        if (!ol.text) {
          blocks.push(block([line(plain(''))]))
          nextPrevKind = undefined
          prevCodeBlockId = undefined
          break
        }
        const hasCodeBlockId = ol.codeBlockId !== undefined
        const isLegacyContinuation = prevKind === 'code_line' && !hasCodeBlockId && prevCodeBlockId === undefined
        const isSameCodeBlock = prevKind === 'code_line' && hasCodeBlockId && ol.codeBlockId === prevCodeBlockId
        const isNewCodeBlock = prevKind === 'code_line' && hasCodeBlockId && prevCodeBlockId !== undefined && ol.codeBlockId !== prevCodeBlockId
        const marginTop = isLegacyContinuation || isSameCodeBlock ? 0 : isNewCodeBlock ? 2 : 1
        blocks.push(block([line(plain(`  ${ol.text}`))], marginTop))
        prevCodeBlockId = ol.codeBlockId
        break
      }

      case 'thinking': {
        blocks.push(block([
          line(dim(`${ol.text}`)),
        ], 0))
        break
      }

      case 'thinking_summary': {
        blocks.push(block([
          line(colored('[REASONING]', 'cyan', { bold: true }), colored(' ✓', 'cyan', { bold: true }), dim(` ${ol.text}`)),
        ], 1))
        break
      }

      case 'tool':
        blocks.push(buildToolBlock(ol.text))
        break

      case 'tool_result':
        blocks.push(block([line(colored(ol.text, 'gray'))]))
        break

      case 'verbose':
        blocks.push(buildVerboseBlock(ol.text))
        break

      case 'error':
        blocks.push(block([line(colored(ol.text, 'red'))]))
        break

      case 'system':
        blocks.push(block([line(dim(ol.text))]))
        break

      case 'run_summary':
        blocks.push(block([line(dim(ol.text))]))
        break

      default:
        break
    }
    if (ol.kind !== 'code_line') prevCodeBlockId = undefined
    prevKind = nextPrevKind
  }

  return blocks
}

function buildToolBlock(text: string): ViewBlock {
  const badgeMatch = text.match(/^\[([^\]]+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const statusMatch = rest.match(/^([●✓✗])\s*(.*)$/)
    const spans = [colored(`[${badge}]`, 'cyan', { bold: true })]

    if (statusMatch) {
      spans.push(colored(` ${statusMatch[1]}`, 'cyan', { bold: true }))
      const tail = statusMatch[2] ?? ''
      if (tail) spans.push(dim(tail.startsWith(' ') ? tail : ` ${tail}`))
    } else if (rest) {
      spans.push(dim(` ${rest}`))
    }

    return block([line(...spans)], 1)
  }
  if (text.startsWith('  ')) {
    const trimmed = text.trimStart()
    if (/^[{}\[\],]/.test(trimmed) || /^"[^"\\]*(?:\\.[^"\\]*)*"\s*:/.test(trimmed)) {
      return block([line(plain(text))])
    }
    return block([line(dim(text))])
  }
  return block([line(plain(text))])
}

function buildVerboseBlock(text: string): ViewBlock {
  const naturalMatch = text.match(/^([●✓✗↻])\s+(LLM|COMPACT|SPILL)\s*(.*)$/)
  if (naturalMatch) {
    const status = naturalMatch[1]!
    const badge = naturalMatch[2]!
    const rest = naturalMatch[3] ?? ''
    const color = verboseStatusColor()
    const spans = [colored(status, color, { bold: true }), colored(` ${badge}`, color, { bold: true })]
    if (rest) spans.push(dim(` ${rest}`))
    return block([line(...spans)], 1)
  }

  const badgeMatch = text.match(/^\[(\w+)\]\s*(.*)$/)
  if (badgeMatch) {
    const badge = badgeMatch[1]!
    const rest = badgeMatch[2] ?? ''
    const statusMatch = rest.match(/^([●✓✗↻])\s*(.*)$/)
    const color = verboseStatusColor()
    const spans = [colored(`[${badge}]`, color, { bold: true })]
    if (statusMatch) {
      spans.push(colored(` ${statusMatch[1]}`, color, { bold: true }))
      const tail = statusMatch[2] ?? ''
      if (tail) spans.push(dim(` ${tail}`))
    } else if (rest) {
      spans.push(dim(` ${rest}`))
    }
    return block([line(...spans)], 1)
  }
  return block([line(dim(text))])
}

function verboseStatusColor(): 'cyan' {
  return 'cyan'
}

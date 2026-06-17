import type { OutputLine } from '../../render/output.js'
import { line, block, plain, dim, bold, colored, type ViewBlock, type StyledLine } from './types.js'
import { wrapTextByWidth } from './prompt.js'

export interface OutputContext {
  prevKind?: string
  columns?: number
}

export function buildOutputBlocks(lines: OutputLine[], context: OutputContext | string = {}): ViewBlock[] {
  const blocks: ViewBlock[] = []
  const initialContext: OutputContext = typeof context === 'string' ? { prevKind: context } : context
  let prevKind: string | undefined = initialContext.prevKind

  for (const ol of lines) {
    let nextPrevKind: string | undefined = ol.kind
    switch (ol.kind) {
      case 'user': {
        const cols = initialContext.columns
        const availWidth = cols ? Math.max(1, cols - 2) : 0
        if (availWidth > 0 && ol.text.length > 0) {
          const chunks = wrapTextByWidth(ol.text, availWidth)
          const userLines = chunks.map((c, k) => {
            const prefix = k === 0 ? bold('❯ ', 'yellow') : plain('  ')
            return line(prefix, bold(ol.text.slice(c.start, c.end)))
          })
          blocks.push(block(userLines, 1))
        } else {
          blocks.push(block([
            line(bold('❯ ', 'yellow'), bold(ol.text)),
          ], 1))
        }
        break
      }

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

      case 'thinking': {
        blocks.push(block([
          line(dim(`${ol.text}`)),
        ], 0))
        break
      }

      case 'thinking_summary': {
        blocks.push(block([
          line(colored('✻', 'cyan', { bold: true }), bold(' thinking'), dim(`  · ${ol.text}`)),
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
        blocks.push(block(ol.text.split('\n').map(l => line(dim(l)))))
        break

      case 'run_summary':
        blocks.push(block([line(dim(ol.text))]))
        break

      default:
        break
    }
    prevKind = nextPrevKind
  }

  return blocks
}

function buildToolBlock(text: string): ViewBlock {
  // Goal/Todo checklist headers keep their `[BADGE]` form.
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

  // Tool call line: `<glyph> <name>  <arg>` (no status mark — status lives on
  // the subordinate line below). Paint glyph cyan, name bold, arg dim.
  const cardMatch = text.match(/^([⌘◫⌕⊕✎·✦◇]) (.+)$/u)
  if (cardMatch) {
    const glyph = cardMatch[1]!
    const rest = cardMatch[2]!.trimEnd()
    const sep = rest.indexOf('  ')
    const name = sep < 0 ? rest : rest.slice(0, sep)
    const arg = sep < 0 ? '' : rest.slice(sep + 2)
    const spans = [colored(glyph, 'cyan', { bold: true }), bold(` ${name}`)]
    if (arg) spans.push(dim(`  ${arg}`))
    return block([line(...spans)], 1)
  }

  // Subordinate status line under a call: `  ✓ · 0.6s · 2 lines` /
  // `  ✗ · exit 1` / `  ↻ · retrying…`. Paint the mark (✓ green / ✗ red /
  // ↻ yellow), the rest dim.
  const statusMatch = text.match(/^ {2}([✓✗↻])(.*)$/u)
  if (statusMatch) {
    const mark = statusMatch[1]!
    const tail = statusMatch[2] ?? ''
    const color = mark === '✗' ? 'red' : mark === '↻' ? 'yellow' : 'green'
    const spans = [colored(`  ${mark}`, color, { bold: true })]
    if (tail) spans.push(dim(tail))
    return block([line(...spans)])
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

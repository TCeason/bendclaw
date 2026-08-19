/**
 * The rounded frame drawn around the prompt editor.
 *
 * A frame owns one invariant: every row it emits is exactly `columns` wide.
 * That is what keeps the right rail in a straight line and lets a selected
 * row's background reach it. Callers get `contentWidth` from the frame rather
 * than deriving it, so content and rails cannot disagree about the budget.
 *
 * Colour comes from the active theme directly. A terminal shows one frame
 * style at a time, so threading a palette through every call site would add
 * noise without adding choice.
 */

import stringWidth from 'string-width'
import { getTheme } from '../../render/theme.js'
import { line, plain, type StyledLine, type StyledSpan } from './types.js'
import { spansWidth, truncateSpansToWidth, truncateToWidth } from './width.js'

/**
 * Below this width the side rails cost more than they earn: rails plus gutters
 * take at least 4 of very few columns, so the frame degrades to plain
 * horizontal rules and hands the full width back to content.
 */
const MIN_FRAMED_COLUMNS = 30

/**
 * Blank columns between each rail and the content, scaled to the terminal.
 * Wide terminals can spend cells on breathing room; narrow ones need every
 * column for text, so the gutter shrinks before content does. Costs no rows.
 */
const GUTTER_STEPS: readonly { minColumns: number; gutter: number }[] = [
  { minColumns: 100, gutter: 3 },
  { minColumns: 60, gutter: 2 },
  { minColumns: 0, gutter: 1 },
]

function gutterFor(columns: number): number {
  return GUTTER_STEPS.find(step => columns >= step.minColumns)?.gutter ?? 1
}

/** Columns a labelled border spends on chrome: `╭─ ` + ` ` + `╮`. */
const LABEL_CHROME_COST = 5

export interface Frame {
  /** Columns available to content inside the rails. */
  readonly contentWidth: number
  /** False on narrow terminals, where rails degrade to plain rules. */
  readonly framed: boolean
  /** Top border, optionally labelled (`╭─ ↑ 3 lines ────╮`). */
  top(label?: string): StyledLine
  /** Bottom border, optionally labelled. */
  bottom(label?: string): StyledLine
  /** Lay one content line out to the full frame width, rails included. */
  row(styled: StyledLine): StyledLine
}

export function createFrame(columns: number): Frame {
  const framed = columns >= MIN_FRAMED_COLUMNS
  const gutterWidth = framed ? gutterFor(columns) : 0
  // Two rails plus a gutter on each side.
  const contentWidth = Math.max(1, framed ? columns - 2 - gutterWidth * 2 : columns)

  const border = (label: string | undefined, left: string, right: string): StyledLine => {
    const hex = getTheme().brandHex
    if (!framed) return line({ text: plainRule(columns, label), hex })
    // `╭─ ` + label + ` ` on the left, one corner on the right.
    const lead = label ? `${left}─ ${truncateToWidth(label, Math.max(0, columns - LABEL_CHROME_COST))} ` : left
    const fill = Math.max(0, columns - stringWidth(lead) - 1)
    return line({ text: `${lead}${'─'.repeat(fill)}${right}`, hex })
  }

  return {
    contentWidth,
    framed,
    top: label => border(label, '╭', '╮'),
    bottom: label => border(label, '╰', '╯'),
    row: styled => {
      const width = spansWidth(styled.spans)
      const spans = width <= contentWidth
        ? styled.spans
        : truncateSpansToWidth(styled.spans, contentWidth)
      const padding = Math.max(0, contentWidth - Math.min(width, contentWidth))
      // A row background has to cover the padding too, otherwise the fill
      // stops at the end of the text instead of reaching the rail.
      const fill: StyledSpan[] = styled.bg
        ? [{ text: ' '.repeat(padding), bg: styled.bg }]
        : [plain(' '.repeat(padding))]

      if (!framed) {
        // No rails to reach, so padding only matters when a background has to
        // span the full width. Truncation still applies.
        if (styled.bg) return { spans: [...spans, ...fill] }
        return spans === styled.spans ? styled : { spans }
      }
      const rail = { text: '│', hex: getTheme().brandHex }
      // The gutter carries the row background so a selected row reads as one
      // continuous band from rail to rail.
      const blanks = ' '.repeat(gutterWidth)
      const gutter = styled.bg ? { text: blanks, bg: styled.bg } : plain(blanks)
      return line(rail, gutter, ...spans, ...fill, gutter, rail)
    },
  }
}

/** `── label ─────` — the degraded border, label in the same slot as framed. */
function plainRule(columns: number, label: string | undefined): string {
  if (!label) return '─'.repeat(columns)
  const lead = `── ${label} `
  return truncateToWidth(lead, columns) + '─'.repeat(Math.max(0, columns - stringWidth(lead)))
}

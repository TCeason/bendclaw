/**
 * The frame drawn around the prompt editor: a rule above, a rule below, and a
 * `>` caret leading the draft.
 *
 * There are deliberately no vertical rails. A terminal copies mouse selections
 * cell by cell, so a boxed editor puts a `│` at both ends of every line the
 * user drags across. Horizontal rules sit on their own rows and never land in
 * a selection of the text between them; the caret is one familiar character
 * on one row.
 *
 * A frame owns one invariant: every rule it emits is exactly `columns` wide,
 * and every content row indents by the same prefix width. Callers get
 * `contentWidth` from the frame rather than deriving it, so content and prefix
 * cannot disagree about the budget.
 *
 * Colour comes from the active theme directly. A terminal shows one frame
 * style at a time, so threading a palette through every call site would add
 * noise without adding choice.
 */

import stringWidth from 'string-width'
import { getTheme } from '../../render/theme/index.js'
import { atLeastHeight, atLeastWidth, heightTier, widthTier } from './breakpoints.js'
import { line, plain, type StyledLine, type StyledSpan } from './types.js'
import { spansWidth, truncateSpansToWidth, truncateToWidth } from './width.js'

/** The caret that leads the draft, and the blank indent every other row keeps. */
export const PROMPT_CARET = '>'
/** `> ` — two columns every framed row spends before content. */
const PREFIX_WIDTH = 2

export interface Frame {
  /** Columns available to content after the prefix. */
  readonly contentWidth: number
  /** False when the terminal is too narrow to spend columns on a caret. */
  readonly framed: boolean
  /**
   * False on very short terminals, where even a horizontal rule is a row the
   * transcript needs more. Callers skip the rule rows entirely.
   */
  readonly ruled: boolean
  /** Top rule, optionally labelled (`── ↑ 3 lines ────`). */
  top(label?: string): StyledLine
  /** Bottom rule, optionally labelled. */
  bottom(label?: string): StyledLine
  /**
   * Lay one content line out behind the prefix. `caret` puts the `>` there;
   * every other row indents by the same width so text lines up underneath.
   */
  row(styled: StyledLine, caret?: boolean): StyledLine
}

export interface FrameOptions {
  /** Terminal rows. Short terminals drop the rules to keep the transcript. */
  rows?: number
  /** Rule and caret hue; defaults to the brand colour. Input modes recolour it. */
  hex?: string
}

export function createFrame(columns: number, options: FrameOptions = {}): Frame {
  const { rows, hex } = options
  // Two chrome rows are affordable down to 10 rows. Below that a framed
  // composer plus footer claims the whole screen, so the rules go.
  const ruled = rows === undefined || atLeastHeight(heightTier(rows), 'sm')
  const framed = ruled && atLeastWidth(widthTier(columns), 'sm')
  const contentWidth = Math.max(1, framed ? columns - PREFIX_WIDTH : columns)
  const accentHex = () => hex ?? getTheme().brandHex

  const rule = (label: string | undefined): StyledLine => line({ text: plainRule(columns, label), hex: accentHex() })

  return {
    contentWidth,
    framed,
    ruled,
    top: rule,
    bottom: rule,
    row: (styled, caret = false) => {
      const width = spansWidth(styled.spans)
      const spans = width <= contentWidth
        ? styled.spans
        : truncateSpansToWidth(styled.spans, contentWidth)
      // A row background has to cover the padding too, otherwise the band
      // stops at the end of the text instead of reaching the right edge.
      // Without a background there is nothing to pad for, and trailing blanks
      // would only end up in a mouse selection.
      const padding = Math.max(0, contentWidth - Math.min(width, contentWidth))
      const fill: StyledSpan[] = styled.bg ? [{ text: ' '.repeat(padding), bg: styled.bg }] : []

      if (!framed) {
        if (styled.bg) return { spans: [...spans, ...fill] }
        return spans === styled.spans ? styled : { spans }
      }
      // A blank spacer row stays genuinely empty: an indent of two spaces
      // is invisible on screen and only shows up on the clipboard.
      if (width === 0 && !styled.bg && !caret) return line()
      // The prefix carries the row background so a selected row reads as one
      // continuous band from the left edge.
      const prefix: StyledSpan = caret
        ? { text: `${PROMPT_CARET} `, hex: accentHex(), ...(styled.bg ? { bg: styled.bg } : {}) }
        : styled.bg ? { text: ' '.repeat(PREFIX_WIDTH), bg: styled.bg } : plain(' '.repeat(PREFIX_WIDTH))
      return line(prefix, ...spans, ...fill)
    },
  }
}

/** `── label ─────` — a full-width rule, label leading when present. */
function plainRule(columns: number, label: string | undefined): string {
  if (!label) return '─'.repeat(columns)
  const lead = `── ${label} `
  return truncateToWidth(lead, columns) + '─'.repeat(Math.max(0, columns - stringWidth(lead)))
}

import { wrapTextWithAnsi } from '../render/wrap.js'
import { PREVIEW_ALERT_PREFIX, PREVIEW_SECTION_PREFIX } from './selector.js'

/** Shared by keyboard handling and rendering: content never sets window size. */
export function previewGeometry(columns: number, rows: number, fraction: number) {
  const available = Math.max(1, Math.floor(Number.isFinite(columns) ? columns : 80) - 1)
  const terminalRows = Math.max(1, Math.floor(Number.isFinite(rows) ? rows : 24))
  const preferred = Math.min(available - 46 - 4, Math.max(24, Math.floor(available * fraction)))
  const sideBySide = preferred >= 24
  const height = Math.max(3, Math.min(12, terminalRows - (sideBySide ? 10 : 15)))
  return {
    width: sideBySide ? preferred : available,
    paneWidth: sideBySide ? preferred : 0,
    height,
    listRows: sideBySide ? Math.max(1, height - 2) : 3,
  }
}

export interface PreviewRow {
  text: string
  heading: boolean
  /** The entry reported something wrong; drawn in the alert colour. */
  alert: boolean
}

/** Wrap preview entries to rows, interpreting the entry markers: the first
 *  entry and `# ` labels are headings, `! ` entries are alerts. */
export function previewRows(preview: string[], width: number): PreviewRow[] {
  return preview.flatMap((entry, index) => {
    const heading = index === 0 || entry.startsWith(PREVIEW_SECTION_PREFIX)
    const alert = entry.startsWith(PREVIEW_ALERT_PREFIX)
    const text = heading && index > 0 ? entry.slice(PREVIEW_SECTION_PREFIX.length)
      : alert ? entry.slice(PREVIEW_ALERT_PREFIX.length)
        : entry
    return wrapTextWithAnsi(text, width).map(text => ({ text, heading, alert }))
  })
}

export function previewScrollLimit(preview: string[], width: number, height: number): number {
  return Math.max(0, previewRows(preview, width).length - Math.max(1, height - 1))
}

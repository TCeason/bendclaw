/**
 * How many blank rows to insert between the transcript body and the
 * spinner/prompt footer so a short frame still ends on the last terminal row.
 *
 * When body + footer already fill (or exceed) the terminal, returns 0 and the
 * normal scroll path keeps the newest content at the bottom.
 */
export function bottomAnchorFiller(
  bodyLines: number,
  footerLines: number,
  termRows: number,
): number {
  const safeBody = Number.isFinite(bodyLines) ? Math.max(0, Math.floor(bodyLines)) : 0
  const safeFooter = Number.isFinite(footerLines) ? Math.max(0, Math.floor(footerLines)) : 0
  const safeRows = Number.isFinite(termRows) ? Math.max(0, Math.floor(termRows)) : 0
  if (safeRows === 0) return 0
  return Math.max(0, safeRows - safeBody - safeFooter)
}

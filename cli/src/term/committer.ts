import type { OutputLine } from '../render/output.js'
import { buildOutputBlocks, blocksToLines } from './viewmodel/index.js'

export interface CommitterDeps {
  compactLines: OutputLine[]
  expandedLines: OutputLine[]
  isExpanded: () => boolean
  columns: () => number
  logLines: (lines: string[]) => void
  requestRender: () => void
}

export class Committer {
  constructor(private readonly deps: CommitterDeps) {}

  contextFor(lines: OutputLine[]): { prevKind?: string; columns?: number } {
    const prev = lines.at(-1)
    return { prevKind: prev?.kind, columns: this.deps.columns() }
  }

  restore(lines: OutputLine[]): void {
    if (lines.length === 0) return
    this.deps.compactLines.push(...lines)
    this.deps.expandedLines.push(...lines)
    this.deps.requestRender()
  }

  commit(lines: OutputLine[]): void {
    if (lines.length === 0) return
    this.deps.compactLines.push(...lines)
    this.deps.expandedLines.push(...lines)
    const visible = this.deps.isExpanded()
      ? this.deps.expandedLines.slice(-lines.length)
      : lines
    this.paint(visible, this.deps.compactLines.slice(0, -lines.length))
  }

  system(id: string, text: string, kind: OutputLine['kind'] = 'system'): void {
    this.commit([{ id, kind, text }])
  }

  paint(lines: OutputLine[], contextLines: OutputLine[]): void {
    const blocks = buildOutputBlocks(lines, this.contextFor(contextLines))
    this.deps.logLines(blocksToLines(blocks))
    this.deps.requestRender()
  }
}

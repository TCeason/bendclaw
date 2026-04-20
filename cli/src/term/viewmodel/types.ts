import chalk from 'chalk'

export interface StyledSpan {
  text: string
  fg?: 'red' | 'green' | 'yellow' | 'cyan' | 'magenta' | 'gray' | 'white'
  dim?: boolean
  bold?: boolean
  inverse?: boolean
  italic?: boolean
}

export interface StyledLine {
  spans: StyledSpan[]
}

export interface ViewBlock {
  lines: StyledLine[]
  marginTop?: number
}

export function styledLineToAnsi(line: StyledLine): string {
  return line.spans.map(span => {
    let s = span.text
    if (!s) return ''

    let result = s
    if (span.fg) {
      switch (span.fg) {
        case 'red': result = chalk.red(result); break
        case 'green': result = chalk.green(result); break
        case 'yellow': result = chalk.yellow(result); break
        case 'cyan': result = chalk.cyan(result); break
        case 'magenta': result = chalk.magenta(result); break
        case 'gray': result = chalk.gray(result); break
        case 'white': result = chalk.white(result); break
      }
    }
    if (span.bold) result = chalk.bold(result)
    if (span.dim) result = chalk.dim(result)
    if (span.italic) result = chalk.italic(result)
    if (span.inverse) result = `\x1b[7m${s}\x1b[27m`
    return result
  }).join('')
}

export function blocksToLines(blocks: ViewBlock[]): string[] {
  const result: string[] = []
  for (const block of blocks) {
    if (block.marginTop) {
      for (let i = 0; i < block.marginTop; i++) result.push('')
    }
    for (const line of block.lines) {
      result.push(styledLineToAnsi(line))
    }
  }
  return result
}

export function plain(text: string): StyledSpan {
  return { text }
}

export function dim(text: string): StyledSpan {
  return { text, dim: true }
}

export function bold(text: string, fg?: StyledSpan['fg']): StyledSpan {
  return { text, bold: true, fg }
}

export function colored(text: string, fg: StyledSpan['fg'], opts?: { bold?: boolean; dim?: boolean }): StyledSpan {
  return { text, fg, bold: opts?.bold, dim: opts?.dim }
}

export function inverse(text: string): StyledSpan {
  return { text, inverse: true }
}

export function line(...spans: StyledSpan[]): StyledLine {
  return { spans }
}

export function block(lines: StyledLine[], marginTop?: number): ViewBlock {
  return { lines, marginTop }
}

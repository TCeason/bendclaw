/**
 * Bracketed paste stream transformer.
 *
 * Strips bracketed paste markers (\x1b[200~ / \x1b[201~) from stdin
 * and wraps paste content with a custom marker that parseInput can recognize.
 */

import { Transform, type TransformCallback } from 'stream'

const BP_OPEN = '\x1b[200~'
const BP_CLOSE = '\x1b[201~'

// Custom marker: use a private-use Unicode character unlikely to appear in real input
const PASTE_START = '\x1b[_PASTE_START_\x1b\\'
const PASTE_END = '\x1b[_PASTE_END_\x1b\\'

export class BracketedPasteTransform extends Transform {
  private inPaste = false
  private pasteContent = ''
  private onEmptyPaste: (() => void) | undefined

  constructor(onEmptyPaste?: () => void) {
    super()
    this.onEmptyPaste = onEmptyPaste
  }

  _transform(chunk: Buffer, _encoding: string, callback: TransformCallback): void {
    let str = chunk.toString('utf8')

    while (true) {
      if (!this.inPaste) {
        const openIdx = str.indexOf(BP_OPEN)
        if (openIdx === -1) {
          // No marker — pass through
          if (str.length > 0) this.push(Buffer.from(str, 'utf8'))
          break
        }
        // Push content before the marker
        if (openIdx > 0) this.push(Buffer.from(str.slice(0, openIdx), 'utf8'))
        str = str.slice(openIdx + BP_OPEN.length)
        this.inPaste = true
        this.pasteContent = ''
      }

      // Inside a paste — look for close marker
      const closeIdx = str.indexOf(BP_CLOSE)
      if (closeIdx === -1) {
        // No close yet — accumulate
        this.pasteContent += str
        break
      }

      this.pasteContent += str.slice(0, closeIdx)
      str = str.slice(closeIdx + BP_CLOSE.length)
      this.inPaste = false

      // Forward paste content wrapped with custom markers
      if (this.pasteContent.length > 0) {
        this.push(Buffer.from(PASTE_START + this.pasteContent + PASTE_END, 'utf8'))
        this.pasteContent = ''
      } else {
        // Empty paste — likely Cmd+V with image in clipboard
        this.onEmptyPaste?.()
      }
    }

    callback()
  }
}

/**
 * Install bracketed paste transform on stdin.
 * Returns the transformed stream and a cleanup function.
 */
export function installBracketedPaste(
  stdin: NodeJS.ReadStream,
  onEmptyPaste?: () => void,
): { stream: NodeJS.ReadStream; cleanup: () => void } {
  const transform = new BracketedPasteTransform(onEmptyPaste)
  stdin.pipe(transform)

  // Cast to ReadStream for compatibility
  const stream = transform as unknown as NodeJS.ReadStream

  const cleanup = () => {
    stdin.unpipe(transform)
  }

  return { stream, cleanup }
}

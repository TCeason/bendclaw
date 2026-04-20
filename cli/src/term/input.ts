/**
 * Input — raw mode stdin handler for the terminal REPL.
 * Parses keypresses and emits structured events.
 */

export type KeyEvent =
  | { type: 'char'; char: string }
  | { type: 'enter' }
  | { type: 'alt-enter' }
  | { type: 'backspace' }
  | { type: 'delete' }
  | { type: 'tab' }
  | { type: 'escape' }
  | { type: 'up' }
  | { type: 'down' }
  | { type: 'left' }
  | { type: 'right' }
  | { type: 'home' }
  | { type: 'end' }
  | { type: 'ctrl'; key: string }
  | { type: 'paste'; text: string }

export type KeyHandler = (event: KeyEvent) => void

/**
 * Parse a raw stdin buffer into key events.
 * Handles common ANSI escape sequences.
 */
export function parseInput(data: Buffer): KeyEvent[] {
  const events: KeyEvent[] = []
  const str = data.toString('utf-8')
  let i = 0

  while (i < str.length) {
    const ch = str[i]!

    // Ctrl+letter (0x01-0x1a except special ones)
    if (ch.charCodeAt(0) >= 1 && ch.charCodeAt(0) <= 26) {
      const code = ch.charCodeAt(0)
      switch (code) {
        case 3: // Ctrl+C
          events.push({ type: 'ctrl', key: 'c' })
          break
        case 4: // Ctrl+D
          events.push({ type: 'ctrl', key: 'd' })
          break
        case 9: // Tab
          events.push({ type: 'tab' })
          break
        case 12: // Ctrl+L
          events.push({ type: 'ctrl', key: 'l' })
          break
        case 13: // Enter
          events.push({ type: 'enter' })
          break
        case 22: // Ctrl+V
          events.push({ type: 'ctrl', key: 'v' })
          break
        case 23: // Ctrl+W
          events.push({ type: 'ctrl', key: 'w' })
          break
        default:
          events.push({ type: 'ctrl', key: String.fromCharCode(code + 96) })
      }
      i++
      continue
    }

    // Escape sequences
    if (ch === '\x1b') {
      // Check for custom paste markers from BracketedPasteTransform
      const startMarker = '\x1b[_PASTE_START_\x1b\\'
      if (str.slice(i, i + startMarker.length) === startMarker) {
        const contentStart = i + startMarker.length
        const endMarker = '\x1b[_PASTE_END_\x1b\\'
        const endIdx = str.indexOf(endMarker, contentStart)
        if (endIdx !== -1) {
          const text = str.slice(contentStart, endIdx)
          events.push({ type: 'paste', text })
          i = endIdx + endMarker.length
          continue
        }
      }

      // Check for raw bracketed paste sequences (fallback if no transform)
      const BP_OPEN = '\x1b[200~'
      const BP_CLOSE = '\x1b[201~'
      if (str.slice(i, i + BP_OPEN.length) === BP_OPEN) {
        const contentStart = i + BP_OPEN.length
        const endIdx = str.indexOf(BP_CLOSE, contentStart)
        if (endIdx !== -1) {
          const text = str.slice(contentStart, endIdx)
          if (text.length > 0) {
            events.push({ type: 'paste', text })
          }
          i = endIdx + BP_CLOSE.length
          continue
        }
      }

      if (i + 1 < str.length && str[i + 1] === '[') {
        // CSI sequence
        const rest = str.slice(i + 2)
        if (rest.startsWith('A')) { events.push({ type: 'up' }); i += 3; continue }
        if (rest.startsWith('B')) { events.push({ type: 'down' }); i += 3; continue }
        if (rest.startsWith('C')) { events.push({ type: 'right' }); i += 3; continue }
        if (rest.startsWith('D')) { events.push({ type: 'left' }); i += 3; continue }
        if (rest.startsWith('H')) { events.push({ type: 'home' }); i += 3; continue }
        if (rest.startsWith('F')) { events.push({ type: 'end' }); i += 3; continue }
        if (rest.startsWith('3~')) { events.push({ type: 'delete' }); i += 4; continue }
        if (rest.startsWith('1~')) { events.push({ type: 'home' }); i += 4; continue }
        if (rest.startsWith('4~')) { events.push({ type: 'end' }); i += 4; continue }
        // Skip unknown CSI sequences
        let j = 0
        while (j < rest.length && rest.charCodeAt(j) >= 0x20 && rest.charCodeAt(j) <= 0x3f) j++
        i += 2 + j + 1
        continue
      }

      // Alt+Enter: ESC followed by CR (0x0d)
      if (i + 1 < str.length && str.charCodeAt(i + 1) === 13) {
        events.push({ type: 'alt-enter' })
        i += 2
        continue
      }

      // Bare escape
      events.push({ type: 'escape' })
      i++
      continue
    }

    // Backspace (0x7f)
    if (ch === '\x7f') {
      events.push({ type: 'backspace' })
      i++
      continue
    }

    // Regular character (including multi-byte UTF-8)
    events.push({ type: 'char', char: ch })
    i++
  }

  return events
}

/**
 * Enable raw mode on stdin and return a cleanup function.
 */
export function enableRawMode(stdin: NodeJS.ReadStream): () => void {
  if (stdin.isTTY) {
    stdin.setRawMode(true)
  }
  stdin.resume()
  stdin.setEncoding('utf-8')

  return () => {
    if (stdin.isTTY) {
      stdin.setRawMode(false)
    }
    stdin.pause()
  }
}

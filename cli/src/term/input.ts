/**
 * Input — raw mode stdin handler for the terminal REPL.
 * Parses keypresses and emits structured events.
 */

export type KeyEvent =
  | { type: 'char'; char: string }
  | { type: 'shift-char'; char: string }
  | { type: 'enter' }
  | { type: 'ctrl-enter' }
  | { type: 'shift-enter' }
  | { type: 'alt-enter' }
  | { type: 'backspace' }
  | { type: 'delete' }
  | { type: 'tab' }
  | { type: 'shift-tab' }
  | { type: 'escape' }
  | { type: 'up' }
  | { type: 'down' }
  | { type: 'left' }
  | { type: 'right' }
  | { type: 'home' }
  | { type: 'end' }
  | { type: 'page-up' }
  | { type: 'page-down' }
  | { type: 'ctrl'; key: string }
  | { type: 'paste'; text: string }

export type KeyHandler = (event: KeyEvent) => void

export type TerminalControlEvent =
  | { type: 'kitty-flags'; flags: number }
  | { type: 'device-attributes' }

export interface EnhancedKeyboardOptions {
  negotiationTimeoutMs?: number
}

export interface EnhancedKeyboardSession {
  /** Backward-compatible cleanup call. */
  (): void
  handleControl(event: TerminalControlEvent): void
  dispose(): void
}

const MOD_SHIFT = 1
const MOD_ALT = 2
const MOD_CTRL = 4
const KITTY_KP_ENTER = 57414
const ENABLE_KITTY_DISAMBIGUATE = '\x1b[>1u'
const QUERY_KITTY_KEYBOARD = '\x1b[?u'
const QUERY_PRIMARY_DEVICE_ATTRIBUTES = '\x1b[c'
const DISABLE_KITTY_KEYBOARD = '\x1b[<u'
const ENABLE_MODIFY_OTHER_KEYS = '\x1b[>4;2m'
const DISABLE_MODIFY_OTHER_KEYS = '\x1b[>4;0m'
const DEFAULT_KEYBOARD_NEGOTIATION_TIMEOUT_MS = 150

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

    // Ctrl+letter (0x01-0x1a).
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
        case 10: // LF. Some terminals map Shift+Enter to a literal line feed.
          events.push({ type: 'shift-enter' })
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
      // Check for raw bracketed paste sequences. TerminalInputBuffer normally
      // reassembles these first; this remains useful for direct parser callers.
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

        const csiU = parseCsiU(rest)
        if (csiU) {
          if (csiU.event) events.push(csiU.event)
          i += 2 + csiU.length
          continue
        }

        const modifyOtherKeys = parseModifyOtherKeys(rest)
        if (modifyOtherKeys) {
          if (modifyOtherKeys.event) events.push(modifyOtherKeys.event)
          i += 2 + modifyOtherKeys.length
          continue
        }

        const modifiedCursor = parseModifiedCursor(rest)
        if (modifiedCursor) {
          if (modifiedCursor.event) events.push(modifiedCursor.event)
          i += 2 + modifiedCursor.length
          continue
        }

        if (rest.startsWith('A')) { events.push({ type: 'up' }); i += 3; continue }
        if (rest.startsWith('B')) { events.push({ type: 'down' }); i += 3; continue }
        if (rest.startsWith('C')) { events.push({ type: 'right' }); i += 3; continue }
        if (rest.startsWith('D')) { events.push({ type: 'left' }); i += 3; continue }
        if (rest.startsWith('H')) { events.push({ type: 'home' }); i += 3; continue }
        if (rest.startsWith('F')) { events.push({ type: 'end' }); i += 3; continue }
        if (rest.startsWith('Z')) { events.push({ type: 'shift-tab' }); i += 3; continue }
        if (rest.startsWith('13;2~')) { events.push({ type: 'shift-enter' }); i += 7; continue }
        if (rest.startsWith('3~')) { events.push({ type: 'delete' }); i += 4; continue }
        if (rest.startsWith('5~')) { events.push({ type: 'page-up' }); i += 4; continue }
        if (rest.startsWith('6~')) { events.push({ type: 'page-down' }); i += 4; continue }
        if (rest.startsWith('1~')) { events.push({ type: 'home' }); i += 4; continue }
        if (rest.startsWith('4~')) { events.push({ type: 'end' }); i += 4; continue }
        // Skip unknown CSI sequences
        let j = 0
        while (j < rest.length && rest.charCodeAt(j) >= 0x20 && rest.charCodeAt(j) <= 0x3f) j++
        i += 2 + j + 1
        continue
      }

      // Alt+Enter: ESC followed by CR (0x0d). Some terminals also use this
      // as a custom Shift+Enter mapping; both mean "insert newline" in evot.
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

    // Regular character. Advance by Unicode code point so non-BMP input is not
    // split into separate surrogate events; grapheme grouping happens in the editor.
    const codePoint = str.codePointAt(i)
    const char = codePoint === undefined ? ch : String.fromCodePoint(codePoint)
    events.push({ type: 'char', char })
    i += char.length
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

export function parseTerminalControlSequence(sequence: string): TerminalControlEvent | undefined {
  const kittyFlags = sequence.match(/^\x1b\[\?(\d+)u$/)
  if (kittyFlags) {
    return { type: 'kitty-flags', flags: Number.parseInt(kittyFlags[1]!, 10) }
  }
  if (/^\x1b\[\?[\d;]*c$/.test(sequence)) return { type: 'device-attributes' }
  return undefined
}

/**
 * Negotiate one enhanced-keyboard protocol. Kitty is requested first; terminals
 * without support answer the DA sentinel (or time out), then fall back to
 * modifyOtherKeys. The returned session consumes negotiation responses and
 * restores only the mode that was actually active.
 */
export function enableEnhancedKeyboard(
  stdout: NodeJS.WriteStream = process.stdout,
  options: EnhancedKeyboardOptions = {},
): EnhancedKeyboardSession {
  type Mode = 'negotiating' | 'kitty' | 'modify-other-keys' | 'disposed'
  let mode: Mode = 'negotiating'
  const timeoutMs = options.negotiationTimeoutMs ?? DEFAULT_KEYBOARD_NEGOTIATION_TIMEOUT_MS
  let timer: ReturnType<typeof setTimeout> | undefined

  const clearTimer = () => {
    if (!timer) return
    clearTimeout(timer)
    timer = undefined
  }
  const useModifyOtherKeys = () => {
    if (mode !== 'negotiating') return
    clearTimer()
    // The Kitty push may already have changed terminal state even if its query
    // response was lost. Pop it before enabling the xterm fallback.
    stdout.write(DISABLE_KITTY_KEYBOARD)
    stdout.write(ENABLE_MODIFY_OTHER_KEYS)
    mode = 'modify-other-keys'
  }

  // Request Kitty flags, query the result, then issue DA as a widely-supported
  // sentinel. Supporting terminals answer Kitty before DA; others answer DA.
  stdout.write(`${ENABLE_KITTY_DISAMBIGUATE}${QUERY_KITTY_KEYBOARD}${QUERY_PRIMARY_DEVICE_ATTRIBUTES}`)
  timer = setTimeout(useModifyOtherKeys, Math.max(0, timeoutMs))

  const handleControl = (event: TerminalControlEvent) => {
    if (mode !== 'negotiating') return
    if (event.type === 'kitty-flags' && event.flags > 0) {
      clearTimer()
      mode = 'kitty'
      return
    }
    useModifyOtherKeys()
  }
  const dispose = () => {
    if (mode === 'disposed') return
    clearTimer()
    if (mode === 'kitty' || mode === 'negotiating') stdout.write(DISABLE_KITTY_KEYBOARD)
    else if (mode === 'modify-other-keys') stdout.write(DISABLE_MODIFY_OTHER_KEYS)
    mode = 'disposed'
  }

  return Object.assign(dispose, { handleControl, dispose })
}

interface ParsedSequence {
  length: number
  event?: KeyEvent
}

function parseCsiU(rest: string): ParsedSequence | null {
  // Kitty CSI-u:
  //   ESC [ <codepoint> u
  //   ESC [ <codepoint> ; <modifier> u
  //   ESC [ <codepoint> ; <modifier> : <event> u
  // Modifiers are 1-indexed, so 2 means Shift and 5 means Ctrl.
  const match = rest.match(/^(\d+)(?::\d*)?(?::\d+)?(?:;(\d+))?(?::(\d+))?u/)
  if (!match) return null
  const codepoint = Number.parseInt(match[1]!, 10)
  const modValue = match[2] ? Number.parseInt(match[2], 10) : 1
  const modifier = modValue - 1
  const eventType = match[3] ? Number.parseInt(match[3], 10) : 1
  const event = eventType === 3 ? undefined : keyEventFromCodepoint(codepoint, modifier)
  return { length: match[0].length, event }
}

function parseModifyOtherKeys(rest: string): ParsedSequence | null {
  // xterm modifyOtherKeys: ESC [ 27 ; <modifier> ; <keycode> ~
  const match = rest.match(/^27;(\d+);(\d+)~/)
  if (!match) return null
  const modValue = Number.parseInt(match[1]!, 10)
  const codepoint = Number.parseInt(match[2]!, 10)
  return { length: match[0].length, event: keyEventFromCodepoint(codepoint, modValue - 1) }
}

function parseModifiedCursor(rest: string): ParsedSequence | null {
  // Modified arrows: ESC [ 1 ; <modifier> A/B/C/D
  // Modified Home/End: ESC [ 1 ; <modifier> H/F
  const match = rest.match(/^1;\d+(?::(\d+))?([ABCDFH])/)
  if (!match) return null
  const eventType = match[1] ? Number.parseInt(match[1], 10) : 1
  if (eventType === 3) return { length: match[0].length }
  const final = match[2]!
  const eventByFinal: Record<string, KeyEvent> = {
    A: { type: 'up' },
    B: { type: 'down' },
    C: { type: 'right' },
    D: { type: 'left' },
    F: { type: 'end' },
    H: { type: 'home' },
  }
  return { length: match[0].length, event: eventByFinal[final] }
}

function keyEventFromCodepoint(codepoint: number, modifier: number): KeyEvent | undefined {
  const normalizedModifier = modifier & ~(64 + 128) // ignore caps/num lock bits
  if (codepoint === 13 || codepoint === KITTY_KP_ENTER) {
    if ((normalizedModifier & MOD_CTRL) !== 0) return { type: 'ctrl-enter' }
    if ((normalizedModifier & MOD_SHIFT) !== 0) return { type: 'shift-enter' }
    if ((normalizedModifier & MOD_ALT) !== 0) return { type: 'alt-enter' }
    return { type: 'enter' }
  }
  if (codepoint === 9) {
    if ((normalizedModifier & MOD_SHIFT) !== 0) return { type: 'shift-tab' }
    return { type: 'tab' }
  }
  if (codepoint === 127) return { type: 'backspace' }
  if (codepoint === 27) return { type: 'escape' }
  if ((normalizedModifier & MOD_CTRL) !== 0) {
    const key = modifiedKeyFromCodepoint(codepoint)
    if (key) return { type: 'ctrl', key }
  }
  if (normalizedModifier === MOD_SHIFT && codepoint >= 0x20 && codepoint <= 0x10ffff) {
    return { type: 'shift-char', char: String.fromCodePoint(codepoint).toLowerCase() }
  }
  if (normalizedModifier === 0 && codepoint >= 0x20 && codepoint <= 0x10ffff) {
    const char = String.fromCodePoint(codepoint)
    return { type: 'char', char }
  }
  return undefined
}

function modifiedKeyFromCodepoint(codepoint: number): string | undefined {
  if (codepoint >= 65 && codepoint <= 90) return String.fromCharCode(codepoint + 32)
  if (codepoint >= 97 && codepoint <= 122) return String.fromCharCode(codepoint)
  if (codepoint === 91) return '['
  if (codepoint === 92) return '\\'
  if (codepoint === 93) return ']'
  if (codepoint === 95 || codepoint === 45) return '-'
  return undefined
}

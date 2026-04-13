/**
 * PromptInput component — Claude Code-style bordered input box.
 * 
 * Layout:
 *   ────────────────────────────────────
 *   ❯ user input text here
 *   ────────────────────────────────────
 *   ? for shortcuts                model
 */

import React, { useState, useRef, useEffect } from 'react'
import { Text, Box, useInput, useStdout } from 'ink'

interface PromptInputProps {
  model: string
  isLoading: boolean
  verbose: boolean
  onSubmit: (text: string) => void
  onInterrupt: () => void
  onToggleVerbose: () => void
}

export function PromptInput({
  model,
  isLoading,
  verbose,
  onSubmit,
  onInterrupt,
  onToggleVerbose,
}: PromptInputProps) {
  const [lines, setLines] = useState<string[]>([''])
  const [cursorLine, setCursorLine] = useState(0)
  const [cursorCol, setCursorCol] = useState(0)
  const historyRef = useRef<string[]>([])
  const historyIndexRef = useRef(-1)
  const savedInputRef = useRef('')
  const { stdout } = useStdout()
  const columns = stdout?.columns ?? 120

  const currentText = () => lines.join('\n')
  const setInputText = (text: string) => {
    const newLines = text.split('\n')
    setLines(newLines)
    const lastLine = newLines.length - 1
    setCursorLine(lastLine)
    setCursorCol(newLines[lastLine]!.length)
  }

  const clearInput = () => {
    setLines([''])
    setCursorLine(0)
    setCursorCol(0)
    historyIndexRef.current = -1
  }

  useInput((ch, key) => {
    if (isLoading) {
      if (key.ctrl && ch === 'c') {
        onInterrupt()
      }
      return
    }

    // Ctrl+C — clear input or exit
    if (key.ctrl && ch === 'c') {
      if (currentText().length === 0) {
        onInterrupt()
      } else {
        clearInput()
      }
      return
    }

    // Ctrl+L — toggle verbose
    if (key.ctrl && ch === 'l') {
      onToggleVerbose()
      return
    }

    // Ctrl+U — clear line before cursor
    if (key.ctrl && ch === 'u') {
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = newLines[cursorLine]!.slice(cursorCol)
        return newLines
      })
      setCursorCol(0)
      return
    }

    // Ctrl+K — clear line after cursor
    if (key.ctrl && ch === 'k') {
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = newLines[cursorLine]!.slice(0, cursorCol)
        return newLines
      })
      return
    }

    // Ctrl+A — move to start of line
    if (key.ctrl && ch === 'a') {
      setCursorCol(0)
      return
    }

    // Ctrl+E — move to end of line
    if (key.ctrl && ch === 'e') {
      setCursorCol(lines[cursorLine]!.length)
      return
    }

    // Ctrl+D — delete char at cursor (or exit if empty)
    if (key.ctrl && ch === 'd') {
      const line = lines[cursorLine]!
      if (currentText().length === 0) {
        onInterrupt()
        return
      }
      if (cursorCol < line.length) {
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine] = line.slice(0, cursorCol) + line.slice(cursorCol + 1)
          return newLines
        })
      } else if (cursorLine < lines.length - 1) {
        // Join with next line
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine] = newLines[cursorLine]! + newLines[cursorLine + 1]!
          newLines.splice(cursorLine + 1, 1)
          return newLines
        })
      }
      return
    }

    // Ctrl+W — delete word before cursor
    if (key.ctrl && ch === 'w') {
      const line = lines[cursorLine]!
      const before = line.slice(0, cursorCol)
      const trimmed = before.replace(/\s+\S*$/, '')
      const newCol = trimmed.length
      setLines((prev) => {
        const newLines = [...prev]
        newLines[cursorLine] = trimmed + line.slice(cursorCol)
        return newLines
      })
      setCursorCol(newCol)
      return
    }

    // Enter — submit (single line) or newline (if Alt/Option+Enter)
    if (key.return) {
      if (key.meta) {
        // Alt+Enter → insert newline
        setLines((prev) => {
          const line = prev[cursorLine]!
          const newLines = [...prev]
          newLines.splice(cursorLine, 1, line.slice(0, cursorCol), line.slice(cursorCol))
          return newLines
        })
        setCursorLine((prev) => prev + 1)
        setCursorCol(0)
        return
      }

      const text = currentText().trim()
      if (text.length > 0) {
        // Add to history
        const history = historyRef.current
        if (history.length === 0 || history[history.length - 1] !== text) {
          history.push(text)
        }
        historyIndexRef.current = -1
        onSubmit(text)
        clearInput()
      }
      return
    }

    // Backspace
    if (key.backspace || key.delete) {
      if (cursorCol > 0) {
        setLines((prev) => {
          const newLines = [...prev]
          const line = newLines[cursorLine]!
          newLines[cursorLine] = line.slice(0, cursorCol - 1) + line.slice(cursorCol)
          return newLines
        })
        setCursorCol((prev) => prev - 1)
      } else if (cursorLine > 0) {
        // Join with previous line
        const prevLineLen = lines[cursorLine - 1]!.length
        setLines((prev) => {
          const newLines = [...prev]
          newLines[cursorLine - 1] = newLines[cursorLine - 1]! + newLines[cursorLine]!
          newLines.splice(cursorLine, 1)
          return newLines
        })
        setCursorLine((prev) => prev - 1)
        setCursorCol(prevLineLen)
      }
      return
    }

    // Arrow up — history or move cursor up
    if (key.upArrow) {
      if (lines.length === 1) {
        // Single line → navigate history
        const history = historyRef.current
        if (history.length === 0) return
        if (historyIndexRef.current === -1) {
          savedInputRef.current = currentText()
          historyIndexRef.current = history.length - 1
        } else if (historyIndexRef.current > 0) {
          historyIndexRef.current--
        }
        setInputText(history[historyIndexRef.current] ?? '')
      } else if (cursorLine > 0) {
        setCursorLine((prev) => prev - 1)
        setCursorCol((prev) => Math.min(prev, lines[cursorLine - 1]!.length))
      }
      return
    }

    // Arrow down — history or move cursor down
    if (key.downArrow) {
      if (lines.length === 1) {
        const history = historyRef.current
        if (historyIndexRef.current === -1) return
        if (historyIndexRef.current < history.length - 1) {
          historyIndexRef.current++
          setInputText(history[historyIndexRef.current] ?? '')
        } else {
          historyIndexRef.current = -1
          setInputText(savedInputRef.current)
        }
      } else if (cursorLine < lines.length - 1) {
        setCursorLine((prev) => prev + 1)
        setCursorCol((prev) => Math.min(prev, lines[cursorLine + 1]!.length))
      }
      return
    }

    // Arrow left/right
    if (key.leftArrow) {
      if (cursorCol > 0) {
        setCursorCol((prev) => prev - 1)
      } else if (cursorLine > 0) {
        setCursorLine((prev) => prev - 1)
        setCursorCol(lines[cursorLine - 1]!.length)
      }
      return
    }
    if (key.rightArrow) {
      const lineLen = lines[cursorLine]!.length
      if (cursorCol < lineLen) {
        setCursorCol((prev) => prev + 1)
      } else if (cursorLine < lines.length - 1) {
        setCursorLine((prev) => prev + 1)
        setCursorCol(0)
      }
      return
    }

    // Tab — insert 2 spaces
    if (key.tab) {
      setLines((prev) => {
        const newLines = [...prev]
        const line = newLines[cursorLine]!
        newLines[cursorLine] = line.slice(0, cursorCol) + '  ' + line.slice(cursorCol)
        return newLines
      })
      setCursorCol((prev) => prev + 2)
      return
    }

    // Ignore other control sequences
    if (key.ctrl || key.escape) return

    // Regular character input
    if (ch) {
      setLines((prev) => {
        const newLines = [...prev]
        const line = newLines[cursorLine]!
        newLines[cursorLine] = line.slice(0, cursorCol) + ch + line.slice(cursorCol)
        return newLines
      })
      setCursorCol((prev) => prev + ch.length)
    }
  })

  if (isLoading) {
    return null
  }

  const borderLine = '─'.repeat(columns)

  return (
    <Box flexDirection="column">
      {/* Top border */}
      <Text dimColor>{borderLine}</Text>

      {/* Input area */}
      {lines.map((line, lineIdx) => (
        <Box key={lineIdx}>
          <Text color="cyan" bold>{lineIdx === 0 ? '❯ ' : '  '}</Text>
          {lineIdx === cursorLine ? (
            line === '' && lines.length === 1 ? (
              // Empty input — show placeholder with cursor
              <Text>
                <Text inverse>{' '}</Text>
                <Text dimColor>Type a message...</Text>
              </Text>
            ) : (
              <CursorLine text={line} cursorCol={cursorCol} />
            )
          ) : (
            <Text>{line || ' '}</Text>
          )}
        </Box>
      ))}

      {/* Bottom border */}
      <Text dimColor>{borderLine}</Text>

      {/* Footer */}
      <Footer
        model={model}
        verbose={verbose}
        columns={columns}
      />
    </Box>
  )
}

// ---------------------------------------------------------------------------
// CursorLine — renders a line with an inverse cursor at the right position
// ---------------------------------------------------------------------------

function CursorLine({ text, cursorCol }: { text: string; cursorCol: number }) {
  const before = text.slice(0, cursorCol)
  const cursorChar = text[cursorCol] ?? ' '
  const after = text.slice(cursorCol + 1)

  return (
    <Text>
      {before}
      <Text inverse>{cursorChar}</Text>
      {after}
    </Text>
  )
}

// ---------------------------------------------------------------------------
// Footer — shortcuts hint + model name
// ---------------------------------------------------------------------------

function Footer({
  model,
  verbose,
  columns,
}: {
  model: string
  verbose: boolean
  columns: number
}) {
  const left = `Ctrl+L ${verbose ? 'brief' : 'verbose'} · /help`
  const right = model
  const gap = Math.max(1, columns - left.length - right.length)

  return (
    <Box>
      <Text dimColor>{left}</Text>
      <Text>{' '.repeat(gap)}</Text>
      <Text dimColor>{right}</Text>
    </Box>
  )
}

/**
 * PromptInput component — text input with prompt prefix.
 * Handles user input, history navigation, and submit.
 */

import React, { useState, useRef } from 'react'
import { Text, Box, useInput } from 'ink'

interface PromptInputProps {
  model: string
  isLoading: boolean
  onSubmit: (text: string) => void
  onInterrupt: () => void
}

export function PromptInput({ model, isLoading, onSubmit, onInterrupt }: PromptInputProps) {
  const [input, setInput] = useState('')
  const [cursorPos, setCursorPos] = useState(0)
  const historyRef = useRef<string[]>([])
  const historyIndexRef = useRef(-1)
  const savedInputRef = useRef('')

  useInput((ch, key) => {
    if (isLoading) {
      if (key.ctrl && ch === 'c') {
        onInterrupt()
      }
      return
    }

    // Ctrl+C — clear input or exit
    if (key.ctrl && ch === 'c') {
      if (input.length === 0) {
        onInterrupt()
      } else {
        setInput('')
        setCursorPos(0)
        historyIndexRef.current = -1
      }
      return
    }

    // Ctrl+U — clear line
    if (key.ctrl && ch === 'u') {
      setInput('')
      setCursorPos(0)
      return
    }

    // Ctrl+A — move to start
    if (key.ctrl && ch === 'a') {
      setCursorPos(0)
      return
    }

    // Ctrl+E — move to end
    if (key.ctrl && ch === 'e') {
      setCursorPos(input.length)
      return
    }

    // Submit
    if (key.return) {
      const trimmed = input.trim()
      if (trimmed.length > 0) {
        // Add to history
        const history = historyRef.current
        if (history.length === 0 || history[history.length - 1] !== trimmed) {
          history.push(trimmed)
        }
        historyIndexRef.current = -1

        onSubmit(trimmed)
        setInput('')
        setCursorPos(0)
      }
      return
    }

    // Backspace
    if (key.backspace || key.delete) {
      if (cursorPos > 0) {
        setInput((prev) => prev.slice(0, cursorPos - 1) + prev.slice(cursorPos))
        setCursorPos((prev) => prev - 1)
      }
      return
    }

    // Arrow up — history previous
    if (key.upArrow) {
      const history = historyRef.current
      if (history.length === 0) return
      if (historyIndexRef.current === -1) {
        savedInputRef.current = input
        historyIndexRef.current = history.length - 1
      } else if (historyIndexRef.current > 0) {
        historyIndexRef.current--
      }
      const entry = history[historyIndexRef.current] ?? ''
      setInput(entry)
      setCursorPos(entry.length)
      return
    }

    // Arrow down — history next
    if (key.downArrow) {
      const history = historyRef.current
      if (historyIndexRef.current === -1) return
      if (historyIndexRef.current < history.length - 1) {
        historyIndexRef.current++
        const entry = history[historyIndexRef.current] ?? ''
        setInput(entry)
        setCursorPos(entry.length)
      } else {
        historyIndexRef.current = -1
        setInput(savedInputRef.current)
        setCursorPos(savedInputRef.current.length)
      }
      return
    }

    // Arrow left/right
    if (key.leftArrow) {
      setCursorPos((prev) => Math.max(0, prev - 1))
      return
    }
    if (key.rightArrow) {
      setCursorPos((prev) => Math.min(input.length, prev + 1))
      return
    }

    // Ignore other control sequences
    if (key.ctrl || key.meta || key.escape) return
    if (key.tab) return

    // Regular character input
    if (ch) {
      setInput((prev) => prev.slice(0, cursorPos) + ch + prev.slice(cursorPos))
      setCursorPos((prev) => prev + ch.length)
    }
  })

  if (isLoading) {
    return null
  }

  // Render input with cursor
  const before = input.slice(0, cursorPos)
  const cursor = input[cursorPos] ?? ' '
  const after = input.slice(cursorPos + 1)

  return (
    <Box>
      <Text backgroundColor="#5a2d82" color="white" bold>
        {' bendclaw '}
      </Text>
      <Text dimColor> {model} </Text>
      <Text bold color="yellow">{'> '}</Text>
      <Text>{before}</Text>
      <Text inverse>{cursor}</Text>
      <Text>{after}</Text>
    </Box>
  )
}

import { line, block, plain, dim, bold, colored, inverse, type ViewBlock, type StyledSpan } from './types.js'
import type { SelectorState } from '../selector.js'
import type { AskState } from '../ask.js'

export type OverlayState =
  | { kind: 'none' }
  | { kind: 'help' }
  | { kind: 'selector'; state: SelectorState }
  | { kind: 'ask-user'; state: AskState }

export function buildOverlayBlocks(overlay: OverlayState, columns: number): ViewBlock[] {
  switch (overlay.kind) {
    case 'none':
      return []
    case 'help':
      return buildHelpBlocks(columns)
    case 'selector':
      return buildSelectorBlocks(overlay.state)
    case 'ask-user':
      return buildAskBlocks(overlay.state)
  }
}

function buildHelpBlocks(columns: number): ViewBlock[] {
  const entries = [
    ['Enter', 'Submit message'],
    ['Alt+Enter', 'Insert newline'],
    ['Ctrl+C', 'Clear / Exit (×2)'],
    ['Esc', 'Clear input / Dismiss / Interrupt'],
    ['↑ / ↓', 'History navigation / multi-line'],
    ['Tab', 'Complete command / path'],
    ['Ctrl+U', 'Clear line before cursor'],
    ['Ctrl+K', 'Clear line after cursor'],
    ['Ctrl+W', 'Delete word before cursor'],
    ['Ctrl+D', 'Delete char / Exit if empty'],
    ['Ctrl+A/E', 'Move to start/end of line'],
    ['Ctrl+L', 'Clear all input'],
    ['Ctrl+O', 'Toggle verbose mode'],
    ['/help', 'Show this help'],
    ['/model <name>', 'Switch model'],
    ['/resume [id]', 'Resume session'],
    ['/new', 'Start new session'],
    ['/goto <n>', 'Go to message'],
    ['/history [n]', 'Show recent messages'],
    ['/compact', 'Compact context'],
    ['/plan', 'Toggle planning mode'],
    ['/env', 'Manage variables'],
    ['/skill', 'Manage skills'],
    ['/update', 'Update evot'],
    ['/verbose', 'Toggle verbose mode'],
    ['/clear', 'Clear screen'],
    ['/exit', 'Exit'],
  ]

  const maxKeyLen = Math.max(...entries.map(e => e[0]!.length))
  const lines = [
    line(bold('  Keyboard Shortcuts & Commands')),
    line(plain('')),
    ...entries.map(([key, desc]) =>
      line(colored(`  ${key!.padEnd(maxKeyLen + 2)}`, 'cyan'), dim(desc!))
    ),
    line(plain('')),
    line(dim('  Press Esc to dismiss')),
  ]

  return [block(lines, 1)]
}

function buildSelectorBlocks(state: SelectorState): ViewBlock[] {
  const lines = [
    line(bold(state.title)),
  ]

  if (state.query) {
    lines.push(line(dim('  search: '), plain(state.query)))
  }

  lines.push(line(plain('')))

  if (state.items.length === 0) {
    lines.push(line(dim('  No matches')))
  } else {
    for (let i = 0; i < state.items.length; i++) {
      const item = state.items[i]!
      const focused = i === state.focusIndex
      const prefix: StyledSpan = focused ? colored('❯ ', 'cyan') : plain('  ')
      const label: StyledSpan = focused ? bold(item.label) : plain(item.label)
      const detail: StyledSpan = item.detail ? dim(` ${item.detail}`) : plain('')
      lines.push(line(prefix, label, detail))
    }
  }

  lines.push(line(plain('')))
  lines.push(line(dim('↑↓ navigate · type to filter · enter select · esc cancel')))
  return [block(lines, 1)]
}

function buildAskBlocks(state: AskState): ViewBlock[] {
  const q = state.questions[state.currentTab]!
  const result = []

  if (state.questions.length > 1) {
    const tabs = state.questions.map((qq, i) => {
      const active = i === state.currentTab
      return active ? bold(qq.header) : dim(qq.header)
    })
    const tabSpans: StyledSpan[] = []
    for (let i = 0; i < tabs.length; i++) {
      if (i > 0) tabSpans.push(plain('  '))
      tabSpans.push(tabs[i]!)
    }
    result.push(line(...tabSpans))
    result.push(line(plain('')))
  }

  result.push(line(bold(q.question)))
  result.push(line(plain('')))

  for (let i = 0; i < q.options.length; i++) {
    const opt = q.options[i]!
    const focused = !state.inOtherMode && i === state.focusIndex
    const prefix: StyledSpan = focused ? colored('❯ ', 'cyan') : plain('  ')
    const label: StyledSpan = focused ? bold(opt.label) : plain(opt.label)
    const desc: StyledSpan = opt.description ? dim(` — ${opt.description}`) : plain('')
    result.push(line(prefix, label, desc))
  }

  const otherFocused = state.inOtherMode
  if (otherFocused && state.otherText) {
    result.push(line(colored('❯ ', 'cyan'), plain(state.otherText), inverse(' ')))
  } else if (otherFocused) {
    result.push(line(colored('❯ ', 'cyan'), inverse(' '), dim(' Type something.')))
  } else {
    const isOtherSelected = state.focusIndex === q.options.length
    const prefix: StyledSpan = isOtherSelected ? colored('❯ ', 'cyan') : plain('  ')
    result.push(line(prefix, dim('Other...')))
  }

  result.push(line(plain('')))
  result.push(line(dim('↑↓ navigate · enter select · esc cancel')))

  return [block(result, 1)]
}

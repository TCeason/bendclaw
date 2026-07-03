import { TermRenderer, type RenderFrame } from './renderer.js'
import { parseInput, enableRawMode, enableEnhancedKeyboard, type KeyEvent } from './input.js'
import { installBracketedPaste } from './bracketed-paste.js'
import { createSpinnerState, advanceSpinner, formatSpinnerLine } from './spinner.js'
import { createSelectorState, selectorExpandItems, selectorClearQuery } from './selector.js'
import { createAskState, handleAskKeyEvent, type AskQuestion } from './ask.js'
import { buildUserMessage, buildThinkingLines, type OutputLine } from '../render/output.js'
import { renderMarkdownCached } from '../render/markdown.js'
import { Agent, QueryStream, fastExit, type SessionMeta, type ConfigInfo } from '../native/index.js'
import { createInitialState, type AppState } from './app/state.js'
import type { AskUserRequest } from './app/types.js'
import { HistoryManager, parseHistoryItems } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { isSlashCommand, resolveCommand, buildHardenPrompt } from '../commands/index.js'
import { renderBanner } from './banner.js'
import {
  buildOutputBlocks,
  buildPromptBlocks,
  buildOverlayBlocks,
  blocksToLines,
  type OverlayState,
  type PromptVMInput,
  type StyledLine,
  type ViewBlock,
} from './viewmodel/index.js'
import { HistoryRenderCache } from './viewmodel/history-cache.js'
import {
  createEditorState,
  getEditorText,
  isEditorEmpty,
  clearEditor,
  insertText,
  backspace,
  moveLeft,
  moveRight,
  moveHome,
  moveEnd,
  applyCompletion,
  refreshGhostHint,
  createHistoryState,
  pushHistory,
  historyPrev,
  historyNext,
  clearLineBefore,
  clearLineAfter,
  deleteForward,
  deleteWordBefore,
  insertNewline,
  moveUp,
  moveDown,
  editorNeedsContinuation,
  type EditorState,
  type HistoryState,
} from './input/editor.js'
import {
  createStreamMachineState,
  reduceRunEvent,
  flushStreaming,
  buildToolStartedLines,
  buildToolFinishedLines,
  buildToolProgressLines,
  type StreamMachineState,
} from './app/stream.js'
import { handleSlashCommand } from './app/commands.js'
import { askStateToResponse } from './app/ask-user.js'
import { syncProvider } from './app/provider.js'
import chalk from 'chalk'
import {
  shouldCollapse,
  cleanPastedText,
  formatPastedTextRef,
  formatImageRef,
  parsePasteRefs,
  stripImageRefs,
  deleteRefBackspace,
  skipRefOnMove,
  resolveSubmitText,
} from './input/paste_refs.js'
import { getImageFromClipboard } from './input/clipboard_image.js'
import { storeImage, formatImageSourceText } from './input/image_store.js'
import type { ContentBlock } from '../native/index.js'
import { tryStartServer, type ServerState } from './app/server.js'
import {
  RESUME_SELECTOR_TITLE,
  formatSessionItems,
  formatSessionWithTextItems,
  isResumeSelectorTitle,
  isSessionIdPrefix,
  resolveSessionByPrefix,
  selectSessionPool,
} from './app/resume.js'
import { findPreviousSession, shouldPreloadStartupSessions, selectResumeMessages, resumeElidedLine } from './app/session-view.js'
import { handleSelectorControl } from './app/selector-control.js'
import { decideReplControl, type ReplControlAction } from './app/repl-control.js'
import { extractAtPrefix, completeAtFile } from '../commands/file-completion.js'
import { GitInfoProvider } from './git-info.js'

const SPINNER_INTERVAL_MS = 100


export interface ReplOptions {
  agent: Agent
  resumeSessionId?: string
  continueLatest?: boolean
  serverPort?: number
  envFile?: string
}

function getGitVersion(cwd: string): string | null {
  try {
    const result = Bun.spawnSync(['git', 'rev-parse', '--short=12', 'HEAD'], { cwd, stdout: 'pipe', stderr: 'pipe' })
    if (result.exitCode !== 0) return null
    const sha = result.stdout.toString().trim()
    return sha || null
  } catch {
    return null
  }
}

function isGitDirty(cwd: string): boolean {
  try {
    const result = Bun.spawnSync(['git', 'status', '--porcelain'], { cwd, stdout: 'pipe', stderr: 'pipe' })
    if (result.exitCode !== 0) return false
    return result.stdout.toString().length > 0
  } catch {
    return false
  }
}

export async function startRepl(opts: ReplOptions): Promise<void> {
  const { agent } = opts
  const { version } = await import('../native/index.js')
  const appVersion = version()
  const renderer = new TermRenderer()
  renderer.init()

  let appState: AppState = {
    ...createInitialState(agent.model, agent.cwd),
  }
  let spinnerState = createSpinnerState()
  let editor: EditorState = createEditorState()
  let historyState: HistoryState
  let isLoading = false
  let streamRef: QueryStream | null = null
  let spinnerTimer: ReturnType<typeof setInterval> | null = null
  let titleFrozen = false
  let destroyed = false
  let sessionId: string | null = null
  let planning = false
  let logMode: import('../native/index.js').ForkedAgent | null = null
  let exitHint = false
  let exitHintTimer: ReturnType<typeof setTimeout> | null = null
  let overlay: OverlayState = { kind: 'none' }
  let streamMachine: StreamMachineState | null = null
  let lastPendingText = ''
  let lastPendingRendered = ''
  let expanded = false
  // Rendered-history cache — see HistoryRenderCache. Committed history is
  // append-only (or fully cleared), never mutated in place, so the flattened
  // ANSI lines are extended incrementally instead of re-flattened every frame.
  // A full rebuild over a long session takes 5–14 ms, which is what made the
  // just-sent message and keystroke echo visibly stall as the conversation
  // grew. Compact and expanded views each get their own cache because their
  // source arrays are appended independently (expanded-only progress/thinking).
  const compactHistoryCache = new HistoryRenderCache()
  const expandedHistoryCache = new HistoryRenderCache()
  function resetHistoryCache() {
    compactHistoryCache.reset()
    expandedHistoryCache.reset()
  }
  const compactLines: OutputLine[] = []
  const expandedLines: OutputLine[] = []
  let lastProgressLineCount = 0
  let lastThinkingLineCount = 0
  let fdAbort: AbortController | null = null
  // Streaming text: use stream machine's pendingText for the viewport.
  const screenLog = new ScreenLog()
  const gitVersion = getGitVersion(agent.cwd)
  const gitDirty = isGitDirty(agent.cwd)
  const markdownRendererVersion = `evot-${appVersion}:markdown-trace-v2${gitVersion ? `:git-${gitVersion}` : ''}${gitDirty ? ':dirty' : ''}`
  let markdownTraceId = 0

  function logMarkdownTrace(outputLines: OutputLine[], renderedLines: string[]) {
    const raw = outputLines.find(line => line.kind === 'assistant' && line.rawMarkdown)?.rawMarkdown
    if (!raw) return
    const firstLine = outputLines.find(line => line.kind === 'assistant' && line.rawMarkdown === raw)
    screenLog.logMarkdownTrace({
      messageId: firstLine?.id ?? `markdown-${++markdownTraceId}`,
      rendererVersion: markdownRendererVersion,
      rawMarkdown: raw,
      renderedLines,
    })
  }

  // Server state
  let serverState: ServerState | null = null
  try {
    serverState = await tryStartServer(opts.serverPort, opts.envFile)
  } catch { /* server start failed — continue without it */ }

  // Paste ref state
  const pastedChunks = new Map<number, string>()
  const pastedImages = new Map<number, { id: number; base64: string; mediaType: string; filePath?: string }>()
  let nextPasteId = 1

  // Update info
  let updateAvailable: { version: string } | null = null
  const updateMgr = new (await import('../update/index.js')).UpdateManager(
    appVersion
  )
  updateMgr.on('update-available', (info: { version: string }) => {
    updateAvailable = { version: info.version }
    renderer.requestRender()
  })
  updateMgr.start()

  const historyMgr = new HistoryManager(agent.cwd)
  const entries = historyMgr.load()
  historyState = createHistoryState(entries)

  let configInfo: ConfigInfo | undefined
  const refreshConfigInfo = () => {
    // Re-read backend config after a model switch so the footer reflects the
    // new provider's effective thinking level (it can differ per provider).
    try { configInfo = agent.configInfo() } catch {}
  }
  refreshConfigInfo()

  let preloadedSessions: SessionMeta[] = []
  if (shouldPreloadStartupSessions(opts)) {
    try { preloadedSessions = await agent.listSessions(opts.continueLatest ? 0 : 20) } catch {}
  }

  // Git info is watched so the footer follows external `git switch` / checkout
  // operations without requiring a REPL restart.
  const gitInfo = new GitInfoProvider(agent.cwd)
  gitInfo.onChange(() => renderer.requestRender())

  setTerminalTitle('✳')

  if (opts.continueLatest) {
    const match = findPreviousSession(preloadedSessions, agent.cwd)
    if (match) {
      await resumeSession(match)
    } else {
      commitLines([{ id: 'sys-continue-err', kind: 'system', text: chalk.red('No conversation found to continue') }])
      cleanup()
      fastExit(1)
    }
  } else if (opts.resumeSessionId) {
    const match = preloadedSessions.find(
      (s) => s.session_id === opts.resumeSessionId || s.session_id.startsWith(opts.resumeSessionId!)
    )
    if (match) {
      await resumeSession(match)
    } else {
      commitLines([{ id: 'sys-resume-err', kind: 'system', text: chalk.red(`Session not found: ${opts.resumeSessionId}`) }])
    }
  }

  renderer.requestRender()

  function getPromptVM(): PromptVMInput {
    return {
      lines: editor.lines,
      cursorLine: editor.cursorLine,
      cursorCol: editor.cursorCol,
      active: overlay.kind === 'none',
      model: appState.model,
      planning,
      logMode: logMode !== null,
      queuedMessages: [],
      exitHint,
      completionCandidates: editor.completionCandidates,
      ghostHint: editor.ghostHint,
      columns: renderer.termCols,
      isLoading,
      placeholder: isEditorEmpty(editor) && !isLoading,
      cwd: appState.cwd,
      gitRepo: gitInfo.getRepo(),
      gitBranch: gitInfo.getBranch(),
      // Footer stats
      inputTokens: appState.sessionTokens.inputTokens,
      outputTokens: appState.sessionTokens.outputTokens,
      cacheReadTokens: appState.sessionTokens.cacheReadTokens,
      contextTokens: appState.sessionTokens.contextTokens,
      contextWindow: appState.sessionTokens.contextWindow,
      thinkingLevel: configInfo?.thinkingLevel ?? '',
    }
  }

  function currentToolProgress(): string {
    return streamMachine?.toolProgress || streamMachine?.lastToolProgress || ''
  }

  // Release notes (shown once after update)
  let releaseNotes: string[] | null = null
  try {
    const { shouldShowReleaseNotes } = await import('../update/seen-version.js')
    if (shouldShowReleaseNotes(appVersion)) {
      const { parseReleaseNotes } = await import('../update/notes.js')
      const { fetchLatestStable } = await import('../update/check.js')
      fetchLatestStable().then((info) => {
        if (info?.body) {
          releaseNotes = parseReleaseNotes(info.body)
          renderer.requestRender()
        }
      }).catch(() => {})
    }
  } catch { /* best effort */ }

  function currentBannerText(): string {
    return renderBanner({
      model: agent.model,
      cwd: agent.cwd,
      configInfo,
      columns: renderer.termCols,
      serverState,
      releaseNotes,
      updateAvailable,
      skillsDirs: agent.skillsDirs(),
    })
  }

  // --- buildFrame: the single render callback for the new differential renderer ---
  function buildFrame(): RenderFrame {
    if (destroyed) return { lines: [] }

    const blocks: ViewBlock[] = []

    // 1. Banner
    const banner = currentBannerText()
    if (banner) {
      blocks.push({ lines: banner.split('\n').map(l => ({ spans: [{ text: l }] })), marginTop: 0 })
    }

    // 2. History (committed output lines) — incrementally cached so the
    // high-frequency spinner/delta/keystroke renders skip re-flattening the
    // whole transcript. The cache extends in place on append and rebuilds only
    // on reset (clear/replace), width change, or shrink. See HistoryRenderCache.
    const cols = renderer.termCols
    const cache = expanded ? expandedHistoryCache : compactHistoryCache
    const cachedHistoryLines = cache.sync(expanded ? expandedLines : compactLines, cols)
    if (cachedHistoryLines.length > 0) {
      blocks.push({ lines: cachedHistoryLines.map(l => ({ spans: [{ text: l }] })), marginTop: 0 })
    }

    // 3. Thinking text preview (shown during reasoning phase before text arrives)
    const pendingThinking = streamMachine?.pendingThinkingText ?? ''
    if (pendingThinking && isLoading) {
      if (expanded) {
        const thinkLines = pendingThinking.split('\n').map(l => ({ spans: [{ text: `  ${l}`, dim: true }] }))
        blocks.push({ lines: thinkLines, marginTop: 1 })
      } else {
        // Compact preview: header + last few lines
        const allThinkLines = pendingThinking.split('\n')
        const totalLines = allThinkLines.length
        const MAX_THINKING_PREVIEW = 4
        const MAX_LINE_WIDTH = 120
        const visible = allThinkLines.slice(-MAX_THINKING_PREVIEW)
        const thinkStyled: StyledLine[] = [
          { spans: [{ text: '✻', fg: 'cyan' as const, bold: true }, { text: ' thinking', bold: true }, { text: `  · ${totalLines} lines…`, dim: true }] },
        ]
        for (const l of visible) {
          const truncated = l.length > MAX_LINE_WIDTH ? l.slice(0, MAX_LINE_WIDTH - 1) + '\u2026' : l
          thinkStyled.push({ spans: [{ text: `  ${truncated}`, dim: true }] })
        }
        blocks.push({ lines: thinkStyled, marginTop: 1 })
      }
    }

    // 4. Streaming content — only re-render markdown when text actually changes
    const pendingText = streamMachine?.pendingText ?? ''
    if (pendingText && isLoading) {
      if (pendingText !== lastPendingText) {
        lastPendingText = pendingText
        lastPendingRendered = renderMarkdownCached(pendingText)
      }
      const mdLines = lastPendingRendered.split('\n')
      // The whole message streams in place here. After a rare overflow drain
      // (drainOverflowBlocks), leading blocks have already committed to history,
      // so the tail is a continuation: use the 2-space prefix instead of a
      // second ⏺ dot. Normal replies never commit mid-stream, so they keep the
      // ⏺ marker for the life of the stream.
      const isContinuation = streamMachine?.assistantCommitted ?? false
      const styledLines = mdLines.map((l, i) => {
        if (i === 0 && !isContinuation) return { spans: [{ text: '\u23fa ', fg: 'cyan' as const }, { text: l }] }
        return { spans: [{ text: `  ${l}` }] }
      })
      blocks.push({ lines: styledLines, marginTop: 1 })
    }

    // 5. Tool progress
    const toolProgress = currentToolProgress()
    if (toolProgress && isLoading) {
      const progLines = toolProgress.split('\n').slice(0, 5).map(l => ({ spans: [{ text: `  ${l}`, dim: true }] }))
      blocks.push({ lines: progLines, marginTop: 1 })
    }

    // 6. Spinner
    if (isLoading && overlay.kind !== 'ask-user') {
      const activeLlmCall = streamMachine?.activeLlmCall ?? false
      const liveOutputTokens = activeLlmCall ? spinnerState.tokenCount : 0
      const spinnerText = formatSpinnerLine(spinnerState, Date.now(), {
        inputTokens: appState.sessionTokens.inputTokens,
        outputTokens: appState.sessionTokens.outputTokens + liveOutputTokens,
        cacheReadTokens: appState.sessionTokens.cacheReadTokens,
      })
      blocks.push({ lines: [{ spans: [{ text: spinnerText }] }], marginTop: 1 })
    }

    // 7. Overlay
    blocks.push(...buildOverlayBlocks(overlay, renderer.termCols))

    // 8. Prompt
    blocks.push(...buildPromptBlocks(getPromptVM()))

    return { lines: blocksToLines(blocks) }
  }

  renderer.setRenderCallback(buildFrame)

  function outputContextFor(lines: OutputLine[]): { prevKind?: string; columns?: number } {
    const prev = lines.length > 0 ? lines[lines.length - 1] : undefined
    return {
      prevKind: prev?.kind,
      columns: renderer.termCols,
    }
  }

  function commitLines(outputLines: OutputLine[]) {
    if (outputLines.length === 0) return
    compactLines.push(...outputLines)
    expandedLines.push(...outputLines)
    // Pure append: the incremental history cache detects the growth by line
    // count, so no dirty flag is needed (which would force a full rebuild).
    // Log for tracing
    const visible = expanded ? expandedLines.slice(-outputLines.length) : outputLines
    const context = outputContextFor(compactLines.slice(0, -outputLines.length))
    const blocks = buildOutputBlocks(visible, context)
    const rendered = blocksToLines(blocks)
    screenLog.logLines(rendered)
    logMarkdownTrace(outputLines, rendered)
    // Trigger re-render — buildFrame will pick up the new lines
    renderer.requestRender()
  }

  /** Commit flush result with optional dual-commit (compact summary vs expanded full). */
  function commitFlushResult(flushed: { lines: OutputLine[]; expandedLines?: OutputLine[] }) {
    if (flushed.lines.length === 0) return
    if (flushed.expandedLines) {
      compactLines.push(...flushed.lines)
      expandedLines.push(...flushed.expandedLines)
      const visible = expanded ? flushed.expandedLines : flushed.lines
      const context = outputContextFor(compactLines.slice(0, -flushed.lines.length))
      const blocks = buildOutputBlocks(visible, context)
      const rendered = blocksToLines(blocks)
      screenLog.logLines(rendered)
      renderer.requestRender()
    } else {
      commitLines(flushed.lines)
    }
  }

  function commitToolStarted(event: import('../native/index.js').RunEvent): void {
    // The call line has no compact/expanded variants, so build it once and
    // append to both histories to keep them aligned.
    const callLines = buildToolStartedLines(event)
    compactLines.push(...callLines)
    expandedLines.push(...callLines)
    const context = outputContextFor(compactLines.slice(0, -callLines.length))
    const blocks = buildOutputBlocks(callLines, context)
    const rendered = blocksToLines(blocks)
    screenLog.logLines(rendered)
    renderer.requestRender()
  }

  function commitToolFinished(event: import('../native/index.js').RunEvent): void {
    const compact = buildToolFinishedLines(event)
    const exp = buildToolFinishedLines(event, true)
    compactLines.push(...compact)
    expandedLines.push(...exp)
    const visible = expanded ? exp : compact
    const context = outputContextFor(compactLines.slice(0, -compact.length))
    const blocks = buildOutputBlocks(visible, context)
    const rendered = blocksToLines(blocks)
    screenLog.logLines(rendered)
    renderer.requestRender()
  }

  /** Toggle expanded view and redraw. */
  function toggleExpanded(): void {
    expanded = !expanded
    // Differential render, not a forced clear. When the content being toggled
    // (e.g. the tool output you just ran) sits in the viewport, the renderer
    // repaints in place from the first changed line down, so the view stays
    // put instead of clearing and re-anchoring to the bottom (which is what
    // made the screen jump). A swap large enough to change history above the
    // viewport still falls back to a full redraw via the renderer's own
    // off-viewport guard. Mirrors pi, which toggles with requestRender().
    renderer.requestRender()
  }

  /** Cycle the model's reasoning effort (Shift+Tab) and reflect it in the footer. */
  function cycleThinkingLevel(): void {
    let level: string | null
    try {
      level = agent.cycleThinkingLevel()
    } catch {
      return
    }
    if (level === null) {
      commitLines([{ id: 'sys-think', kind: 'system', text: '  This model has no selectable thinking level' }])
      return
    }
    refreshConfigInfo()
    const label = level === 'off' ? 'off' : level
    commitLines([{ id: 'sys-think', kind: 'system', text: `  Thinking level → ${label}` }])
    renderer.requestRender()
  }

  function setTerminalTitle(suffix?: string) {
    if (titleFrozen) return
    const dirName = agent.cwd.split('/').pop() || agent.cwd
    const base = `evot - ${dirName}`
    const portPart = serverState ? ` · :${serverState.port}` : ''
    const title = suffix ? `${suffix} ${base}${portPart}` : `${base}${portPart}`
    process.stdout.write(`\x1b]0;${title}\x07`)
  }

  function freezeTerminalTitle(suffix?: string) {
    const dirName = agent.cwd.split('/').pop() || agent.cwd
    const base = `evot - ${dirName}`
    const portPart = serverState ? ` · :${serverState.port}` : ''
    const title = suffix ? `${suffix} ${base}${portPart}` : `${base}${portPart}`
    process.stdout.write(`\x1b]0;${title}\x07`)
    titleFrozen = true
  }

  function unfreezeTerminalTitle() {
    titleFrozen = false
  }

  let titleFrame = 0
  const TITLE_INTERVAL_FRAMES = Math.round(960 / SPINNER_INTERVAL_MS) // ~960ms like Claude Code

  function startSpinner() {
    if (spinnerTimer) return
    titleFrame = 0
    spinnerTimer = setInterval(() => {
      spinnerState = advanceSpinner(spinnerState)
      renderer.requestRender()
      // Terminal title animation — update at ~960ms like Claude Code.
      if (spinnerState.frame % TITLE_INTERVAL_FRAMES === 0) {
        const glyphs = ['⠂', '⠐']
        const idx = titleFrame % glyphs.length
        titleFrame++
        setTerminalTitle(glyphs[idx])
      }
    }, SPINNER_INTERVAL_MS)
  }

  function stopSpinner() {
    if (spinnerTimer) {
      clearInterval(spinnerTimer)
      spinnerTimer = null
    }
    setTerminalTitle('✳')
  }

  async function resumeSession(session: SessionMeta) {
    try {
      const transcript = await agent.loadTranscript(session.session_id)
      sessionId = session.session_id
      // Fields may be missing when the caller passes a partial SessionMeta
      // (e.g. the resume selector only knows the id); fetch the full record.
      let model = session.model
      let thinkingLevel = session.thinking_level
      if (!model || thinkingLevel === undefined) {
        const full = await agent.findSession(session.session_id)
        if (full) {
          if (!model) model = full.model
          if (thinkingLevel === undefined) thinkingLevel = full.thinking_level
        }
      }
      if (model) {
        agent.model = model
      }
      // Restore the session's reasoning effort so a resumed conversation keeps
      // the level it was last run with (no-op for non-reasoning models).
      if (thinkingLevel) {
        agent.restoreThinkingLevel(thinkingLevel)
      }
      if (model || thinkingLevel) {
        refreshConfigInfo()
      }
      appState = { ...appState, sessionId: session.session_id, model: model || appState.model }
      const { messagesToOutputLines } = await import('../render/output.js')
      const { transcriptToMessages } = await import('../session/transcript.js')
      const messages = transcriptToMessages(transcript as any)
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      // Only render the most recent messages to scrollback. Rendering the whole
      // transcript re-runs markdown (marked lex + ANSI + table align) per
      // message, which is O(total) and reaches ~500ms on very long sessions.
      // The hidden messages stay in the model's context (the backend restores
      // it by session_id independently of this display transcript), so this
      // only trims what's painted, not what the model remembers.
      const { shown, hidden } = selectResumeMessages(messages)
      if (hidden > 0) commitLines([resumeElidedLine(hidden)])
      commitLines(messagesToOutputLines(shown))
      commitLines([
        { id: 'sys-resumed-gap', kind: 'system', text: '' },
        { id: 'sys-resumed', kind: 'system', text: chalk.dim(`  resumed session ${session.session_id.slice(0, 8)}`) },
      ])
    } catch (err: any) {
      commitLines([{ id: 'sys-err', kind: 'error', text: `Failed to resume: ${err?.message ?? err}` }])
    }
  }

  /** Get expanded text — resolves paste refs, strips only resolved image refs. */
  function getExpandedText(resolvedImageIds?: Set<number>): string {
    return resolveSubmitText(getEditorText(editor), pastedChunks, resolvedImageIds ?? null)
  }

  /** Get display text (raw with refs intact). */
  function getDisplayText(): string {
    return getEditorText(editor).trim()
  }

  /** Clear editor and paste state. */
  function clearAll() {
    editor = clearEditor(editor)
    pastedChunks.clear()
    pastedImages.clear()
  }

  /** Insert pasted text, collapsing large pastes into refs. */
  function insertPaste(raw: string) {
    const cleaned = cleanPastedText(raw)
    if (shouldCollapse(cleaned)) {
      const id = nextPasteId++
      const numLines = (cleaned.match(/\n/g) || []).length
      pastedChunks.set(id, cleaned)
      const ref = formatPastedTextRef(id, numLines)
      editor = insertText(editor, ref)
    } else {
      editor = insertText(editor, cleaned)
    }
  }

  /** Try to paste image from clipboard (Ctrl+V). */
  async function tryPasteImage() {
    const img = await getImageFromClipboard()
    if (img) {
      const id = nextPasteId++
      // Store to disk immediately so images survive past session memory
      const filePath = await storeImage(img.base64, img.mediaType)
      pastedImages.set(id, { id, base64: img.base64, mediaType: img.mediaType, filePath: filePath ?? undefined })
      editor = insertText(editor, formatImageRef(id))
      renderer.requestRender()
    }
  }

  /** Build content blocks for images. Returns blocks and resolved image IDs. */
  function buildImageContentBlocks(): { blocks: ContentBlock[]; resolvedIds: Set<number> } | null {
    const displayText = getDisplayText()
    const imageRefs = parsePasteRefs(displayText).filter(r => r.type === 'image')
    const resolved: { id: number; base64: string; mediaType: string; filePath?: string }[] = []
    const unresolvedIds = new Set<number>()
    for (const ref of imageRefs) {
      const img = pastedImages.get(ref.id)
      if (img) {
        resolved.push(img)
      } else {
        unresolvedIds.add(ref.id)
      }
    }
    if (resolved.length === 0) return null
    const blocks: ContentBlock[] = []
    // Only strip resolved image refs from text — unresolved ones stay as [Image #N]
    const text = getExpandedText(new Set(resolved.map(r => r.id)))
    // Annotate with image source paths so the model can reference files on disk
    const sourceAnnotations = resolved
      .filter(r => r.filePath)
      .map(r => formatImageSourceText(r.id, r.filePath!))
      .join('\n')
    const fullText = sourceAnnotations ? `${text}\n${sourceAnnotations}` : text
    if (fullText) blocks.push({ type: 'text', text: fullText })
    for (const img of resolved) {
      blocks.push({
        type: 'image',
        mimeType: img.mediaType,
        source: img.filePath
          ? { type: 'path', path: img.filePath }
          : { type: 'base64', data: img.base64 },
      })
    }
    return { blocks, resolvedIds: new Set(resolved.map(r => r.id)) }
  }

  async function runQuery(text: string, contentJson?: string, prebuiltStream?: QueryStream) {
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderer.requestRender()

    try {
      const stream = prebuiltStream
        ?? await agent.query(text, sessionId ?? undefined, planning ? 'planning_interactive' : 'interactive', contentJson)
      streamRef = stream
      sessionId = stream.sessionId ?? sessionId
      appState = { ...appState, sessionId: sessionId }
      screenLog.bind(stream.sessionId)

      for await (const event of stream) {
        if (destroyed) break
        if (!streamMachine) break

        if (event.kind === 'ask_user') {
          
          const payload = (event.payload ?? {}) as { questions?: AskUserRequest['questions'] }
          if (payload.questions && payload.questions.length > 0) {
            const questions: AskQuestion[] = payload.questions.map(q => ({
              header: q.header,
              question: q.question,
              options: q.options.map(o => ({ label: o.label, description: o.description })),
            }))
            overlay = { kind: 'ask-user', state: createAskState(questions) }
            freezeTerminalTitle('?')
            // Append a single prompt line to scroll zone so the question
            // appears inline with message history; the actual interactive UI
            // renders in the status area and updates in-place.
            
            renderer.requestRender()
          }
          continue
        }

        const update = reduceRunEvent(streamMachine!, event, { termRows: renderer.termRows })

        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState

        // Request re-render on each delta so streaming text appears
        if (event.kind === 'assistant_delta') {
          renderer.requestRender()
        }

        // When a tool starts, stream machine already committed pending text via update.commitLines
        if (!update.suppressToolStarted && event.kind === 'tool_started') {
          commitToolStarted(event)
          lastProgressLineCount = 0
        }
        if (!update.suppressToolFinished && event.kind === 'tool_finished') {
          commitToolFinished(event)
          lastProgressLineCount = 0
        }

        // In expanded mode, commit tool progress lines to scroll area
        if (expanded && event.kind === 'tool_progress') {
          const text = ((event.payload ?? {}) as Record<string, any>).text as string | undefined
          if (text) {
            const allLines = text.split('\n')
            const baseline = Math.max(lastProgressLineCount, currentToolProgress().split('\n').length)
            const newLines = allLines.slice(baseline)
            lastProgressLineCount = allLines.length
            if (newLines.length > 0) {
              const expandedProgress = buildToolProgressLines({ ...event, payload: { ...(event.payload ?? {}), text: newLines.join('\n') } }, true)
              expandedLines.push(...expandedProgress)
              renderer.requestRender()
            }
          }
        }

        // In expanded mode, stream thinking lines to scroll area
        if (expanded && event.kind === 'assistant_delta') {
          const thinkingDelta = ((event.payload ?? {}) as Record<string, any>).thinking_delta as string | undefined
          if (thinkingDelta && streamMachine?.pendingThinkingText) {
            const allLines = streamMachine.pendingThinkingText.split('\n')
            const newLines = allLines.slice(lastThinkingLineCount)
            // Only commit complete lines (exclude the last partial line)
            const completeNewLines = newLines.length > 1 ? newLines.slice(0, -1) : []
            lastThinkingLineCount = allLines.length - 1
            if (completeNewLines.length > 0) {
              const thinkingOutputLines = buildThinkingLines(completeNewLines.join('\n'))
              expandedLines.push(...thinkingOutputLines)
              renderer.requestRender()
            }
          }
          // Reset when thinking ends (first text delta after thinking)
          const textDelta = ((event.payload ?? {}) as Record<string, any>).delta as string | undefined
          if (textDelta && lastThinkingLineCount > 0) {
            lastThinkingLineCount = 0
          }
        }

        if (update.commitLines.length > 0) {
          if (update.expandedCommitLines) {
            // Dual-commit: compact in compactLines, expanded in expandedLines
            const compact = update.commitLines
            const exp = update.expandedCommitLines
            compactLines.push(...compact)
            expandedLines.push(...exp)
            const visible = expanded ? exp : compact
            const context = outputContextFor(compactLines.slice(0, -compact.length))
            const blocks = buildOutputBlocks(visible, context)
            const rendered = blocksToLines(blocks)
            screenLog.logLines(rendered)
            logMarkdownTrace(visible, rendered)
            renderer.requestRender()
          } else {
            commitLines(update.commitLines)
          }
        }

        // writeLines are log-only: LLM/COMPACT/SPILL stats that don't render in
        // the TUI. Run them through the same formatting pipeline so screen.log
        // still captures the observability detail for post-hoc debug.
        if (update.writeLines.length > 0) {
          const blocks = buildOutputBlocks(update.writeLines, { columns: renderer.termCols })
          const rendered = blocksToLines(blocks)
          screenLog.logLines(rendered)
        }

        if (update.rerenderStatus) renderer.requestRender()
      }

      if (streamMachine) {
        
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        appState = final.state.appState
        commitFlushResult(final)
      }
    } catch (err: any) {
      
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      commitLines([{ id: 'sys-err', kind: 'error', text: err?.message ?? String(err) }])
    } finally {
      unfreezeTerminalTitle()
      streamRef = null
      isLoading = false
      streamMachine = null
      stopSpinner()
      renderer.requestRender()
    }
  }

  function handleKey(event: KeyEvent) {
    const actions = decideReplControl({
      event,
      overlay,
      isLoading,
      hasStream: streamRef !== null,
      editor,
      exitHint,
      logMode: logMode !== null,
    })

    for (const action of actions) {
      if (applyReplControlAction(action, event)) return
    }
  }

  function applyReplControlAction(action: ReplControlAction, event: KeyEvent): boolean {
    switch (action.kind) {
      case 'interrupt':
        interruptStream('sys-int', '  Interrupted.')
        return true
      case 'exit':
        cleanup()
        if (sessionId) {
          process.stdout.write(`\n\x1b[90m${'─'.repeat(80)}\x1b[0m\n`)
          process.stdout.write(`\x1b[90mResume: evot --resume ${sessionId}\x1b[0m\n\n`)
        }
        fastExit(0)
      case 'show-exit-hint':
        exitHint = true
        renderer.requestRender()
        if (exitHintTimer) clearTimeout(exitHintTimer)
        exitHintTimer = setTimeout(() => { exitHint = false; renderer.requestRender() }, 2000)
        return true
      case 'clear-editor':
        editor = clearEditor(editor)
        renderer.requestRender()
        return true
      case 'clear-exit-hint':
        exitHint = false
        return false
      case 'cancel-ask':
        overlay = { kind: 'none' }
        unfreezeTerminalTitle()
        interruptStream('sys-ask-cancel', '  ⏺ Cancelled.')
        return true
      case 'clear-selector-query':
        if (overlay.kind === 'selector') overlay = { kind: 'selector', state: selectorClearQuery(overlay.state) }
        renderer.requestRender()
        return true
      case 'close-overlay': {
        const redraw = overlay.kind === 'selector'
        overlay = { kind: 'none' }
        if (redraw) {
          renderer.fullRedraw()
        } else {
          renderer.requestRender()
        }
        return true
      }
      case 'exit-log-mode':
        logMode = null
        commitLines([{ id: 'sys-log-exit', kind: 'system', text: '  [log mode] exited' }])
        renderer.requestRender()
        return true
      case 'selector-key':
        handleSelectorKey(event)
        return true
      case 'ask-key':
        handleAskKey(event)
        return true
      case 'toggle-expanded':
        toggleExpanded()
        return true
      case 'loading-enter':
        handleLoadingEnter()
        return true
      case 'loading-char':
        if (event.type === 'char') {
          editor = insertText(editor, event.char)
          renderer.requestRender()
        }
        return true
      case 'loading-paste':
        if (event.type === 'paste') {
          insertPaste(event.text)
          renderer.requestRender()
        }
        return true
      case 'normal-key':
        handleNormalKey(event)
        return true
    }
  }

  /** Flush any in-progress streaming content to committed output.
   *  Call before clearing streaming state on any abort/cancel path. */
  function flushStreamContent() {
    
    // Flush anything the stream machine accumulated
    if (!streamMachine) return
    const flushed = flushStreaming(streamMachine)
    streamMachine = flushed.state
    commitFlushResult(flushed)
    // Preserve tool progress that was only shown in the status area
    const progress = streamMachine.lastToolProgress
    if (progress) {
      const toolName = streamMachine.spinnerState.toolName ?? 'Bash'
      commitLines(buildToolProgressLines(
        { kind: 'tool_progress', payload: { tool_name: toolName, text: progress } } as any,
        expanded,
      ))
    }
  }

  function interruptStream(id: string, text: string) {
    unfreezeTerminalTitle()
    if (streamRef) {
      streamRef.abort()
      streamRef = null
    }
    isLoading = false
    flushStreamContent()
    streamMachine = null
    stopSpinner()
    commitLines([{ id, kind: 'system', text }])
  }

  function formatLogPaths(logPath: string | null, markdownPath: string | null): string | null {
    if (!logPath) return null
    const lines = [`  Log: ${logPath}`]
    if (markdownPath) lines.push(`  Markdown: ${markdownPath}`)
    return lines.join('\n')
  }

  function handleLoadingEnter() {
    const displayText = getDisplayText()
    const imageResult = buildImageContentBlocks()
    const imageBlocks = imageResult?.blocks ?? null
    const expandedText = imageResult
      ? getExpandedText(imageResult.resolvedIds)
      : getExpandedText()

    const trimmed = (expandedText || '').trim()
    if (trimmed === '/log') {
      clearAll()
      const logPath = screenLog.filePath
      const markdownPath = screenLog.markdownTraceFilePath
      if (logPath) {
        const text = formatLogPaths(logPath, markdownPath)
        commitLines([{ id: 'sys-log', kind: 'system', text: text ?? `  Log: ${logPath}` }])
      }
      else commitLines([{ id: 'sys-log', kind: 'system', text: '  No active screen log.' }])
      renderer.requestRender()
      return
    }

    if ((expandedText || imageBlocks) && streamRef) {
      if (imageBlocks) {
        const contentJson = JSON.stringify(imageBlocks)
        streamRef.steer('', contentJson)
      } else {
        streamRef.steer(expandedText)
      }
      commitLines(buildUserMessage(displayText))
      clearAll()
      renderer.requestRender()
    }
  }

  function handleNormalKey(event: KeyEvent) {
    if (event.type === 'ctrl') {
      switch (event.key) {
        case 'u':
          editor = clearLineBefore(editor)
          renderer.requestRender()
          return
        case 'k':
          editor = clearLineAfter(editor)
          renderer.requestRender()
          return
        case 'd':
          if (isEditorEmpty(editor)) {
            cleanup()
            fastExit(0)
          }
          editor = deleteForward(editor)
          renderer.requestRender()
          return
        case 'w':
          editor = deleteWordBefore(editor)
          renderer.requestRender()
          return
        case 'a':
          editor = moveHome(editor)
          renderer.requestRender()
          return
        case 'e':
          editor = moveEnd(editor)
          renderer.requestRender()
          return
        case 'l':
          clearAll()
          renderer.requestRender()
          return
        case 'v':
          tryPasteImage()
          return
        case 'o':
          toggleExpanded()
          return
        default:
          return
      }
    }

    switch (event.type) {
      case 'enter': {
        const rawText = getEditorText(editor).trim()
        if (!rawText) return
        // Check for continuation (unclosed fences, trailing backslash)
        if (editorNeedsContinuation(editor)) {
          editor = insertNewline(editor)
          renderer.requestRender()
          return
        }
        const displayText = getDisplayText()
        const imageResult = buildImageContentBlocks()
        const imageBlocks = imageResult?.blocks ?? null
        // expandedText: only strip image refs that have resolved data.
        // Unresolved ones (e.g. from history) stay as [Image #N] text markers.
        const expandedText = imageResult
          ? getExpandedText(imageResult.resolvedIds)
          : getExpandedText()
        // Allow image-only or text-only submissions
        if (!expandedText && !imageBlocks) return
        clearAll()
        renderer.requestRender()
        if (isSlashCommand(expandedText || rawText)) {
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
          }
          handleSlashInput(expandedText || rawText)
        } else if (logMode) {
          // In log mode, send to forked agent
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
          }
          runLogQuery(logMode, expandedText)
        } else {
          // Save to history
          if (displayText) {
            historyMgr.append(displayText)
            historyState = pushHistory(historyState, displayText)
          }
          commitLines(buildUserMessage(displayText))
          if (imageBlocks) {
            const contentJson = JSON.stringify(imageBlocks)
            runQuery('', contentJson)
          } else {
            runQuery(expandedText)
          }
        }
        break
      }
      case 'shift-enter':
      case 'alt-enter': {
        editor = insertNewline(editor)
        renderer.requestRender()
        break
      }
      case 'shift-tab': {
        cycleThinkingLevel()
        break
      }
      case 'tab': {
        // Try @file completion first (async)
        const currentLine = editor.lines[editor.cursorLine] ?? ''
        const beforeCursor = currentLine.slice(0, editor.cursorCol)
        const atPrefix = extractAtPrefix(beforeCursor)
        if (atPrefix) {
          // Cancel any previous fd search
          if (fdAbort) fdAbort.abort()
          fdAbort = new AbortController()
          completeAtFile(beforeCursor, appState.cwd, fdAbort.signal).then(result => {
            if (!result || result.items.length === 0) return
            if (result.items.length === 1) {
              // Single match — apply directly
              const item = result.items[0]!
              const line = editor.lines[editor.cursorLine] ?? ''
              const newLine = line.slice(0, result.prefixStart) + item.value + (item.isDirectory ? '' : ' ') + line.slice(editor.cursorCol)
              const newCol = result.prefixStart + item.value.length + (item.isDirectory ? 0 : 1)
              const lines = [...editor.lines]
              lines[editor.cursorLine] = newLine
              editor = { ...editor, lines, cursorCol: newCol, ghostHint: '', completionCandidates: [] }
            } else {
              // Multiple matches — show candidates and apply common prefix
              const labels = result.items.map(i => i.label)
              const values = result.items.map(i => i.value)
              const common = commonPrefixOf(values)
              if (common.length > atPrefix.prefix.length) {
                const line = editor.lines[editor.cursorLine] ?? ''
                const newLine = line.slice(0, result.prefixStart) + common + line.slice(editor.cursorCol)
                const newCol = result.prefixStart + common.length
                const lines = [...editor.lines]
                lines[editor.cursorLine] = newLine
                editor = { ...editor, lines, cursorCol: newCol, ghostHint: '', completionCandidates: labels }
              } else {
                editor = { ...editor, completionCandidates: labels }
              }
            }
            renderer.requestRender()
          }).catch(() => { /* ignore */ })
          break
        }
        // Sync slash/path completion
        const result = applyCompletion(editor)
        if (result.applied) {
          editor = result.state
          renderer.requestRender()
        }
        break
      }
      case 'char':
        editor = insertText(editor, event.char)
        editor = refreshGhostHint(editor)
        renderer.requestRender()
        // Auto-trigger @file completion on any char while in @ context
        {
          const charLine = editor.lines[editor.cursorLine] ?? ''
          const charBefore = charLine.slice(0, editor.cursorCol)
          const charAtPrefix = extractAtPrefix(charBefore)
          if (charAtPrefix) {
            if (fdAbort) fdAbort.abort()
            fdAbort = new AbortController()
            completeAtFile(charBefore, appState.cwd, fdAbort.signal).then(atResult => {
              if (!atResult || atResult.items.length === 0) {
                if (editor.completionCandidates.length > 0) {
                  editor = { ...editor, completionCandidates: [] }
                  renderer.requestRender()
                }
                return
              }
              const labels = atResult.items.map(i => i.label)
              editor = { ...editor, completionCandidates: labels }
              renderer.requestRender()
            }).catch(() => { /* ignore */ })
          } else if (editor.completionCandidates.length > 0) {
            editor = { ...editor, completionCandidates: [] }
            renderer.requestRender()
          }
        }
        break
      case 'paste':
        insertPaste(event.text)
        renderer.requestRender()
        break
      case 'backspace': {
        // Check if we should delete an entire paste ref
        const currentLine = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(currentLine)
        const refDel = deleteRefBackspace(currentLine, editor.cursorCol, refs)
        if (refDel) {
          const deletedRef = refs.find(r => r.end === editor.cursorCol)
          if (deletedRef) {
            pastedChunks.delete(deletedRef.id)
            pastedImages.delete(deletedRef.id)
          }
          const newLines = [...editor.lines]
          newLines[editor.cursorLine] = refDel.newLine
          editor = { ...editor, lines: newLines, cursorCol: refDel.newCursorCol, ghostHint: '', completionCandidates: [] }
        } else {
          editor = backspace(editor)
        }
        editor = refreshGhostHint(editor)
        renderer.requestRender()
        // Update @file candidates after backspace
        {
          const bsLine = editor.lines[editor.cursorLine] ?? ''
          const bsBefore = bsLine.slice(0, editor.cursorCol)
          const bsAtPrefix = extractAtPrefix(bsBefore)
          if (bsAtPrefix) {
            if (fdAbort) fdAbort.abort()
            fdAbort = new AbortController()
            completeAtFile(bsBefore, appState.cwd, fdAbort.signal).then(atResult => {
              if (!atResult || atResult.items.length === 0) {
                if (editor.completionCandidates.length > 0) {
                  editor = { ...editor, completionCandidates: [] }
                  renderer.requestRender()
                }
                return
              }
              const labels = atResult.items.map(i => i.label)
              editor = { ...editor, completionCandidates: labels }
              renderer.requestRender()
            }).catch(() => { /* ignore */ })
          } else if (editor.completionCandidates.length > 0) {
            editor = { ...editor, completionCandidates: [] }
            renderer.requestRender()
          }
        }
        break
      }
      case 'left': {
        const line = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(line)
        const skip = skipRefOnMove(editor.cursorCol, 'left', refs)
        if (skip !== null) {
          editor = { ...editor, cursorCol: skip, ghostHint: '' }
        } else {
          editor = moveLeft(editor)
        }
        renderer.requestRender()
        break
      }
      case 'right': {
        const line = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(line)
        const skip = skipRefOnMove(editor.cursorCol, 'right', refs)
        if (skip !== null) {
          editor = { ...editor, cursorCol: skip, ghostHint: '' }
        } else {
          editor = moveRight(editor)
        }
        renderer.requestRender()
        break
      }
      case 'home':
        editor = moveHome(editor)
        renderer.requestRender()
        break
      case 'end':
        editor = moveEnd(editor)
        renderer.requestRender()
        break
      case 'up': {
        // Multi-line: move cursor up within editor, unless already at first line
        if (editor.lines.length > 1 && editor.cursorLine > 0) {
          editor = moveUp(editor)
          renderer.requestRender()
          break
        }
        // At first line or single-line: history navigation
        const result = historyPrev(historyState, editor)
        if (result.changed) {
          historyState = result.history
          editor = result.editor
          renderer.requestRender()
        }
        break
      }
      case 'down': {
        // Multi-line: move cursor down within editor, unless already at last line
        if (editor.lines.length > 1 && editor.cursorLine < editor.lines.length - 1) {
          editor = moveDown(editor)
          renderer.requestRender()
          break
        }
        // At last line or single-line: history navigation
        const result = historyNext(historyState, editor)
        if (result.changed) {
          historyState = result.history
          editor = result.editor
          renderer.requestRender()
        }
        break
      }
      case 'page-up':
      case 'page-down':
        break
      default:
        break
    }
  }

  async function handleSlashInput(text: string) {
    const result = handleSlashCommand(text, {
      agent,
      appState,
      configInfo,
      preloadedSessions,
      planning,
    })
    appState = result.appState
    planning = result.planning
    if (result.overlay) overlay = result.overlay
    if (result.clearScreen) {
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
    }
    if (result.clearContext) {
      // Abort any in-flight streaming and clear local context view without switching sessions.
      if (isLoading && streamRef) {
        streamRef.abort(); streamRef = null; isLoading = false
        flushStreamContent()
        streamMachine = null; stopSpinner()
      }
      sessionId = null
      appState = { ...createInitialState(appState.model, agent.cwd) }
      gitInfo.setCwd(agent.cwd)
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      try { preloadedSessions = await agent.listSessions(20) } catch {}
    }
    if (result.newSession) {
      // Abort any in-flight streaming
      if (isLoading && streamRef) {
        streamRef.abort(); streamRef = null; isLoading = false
        flushStreamContent()
        streamMachine = null; stopSpinner()
      }
      // Start and bind a fresh empty session so /resume can see it immediately.
      const newSession = await agent.createSession()
      sessionId = newSession.session_id
      appState = { ...createInitialState(newSession.model || appState.model, agent.cwd), sessionId }
      gitInfo.setCwd(agent.cwd)
      renderer.clearScreen()
      compactLines.length = 0
      expandedLines.length = 0
      resetHistoryCache()
      try { preloadedSessions = await agent.listSessions(20) } catch { preloadedSessions = [newSession] }
      commitLines([{ id: 'sys-new-session', kind: 'system', text: chalk.dim(`  new session ${sessionId.slice(0, 8)}`) }])
    }
    if (result.exit) { cleanup(); fastExit(0) }
    if (result.resumeSession) await resumeSession(result.resumeSession)
    if (result.systemLines.length > 0) commitLines(result.systemLines)

    // Handle async commands that the simple handleSlashCommand can't do
    const resolved = resolveCommand(text)
    if (resolved.kind !== 'resolved') {
      renderer.requestRender()
      return
    }
    const { name, args } = resolved

    if (name === '/goto') {
      if (!args) {
        commitLines([{ id: 'sys-goto', kind: 'system', text: '  Usage: /goto <message_number>' }])
      } else {
        try {
          const outcome = await agent.submit(`/goto ${args}`, sessionId ?? undefined)
          if (outcome.kind === 'command') {
            commitLines([{ id: 'sys-goto', kind: 'system', text: `  ${outcome.message}` }])
          }
        } catch (err: any) {
          commitLines([{ id: 'sys-goto-err', kind: 'system', text: chalk.red(`  Goto failed: ${err?.message ?? err}`) }])
        }
      }
    } else if (name === '/history') {
      try {
        // Fetch a large set for search, display only the most recent entries
        const displayLimit = args ? parseInt(args, 10) : 20
        const searchLimit = Math.max(displayLimit, 200)
        const searchCmd = `/history ${searchLimit}`
        const outcome = await agent.submit(searchCmd, sessionId ?? undefined)
        if (outcome.kind === 'command') {
          const allItems = parseHistoryItems(outcome.message)
          if (allItems.length === 0) {
            commitLines([{ id: 'sys-hist', kind: 'system', text: `  ${outcome.message}` }])
          } else {
            // Mark user entries as goto-able, assistant as preview-only
            const annotate = (items: typeof allItems) => items.map(item => ({
              ...item,
              detail: item.role === 'user' ? `↩ ${item.detail}` : `  ${item.detail}`,
              focusable: item.role === 'user',
            }))
            const displayItems = allItems.slice(-displayLimit)
            overlay = {
              kind: 'selector',
              state: createSelectorState('History  (↩ goto · enter preview)', annotate(displayItems), annotate(allItems)),
            }
          }
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-hist-err', kind: 'system', text: chalk.red(`  History failed: ${err?.message ?? err}`) }])
      }
    } else if (name === '/env') {
      handleEnvCommand(args)
    } else if (name === '/harden') {
      const subject = buildHardenPrompt(args)
      commitLines(buildUserMessage(text.trim()))
      runQuery(subject)
    } else if (name === '/skill') {
      await handleSkillCommand(args)
    } else if (name === '/copy') {
      await handleCopyCommand()
    } else if (name === '/update') {
      await handleUpdateCommand()
    } else if (name === '/act' || name === '/done') {
      if (logMode) {
        logMode = null
        commitLines([{ id: 'sys-log-exit', kind: 'system', text: '  [log mode] exited' }])
      } else {
        planning = false
        commitLines([{ id: 'sys-act', kind: 'system', text: '  planning: off' }])
      }
    } else if (name === '/_dump') {
      try {
        const outcome = await agent.submit(
          `/_dump${args ? ' ' + args : ''}`,
          sessionId ?? undefined,
          planning ? 'planning_interactive' : 'interactive',
        )
        if (outcome.kind === 'command') {
          const lines = (outcome.message ?? '').split('\n').map((line, i) => ({
            id: `sys-dump-${i}`,
            kind: 'system' as const,
            text: `  ${line}`,
          }))
          commitLines(lines.length > 0 ? lines : [{ id: 'sys-dump', kind: 'system', text: '  (no dump output)' }])
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-dump-err', kind: 'system', text: chalk.red(`  /_dump failed: ${err?.message ?? err}`) }])
      }
    } else if (name === '/log') {
      await handleLogCommand(args)
    } else if (name === '/resume') {
      try {
        if (args && isSessionIdPrefix(args)) {
          const allSessions: SessionMeta[] = await agent.listSessions(0)
          const resolved = resolveSessionByPrefix(allSessions, args)
          if (resolved.kind === 'matched') {
            await resumeSession(resolved.session)
          } else {
            openResumeSelector(args)
          }
        } else {
          openResumeSelector(args || undefined)
        }
      } catch (err: any) {
        commitLines([{ id: 'sys-r-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
      }
    } else if (name === '/model' && !args) {
      // Show model selector overlay
      const models = configInfo?.availableModels ?? [agent.model]
      if (models.length > 1) {
        overlay = {
          kind: 'selector',
          state: {
            ...createSelectorState('Select model', models.map(m => ({
              label: m,
              detail: m === agent.model ? '(current)' : undefined,
            }))),
            focusIndex: Math.max(0, models.indexOf(agent.model)),
          },
        }
      } else {
        commitLines([{ id: 'sys-m', kind: 'system', text: '  Only one model available.' }])
      }
    }

    renderer.requestRender()
  }

  function openResumeSelector(initialQuery?: string) {
    agent.listSessions(0).then(allSessions => {
      const pool = selectSessionPool(allSessions, agent.cwd)
      if (pool.length === 0) {
        commitLines([{ id: 'sys-r', kind: 'system', text: '  No sessions found' }])
        return
      }
      const metaItems = formatSessionItems(pool.slice(0, 20))
      const allMetaItems = formatSessionItems(pool)
      overlay = {
        kind: 'selector',
        state: createSelectorState(RESUME_SELECTOR_TITLE, metaItems, allMetaItems, initialQuery),
      }
      renderer.requestRender()
      agent.listSessionsWithText(0).then(allWithText => {
        if (overlay.kind !== 'selector' || !isResumeSelectorTitle(overlay.state.title)) return
        const fullPool = selectSessionPool(allWithText, agent.cwd)
        const fullItems = formatSessionWithTextItems(fullPool)
        overlay = {
          kind: 'selector',
          state: selectorExpandItems(overlay.state, fullItems),
        }
        renderer.requestRender()
      }).catch(() => {})
    }).catch((err: any) => {
      commitLines([{ id: 'sys-r-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
    })
  }

  function handleEnvCommand(args: string) {
    const sub = args.trim()
    if (!sub) {
      const vars = agent.listVariables()
      if (vars.length === 0) {
        commitLines([{ id: 'sys-env', kind: 'system', text: '  No variables set' }])
      } else {
        for (const v of vars) {
          commitLines([{ id: `sys-env-${v.key}`, kind: 'system', text: `  ${v.key}=${v.value}` }])
        }
      }
    } else if (sub.startsWith('set ')) {
      const eq = sub.slice(4).trim()
      const eqIdx = eq.indexOf('=')
      if (eqIdx <= 0) {
        commitLines([{ id: 'sys-env-err', kind: 'system', text: '  Usage: /env set KEY=VALUE' }])
      } else {
        const key = eq.slice(0, eqIdx)
        const value = eq.slice(eqIdx + 1)
        agent.setVariable(key, value)
        commitLines([{ id: 'sys-env-set', kind: 'system', text: `  ${key}=${value}` }])
      }
    } else if (sub.startsWith('del ')) {
      const key = sub.slice(4).trim()
      agent.deleteVariable(key)
      commitLines([{ id: 'sys-env-del', kind: 'system', text: `  deleted: ${key}` }])
    } else {
      commitLines([{ id: 'sys-env-err', kind: 'system', text: '  Usage: /env [set K=V | del K]' }])
    }
  }

  async function handleCopyCommand() {
    // Scan committed history back-to-front for the most recent assistant
    // message and copy its raw markdown source (not the ANSI-rendered form).
    let raw: string | undefined
    for (let i = compactLines.length - 1; i >= 0; i--) {
      const l = compactLines[i]!
      if (l.kind === 'assistant' && l.rawMarkdown) { raw = l.rawMarkdown; break }
    }
    if (!raw || !raw.trim()) {
      commitLines([{ id: 'sys-copy', kind: 'system', text: '  No agent messages to copy yet.' }])
      return
    }
    try {
      const { copyToClipboard } = await import('../render/clipboard.js')
      await copyToClipboard(raw)
      commitLines([{ id: 'sys-copy', kind: 'system', text: '  Copied last agent message (Markdown source) to clipboard' }])
    } catch (err: any) {
      commitLines([{ id: 'sys-copy-err', kind: 'system', text: chalk.red(`  Copy failed: ${err?.message ?? err}`) }])
    }
  }

  async function handleSkillCommand(args: string) {
    const sub = args.trim()
    if (!sub || sub === 'list') {
      try {
        const { skillList } = await import('../commands/skill.js')
        commitLines([{ id: 'sys-skill', kind: 'system', text: skillList(agent.skillsDirs()) }])
      } catch {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  skill list unavailable' }])
      }
    } else if (sub.startsWith('install ')) {
      const source = sub.slice(8).trim()
      if (!source) {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill install <owner/repo>' }])
      } else {
        commitLines([{ id: 'sys-skill-inst', kind: 'system', text: `  cloning ${source}` }])
        renderer.requestRender()
        try {
          const { skillInstall } = await import('../commands/skill.js')
          const forked = agent.fork('You analyze skills and provide setup guides.')
          const result = await skillInstall(source, forked, (msg, level) => {
            commitLines([{ id: `sys-skill-${Date.now()}`, kind: 'system', text: `  ${msg}` }])
            renderer.requestRender()
          })
          if (result) commitLines([{ id: 'sys-skill-done', kind: 'system', text: `  ${result}` }])
        } catch (err: any) {
          commitLines([{ id: 'sys-skill-err', kind: 'system', text: chalk.red(`  install failed: ${err?.message ?? err}`) }])
        }
      }
    } else if (sub.startsWith('remove ')) {
      const name = sub.slice(7).trim()
      if (!name) {
        commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill remove <name>' }])
      } else {
        try {
          const { skillRemove } = await import('../commands/skill.js')
          commitLines([{ id: 'sys-skill-rm', kind: 'system', text: skillRemove(name) }])
        } catch {
          commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  skill remove unavailable' }])
        }
      }
    } else {
      commitLines([{ id: 'sys-skill-err', kind: 'system', text: '  Usage: /skill [list | install <source> | remove <name>]' }])
    }
    renderer.requestRender()
  }

  async function handleUpdateCommand() {
    commitLines([{ id: 'sys-upd', kind: 'system', text: '  checking for updates...' }])
    renderer.requestRender()
    try {
      const { runUpdate } = await import('../update/index.js')
      const { version } = await import('../native/index.js')
      const result = await runUpdate(version())
      switch (result.kind) {
        case 'up_to_date':
          commitLines([{ id: 'sys-upd-ok', kind: 'system', text: '  ✓ evot is up to date.' }])
          break
        case 'updated': {
          const lines: string[] = [`  ✓ updated ${result.from} → ${result.to}. restart evot to apply.`]
          if (result.notes && result.notes.length > 0) {
            lines.push('')
            lines.push(`  What's new in ${result.to}:`)
            for (const note of result.notes) {
              lines.push(`    • ${note}`)
            }
          }
          commitLines([{ id: 'sys-upd-ok', kind: 'system', text: lines.join('\n') }])
          break
        }
        case 'error':
          commitLines([{ id: 'sys-upd-err', kind: 'system', text: chalk.red(`  ✗ ${result.message}`) }])
          break
      }
    } catch (err: any) {
      commitLines([{ id: 'sys-upd-err', kind: 'system', text: chalk.red(`  ✗ update failed: ${err?.message ?? err}`) }])
    }
  }

  async function handleLogCommand(args: string) {
    const query = args.trim()
    const { join } = await import('path')
    const { homedir } = await import('os')
    const logDir = join(homedir(), '.evotai', 'logs')
    const sid = sessionId

    if (query.startsWith('up')) {
      // /log up [session_id] — upload/share session
      const upArg = query.slice(2).trim()
      let resolvedSid = upArg || sid
      if (!resolvedSid) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: '  No active session to upload.' }])
        return
      }
      if (upArg && upArg.length < 36) {
        try {
          const sessions = await agent.listSessions(20)
          const matches = sessions.filter(s => s.session_id.startsWith(upArg))
          if (matches.length === 0) {
            commitLines([{ id: 'sys-log-err', kind: 'system', text: `  Session not found: ${upArg}` }])
            return
          }
          if (matches.length > 1) {
            commitLines([{ id: 'sys-log-err', kind: 'system', text: `  Ambiguous session id: ${upArg} (${matches.length} matches)` }])
            return
          }
          resolvedSid = matches[0]!.session_id
        } catch (err: any) {
          commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Failed to list sessions: ${err?.message ?? err}`) }])
          return
        }
      }
      commitLines([{ id: 'sys-log-up', kind: 'system', text: `  packing session ${resolvedSid!.slice(0, 8)}...` }])
      renderer.requestRender()
      try {
        const { logPut } = await import('../commands/log-share.js')
        const result = await logPut(resolvedSid!)
        commitLines([{ id: 'sys-log-url', kind: 'system', text: `  uploaded. share this link:\n  ${result.url}\n  ⏳ link expires in 60 minutes` }])
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Export failed: ${err?.message ?? err}`) }])
      }
    } else if (query.startsWith('dl ')) {
      // /log dl <url#password>
      const dlUrl = query.slice(3).trim()
      if (!dlUrl) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: '  Usage: /log dl <url#password>' }])
        return
      }
      commitLines([{ id: 'sys-log-dl', kind: 'system', text: '  downloading and importing...' }])
      renderer.requestRender()
      try {
        const { logGet } = await import('../commands/log-share.js')
        const result = await logGet(dlUrl)
        commitLines([{ id: 'sys-log-dl-ok', kind: 'system', text: `  imported session: ${result.sessionId}\n  resume with: /resume ${result.sessionId.slice(0, 8)}` }])
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Import failed: ${err?.message ?? err}`) }])
      }
    } else if (!query) {
      const logPath = screenLog.filePath
      const markdownPath = screenLog.markdownTraceFilePath
      if (logPath) {
        const text = formatLogPaths(logPath, markdownPath)
        commitLines([{ id: 'sys-log', kind: 'system', text: text ?? `  Log: ${logPath}` }])
      }
      else if (sid) {
        const text = formatLogPaths(
          join(logDir, `${sid}.screen.log`),
          join(logDir, `${sid}.markdown.log`),
        )
        commitLines([{ id: 'sys-log', kind: 'system', text: text ?? `  Log: ${join(logDir, `${sid}.screen.log`)}` }])
      }
      else commitLines([{ id: 'sys-log', kind: 'system', text: `  Log dir: ${logDir} (no active session)` }])
    } else if (!sid) {
      commitLines([{ id: 'sys-log-err', kind: 'system', text: '  No active session to analyze.' }])
    } else {
      // /log <query> — fork agent to analyze log
      const logPath = join(logDir, `${sid}.screen.log`)
      const systemPrompt = [
        'You are in a temporary log analysis session.',
        'This session is not persisted and does not affect the main session context.',
        '',
        `Screen log file to analyze:\n${logPath}`,
        `Raw markdown trace, if present:\n${join(logDir, `${sid}.markdown.log`)}`,
        '',
        'Rules:',
        '- Read relevant log sections before answering; do not guess',
        '- Prefer partial reads; avoid loading the entire file at once',
        '- Use search to locate key information when needed',
        '- Do not modify any files',
      ].join('\n')
      try {
        const forked = agent.fork(systemPrompt)
        logMode = forked
        commitLines([{ id: 'sys-log-mode', kind: 'system', text: `  [log mode] analyzing: ${logPath}\n  not persisted. press Esc to exit.` }])
        renderer.requestRender()
        await runLogQuery(forked, query)
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Fork failed: ${err?.message ?? err}`) }])
      }
    }
    renderer.requestRender()
  }

  async function runLogQuery(forked: import('../native/index.js').ForkedAgent, prompt: string) {
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderer.requestRender()
    commitLines(buildUserMessage(prompt))

    try {
      const stream = await forked.query(prompt)
      streamRef = stream

      // Reuse the main streaming path so log-mode inherits pi-aligned behavior:
      // the whole message stays in the dynamic zone and only commits at
      // markdown-safe boundaries, so tables/lists/code blocks are never torn
      // (the old per-newline commit here split every table row into its own
      // buildAssistantLines call).
      for await (const event of stream) {
        if (destroyed) break
        if (!streamMachine) break

        const update = reduceRunEvent(streamMachine, event, { termRows: renderer.termRows })
        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState

        if (event.kind === 'assistant_delta') renderer.requestRender()
        if (!update.suppressToolStarted && event.kind === 'tool_started') commitToolStarted(event)
        if (event.kind === 'tool_progress') commitLines(buildToolProgressLines(event, true))
        if (!update.suppressToolFinished && event.kind === 'tool_finished') commitToolFinished(event)
        if (update.commitLines.length > 0) commitLines(update.commitLines)
        if (update.rerenderStatus) renderer.requestRender()
      }

      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        appState = final.state.appState
        commitFlushResult(final)
      }
    } catch (err: any) {
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Log query failed: ${err?.message ?? err}`) }])
    } finally {
      streamRef = null
      isLoading = false
      streamMachine = null
      stopSpinner()
      renderer.requestRender()
    }
  }

  function handleSelectorKey(event: KeyEvent) {
    if (overlay.kind !== 'selector') return
    const action = handleSelectorControl(overlay.state, event)

    switch (action.kind) {
      case 'update':
        overlay = { kind: 'selector', state: action.state }
        renderer.requestRender()
        return
      case 'close':
        overlay = { kind: 'none' }
        renderer.requestRender()
        return
      case 'resume':
        overlay = { kind: 'none' }
        resumeSession({ session_id: action.sessionId } as SessionMeta).then(() => renderer.requestRender())
        renderer.requestRender()
        return
      case 'history-goto':
        overlay = { kind: 'none' }
        handleSlashInput(`/goto ${action.seq}`)
        renderer.requestRender()
        return
      case 'history-preview':
        overlay = { kind: 'none' }
        commitLines([{ id: 'sys-hist-preview', kind: 'system', text: `  ${action.label} assistant: ${action.text}` }])
        renderer.requestRender()
        return
      case 'select-model':
        overlay = { kind: 'none' }
        agent.model = action.model
        syncProvider(agent, action.model, configInfo)
        refreshConfigInfo()
        appState = { ...appState, model: action.model }
        commitLines([{ id: 'sys-model', kind: 'system', text: `  Model → ${action.model}` }])
        renderer.requestRender()
        return
      case 'delete-session':
        overlay = { kind: 'selector', state: action.state }
        agent.deleteSession(action.sessionId).then(ok => {
          if (ok) {
            commitLines([{ id: 'sys-del', kind: 'system', text: `  Deleted session ${action.label}` }])
          }
        })
        renderer.requestRender()
        return
      case 'none':
        return
    }
  }

  function handleAskKey(event: KeyEvent) {
    if (overlay.kind !== 'ask-user') return

    const extra = event.type === 'char'
      ? event.char
      : event.type === 'paste'
        ? event.text
        : event.type === 'ctrl' && (event.key === 'n' || event.key === 'p')
          ? event.key
          : undefined
    const eventType = event.type === 'ctrl' && (event.key === 'n' || event.key === 'p')
      ? `ctrl+${event.key}`
      : event.type
    const result = handleAskKeyEvent(overlay.state, eventType, extra)

    switch (result.action) {
      case 'cancel':
        if (streamRef) {
          streamRef.abort()
          streamRef = null
        }
        overlay = { kind: 'none' }
        unfreezeTerminalTitle()
        isLoading = false
        flushStreamContent()
        streamMachine = null
        stopSpinner()
        commitLines([{ id: 'sys-ask-cancel', kind: 'system', text: '  ⏺ Cancelled.' }])
        renderer.requestRender()
        return
      case 'submit':
        if (streamRef) {
          const response = askStateToResponse(result.state)
          streamRef.respondAskUser(JSON.stringify({ Answered: response }))
          overlay = { kind: 'none' }
          unfreezeTerminalTitle()
          const answerLines: OutputLine[] = response.flatMap((r, i) => ([
            {
              id: `sys-ask-${i}-question`,
              kind: 'system' as const,
              text: `  • ${r.question}`,
            },
            {
              id: `sys-ask-${i}-answer`,
              kind: 'system' as const,
              text: `    → ${r.answer}`,
            },
          ]))
          commitLines(answerLines)
        }
        renderer.requestRender()
        return
      case 'update':
        overlay = { kind: 'ask-user', state: result.state }
        renderer.requestRender()
        return
    }
  }

  const disableRaw = enableRawMode(process.stdin)
  const disableEnhancedKeyboard = enableEnhancedKeyboard(process.stdout)
  const { stream: pasteStream, cleanup: cleanupPaste } = installBracketedPaste(process.stdin, () => {
    // Empty paste — likely Cmd+V with image in clipboard
    tryPasteImage()
  })
  pasteStream.on('data', (data: Buffer) => {
    const events = parseInput(data)
    for (const ev of events) handleKey(ev)
  })

  process.stdout.write('\x1b[?2004h')
  renderer.requestRender()

  function cleanup() {
    destroyed = true
    unfreezeTerminalTitle()
    stopSpinner()
    gitInfo.dispose()
    updateMgr.cleanup()
    if (exitHintTimer) clearTimeout(exitHintTimer)
    process.stdout.write('\x1b[?2004l')
    disableEnhancedKeyboard()
    setTerminalTitle()
    cleanupPaste()
    disableRaw()
    renderer.destroy()
  }

  process.on('SIGINT', () => { cleanup(); fastExit(130) })
  process.on('SIGTERM', () => { cleanup(); fastExit(143) })

  await new Promise<void>(() => {})
}

function commonPrefixOf(strings: string[]): string {
  if (strings.length === 0) return ''
  let prefix = strings[0]!
  for (let i = 1; i < strings.length; i++) {
    const s = strings[i]!
    let j = 0
    while (j < prefix.length && j < s.length && prefix[j] === s[j]) j++
    prefix = prefix.slice(0, j)
  }
  return prefix
}

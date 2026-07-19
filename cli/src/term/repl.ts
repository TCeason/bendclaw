import { TermRenderer, type RenderFrame } from './renderer.js'
import {
  enableRawMode,
  enableEnhancedKeyboard,
  type EnhancedKeyboardSession,
  type KeyEvent,
} from './input.js'
import { TerminalInputBuffer } from './input/buffer.js'
import { createSpinnerState, advanceSpinner, formatSpinnerLine, setSpinnerPhase, spinnerStatsFromLastUsage } from './spinner.js'
import { createSelectorState, selectorExpandItems, selectorClearQuery, selectorFocusOn, type SelectorItem } from './selector.js'
import { createAskState, handleAskKeyEvent, type AskQuestion } from './ask.js'
import { buildAssistantLines, buildUserMessage, messagesToOutputLines, type OutputLine } from '../render/output.js'
import { wrapTextWithAnsi } from '../render/wrap.js'
import { Agent, QueryStream, fastExit, type CompactionTask, type ManualCompactionOutcome, type SessionMeta, type ConfigInfo, type QueuedPrompt } from '../native/index.js'
import { createInitialState, type AppState } from './app/state.js'
import { assistantToolCalls } from './app/assistant-content.js'
import type { UIAssistantBlock } from './app/types.js'
import { assistantMessageToOutputLines } from '../render/assistant.js'
import { HistoryManager } from '../session/history.js'
import { ScreenLog } from '../session/screen-log.js'
import { RendererTrace } from '../session/renderer-trace.js'
import { findLastAssistantMarkdown } from '../session/assistant-markdown.js'
import { isSlashCommand, resolveCommand, buildHardenPrompt } from '../commands/index.js'
import { renderBanner } from './banner.js'
import {
  buildOutputBlocks,
  buildPromptBlocks,
  buildPromptFooterBlocks,
  buildOverlayBlocks,
  buildSelectorRegionLines,
  updateLiveHeight,
  formatQueuedMessageLines,
  blocksToLines,
  type OverlayState,
  type PromptVMInput,
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
  acceptCompletion,
  closeCompletion,
  moveCompletion,
  showCompletions,
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
  type StreamMachineState,
} from './app/stream.js'
import { handleSlashCommand } from './app/commands.js'
import { askStateToResponse } from './app/ask-user.js'
import { createExtensionHost, type ExtensionHost } from '../ext/index.js'
import type { AskUserAnswer, AskUserParams } from '../ext/index.js'
import { extractPlanItems, type PlanModeItem } from './plan-mode.js'
import { currentModelSpec, formatModelLabel, modelOptions, selectModelOption, sortModelOptionsForSelector } from './app/provider.js'
import chalk from 'chalk'
import {
  shouldCollapse,
  cleanPastedText,
  formatPastedTextRef,
  formatImageRef,
  parsePasteRefs,
  stripImageRefs,
  deleteRefBackspace,
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
import { findPreviousSession, shouldPreloadStartupSessions, selectResumeMessages, resumeElidedLine, resumeModelUnavailableNote } from './app/session-view.js'
import { handleSelectorControl } from './app/selector-control.js'
import { decideReplControl, type ReplControlAction } from './app/repl-control.js'
import { replaceOrPushStatusLine } from './app/status-line.js'
import { mergeQueuedIntoEditorText } from './app/queue-restore.js'
import {
  createQueueSelectorState,
  isQueueManageShortcut,
  type ManagedQueuedPrompt,
} from './app/queue-manage.js'
import { extractAtPrefix, completeAtFile } from '../commands/file-completion.js'
import { transcriptToMessages } from '../session/transcript.js'
import { GitInfoProvider } from './git-info.js'

const SPINNER_INTERVAL_MS = 100

type QueuedUserMessage = QueuedPrompt & { text: string; queue: 'steering' | 'follow_up' }
type QueuedCompactionSubmission = { displayText: string; expandedText: string; contentJson?: string }


export interface ReplOptions {
  agent: Agent
  resumeSessionId?: string
  continueLatest?: boolean
  serverPort?: number
  envFile?: string
}

export async function startRepl(opts: ReplOptions): Promise<void> {
  const { agent } = opts
  const { version } = await import('../native/index.js')
  const appVersion = version()
  const rendererTrace = new RendererTrace()
  const renderer = new TermRenderer({
    trace: rendererTrace.isEnabled ? entry => rendererTrace.log(entry) : undefined,
  })
  renderer.init()

  let appState: AppState = {
    ...createInitialState(agent.model, agent.cwd),
  }
  let spinnerState = createSpinnerState()
  let editor: EditorState = createEditorState()
  let historyState: HistoryState
  let isLoading = false
  let streamRef: QueryStream | null = null
  let compactionTask: CompactionTask | null = null
  let queuedCompactionSubmissions: QueuedCompactionSubmission[] = []
  let spinnerTimer: ReturnType<typeof setInterval> | null = null
  let titleFrozen = false
  let destroyed = false
  let disableRaw: (() => void) | null = null
  let enhancedKeyboard: EnhancedKeyboardSession | null = null
  let inputBuffer: TerminalInputBuffer | null = null
  let escapeFlushTimer: ReturnType<typeof setTimeout> | undefined
  let onInputData: ((data: Buffer | string) => void) | null = null
  let sessionId: string | null = null
  let planning = false
  let logMode: import('../native/index.js').ForkedAgent | null = null
  let exitHint = false
  let exitHintTimer: ReturnType<typeof setTimeout> | null = null
  let overlay: OverlayState = { kind: 'none' }
  // Promise-based bridge for the ask-user overlay. Both the `ask_user` host
  // tool and the plan-review flow present questions through the same overlay
  // and await the user's answers here. Resolves with the collected answers, or
  // null when the user cancels/skips.
  let pendingAsk: ((answers: AskUserAnswer[] | null) => void) | null = null

  /** Resolve any awaiting ask/plan-review overlay as cancelled. Safe to call
   *  on every teardown path (interrupt, cancel, overlay close, re-present) so a
   *  suspended host-tool dispatch never strands the run loop. */
  function resolvePendingAsk() {
    if (pendingAsk) {
      pendingAsk(null)
      pendingAsk = null
    }
  }

  function presentAskQuestions(questions: AskQuestion[]): Promise<AskUserAnswer[] | null> {
    // Only one ask overlay can be active at a time; resolve any prior one as
    // cancelled before opening the next.
    resolvePendingAsk()
    overlay = { kind: 'ask-user', state: createAskState(questions) }
    freezeTerminalTitle('?')
    renderer.requestRender()
    return new Promise(resolve => {
      pendingAsk = resolve
    })
  }

  // Extension host: owns ask_user and any future host tools. The engine
  // advertises their specs and delegates execution back here via
  // `host_tool_call` events (dispatched in the run loop below).
  const extensionHost: ExtensionHost = createExtensionHost({
    collectAnswers: (params: AskUserParams) =>
      presentAskQuestions(
        params.questions.map(q => ({
          header: q.header,
          question: q.question,
          options: q.options.map(o => ({ label: o.label, description: o.description })),
        })),
      ),
  })

  // Plan-mode state (pi-style): `/plan` enters read-only planning, the model
  // writes a `Plan:` section, and after the turn the extracted steps drive an
  // Execute / Stay / Refine review. Progress is not rendered as a sticky
  // checklist (it only advances on turn-end [DONE:n] tags and is easy to stick
  // when a step is never tagged). The review overlay owns the plan display.
  let planModeItems: PlanModeItem[] = []
  let lastReviewedPlanMarkdown = ''

  function latestAssistantMarkdown(): string | null {
    return findLastAssistantMarkdown(compactLines)?.rawMarkdown ?? null
  }

  async function maybeReviewPlanAfterTurn(): Promise<void> {
    if (!planning) return
    const markdown = latestAssistantMarkdown()
    if (!markdown || markdown === lastReviewedPlanMarkdown) return
    const extracted = extractPlanItems(markdown)
    if (extracted.length === 0) return

    lastReviewedPlanMarkdown = markdown
    planModeItems = extracted
    renderer.requestRender()

    const planList = planModeItems.map(item => `${item.step}. ☐ ${item.text}`).join('\n')
    const answers = await presentAskQuestions([
      {
        header: 'Plan',
        question: `Plan mode - what next?\n${planList}\n\nChoose an action, or type refinement feedback as custom text.`,
        options: [
          { label: 'Execute the plan', description: 'Leave plan mode and restore write tools.' },
          { label: 'Stay in plan mode', description: 'Keep planning without executing yet.' },
          { label: 'Refine the plan', description: 'Return to the prompt to enter refinement feedback.' },
        ],
      },
    ])
    if (!answers || answers.length === 0) return

    const choice = answers[0]!.answer
    if (choice === 'Execute the plan') {
      planning = false
      commitLines([{ id: 'sys-plan-exec', kind: 'system', text: '  planning: off · executing plan' }])
      const remaining = planModeItems
        .map(item => `${item.step}. ${item.text}`)
        .join('\n')
      // Drop the sticky checklist for execution: it only advanced on turn-end
      // [DONE:n] tags and stuck when a step was never tagged.
      planModeItems = []
      const execMessage = `Execute the plan.\n\nRemaining steps:\n${remaining}\n\nExecute each step in order.`
      commitLines(buildUserMessage(execMessage))
      await runQuery(execMessage)
      return
    }

    if (choice === 'Stay in plan mode' || choice === 'Skipped') {
      commitLines([{ id: 'sys-plan-stay', kind: 'system', text: '  planning: on · staying in plan mode' }])
      renderer.requestRender()
      return
    }

    if (choice === 'Refine the plan') {
      editor = insertText(clearEditor(editor), 'Refine the plan: ')
      commitLines([{ id: 'sys-plan-refine', kind: 'system', text: '  planning: on · enter refinement feedback' }])
      renderer.requestRender()
      return
    }

    commitLines(buildUserMessage(choice))
    await runQuery(choice)
  }
  let streamMachine: StreamMachineState | null = null
  // Messages sent mid-stream: held in the prompt zone (pi-style ❯ queue) and
  // committed to history at the turn boundary, so they never render above the
  // still-streaming reply.
  let queuedUserMessages: QueuedUserMessage[] = []
  let editingQueuedPrompt: ManagedQueuedPrompt | null = null
  let stashedQueueEditDraft = ''
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
  let fdAbort: AbortController | null = null
  const screenLog = new ScreenLog()
  let liveContentMaxHeight = 0
  let liveContentWidth = renderer.termCols

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
      provider: configInfo?.provider ?? '',
      planning,
      logMode: logMode !== null,
      dashboardUrl: serverState?.address ?? null,
      exitHint,
      completion: editor.completion,
      ghostHint: editor.ghostHint,
      columns: renderer.termCols,
      rows: renderer.termRows,
      placeholder: isEditorEmpty(editor) && !isLoading,
      cwd: appState.cwd,
      gitBranch: gitInfo.getBranch(),
      // Footer shows session state only (context/model/thinking). Per-call
      // token usage renders on the spinner; session totals belong to logs.
      contextTokens: appState.sessionTokens.contextTokens,
      contextWindow: appState.sessionTokens.contextWindow,
      thinkingLevel: configInfo?.thinkingLevel ?? '',
    }
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
      version: appVersion,
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
  // Live-partial memo: spinner ticks (10/s) and keystrokes repaint the frame
  // without changing the assistant content. The reducer replaces the content
  // array on every real change, so reference equality is an exact dirty check
  // and pure-repaint frames skip the Markdown pipeline entirely.
  const EMPTY_ASSISTANT_CONTENT: UIAssistantBlock[] = []
  let partialBlocksMemo: {
    content: UIAssistantBlock[]
    expanded: boolean
    streaming: boolean
    columns: number
    blocks: ViewBlock[]
  } | null = null

  function buildPartialAssistantBlocks(): ViewBlock[] {
    const content = streamMachine?.appState.currentAssistantContent ?? EMPTY_ASSISTANT_CONTENT
    // Only provider deltas are provisional. After assistant_completed the
    // message may remain live while tools execute, but its Markdown geometry
    // is final and trailing tables must become visible immediately.
    const streaming = spinnerState.streaming
    const columns = renderer.termCols
    if (
      partialBlocksMemo
      && partialBlocksMemo.content === content
      && partialBlocksMemo.expanded === expanded
      && partialBlocksMemo.streaming === streaming
      && partialBlocksMemo.columns === columns
    ) {
      return partialBlocksMemo.blocks
    }
    // Use the exact ordered committed-output pipeline for the live partial. This
    // keeps thinking/text/tool positions, margins, and prefixes stable through
    // completion instead of rendering tool calls in a detached layer.
    const blocks = buildOutputBlocks(assistantMessageToOutputLines(content, expanded, {
      streaming,
    }), {
      columns,
    })
    partialBlocksMemo = { content, expanded, streaming, columns, blocks }
    return blocks
  }

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
    if (cols !== liveContentWidth) {
      liveContentWidth = cols
      liveContentMaxHeight = 0
    }
    const cache = expanded ? expandedHistoryCache : compactHistoryCache
    const cachedHistoryLines = cache.sync(expanded ? expandedLines : compactLines, cols)
    if (cachedHistoryLines.length > 0) {
      blocks.push({ lines: cachedHistoryLines.map(l => ({ spans: [{ text: l }] })), marginTop: 0 })
    }

    // 3. Ordered partial assistant message (thinking/text/tool calls). Markdown
    // prefixes can legitimately reparse into fewer rows as a fence/list/table
    // becomes complete. Track history + partial as one region: when completion
    // moves the same content from partial into history its total height remains
    // continuous, and any transient parser shrink is absorbed above the footer.
    const partialBlocks = buildPartialAssistantBlocks()
    blocks.push(...partialBlocks)
    const livePartialHeight = blocksToLines(partialBlocks).length
    const liveContentHeight = cachedHistoryLines.length + livePartialHeight
    // The monotonic-height guard is only needed while visible partial content is
    // being reparsed. At the start of a fresh LLM call currentAssistantContent is
    // empty; retaining the previous call's peak then creates up to eight literal
    // blank rows above Thinking…. Reset immediately until the first visible block.
    const liveHeight = updateLiveHeight(
      liveContentMaxHeight,
      liveContentHeight,
      isLoading && livePartialHeight > 0,
    )
    liveContentMaxHeight = liveHeight.maxHeight
    if (liveHeight.padding > 0) {
      blocks.push({
        lines: Array.from({ length: liveHeight.padding }, () => ({ spans: [{ text: '' }] })),
        marginTop: 0,
      })
    }

    const contentLines = blocksToLines(blocks)
    const toolCalls = assistantToolCalls(streamMachine?.appState.currentAssistantContent ?? [])
    let spinnerBlock: ViewBlock | null = null
    // pi keeps statusContainer before editorContainer, so the active-run status
    // remains visible even while a selector replaces the editor.
    if (isLoading && overlay.kind !== 'ask-user') {
      const usagePending = streamMachine?.activeLlmCall ?? false
      const liveOutputTokens = usagePending && toolCalls.length === 0 ? spinnerState.tokenCount : 0
      // Usage arrives only when the provider completes this call. During an
      // active call, show only its live output estimate; retaining the previous
      // call here would present stale cache/input values as if they were current.
      const spinnerText = formatSpinnerLine(
        spinnerState,
        Date.now(),
        // Manual compaction reports no per-call usage of its own; showing the
        // previous run's tokens here would misattribute them to compaction.
        compactionTask
          ? undefined
          : spinnerStatsFromLastUsage(
              appState.currentRunStats.lastLlmUsage,
              liveOutputTokens,
              usagePending,
            ),
      )
      spinnerBlock = {
        lines: wrapTextWithAnsi(spinnerText, renderer.termCols).map(text => ({ spans: [{ text }] })),
        marginTop: 1,
      }
    }

    // Match pi's sibling order before editorContainer: pending messages, then
    // status. The queue manager suppresses the duplicate pending-message copy
    // because the selector itself is displaying those same entries.
    const preEditorBlocks: ViewBlock[] = []
    const queueManagerOpen = overlay.kind === 'selector' && overlay.state.title === 'Prompt queue'
    const queueLines = queueManagerOpen
      ? []
      : formatQueuedMessageLines([
          ...queuedUserMessages.map(message => message.text),
          ...queuedCompactionSubmissions.map(message => message.displayText),
        ])
    if (queueLines.length > 0) {
      preEditorBlocks.push({
        lines: queueLines.map(text => ({ spans: [{ text, dim: true }] })),
        marginTop: 1,
      })
      // Queue already owns the blank line above the input unit.
      if (spinnerBlock) spinnerBlock = { ...spinnerBlock, marginTop: 0 }
    }
    if (spinnerBlock) preEditorBlocks.push(spinnerBlock)

    // A selector replaces only pi's editorContainer. Its preceding queue/status
    // siblings and following footer sibling remain in normal document flow.
    if (overlay.kind === 'selector') {
      return {
        lines: [
          ...contentLines,
          ...blocksToLines(preEditorBlocks),
          ...buildSelectorRegionLines(overlay.state, renderer.termCols),
          ...blocksToLines(buildPromptFooterBlocks(getPromptVM())),
        ],
      }
    }

    const modalLines = blocksToLines(buildOverlayBlocks(overlay, renderer.termCols))
    const footerBlocks = [...preEditorBlocks]
    footerBlocks.push(...buildPromptBlocks(getPromptVM(), {
      attachedAbove: spinnerBlock !== null || queueLines.length > 0,
    }))

    return {
      lines: [...contentLines, ...blocksToLines(footerBlocks)],
      ...(modalLines.length > 0 ? { overlay: { lines: modalLines } } : {}),
    }
  }

  renderer.setRenderCallback(buildFrame)

  function outputContextFor(lines: OutputLine[]): { prevKind?: string; columns?: number } {
    const prev = lines.length > 0 ? lines[lines.length - 1] : undefined
    return {
      prevKind: prev?.kind,
      columns: renderer.termCols,
    }
  }

  function restoreLines(outputLines: OutputLine[]) {
    if (outputLines.length === 0) return
    compactLines.push(...outputLines)
    expandedLines.push(...outputLines)
    // Resume is a projection of persisted transcript state, not a new terminal
    // event. Paint it without appending it back into screen/markdown logs.
    renderer.requestRender()
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
    // Trigger re-render — buildFrame will pick up the new lines
    renderer.requestRender()
  }

  /** Commit a transient status line (model / thinking level). Rapid re-toggles
   *  replace the previous status in place instead of stacking a new line each
   *  time. Model and thinking share one status slot so alternating switches
   *  stay single-line. Only the trailing line is eligible for replacement, so a
   *  later user message or other output freezes the prior status into history. */
  function commitStatusLine(line: OutputLine) {
    const replaced = replaceOrPushStatusLine(compactLines, line)
    replaceOrPushStatusLine(expandedLines, line)
    // In-place mutation invalidates the append-only history cache prefix.
    if (replaced) resetHistoryCache()
    const context = outputContextFor(compactLines.slice(0, -1))
    const blocks = buildOutputBlocks([line], context)
    const rendered = blocksToLines(blocks)
    screenLog.logLines(rendered)
    renderer.requestRender()
  }

  /** Commit slash-command system lines, collapsing model/thinking status in place. */
  function commitSystemLines(outputLines: OutputLine[]) {
    for (const line of outputLines) {
      if (line.kind === 'system' && (line.id === 'sys-model' || line.id === 'sys-think')) {
        commitStatusLine(line)
      } else {
        commitLines([line])
      }
    }
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

  /** Toggle expanded view and redraw. */
  function toggleExpanded(): void {
    expanded = !expanded
    // An explicit Ctrl+O layout change should take effect immediately rather
    // than being mistaken for parser-induced shrink by the live-height guard.
    liveContentMaxHeight = 0
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
      commitStatusLine({ id: 'sys-think', kind: 'system', text: '  This model has no selectable thinking level' })
      return
    }
    refreshConfigInfo()
    const label = level === 'off' ? 'off' : level
    commitStatusLine({ id: 'sys-think', kind: 'system', text: `  Thinking level → ${label}` })
    renderer.requestRender()
  }

  function setTerminalTitle(suffix?: string, force = false) {
    if (titleFrozen && !force) return
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
      if (streamMachine) {
        streamMachine = { ...streamMachine, spinnerState }
      }
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
    // Always replace the final animated glyph. An ask overlay can keep the
    // title frozen while the run settles; normal title writes are correctly
    // blocked then, but the completed state must not remain stuck on ·/⠂/⠐.
    setTerminalTitle('✳', true)
  }

  async function resumeSession(session: SessionMeta) {
    try {
      const transcript = await agent.loadTranscript(session.session_id)
      // Fields may be missing when the caller passes a partial SessionMeta
      // (e.g. the resume selector only knows the id); fetch the full record.
      let model = session.model
      let provider = session.provider
      let thinkingLevel = session.thinking_level
      if (!model || !provider || thinkingLevel === undefined) {
        const full = await agent.findSession(session.session_id)
        if (full) {
          if (!model) model = full.model
          if (!provider) provider = full.provider
          if (thinkingLevel === undefined) thinkingLevel = full.thinking_level
        }
      }

      // Restore model selection when possible. A removed/renamed provider must
      // never block resume — keep the current live model and let the user pick
      // a replacement with /model after the transcript is painted.
      let modelRestoreNote: string | null = null
      if (model) {
        const preferred = provider ? `${provider}:${model}` : model
        try {
          agent.setProvider(preferred)
        } catch {
          // Do not force the raw model id onto the current provider: a missing
          // channel like `grok` would leave anthropic/openai holding a foreign
          // model name. Keep the live selection and surface a switch hint.
          const kept = agent.model
          modelRestoreNote = resumeModelUnavailableNote({
            provider: provider || undefined,
            model,
            keptModel: kept,
          })
        }
      }
      // Restore the session's reasoning effort so a resumed conversation keeps
      // the level it was last run with (no-op for non-reasoning models).
      if (thinkingLevel) {
        agent.restoreThinkingLevel(thinkingLevel)
      }

      sessionId = session.session_id
      rendererTrace.bind(session.session_id)
      refreshConfigInfo()
      appState = { ...appState, sessionId: session.session_id, model: agent.model }
      const { messagesToOutputLines } = await import('../render/output.js')
      const { transcriptToMessages } = await import('../session/transcript.js')
      const messages = transcriptToMessages(transcript as any)
      // A resumed session starts with no active plan; plan mode is re-entered
      // via /plan on the live conversation.
      planModeItems = []
      lastReviewedPlanMarkdown = ''
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
      if (hidden > 0) restoreLines([resumeElidedLine(hidden)])
      restoreLines(messagesToOutputLines(shown))
      restoreLines([
        { id: 'sys-resumed-gap', kind: 'system', text: '' },
        { id: 'sys-resumed', kind: 'system', text: chalk.dim(`  resumed session ${session.session_id.slice(0, 8)}`) },
      ])
      if (modelRestoreNote) {
        restoreLines([{ id: 'sys-resume-model', kind: 'system', text: chalk.dim(modelRestoreNote) }])
      }
    } catch (err: any) {
      commitLines([{ id: 'sys-err', kind: 'error', text: `Failed to resume: ${err?.message ?? err}` }])
    }
  }

  async function rebuildAfterManualCompaction(outcome: Extract<ManualCompactionOutcome, { status: 'compacted' }>) {
    if (!sessionId) return
    const transcript = await agent.loadContextTranscript(sessionId)
    const messages = transcriptToMessages(transcript as any).filter(message =>
      !(message.role === 'user' && message.text.startsWith('The conversation history before this point was compacted into the following summary:')),
    )
    const { shown, hidden } = selectResumeMessages(messages)

    appState = {
      ...appState,
      messages,
      sessionTokens: {
        ...appState.sessionTokens,
        contextTokens: outcome.tokens_after,
        contextWindow: outcome.context_window || appState.sessionTokens.contextWindow,
      },
    }
    renderer.clearScreen()
    compactLines.length = 0
    expandedLines.length = 0
    resetHistoryCache()
    if (hidden > 0) restoreLines([resumeElidedLine(hidden)])
    restoreLines(messagesToOutputLines(shown))

    const label = { id: 'sys-compact-label', kind: 'system' as const, text: '  [compaction]' }
    const status = {
      id: 'sys-compact-result',
      kind: 'system' as const,
      text: `  Compacted from ${outcome.tokens_before.toLocaleString()} to ${outcome.tokens_after.toLocaleString()} tokens (ctrl+o to expand)`,
    }
    compactLines.push(label, status)
    expandedLines.push(
      label,
      { ...status, text: `  Compacted from ${outcome.tokens_before.toLocaleString()} to ${outcome.tokens_after.toLocaleString()} tokens` },
      ...buildAssistantLines(outcome.summary),
    )
    if (outcome.used_fallback) {
      const fallback = {
        id: 'sys-compact-fallback',
        kind: 'system' as const,
        text: '  Note: the LLM summary was unavailable; a deterministic fallback summary was used.',
      }
      compactLines.push(fallback)
      expandedLines.push(fallback)
    }
    if (outcome.context_window > 0 && outcome.tokens_after >= outcome.context_window) {
      const warning = {
        id: 'sys-compact-warning',
        kind: 'error' as const,
        text: `Context is still ${outcome.tokens_after.toLocaleString()} tokens, above this model's ${outcome.context_window.toLocaleString()}-token window. Switch to a larger-context model or start a new session.`,
      }
      compactLines.push(warning)
      expandedLines.push(warning)
    }
    renderer.requestRender()
  }

  async function submitQueuedAfterCompaction() {
    const submissions = queuedCompactionSubmissions
    queuedCompactionSubmissions = []
    for (const submission of submissions) {
      commitLines(buildUserMessage(submission.displayText))
      await runQuery(submission.expandedText, submission.contentJson)
    }
  }

  async function runManualCompaction(customInstructions: string) {
    if (!sessionId) {
      commitLines([{ id: 'sys-compact', kind: 'system', text: '  Nothing to compact: no active session.' }])
      return
    }

    isLoading = true
    spinnerState = setSpinnerPhase(createSpinnerState(), 'executing', 'compact')
    startSpinner()
    compactionTask = agent.compact(sessionId, customInstructions || undefined)
    renderer.requestRender()
    try {
      const outcome = await compactionTask.result()
      if (outcome.status === 'compacted') {
        await rebuildAfterManualCompaction(outcome)
      } else if (outcome.status === 'cancelled') {
        commitLines([{ id: 'sys-compact-cancelled', kind: 'system', text: '  Compaction cancelled.' }])
      } else {
        commitLines([{ id: 'sys-compact-empty', kind: 'system', text: '  Nothing to compact.' }])
      }
    } catch (err: any) {
      commitLines([{ id: 'sys-compact-err', kind: 'error', text: `Compact failed: ${err?.message ?? err}` }])
    } finally {
      compactionTask = null
      isLoading = false
      stopSpinner()
      renderer.requestRender()
    }
    await submitQueuedAfterCompaction()
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
    liveContentMaxHeight = 0
    isLoading = true
    spinnerState = createSpinnerState()
    streamMachine = createStreamMachineState(appState, spinnerState)
    startSpinner()
    renderer.requestRender()

    let completed = false
    try {
      const stream = prebuiltStream
        ?? await agent.query(text, sessionId ?? undefined, planning ? 'planning_interactive' : 'interactive', contentJson, extensionHost.specsJson())
      streamRef = stream
      sessionId = stream.sessionId ?? sessionId
      appState = { ...appState, sessionId: sessionId }
      screenLog.bind(stream.sessionId)
      rendererTrace.bind(stream.sessionId)

      for await (const event of stream) {
        if (destroyed) break
        if (!streamMachine) break

        if (event.kind === 'host_tool_call') {
          // The engine delegated a host-owned tool (ask_user). Run it via the
          // extension host and send the result back. The tool's execute may
          // drive interactive UI (the ask overlay) and awaits the user.
          const call = (event.payload ?? {}) as {
            tool_name?: string
            tool_call_id?: string
            arguments?: Record<string, unknown>
          }
          if (call.tool_name && call.tool_call_id) {
            const response = await extensionHost.dispatch({
              tool_name: call.tool_name,
              tool_call_id: call.tool_call_id,
              arguments: call.arguments ?? {},
            })
            if (streamRef) {
              await streamRef.respondHostTool(JSON.stringify(response))
            }
          }
          continue
        }

        const update = reduceRunEvent(streamMachine!, event, { termRows: renderer.termRows })

        streamMachine = update.state
        appState = update.state.appState
        spinnerState = update.state.spinnerState

        // Git commands run inside tool subprocesses. Refresh synchronously when
        // any tool settles instead of waiting for the debounced HEAD watcher;
        // otherwise the completed answer can still render the previous branch.
        if (event.kind === 'tool_finished') gitInfo.refresh()

        // Request re-render on each delta so streaming text appears
        if (event.kind === 'assistant_delta') {
          renderer.requestRender()
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
            renderer.requestRender()
          } else {
            commitLines(update.commitLines)
          }
        }

        // Reconcile against the native queue instead of draining the visible
        // copy wholesale: OneAtATime mode may consume only the first of several
        // queued prompts at this boundary.
        if (event.kind === 'turn_started') reconcileQueuedUserMessages()

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
      // Safety net: commit only prompts the engine actually consumed. A prompt
      // queued during the final poll can still be pending when the run settles.
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
      completed = true
    } catch (err: any) {
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      commitLines([{ id: 'sys-err', kind: 'error', text: err?.message ?? String(err) }])
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
    } finally {
      unfreezeTerminalTitle()
      streamRef = null
      isLoading = false
      streamMachine = null
      stopSpinner()
      renderer.requestRender()
    }

    if (!completed) return

    await maybeReviewPlanAfterTurn()
  }

  function handleKey(event: KeyEvent) {
    if (editingQueuedPrompt) {
      if (event.type === 'escape' || (event.type === 'ctrl' && event.key === 'c')) {
        cancelQueueEdit()
        return
      }
      if (isQueueManageShortcut(event)) {
        commitLines([{ id: 'sys-queue-edit-lock', kind: 'system', text: '  Finish or discard the queued prompt edit first.' }])
        return
      }
    }
    if (isQueueManageShortcut(event)) {
      if (overlay.kind === 'selector' && overlay.state.title === 'Prompt queue') {
        overlay = { kind: 'none' }
        renderer.requestRender()
      } else if (streamRef && queuedUserMessages.length > 0) {
        openQueueSelector()
      }
      return
    }

    const actions = decideReplControl({
      event,
      overlay,
      isLoading,
      hasStream: streamRef !== null,
      editor,
      exitHint,
      logMode: logMode !== null,
      hasQueuedPrompt: queuedUserMessages.length > 0,
      isCompacting: compactionTask !== null,
    })

    for (const action of actions) {
      if (applyReplControlAction(action, event)) return
    }
  }

  function applyReplControlAction(action: ReplControlAction, event: KeyEvent): boolean {
    switch (action.kind) {
      case 'restore-queued':
        restoreLastQueuedUserMessageToEditor()
        return true
      case 'interrupt':
        if (compactionTask) {
          compactionTask.abort()
          return true
        }
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
      case 'close-completion':
        editor = closeCompletion(editor)
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
        // The selector is part of the normal frame. Match pi's overlay lifecycle:
        // removing it is a regular differential render, not a forced reset.
        // An ask overlay can be closed without a stream (e.g. leftover overlay);
        // resolve its awaiting promise so nothing stays suspended.
        if (overlay.kind === 'ask-user') resolvePendingAsk()
        overlay = { kind: 'none' }
        renderer.requestRender()
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
        if (event.type === 'char' || event.type === 'shift-char') {
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
  }

  function interruptStream(id: string, text: string) {
    unfreezeTerminalTitle()
    // If an ask/plan-review overlay is awaiting, resolve it as cancelled so the
    // suspended host-tool dispatch in runQuery unblocks instead of hanging the
    // run loop forever.
    resolvePendingAsk()
    if (streamRef) {
      streamRef.abort()
      streamRef = null
    }
    isLoading = false
    flushStreamContent()
    streamMachine = null
    stopSpinner()
    // Mid-stream queue was steered but the run is aborted — put it back in the
    // editor so the user can edit and press Enter, instead of committing it as
    // history under the Interrupted notice.
    restoreQueuedUserMessagesToEditor()
    commitLines([{ id, kind: 'system', text }])
  }

  function queueEntryText(entry: QueuedPrompt): string {
    const message = entry.message as { role?: string; content?: Array<{ type?: string; text?: string }> }
    const text = message.content
      ?.filter(content => content.type === 'text' && typeof content.text === 'string')
      .map(content => content.text)
      .join('\n')
      .trim()
    return text || '(non-text prompt)'
  }

  function managedQueueEntries(): ManagedQueuedPrompt[] {
    if (!streamRef) return []
    const visible = new Map(queuedUserMessages.map(message => [message.id, message.text]))
    try {
      const collect = (queue: 'steering' | 'follow_up') => streamRef!
        .queuedPrompts(queue)
        .map(entry => ({
          queue,
          id: entry.id,
          version: entry.version,
          text: visible.get(entry.id) ?? queueEntryText(entry),
        }))
      return [...collect('steering'), ...collect('follow_up')]
    } catch {
      return []
    }
  }

  function openQueueSelector() {
    const entries = managedQueueEntries()
    if (entries.length === 0) {
      overlay = { kind: 'none' }
      commitLines([{ id: 'sys-queue-empty', kind: 'system', text: '  No queued prompts.' }])
      return
    }
    overlay = { kind: 'selector', state: createQueueSelectorState(entries) }
    renderer.requestRender()
  }

  function editQueuedPrompt(entry: ManagedQueuedPrompt) {
    if (!streamRef) return
    editingQueuedPrompt = entry
    stashedQueueEditDraft = getEditorText(editor)
    clearAll()
    editor = insertText(editor, entry.text)
    overlay = { kind: 'none' }
    commitLines([{ id: 'sys-queue-edit', kind: 'system', text: '  Editing queued prompt · Enter save · Esc discard' }])
    renderer.requestRender()
  }

  function finishQueueEdit() {
    editingQueuedPrompt = null
    clearAll()
    editor = insertText(editor, stashedQueueEditDraft)
    stashedQueueEditDraft = ''
    renderer.requestRender()
  }

  function cancelQueueEdit() {
    finishQueueEdit()
    commitLines([{ id: 'sys-queue-edit-cancel', kind: 'system', text: '  Queue edit discarded.' }])
  }

  function saveQueueEdit(text: string) {
    if (!streamRef || !editingQueuedPrompt || !text.trim()) return
    const entry = editingQueuedPrompt
    try {
      const updated = streamRef.updateQueuedPrompt(entry.queue, entry.id, entry.version, text)
      queuedUserMessages = queuedUserMessages.map(message => message.id === entry.id
        ? { ...message, version: updated.version, text }
        : message)
      finishQueueEdit()
      commitLines([{ id: 'sys-queue-edit-save', kind: 'system', text: '  Queued prompt updated.' }])
    } catch (err: any) {
      const current = managedQueueEntries().find(candidate => candidate.id === entry.id)
      if (current) editingQueuedPrompt = { ...current, text }
      else finishQueueEdit()
      commitLines([{ id: 'sys-queue-err', kind: 'system', text: chalk.red(`  Queue edit failed: ${err?.message ?? err}`) }])
      renderer.requestRender()
    }
  }

  function removeQueuedPrompt(entry: ManagedQueuedPrompt) {
    if (!streamRef) return
    try {
      streamRef.removeQueuedPrompt(entry.queue, entry.id, entry.version)
      queuedUserMessages = queuedUserMessages.filter(message => message.id !== entry.id)
      openQueueSelector()
    } catch (err: any) {
      reconcileQueuedUserMessages()
      commitLines([{ id: 'sys-queue-err', kind: 'system', text: chalk.red(`  Queue remove failed: ${err?.message ?? err}`) }])
      openQueueSelector()
    }
  }

  /** Pull the newest queued prompt back into the editor without
   *  aborting the active run. Native optimistic version matching prevents an
   *  already-consumed prompt from being silently edited. */
  function restoreLastQueuedUserMessageToEditor() {
    if (!streamRef || queuedUserMessages.length === 0) return
    const queued = queuedUserMessages[queuedUserMessages.length - 1]!
    try {
      streamRef.removeQueuedPrompt(queued.queue, queued.id, queued.version)
      queuedUserMessages = queuedUserMessages.slice(0, -1)
      const next = mergeQueuedIntoEditorText([queued.text], getEditorText(editor))
      editor = insertText(clearEditor(editor), next)
      renderer.requestRender()
    } catch {
      // The engine already consumed it at a turn boundary; normal event handling
      // will commit the visible copy to history.
    }
  }

  /** Move mid-stream queued messages into the input box after an interrupt. */
  function restoreQueuedUserMessagesToEditor() {
    if (queuedUserMessages.length === 0) return
    const messages = queuedUserMessages.map(message => message.text)
    queuedUserMessages = []
    const next = mergeQueuedIntoEditorText(messages, getEditorText(editor))
    editor = insertText(clearEditor(editor), next)
    renderer.requestRender()
  }

  function formatLogPaths(
    logPath: string | null,
    rendererPath: string | null = null,
  ): string | null {
    if (!logPath) return null
    const lines = [`  Log: ${logPath}`]
    if (rendererPath) lines.push(`  Renderer run: ${rendererPath}`)
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
    // A slash command may be typed with images attached; probe the visible
    // draft too so "/compact" plus an image ref is still treated as a command.
    const commandProbe = trimmed || displayText.trim()
    if (compactionTask) {
      if (commandProbe && isSlashCommand(commandProbe)) {
        // Not silently swallowed: commands cannot run mid-compaction and are
        // not queueable prompts. Keep the draft in the editor for later.
        commitLines([{ id: 'sys-compact-cmd', kind: 'system', text: "  Commands don't run during compaction. Press Esc to cancel it, or wait for it to finish." }])
        renderer.requestRender()
        return
      }
      if (trimmed || imageBlocks) {
        const queuedDisplay = displayText || '(image prompt)'
        queuedCompactionSubmissions.push({
          displayText: queuedDisplay,
          expandedText,
          ...(imageBlocks ? { contentJson: JSON.stringify(imageBlocks) } : {}),
        })
        historyMgr.append(queuedDisplay)
        historyState = pushHistory(historyState, queuedDisplay)
        clearAll()
        renderer.requestRender()
      }
      return
    }
    if (editingQueuedPrompt) {
      saveQueueEdit(expandedText)
      return
    }
    if (trimmed === '/log') {
      clearAll()
      const logPath = screenLog.filePath
      if (logPath) {
        const text = formatLogPaths(logPath, rendererTrace.filePath)
        commitLines([{ id: 'sys-log', kind: 'system', text: text ?? `  Log: ${logPath}` }])
      }
      else commitLines([{ id: 'sys-log', kind: 'system', text: '  No active screen log.' }])
      renderer.requestRender()
      return
    }

    // Slash commands are not model input: queueing one as follow-up text
    // would send "/compact" to the LLM as conversation. Keep the draft in the
    // editor and tell the user instead.
    if (commandProbe && isSlashCommand(commandProbe) && streamRef) {
      commitLines([{ id: 'sys-cmd-busy', kind: 'system', text: "  Commands don't queue while a response is running. Press Esc to interrupt, or wait for the turn to finish." }])
      renderer.requestRender()
      return
    }

    if ((expandedText || imageBlocks) && streamRef) {
      if (imageBlocks) {
        const contentJson = JSON.stringify(imageBlocks)
        const queued = streamRef.followUp('', contentJson)
        queuedUserMessages.push({ ...queued, text: displayText || '(image prompt)', queue: 'follow_up' })
      } else {
        const queued = streamRef.followUp(expandedText)
        if (displayText) queuedUserMessages.push({ ...queued, text: displayText, queue: 'follow_up' })
      }
      // Save to input history like a normal submission.
      if (displayText) {
        historyMgr.append(displayText)
        historyState = pushHistory(historyState, displayText)
      }
      // Queue instead of committing now: history renders above the streaming
      // block, so an immediate commit lands above the incoming reply.
      // Plain and structured prompts are follow-ups, matching grok's default
      // "finish the current turn, then drain FIFO" behavior.
      clearAll()
      renderer.requestRender()
    }
  }

  /** Commit queued prompts that are no longer present in either native queue. */
  function reconcileQueuedUserMessages() {
    if (queuedUserMessages.length === 0 || !streamRef) return
    let remainingIds: Set<string>
    try {
      remainingIds = new Set([
        ...streamRef.queuedPrompts('steering'),
        ...streamRef.queuedPrompts('follow_up'),
      ].map(entry => entry.id))
    } catch {
      return
    }
    const remaining: QueuedUserMessage[] = []
    for (const message of queuedUserMessages) {
      if (remainingIds.has(message.id)) remaining.push(message)
      else commitLines(buildUserMessage(message.text))
    }
    queuedUserMessages = remaining
    if (remaining.length === 0 && overlay.kind === 'selector' && overlay.state.title === 'Prompt queue') {
      overlay = { kind: 'none' }
    }
    if (editingQueuedPrompt && !remainingIds.has(editingQueuedPrompt.id)) {
      finishQueueEdit()
      commitLines([{ id: 'sys-queue-edit-consumed', kind: 'system', text: '  Queued prompt was already consumed; edit closed.' }])
    }
  }

  function refreshFileCompletions(acceptSingle: boolean): void {
    const lineIndex = editor.cursorLine
    const cursorCol = editor.cursorCol
    const beforeCursor = editor.lines[lineIndex]!.slice(0, cursorCol)
    const prefix = extractAtPrefix(beforeCursor)
    if (!prefix) return

    fdAbort?.abort()
    const controller = new AbortController()
    fdAbort = controller
    completeAtFile(beforeCursor, appState.cwd, controller.signal).then(result => {
      if (controller.signal.aborted) return
      const currentBefore = editor.lines[lineIndex]?.slice(0, cursorCol)
      if (editor.cursorLine !== lineIndex || editor.cursorCol !== cursorCol || currentBefore !== beforeCursor) return
      if (!result) {
        editor = closeCompletion(editor)
      } else {
        const items = result.items.map(item => ({
          label: item.label,
          value: item.value + (item.isDirectory ? '' : ' '),
        }))
        editor = showCompletions(editor, items, result.prefixStart, cursorCol)
        if (acceptSingle && items.length === 1) editor = acceptCompletion(editor)
      }
      renderer.requestRender()
    }).catch(() => {})
  }

  function deleteAtCursor() {
    const line = editor.lines[editor.cursorLine]!
    const deletedRef = parsePasteRefs(line).find(ref => ref.start === editor.cursorCol)
    if (deletedRef) {
      pastedChunks.delete(deletedRef.id)
      pastedImages.delete(deletedRef.id)
    }
    editor = deleteForward(editor)
    renderer.requestRender()
  }

  function deleteWordAtCursor() {
    const lineIndex = editor.cursorLine
    const cursorCol = editor.cursorCol
    const refs = parsePasteRefs(editor.lines[lineIndex]!)
    const nextEditor = deleteWordBefore(editor)
    if (nextEditor.cursorLine === lineIndex) {
      for (const ref of refs) {
        if (ref.start < cursorCol && ref.end > nextEditor.cursorCol) {
          pastedChunks.delete(ref.id)
          pastedImages.delete(ref.id)
        }
      }
    }
    editor = nextEditor
    renderer.requestRender()
  }

  function handleNormalKey(event: KeyEvent) {
    if (editor.completion) {
      if (event.type === 'up' || event.type === 'down') {
        editor = moveCompletion(editor, event.type === 'up' ? -1 : 1)
        renderer.requestRender()
        return
      }
      if (event.type === 'enter' || event.type === 'tab') {
        editor = acceptCompletion(editor)
        renderer.requestRender()
        return
      }
    }

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
          deleteAtCursor()
          return
        case 'w':
          deleteWordAtCursor()
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
        const beforeCursor = editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol)
        if (extractAtPrefix(beforeCursor)) {
          refreshFileCompletions(true)
          break
        }
        const result = applyCompletion(editor)
        if (result.applied) {
          editor = result.state
          renderer.requestRender()
        }
        break
      }
      case 'char':
      case 'shift-char':
        editor = refreshGhostHint(insertText(editor, event.char))
        renderer.requestRender()
        if (extractAtPrefix(editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol))) {
          refreshFileCompletions(false)
        }
        break
      case 'paste':
        insertPaste(event.text)
        renderer.requestRender()
        break
      case 'delete':
        deleteAtCursor()
        break
      case 'backspace': {
        const currentLine = editor.lines[editor.cursorLine]!
        const refs = parsePasteRefs(currentLine)
        const refDel = deleteRefBackspace(currentLine, editor.cursorCol, refs)
        if (refDel) {
          const deletedRef = refs.find(ref => ref.end === editor.cursorCol)
          if (deletedRef) {
            pastedChunks.delete(deletedRef.id)
            pastedImages.delete(deletedRef.id)
          }
          const lines = [...editor.lines]
          lines[editor.cursorLine] = refDel.newLine
          editor = {
            ...editor,
            lines,
            cursorCol: refDel.newCursorCol,
            preferredVisualCol: undefined,
            ghostHint: '',
            completion: null,
          }
        } else {
          editor = backspace(editor)
        }
        editor = refreshGhostHint(editor)
        renderer.requestRender()
        if (extractAtPrefix(editor.lines[editor.cursorLine]!.slice(0, editor.cursorCol))) {
          refreshFileCompletions(false)
        }
        break
      }
      case 'left':
        editor = moveLeft(editor)
        renderer.requestRender()
        break
      case 'right':
        editor = moveRight(editor)
        renderer.requestRender()
        break
      case 'home':
        editor = moveHome(editor)
        renderer.requestRender()
        break
      case 'end':
        editor = moveEnd(editor)
        renderer.requestRender()
        break
      case 'up': {
        const moved = moveUp(editor, Math.max(1, renderer.termCols - 2))
        if (moved !== editor) {
          editor = moved
          renderer.requestRender()
          break
        }
        // At the top visual row: navigate history.
        const result = historyPrev(historyState, editor)
        if (result.changed) {
          historyState = result.history
          editor = result.editor
          renderer.requestRender()
        }
        break
      }
      case 'down': {
        const moved = moveDown(editor, Math.max(1, renderer.termCols - 2))
        if (moved !== editor) {
          editor = moved
          renderer.requestRender()
          break
        }
        // At the bottom visual row: navigate history.
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
    // Model configuration can change while the CLI is running. Refresh before
    // resolving `/model` so the selector and model cycling use the latest env file.
    const pendingCommand = resolveCommand(text)
    if (pendingCommand.kind === 'resolved' && pendingCommand.name === '/model') {
      try {
        configInfo = agent.configInfo()
      } catch (err: any) {
        commitLines([{ id: 'sys-model-config', kind: 'system', text: chalk.red(`  Failed to reload model config: ${err?.message ?? err}`) }])
        renderer.requestRender()
        return
      }
    }

    let result
    try {
      result = handleSlashCommand(text, {
      agent,
      appState,
      configInfo,
      preloadedSessions,
      planning,
    })
    } catch (err: any) {
      commitLines([{ id: 'sys-command-err', kind: 'system', text: chalk.red(`  Command failed: ${err?.message ?? err}`) }])
      renderer.requestRender()
      return
    }
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
      planModeItems = []
      lastReviewedPlanMarkdown = ''
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
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      // Start and bind a fresh empty session so /resume can see it immediately.
      const newSession = await agent.createSession()
      sessionId = newSession.session_id
      rendererTrace.bind(newSession.session_id)
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
    if (result.systemLines.length > 0) commitSystemLines(result.systemLines)

    // Handle async commands that the simple handleSlashCommand can't do
    const resolved = resolveCommand(text)
    if (resolved.kind !== 'resolved') {
      renderer.requestRender()
      return
    }
    const { name, args } = resolved

    if (name === '/model' && args) {
      refreshConfigInfo()
      appState = { ...appState, model: agent.model }
    }

    if (name === '/plan') {
      planModeItems = []
      lastReviewedPlanMarkdown = ''
      renderer.requestRender()
    }

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
    } else if (name === '/compact') {
      await runManualCompaction(args)
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
        planModeItems = []
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
      const models = modelOptions(configInfo, agent.model)
      if (models.length > 1) {
        const activeSpec = currentModelSpec(configInfo, agent.model)
        const sortedModels = sortModelOptionsForSelector(models, activeSpec)
        const items: SelectorItem[] = sortedModels.map(option => ({
          label: option.model,
          detail: option.provider,
          id: option.spec,
          selected: option.spec === activeSpec,
          searchText: `${option.model} ${option.provider}`,
        }))
        overlay = {
          kind: 'selector',
          state: selectorFocusOn(
            {
              ...createSelectorState('Models', items),
              presentation: 'model',
              circularNavigation: true,
            },
            item => item.id === activeSpec,
          ),
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
    // Last assistant raw markdown → clipboard (shared locator with plan / shot).
    const last = findLastAssistantMarkdown(compactLines)
    if (!last) {
      commitLines([{ id: 'sys-copy', kind: 'system', text: '  No agent messages to copy yet.' }])
      return
    }
    try {
      const { copyToClipboard } = await import('../render/clipboard.js')
      await copyToClipboard(last.rawMarkdown)
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

    if (query === 'shot' || query.startsWith('shot ')) {
      // /log shot exports the latest committed assistant turn from in-memory
      // history. This keeps renderer diagnostics off the TUI hot path.
      const unsupportedTarget = query.slice(4).trim()
      if (unsupportedTarget) {
        commitLines([{ id: 'sys-log-shot-target', kind: 'system', text: '  /log shot exports the latest assistant turn; message ids are no longer supported.' }])
        return
      }
      try {
        const { writeMarkdownShot } = await import('../commands/log-shot.js')
        const result = await writeMarkdownShot({
          historyLines: compactLines,
          columns: renderer.termCols,
          open: false,
          header: {
            model: appState.model || agent.model,
            thinkingLevel: configInfo?.thinkingLevel,
            sessionId: sessionId ?? undefined,
            cwd: agent.cwd,
            branch: gitInfo.getBranch() ?? undefined,
          },
        })
        const lines = [
          `  Shot: ${result.messageId}${result.chunkCount > 1 ? ` (${result.chunkCount} chunks)` : ''}`,
          `  HTML: ${result.htmlPath}`,
        ]
        if (result.pngPath) lines.push(`  PNG:  ${result.pngPath}`)
        else lines.push('  PNG:  (Chrome not available — HTML only)')
        commitLines([{ id: 'sys-log-shot', kind: 'system', text: lines.join('\n') }])
      } catch (err: any) {
        commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Shot failed: ${err?.message ?? err}`) }])
      }
    } else if (query.startsWith('up')) {
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
      if (logPath) {
        const text = formatLogPaths(logPath, rendererTrace.filePath)
        commitLines([{ id: 'sys-log', kind: 'system', text: text ?? `  Log: ${logPath}` }])
      }
      else if (sid) {
        const text = formatLogPaths(join(logDir, `${sid}.screen.log`))
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
    liveContentMaxHeight = 0
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
        if (update.commitLines.length > 0) commitLines(update.commitLines)
        if (event.kind === 'turn_started') reconcileQueuedUserMessages()
        if (update.rerenderStatus) renderer.requestRender()
      }

      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        appState = final.state.appState
        commitFlushResult(final)
      }
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
    } catch (err: any) {
      if (streamMachine) {
        const final = flushStreaming(streamMachine)
        streamMachine = final.state
        commitFlushResult(final)
      }
      commitLines([{ id: 'sys-log-err', kind: 'system', text: chalk.red(`  Log query failed: ${err?.message ?? err}`) }])
      reconcileQueuedUserMessages()
      restoreQueuedUserMessagesToEditor()
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
      case 'select-model': {
        overlay = { kind: 'none' }
        try {
          agent.setProvider(action.spec)
          refreshConfigInfo()
          const selected = selectModelOption(configInfo, action.spec)
          const model = selected?.model ?? agent.model
          const provider = selected?.provider ?? configInfo?.provider ?? ''
          appState = { ...appState, model }
          commitStatusLine({ id: 'sys-model', kind: 'system', text: `  Model → ${formatModelLabel(model, provider)}` })
        } catch (err: any) {
          commitLines([{ id: 'sys-model-err', kind: 'system', text: chalk.red(`  Failed to switch model: ${err?.message ?? err}`) }])
        }
        renderer.requestRender()
        return
      }
      case 'delete-session':
        overlay = { kind: 'selector', state: action.state }
        agent.deleteSession(action.sessionId).then(ok => {
          if (ok) {
            commitLines([{ id: 'sys-del', kind: 'system', text: `  Deleted session ${action.label}` }])
          }
        })
        renderer.requestRender()
        return
      case 'queue-edit':
        editQueuedPrompt(action.entry)
        return
      case 'queue-remove':
        removeQueuedPrompt(action.entry)
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
        // Resolve the awaiting host tool as cancelled; the tool maps this to a
        // tool error so the engine's run continues rather than hanging.
        resolvePendingAsk()
        overlay = { kind: 'none' }
        unfreezeTerminalTitle()
        commitLines([{ id: 'sys-ask-cancel', kind: 'system', text: '  ⏺ Cancelled.' }])
        renderer.requestRender()
        return
      case 'submit':
        {
          const response = askStateToResponse(result.state)
          if (pendingAsk) {
            pendingAsk(response)
            pendingAsk = null
          }
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

  disableRaw = enableRawMode(process.stdin)
  const terminalInput = new TerminalInputBuffer({
    onEmptyPaste: tryPasteImage,
    onControl: event => enhancedKeyboard?.handleControl(event),
  })
  inputBuffer = terminalInput
  const dispatchInputEvents = (events: KeyEvent[]) => {
    for (const event of events) {
      if (destroyed) break
      handleKey(event)
    }
  }
  const inputHandler = (data: Buffer | string) => {
    if (escapeFlushTimer) {
      clearTimeout(escapeFlushTimer)
      escapeFlushTimer = undefined
    }
    dispatchInputEvents(terminalInput.write(data))
    if (terminalInput.hasAmbiguousEscape) {
      escapeFlushTimer = setTimeout(() => {
        escapeFlushTimer = undefined
        dispatchInputEvents(terminalInput.flushPending())
      }, 10)
    }
  }
  onInputData = inputHandler
  process.stdin.on('data', inputHandler)
  enhancedKeyboard = enableEnhancedKeyboard(process.stdout)

  process.stdout.write('\x1b[?2004h')
  renderer.requestRender()

  function cleanup() {
    if (destroyed) return
    destroyed = true
    queuedCompactionSubmissions = []
    unfreezeTerminalTitle()
    compactionTask?.abort()
    streamRef?.abort()
    stopSpinner()
    gitInfo.dispose()
    updateMgr.cleanup()
    if (exitHintTimer) clearTimeout(exitHintTimer)
    if (escapeFlushTimer) {
      clearTimeout(escapeFlushTimer)
      escapeFlushTimer = undefined
    }
    if (onInputData) {
      process.stdin.off('data', onInputData)
      onInputData = null
    }
    inputBuffer?.discard()
    inputBuffer = null
    process.stdout.write('\x1b[?2004l')
    enhancedKeyboard?.dispose()
    enhancedKeyboard = null
    setTerminalTitle()
    disableRaw?.()
    disableRaw = null
    renderer.destroy()
    rendererTrace.close()
  }

  process.on('SIGINT', () => { cleanup(); fastExit(130) })
  process.on('SIGTERM', () => { cleanup(); fastExit(143) })

  await new Promise<void>(() => {})
}

import { describe, test, expect } from 'bun:test'
import { renderMarkdown, formatToken } from '../src/render/markdown.js'
import { getTheme } from '../src/render/theme.js'
import chalk from 'chalk'
import { marked, type Token } from 'marked'
import stripAnsi from 'strip-ansi'

// Helper: render markdown and strip ANSI codes for assertion
function render(md: string): string {
  return stripAnsi(renderMarkdown(md))
}

// Helper: lex a single token from markdown
function lexFirst(md: string): Token {
  const tokens = marked.lexer(md)
  return tokens[0]!
}

describe('renderMarkdown', () => {
  test('renders plain text', () => {
    expect(render('hello world')).toBe('hello world')
  })

  test('returns empty/whitespace input as-is', () => {
    expect(renderMarkdown('')).toBe('')
    expect(renderMarkdown('  ')).toBe('  ')
  })

  test('renders headings', () => {
    const result = render('# Title')
    expect(result).toContain('Title')
  })

  test('renders h2', () => {
    const result = render('## Subtitle')
    expect(result).toContain('Subtitle')
  })

  test('renders bold text', () => {
    const result = render('this is **bold** text')
    expect(result).toContain('bold')
  })

  test('renders italic text', () => {
    const result = render('this is *italic* text')
    expect(result).toContain('italic')
  })

  test('renders inline code', () => {
    const result = render('use `foo()` here')
    expect(result).toContain('foo()')
  })

  test('renders unclosed code fence as code', () => {
    const result = render('```sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(result).not.toContain('```')
  })

  test('fenced code blocks render with a left gutter', () => {
    // Code blocks and prose must be visually distinct. Without a gutter,
    // back-to-back prose and code collapse into indistinguishable
    // paragraphs on terminals without syntax highlighting.
    const md = '介绍一下：\n```bash\nnpm install\n```\n之后运行。'
    const result = render(md)
    expect(result).toMatch(/│ npm install/)
  })

  test('renders unclosed tilde fence as code', () => {
    const result = render('~~~sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(result).not.toContain('~~~')
  })

  test('repairs unclosed code fence before later prose', () => {
    const md = '```json\n[\n  {"id":"evt-001"}\n]\n\n原样保存，没有任何转换。'
    const result = render(md)
    expect(result).toContain('{"id":"evt-001"}')
    expect(result).toContain('原样保存')
    expect(result).not.toContain('```')
  })

  test('splits fence close glued to following heading', () => {
    const result = render('```json\n{\n  "id": "tr-abc"\n}\n```### 5.1 接口')
      .replace(/\u200b/g, '')

    expect(result).toContain('"id": "tr-abc"')
    expect(result).toContain('5.1 接口')
    expect(result).not.toContain('```')
  })

  test('splits fence close glued to heading without a space', () => {
    // Models often omit the space in CJK contexts: `\`\`\`##改进清单`.
    // We should normalise it so marked sees `## 改进清单`.
    const result = render('```json\n{"x":1}\n```##改进清单（共 8 项，零语义风险）')
      .replace(/\u200b/g, '')

    expect(result).toContain('"x":1')
    expect(result).toContain('改进清单（共 8 项，零语义风险）')
    expect(result).not.toContain('##改进清单')
    expect(result).not.toContain('```')
  })

  test('promotes ATX heading glued after a preceding paragraph', () => {
    // `…零语义风险） ##粘连` — split the heading onto its own line.
    const result = render('目标保持不变。 ##粘连').replace(/\u200b/g, '')

    expect(result).toContain('目标保持不变。')
    expect(result).toContain('粘连')
    // Should not keep `) ##` on a single line.
    expect(result).not.toMatch(/目标保持不变。\s*##/)
  })

  test('promotes ATX heading glued with zero space after CJK punctuation', () => {
    // Models often drop the space entirely: `。###档 1`.
    const result = render('每档独立可落地。###档 1 ·零改动').replace(/\u200b/g, '')

    expect(result).toContain('每档独立可落地。')
    expect(result).toContain('档 1 ·零改动')
    expect(result).not.toMatch(/。###/)
  })

  test('promotes ATX heading glued with zero space after CJK character', () => {
    // No punctuation at all — `这个###档 1` still needs a line break.
    const result = render('追加两节： 比如这个###档 1').replace(/\u200b/g, '')

    expect(result).toContain('比如这个')
    expect(result).toContain('档 1')
    expect(result).not.toMatch(/这个###/)
  })

  test('promotes ATX heading glued before a body that has its own space', () => {
    // `前言说明。### 一句话总结` — the heading body is well-formed (`###`
    // followed by a space), only the preceding prose is glued to the marker.
    const result = render('前言说明。### 一句话总结\n\n正文').replace(/\u200b/g, '')

    expect(result).toContain('前言说明。')
    expect(result).toContain('一句话总结')
    expect(result).toContain('正文')
    expect(result).not.toMatch(/。###/)
  })

  test('normalises bullet marker glued to CJK body', () => {
    // `-这个改动` / `-summary 的详略` — CommonMark needs `- body`.
    const result = render('-这个改动是不是也管用？\n- 另一条').replace(/\u200b/g, '')
    // Should render as a list: bullet + body, not a dash-prefixed paragraph.
    expect(result).toContain('- 这个改动是不是也管用？')
    expect(result).toContain('- 另一条')
  })

  test('does not normalise `-`/`*` that are not bullet intents', () => {
    // Guard rails — make sure we don't mangle negatives, CLI flags, HR, or
    // emphasis markers.
    expect(render('-1.5 is negative').replace(/\u200b/g, '')).toContain('-1.5 is negative')
    expect(render('--flag is an option').replace(/\u200b/g, '')).toContain('--flag is an option')
    expect(render('---').replace(/\u200b/g, '')).toContain('---')
    expect(render('use *emphasis* here').replace(/\u200b/g, '')).toContain('emphasis')
  })

  test('splits bullet glued to end of prose after a colon', () => {
    // CJK colon `：` is the trigger; prose itself can be ASCII.
    const result = render('consensus：- sliced by something').replace(
      /\u200b/g,
      '',
    )
    expect(result).toContain('consensus：')
    expect(result).toContain('- sliced by something')
    expect(result).not.toMatch(/consensus：[ \t]*- /)
  })

  test('splits ordered list glued to end of prose after a colon', () => {
    // CJK colon `：` is the trigger; prose itself can be ASCII.
    const result = render('consensus：1. must have baseline').replace(/\u200b/g, '')
    expect(result).toContain('consensus：')
    expect(result).toContain('must have baseline')
    expect(result).not.toMatch(/consensus：[ \t]*1\. /)
  })

  test('normalises ordered item glued to non-ASCII body', () => {
    // `3.多指标` — CommonMark needs `3. body`. CJK body is essential:
    // ORDERED_MISSING_SPACE_RE only fires when the body is non-ASCII so
    // decimals/versions stay intact.
    const result = render('1. first\n2.二\n3.三').replace(/\u200b/g, '')
    expect(result).toContain('二')
    expect(result).toContain('三')
    expect(result).not.toContain('2.二')
    expect(result).not.toContain('3.三')
  })

  test('does not normalise decimals/versions as ordered list', () => {
    // Guard rails — decimals and version strings must stay intact.
    expect(render('3.14 is pi').replace(/\u200b/g, '')).toContain('3.14')
    expect(render('see v1.2.3 for details').replace(/\u200b/g, '')).toContain('v1.2.3')
    expect(render('192.168.1.1 is local').replace(/\u200b/g, '')).toContain(
      '192.168.1.1',
    )
  })

  test('does not split inline JSON-like objects with decimals after a colon', () => {
    // Guard rails for ORDERED_GLUED_AFTER_COLON_RE: `task_1: 0.8` is a
    // colon + decimal, not a colon + ordered marker. Splitting it would
    // rip the line apart and the decimal would become a fake list number.
    const md = 'scores_by_task: {task_1: 0.8, task_2: 1.6}'
    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('{task_1: 0.8, task_2: 1.6}')
    expect(result).not.toMatch(/^\s*0\.\s+8/m)
    expect(result).not.toMatch(/^\s*1\.\s+6/m)
  })

  test('dedents over-indented ordered item so it stays in the list', () => {
    // Models routinely over-indent mid-list items (`     3.多`). ≥4 spaces
    // would otherwise trigger a code block or lazy continuation. CJK body
    // is essential to trigger the dedent regex.
    const md = '1. first item\n2. second item\n     3.三'
    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('三')
    expect(result).not.toContain('     3.')
  })

  test('normalises ATX heading missing the space after #', () => {
    const result = render('##改进清单').replace(/\u200b/g, '')
    expect(result).toContain('改进清单')
    expect(result).not.toContain('##改进清单')
  })

  test('normalises single-# heading when body is non-ASCII', () => {
    // `#改进清单` is a valid CJK heading intent. We only rewrite single-`#`
    // when the body is non-ASCII so `#1` / `#include` stay untouched.
    const result = render('#改进清单').replace(/\u200b/g, '')
    expect(result).toContain('改进清单')
    expect(result).not.toContain('#改进清单')
  })

  test('preserves `#1` and `#include` (single-# ASCII body)', () => {
    // Issue refs and preprocessor directives must not become headings.
    const issue = render('see #1 for context')
    expect(issue).toContain('#1')
    const preproc = render('#include <stdio.h>')
    expect(preproc).toContain('#include')
  })

  test('splits fence close glued to following list item', () => {
    const result = render('```ts\nconst a = 1\n```- 命中规则')

    expect(result).toContain('const a = 1')
    expect(result).toContain('- 命中规则')
    expect(result).not.toContain('```')
  })

  test('splits fence close glued to following inline bold prose', () => {
    const result = render('```rust\nErr(ToolError::Failed(format!("binary")))\n```**为什么安全**：只加元数据，不动内容。')

    expect(result).toContain('ToolError::Failed')
    expect(result).toContain('为什么安全')
    expect(result).toContain('只加元数据')
    expect(result).not.toContain('```')
  })

  test('splits fence close glued to following chinese prose', () => {
    const result = render('```text\nsrc/engine/\nsrc/engine/Cargo.toml\n```**为什么安全**：多给信息不压信息。')

    expect(result).toContain('src/engine/Cargo.toml')
    expect(result).toContain('多给信息不压信息')
    expect(result).not.toContain('```')
  })

  test('splits trailing fence close glued to last code line', () => {
    // Models sometimes omit the newline before the closing fence, e.g.
    // `    }\`\`\``. The renderer used to leak literal backticks; we now
    // split the marker onto its own line so marked sees a clean code block.
    const md = '输出:\n```json\n   {\n  "diagnoses": ["a"],\n  "hypotheses": ["b"]\n    }```'
    const result = render(md)
    expect(result).toContain('"diagnoses"')
    expect(result).toContain('"hypotheses"')
    expect(result).not.toContain('```')
  })

  test('repairs unclosed fence before following markdown heading without a blank line', () => {
    const result = render('```json\n{\n  "id": "tr-abc"\n}\n## 第 8 站：补充 input / output')

    expect(result).toContain('"id": "tr-abc"')
    expect(result.replace(/\u200b/g, '')).toContain('第 8 站：补充 input / output')
    expect(result).not.toContain('```json')
  })

  test('repairs completed json fence before plain chinese paragraph', () => {
    const result = render('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n原始事件继续说明')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toContain('原始事件继续说明')
    expect(result).not.toContain('```json')
  })

  test('repairs completed json fence before markdown hr without a blank line', () => {
    const result = render('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n---\n第 8 站：补充 input / output')
      .replace(/\u200b/g, '')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toContain('---')
    expect(result).toContain('第 8 站：补充 input / output')
    expect(result).not.toContain('```json')
  })

  test('keeps adjacent prose compact', () => {
    const result = render('第一行\n第二行\n第三行')

    expect(result).toBe('第一行\n第二行\n第三行')
  })

  test('keeps list items compact', () => {
    const result = render('- 第一项\n- 第二项\n- 第三项')

    expect(result).toBe('- 第一项\n- 第二项\n- 第三项')
  })

  test('wraps very long plain lines', () => {
    const prev = process.stdout.columns
    process.stdout.columns = 40
    try {
      const result = render('INSERT ' + 'x'.repeat(80))
      expect(result.split('\n').length).toBeGreaterThan(1)
    } finally {
      process.stdout.columns = prev
    }
  })

  test('keeps list item continuation lines hanging indented', () => {
    const prev = process.stdout.columns
    process.stdout.columns = 42
    try {
      const result = render('- 检查 `cli/src/render/markdown.ts` 的渲染链路：这是很长很长很长很长很长的一行')
        .replace(/\u200b/g, '')
      const lines = result.split('\n')

      expect(lines.length).toBeGreaterThan(1)
      expect(lines[0]).toStartWith('- ')
      expect(lines.slice(1).every(line => line.startsWith('  '))).toBe(true)
      expect(lines.slice(1).every(line => !line.startsWith('- '))).toBe(true)
    } finally {
      process.stdout.columns = prev
    }
  })

  test('does not insert word boundaries around every CJK punctuation mark', () => {
    const result = renderMarkdown('中文，标点。继续：说明')

    expect(result).not.toContain(`\u200b，`)
    expect(result).not.toContain(`，\u200b`)
    expect(result).not.toContain(`\u200b。`)
    expect(result).not.toContain(`。\u200b`)
    expect(result).not.toContain(`\u200b：`)
    expect(result).not.toContain(`：\u200b`)
  })

  test('inserts pangu space between CJK and latin/digit characters', () => {
    // `一条trace` reads better (and wraps better) as `一条 trace`.
    const result = render('钉住一条trace，把它当 target').replace(/\u200b/g, '')
    expect(result).toContain('一条 trace')
    expect(result).not.toMatch(/一条trace/)
  })

  test('does not touch latin/digit inside inline code and links', () => {
    // Inline code must stay verbatim so copy-paste matches source.
    const inline = render('用 `eval()` 跑一下 42次').replace(/\u200b/g, '')
    expect(inline).toContain('eval()')
    // Surrounding CJK↔digit still gets a space: `42次` → `42 次`.
    expect(inline).toContain('42 次')
  })

  test('detects markdown syntax after the first 500 characters', () => {
    const result = render(`${'a'.repeat(520)}\n\n# Tail heading`)

    expect(result).toContain('Tail heading')
    expect(result).not.toContain('# Tail heading')
  })

  test('renders code blocks', () => {
    const result = render('```js\nconst x = 1\n```')
    expect(result).toContain('const x = 1')
  })

  test('renders unordered lists', () => {
    const result = render('- one\n- two\n- three')
    expect(result).toContain('- one')
    expect(result).toContain('- two')
    expect(result).toContain('- three')
  })

  test('renders ordered lists', () => {
    const result = render('1. first\n2. second')
    expect(result).toContain('1.')
    expect(result).toContain('first')
    expect(result).toContain('second')
  })

  test('renders blockquotes', () => {
    const result = render('> quoted text')
    expect(result).toContain('quoted text')
  })

  test('blockquote text is italic but not dimmed', () => {
    // Dimming long CJK quotes on dark backgrounds is nearly invisible. We
    // keep italic for emphasis but drop the dim grey foreground.
    const theme = getTheme()
    expect(theme.blockquoteText.paint('x')).toBe(chalk.italic('x'))
    const raw = renderMarkdown('> 引用的一段中文文本')
    expect(raw).toContain(theme.blockquoteText.paint('引用的一段中文文本'))
  })

  test('renders links', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = render('[click](https://example.com)')
      expect(result).toContain('click')
      // With OSC 8 on, the URL is attached as a hyperlink target inside the
      // escape sequence (not stripped by stripAnsi, so test the raw output).
      const raw = renderMarkdown('[click](https://example.com)')
      expect(raw).toContain('https://example.com')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('link fallback without OSC 8 shows only the display text', () => {
    // Claudecode-style: when hyperlinks are off we drop the trailing `(url)`
    // to keep prose quiet. The raw URL only appears if there is no text.
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '0'
    try {
      const result = render('[click](https://example.com)')
      expect(result).toContain('click')
      expect(result).not.toContain('(https://example.com)')
      expect(result).not.toContain('https://example.com')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('link fallback without OSC 8 shows bare URL when there is no text', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '0'
    try {
      const result = render('<https://example.com>')
      expect(result).toContain('https://example.com')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('h1 heading is bold italic underlined', () => {
    // Follow claudecode: h1 gets bold+italic+underline, no hue.
    const theme = getTheme()
    expect(theme.h1.paint('Title')).toBe(chalk.bold.italic.underline('Title'))
    const raw = renderMarkdown('# Title')
    expect(raw).toContain(theme.h1.paint('Title'))
  })

  test('h2 heading is plain bold without colour', () => {
    // h2+ is bold only; coloured headings feel chatty in long responses.
    const theme = getTheme()
    expect(theme.h2.paint('Subtitle')).toBe(chalk.bold('Subtitle'))
    const raw = renderMarkdown('## Subtitle')
    expect(raw).toContain(theme.h2.paint('Subtitle'))
    // No 24-bit RGB colour sequences should be emitted for the heading body.
    expect(raw).not.toContain('\x1b[38;2;')
  })

  test('renders horizontal rules', () => {
    // Claudecode-style: literal `---`, not a box-drawing row.
    const result = render('---')
    expect(result).toContain('---')
  })

  test('splits hr glued to end of sentence without whitespace', () => {
    // Models sometimes emit `通用框架。---\n核心抽象` (no space/newline before
    // the --- marker). Treat it as a thematic break, not literal dashes.
    const result = render('通用框架。---\n核心抽象三个独立').replace(/\u200b/g, '')
    expect(result).toContain('通用框架。')
    expect(result).toContain('核心抽象三个独立')
    expect(result).not.toMatch(/通用框架。---/)
  })

  test('splits hr glued before heading', () => {
    const result = render('---### 方案分层：从零代码到完整 Eval').replace(/\u200b/g, '')

    expect(result).toContain('---\n\n方案分层：从零代码到完整 Eval')
    expect(result).not.toContain('---###')
  })

  test('does not split em-dash mid-sentence', () => {
    // Plain em-dash usage with surrounding spaces must stay intact.
    const result = render('foo --- bar').replace(/\u200b/g, '')
    expect(result).toContain('foo --- bar')
  })

  test('preserves hand-drawn box art whitespace via code-fence wrap', () => {
    // Hand-drawn boxes go through a code-fence wrapper so marked does not
    // collapse the internal whitespace as paragraph text. We do NOT pad
    // ambiguous-width pictographs (terminals disagree about their width),
    // so the fix only guarantees the block is preserved verbatim.
    const box = [
      '┌──────────────────┐',
      '│ Datafuse         │',
      '├──────────────────┤',
      '│ 🏠 Dashboard     │',
      '│ 🛠  Evaluators   │',
      '│ ▶  Eval Runs     │',
      '└──────────────────┘',
    ].join('\n')
    const rendered = stripAnsi(renderMarkdown(box)).replace(/\u200b/g, '')
    // Every original line should appear verbatim in the output.
    for (const line of box.split('\n')) {
      expect(rendered).toContain(line)
    }
    // And no raw fence markers should leak through.
    expect(rendered).not.toContain('```')
  })

  test('preserves box art with non-border interior lines', () => {
    // Regression: models sometimes emit boxes whose interior includes lines
    // that don't start with `│` (labels like `Error`, `Trace → ...`). The
    // block detector used to truncate at the first such line, letting the
    // remainder leak into paragraph parsing — where stray ASCII `|` got
    // treated as GFM table column separators, producing misaligned output.
    // By matching `┌...┐`/`└...┘` pairs we keep the whole block inside one
    // code fence regardless of interior line shape.
    const box = [
      '┌─ Samples ─┐',
      '│ ▼ × failed    a1b2c3d4e5f6…   8.2s   │ │',
      'Error   | |',
      '  no numeric metrics in judge output:    | |',
      '  instruction_following,groundedness,... | |',
      'Trace  → 7efc7485db736bcaa114efe991d8cb3 ↗   | |',
      'Sample → dsi_motxmny0_e252389b               | |',
      '[↻ Retry this sample]                        │',
      '└────────────┘',
    ].join('\n')
    const rendered = stripAnsi(renderMarkdown(box)).replace(/\u200b/g, '')
    for (const line of box.split('\n')) {
      expect(rendered).toContain(line)
    }
    // The interior `| |` tokens must NOT be promoted into a GFM table — a
    // table render would produce header separator lines like `┌─┬─┐` on
    // their own at the top of the output, which we assert absent here.
    expect(rendered).not.toContain('```')
    // Ensure the `Error` line keeps its trailing `| |` text intact rather
    // than being split across synthesized table columns.
    expect(rendered).toContain('Error   | |')
  })

  test('preserves tree-style directory listings with branch connectors', () => {
    // `tree`-style output uses `├──` / `└──` connectors but no closed
    // `┌────┐` border. Without special handling the lines get merged as
    // paragraph text (consecutive connectors collapse onto one line) and
    // indentation-only whitespace is lost, producing misaligned output.
    const tree = [
      'evot/',
      '├── .gitignore',
      '├── Cargo.lock',
      '├── Cargo.toml',
      '├── src/',
      '│   ├── app/',
      '│   │   └── src/',
      '│   │       └── lib.rs',
      '│   └── engine/',
      '│       └── src/',
      '│           └── lib.rs',
      '└── cli/',
      '    └── src/',
      '        └── cli.ts',
    ].join('\n')
    const rendered = stripAnsi(renderMarkdown(tree)).replace(/\u200b/g, '')
    for (const line of tree.split('\n')) {
      expect(rendered).toContain(line)
    }
    expect(rendered).not.toContain('```')
  })

  test('renders tables with box-drawing characters', () => {
    const md = '| A | B |\n|---|---|\n| 1 | 2 |'
    const result = render(md)
    expect(result).toContain('A')
    expect(result).toContain('B')
    expect(result).toContain('1')
    expect(result).toContain('2')
    expect(result).toContain('┌')
    expect(result).toContain('┐')
    expect(result).toContain('│')
    expect(result).toContain('├')
    expect(result).toContain('┤')
    expect(result).toContain('└')
    expect(result).toContain('┘')
  })

  test('aligns emoji-capable symbols in tables using terminal width', () => {
    const md = [
      '| 状态 | 工具 | 说明 |',
      '|---|---|---|',
      '| ▶ | 🛠Read | 读取文件内容 |',
      '| ▶ | 🛠Edit | 编辑已有文件 |',
      '| ▶ | 🛠Bash | 执行 shell 命令 |',
      '| ▶ | 🛠Grep | 搜索代码 |',
    ].join('\n')
    const result = render(md).replace(/\u200b/g, '')
    const lines = result.split('\n').filter(Boolean)

    expect(lines).toContain('│ ▶    │ 🛠Read │ 读取文件内容    │')
    expect(lines).toContain('│ ▶    │ 🛠Edit │ 编辑已有文件    │')
  })

  test('collapses excessive newlines', () => {
    const result = renderMarkdown('hello\n\n\n\nworld')
    expect(result).not.toContain('\n\n\n')
  })

  test('falls back to raw text on parse error', () => {
    // renderMarkdown should never throw
    const result = renderMarkdown('just plain text')
    expect(result).toContain('just plain text')
  })

  test('strips system-reminder prompt tags before rendering', () => {
    // Models occasionally echo the reminder envelope into visible output.
    // Match claudecode's stripPromptXMLTags: drop the tags and their body.
    const result = render('hello\n<system-reminder>\ninternal only\n</system-reminder>\nworld')
    expect(result).toContain('hello')
    expect(result).toContain('world')
    expect(result).not.toContain('system-reminder')
    expect(result).not.toContain('internal only')
  })

  test('strips claudecode-style analysis tags', () => {
    const result = render('<commit_analysis>\nhidden\n</commit_analysis>\nvisible body')
    expect(result).toContain('visible body')
    expect(result).not.toContain('hidden')
    expect(result).not.toContain('commit_analysis')
  })
})

describe('formatToken', () => {
  test('renders paragraph token', () => {
    const token = lexFirst('hello world')
    const result = stripAnsi(formatToken(token))
    expect(result).toContain('hello world')
  })

  test('renders space token as newline', () => {
    const result = formatToken({ type: 'space', raw: '\n\n' } as Token)
    expect(result).toBe('\n')
  })

  test('renders br token as newline', () => {
    const result = formatToken({ type: 'br', raw: '\n' } as Token)
    expect(result).toBe('\n')
  })

  test('renders escape token as text', () => {
    const result = formatToken({ type: 'escape', raw: '\\)', text: ')' } as Token)
    expect(result).toBe(')')
  })

  test('renders hr as horizontal line', () => {
    const result = stripAnsi(formatToken({ type: 'hr', raw: '---' } as Token))
    expect(result).toContain('---')
  })

  test('renders image as href', () => {
    const result = formatToken({ type: 'image', raw: '![alt](url)', href: 'https://img.png', text: 'alt' } as Token)
    expect(result).toBe('https://img.png')
  })

  test('returns empty string for unknown token types', () => {
    const result = formatToken({ type: 'html', raw: '<div>' } as Token)
    expect(result).toBe('')
  })
})

// ---------------------------------------------------------------------------
// File path linkification in codespan and text
// ---------------------------------------------------------------------------

describe('file path linkification', () => {
  const OSC8_START = '\x1b]8;;'

  test('codespan with absolute path produces file:// hyperlink', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('see `/tmp/simple.md`')
      expect(result).toContain(OSC8_START)
      expect(result).toContain('file:///tmp/simple.md')
      // The path text should still be present
      expect(stripAnsi(result)).toContain('/tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('codespan with non-path content does not linkify', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('use `foo()` here')
      expect(result).not.toContain(OSC8_START)
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('plain text with absolute path produces file:// hyperlink', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '1'
    try {
      const result = renderMarkdown('已生成：/tmp/simple.md')
      expect(result).toContain(OSC8_START)
      expect(result).toContain('file:///tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })

  test('no hyperlink when FORCE_HYPERLINK=0', () => {
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '0'
    try {
      const result = renderMarkdown('see `/tmp/simple.md`')
      expect(result).not.toContain(OSC8_START)
      expect(stripAnsi(result)).toContain('/tmp/simple.md')
    } finally {
      if (prev === undefined) delete process.env.FORCE_HYPERLINK
      else process.env.FORCE_HYPERLINK = prev
    }
  })
})

import { splitMarkdownBlocks } from '../src/render/markdown.js'

describe('splitMarkdownBlocks', () => {
  test('empty text returns empty', () => {
    expect(splitMarkdownBlocks('')).toEqual({ completed: '', pending: '' })
  })

  test('single paragraph without blank line stays pending', () => {
    const result = splitMarkdownBlocks('hello world')
    expect(result.completed).toBe('')
    expect(result.pending).toBe('hello world')
  })

  test('two paragraphs split at blank line', () => {
    const text = 'paragraph one\n\nparagraph two'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('paragraph one\n\n')
    expect(result.pending).toBe('paragraph two')
  })

  test('multiple paragraphs split at last blank line', () => {
    const text = 'para one\n\npara two\n\npara three'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('para one\n\npara two\n\n')
    expect(result.pending).toBe('para three')
  })

  test('code fence keeps content pending until closed', () => {
    const text = 'intro\n\n```js\nconst x = 1\n```\n\nafter'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('intro')
    expect(result.completed).toContain('```')
    expect(result.pending).toBe('after')
  })

  test('unclosed code fence keeps everything pending', () => {
    const text = 'intro\n\n```js\nconst x = 1\nmore code'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('intro\n\n')
    expect(result.pending).toBe('```js\nconst x = 1\nmore code')
  })

  test('unclosed code fence can commit following markdown after heuristic repair', () => {
    const text = '```json\n[\n  {"id":"evt-001"}\n]\n\n## next'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('```json\n[\n  {"id":"evt-001"}\n]\n\n')
    expect(result.pending).toBe('## next')
  })

  test('unclosed code fence can commit following horizontal rule after heuristic repair', () => {
    const text = '最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n---\n第 8 站：补充 input / output'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('最终合并结果：\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n')
    expect(result.pending).toBe('---\n第 8 站：补充 input / output')
  })

  test('trailing blank line makes everything completed', () => {
    const text = 'hello world\n\n'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toBe('hello world\n\n')
    expect(result.pending).toBe('')
  })

  test('heading followed by paragraph', () => {
    const text = '# Title\n\nSome text\n\nMore text'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('# Title')
    expect(result.completed).toContain('Some text')
    expect(result.pending).toBe('More text')
  })

  test('tilde code fence handled', () => {
    const text = 'before\n\n~~~\ncode\n~~~\n\nafter'
    const result = splitMarkdownBlocks(text)
    expect(result.completed).toContain('before')
    expect(result.completed).toContain('~~~')
    expect(result.pending).toBe('after')
  })
})

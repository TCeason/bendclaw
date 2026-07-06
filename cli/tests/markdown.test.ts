import { describe, test, expect } from 'bun:test'
import { renderMarkdown } from '../src/render/markdown.js'
import { formatToken } from '../src/markdown/render/ansi.js'
import { getTheme, resetThemeCache } from '../src/render/theme.js'
import chalk from 'chalk'
import { marked, type Token } from 'marked'
import stripAnsi from 'strip-ansi'
import stringWidth from 'string-width'
import { withColumns } from './helpers/stdout-columns.js'

// Helper: render markdown and strip ANSI codes for assertion
function render(md: string): string {
  return stripAnsi(renderMarkdown(md))
}

// Helper: assert every ``` fence sits alone on its line and is never glued to
// adjacent content. Code blocks now render WITH visible ```lang / ``` borders
// (aligned with pi), so the old `not.toContain('```')` proxy — which meant
// "the fence was consumed / didn't leak / didn't glue to a neighbor" — is
// expressed directly here: a fence line is exactly ``` optionally followed by a
// language tag, with nothing else on the line.
function fencesWellFormed(rendered: string): boolean {
  for (const line of rendered.split('\n')) {
    if (!line.includes('```')) continue
    if (!/^```[\w+-]*$/.test(line.trim())) return false
  }
  return true
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

  test('keeps the ### prefix for H3–H6, drops it for H1/H2 (pi-aligned)', () => {
    // H1/H2 are visually distinct via styling, but H3–H6 all render as plain
    // bold, so the hash prefix is what makes their levels distinguishable.
    expect(render('# H1')).not.toContain('# H1')
    expect(render('## H2')).not.toContain('## H2')
    expect(render('### H3')).toContain('### H3')
    expect(render('#### H4')).toContain('#### H4')
    expect(render('##### H5')).toContain('##### H5')
    expect(render('###### H6')).toContain('###### H6')
  })

  test('renders indented h3 headings', () => {
    const result = render('2 行 verbose，工具调用是视觉主体。\n\n  ### 改造后（ctrl+o 展开，和改造前等价）\n\n完全等于改造前的 11 行 — 数据一字不差。')

    // The indented `### ` is recognized as an H3 (not indented code). H3+ keeps
    // its `###` prefix (aligned with pi), but the source-line indent is dropped.
    expect(result).toContain('### 改造后（ctrl+o 展开，和改造前等价）')
    expect(result).not.toContain('  ### 改造后')
  })

  test('renders bold text', () => {
    const result = render('this is **bold** text')
    expect(result).toContain('bold')
  })

  test('renders strong emphasis when closing marker is glued to following CJK text', () => {
    const md = '换句话说：**KV cache 的持久化是推理引擎的特权，因为 KV cache 本来就是推理引擎自己家的东西。**调 API 的用户没这个能力，本地跑的引擎作者想怎么存就怎么存。'
    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('KV cache 的持久化是推理引擎的特权')
    expect(result).toContain('调 API')
    expect(result).toContain('的用户没这个能力')
    expect(result).not.toContain('**KV cache')
    expect(result).not.toContain('东西。**调')
  })

  test('does not expose emphasis separator comments', () => {
    const result = render('说明：**重要。**下一句').replace(/\u200b/g, '')
    expect(result).toContain('说明：')
    expect(result).toContain('重要。')
    expect(result).toContain('下一句')
    expect(result).not.toContain('<!-- -->')
    expect(result).not.toContain('**重要')
  })

  test('does not rewrite emphasis-like text inside code fences', () => {
    const result = render('```text\nliteral **重要。**下一句\n```').replace(/\u200b/g, '')

    expect(result).toContain('literal **重要。**下一句')
    expect(result).not.toContain('<!-- -->')
  })

  test('renders italic text', () => {
    const result = render('this is *italic* text')
    expect(result).toContain('italic')
  })

  test('does not split emphasis after Chinese punctuation into a glued bullet', () => {
    const result = render('这是普通文本。**这是粗体文本**。*这是斜体文本*。')
      .replace(/\u200b/g, '')

    expect(result).toContain('这是普通文本。')
    expect(result).toContain('这是粗体文本')
    expect(result).toContain('这是斜体文本')
    expect(result).not.toContain('\n- 这是斜体文本')
  })

  test('renders inline code', () => {
    const result = render('use `foo()` here')
    expect(result).toContain('foo()')
  })

  test('renders unclosed code fence as code', () => {
    const result = render('```sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('wraps implicit SQL snippets in code blocks', () => {
    const md = [
      'Minimal test table',
      '',
      'DROP TABLE IF EXISTS decimal_driver_compat;',
      '',
      'CREATE TABLE decimal_driver_compat (',
      '    id INT,',
      '    d38 DECIMAL(38, 18),',
      '    d76 DECIMAL(76, 18)',
      ');',
      '',
      'Continue explanation.',
    ].join('\n')

    const result = render(md)

    expect(result).toContain('DROP TABLE IF EXISTS decimal_driver_compat;')
    expect(result).toMatch(/^  CREATE TABLE decimal_driver_compat \($/m)
    expect(result).toMatch(/^ {6}id INT,$/m)
    expect(result).toContain('Continue explanation.')
    expect(result).toContain('```sql')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('wraps implicit Java and Python driver snippets in code blocks', () => {
    const md = [
      'Java MariaDB JDBC example',
      '',
      'try (ResultSet rs = stmt.executeQuery("SELECT d38, d76 FROM decimal_driver_compat")) {',
      '    ResultSetMetaData md = rs.getMetaData();',
      '    System.out.println(md.getPrecision(1));',
      '}',
      '',
      'Python driver example',
      '',
      'from decimal import Decimal',
      '',
      'cur.execute("SELECT d38, d76 FROM decimal_driver_compat ORDER BY id")',
      'for row in cur.fetchall():',
      '    print(type(row[0]), row[0])',
    ].join('\n')

    const result = render(md)

    expect(result).toMatch(/^  try \(ResultSet rs = stmt\.executeQuery/m)
    expect(result).toMatch(/^ {6}ResultSetMetaData md = rs\.getMetaData\(\);$/m)
    expect(result).toMatch(/^from decimal import Decimal$/m)
    expect(result).toMatch(/^ {4}print\(type\(row\[0\]\), row\[0\]\)$/m)
    // Java is detected as implicit code and fenced; the Python snippet here
    // stays plain prose (rendered at column 0), so only the Java fence appears.
    expect(result).toContain('```java')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('stops implicit Java block at blank line before prose', () => {
    const md = [
      'try (ResultSet rs = stmt.executeQuery("SELECT 1")) {',
      '    System.out.println(rs.getInt(1));',
      '}',
      '',
      'Driver compatibility summary',
      '',
      '- Java: OK',
    ].join('\n')

    const result = render(md)

    expect(result).toMatch(/^  try \(ResultSet rs = stmt\.executeQuery/m)
    expect(result).toContain('Driver compatibility summary')
    expect(result).toContain('- Java: OK')
  })

  test('does not swallow following CJK prose into implicit SQL block', () => {
    const md = [
      'SELECT d38, d76',
      'FROM decimal_driver_compat',
      'ORDER BY id;',
      '这里重点是：',
      '',
      '- d38：普通 DECIMAL(38,18)',
    ].join('\n')

    const result = render(md)

    expect(result).toMatch(/^  SELECT d38, d76$/m)
    expect(result).toContain('这里重点是：')
    expect(result).toContain('- d38：普通 DECIMAL(38,18)')
  })

  test('renders markdown tables from decimal compatibility log as actual tables', () => {
    const result = render([
      '## Verification matrix',
      '',
      '| Client | Protocol | Focus | Risk |',
      '|---|---|---|---|',
      '| Java mariadb-java-client | MySQL | `ResultSet.getBigDecimal()` precision/scale | low |',
      '| Spark JDBC | MySQL JDBC | **DecimalType limit 38** | **high** |',
    ].join('\n')).replace(/\u200b/g, '')

    expect(result).toContain('Verification matrix')
    expect(result).toContain('┌')
    expect(result).toContain('Java')
    expect(result).toContain('mariadb-java-client')
    expect(result).toContain('Spark JDBC')
    expect(result).not.toContain('|---|---|---|---|')
  })

  test('renders markdown table when header and separator are glued', () => {
    const result = render([
      '| Client | Protocol | Focus ||---|---|---|',
      '| Java | MySQL | `getBigDecimal()` precision |',
      '| Spark JDBC | MySQL JDBC | **DecimalType limit** |',
    ].join('\n')).replace(/\u200b/g, '')

    expect(result).toContain('┌')
    expect(result).toContain('Client')
    expect(result).toContain('Spark JDBC')
    expect(result).not.toContain('|---|---|---|')
  })

  test('preserves box-drawing conclusion tables from decimal compatibility log', () => {
    const result = render([
      'Final delivery table',
      '',
      '┌─────────────┬─────────────────────────────┬──────────┐',
      '│    Link     │         Driver              │ Result   │',
      '├─────────────┼─────────────────────────────┼──────────┤',
      '│ Java main   │ mariadb-java-client x.y.z   │ OK       │',
      '└─────────────┴─────────────────────────────┴──────────┘',
    ].join('\n'))

    expect(result).toContain('Final delivery table')
    expect(result).toContain('┌─────────────┬')
    expect(result).toContain('│ Java main   │ mariadb-java-client x.y.z   │ OK')
    // Box-drawing diagram art gets no fence (rendered as-is, not code).
    expect(result).not.toContain('```text')
  })

  test('fenced code blocks align with prose left padding', () => {
    const md = 'Before:\n```bash\nnpm install\n```\nAfter.'
    const result = render(md)
    expect(result).toMatch(/^  npm install$/m)
    // Must not leave literal box-drawing characters.
    expect(result).not.toContain('│')
  })

  test('code comments render as pure code lines', () => {
    const md = [
      '```bash',
      '# Run checks in parallel where possible',
      'cargo fmt --check &',
      '```',
    ].join('\n')
    const result = render(md)
    expect(result).toMatch(/^  # Run checks in parallel where possible$/m)
    expect(result).toMatch(/^  cargo fmt --check &$/m)
    expect(result).not.toContain('│')
  })

  test('aligns trailing SQL comments in fenced code blocks', () => {
    const restore = withColumns(100)
    try {
      const md = [
        '```sql',
        'CREATE TABLE tracelake.events (',
        '  id   STRING NOT NULL STATS_TRUNCATE_LEN 24,   -- 16 hex+ 裕量',
        '  trace_id   STRING NOT NULL STATS_TRUNCATE_LEN 40,    -- 32 hex + 裕量',
        '  parent_id  STRING DEFAULT \'\'STATS_TRUNCATE_LEN 24,',
        '  session_id STRING DEFAULT \'\'STATS_TRUNCATE_LEN 32,   -- session-xxxxxxxxxxxx',
        '  response_id     STRING DEFAULT \'\' STATS_TRUNCATE_LEN 48, -- "resp_" + 16 hex',
        ') CLUSTER BY (start_time, trace_id);',
        '```',
      ].join('\n')
      const result = render(md)
      const commentColumns = result
        .split('\n')
        .filter(line => line.includes('--'))
        .map(line => line.indexOf('--'))

      expect(new Set(commentColumns).size).toBe(1)
    } finally {
      restore()
    }
  })

  test('aligns standalone comments with nearby Python code indentation', () => {
    const restore = withColumns(100)
    try {
      const md = [
        '```python',
        'def generate_trace(profile: str, target_spans: int) -> Iterator[...]:',
        '      trace_id = rand_hex(32)                 # 32 hex → 128-bit',
        '      root_id = rand_hex(16)                  # 16 hex → 64-bit',
        '      # span:',
        '      "id":rand_hex(16),                      # 16 hex → 64-bit',
        '     # session_id: 自定义，非 OTel',
        '      session_id = f"session-{rand_hex(12)}"  # "session-xxxxxxxxxxxx" = 20 字符',
        '```',
      ].join('\n')
      const result = render(md)

      expect(result).toMatch(/^ {8}# span:$/m)
      expect(result).toMatch(/^ {8}# session_id: 自定义，非 OTel$/m)
    } finally {
      restore()
    }
  })

  test('JSON fenced code blocks are highlighted', () => {
    const prevLevel = chalk.level
    chalk.level = 3
    try {
      const result = renderMarkdown('```json\n{\n  "name": "evot",\n  "enabled": true\n}\n```')
      expect(result).toContain('\x1b[')
      expect(result).toContain('"name"')
      expect(result).toContain('true')
    } finally {
      chalk.level = prevLevel
    }
  })

  test('unlabelled fenced code blocks use plaintext instead of language guessing', () => {
    // Reference renderer passes `plaintext` when no fence info string is present.
    // Auto-detection would colour random words and make unlabelled snippets
    // inconsistent with the reference renderer. The ```` fence borders may carry
    // their own dim styling, so assert specifically that the code CONTENT line
    // is uncoloured rather than the whole block.
    const result = renderMarkdown('```\nconst value = 1\n```')
    expect(result).toContain('const value = 1')
    const contentLine = stripAnsi(result)
      .split('\n')
      .find(l => l.includes('const value = 1'))
    expect(contentLine).toBeDefined()
    // The content line, as emitted, contains no ANSI escape (no highlighting).
    const rawContentLine = result.split('\n').find(l => l.includes('const value = 1'))
    expect(rawContentLine).not.toContain('\x1b[')
  })

  test('highlights common alias tags (proto/jsonc/mdx/env/…)', async () => {
    // highlight.js doesn't register these names directly, but models routinely
    // write them in fences. The renderer maps them onto the closest supported
    // grammar so the block is still coloured. We verify the mapping by
    // asserting the target language is one cli-highlight recognises, which is
    // environment-independent (doesn't depend on FORCE_COLOR / TTY detection).
    const cliHighlight = await import('cli-highlight')
    const aliases: Array<[string, string]> = [
      ['proto', 'protobuf'],
      ['jsonc', 'json'],
      ['json5', 'json'],
      ['ndjson', 'json'],
      ['mdx', 'markdown'],
      ['env', 'ini'],
      ['dotenv', 'ini'],
      ['fish', 'bash'],
      ['vue', 'html'],
      ['svelte', 'html'],
      ['log', 'accesslog'],
    ]
    for (const [_alias, target] of aliases) {
      expect(cliHighlight.supportsLanguage(target)).toBe(true)
    }
    // Sanity: content is preserved through the render path even without
    // knowing whether colour is enabled.
    for (const [alias] of aliases) {
      const rendered = render('```' + alias + '\nSAMPLE\n```')
      expect(rendered).toContain('SAMPLE')
    }
  })
  test('splits opening fence glued to preceding prose', () => {
    // `Intro```tsx` — the model forgot the newline
    // before the fence marker. Must be split into prose + fenced block so
    // the snippet renders as a code block instead of leaking literal
    // backticks into the paragraph.
    const md = 'Intro```tsx\nconst x: number = 1;\n```\nAfter'
    const result = render(md)
    expect(result).toContain('Intro')
    expect(result).toContain('const x: number = 1;')
    expect(fencesWellFormed(result)).toBe(true)
    // Prose line must not carry the fence marker or code content.
    expect(result).toMatch(/^Intro$/m)
    expect(result).toMatch(/^  const x: number = 1;$/m)
  })

  test('splits hr marker glued to bold emphasis on next section', () => {
    // `---**SQL notes**` — the HR separator and the bold section title
    // share a line with no blank around them. Must be split so the HR renders
    // as a separator and the bold text survives on its own line.
    const md = 'Previous sentence.\n---**SQL notes**\n```sql\nSELECT 1\n```'
    const result = render(md)
    expect(result).toContain('Previous sentence')
    expect(result).toContain('SQL notes')
    expect(result).toContain('SELECT 1')
    // Bold markers must be stripped by the renderer.
    expect(result).not.toContain('**SQL')
  })

  test('preserves leading indentation of tree/box-drawing paragraphs', () => {
    // A paragraph whose first line starts with spaces (common for tree
    // diagrams under an unindented root) must keep that indent verbatim —
    // otherwise the first row shifts left and the branches below no longer
    // line up with their parent. Regression for `formatTokens` using
    // `trim()` and eating the leading spaces of the first block.
    const md = [
      '  │  │     ├─ lib.rs',
      '  │  │     ├─ retry.rs',
      '  │  │     └─ tools/',
    ].join('\n')
    const result = render(md)
    expect(result).toMatch(/^ {4}│  │     ├─ lib\.rs$/m)
    expect(result).toMatch(/^ {4}│  │     ├─ retry\.rs$/m)
    expect(result).toMatch(/^ {4}│  │     └─ tools\/$/m)
  })

  test('aligns tree trailing comments to a shared start column', () => {
    // A directory tree with trailing `# …` comments whose `#` columns already
    // line up in the source. The box-drawing preservation pass aligns the
    // comment START columns (two spaces past the widest prefix), so every `#`
    // lands in the same column. Regression for aligning the END column
    // instead, which scattered the `#` markers across different columns and
    // destroyed alignment the author already got right.
    const md = [
      '```',
      'src/',
      '├── host.ts        # host loader',
      '├── api.ts         # public api surface',
      '├── context.ts     # ui primitives',
      '```',
    ].join('\n')

    const rendered = render(md).replace(/\u200b/g, '')
    const hashColumns = rendered
      .split('\n')
      .filter(line => /[├└]/.test(line) && line.includes('#'))
      .map(line => stringWidth(line.slice(0, line.indexOf('#'))))

    expect(hashColumns.length).toBe(3)
    // Every `#` must land in the same column.
    expect(new Set(hashColumns).size).toBe(1)
    // Comments and their prefixes are preserved verbatim (only spacing changes).
    expect(rendered).toContain('# host loader')
    expect(rendered).toContain('# public api surface')
    expect(rendered).toContain('# ui primitives')
  })

  test('styles plain ascii diagrams without changing their visible text', () => {
    const prevLevel = chalk.level
    chalk.level = 3
    const diagram = [
      'prompt ──► Prefill ──► K/V 写入 cache',
      '           │',
      '           ▼',
      '      Decode --> token',
    ].join('\n')

    try {
      const rendered = renderMarkdown(`\`\`\`text\n${diagram}\n\`\`\``)
      const plain = stripAnsi(rendered).replace(/\u200b/g, '')

      for (const line of diagram.split('\n')) {
        expect(plain).toContain(line)
      }
      expect(rendered).not.toBe(plain)
      expect(plain).not.toContain('```')
    } finally {
      chalk.level = prevLevel
    }
  })

  test('aligns drifted diagram box borders and branch markers conservatively', () => {
    const md = [
      '```',
      '物理显存:   ┌────┬────┬────┐',
      '   │ B3 │ B0 │ B5 │',
      '            └────┴────┴────┘',
      '',
      '2. Prefill',
      '   ├─► 把 prompt 送入模型├─► 逐层计算',
      '3.Decode 循环',
      ' ├─► 取上一步生成的 token',
      '    └─► 采样下一个 token',
      '```',
    ].join('\n')

    const rendered = render(md).replace(/\u200b/g, '')

    expect(rendered).toMatch(/^  物理显存:   ┌────┬────┬────┐$/m)
    expect(rendered).toMatch(/^              │ B3 │ B0 │ B5 │$/m)
    expect(rendered).toMatch(/^              └────┴────┴────┘$/m)
    expect(rendered).toMatch(/^      ├─► 把 prompt 送入模型├─► 逐层计算$/m)
    expect(rendered).toMatch(/^      ├─► 取上一步生成的 token$/m)
    expect(rendered).toMatch(/^      └─► 采样下一个 token$/m)
  })

  test('preserves flow diagrams with box-drawing characters as plain code', () => {
    // Flow diagrams are not necessarily directory trees, but they still rely
    // on exact spaces around `│` / `├─` columns. The renderer should keep the
    // block verbatim and must not add the old code-block gutter.
    const md = [
      '  Agent → Tool.call(args)                │',
      '     ▼',
      '   Session::goto(url)',
      '            │',
      '     ├─ preflight: daemon.health().await',
      '    │    ├─ Stopped→ ToolError("daemon not running")  │ ├─ NoExtension',
      '    │    └─ Ready     → ok',
      '            │',
      '          ▼',
      '      daemon.send("navigate", ...)',
    ].join('\n')
    const result = render(md)
    expect(result).toMatch(/^ {4}Agent → Tool\.call\(args\) {16}│$/m)
    expect(result).toMatch(/^ {6}│ {4}├─ Stopped→ ToolError\("daemon not running"\) {2}│ ├─ NoExtension$/m)
    expect(result).toMatch(/^ {8}daemon\.send\("navigate", \.\.\.\)$/m)
    expect(result).not.toMatch(/^ │ /m)
  })

  test('does not normalize hr markers inside preserved box-drawing diagrams', () => {
    // The box-drawing preservation pass wraps this paragraph in a synthetic
    // code fence. Later markdown repair passes must respect that fence and
    // keep `---` as diagram content, not reinterpret it as an HR separator.
    const md = [
      '  root │',
      '  ---',
      '  └─ child',
    ].join('\n')
    const result = render(md)
    expect(result).toMatch(/^ {4}root │$/m)
    expect(result).toMatch(/^ {4}---$/m)
    expect(result).toMatch(/^ {4}└─ child$/m)
  })

  test('does not normalize hr markers inside authored code fences', () => {
    const md = [
      '```text',
      'alpha',
      '---',
      'omega',
      '```',
    ].join('\n')
    const result = render(md)
    expect(result).toMatch(/^  alpha$/m)
    expect(result).toMatch(/^  ---$/m)
    expect(result).toMatch(/^  omega$/m)
  })

  test('splits ordered list item glued after CJK sentence terminator', () => {
    // `一。2. 二` — the second list item is glued to the prose of the
    // first. Must break onto its own line so both `1.` and `2.` render as
    // ordered-list items.
    const md = [
      'List:',
      '',
      '1. 一。2. 二。',
    ].join('\n')
    const result = render(md)
    expect(result).toContain('1.')
    expect(result).toContain('2.')
    // `。2.` must not remain glued on the same visual line — the split
    // should put the second item on a fresh line (leading `2.` at col 0).
    expect(result).not.toMatch(/一。[ \t]*2\./)
    expect(result).toMatch(/^2\. 二/m)
  })

  test('renders unclosed tilde fence as code', () => {
    const result = render('~~~sql\nSELECT 1')
    expect(result).toContain('SELECT 1')
    expect(result).not.toContain('~~~')
  })

  test('repairs unclosed code fence before later prose', () => {
    const md = '```json\n[\n  {"id":"evt-001"}\n]\n\ntext after.'
    const result = render(md)
    expect(result).toContain('{"id":"evt-001"}')
    expect(result).toContain('text after')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('keeps consecutive SQL statements inside one fence', () => {
    // Two `ALTER ... ;` statements: the trailing semicolon on the first must
    // not trip the "code completed" heuristic into closing the fence early and
    // wrapping the second statement in its own block. ALTER/MERGE/TRUNCATE are
    // recognised as code keywords so they are never mistaken for prose.
    const result = render('```sql\nALTER TASK t_etl RESUME;\nALTER TASK t_ingest RESUME;\n```后续说明。')
      .replace(/\u200b/g, '')

    expect(result).toContain('ALTER TASK t_etl RESUME;')
    expect(result).toContain('ALTER TASK t_ingest RESUME;')
    expect(result).toContain('后续说明。')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits fence close glued to following heading', () => {
    const result = render('```json\n{\n  "id": "tr-abc"\n}\n```### 5.1 API')
      .replace(/\u200b/g, '')

    expect(result).toContain('"id": "tr-abc"')
    expect(result).toContain('5.1 API')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits fence close glued to heading without a space', () => {
    // Models often omit the space in CJK contexts: `\`\`\`##题`.
    // We should normalise it so marked sees `## 题`.
    const result = render('```json\n{"x":1}\n```##题（8 项）')
      .replace(/\u200b/g, '')

    expect(result).toContain('"x":1')
    expect(result).toContain('题（8 项）')
    expect(result).not.toContain('##题')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('promotes ATX heading glued after a preceding paragraph', () => {
    // `。 ##题` — split the heading onto its own line.
    const result = render('句。 ##题').replace(/\u200b/g, '')

    expect(result).toContain('句。')
    expect(result).toContain('题')
    // Should not keep `。 ##` on a single line.
    expect(result).not.toMatch(/句。\s*##/)
  })

  test('promotes ATX heading glued with zero space after CJK punctuation', () => {
    // Models often drop the space entirely: `。###题`.
    const result = render('句。###题').replace(/\u200b/g, '')

    expect(result).toContain('句。')
    expect(result).toContain('题')
    expect(result).not.toMatch(/。###/)
  })

  test('promotes ATX heading glued with zero space after CJK character', () => {
    // No punctuation at all — `文###题` still needs a line break.
    const result = render('文###题').replace(/\u200b/g, '')

    expect(result).toContain('文')
    expect(result).toContain('题')
    expect(result).not.toMatch(/文###/)
  })

  test('promotes ATX heading glued before a body that has its own space', () => {
    // `句。### Heading` — the heading body is well-formed (`###`
    // followed by a space), only the preceding prose is glued to the marker.
    const result = render('句。### Heading\n\nbody').replace(/\u200b/g, '')

    expect(result).toContain('句。')
    expect(result).toContain('Heading')
    expect(result).toContain('body')
    expect(result).not.toMatch(/。###/)
  })

  test('promotes ATX heading glued after ascii sentence punctuation', () => {
    const result = render('done.## Next section\n\nbody').replace(/\u200b/g, '')

    expect(result).toContain('done.')
    expect(result).toContain('Next section')
    expect(result).not.toMatch(/done\.##/)
  })

  test('normalises bullet marker glued to CJK body', () => {
    // `-项` — CommonMark needs `- body`.
    const result = render('-项\n- 二').replace(/\u200b/g, '')
    // Should render as a list: bullet + body, not a dash-prefixed paragraph.
    expect(result).toContain('- 项')
    expect(result).toContain('- 二')
  })

  test('does not normalise `-`/`*` that are not bullet intents', () => {
    // Guard rails — make sure we don't mangle negatives, CLI flags, HR, or
    // emphasis markers.
    expect(render('-1.5 is negative').replace(/\u200b/g, '')).toContain('-1.5 is negative')
    expect(render('--flag is an option').replace(/\u200b/g, '')).toContain('--flag is an option')
    // A bare `---` is an hr and now renders as a full-width ─ rule.
    expect(render('---').replace(/\u200b/g, '')).toMatch(/─+/)
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

  test('normalises ascii bullet marker glued before CJK prose', () => {
    const result = render('-prompt 不是扩展：丢弃 checkpoint').replace(/\u200b/g, '')
    expect(result).toContain('- prompt 不是扩展')
    expect(result).not.toContain('-prompt')
  })

  test('normalises ascii bullet marker glued before CJK colon detail', () => {
    const result = render('模型结构带来的 KV 分层（ds4_layer_compress_ratio）：-Layer0-1：dense，ratio=0，只有 raw cache').replace(/\u200b/g, '')
    expect(result).toContain('模型结构带来的 KV 分层')
    expect(result).toContain('- Layer0-1：dense')
    expect(result).not.toContain('：-Layer0')
  })

  test('splits bullet glued after sentence punctuation', () => {
    const result = render('- **跳过前 24050 的 prefill**，只 prefill 新增的 350 token（~1.5 秒）- Decode 继续').replace(/\u200b/g, '')
    expect(result).toContain('- 跳过前 24050')
    expect(result).toContain('- Decode 继续')
    expect(result).not.toContain('秒）- Decode')
  })

  test('does not normalise ascii bullet-like command options', () => {
    expect(render('-p value').replace(/\u200b/g, '')).toContain('-p value')
    expect(render('-1 remains negative').replace(/\u200b/g, '')).toContain('-1 remains negative')
    expect(render('config:-foo:bar').replace(/\u200b/g, '')).toContain('config:-foo:bar')
    expect(render('This is fine - not a bullet').replace(/\u200b/g, '')).toContain('This is fine - not a bullet')
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
    const result = render('```ts\nconst a = 1\n```- item')

    expect(result).toContain('const a = 1')
    expect(result).toContain('- item')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits fence close glued to following inline bold prose', () => {
    const result = render('```rust\nErr(ToolError::Failed(format!("binary")))\n```**safe**: metadata only.')

    expect(result).toContain('ToolError::Failed')
    expect(result).toContain('safe')
    expect(result).toContain('metadata only')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits fence close glued to following prose', () => {
    const result = render('```text\nsrc/engine/\nsrc/engine/Cargo.toml\n```**safe**: more info.')

    expect(result).toContain('src/engine/Cargo.toml')
    expect(result).toContain('more info')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits trailing fence close glued to last code line', () => {
    // Models sometimes omit the newline before the closing fence, e.g.
    // `    }\`\`\``. The renderer used to leak literal backticks; we now
    // split the marker onto its own line so marked sees a clean code block.
    const md = 'Output:\n```json\n   {\n  "diagnoses": ["a"],\n  "hypotheses": ["b"]\n    }```'
    const result = render(md)
    expect(result).toContain('"diagnoses"')
    expect(result).toContain('"hypotheses"')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits trailing fence close glued to last code line and following prose', () => {
    const result = render('Output:\n```python\nprint("hello")```Continue explanation.')

    expect(result).toContain('print("hello")')
    expect(result).toContain('Continue explanation.')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits fence close glued to following plain prose', () => {
    const result = render('```python\nprint("hello")\n```Continue explanation.')

    expect(result).toContain('print("hello")')
    expect(result).toContain('Continue explanation.')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('splits single-line fenced code', () => {
    const result = render('```const value = 1```')

    expect(result).toContain('const value = 1')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('repairs unclosed fence before following markdown heading without a blank line', () => {
    const result = render('```json\n{\n  "id": "tr-abc"\n}\n## Step 8: input / output')

    expect(result).toContain('"id": "tr-abc"')
    expect(result.replace(/\u200b/g, '')).toContain('Step 8: input / output')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('repairs completed json fence before plain chinese paragraph', () => {
    const result = render('Final:\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\ntext after')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toContain('text after')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('repairs completed json fence before markdown hr without a blank line', () => {
    const result = render('Final:\n```json\n{\n  "id": "tr-abc",\n  "is_deleted": 0\n}\n---\nStep 8: input / output')
      .replace(/\u200b/g, '')

    expect(result).toContain('"is_deleted": 0')
    expect(result).toMatch(/^─+$/m)
    expect(result).toContain('Step 8: input / output')
    expect(fencesWellFormed(result)).toBe(true)
  })

  test('repairs shell fence before following CJK prose and keeps later text fence visible', () => {
    const md = [
      '```bash',
      'cd cli && bun test tests/term-commands.test.ts tests/repl-control.test.ts tests/outputLines.test.ts tests/viewmodel-output.test.ts tests/term-stream.test.ts',
      '',
      '重点：   ⏺ 当前目录有 12 个文件...',
      '',
      '所有 verbose 块塌成一行 dim gutter，工具调用主导视觉。',
      '',
      '**Verbose 模式（ctrl+o 切换）**',
      '',
      '```text',
      '❯ 帮我列一下目录',
      '',
      '  ⋮ llm · claude-sonnet-4 · turn 1 · 3 msgs',
      '  ⋮ ctx  ████░░░░░░░░░░░░░░░░  ~12k / 200k · 6%',
      '  ⋮ tok  sys 8k · user 2k · tool 2k',
    ].join('\n')

    const result = render(md)

    expect(result).toMatch(/^  cd cli && bun test tests\/term-commands\.test\.ts/m)
    expect(result).toContain('重点：')
    expect(result).toContain('Verbose 模式（ctrl+o 切换）')
    expect(result).toMatch(/^  ❯ 帮我列一下目录$/m)
    expect(result).toMatch(/^ {4}⋮ llm · claude-sonnet-4 · turn 1 · 3 msgs$/m)
    expect(result).not.toContain('```text')
  })

  test('repairs stray fence close before following markdown blocks', () => {
    const md = [
      '```',
      '循环到 EOS 或达到长度上限。',
      '',
      '## 关键工程点',
      '',
      '**内存分配策略**- 静态分配：一次开 `max_seq_len`，简单但浪费。',
      '- PagedAttention（vLLM）：把 cache 切成固定 block。',
      '',
      '**batch 管理**',
      '- Continuous batching：请求长度不一。',
    ].join('\n')

    const result = render(md).replace(/\u200b/g, '')

    expect(result).toContain('循环到 EOS 或达到长度上限。')
    expect(result).toContain('关键工程点')
    expect(result).toContain('内存分配策略')
    expect(result).toContain('- 静态分配：一次开 max_seq_len，简单但浪费。')
    expect(result).toContain('batch 管理')
    expect(result).not.toContain('```')
    expect(result).not.toContain('## 关键工程点')
    expect(result).not.toContain('**内存分配策略**-')
  })

  test('repairs unclosed text diagram fence before following bold paragraph', () => {
    const md = [
      '一张图总结',
      '',
      '```text',
      '      冷启动 (昨天)       命中磁盘 (今天)',
      '    ─────────────────                ──────────────────',
      '  prompt: A B C            prompt: A B C D E F',
      '     │                                 │',
      '     ├─ prefill 3 次前向   ├─ 扫磁盘，SHA1 匹配',
      '',
      '**模型权重从头到尾是同一份只读数据。变的只是那些 K/V缓冲区里装的数值。**',
    ].join('\n')

    const result = render(md).replace(/\u200b/g, '')

    expect(result).toContain('冷启动 (昨天)')
    expect(result).toContain('prompt: A B C')
    expect(result).toContain('模型权重从头到尾是同一份只读数据')
    expect(result).not.toContain('```text')
    expect(result).not.toContain('**模型权重')
  })

  test('does not close plain text fence without diagram content before bold literal', () => {
    const md = [
      '```text',
      'literal plain text',
      '',
      '**not markdown**',
    ].join('\n')

    const result = render(md).replace(/\u200b/g, '')

    expect(result).toContain('literal plain text')
    expect(result).toContain('**not markdown**')
  })

  test('keeps adjacent prose compact', () => {
    const result = render('a\nb\nc')

    expect(result).toBe('a\nb\nc')
  })

  test('keeps list items compact', () => {
    const result = render('- one\n- two\n- three')

    expect(result).toBe('- one\n- two\n- three')
  })

  test('wraps very long plain lines', () => {
    const restore = withColumns(40)
    try {
      const result = render('INSERT ' + 'x'.repeat(80))
      expect(result.split('\n').length).toBeGreaterThan(1)
    } finally {
      restore()
    }
  })

  test('soft-wraps an over-wide heading instead of overrunning the terminal', () => {
    // Models sometimes glue an entire paragraph onto a heading line with no
    // newline, so the lexer parses one giant h2. Without wrapping it overruns
    // the terminal width and gets visually truncated. Every wrapped line must
    // stay within the content width (columns - SAFETY_MARGIN).
    const columns = 100
    const restore = withColumns(columns)
    try {
      const heading = '## 用这个 demo 的真实任务来说demo 里有个任务（我从 data/teacher_sft.jsonl 和 data/teacher_rl.jsonl 里读出来的真实数据）很长很长的一段文字需要换行处理才行。'
      const lines = render(heading).split('\n').filter(Boolean)
      expect(lines.length).toBeGreaterThan(1)
      for (const line of lines) {
        expect(stringWidth(line)).toBeLessThanOrEqual(columns - 4)
      }
    } finally {
      restore()
    }
  })

  test('keeps wrapped paragraph continuation flush with first line', () => {
    const restore = withColumns(84)
    try {
      const result = render('Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.')
      const lines = result.split('\n').filter(Boolean)

      expect(lines.length).toBeGreaterThan(1)
      expect(lines.every(line => !line.startsWith(' '))).toBe(true)
    } finally {
      restore()
    }
  })

  test('keeps list item continuation lines hanging indented', () => {
    const restore = withColumns(42)
    try {
      const result = render('- Check `cli/src/render/markdown.ts` with a very very very very long line')
        .replace(/\u200b/g, '')
      const lines = result.split('\n')

      expect(lines.length).toBeGreaterThan(1)
      expect(lines[0]).toStartWith('- ')
      expect(lines.slice(1).every(line => line.startsWith('  '))).toBe(true)
      expect(lines.slice(1).every(line => !line.startsWith('- '))).toBe(true)
    } finally {
      restore()
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

  test('link fallback without OSC 8 shows only the URL', () => {
    // Mirror claudecode: when hyperlinks aren't supported, createHyperlink
    // returns the bare URL. The display text is dropped rather than
    // surfaced as `text (url)` because (a) parentheses are noisy in
    // paragraph-style prose and (b) search/copy still works with the URL
    // alone.
    const prev = process.env.FORCE_HYPERLINK
    process.env.FORCE_HYPERLINK = '0'
    try {
      const result = render('[click](https://example.com)')
      expect(result).toContain('click')
      expect(result).toContain('https://example.com')
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

  test('h1 heading is gold, bold, italic, underlined', () => {
    // evot accent gold + emphasis (matches banner headers + pi's mdHeading).
    // Pin the dark theme + colour level so the assertion is deterministic:
    // getTheme() otherwise caches whichever theme the ambient env resolved
    // (dark locally, light under CI), which flips the accent hue.
    const prevLevel = chalk.level
    const prevTheme = process.env.EVOT_THEME
    chalk.level = 3
    process.env.EVOT_THEME = 'dark'
    resetThemeCache()
    try {
      const theme = getTheme()
      expect(theme.h1.paint('Title')).toBe(chalk.hex('#f0c674').bold.italic.underline('Title'))
      const raw = renderMarkdown('# Title')
      expect(raw).toContain(theme.h1.paint('Title'))
    } finally {
      chalk.level = prevLevel
      if (prevTheme === undefined) delete process.env.EVOT_THEME
      else process.env.EVOT_THEME = prevTheme
      resetThemeCache()
    }
  })

  test('h2 heading is gold and bold', () => {
    // h2+ carry the accent so every level reads as a distinct section marker.
    // See h1 test: pin dark theme + colour level for a deterministic hue.
    const prevLevel = chalk.level
    const prevTheme = process.env.EVOT_THEME
    chalk.level = 3
    process.env.EVOT_THEME = 'dark'
    resetThemeCache()
    try {
      const theme = getTheme()
      expect(theme.h2.paint('Subtitle')).toBe(chalk.hex('#f0c674').bold('Subtitle'))
      const raw = renderMarkdown('## Subtitle')
      expect(raw).toContain(theme.h2.paint('Subtitle'))
    } finally {
      chalk.level = prevLevel
      if (prevTheme === undefined) delete process.env.EVOT_THEME
      else process.env.EVOT_THEME = prevTheme
      resetThemeCache()
    }
  })

  test('list markers carry the teal accent, checkbox stays uncoloured', () => {
    // pi tints list markers with its accent; evot mirrors this with the teal
    // secondary accent so list structure reads at a glance. The [ ]/[x] task
    // glyph is left uncoloured so todo state isn't lost in the accent hue.
    const prevLevel = chalk.level
    chalk.level = 3
    try {
      const bulletMarker = getTheme().bullet.paint('-')
      const orderedMarker = getTheme().listNumber.paint('1.')
      const unordered = renderMarkdown('- item one')
      expect(unordered).toContain(bulletMarker)
      expect(stripAnsi(unordered)).toContain('- item one')

      const ordered = renderMarkdown('1. first')
      expect(ordered).toContain(orderedMarker)
      expect(stripAnsi(ordered)).toContain('1. first')

      // The checkbox glyph is emitted verbatim, not wrapped in the accent hue.
      const task = renderMarkdown('- [x] done')
      expect(task).toContain(bulletMarker)
      expect(task).toContain('[x]')
      expect(stripAnsi(task)).toContain('- [x] done')
    } finally {
      chalk.level = prevLevel
    }
  })

  test('renders horizontal rules', () => {
    // Full-width box-drawing rule (aligned with pi), not a literal `---`.
    const result = render('---')
    expect(result).toMatch(/^─+$/m)
    expect(result).not.toContain('---')
  })

  test('splits hr glued to end of sentence without whitespace', () => {
    // Models sometimes emit `通用框架。---\n核心抽象` (no space/newline before
    // the --- marker). Treat it as a thematic break, not literal dashes.
    const result = render('通用框架。---\n核心抽象三个独立').replace(/\u200b/g, '')
    expect(result).toContain('通用框架。')
    expect(result).toContain('核心抽象三个独立')
    expect(result).not.toMatch(/通用框架。---/)
  })

  test('splits hr glued to heading with zero space and CJK body', () => {
    // `---###第 4 / 5 步` — models omit every whitespace between the HR,
    // the `###`, and the heading body. `---` + heading are ASCII; the CJK
    // body is kept because real-world failures came from CJK content.
    const result = render('---###第 4 / 5 步').replace(/\u200b/g, '')
    // hr renders as a full-width ─ rule (aligned with pi); heading keeps `###`.
    expect(result).toMatch(/^─+$/m)
    expect(result).toContain('### 第 4 / 5 步')
    expect(result).not.toContain('###第')
    expect(result).not.toContain('---')
  })

  test('splits hr glued to heading after preceding prose', () => {
    const result = render('prose.\n---###step 4').replace(/\u200b/g, '')
    expect(result).toContain('prose.')
    expect(result).toContain('step 4')
    expect(result).not.toContain('---###')
  })

  test('splits hr glued before heading', () => {
    const result = render('---### 方案分层：从零代码到完整 Eval').replace(/\u200b/g, '')

    // The glued `---###` is split into an hr (full-width ─ rule, aligned with
    // pi) and an H3. H3+ keeps its `###` prefix, so the heading shows the marker.
    expect(result).toMatch(/^─+\n\n### 方案分层：从零代码到完整 Eval/m)
    expect(result).not.toContain('---')
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
      '│ Service          │',
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

  test('aligns trailing comments in tree-style directory listings', () => {
    const restore = withColumns(120)
    try {
      const tree = [
        'Directory',
        '',
        '  cli/',
        '  ├── src/',
        '  │   ├── render/',
        '  │   │   ├── verbose.ts      # [modify] 4 formatter 改为 { text, expandedText }',
        '  │   │   └── output.ts                # [modify] buildRunSummary 紧凑化',
        '  │   ├── term/',
        '  │   │   └── app/',
        '  │   │       ├── reducer.ts           # [modify] llm_started/compact_started 存 expandedText',
        '  │   │       └── stream.ts            # [modify] 合并 started/completed 分支，统一 dual-commit',
        '  │   └── session/',
        '  │       └── transcript.ts       #[modify] 存储新字段，resume 兼容老会话',
        '  └── tests/',
        '  ├── outputLines.test.ts          # [modify] 断言 text 单行 + expandedText 详情',
        '   └── term-stream.test.ts          # [modify] 4 种事件都产出 expandedCommitLines',
      ].join('\n')
      const rendered = stripAnsi(renderMarkdown(tree)).replace(/\u200b/g, '')
      // Comment START columns are aligned (two spaces past the widest prefix),
      // so every `#` lands in the same column regardless of comment length.
      const commentStarts = rendered
        .split('\n')
        .filter(line => line.includes('modify]'))
        .map(line => stringWidth(line.slice(0, line.indexOf('#'))))

      expect(new Set(commentStarts).size).toBe(1)
      expect(rendered).toContain('  │       └── transcript.ts')
      expect(rendered).toContain('      ├── outputLines.test.ts')
      expect(rendered).toContain('      └── term-stream.test.ts')
    } finally {
      restore()
    }
  })

  test('aligns trailing comment starts in tree-style directory listings', () => {
    const restore = withColumns(120)
    try {
      const tree = [
        '⏺ /Users/bohu/github/evotai/evot',
        '  ├── Cargo.toml     # Rust workspace root (engine/app/addon)',
        '  ├── Cargo.lock',
        '  ├── Makefile          # make check/build/test 入口',
        '  ├── README.md',
        '  ├── CLAUDE.md     # 项目级 AI 指令',
        '  ├── FEATURE_COMPARISON.md',
        '  ├── install.sh                    # 安装脚本',
        '  ├── rust-toolchain.toml',
        '  ├── rustfmt.toml',
        '  │',
        '  ├── .github/workflows/  # CI 与发布流水线',
        '  │   ├── ci.yml',
        '  │   └── release.yml',
        '  │',
        '  ├── src/      # Rust 核心代码',
        '  │   ├── engine/              # evotengine — agent 运行时',
        '  │   │   ├── Cargo.toml',
        '  │   │   └── src/',
        '  │   │       ├── lib.rs   # 仅模块声明与 re-export',
        '  │   │       ├── retry.rs       # 通用重试逻辑',
        '  │   │   ├── agent/  # Agent 主体',
        '  │   │ │   ├── agent.rs      #  Agent 结构与生命周期',
        '  │   │       │   ├── handle.rs   #   外部控制句柄',
        '  │   │       │   └── run.rs #   单次 run 驱动',
        '  │   │       ├── context/          # 上下文管理',
        '  │   │       │   ├── tokens.rs   #   token 计数',
      ].join('\n')
      const rendered = stripAnsi(renderMarkdown(tree)).replace(/\u200b/g, '')
      const commentStarts = rendered
        .split('\n')
        .filter(line => line.includes('#'))
        .map(line => stringWidth(line.slice(0, line.indexOf('#'))))

      // Every `#` marker lands in the same column.
      expect(new Set(commentStarts).size).toBe(1)
    } finally {
      restore()
    }
  })

  test('does not treat markdown tables containing box-drawing text as streaming tree tails', () => {
    const table = [
      '| Name | Shape |',
      '| --- | --- |',
      '| flow | │ box │ |',
      '',
      'next',
    ].join('\n')

    expect(stripAnsi(renderMarkdown(table))).toContain('Shape')
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

  test('renders table when heading is glued to table header', () => {
    const md = [
      '## 架构层面的区别| 方面 | Pi | Evot |',
      '|------|-----|------|',
      '| 组件模型 | Component 树 | buildFrame() |',
    ].join('\n')

    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('┌')
    expect(result).toContain('方面')
    expect(result).toContain('组件模型')
    expect(result).toContain('架构层面的区别')
    expect(result).not.toContain('|------|')
  })

  test('repairs table rows glued to separators and following prose', () => {
    const md = [
      'Storage cost estimate',
      '',
      '| Storage | Unit price | Monthly cost |',
      '|---|---|---|| OSS Standard | 0.148 CNY/GB/month | ~30 CNY |',
      '| OSS Infrequent Access | 0.08 CNY/GB/month | ~16 CNY |',
      '| OSS Archive | 0.033 CNY/GB/month | ~7 CNY |Archive is cheapest for write-once, rarely-read compliance retention.',
    ].join('\n')

    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('┌')
    expect(result).toContain('└')
    expect(result).toContain('OSS Standard')
    expect(result).toContain('0.148')
    expect(result).toContain('CNY/GB/month')
    expect(result).toContain('~7')
    expect(result).toContain('CNY')
    expect(result).toContain('Archive is cheapest for write-once, rarely-read compliance retention.')
    expect(result).not.toContain('|---|---|---|')
    expect(result).not.toContain('|Archive is')
  })

  test('renders <br> inside table cells as a line break', () => {
    // GFM tables don't support literal newlines in a cell, so models use
    // `<br>` to force bullet-style line breaks. Previously our renderer
    // dropped all html tokens, which glued the fragments together into one
    // long blob and word-wrapped anywhere.
    const restore = withColumns(120)
    try {
      const md = [
        '| 维度 | Rust |',
        '|------|------|',
        '| 类型系统 | 静态强类型<br>编译期检查<br>所有权 + 借用 |',
      ].join('\n')
      const result = render(md).replace(/\u200b/g, '')
      const lines = result.split('\n')
      // Each fragment should appear on its own line inside the cell.
      expect(lines.some(l => /│\s*静态强类型\s+│/.test(l))).toBe(true)
      expect(lines.some(l => /│\s*编译期检查\s+│/.test(l))).toBe(true)
      expect(lines.some(l => /│\s*所有权 \+ 借用\s+│/.test(l))).toBe(true)
      // The column width should be based on the longest visual <br> line,
      // not the combined width of every line joined by newlines.
      const borderLine = lines.find(l => l.startsWith('┌'))
      expect(borderLine).toBeDefined()
      expect(borderLine!.length).toBeLessThan(80)
      // And the literal `<br>` must not leak into the output.
      expect(result).not.toContain('<br>')
    } finally {
      restore()
    }
  })

  test('keeps CJK-heavy tables as horizontal tables even when cells wrap', () => {
    // Regression: previously any row whose cell wrapped past 4 lines flipped
    // the whole table to a `label: value` key-value fallback with `────`
    // row separators. CJK-heavy rows tripped this trigger routinely, so
    // legitimate tables silently turned into verbose lists.
    const restore = withColumns(80)
    try {
      const md = [
        '| 事件 | 触发点 | 行为 |',
        '|---|---|---|',
        '| TurnStarted | tasks/mod.rs:332 | 快照 turn_id 和当前 TokenUsage 作为基线；从 DB 读 goal，把 goal_id 绑到这一 turn 的计量快照 |',
        '| ToolCompleted | tools/registry.rs:490 每次工具调用后 | 工具名不是 update_goal 时，调用 account_thread_goal_progress（允许 budget steering 注入） |',
        '| TurnFinished | tasks/mod.rs:737 | 完成时再做一次最终计量；清理 turn 快照；取消 continuation 标记 |',
      ].join('\n')
      const result = render(md).replace(/\u200b/g, '')
      // Must render as a horizontal table with borders, not as the
      // key-value vertical fallback.
      expect(result).toContain('┌')
      expect(result).toContain('└')
      expect(result).toContain('事件')
      expect(result).toContain('触发点')
      // The vertical fallback uses a long `─` separator line between rows —
      // reject any line that is purely `─` repeated (no `┬`/`┴`/`┼`/`│`).
      const lines = result.split('\n')
      const sepOnly = lines.find(l => /^─{10,}$/.test(l))
      expect(sepOnly).toBeUndefined()
      // And must not rewrite the row as `事件: TurnStarted` / `触发点: …`.
      expect(result).not.toMatch(/^事件:\s/m)
      expect(result).not.toMatch(/^触发点:\s/m)
    } finally {
      restore()
    }
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
    // Match the reference tag stripper: drop the tags and their body.
    const result = render('hello\n<system-reminder>\ninternal only\n</system-reminder>\nworld')
    expect(result).toContain('hello')
    expect(result).toContain('world')
    expect(result).not.toContain('system-reminder')
    expect(result).not.toContain('internal only')
  })

  test('strips analysis tags', () => {
    const result = render('<commit_analysis>\nhidden\n</commit_analysis>\nvisible body')
    expect(result).toContain('visible body')
    expect(result).not.toContain('hidden')
    expect(result).not.toContain('commit_analysis')
  })

  test('keeps inline prose mentions of reminder tags and the content after them', () => {
    // Regression: a lazy global strip used to match from an in-prose
    // `<system-reminder>` mention to a later `</system-reminder>` and delete
    // everything in between — including unrelated tables and trailing prose.
    const md = [
      '核心区别：skill 菜单作为 `<system-reminder>` 消息注入对话。',
      '',
      '| 方面 | Claude Code | evot |',
      '|---|---|---|',
      '| 菜单位置 | `<system-reminder>` 消息 | 工具 description |',
      '| 截断策略 | 动态预算 | 固定 250 字 |',
      '',
      '然后把菜单从工具描述里删掉。',
    ].join('\n')
    const result = render(md).replace(/\u200b/g, '')
    expect(result).toContain('核心区别')
    expect(result).toContain('菜单位置')
    expect(result).toContain('截断策略')
    expect(result).toContain('然后把菜单从工具描述里删掉')
    // The inline tag mention is preserved (not stripped as an envelope).
    expect(result).toContain('<system-reminder>')
  })

  test('does not strip reminder tags inside fenced code blocks', () => {
    const md = [
      '示例：',
      '',
      '```',
      '<system-reminder>',
      'The following skills are available:',
      '</system-reminder>',
      '```',
      '',
      '后续说明。',
    ].join('\n')
    const result = render(md)
    expect(result).toContain('<system-reminder>')
    expect(result).toContain('The following skills are available:')
    expect(result).toContain('</system-reminder>')
    expect(result).toContain('后续说明')
  })
})

// ---------------------------------------------------------------------------
// GFM task-list checkboxes
// ---------------------------------------------------------------------------

describe('task-list checkboxes', () => {
  test('renders unchecked and checked boxes in an unordered list', () => {
    const result = stripAnsi(renderMarkdown('- [ ] todo item\n- [x] done item\n- normal item'))
    expect(result).toContain('- [ ] todo item')
    expect(result).toContain('- [x] done item')
    // Non-task items keep a plain bullet with no checkbox.
    expect(result).toContain('- normal item')
    expect(result).not.toContain('- [ ] normal')
  })

  test('renders checkboxes in an ordered list', () => {
    const result = stripAnsi(renderMarkdown('1. [ ] numbered todo\n2. [x] numbered done'))
    expect(result).toContain('1. [ ] numbered todo')
    expect(result).toContain('2. [x] numbered done')
  })

  test('renders checkboxes in nested lists', () => {
    const result = stripAnsi(renderMarkdown('- [ ] outer\n  - [x] nested done\n  - nested normal'))
    expect(result).toContain('- [ ] outer')
    expect(result).toContain('- [x] nested done')
    expect(result).toContain('- nested normal')
  })

  test('wrapped task item aligns continuation under the text', () => {
    const restore = withColumns(40)
    try {
      const md = '- [x] alpha beta gamma delta epsilon zeta eta theta iota'
      const lines = stripAnsi(renderMarkdown(md)).split('\n')
      expect(lines.length).toBeGreaterThan(1)
      expect(lines[0]).toMatch(/^- \[x\] /)
      // Continuation lines indent to the "- [x] " prefix width (6 columns).
      // The wrap primitive preserves the break space, so allow an optional extra space.
      expect(lines[1]).toMatch(/^ {6}/)
      expect(lines[1].trim().length).toBeGreaterThan(0)
    } finally {
      restore()
    }
  })
})

// ---------------------------------------------------------------------------
// Nested inline style continuity
//
// When an inline token (strong/em/codespan/link) closes, it emits its own ANSI
// close code (22/23/24/39 — never a full \x1b[0m reset). We rely on chalk to
// re-open the surrounding style so text after the nested token keeps the outer
// heading/blockquote/emphasis styling. These tests lock that behaviour so a
// theme change or a stray full-reset can't silently strip styling mid-line.
// ---------------------------------------------------------------------------

describe('nested inline style continuity', () => {
  function renderColored(md: string): string {
    const prevLevel = chalk.level
    chalk.level = 3
    try {
      return renderMarkdown(md)
    } finally {
      chalk.level = prevLevel
    }
  }

  test('inline content never emits a full reset (\\x1b[0m)', () => {
    const samples = [
      '# Title with **STRONG** word',
      '# Run `npm test` before commit',
      '## See [docs](https://x.com) now',
      '### Note *emphasis* here',
      '> quote with **bold** and more',
      '> quote with *emphasis* and more text',
      'text ***both*** tail',
    ]
    for (const md of samples) {
      const out = renderColored(md)
      expect(out).not.toContain('\x1b[0m')
    }
  })

  test('italic reopens after a nested em closes inside a blockquote', () => {
    const out = renderColored('> quote with *emphasis* and more text')
    // The nested em closes with \x1b[23m; the trailing text must be re-wrapped
    // in \x1b[3m so it stays italic.
    expect(out).toContain('\x1b[23m\x1b[3m')
    // Sanity: visible text is intact.
    expect(stripAnsi(out)).toContain('quote with emphasis and more text')
  })

  test('heading keeps bold/italic/underline open across a nested codespan', () => {
    const out = renderColored('# Run `npm test` before commit')
    // h1 now opens with the gold accent colour before the decorations, so the
    // string no longer starts with the bold escape. What still matters: the
    // decorations (bold+italic+underline) open together, and the codespan only
    // toggles the foreground colour (39 close) rather than a full reset, so
    // they stay open for the tail.
    expect(out).toContain('\x1b[1m\x1b[3m\x1b[4m')
    expect(out.startsWith('\x1b[')).toBe(true)
    expect(out).toContain('\x1b[39m')
    expect(out).not.toContain('\x1b[0m')
    expect(stripAnsi(out)).toBe('Run npm test before commit')
  })

  test('bold survives after a nested strong closes in a heading', () => {
    const out = renderColored('# Title with **STRONG** word')
    // strong closes bold with \x1b[22m; chalk re-opens \x1b[1m for ' word'.
    expect(out).toContain('\x1b[22m\x1b[1m')
    expect(stripAnsi(out)).toBe('Title with STRONG word')
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
    expect(result).toMatch(/^─+$/m)
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

describe('block styling aligned with pi', () => {
  test('code blocks render with ```lang / ``` borders', () => {
    const out = render('```js\nconst a = 1\n```').split('\n').filter(l => l.trim())
    expect(out[0]).toBe('```js')
    expect(out[out.length - 1]).toBe('```')
    expect(out.some(l => l === '  const a = 1')).toBe(true)
  })

  test('unlabelled code block uses a bare ``` (no plaintext tag)', () => {
    const out = render('```\nplain\n```').split('\n').filter(l => l.trim())
    expect(out[0]).toBe('```')
    expect(out[out.length - 1]).toBe('```')
  })

  test('box-drawing diagram art gets no fence', () => {
    const out = render('```\nA --> B\n│ tree\n```')
    expect(out).not.toContain('```')
    expect(out).toContain('A --> B')
  })

  test('blockquote uses the │ border (not ▎)', () => {
    const out = render('> quoted')
    expect(out).toContain('│ quoted')
    expect(out).not.toContain('▎')
  })

  test('hr renders as a full-width ─ rule capped at 80 cols', () => {
    const rule = render('---').split('\n').find(l => /─/.test(l)) ?? ''
    expect(/^─+$/.test(rule)).toBe(true)
    expect(rule.length).toBeLessThanOrEqual(80)
    expect(render('---')).not.toContain('---')
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

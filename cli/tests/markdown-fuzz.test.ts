import { dirname, join } from 'path'
import { fileURLToPath } from 'url'
import { existsSync, readFileSync } from 'fs'
import { describe, expect, test } from 'bun:test'
import stripAnsi from 'strip-ansi'
import { renderMarkdown } from '../src/render/markdown.js'
import { MARKDOWN_NORMALIZE_STAGES, applyMarkdownNormalizeStages } from '../src/markdown/normalize/index.js'

interface Case {
  kind: 'fence' | 'headingListGlue' | 'table' | 'implicitCode' | 'boxDrawing' | 'diagram' | 'seed'
  stage: string
  name: string
  input: string
  expected: string[]
  absent?: RegExp[]
}

const DEFAULT_SEED = 20260509
const DEFAULT_CASES = 300
const __dirname = dirname(fileURLToPath(import.meta.url))
const MARKDOWN_TEST_PATH = join(__dirname, 'markdown.test.ts')
const EXTERNAL_SEED_SEPARATOR_RE = /^---\s*evot-markdown-seed\s*---$/m

const HIGH_RISK_LLM_SEEDS = [
  [
    'Compatibility result:',
    '```sql',
    'CREATE TABLE t (',
    '    id INT,',
    '    d DECIMAL(38, 18)',
    ');```## Driver summary',
    '| Client | Result ||---|---|',
    '| Java | OK |Continue explanation.',
  ].join('\n'),
  [
    '```python',
    'from decimal import Decimal',
    'cur.execute("SELECT d FROM t")',
    'for row in cur.fetchall():',
    '    print(type(row[0]), row[0])```Continue explanation.',
  ].join('\n'),
  [
    '整体流程',
    '',
    '  ┌─────────────────────────────────────────────────────────────┐',
    '  │  输入 Prompt: "The capital ofFrance"  (L=5 tokens)          │',
    '  └──────────────────────────┬──────────────────────────────────┘',
    '                   ▼',
    '          ┌──────────────────────────────────────┐',
    '          │Prefill 阶段 (一次性并行计算)       │',
    '          └──────────────────────────────────────┘',
    '                       │',
    '       ┌─────────────────────┼─────────────────────┐',
    '       ▼                     ▼            ▼',
    '    Q[5,d]              K[5,d]                V[5,d]',
    '       │                     │              │',
    '       │                ▼                     ▼',
    '       │              ┌─────────────┐      ┌─────────────┐',
    '       │      │ KV Cache[0] │      │ KV Cache[0] │',
    '       │           │ K: [5,d] │      │ V: [5,d]    │',
    '       │              └─────────────┘      └─────────────┘',
    '       ▼',
    '    Attention(Q, K, V) → logits →采样→ token_6 = "is"',
    '                  │',
    '                                       ▼',
    '          ┌──────────────────────────────────────┐',
    '          │   Decode 阶段 (每步只输入 1 个 token) │',
    '          └──────────────────────────────────────┘',
    '',
    '单步 Decode 的张量视角',
    '',
    'Step t (cache 当前长度 = L)',
    '─────────────────────────────────────────────────────────',
    '  input_ids:  [1]     ← 只送 1 个新 token',
    '     │',
    '     ▼  embedding',
    '  x: [B, 1, D]',
    '   │',
    '├──► W_q ──► Q_new: [B, H, 1,   d]',
    '     ├──► W_k ──► K_new: [B, H, 1,   d]  ┐',
    '     └──► W_v ──► V_new: [B, H, 1,   d]  ┘',
    '                                  │ append',
    '                                         ▼',
    '         Cache.K: [B, H, L, d] ──► [B, H, L+1, d]',
    '      Cache.V: [B, H, L, d] ──► [B, H, L+1, d]',
    '',
    '## PagedAttention 的分页视角（vLLM）',
    '',
    '              │        │        │        │',
    '              ▼        ▼        ▼   ▼',
    '  block table:  [ blk#7 , blk#2 , blk#9 , blk#3 , ... ]',
    '               │        │        │   │',
    '  物理显存池:      ▼        ▼        ▼  ▼',
    '  ┌────┬────┬────┬────┬────┬────┬────┬────┬────┐',
    '  │blk0│blk1│blk2│blk3│... │blk7│... │blk9│... │   ��块 = block_size',
    '  └────┴────┴────┴────┴────┴────┴────┴────┴────┘      个 token的 K/V 优点：  · 变长序列无碎片',
    ' ·多请求共享相同前缀块 (prefix cache)',
    '   · fork/ rewind 只改 block table，不复制数据',
    '',
    '核心就两句：**prefill 把整个 prompt的 K/V 一次写满 cache；decode 每步只算 1 个新 token 的 Q/K/V，K/V append、Q 对全 cache 做 attention。**',
  ].join('\n'),
]

function render(md: string): string {
  return stripAnsi(renderMarkdown(md)).replace(/\u200b/g, '')
}

function createRng(seed: number): () => number {
  let state = seed >>> 0
  return () => {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0
    return state / 0x100000000
  }
}

function pick<T>(rng: () => number, values: T[]): T {
  return values[Math.floor(rng() * values.length)]!
}

function maybe(rng: () => number, value: string): string {
  return rng() < 0.5 ? value : ''
}

function readExternalSeeds(): string[] {
  const seedPath = process.env.MARKDOWN_FUZZ_SEED_FILE
  if (!seedPath || !existsSync(seedPath)) return []
  const content = readFileSync(seedPath, 'utf8').trim()
  if (!content) return []
  return content
    .split(EXTERNAL_SEED_SEPARATOR_RE)
    .map(seed => seed.trim())
    .filter(Boolean)
}

function unescapeQuotedString(raw: string): string {
  try {
    return JSON.parse(`"${raw.replace(/"/g, '\\"')}"`)
  } catch {
    return raw.replace(/\\n/g, '\n').replace(/\\`/g, '`').replace(/\\'/g, "'")
  }
}

function readMarkdownTestSeeds(): string[] {
  if (!existsSync(MARKDOWN_TEST_PATH)) return []
  const source = readFileSync(MARKDOWN_TEST_PATH, 'utf8')
  const seeds: string[] = []
  const templateRe = /render(?:Markdown)?\(\s*`([\s\S]*?)`\s*\)/g
  for (const match of source.matchAll(templateRe)) {
    const seed = match[1]?.trim()
    if (seed) seeds.push(seed.replace(/\\`/g, '`'))
  }

  const quotedRe = /render(?:Markdown)?\(\s*(['"])((?:\\.|(?!\1)[\s\S])*?)\1\s*\)/g
  for (const match of source.matchAll(quotedRe)) {
    const seed = match[2]?.trim()
    if (seed) seeds.push(unescapeQuotedString(seed))
  }

  const arrayJoinRe = /render(?:Markdown)?\(\s*\[([\s\S]*?)\]\s*\.join\(['"]\\n['"]\)\s*\)/g
  for (const match of source.matchAll(arrayJoinRe)) {
    const body = match[1] ?? ''
    const lines: string[] = []
    const lineRe = /(['"])((?:\\.|(?!\1)[\s\S])*?)\1\s*,?/g
    for (const lineMatch of body.matchAll(lineRe)) {
      lines.push(unescapeQuotedString(lineMatch[2] ?? ''))
    }
    const seed = lines.join('\n').trim()
    if (seed) seeds.push(seed)
  }

  return [...new Set(seeds)].filter(seed => seed.length >= 10 && seed.length <= 4000)
}

function mutateSeed(seed: string, rng: () => number): string {
  let text = seed
  const mutations = [
    () => { text = text.replace(/\n```/g, '```') },
    () => { text = text.replace(/```\n/g, '```') },
    () => { text = text.replace(/\n(#{1,6})\s+/g, '$1') },
    () => { text = text.replace(/\n(\|[-:| ]+\|)\n/g, '$1\n') },
    () => { text = text.replace(/\n(-\s+)/g, '$1') },
    () => { text = text.replace(/\n\n/g, '\n') },
    () => { text = text.replace(/\n(\s*[┌├└│▼▲┤┬┴─])/g, '$1') },
    () => { text = text.replace(/([│┐┘┤])\n(\s*[│┌├└▼▲])/g, '$1$2') },
  ]
  const rounds = 1 + Math.floor(rng() * 3)
  for (let i = 0; i < rounds; i++) pick(rng, mutations)()
  return text
}

function makeSeedCase(rng: () => number, seeds: string[]): Case {
  const seed = pick(rng, seeds.length > 0 ? seeds : HIGH_RISK_LLM_SEEDS)
  const input = mutateSeed(seed, rng)
  return {
    kind: 'seed',
    stage: 'seed-mutation',
    name: 'llm-seed',
    input,
    expected: [],
    absent: [/```/, /\|---/, /<!-- -->/],
  }
}

function makeFenceCase(rng: () => number): Case {
  const lang = pick(rng, ['sql', 'python', 'bash', 'text'])
  const code = pick(rng, [
    'SELECT 1;',
    'print("hello")',
    'npm test',
    'literal **bold** text',
  ])
  const tail = pick(rng, ['Continue explanation.', '## Next section', '- next item'])
  const input = pick(rng, [
    `Intro.\n\n\`\`\`${lang}\n${code}\n\`\`\`${tail}`,
    `Intro.\`\`\`${lang}\n${code}\n\`\`\`\n${tail}`,
    `Intro.\n\n\`\`\`${lang}\n${code}`,
    `Intro.\n\n\`\`\`${lang}\n${code}\`\`\`${tail}`,
  ])

  return {
    kind: 'fence',
    stage: 'fence-repair',
    name: `fence-${lang}`,
    input,
    expected: [code],
    absent: [/```/],
  }
}

function makeHeadingListGlueCase(rng: () => number): Case {
  const input = pick(rng, [
    'Intro sentence.## Heading\nBody text.',
    'Intro sentence.### Heading\nBody text.',
    'Summary:- first item\n- second item',
    'Steps:1. first item\n2. second item',
    'Keep decimals: task_1: 0.8 and v1.2.3 remain text.',
  ])

  return {
    kind: 'headingListGlue',
    stage: 'heading-list-glue',
    name: 'heading-list-glue',
    input,
    expected: input.includes('Heading') ? ['Heading'] : input.includes('first item') ? ['first item'] : ['task_1: 0.8', 'v1.2.3'],
    absent: [/\.#{1,6}\S/, /:[-*+]\S/, /:\d+\.\s*first item/],
  }
}

function makeTableCase(rng: () => number): Case {
  const header = '| Client | Protocol | Focus |'
  const separator = '|---|---|---|'
  const rows = [
    '| Java | MySQL | `getBigDecimal()` precision |',
    '| Spark JDBC | MySQL JDBC | **DecimalType limit** |',
  ]
  const input = pick(rng, [
    `${header}\n${separator}\n${rows.join('\n')}`,
    `${header}${separator}\n${rows.join('\n')}`,
    `${header}\n${separator}\n${rows.join('\n')}Continue explanation.`,
    `Table:\n${header}\n${separator}\n${rows.join('\n')}`,
  ])

  return {
    kind: 'table',
    stage: 'table-glue',
    name: 'table-glue',
    input,
    expected: ['Client', 'Java', 'Spark JDBC'],
    absent: [/\|---/],
  }
}

function makeImplicitCodeCase(rng: () => number): Case {
  const prose = pick(rng, ['Continue explanation.', 'Driver compatibility summary', '这里重点是：'])
  const code = pick(rng, [
    [
      'CREATE TABLE decimal_driver_compat (',
      '    id INT,',
      '    d38 DECIMAL(38, 18)',
      ');',
    ],
    [
      'try (ResultSet rs = stmt.executeQuery("SELECT 1")) {',
      '    System.out.println(rs.getInt(1));',
      '}',
    ],
    [
      'from decimal import Decimal',
      'cur.execute("SELECT d38 FROM decimal_driver_compat")',
      'for row in cur.fetchall():',
      '    print(type(row[0]), row[0])',
    ],
    [
      'val df = spark.read.format("jdbc")',
      '    .option("dbtable", "decimal_driver_compat")',
      '    .load()',
    ],
  ])
  const input = `${maybe(rng, 'Example:\n\n')}${code.join('\n')}\n\n${prose}`

  return {
    kind: 'implicitCode',
    stage: 'implicit-code-blocks',
    name: 'implicit-code',
    input,
    expected: [code[0]!, prose],
    absent: [/```/],
  }
}

function makeBoxDrawingCase(rng: () => number): Case {
  const input = pick(rng, [
    [
      'Final table:',
      '',
      '┌────────┬────────┐',
      '│ Link   │ Result │',
      '├────────┼────────┤',
      '│ Java   │ OK     │',
      '└────────┴────────┘',
    ].join('\n'),
    [
      'Tree:',
      '',
      'project',
      '├── cli',
      '│   └── tests',
      '└── src',
    ].join('\n'),
  ])

  return {
    kind: 'boxDrawing',
    stage: 'box-drawing-preserve',
    name: 'box-drawing',
    input,
    expected: input.includes('┌') ? ['┌', '│', '└'] : ['├── cli', '└── src'],
    absent: [/```text/],
  }
}

function makeCase(rng: () => number, seedCorpus: string[]): Case {
  if (seedCorpus.length > 0 && rng() < 0.45) return makeSeedCase(rng, seedCorpus)
  if (rng() < 0.2) return makeSeedCase(rng, HIGH_RISK_LLM_SEEDS)
  const kind = pick<Exclude<Case['kind'], 'seed'>>(rng, ['fence', 'headingListGlue', 'table', 'implicitCode', 'boxDrawing'])
  switch (kind) {
    case 'fence': return makeFenceCase(rng)
    case 'headingListGlue': return makeHeadingListGlueCase(rng)
    case 'table': return makeTableCase(rng)
    case 'implicitCode': return makeImplicitCodeCase(rng)
    case 'boxDrawing': return makeBoxDrawingCase(rng)
    case 'diagram': return makeBoxDrawingCase(rng)
    case 'seed': return makeSeedCase(rng, seedCorpus)
  }
}

function boxLineCount(text: string): number {
  return text.split('\n').filter(line => /[┌┐└┘├┤┬┴┼│─▼▲]/.test(line)).length
}

function assertCase(c: Case, output: string): void {
  expect(output.length).toBeLessThan(c.input.length * 20 + 1000)
  expect(output).not.toContain('<!-- -->')
  for (const expected of c.expected) {
    expect(output).toContain(expected)
  }
  for (const absent of c.absent ?? []) {
    expect(output).not.toMatch(absent)
  }

  if (c.kind === 'table') {
    expect(output).toContain('┌')
  }
  if (c.kind === 'implicitCode' && c.input.includes('    ')) {
    expect(output).toMatch(/^ {4}\S/m)
  }
  if (c.kind === 'boxDrawing' || c.input.includes('┌') || c.input.includes('├──')) {
    expect(output).toMatch(/[┌└├│]/)
    expect(boxLineCount(output)).toBeGreaterThanOrEqual(Math.min(3, boxLineCount(c.input)))
  }
  if (c.kind === 'seed' && boxLineCount(c.input) >= 8) {
    expect(boxLineCount(output)).toBeGreaterThanOrEqual(Math.floor(boxLineCount(c.input) * 0.6))
    expect(output).toContain('Cache')
  }
}

describe('markdown targeted fuzz', () => {
  test('normalizer exposes named stages for diagnostics', () => {
    expect(MARKDOWN_NORMALIZE_STAGES.map(stage => stage.name)).toEqual([
      'fence-open-glue',
      'fence-close-repair',
      'heading-list-glue',
      'implicit-code-blocks',
      'table-glue',
      'box-drawing-preserve',
      'hr-boundary',
      'emphasis-boundary',
    ])
  })

  test('extracts existing markdown tests as fuzz seeds', () => {
    expect(readMarkdownTestSeeds().length).toBeGreaterThan(50)
  })

  test('rendering invariants hold for mutated markdown cases', () => {
    const seed = Number(process.env.MARKDOWN_FUZZ_SEED ?? DEFAULT_SEED)
    const cases = Number(process.env.MARKDOWN_FUZZ_CASES ?? DEFAULT_CASES)
    const rng = createRng(seed)
    const seedCorpus = [...HIGH_RISK_LLM_SEEDS, ...readMarkdownTestSeeds(), ...readExternalSeeds()]

    for (let i = 0; i < cases; i++) {
      const c = makeCase(rng, seedCorpus)
      let output = ''
      try {
        output = render(c.input)
        assertCase(c, output)
      } catch (error) {
        const normalized = applyMarkdownNormalizeStages(c.input)
        throw new Error([
          `markdown fuzz failed seed=${seed} case=${i} kind=${c.kind} stage=${c.stage} name=${c.name}`,
          '--- input ---',
          c.input,
          '--- normalized ---',
          normalized,
          '--- output ---',
          output,
          '--- error ---',
          error instanceof Error ? error.message : String(error),
        ].join('\n'))
      }
    }
  }, Number(process.env.MARKDOWN_FUZZ_TIMEOUT_MS ?? Math.max(5000, Number(process.env.MARKDOWN_FUZZ_CASES ?? DEFAULT_CASES) * 2)))
})

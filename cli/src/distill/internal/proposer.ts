/**
 * proposer — turn a high-level DomainSpec into concrete TaskSpecs.
 *
 * "Author a coding task" is itself a task for a strong agent, so we drive evot
 * once and ask it to emit a batch of task specs as JSON. The agent decides what
 * to build (builder_prompt), what to ask (prompt), and how success is checked
 * (verifier) — the human only supplies the direction.
 *
 * Loading curated tasks from a JSONL file is the same TaskSpec shape, so both
 * sources flow into one pipeline.
 */

import { readFile } from 'node:fs/promises'
import { randomBytes } from 'node:crypto'
import type { DomainSpec, TaskSpec } from './types.js'
import { runAgent } from './runner.js'

interface ProposerReporter {
  phaseEvent: (label: string, kind: string, payload: Record<string, unknown>) => void
  phase?: (msg: string) => void
}

const PROPOSER_SYSTEM = `You author compact, self-contained coding tasks for an autonomous agent dataset.
Do not call tools. Do not write files. Return ONLY the final JSON array directly in your assistant text.

Separation of responsibilities is mandatory:
- The task author describes WHAT to build and supplies verifier tests.
- The distill runtime owns HOW to install dependencies and run commands.
- Do not put pip, python -m venv, virtualenv, pytest runs, or dependency installation in builderPrompt or setup.

Each element must use this shape:
{
  "prompt": "concise solver task, including API behavior and edge cases",
  "answer": "one-line description of the success state",
  "workspace": {
    "source":"agent_scaffold",
    "builderPrompt":"create only the unsolved BASE project; include requirements.txt and smoke tests, but do not solve the task and do not install/run anything",
    "setup":["mkdir -p verify", "cat > verify/test_<name>.py <<'PY'\n...pytest tests...\nPY"]
  },
  "verifier": {"checkCommand":"python -m pytest verify -q", "expectedExitCode":0},
  "protectedPaths": ["verify/**"]
}

Strict rules:
- Builder creates source files, requirements.txt, and optional smoke tests only.
- Builder must NOT install dependencies, create venvs, or run pytest.
- setup may ONLY create verifier files under verify/.
- verifier.checkCommand must run pytest against verify/ only, preferably: python -m pytest verify -q.
- protectedPaths must include verify/**.
- The base project must NOT already satisfy the verifier.
- Keep tasks offline, deterministic, and workspace-relative. Never use absolute paths, /workspace, /tmp, or home paths.`

/** Drive evot to author `domain.n` tasks. Never throws; returns [] on failure. */
export async function proposeTasks(
  domain: DomainSpec,
  evotBin: string,
  proposerCwd: string,
  targetTurns: number,
  model?: string,
  envFile?: string,
  reporter?: ProposerReporter,
): Promise<TaskSpec[]> {
  const types = domain.taskTypes?.length ? ` Task types: ${domain.taskTypes.join(', ')}.` : ''
  const prompt = `Domain: ${domain.domain}.${types} Author ${domain.n} varied tasks as a JSON array following the system instructions. Target difficulty: each task should usually require about ${targetTurns} solver turns for a competent coding agent. This is a difficulty target, not a hard limit.`

  const maxAttempts = 3
  for (let attempt = 1; attempt <= maxAttempts; attempt++) {
    reporter?.phase?.(`proposer attempt ${attempt}/${maxAttempts} for domain: ${domain.domain}`)
    const res = await runAgent({
      cwd: proposerCwd,
      prompt,
      model,
      envFile,
      systemPrompt: PROPOSER_SYSTEM,
      limits: { maxTurns: 4 },
      timeoutSec: 300,
      evotBin,
      onEvent: (ev) => reporter?.phaseEvent('proposer', ev.kind, ev.payload),
    })

    const text = lastAssistantText(res.events)
    const specs = parseTaskArray(text).map((s) => withDefaults(s, 'evot_auto'))
    const valid = specs.map(canonicalizeAutoTask).filter((t): t is TaskSpec => validateAutoTask(t) === null)
    if (valid.length > 0) return valid.slice(0, domain.n)

    const reason = proposerFailureReason(res.events, res.error, text)
    reporter?.phase?.(`proposer attempt ${attempt}/${maxAttempts} failed: ${reason}`)
  }

  reporter?.phase?.(`proposer fallback: generated ${domain.n} deterministic task(s) for domain: ${domain.domain}`)
  return fallbackTasks(domain).slice(0, domain.n).map((t) => withDefaults(t, 'evot_fallback'))
}

/** Load curated TaskSpecs from a JSONL file. */
export async function loadTasks(path: string): Promise<TaskSpec[]> {
  const body = await readFile(path, 'utf8')
  const out: TaskSpec[] = []
  for (const line of body.split('\n')) {
    const t = line.trim()
    if (!t) continue
    try {
      out.push(withDefaults(JSON.parse(t), 'curated'))
    } catch {
      // skip malformed lines rather than abort the whole file
    }
  }
  return out
}

function validateAutoTask(t: TaskSpec): string | null {
  if (!t.prompt.trim()) return 'empty prompt'
  if (!t.answer.trim()) return 'empty answer'
  if (t.workspace.source !== 'agent_scaffold') return 'auto tasks must use agent_scaffold'
  if (!t.workspace.builderPrompt.trim()) return 'empty builder prompt'
  if (hasRuntimeCommand(t.workspace.builderPrompt)) return 'builder prompt mixes in runtime commands'
  const setup = t.workspace.setup ?? []
  if (!setup.length) return 'missing verifier setup'
  for (const cmd of setup) {
    const head = cmd.trim().split('\n')[0]
    if (hasRuntimeCommand(head)) return `setup mixes in runtime command: ${head}`
    if (!/^mkdir -p verify\b/.test(head) && !/^cat\s+>\s+verify\/[A-Za-z0-9_.-]+\.py\s+<<'?\w+'?/.test(head)) {
      return `setup must only write verify files: ${head}`
    }
  }
  if (hasRuntimeCommand(t.verifier.checkCommand.replace(/python\s+-m\s+pytest|pytest/g, ''))) {
    return 'verifier contains non-pytest runtime commands'
  }
  if (!/^(python\s+-m\s+pytest|pytest)\s+verify(\/|\s|$)/.test(t.verifier.checkCommand.trim())) {
    return 'verifier must run pytest against verify/'
  }
  if (!(t.protectedPaths ?? []).some((p) => p === 'verify/**' || p.startsWith('verify/'))) {
    return 'missing protected verify paths'
  }
  return null
}

function canonicalizeAutoTask(t: TaskSpec): TaskSpec {
  return {
    ...t,
    verifier: { checkCommand: 'python -m pytest verify -q', expectedExitCode: t.verifier.expectedExitCode ?? 0 },
    protectedPaths: Array.from(new Set([...(t.protectedPaths ?? []), 'verify/**'])),
  }
}

function hasRuntimeCommand(text: string): boolean {
  return /(^|[;&|]\s*)(pip3?|python3?\s+-m\s+venv|pytest|uv\s+pip|poetry|virtualenv)\b|\.venv/.test(text)
}

function proposerFailureReason(
  events: { kind: string; payload: Record<string, unknown> }[],
  error: string | undefined,
  text: string,
): string {
  const evError = [...events].reverse().find((e) => e.kind === 'error')
  if (evError) return String(evError.payload.message ?? 'event error').slice(0, 180)
  if (error) return error
  if (!text.trim()) return 'empty assistant response'
  return 'no valid task JSON parsed'
}

function fallbackTasks(domain: DomainSpec): Partial<TaskSpec>[] {
  const baseBuilder = (extra: string) =>
    `Create a minimal Python Flask project at the workspace root. Create requirements.txt with flask and pytest. Create app.py with a Flask app and a GET /health endpoint returning JSON {"status":"ok"}. Create tests/test_smoke.py asserting /health returns 200. ${extra} Do not install dependencies, create virtualenvs, or run tests. Use only relative paths.`

  const tasks: Partial<TaskSpec>[] = [
    {
      id: 'fallback_flask_todos',
      prompt: 'Implement an in-memory JSON REST API for todos in app.py. Add GET /todos returning all todos, POST /todos accepting {title} and creating {id,title,done:false} with status 201, GET /todos/<int:id> returning 200 or 404, PUT /todos/<int:id> updating title/done and returning 200 or 404, and DELETE /todos/<int:id> returning 204 or 404. Missing or empty title on POST must return 400. Keep /health working.',
      answer: 'Flask todo CRUD API passes verify/test_todos.py.',
      workspace: {
        source: 'agent_scaffold',
        builderPrompt: baseBuilder('Do not implement any /todos routes.'),
        setup: [
          'mkdir -p verify',
          `cat > verify/test_todos.py <<'PYEOF'
from app import app


def client():
    app.config['TESTING'] = True
    return app.test_client()


def test_todo_crud():
    c = client()
    assert c.get('/todos').status_code == 200
    r = c.post('/todos', json={'title': 'write tests'})
    assert r.status_code == 201
    todo = r.get_json()
    assert todo['id'] == 1 and todo['title'] == 'write tests' and todo['done'] is False
    assert c.get('/todos/1').get_json()['title'] == 'write tests'
    r = c.put('/todos/1', json={'done': True})
    assert r.status_code == 200 and r.get_json()['done'] is True
    assert c.delete('/todos/1').status_code == 204
    assert c.get('/todos/1').status_code == 404


def test_todo_validation():
    c = client()
    assert c.post('/todos', json={}).status_code == 400
    assert c.post('/todos', json={'title': ''}).status_code == 400
PYEOF`,
        ],
      },
      verifier: { checkCommand: 'python -m pytest verify -q', expectedExitCode: 0 },
      protectedPaths: ['verify/**'],
    },
    {
      id: 'fallback_flask_auth',
      prompt: 'Implement user registration and login in app.py using an in-memory users dict. POST /register accepts {username,password}, validates username length >= 3 and password length >= 8, returns 400 on validation errors, 409 on duplicate username, otherwise stores a hashed password using werkzeug.security.generate_password_hash and returns {username} with status 201. POST /login accepts {username,password}, returns 401 for unknown user or wrong password using check_password_hash, and returns JSON containing token on success. Do not store plaintext passwords. Keep /health working.',
      answer: 'Flask registration and login pass verify/test_auth.py.',
      workspace: {
        source: 'agent_scaffold',
        builderPrompt: baseBuilder('Add an empty users = {} store. Do not implement /register or /login.'),
        setup: [
          'mkdir -p verify',
          `cat > verify/test_auth.py <<'PYEOF'
from app import app, users


def client():
    users.clear()
    app.config['TESTING'] = True
    return app.test_client()


def test_register_login_and_hashing():
    c = client()
    r = c.post('/register', json={'username': 'alice', 'password': 'password1'})
    assert r.status_code == 201
    assert r.get_json()['username'] == 'alice'
    assert users['alice']['password'] != 'password1'
    assert c.post('/register', json={'username': 'alice', 'password': 'password1'}).status_code == 409
    assert c.post('/login', json={'username': 'alice', 'password': 'wrongpass'}).status_code == 401
    r = c.post('/login', json={'username': 'alice', 'password': 'password1'})
    assert r.status_code == 200 and 'token' in r.get_json()


def test_register_validation():
    c = client()
    assert c.post('/register', json={'username': 'ab', 'password': 'password1'}).status_code == 400
    assert c.post('/register', json={'username': 'alice', 'password': 'short'}).status_code == 400
PYEOF`,
        ],
      },
      verifier: { checkCommand: 'python -m pytest verify -q', expectedExitCode: 0 },
      protectedPaths: ['verify/**'],
    },
    {
      id: 'fallback_flask_products',
      prompt: 'Implement GET /products in app.py using the existing PRODUCTS list. Return JSON {items, meta} where meta has page, per_page, total. Support page and per_page integer query params with defaults 1 and 10; invalid or non-positive values return 400. Support min_price and max_price numeric filters applied before pagination; invalid numeric filters return 400. Keep /health working.',
      answer: 'Flask products pagination and filtering pass verify/test_products.py.',
      workspace: {
        source: 'agent_scaffold',
        builderPrompt: baseBuilder('Create data.py with PRODUCTS = [{"id": i + 1, "name": f"p{i + 1}", "price": (i + 1) * 10} for i in range(25)]. Do not implement /products.'),
        setup: [
          'mkdir -p verify',
          `cat > verify/test_products.py <<'PYEOF'
from app import app


def client():
    app.config['TESTING'] = True
    return app.test_client()


def test_default_page():
    r = client().get('/products')
    assert r.status_code == 200
    data = r.get_json()
    assert len(data['items']) == 10
    assert data['meta']['page'] == 1
    assert data['meta']['per_page'] == 10
    assert data['meta']['total'] == 25


def test_filter_then_paginate():
    r = client().get('/products?min_price=50&max_price=120&per_page=3&page=2')
    data = r.get_json()
    assert data['meta']['total'] == 8
    assert len(data['items']) == 3
    assert all(50 <= p['price'] <= 120 for p in data['items'])


def test_invalid_params():
    c = client()
    assert c.get('/products?page=0').status_code == 400
    assert c.get('/products?per_page=no').status_code == 400
    assert c.get('/products?min_price=x').status_code == 400
PYEOF`,
        ],
      },
      verifier: { checkCommand: 'python -m pytest verify -q', expectedExitCode: 0 },
      protectedPaths: ['verify/**'],
    },
  ]

  if (!/flask|python|后端|backend/i.test(domain.domain)) return tasks
  return tasks
}

function withDefaults(raw: Partial<TaskSpec> & Record<string, any>, source: string): TaskSpec {
  const workspace = normalizeWorkspace(raw.workspace)
  return {
    id: raw.id || `${source === 'curated' ? 'cur' : 'auto'}_${randomBytes(3).toString('hex')}`,
    prompt: raw.prompt ?? '',
    answer: raw.answer ?? '',
    workspace,
    verifier: normalizeVerifier(raw.verifier),
    referencePatch: raw.referencePatch ?? raw.reference_patch,
    protectedPaths: raw.protectedPaths ?? raw.protected_paths,
    limits: raw.limits,
    targetTurns: raw.targetTurns ?? raw.target_turns,
    split: raw.split ?? 'train',
    source,
  }
}

function normalizeWorkspace(raw: any): TaskSpec['workspace'] {
  if (!raw || typeof raw !== 'object') return { source: 'inline', files: {} }
  const setup = Array.isArray(raw.setup) ? raw.setup.map(rewriteWorkspacePath) : undefined
  if (raw.source === 'agent_scaffold') {
    return {
      source: 'agent_scaffold',
      builderPrompt: String(raw.builderPrompt ?? raw.builder_prompt ?? ''),
      ...(setup ? { setup } : {}),
    }
  }
  if (raw.source === 'inline') return { source: 'inline', files: raw.files ?? {}, ...(setup ? { setup } : {}) }
  if (raw.source === 'dir') return { source: 'dir', path: String(raw.path ?? ''), ...(setup ? { setup } : {}) }
  if (raw.source === 'git' || raw.source === 'git_local') {
    return { source: raw.source, repo: String(raw.repo ?? ''), ref: raw.ref, ...(setup ? { setup } : {}) }
  }
  return { source: 'inline', files: raw.files ?? {}, ...(setup ? { setup } : {}) }
}

function normalizeVerifier(raw: any) {
  const command = String(raw?.checkCommand ?? raw?.check_command ?? raw?.expected_command ?? 'true')
  return {
    checkCommand: rewriteWorkspacePath(command),
    expectedExitCode: raw?.expectedExitCode ?? raw?.expected_exit_code ?? 0,
  }
}

function rewriteWorkspacePath(cmd: string): string {
  return cmd.replaceAll('/workspace/', '').replaceAll('/workspace', '.')
}

function lastAssistantText(events: { kind: string; payload: Record<string, unknown> }[]): string {
  for (let i = events.length - 1; i >= 0; i--) {
    const ev = events[i]
    if (ev.kind !== 'assistant_completed') continue
    const blocks = (ev.payload.content as Record<string, unknown>[]) ?? []
    const text = blocks
      .filter((b) => b.type === 'text')
      .map((b) => String(b.text ?? ''))
      .join('\n')
    if (text.trim()) return text
  }
  return ''
}

/** Extract the first JSON array from agent text (it may wrap it in prose/fences). */
function parseTaskArray(text: string): Partial<TaskSpec>[] {
  const start = text.indexOf('[')
  const end = text.lastIndexOf(']')
  if (start === -1 || end <= start) return []
  try {
    const arr = JSON.parse(text.slice(start, end + 1))
    return Array.isArray(arr) ? arr : []
  } catch {
    return []
  }
}

<p align="center">
  <img src="https://github.com/user-attachments/assets/d241ce05-ea15-4932-bec8-f8705f39dbba" alt="bendclaw" />
</p>

<p align="center">
  <strong>BendClaw — AgentOS for AI Teams</strong>
</p>

<p align="center">
  One shared brain. Agents that remember, learn from each other, and work as a team.
</p>


---

## What is BendClaw

BendClaw is an AgentOS — an operating system for running AI agent teams. Not a single-agent framework. Not a chatbot wrapper. A runtime where multiple agents share one brain, learn from each other's runs, and collaborate on tasks across a distributed cluster.

Traditional agent runtimes treat each agent as isolated — separate memory, separate context, separate mistakes. BendClaw is different: all agents read and write to one shared Databend store. When one agent discovers a better approach, every agent in the team inherits it.

BendClaw is the core runtime behind [evot.ai](https://evot.ai).

---

## Why BendClaw

### Shared Brain — Collective Intelligence

All agents read and write to one Databend store. Memories, skills, and config are shared. On top of that, every run produces two kinds of recall:

- **Knowledge** — automatically extracted from tool executions. Structured metadata: what was done, where, when, by which run.
- **Learnings** — written by agents themselves. Patterns with conditions (when to apply), strategies (how to apply), priority, and confidence scores that track success/failure over time.

Both are injected into every future run's prompt. When one agent learns something, the entire team benefits on the next run. The team gets smarter together, not just individually.

### AI Swarm — Cluster-Native Collaboration

BendClaw nodes form a swarm. Each node registers with a cluster registry, sends heartbeats, and discovers peers automatically. Any agent can decompose a complex goal, fan out subtasks to agents on remote nodes via `cluster_dispatch`, and assemble results with `cluster_collect`. No central orchestrator — any agent can be the coordinator.

### Elastic — Scale to Zero, Scale to N

All persistent state lives in Databend. Instances hold only ephemeral runtime state (active sessions, peer cache, dispatch tracking) — all reconstructable on restart. Add nodes, remove them, scale to zero. No coordination overhead.

---

## How It Works

```
 Client / Console              BendClaw                        Databend
   │                              │                               │
   │── POST /v1/runs ────────────▶│                               │
   │                              │── load config, history ──────▶│
   │                              │── load learnings + knowledge ▶│
   │                              │── load variables ────────────▶│
   │                              │◀── prompt layers (11) ────────│
   │                              │                               │
   │◀── SSE: events ────────────  │── agent loop ─────────────────│
   │                              │   ┌─ LLM call                 │
   │                              │   ├─ tool execution           │
   │                              │   ├─ cluster_dispatch ──────▶ peer node
   │                              │   ├─ trace recording ────────▶│
   │                              │   └─ repeat until done        │
   │                              │                               │
   │                              │── persist run + events ──────▶│
   │                              │── recall: extract knowledge ─▶│
   │                              │                               │
   │                              │   Next run inherits all ─────▶│
```

### Prompt Layers

Every run assembles a system prompt from 11 layers:

```
Identity → Soul → System Prompt → Skills → Tools → Learnings → Recall → Variables → Recent Errors → Cluster → Runtime
```

- **Learnings**: agent-written patterns with conditions, strategies, and priority
- **Recall**: auto-extracted knowledge from past tool executions
- **Variables**: user-defined key-value pairs injected into context
- **Cluster**: peer node topology for distributed dispatch
- **Recent Errors**: errors from recent runs, so agents avoid repeating mistakes

---

## Architecture

```
                    Instances are ephemeral — scale in/out freely
                                      │
             ┌────────────────────────┼────────────────────────┐
             ▼                        ▼                        ▼
    ┌──────────────┐         ┌──────────────┐         ┌──────────────┐
    │  BendClaw    │◄───────▶│  BendClaw    │◄───────▶│  BendClaw    │
    │  ┌─────────┐ │ cluster │  ┌─────────┐ │ cluster │  ┌─────────┐ │
    │  │ Gateway │ │  RPC    │  │ Gateway │ │  RPC    │  │ Gateway │ │
    │  ├─────────┤ │         │  ├─────────┤ │         │  ├─────────┤ │
    │  │ Kernel  │ │         │  │ Kernel  │ │         │  │ Kernel  │ │
    │  ├─────────┤ │         │  ├─────────┤ │         │  ├─────────┤ │
    │  │Scheduler│ │         │  │Scheduler│ │         │  │Scheduler│ │
    │  └─────────┘ │         │  └─────────┘ │         │  └─────────┘ │
    └──────┬───────┘         └──────┬───────┘         └──────┬───────┘
           └────────────────────────┼────────────────────────┘
                                    ▼
                ┌─────────────────────────────────────┐
                │              Databend               │
                │                                     │
                │  sessions · runs · run_event        │
                │  memories (vector + FTS)            │
                │  learnings · knowledge              │
                │  skills · traces · spans            │
                │  tasks · task_history               │
                │  config · config_versions           │
                │  variables · feedback               │
                │  channel_accounts · channel_messages│
                │                                     │
                │  One store per agent. Shared brain. │
                └─────────────────────────────────────┘
```

| Layer | Role |
|---|---|
| **Gateway** | HTTP routing, SSE streaming, Bearer auth, CORS, request logging |
| **Kernel** | Agent loop, LLM router (Anthropic / OpenAI) with circuit breaker and failover, tool registry, context compaction, prompt builder |
| **Recall** | Post-run knowledge extraction, learning accumulation, prompt injection |
| **Scheduler** | Cron-based task polling (15s interval), per-agent task execution |
| **Cluster** | Node registration, heartbeat (30s), peer discovery, subtask dispatch and collection |
| **Channels** | Webhook ingestion, channel account management, supervisor for receiver lifecycle |
| **Databend** | Single source of truth — all agent data, one database per agent |

---

## Built-in Tools

| Category | Tools | Description |
|---|---|---|
| **File** | `file_read`, `file_write`, `file_edit`, `list_dir` | Workspace file operations (sandbox mode optional) |
| **Shell** | `shell` | Allowlisted commands with configurable timeout |
| **Memory** | `memory_write`, `memory_read`, `memory_search`, `memory_list`, `memory_delete` | Long-term memory with vector + full-text search |
| **Skill** | `skill_read`, `create_skill`, `remove_skill` | Skill documentation access and management |
| **Recall** | `learning_write`, `learning_search`, `knowledge_search` | Agent self-improvement: write learnings, search accumulated knowledge |
| **Task** | `task_create`, `task_list`, `task_get`, `task_update`, `task_delete`, `task_toggle`, `task_history` | Cron task self-management |
| **Web** | `web_search`, `web_fetch` | Web search and page fetching |
| **Databend** | `databend` | SQL queries against the agent's Databend database |
| **Channel** | `channel_send` | Send messages through connected channels |
| **Cluster** | `cluster_nodes`, `cluster_dispatch`, `cluster_collect` | Discover peers, dispatch subtasks to other agents, collect results |

---

## API

All endpoints served from `/v1`. All routes require `Authorization: Bearer <key>` except `/health` and channel webhooks.

<details>
<summary>Agents</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents` | GET | List agents |
| `/v1/agents/{agent_id}` | GET / DELETE | Get or delete agent |
| `/v1/agents/{agent_id}/setup` | POST | Create agent database |

</details>

<details>
<summary>Sessions</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/sessions` | GET / POST | List or create sessions |
| `/v1/agents/{agent_id}/sessions/{session_id}` | GET / PUT / DELETE | Session CRUD |

</details>

<details>
<summary>Runs</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/runs` | GET / POST | List runs or start a run (JSON or SSE) |
| `/v1/agents/{agent_id}/runs/{run_id}` | GET | Get run with events |
| `/v1/agents/{agent_id}/runs/{run_id}/cancel` | POST | Cancel run |
| `/v1/agents/{agent_id}/runs/{run_id}/continue` | POST | Continue paused run |
| `/v1/agents/{agent_id}/runs/{run_id}/events` | GET | List run events |
| `/v1/agents/{agent_id}/sessions/{session_id}/runs` | GET | Runs for session |

</details>

<details>
<summary>Memories</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/memories` | GET / POST | List or create memories |
| `/v1/agents/{agent_id}/memories/{memory_id}` | GET / DELETE | Get or delete memory |
| `/v1/agents/{agent_id}/memories/search` | POST | Semantic + full-text search |

</details>

<details>
<summary>Learnings</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/learnings` | GET / POST | List or create learnings |
| `/v1/agents/{agent_id}/learnings/{learning_id}` | GET / DELETE | Get or delete learning |
| `/v1/agents/{agent_id}/learnings/search` | POST | Search learnings |

</details>

<details>
<summary>Knowledge</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/knowledge` | GET / POST | List or create knowledge entries |
| `/v1/agents/{agent_id}/knowledge/{knowledge_id}` | GET / DELETE | Get or delete knowledge |
| `/v1/agents/{agent_id}/knowledge/search` | POST | Search knowledge |

</details>

<details>
<summary>Skills</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/skills` | GET / POST | List or create skills |
| `/v1/agents/{agent_id}/skills/{skill_name}` | GET / DELETE | Get or delete skill |

</details>

<details>
<summary>Hub</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/hub/skills` | GET | List available hub skills |
| `/v1/hub/skills/{skill_name}/credentials` | GET | Required credentials for a skill |
| `/v1/hub/status` | GET | Hub sync status |

</details>

<details>
<summary>Config</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/config` | GET / PUT | Read or update config |
| `/v1/agents/{agent_id}/config/versions` | GET | List config versions |
| `/v1/agents/{agent_id}/config/versions/{version}` | GET | Get specific version |
| `/v1/agents/{agent_id}/config/rollback` | POST | Roll back to a version |

</details>

<details>
<summary>Traces</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/traces` | GET | List traces |
| `/v1/agents/{agent_id}/traces/summary` | GET | Trace summary |
| `/v1/agents/{agent_id}/traces/{trace_id}` | GET | Get trace |
| `/v1/agents/{agent_id}/traces/{trace_id}/spans` | GET | List spans |

</details>

<details>
<summary>Usage</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/usage` | GET | Agent usage summary |
| `/v1/agents/{agent_id}/usage/daily` | GET | Daily usage breakdown |
| `/v1/usage/summary` | GET | Global usage across all agents |

</details>

<details>
<summary>Variables</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/variables` | GET / POST | List or create variables |
| `/v1/agents/{agent_id}/variables/{var_id}` | GET / PUT / DELETE | Variable CRUD |

</details>

<details>
<summary>Tasks</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/tasks` | GET / POST | List or create scheduled tasks |
| `/v1/agents/{agent_id}/tasks/{task_id}` | PUT / DELETE | Update or delete task |
| `/v1/agents/{agent_id}/tasks/{task_id}/toggle` | POST | Enable or disable task |
| `/v1/agents/{agent_id}/tasks/{task_id}/history` | GET | Task execution history |

</details>

<details>
<summary>Feedback</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/feedback` | GET / POST | List or create feedback |
| `/v1/agents/{agent_id}/feedback/{feedback_id}` | DELETE | Delete feedback |

</details>

<details>
<summary>Channels</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/channels/accounts` | GET / POST | List or create channel accounts |
| `/v1/agents/{agent_id}/channels/accounts/{account_id}` | GET / DELETE | Get or delete channel account |
| `/v1/agents/{agent_id}/channels/messages` | GET | List channel messages |
| `/v1/agents/{agent_id}/channels/webhook/{account_id}` | POST | Receive inbound webhook (no auth) |

</details>

<details>
<summary>Stats & Health</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/v1/stats/sessions` | GET | Active session stats |
| `/v1/stats/can_suspend` | GET | Whether the instance can suspend |

</details>

---

## Configuration

Three-layer merge: **config file** < **env vars** < **CLI args**.

| Env var | Config path | Description |
|---|---|---|
| `BENDCLAW_INSTANCE_ID` | `instance_id` | **Required.** Unique ID for this instance |
| `BENDCLAW_STORAGE_DATABEND_API_BASE_URL` | `storage.databend_api_base_url` | **Required.** Databend Cloud API URL |
| `BENDCLAW_STORAGE_DATABEND_API_TOKEN` | `storage.databend_api_token` | **Required.** Databend API token |
| `BENDCLAW_SERVER_BIND_ADDR` | `server.bind_addr` | Listen address (default `127.0.0.1:8787`) |
| `BENDCLAW_AUTH_KEY` | `auth.api_key` | Bearer auth key (empty = auth disabled) |

All configuration — including instance ID, Databend credentials, LLM providers, and cluster settings — can be obtained and managed through [evot.ai](https://evot.ai). See [`configs/bendclaw.toml.example`](configs/bendclaw.toml.example) for a minimal local config template.

---

## Development

```bash
make setup    # install protoc, git hooks
make run      # start with dev config at localhost:8787
make check    # fmt + clippy
make test     # unit + integration + contract (no credentials needed)
make test-live  # requires Databend credentials
make coverage   # generate HTML coverage report
```

First run creates `~/.bendclaw/bendclaw_dev.toml` from the example config. Configure your LLM provider API keys and Databend credentials before use.

---

## License

Apache-2.0

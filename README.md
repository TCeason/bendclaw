<p align="center">
  <strong>Evot</strong>
</p>

<p align="center">
  An agent engine that completes complex, long-running work with minimal tokens and maximum quality.
</p>

<p align="center">
  <a href="#benchmark">Benchmark</a> &middot;
  <a href="#-why-evot">Why</a> &middot;
  <a href="#installation">Install</a> &middot;
  <a href="#quickstart">Quickstart</a> &middot;
  <a href="#development">Dev</a> &middot;
  <a href="#community">Community</a>
</p>

<p align="center">
  <video src="https://github.com/user-attachments/assets/0c089005-51db-48da-977e-6339b5fb9093"></video>
</p>

## Benchmark

Same task, same eval environment, different models. evot completes the work with fewer tokens, less time, and lower cost — across both frontier and open-source models.

<table align="center">
  <tr>
    <td align="center"><strong>Claude Opus 4.6</strong></td>
    <td align="center"><strong>DeepSeek V4 Pro</strong></td>
  </tr>
  <tr>
    <td><a href=".github/assets/benchmark-opus-4.6.png"><img src=".github/assets/benchmark-opus-4.6.png" alt="evot benchmark — Claude Opus 4.6" width="480" /></a></td>
    <td><a href=".github/assets/benchmark-deepseek-v4-pro.png"><img src=".github/assets/benchmark-deepseek-v4-pro.png" alt="evot benchmark — DeepSeek V4 Pro" width="480" /></a></td>
  </tr>
</table>

> Task: Fix a real bug in serde_json ([issue #979](https://github.com/serde-rs/json/issues/979)) — investigate root cause, apply fix, write regression test, verify all tests pass.

| Model | Metric | evot | claude-code | Difference |
|-------|--------|------|-------------|------------|
| Opus 4.6 | Cost | $2.24 | $6.16 | **64% cheaper** |
| Opus 4.6 | Time | 2m 56s | 3m 51s | **24% faster** |
| Opus 4.6 | Input tokens | 574.8K | 1.5M | **62% fewer** |
| DeepSeek V4 Pro | Cost | $0.02 | $0.07 | **67% cheaper** |
| DeepSeek V4 Pro | Time | 6m 10s | 16m 34s | **63% faster** |
| DeepSeek V4 Pro | Input tokens | 42.9K | 133.8K | **68% fewer** |

All agents produce correct, passing code. The difference is in how they manage context.

### Why is evot faster and cheaper?

Evot's goal: **complete tasks fast and well, without wasting a single token.** Every design decision serves this — give the LLM less context, but higher quality context.

Other agents accumulate everything and call the LLM to summarize when context overflows — extra tokens, extra latency. Evot uses **zero LLM calls for context management**:

- **Algorithmic compaction** — a four-pass Rust pipeline (Reclaim → Shrink → Collapse → Evict) runs in microseconds between every turn. Images downgrade to path references; old turns collapse to one-line summaries.
- **Spill to disk** — large tool results write to disk with a short preview. The model re-reads on demand instead of carrying megabytes in context.
- **Compaction markers** — structured metadata (files modified, conclusions, environment state) survives compaction. Progress is never lost.

Fewer tokens, higher signal density. Fast, high-quality task completion — no token wasted.

**Quantitative benchmarking against the best.**
Evot maintains a reproducible eval pipeline that runs the same real-world tasks against Claude Code and Codex (latest versions). Every engine change is validated against these baselines — token usage, cost, time, and task success rate must improve or hold. This ensures continuous improvement without regression.

## 📢 News

- **2026-05-17** [REPL] `/goal` — autonomous objectives, e.g. `/goal remove unwraps in Rust context compaction`.
- **2026-05-11** [Skills] Built-in `opencli` — control the browser, use logged-in cookies, read Feishu/Lark messages, Twitter/X timelines, and more.
- **2026-05-11** [Slim] Tool outputs now auto-compact, with token savings shown inline.
- **2026-05-08** [REPL] `/harden` — stress-test plans and git changes before shipping. Inspired by [@cjzafir](https://x.com/cjzafir/status/2052110266566107321).
- **2026-05-02** [Skills] Builtin skill support — `review` ships built-in, no install needed.
- **2026-04-28** [Image] Resize, preserve through compaction, persist to disk.
- **2026-04-23** [Search] Full-text session search — `/resume <query>` to find any past conversation.
- **2026-04-18** [REPL] `/history` + `/goto` — time-travel through conversation context.

---

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://evot.ai/install | sh
```

### From source

```bash
git clone https://github.com/evotai/evot.git
cd evot
make setup && make install
evot
```

## Quickstart

**1. Set your API key**

Create `~/.evotai/evot.env`:

```env
# Anthropic (default)
EVOT_LLM_ANTHROPIC_API_KEY=sk-ant-...
EVOT_LLM_ANTHROPIC_BASE_URL=your-anthropic-base-url
EVOT_LLM_ANTHROPIC_MODEL=claude-opus-4-6
# Multiple models: EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6,claude-opus-4-6

# Or OpenAI
# EVOT_LLM_OPENAI_API_KEY=sk-...
# EVOT_LLM_OPENAI_BASE_URL=your-openai-base-url/v1
# EVOT_LLM_OPENAI_MODEL=gpt-5.5

# Or DeepSeek (Anthropic-compatible)
# EVOT_LLM_DEEPSEEK_API_KEY=sk-...
# EVOT_LLM_DEEPSEEK_BASE_URL=https://api.deepseek.com/anthropic
# EVOT_LLM_DEEPSEEK_PROTOCOL=anthropic
# EVOT_LLM_DEEPSEEK_MODEL=deepseek-v4-pro

# Or Xiaomi MiMo-V2.5-Pro (Anthropic-compatible)
# EVOT_LLM_XIAOMI_API_KEY=tp-...
# EVOT_LLM_XIAOMI_BASE_URL=https://token-plan-cn.xiaomimimo.com/anthropic
# EVOT_LLM_XIAOMI_PROTOCOL=anthropic
# EVOT_LLM_XIAOMI_MODEL=mimo-v2.5-pro
```

> Use `--model provider:model` for one-off overrides.

**2. Run**

```bash
evot                                          # interactive REPL
evot -p "summarize today's PRs"               # one-shot task
evot -p "review this" -f ./src/main.rs        # attach file context
evot -p "continue work" -c                   # continue latest session in cwd
evot -p "continue work" -r my-session         # resume or create session
```

<details>
<summary><b>CLI flags & options</b></summary>

| Flag | Description |
|------|-------------|
| `-p, --prompt` | Run a single prompt and exit |
| `-f, --file <path>` | Attach file/directory context |
| `-c, --continue` | Continue the latest session in the current directory |
| `-r, --resume <id>` | Resume or create a session |
| `--model <model>` | Override the configured model |
| `--verbose` | Enable info-level logging |

</details>

## Development

```bash
make setup        # install Rust toolchain, git hooks
make test         # all tests (engine + CLI)
make install      # compile standalone binary to ~/.evotai/bin/evot
```

## Community

- [**GitHub Issues**](https://github.com/evotai/evot/issues) — Bug reports / Feature
- [**Twitter @Evot_AI**](https://twitter.com/Evot_AI) — Announcements

## License

Apache-2.0

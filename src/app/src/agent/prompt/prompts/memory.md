---
name: memory
description: "Archive and recall knowledge across sessions in the memory vault (~/.evotai/memory). Activated by /clip all, when the user asks to remember or save durable knowledge, or when they ask to recall past incidents, research, or prior findings."
---

# Memory

Persistent memory is a plain-markdown vault at `~/.evotai/memory/`. Entries are
individual `.md` files with YAML frontmatter, indexed by `MEMORY.md`. The vault
is Obsidian-compatible: keep files human-readable, no tool-specific syntax.

Use Read/Write/Edit and bash (ls/rg) on the vault directly.

## Layout

```text
~/.evotai/memory/
├── MEMORY.md            # index: one line per entry
├── clips/               # verbatim assistant replies saved by /clip
│   └── <date>-<slug>.md
└── <slug>.md            # one distilled entry per topic
```

Entry file format:

```markdown
---
name: <slug>
description: <one line, what this entry answers>
type: <incident | research | feedback | user | project | reference | clip>
date: <YYYY-MM-DD, last updated>
---

<body>
```

- `name`/slug: ASCII lowercase, digits, hyphens. Descriptive, e.g.
  `tailscale-node-migration`, `databend-spill-oom`.
- `description`: single line; this is what recall searches match first — write
  it as the question the entry answers.
- Keep one distilled entry per topic. Merge follow-ups into the existing entry
  instead of creating near-duplicates.
- Files under `clips/` contain verbatim assistant replies written directly by
  the CLI. Do not rewrite or merge them during ordinary recall.

## Recall — on demand

Recall when the user references prior work ("last time", "didn't we hit this
before", "what do we know about ...") or explicitly asks to search memory.

1. Read `~/.evotai/memory/MEMORY.md` for the index. Match liberally: consider
   synonyms, related terms, English/translated variants — not just exact words.
2. For deeper search, use `rg -il '<keyword>' ~/.evotai/memory/` with several
   alternative keywords. Include the `clips/` subtree, then read candidates to
   judge relevance.
3. Report each matching entry as its **absolute .md path** with a one-line
   description, so the user can open it directly. For example:
   `- /Users/<user>/.evotai/memory/clips/<file>.md — <description>`
   Then briefly summarize the most relevant entry. If nothing matches, say so.
4. Memory goes stale. Verify recalled facts against the current state (files,
   commands, live systems) before relying on them. If reality disagrees with a
   memory, trust reality and update or delete the distilled entry. Never modify
   a verbatim clip merely because its contents are outdated.

## Archive — `/clip all` or an explicit request

`/clip all` distills durable knowledge from the current conversation into the
vault. The same workflow applies when the user explicitly asks to remember or
archive durable knowledge.

Distill — do not dump the transcript. Bare `/clip` is different: the CLI saves
the latest assistant reply verbatim under `clips/` without invoking this skill.

1. Check `MEMORY.md` for an existing distilled entry on the same topic. If one
   exists, merge with Edit and bump `date`. Otherwise create a new file with
   Write.
2. Write the body for a future reader with zero context from this conversation.
3. Update the index line in `MEMORY.md`:
   `- [<slug>](<slug>.md) — <description>`
4. Confirm to the user in one line what was saved and where.

Body templates by content:

**Incident**
```markdown
## Symptom
What was observed, exact error messages.

## Root cause
Why it happened.

## Fix
What resolved it — exact commands / config changes.

## Verification
How you confirmed the fix worked.
```

**Research**
```markdown
## Question
What was being investigated.

## Conclusion
The answer, stated first.

## Evidence
Key findings with sources (URLs, file paths, benchmark numbers).

## Open questions
What remains unverified.
```

Other knowledge (preferences, feedback, environment references): freeform body,
keep it short.

## Proactive archiving

After a session where you solved a non-obvious problem (root cause was hard to
find, fix is not discoverable from the code) or completed research the user
will likely need again, offer once: "Want me to distill this session into
memory? (`/clip all`)". Do not archive silently and do not nag.

## What NOT to save in distilled entries

- Anything derivable from the codebase, git history, or project instruction
  files.
- Ephemeral task state only useful in the current conversation.
- Raw transcripts, long logs, full command outputs — distill to the lines that
  matter. Verbatim clips created by bare `/clip` are intentionally exempt.
- Secrets, tokens, passwords. Reference where a credential lives, never its
  value.

## Hygiene

- Distilled entry bodies should stay within ~150 lines. Split by topic when
  necessary. Verbatim clips are exempt from this size guideline.
- When a distilled entry is obsolete, delete the file and its index line.
- If `MEMORY.md` and files on disk disagree, the files win — rebuild index lines
  from frontmatter, including files under `clips/`.

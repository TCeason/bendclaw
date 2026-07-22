---
name: memory
description: "Archive and recall knowledge across sessions in the memory vault (~/.evotai/memory). Activated by the /mem command (bare = archive, with terms = search), or when the user asks to remember/memorize/save something for later, or asks to recall past incidents, research, or prior findings."
---

# Memory

Persistent memory is a plain-markdown vault at `~/.evotai/memory/`. Entries are
individual `.md` files with YAML frontmatter, indexed by `MEMORY.md`. The vault
is Obsidian-compatible: keep files human-readable, no tool-specific syntax.

Use Read/Write/Edit and bash (ls/rg) on the vault directly.

## Layout

```
~/.evotai/memory/
├── MEMORY.md            # index: one line per entry
└── <slug>.md            # one entry per topic
```

Entry file format:

```markdown
---
name: <slug>
description: <one line, what this entry answers>
type: <incident | research | feedback | user | project | reference>
date: <YYYY-MM-DD, last updated>
---

<body>
```

- `name`/slug: ASCII lowercase, digits, hyphens. Descriptive, e.g.
  `tailscale-node-migration`, `databend-spill-oom`.
- `description`: single line; this is what recall searches match first — write
  it as the question the entry answers.
- Keep one entry per topic. Merge follow-ups into the existing entry instead of
  creating near-duplicates.

## Recall — the /mem <terms> command, or on demand

The user searches memory with `/mem <terms>`. Also recall on your own when the
user references prior work ("last time", "didn't we hit this before", "what do
we know about ...").

1. Read `~/.evotai/memory/MEMORY.md` for the index. Match liberally: consider
   synonyms, related terms, English/translated variants — not just the exact
   words given.
2. For deeper search: `rg -il '<keyword>' ~/.evotai/memory/` with several
   alternative keywords, then read the candidate entries to judge relevance.
3. Report each matching entry as its **absolute .md path** with a one-line
   description, so the user can open it directly:
   `- /Users/<user>/.evotai/memory/<slug>.md — <description>`
   Then briefly summarize the most relevant entry. If nothing matches, say so.
4. Memory goes stale. Verify recalled facts against the current state (files,
   commands, live systems) before relying on them. If reality disagrees with a
   memory, trust reality and update or delete the entry.

## Archive — the bare /mem command

The user archives with the bare `/mem` command: distill the durable knowledge
from the current conversation into the vault.

Distill — do not dump the transcript.

1. Check `MEMORY.md` for an existing entry on the same topic. If one exists,
   merge with Edit and bump `date`. Otherwise create a new file with Write.
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
will likely need again, offer once: "Want me to save this to memory? (/mem)".
Don't archive silently and don't nag.

## What NOT to save

- Anything derivable from the codebase, git history, or project instruction
  files.
- Ephemeral task state only useful in the current conversation.
- Raw transcripts, long logs, full command outputs — distill to the lines that
  matter.
- Secrets, tokens, passwords. Reference where a credential lives, never its
  value.

## Hygiene

- Entry body ≤ ~150 lines. If it grows past that, split by topic.
- When an entry is obsolete, delete the file and its index line.
- If `MEMORY.md` and the files on disk disagree, the files win — rebuild the
  index lines from the entries' frontmatter.

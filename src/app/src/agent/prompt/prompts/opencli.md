---
name: opencli
description: "Use OpenCLI for websites, browser sessions, logged-in web apps, Twitter/X, Feishu/Lark, Hacker News, search, page interaction, extraction, and registered external CLIs. Trigger on: browser, browse, open page, click, fill form, extract page, twitter, x/twitter, feishu, lark, hackernews, search."
---

# OpenCLI

OpenCLI exposes websites, browser sessions, web apps, desktop apps, and registered tools as CLI commands.
Use it when a site/app/service can be operated through an OpenCLI adapter or the browser bridge.

## Basic flow

1. Check the binary first:

```bash
command -v opencli
```

If missing, check Node.js:

```bash
node -v
```

If Node.js is available and major version is >= 21, install OpenCLI automatically:

```bash
npm install -g @jackwener/opencli
command -v opencli
opencli doctor
```

If Node.js is missing or older than 21, stop and tell the user to install or upgrade Node.js first.

2. Discover live capabilities:

```bash
opencli list -f json
opencli <adapter> -h
opencli <adapter> <command> -h
```

3. Prefer a matching adapter when present. Otherwise use `opencli browser`.
4. Prefer structured output (`-f json`) when supported.
5. Do not guess command names or flags; use live help.

## Browser dependency

Only check/install browser support when the selected path needs it:

- `opencli browser ...`
- logged-in cookies or session state
- page UI automation, clicking, forms, extraction from a live tab
- adapters whose live metadata/help shows `COOKIE`, `INTERCEPT`, or `UI` strategy

For `PUBLIC` or `LOCAL` adapters, do not require the Chrome extension.

When browser support is needed, run:

```bash
opencli doctor
```

If the browser bridge or Chrome extension is unavailable, stop browser-dependent execution and open the extension install page for the user:

```bash
open "https://chromewebstore.google.com/detail/opencli/ildkmabpimmkaediidaifkhjpohdnifk"
```

Then ask the user to click "Add to Chrome", enable the extension, keep Chrome running, log into the target site if needed, and retry. Do not attempt to install the extension silently.

Do not ask the user to export cookies. For cookie/session tasks, run commands in the bound browser context instead.

## Common routing hints

Confirm names with `opencli list -f json` before use.

- `browser`: ordinary websites, logged-in pages, clicking, forms, extraction.
- `twitter`: Twitter/X timelines, search, posts, profiles, notifications.
- `feishu` / `lark` / `lark-cli`: chats, messages, docs, search, sending.
- `hackernews`: stories and discussion search.
- `github`: repositories, issues, PRs, code lookup.
- `google` or search adapters: broad web lookup.

## Feishu / Lark priority

For any Feishu or Lark task (messages, groups, docs, calendar, contacts, etc.), always prefer `opencli lark-cli` over browser-based access. The `lark-cli` adapter provides structured API access that is faster, more reliable, and does not require browser/extension setup.

Try this first:

```bash
opencli lark-cli --help
opencli lark-cli <subcommand> --help
```

Key subcommands: `im` (messages/groups), `docs` (documents), `calendar`, `contacts`, `drive`.
Only fall back to `opencli browser` for Feishu if `lark-cli` is unavailable or the specific operation is not supported.

## Browser workflow

For a new page:

```bash
opencli doctor
opencli browser open <url>
opencli browser state
```

For an already-open logged-in tab:

```bash
opencli browser bind --domain <domain>
opencli browser --workspace bound:default state
```

Use `state`, `find`, `click`, `type`, `keys`, `get`, and `extract`. Refresh state after navigation or major DOM changes. Do not reuse stale refs.

## Safety and failures

Reading, searching, listing, and extracting are usually safe. Sending, posting, liking, deleting, editing, following, purchasing, settings changes, and mutating SQL are mutations.

If a command fails: read the error, check live help/strategy, run `opencli doctor` for browser-dependent tasks, and retry only when the fix is clear. Report missing installation, missing extension, login, CAPTCHA, rate limit, or API failure directly.

Do not expose credentials, cookies, tokens, or private browser data. Do not invent data when OpenCLI cannot retrieve it.

Final answer: report the result, not the mechanics; mention sources/pages only when helpful.

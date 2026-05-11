---
name: opencli
description: "Use OpenCLI for websites, browser sessions, logged-in web apps, Twitter/X, Feishu/Lark, Hacker News, Databend Cloud at app.databend.com, search, page interaction, extraction, and registered external CLIs. Trigger on: browser, browse, open page, click, fill form, extract page, twitter, x/twitter, feishu, lark, hackernews, databend cloud, app.databend.com, search."
---

# OpenCLI

OpenCLI exposes websites, browser sessions, web apps, desktop apps, and registered tools as CLI commands.
Use it when a site/app/service can be operated through an OpenCLI adapter or the browser bridge.

## Basic flow

1. Check the binary first:

```bash
command -v opencli
```

If missing, stop and tell the user OpenCLI is not installed or not on `PATH`; it requires Node.js >= 21 and can be installed with `npm install -g @jackwener/opencli`.

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

Run `opencli doctor` before browser-dependent work:

- `opencli browser ...`
- adapters with `COOKIE`, `INTERCEPT`, or `UI` strategy

If the browser bridge or Chrome extension is unavailable, stop browser-dependent execution and ask the user to enable the OpenCLI Chrome extension, keep Chrome running, and log into the target site if needed. Chrome extension: `https://chromewebstore.google.com/detail/opencli/ildkmabpimmkaediidaifkhjpohdnifk`. `PUBLIC` and `LOCAL` adapters may still be used.

Any task that needs logged-in cookies or session state requires the Chrome extension/browser bridge. Do not ask the user to export cookies; run commands in the bound browser context instead.

## Common routing hints

Confirm names with `opencli list -f json` before use.

- `browser`: ordinary websites, logged-in pages, clicking, forms, extraction.
- `twitter`: Twitter/X timelines, search, posts, profiles, notifications.
- `feishu` / `lark` / `lark-cli`: chats, messages, search, sending.
- `hackernews`: stories and discussion search.
- `github`: repositories, issues, PRs, code lookup.
- `google` or search adapters: broad web lookup.
- `app.databend.com`: Databend Cloud; use browser auth plus Cloud APIs, not local Databend config.

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

## Databend Cloud

For Databend Cloud at `https://app.databend.com/`, the browser tab is only an auth carrier. It requires the Chrome extension/browser bridge because the SQL/API calls need the user's logged-in cookies. Use `opencli browser eval` so browser cookies are sent with `credentials: "include"`.

Do not use DOM, URL, localStorage, side menus, visible worksheet UI, or metadata endpoints as the answer source for database/table/SQL tasks.

For database/table/SQL tasks, use worksheet SQL API:

- Discover orgs: `GET /api/v1/my/orgs`
- Discover warehouses: `GET /api/v1/admin/orgs/<orgSlug>/warehouses`
- Warehouse fallback: `GET /api/v1/orgs/<orgSlug>/tenant/warehouses`
- Execute SQL: `POST /v1/query`
- Follow results: returned `next_uri`, `stats_uri`, `final_uri`, or `GET /v1/query/<id>/page/<n>`

Required SQL headers:

- `X-DATABENDCLOUD-ORG: <orgSlug>`
- `X-DATABENDCLOUD-WAREHOUSE: <warehouseName>`

SQL mapping:

- "show/list databases" -> `SHOW DATABASES` through `POST /v1/query`
- "list tables" -> `SHOW TABLES` or `SHOW FULL TABLES FROM <database>` through `POST /v1/query`
- "describe table" -> `DESCRIBE <database>.<table>` through `POST /v1/query`

Do not answer database lists from `/api/v1/orgs/<orgSlug>/tenant/databases`. If `/v1/query` fails, report/debug the SQL API failure instead of silently switching endpoints.

Minimal Databend Cloud pattern:

```bash
opencli browser bind --domain app.databend.com
opencli browser --workspace bound:default eval '(async () => {
  const sql = "SHOW DATABASES";
  async function json(url, init = {}) {
    const res = await fetch(url, { credentials: "include", ...init });
    let body;
    try { body = await res.json(); } catch { body = await res.text(); }
    return { ok: res.ok, status: res.status, url, body };
  }

  const orgsResp = await json("/api/v1/my/orgs");
  const orgs = Array.isArray(orgsResp.body?.data) ? orgsResp.body.data : orgsResp.body;
  const org = Array.isArray(orgs) ? orgs.find(o => o.default || o.isDefault) || orgs[0] : undefined;
  const orgSlug = org?.orgSlug || org?.slug || org?.name;
  if (!orgSlug) return { step: "choose-org", orgsResp };

  const whResp = await json(`/api/v1/admin/orgs/${orgSlug}/warehouses`);
  const whFallback = whResp.ok ? undefined : await json(`/api/v1/orgs/${orgSlug}/tenant/warehouses`);
  const whBody = whResp.ok ? whResp.body : whFallback.body;
  const warehouses = Array.isArray(whBody?.data) ? whBody.data : whBody;
  const warehouse = Array.isArray(warehouses) ? warehouses[0] : undefined;
  const warehouseName = warehouse?.name || warehouse?.warehouseName;
  if (!warehouseName) return { step: "choose-warehouse", orgSlug, whResp, whFallback };

  const query = await json("/v1/query", {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "X-DATABENDCLOUD-ORG": orgSlug,
      "X-DATABENDCLOUD-WAREHOUSE": warehouseName
    },
    body: JSON.stringify({ sql, string_fields: true })
  });
  return { orgSlug, warehouseName, query };
})()'
```

For mutating SQL (`CREATE`, `DROP`, `ALTER`, `INSERT`, `UPDATE`, `DELETE`, grants, settings changes), proceed only when the user explicitly asks and the target is unambiguous.

## Safety and failures

Reading, searching, listing, and extracting are usually safe. Sending, posting, liking, deleting, editing, following, purchasing, settings changes, and mutating SQL are mutations.

If a command fails: read the error, check live help/strategy, run `opencli doctor` for browser-dependent tasks, and retry only when the fix is clear. Report missing installation, missing extension, login, CAPTCHA, rate limit, or API failure directly.

Do not expose credentials, cookies, tokens, or private browser data. Do not invent data when OpenCLI cannot retrieve it.

Final answer: report the result, not the mechanics; mention sources/pages only when helpful.

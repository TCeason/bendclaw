/* Shared left rail, loaded as a classic script in <head> on every page.
 *
 * `<evot-rail>` is defined before the body is parsed, so the browser renders
 * the rail while it parses the page, before first paint. The old approach
 * moved nodes in after the page loaded, and the late insert made the layout
 * jump. The session list and account row are painted from a small cache first,
 * so opening a trace or settings page shows the same rail at once instead of
 * a skeleton. Chat (`data-mode="chat"`) then takes the list over with its full
 * behaviour. Other pages refresh it themselves, and each row links back to
 * that session in Chat.
 */
(() => {
  const SESSIONS_KEY = "evot-rail-sessions";
  const ACCOUNT_KEY = "evot-rail-account";
  const PAGE = 30;
  const PRIMARY = [{ href: "/chat", name: "Chat" }];
  const SETTINGS = [
    { href: "/models", name: "Models" },
    { href: "/feishu", name: "Feishu" },
  ];

  const esc = (value) => String(value ?? "").replace(/[&<>"']/g, (c) => (
    { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]
  ));
  const label = (s) => s.custom_title || s.title || s.user_prompts?.[0] || "Untitled session";
  function relTime(iso) {
    const then = Date.parse(iso || "");
    if (Number.isNaN(then)) return "";
    const secs = Math.max(0, (Date.now() - then) / 1000);
    if (secs < 60) return "just now";
    if (secs < 3600) return Math.floor(secs / 60) + "m ago";
    if (secs < 86400) return Math.floor(secs / 3600) + "h ago";
    const days = Math.floor(secs / 86400);
    return days < 30 ? days + "d ago" : new Date(then).toLocaleDateString();
  }

  function read(key) {
    try { return JSON.parse(window.localStorage.getItem(key) || "null"); } catch (_) { return null; }
  }
  function write(key, value) {
    try { window.localStorage.setItem(key, JSON.stringify(value)); } catch (_) { /* Cache is optional. */ }
  }
  /** Only what a row renders; the rest of a session never touches storage. */
  function rememberSessions(list) {
    if (!Array.isArray(list)) return;
    write(SESSIONS_KEY, list.slice(0, PAGE).filter((s) => s?.session_id).map((s) => ({
      session_id: s.session_id,
      custom_title: s.custom_title || "",
      title: s.title || "",
      user_prompts: s.user_prompts?.[0] ? [s.user_prompts[0]] : [],
      updated_at: s.updated_at || "",
    })));
  }
  const cachedSessions = () => (Array.isArray(read(SESSIONS_KEY)) ? read(SESSIONS_KEY) : null);
  const rememberAccount = (account) => write(ACCOUNT_KEY, account);

  function here() {
    return window.location.pathname.replace(/\/+$/, "") || "/";
  }
  function activeSession() {
    const trace = here().match(/^\/sessions\/([^/]+)\/trace$/);
    if (trace) return decodeURIComponent(trace[1]);
    return new URLSearchParams(window.location.search).get("session") || "";
  }
  function isActive(href) {
    const path = here();
    return path === href || (href === "/chat" && (path === "/" || /^\/sessions\/[^/]+\/trace$/.test(path)));
  }
  const links = (items) => items.map((item) => {
    const on = isActive(item.href);
    return '<a href="' + item.href + '"' + (on ? ' class="active" aria-current="page"' : "") + ">" +
      esc(item.name) + "</a>";
  }).join("");

  const SKELETON = '<div class="sidebar-empty" aria-busy="true">' +
    '<div class="sk-row"><span class="sk wide"></span></div>'.repeat(5) + "</div>";

  /** Chat rows mirror chat.js' markup; other pages link into Chat instead. */
  function rowsHtml(list, chat) {
    if (!list) return SKELETON;
    if (!list.length) return '<div class="sidebar-empty">No conversations yet</div>';
    const active = activeSession();
    return list.filter((s) => s?.session_id).map((s) => {
      const id = esc(s.session_id);
      const body = '<span class="recent-title">' + esc(label(s)) + "</span>" +
        '<span class="recent-time">' + esc(relTime(s.updated_at)) + "</span>";
      const open = chat
        ? '<button type="button" class="recent-open" data-session="' + id + '">' + body + "</button>"
        : '<a class="recent-open" href="/chat?session=' + encodeURIComponent(s.session_id) + '">' + body + "</a>";
      return '<div class="recent-item' + (s.session_id === active ? " active" : "") + '">' + open + "</div>";
    }).join("");
  }

  function accountHtml(account, chat) {
    if (!account) return "";
    const tag = chat ? 'button type="button"' : 'a href="/chat"';
    const end = chat ? "button" : "a";
    return account.email
      ? "<" + tag + ' class="auth-row" title="Account"><span class="ws-icon" aria-hidden="true">◉</span>' +
        '<span class="auth-email">' + esc(account.email) + "</span></" + end + ">"
      : "<" + tag + ' class="auth-row"><span class="ws-icon" aria-hidden="true">→</span><span>Log in</span></' + end + ">";
  }

  function themeButton() {
    const target = document.documentElement.dataset.theme === "light" ? "dark" : "light";
    return '<button class="theme-toggle" type="button" aria-label="Switch to ' + target +
      ' mode" title="Switch to ' + target + ' mode" aria-pressed="' + String(target === "dark") + '">' +
      target[0].toUpperCase() + target.slice(1) + " mode</button>";
  }

  function html(chat) {
    return '<div class="rail-brand"><a class="brand" href="/"><span class="mark">' +
      '<svg width="16" height="16" viewBox="0 0 18 18" fill="none" aria-hidden="true">' +
      '<rect x="1" y="1" width="16" height="16" stroke="currentColor" stroke-width="1.6"/>' +
      '<rect x="5.5" y="5.5" width="7" height="7" fill="currentColor"/></svg>' +
      "</span>evot <span>console</span></a></div>" +
      '<nav class="rail-nav rail-primary" aria-label="Console">' + links(PRIMARY) + "</nav>" +
      '<div class="rail-slot"><div class="chat-sidebar" aria-label="Chat sessions">' +
      '<button class="new-chat" id="newChat" type="button"><span aria-hidden="true">＋</span><span>New chat</span></button>' +
      '<button class="session-search" id="openSearch" type="button"><span aria-hidden="true">⌕</span>' +
      "<span>Search sessions</span><kbd>⌘K</kbd></button>" +
      '<div class="sidebar-label">Recent</div>' +
      '<nav class="recent-sessions" id="recentSessions" aria-label="Recent sessions">' +
      rowsHtml(cachedSessions(), chat) + "</nav>" +
      '<div class="sidebar-auth" id="authBox" aria-live="polite">' + accountHtml(read(ACCOUNT_KEY), chat) + "</div>" +
      "</div></div>" +
      '<section class="rail-settings" aria-label="Settings"><div class="rail-label">Settings</div>' +
      '<nav class="rail-nav" aria-label="Settings pages">' + links(SETTINGS) + "</nav>" +
      '<div class="rail-row"><span>Appearance</span>' + themeButton() + "</div></section>";
  }

  /** Outside Chat the rail refreshes itself; every action lands in Chat. */
  async function hydrate(rail) {
    rail.querySelector("#newChat")?.addEventListener("click", () => { window.location.href = "/chat"; });
    rail.querySelector("#openSearch")?.addEventListener("click", () => { window.location.href = "/chat?search=1"; });
    const list = rail.querySelector("#recentSessions");
    const auth = rail.querySelector("#authBox");
    try {
      const response = await fetch("/api/sessions?limit=" + PAGE + "&offset=0");
      if (response.ok) {
        const items = (await response.json())?.items;
        if (Array.isArray(items)) {
          rememberSessions(items);
          if (list) list.innerHTML = rowsHtml(cachedSessions(), false);
        }
      }
    } catch (_) { /* Keep the cached rows. */ }
    try {
      const response = await fetch("/api/auth/session");
      if (response.ok) {
        const session = await response.json();
        const account = session.logged_in ? { email: session.email || session.name || "Account" } : {};
        rememberAccount(account);
        if (auth) auth.innerHTML = accountHtml(account, false);
      }
    } catch (_) { /* Keep the cached account row. */ }
  }

  class EvotRail extends HTMLElement {
    connectedCallback() {
      if (this.dataset.ready) return;
      this.dataset.ready = "true";
      const chat = this.dataset.mode === "chat";
      this.innerHTML = html(chat);
      if (!chat) void hydrate(this);
    }
  }

  window.evotRail = { html, rowsHtml, cachedSessions, rememberSessions, rememberAccount };
  if (!customElements.get("evot-rail")) customElements.define("evot-rail", EvotRail);
})();

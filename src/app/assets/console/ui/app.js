/* Shared helpers for the evot local console pages: escaping, fetch wrappers,
   toasts, and the sidenav active marker. No framework and no build step — each
   page is a plain document that loads this file. */

import { createJsonClient } from "./json-client.js";

export const $ = (id) => document.getElementById(id);

/** Escape for interpolation into HTML. Always used on server-supplied text. */
export function esc(value) {
  const div = document.createElement("div");
  div.textContent = value == null ? "" : String(value);
  return div.innerHTML;
}

let toastTimer = 0;
let loadBarCount = 0;
let loadBarEl = null;
let loadBarHide = 0;

function loadBar() {
  if (!loadBarEl || !document.body.contains(loadBarEl)) {
    const el = document.createElement("div");
    el.className = "load-bar";
    el.setAttribute("aria-hidden", "true");
    loadBarEl = el;
  }
  // Keep the bar a direct child of body: page chrome relocates existing
  // children, and a nested bar can be clipped by overflow:hidden regions.
  if (loadBarEl.parentNode !== document.body) document.body.appendChild(loadBarEl);
  return loadBarEl;
}

/** Top-of-window refresh bar, shared by every in-flight JSON request. */
export function beginLoad() {
  loadBarCount += 1;
  const el = loadBar();
  window.clearTimeout(loadBarHide);
  el.className = "load-bar on";
}

export function endLoad() {
  loadBarCount = Math.max(0, loadBarCount - 1);
  if (loadBarCount > 0) return;
  const el = loadBar();
  el.className = "load-bar done";
  loadBarHide = window.setTimeout(() => {
    if (loadBarCount === 0) el.className = "load-bar";
  }, 220);
}

/** Shimmer rows matching the admin skeleton. */
export function skeletonHtml(rows = 5, kind = "line") {
  if (kind === "form") {
    return '<div class="panel" aria-busy="true">' +
      '<div class="panel-body">' +
      Array.from({ length: rows }, () =>
        '<div class="sk-row"><span class="sk mid"></span></div>' +
        '<div class="sk-row"><span class="sk wide"></span></div>',
      ).join("") +
      "</div></div>";
  }
  if (kind === "cloud") {
    return Array.from({ length: rows }, () =>
      '<div class="sk-row"><span class="sk mid"></span><span class="sk wide"></span></div>',
    ).join("");
  }
  return Array.from({ length: rows }, () =>
    '<div class="sk-row"><span class="sk wide"></span><span class="sk narrow"></span></div>',
  ).join("");
}

/**
 * Show a transient message. `kind` may be "err" to render it as a failure.
 * Passing no message hides the toast immediately.
 */
export function toast(message, kind) {
  const el = $("toast");
  if (!el) return;
  window.clearTimeout(toastTimer);
  if (!message) {
    el.className = "toast";
    return;
  }
  el.textContent = message;
  el.className = "toast show" + (kind ? " " + kind : "");
  toastTimer = window.setTimeout(() => {
    el.className = "toast";
  }, kind === "err" ? 6000 : 2600);
}

export const { getJson, postJson } = createJsonClient({ begin: beginLoad, end: endLoad });

/**
 * Render the shared rail + page header into `document.body` and return the
 * element that page content goes into. Called before any page-specific rendering.
 *
 * Not usable by a page whose markup must survive: this replaces `body`.
 *
 * `title` is the page heading; `actions` is optional HTML for the header's
 * right side; `lede` is an optional one-line description under the heading.
 *
 * `fill` gives the content the remaining viewport without padding, for pages
 * that manage their own scrolling regions. The header bar is identical either way.
 *
 * `actions` is inserted as HTML, so it must only ever be a literal from the
 * calling page — never server data.
 */
export function mountShell({ title, lede, actions, fill }) {
  // Every page gets the same compact header bar as Chat's conversation header.
  const head =
    '<header class="page-head"><div class="page-titles"><h1>' + esc(title) + "</h1>" +
    '<p class="page-meta"><span>' + esc(lede || "") + '</span><span class="cwd" id="shell-cwd"></span></p>' +
    '</div><div class="page-actions">' + (actions || "") + "</div></header>";

  if (fill) document.documentElement.classList.add("fill");
  document.title = title + " · evot";

  document.body.innerHTML =
    '<div class="shell' + (fill ? " fill" : "") + '">' +
    // rail.js (loaded in <head>) upgrades this element synchronously.
    '<evot-rail class="sidenav"></evot-rail>' +
    '<div class="main">' +
    head +
    '<div class="' + (fill ? "fill-content" : "content") + '" id="content"></div>' +
    "</div></div>" +
    '<div class="toast" id="toast" role="status" aria-live="polite"></div>';

  return $("content");
}

/** Show the config path in the page header meta line. Ignored when unavailable. */
export function setShellCwd(text) {
  const el = $("shell-cwd");
  if (el && text) el.textContent = text;
}

/** Compact byte count, e.g. 1.4 GB. */
export function formatBytes(n) {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return (v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)) + " " + units[i];
}

/** Compact count, e.g. 12.3k. */
export function formatCount(n) {
  if (!Number.isFinite(n)) return "0";
  if (n < 1000) return String(n);
  if (n < 1_000_000) return (n / 1000).toFixed(n < 10_000 ? 1 : 0) + "k";
  return (n / 1_000_000).toFixed(1) + "M";
}

/** Coarse relative time; the console only needs day-level precision. */
export function relTime(iso) {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return "";
  const secs = Math.max(0, (Date.now() - then) / 1000);
  if (secs < 60) return "just now";
  if (secs < 3600) return Math.floor(secs / 60) + "m ago";
  if (secs < 86400) return Math.floor(secs / 3600) + "h ago";
  const days = Math.floor(secs / 86400);
  if (days < 30) return days + "d ago";
  return new Date(then).toLocaleDateString();
}

/* Catalog tier → display name. THE single copy: the Models page's Cloud rows
   and the Chat composer's optgroups both render server tier ids through this,
   so a relabel lands everywhere at once and the per-protocol provider names
   (evot-pro-anthropic, …) never reach either surface. Unknown tiers degrade to
   Title Case of the raw id, so a new server-side tier groups fine on its own
   instead of vanishing or leaking enum soup. */
const TIER_LABELS = { base: "Evot Free", special: "Evot Premium" };

export function tierLabel(tier) {
  const key = String(tier || "").trim().toLowerCase();
  if (!key) return "";
  return TIER_LABELS[key] || key.charAt(0).toUpperCase() + key.slice(1);
}

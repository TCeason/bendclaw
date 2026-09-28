// Shared Console theme: run in <head> so a saved light theme paints immediately.
(() => {
  const key = "evot-theme";
  const root = document.documentElement;
  let storage;
  try { storage = window.localStorage; } catch (_) { /* Private browsing. */ }

  function readTheme() {
    try {
      // Migrate the chat-only preference from earlier Console builds.
      return (storage?.getItem(key) ?? storage?.getItem("evot-chat-theme")) === "light" ? "light" : "dark";
    } catch (_) { return "dark"; }
  }

  function applyTheme(theme) {
    if (theme === "light") root.dataset.theme = "light";
    else delete root.dataset.theme;
    document.querySelectorAll(".theme-toggle").forEach((button) => {
      const target = theme === "light" ? "dark" : "light";
      const label = `${target[0].toUpperCase()}${target.slice(1)} mode`;
      if (button.textContent !== label) button.textContent = label;
      button.setAttribute("aria-label", `Switch to ${target} mode`);
      button.setAttribute("title", `Switch to ${target} mode`);
      button.setAttribute("aria-pressed", String(theme === "light"));
    });
  }

  applyTheme(readTheme());
  // Models and Feishu build their headers after DOMContentLoaded; observe only
  // additions to the page shell instead of coupling the theme to those modules.
  function bindButton(button) {
    if (button.dataset.themeBound) return;
    button.dataset.themeBound = "true";
    button.addEventListener("click", () => {
      const next = root.dataset.theme === "light" ? "dark" : "light";
      applyTheme(next);
      try { storage?.setItem(key, next); } catch (_) { /* In-memory toggle still works. */ }
    });
  }
  function syncButtons() {
    const buttons = document.querySelectorAll(".theme-toggle");
    buttons.forEach(bindButton);
    applyTheme(root.dataset.theme);
  }
  document.addEventListener("DOMContentLoaded", () => {
    syncButtons();
    new MutationObserver(syncButtons).observe(document.body, { childList: true });
  });
})();

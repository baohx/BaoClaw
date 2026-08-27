// BaoClaw Web theme system
// - Applies a theme by setting <html data-theme="...">
// - "auto" (default) removes the attribute and follows prefers-color-scheme:
//   the dark :root palette applies as-is, and a prefers-color-scheme: light
//   media block in style.css overrides it for light OS settings.
// - Choice persists in localStorage ("baoclaw-theme").
// - Mermaid re-initializes per theme so diagrams match (dark vs light base).

(function () {
  "use strict";

  const STORAGE_KEY = "baoclaw-theme";

  // id → label. Order = picker order. Grouped visually with optgroups below.
  const THEMES = [
    { id: "auto", label: "Auto (system)", group: "General", mermaid: null },
    { id: "", label: "BaoClaw Classic", group: "General", mermaid: "dark" },
    {
      id: "catppuccin-latte",
      label: "Catppuccin Latte",
      group: "Catppuccin",
      mermaid: "default",
    },
    {
      id: "catppuccin-frappe",
      label: "Catppuccin Frappé",
      group: "Catppuccin",
      mermaid: "dark",
    },
    {
      id: "catppuccin-macchiato",
      label: "Catppuccin Macchiato",
      group: "Catppuccin",
      mermaid: "dark",
    },
    {
      id: "catppuccin-mocha",
      label: "Catppuccin Mocha",
      group: "Catppuccin",
      mermaid: "dark",
    },
    {
      id: "tokyo-night",
      label: "Tokyo Night",
      group: "Tokyo Night",
      mermaid: "dark",
    },
    {
      id: "tokyo-night-storm",
      label: "Tokyo Night Storm",
      group: "Tokyo Night",
      mermaid: "dark",
    },
    {
      id: "tokyo-night-day",
      label: "Tokyo Night Day",
      group: "Tokyo Night",
      mermaid: "default",
    },
    { id: "dracula", label: "Dracula", group: "Popular", mermaid: "dark" },
    { id: "nord", label: "Nord", group: "Popular", mermaid: "dark" },
    {
      id: "gruvbox-dark",
      label: "Gruvbox Dark",
      group: "Gruvbox",
      mermaid: "dark",
    },
    {
      id: "gruvbox-light",
      label: "Gruvbox Light",
      group: "Gruvbox",
      mermaid: "default",
    },
  ];

  const isLight = (id) => /latte|day|light/.test(id);

  function applyHljs(id) {
    // highlightjs ships separate light/dark stylesheets; toggle them so
    // syntax colors match the active theme family.
    const light = document.getElementById("hljs-light");
    const dark = document.getElementById("hljs-dark");
    if (!light || !dark) return;
    if (id === "auto") {
      const prefersLight = window.matchMedia(
        "(prefers-color-scheme: light)",
      ).matches;
      light.disabled = !prefersLight;
      dark.disabled = prefersLight;
    } else {
      const useLight = isLight(id);
      light.disabled = !useLight;
      dark.disabled = useLight;
    }
  }

  function apply(id) {
    if (!THEMES.some((t) => t.id === id)) id = "auto";
    applyHljs(id);
    if (id === "auto") {
      delete document.documentElement.dataset.theme;
      const mq = window.matchMedia("(prefers-color-scheme: light)");
      setMermaidTheme(mq.matches ? "default" : "dark");
    } else {
      document.documentElement.dataset.theme = id;
      setMermaidTheme(THEMES.find((t) => t.id === id).mermaid || "dark");
    }
  }

  let currentMermaidTheme = null;
  function setMermaidTheme(theme) {
    if (theme === currentMermaidTheme || typeof mermaid === "undefined") return;
    currentMermaidTheme = theme;
    try {
      mermaid.initialize({ startOnLoad: false, securityLevel: "loose", theme });
    } catch (e) {
      /* noop */
    }
  }

  function save(id) {
    try {
      localStorage.setItem(STORAGE_KEY, id);
    } catch (e) {
      /* private mode */
    }
  }

  function load() {
    try {
      const v = localStorage.getItem(STORAGE_KEY);
      return THEMES.some((t) => t.id === v) ? v : "auto";
    } catch (e) {
      return "auto";
    }
  }

  function buildPicker(select, current) {
    select.innerHTML = "";
    const groups = [...new Set(THEMES.map((t) => t.group))];
    for (const g of groups) {
      const og = document.createElement("optgroup");
      og.label = g;
      for (const t of THEMES.filter((x) => x.group === g)) {
        const opt = document.createElement("option");
        opt.value = t.id;
        opt.textContent = t.label;
        og.appendChild(opt);
      }
      select.appendChild(og);
    }
    select.value = current;
  }

  function init() {
    const select = document.getElementById("theme-picker");
    if (!select) return;
    const initial = load();
    buildPicker(select, initial);
    apply(initial);
    select.addEventListener("change", () => {
      apply(select.value);
      save(select.value);
    });
    // Follow live OS scheme changes while in "auto"
    window
      .matchMedia("(prefers-color-scheme: light)")
      .addEventListener("change", () => {
        if (load() === "auto") apply("auto");
      });
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

  window.BaoClawThemes = { apply, load, THEMES };
})();

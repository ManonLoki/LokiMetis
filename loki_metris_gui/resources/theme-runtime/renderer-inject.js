((cssText, themeCssText, backgroundDataUrl, themeAssets, themeConfig) => {
  const STATE_KEY = "__CODEX_DREAM_SKIN_STATE__";
  const DISABLED_KEY = "__CODEX_DREAM_SKIN_DISABLED__";
  const STYLE_ID = "codex-dream-skin-style";
  const CHROME_ID = "codex-dream-skin-chrome";
  const VERSION = __DREAM_SKIN_VERSION_JSON__;
  const theme = themeConfig && typeof themeConfig === "object" ? themeConfig : {};
  const HOST_SELECTORS = Object.freeze({
    shellMain: "main.main-surface, #root",
    home: "[data-testid='home-icon'], [data-feature='game-source']",
    chat: ".thread-scroll-container, [data-virtualized-turn-content], [data-message-author-role]",
    pullRequests: "#pull-request-inbox-search, [id$='-pull-request-summary-tab'], [id$='-pull-request-code-tab'], [id$='-pull-request-activity-tab']",
    sites: "#appgen-site-search, [data-testid='library-file-thumbnail']",
    automations: "#automation-detail-panel-title, [aria-labelledby='automation-detail-panel-title'], .automation-row",
    plugins: "#plugins-page-search, #plugins-page-manage-search, [id^='plugins-search-'], [id^='plugins-marketplace-']",
    settings: "[data-settings-panel-slug], input[role='searchbox']",
  });
  const ROUTE_SURFACES = Object.freeze([
    [/^\/pull-requests(?:\/|$)/, "pull-requests"],
    [/^\/sites(?:\/|$)/, "sites"],
    [/^\/automations(?:\/|$)/, "automations"],
    [/^\/skills(?:\/|$)/, "plugins"],
    [/^\/settings(?:\/|$)/, "settings"],
  ]);
  const objectUrls = [];
  let observer = null;
  let timer = null;
  let scheduled = null;
  let stopped = false;
  let runtimeState = null;
  let ownedStyle = null;
  let ownedChrome = null;
  // 外层包装器在namespace转换后提供归属探针，原始导出协议仍可独立执行。
  const hasForeignOwner = typeof __lokiMetrisHasForeignOwner === "function"
    ? __lokiMetrisHasForeignOwner : () => false;
  const colorScheme = window.matchMedia?.("(prefers-color-scheme: dark)") || null;

  const previous = window[STATE_KEY];
  if (previous?.cleanup) {
    try { previous.cleanup(); } catch {}
  }
  window[DISABLED_KEY] = false;

  const objectUrl = (dataUrl) => {
    if (!dataUrl) return "";
    const comma = dataUrl.indexOf(",");
    if (comma < 0) return "";
    const mime = /^data:([^;,]+)/.exec(dataUrl)?.[1] || "application/octet-stream";
    const binary = atob(dataUrl.slice(comma + 1));
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
    objectUrls.push(url);
    return url;
  };

  const assets = {
    background: objectUrl(backgroundDataUrl),
    backgrounds: Object.fromEntries(
      Object.entries(themeAssets?.backgrounds || {})
        .filter(([mode, value]) => ["light", "dark"].includes(mode) && typeof value === "string" && value)
        .map(([mode, value]) => [mode, objectUrl(value)]),
    ),
  };

  const detectShell = () => {
    const marker = [
      document.documentElement?.getAttribute("data-theme"),
      document.documentElement?.getAttribute("data-appearance"),
      document.body?.getAttribute("data-theme"),
      document.documentElement?.className,
      document.body?.className,
    ].filter(Boolean).join(" ").toLowerCase();
    if (/\belectron-dark\b|\b(dark|theme-dark|appearance-dark)\b/.test(marker)) return "dark";
    if (/\belectron-light\b|\b(light|theme-light|appearance-light)\b/.test(marker)) return "light";
    return colorScheme?.matches ? "dark" : "light";
  };

  const routePath = () => {
    const initialRoute = new URLSearchParams(location.search).get("initialRoute");
    const hashPath = location.hash.match(/^#(\/[^?]*)/)?.[1];
    return initialRoute || hashPath || location.pathname || "/";
  };

  const detectSurface = () => {
    for (const [pattern, surface] of ROUTE_SURFACES) {
      if (pattern.test(routePath())) return surface;
    }
    for (const [surface, selector] of [
      ["pull-requests", HOST_SELECTORS.pullRequests],
      ["sites", HOST_SELECTORS.sites],
      ["automations", HOST_SELECTORS.automations],
      ["plugins", HOST_SELECTORS.plugins],
      ["settings", HOST_SELECTORS.settings],
    ]) {
      if (document.querySelector(selector)) return surface;
    }
    if (document.querySelector(HOST_SELECTORS.home)) return "home";
    if (document.querySelector(HOST_SELECTORS.chat)) return "chat";
    return "other";
  };

  const applyTokens = (root, shell) => {
    const colors = theme.colors?.[shell];
    const tokenMap = {
      background: "--skin-bg",
      panel: "--skin-panel",
      panelAlt: "--skin-panel-alt",
      accent: "--skin-accent",
      accentAlt: "--skin-accent-alt",
      text: "--skin-text",
      muted: "--skin-muted",
      line: "--skin-line",
    };
    for (const [key, token] of Object.entries(tokenMap)) {
      if (typeof colors?.[key] === "string") root.style.setProperty(token, colors[key]);
    }
    const background = assets.backgrounds[shell] || assets.background;
    if (background) {
      root.style.setProperty("--skin-background-image", `url(${JSON.stringify(background)})`);
    }
  };

  const ensure = () => {
    if (stopped) return;
    if (hasForeignOwner() || window[STATE_KEY] !== runtimeState) { quiesce(); return; }
    if (window[DISABLED_KEY]) return;
    const root = document.documentElement;
    const shellMain = document.querySelector(HOST_SELECTORS.shellMain);
    if (!root || !document.body || !shellMain) return;
    const shell = detectShell();
    const surface = detectSurface();
    root.classList.add("codex-dream-skin");
    root.dataset.dreamShell = shell;
    root.dataset.dreamSurface = surface;
    applyTokens(root, shell);

    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || root).appendChild(style);
    }
    ownedStyle = style;
    if (style.dataset.skinVersion !== VERSION) {
      style.textContent = themeCssText ? `${cssText}\n${themeCssText}` : cssText;
      style.dataset.skinVersion = VERSION;
    }

    document.getElementById(CHROME_ID)?.remove();
  };

  const schedule = () => {
    if (stopped) return;
    if (hasForeignOwner() || window[STATE_KEY] !== runtimeState) { quiesce(); return; }
    if (scheduled !== null) clearTimeout(scheduled);
    scheduled = setTimeout(() => { scheduled = null; ensure(); }, 160);
  };

  // 仅停止当前闭包；其它应用接管后保留共享表面，账本留待安全cleanup。
  const quiesce = () => {
    stopped = true;
    if (scheduled !== null) clearTimeout(scheduled);
    scheduled = null;
    observer?.disconnect();
    if (timer !== null) clearInterval(timer);
    timer = null;
    colorScheme?.removeEventListener?.("change", schedule);
    if (runtimeState) runtimeState.quiesced = true;
    if (window[STATE_KEY] === runtimeState) {
      window[DISABLED_KEY] = true;
      if (ownedStyle && document.getElementById(STYLE_ID) === ownedStyle) ownedStyle.remove();
      if (ownedChrome && document.getElementById(CHROME_ID) === ownedChrome) ownedChrome.remove();
    }
    for (const url of objectUrls) URL.revokeObjectURL(url);
    objectUrls.length = 0;
    return true;
  };

  const cleanup = () => {
    const ownsCurrent = window[STATE_KEY] === runtimeState;
    quiesce();
    if (!ownsCurrent || hasForeignOwner()) return true;
    const root = document.documentElement;
    root?.classList.remove("codex-dream-skin");
    if (root) {
      delete root.dataset.dreamShell;
      delete root.dataset.dreamSurface;
    }
    for (const token of ["--skin-bg", "--skin-panel", "--skin-panel-alt", "--skin-accent", "--skin-accent-alt", "--skin-text", "--skin-muted", "--skin-line", "--skin-background-image", "--dream-skin-art"]) {
      root?.style.removeProperty(token);
    }
    if (window[STATE_KEY] === runtimeState) delete window[STATE_KEY];
    return true;
  };

  observer = new MutationObserver((mutations) => {
    if (mutations.some((mutation) => mutation.type === "childList"
      || mutation.target === document.documentElement
      || mutation.target === document.body)) schedule();
  });
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "data-theme", "data-appearance"],
    childList: true,
    subtree: true,
  });
  colorScheme?.addEventListener?.("change", schedule);
  timer = setInterval(ensure, 5000);
  runtimeState = { version: VERSION, observer, timer, cleanup, quiesce, ensure, quiesced: false };
  window[STATE_KEY] = runtimeState;
  ensure();
})(
  __DREAM_SKIN_CSS_JSON__,
  __DREAM_SKIN_THEME_CSS_JSON__,
  __DREAM_SKIN_ART_JSON__,
  __DREAM_SKIN_THEME_ASSETS_JSON__,
  __DREAM_SKIN_THEME_JSON__
);

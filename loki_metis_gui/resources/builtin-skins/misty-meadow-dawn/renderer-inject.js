((cssText, artDataUrl, profileDataUrl, galleryDataUrl, themeConfig) => {
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
  const urls = [];
  let observer = null;
  let timer = null;
  let scheduled = null;

  const previous = window[STATE_KEY];
  if (previous?.cleanup) {
    try { previous.cleanup(); } catch {
      previous.observer?.disconnect();
      if (previous.timer) clearInterval(previous.timer);
    }
  }
  window[DISABLED_KEY] = false;

  const objectUrl = (dataUrl) => {
    if (!dataUrl) return "";
    const comma = dataUrl.indexOf(",");
    const mime = /^data:([^;,]+)/.exec(dataUrl)?.[1] || "application/octet-stream";
    const binary = atob(dataUrl.slice(comma + 1));
    const bytes = Uint8Array.from(binary, (character) => character.charCodeAt(0));
    const url = URL.createObjectURL(new Blob([bytes], { type: mime }));
    urls.push(url);
    return url;
  };

  const assets = {
    art: objectUrl(artDataUrl),
    profile: objectUrl(profileDataUrl),
    gallery: objectUrl(galleryDataUrl),
  };

  const detectShell = () => {
    const marker = [
      document.documentElement?.getAttribute("data-theme"),
      document.documentElement?.getAttribute("data-appearance"),
      document.body?.getAttribute("data-theme"),
      document.documentElement?.className,
      document.body?.className,
    ].filter(Boolean).join(" ").toLowerCase();
    if (/\b(dark|theme-dark|appearance-dark)\b/.test(marker)) return "dark";
    if (/\b(light|theme-light|appearance-light)\b/.test(marker)) return "light";
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
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
    const headerLabel = document.querySelector("[data-testid='app-shell-header-context-menu-surface']")?.textContent?.trim().toLowerCase();
    const headerSurfaces = new Map([
      ["pull request", "pull-requests"], ["pull requests", "pull-requests"],
      ["站点", "sites"], ["sites", "sites"],
      ["已安排", "automations"], ["automations", "automations"], ["scheduled", "automations"],
      ["插件", "plugins"], ["plugins", "plugins"],
      ["设置", "settings"], ["settings", "settings"],
    ]);
    if (headerSurfaces.has(headerLabel)) return headerSurfaces.get(headerLabel);
    if (document.querySelector(HOST_SELECTORS.home)) return "home";
    if (document.querySelector(HOST_SELECTORS.chat)) return "chat";
    return "other";
  };

  const applyTokens = (root, shell) => {
    const palettes = theme.colors || {};
    const colors = palettes[shell] || palettes;
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
      if (typeof colors[key] === "string" && colors[key]) root.style.setProperty(token, colors[key]);
    }
    if (assets.art) root.style.setProperty("--dream-skin-art", `url(${JSON.stringify(assets.art)})`);
  };

  const addText = (parent, tag, value) => {
    if (typeof value !== "string" || !value) return;
    const node = document.createElement(tag);
    node.textContent = value;
    parent.appendChild(node);
  };

  const renderPanels = (chrome) => {
    chrome.replaceChildren();
    const panels = theme.extensions?.photoPanels;
    if (!Array.isArray(panels)) return;
    for (const panel of panels.filter((item) => item?.enabled).slice(0, 2)) {
      const imageUrl = assets[panel.assetSlot];
      if (!imageUrl) continue;
      const figure = document.createElement("figure");
      figure.className = "skin-photo-panel";
      figure.dataset.panelId = String(panel.id || panel.assetSlot || "panel");
      figure.dataset.anchor = panel.anchor || "left-top";
      figure.dataset.fit = panel.fit === "cover" ? "cover" : "contain";
      figure.dataset.visibleOn = Array.isArray(panel.visibleOn) ? panel.visibleOn.join(" ") : "chat";
      figure.dataset.minViewportWidth = String(panel.minViewportWidth || 1280);
      figure.style.setProperty("--panel-x", `${panel.offset?.x ?? 24}px`);
      figure.style.setProperty("--panel-y", `${panel.offset?.y ?? 24}px`);
      figure.style.width = `${panel.size?.width ?? 304}px`;
      figure.style.height = `${panel.size?.height ?? 360}px`;

      const image = document.createElement("img");
      image.src = imageUrl;
      image.alt = "";
      figure.appendChild(image);
      if (panel.title || panel.caption) {
        const caption = document.createElement("figcaption");
        addText(caption, "b", panel.title);
        addText(caption, "span", panel.caption);
        figure.appendChild(caption);
      }
      chrome.appendChild(figure);
    }
  };

  const ensure = () => {
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
    if (style.dataset.skinVersion !== VERSION) {
      style.textContent = cssText;
      style.dataset.skinVersion = VERSION;
    }

    let chrome = document.getElementById(CHROME_ID);
    if (!chrome) {
      chrome = document.createElement("div");
      chrome.id = CHROME_ID;
      chrome.setAttribute("aria-hidden", "true");
      document.body.appendChild(chrome);
      renderPanels(chrome);
    }
    const rect = shellMain.getBoundingClientRect();
    Object.assign(chrome.style, {
      left: `${Math.round(rect.left)}px`,
      top: `${Math.round(rect.top)}px`,
      width: `${Math.round(rect.width)}px`,
      height: `${Math.round(rect.height)}px`,
    });
    chrome.dataset.surface = surface;
    chrome.querySelectorAll(".skin-photo-panel").forEach((panel) => {
      panel.hidden = window.innerWidth < Number(panel.dataset.minViewportWidth || 1280);
    });
  };

  const schedule = () => {
    if (scheduled !== null) clearTimeout(scheduled);
    scheduled = setTimeout(() => { scheduled = null; ensure(); }, 160);
  };

  const cleanup = () => {
    window[DISABLED_KEY] = true;
    if (scheduled !== null) clearTimeout(scheduled);
    observer?.disconnect();
    if (timer) clearInterval(timer);
    window.removeEventListener("resize", schedule);
    const root = document.documentElement;
    root?.classList.remove("codex-dream-skin");
    if (root) {
      delete root.dataset.dreamShell;
      delete root.dataset.dreamSurface;
    }
    [
      "--skin-bg", "--skin-panel", "--skin-panel-alt", "--skin-accent",
      "--skin-accent-alt", "--skin-text", "--skin-muted", "--skin-line",
      "--dream-skin-art",
    ].forEach((name) => root?.style.removeProperty(name));
    document.getElementById(STYLE_ID)?.remove();
    document.getElementById(CHROME_ID)?.remove();
    urls.forEach((url) => URL.revokeObjectURL(url));
    if (window[STATE_KEY]?.cleanup === cleanup) delete window[STATE_KEY];
    return true;
  };

  observer = new MutationObserver(schedule);
  observer.observe(document.documentElement, { childList: true, subtree: true });
  timer = setInterval(ensure, 5000);
  window.addEventListener("resize", schedule, { passive: true });
  window[STATE_KEY] = {
    cleanup,
    ensure,
    observer,
    timer,
    version: VERSION,
    themeId: theme.id || "custom",
    hostSelectors: HOST_SELECTORS,
  };
  ensure();
  return { installed: true, version: VERSION, themeId: theme.id || "custom" };
})(__DREAM_SKIN_CSS_JSON__, __DREAM_SKIN_ART_JSON__, __DREAM_SKIN_AVATAR_JSON__, __DREAM_SKIN_FRIENDS_JSON__, __DREAM_SKIN_THEME_JSON__)

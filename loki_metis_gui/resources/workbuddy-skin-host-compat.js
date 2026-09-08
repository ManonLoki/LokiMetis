(() => {
  "use strict";

  const STATE_KEY = "__BIFANG_CODEX_SKIN_COMPAT__";
  const VERSION = "5";
  const HOST = "workBuddy";
  const VERSION_ATTRIBUTE = "data-loki-metis-workbuddy-skin-compat";
  const STYLE_ID = "loki-metis-workbuddy-skin-compat-style";
  const DEBOUNCE_MILLISECONDS = 160;
  const HOST_SELECTORS = Object.freeze({
    verifiedBody:
      'body[data-application-name="workbuddy"][data-electron-desktop="true"][data-product-name="WorkBuddy"]',
    appMount: "#root",
    shell: ".teams-container",
    sidebarFrames:
      '[data-view-id="sidebar"], [data-view-id="sidebar"] .conversation-sidebar',
    sidebarSurfaces: '[data-view-id="sidebar"] .conversation-list',
    mainFrames:
      '[data-view-id="main-content"], [data-view-id="main-content"] .teams-main-content',
    mainSurfaces: '[data-view-id="main-content"] .main-content',
    mainContent:
      '[data-view-id="main-content"] :is(.wb-home-page, .chat-container, .wb-cb-chat)',
    headers: "#workbuddy-menubar-container, #workbuddy-window-controls-container",
    details: ".detail-panel-container, .sidebar-next",
    chatRoots: '[data-view-id="main-content"] .wb-cb-chat',
    composerSeed: '[data-cb-chat-input-toolbar-selector="true"]',
    composerConfirmation: '[data-cb-chat-input-toolbar-right="true"]',
    composerEditor: '[role="textbox"][contenteditable="true"]',
  });
  const HOST_SELECTOR = HOST_SELECTORS.verifiedBody;
  const COMPATIBILITY_SELECTOR = [
    HOST_SELECTORS.shell,
    HOST_SELECTORS.sidebarFrames,
    HOST_SELECTORS.sidebarSurfaces,
    HOST_SELECTORS.mainFrames,
    HOST_SELECTORS.mainSurfaces,
    HOST_SELECTORS.mainContent,
    HOST_SELECTORS.headers,
    HOST_SELECTORS.details,
    HOST_SELECTORS.composerSeed,
    HOST_SELECTORS.composerConfirmation,
    HOST_SELECTORS.composerEditor,
  ].join(", ");
  const RULES = {
    shell: "workbuddy-shell-surface",
    sidebar: "workbuddy-sidebar-surface",
    main: "workbuddy-main-surface",
    header: "workbuddy-application-header",
    composer: "workbuddy-composer-surface",
    detail: "workbuddy-detail-surface",
  };
  const STYLE_TEXT = `
html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-shell {
  color: var(--skin-text, CanvasText) !important;
  background-color: var(--skin-bg, Canvas) !important;
  background-image:
    linear-gradient(
      90deg,
      color-mix(in srgb, var(--skin-bg, Canvas) 48%, transparent),
      color-mix(in srgb, var(--skin-bg, Canvas) 16%, transparent) 58%,
      color-mix(in srgb, var(--skin-bg, Canvas) 52%, transparent)
    ),
    var(--skin-background-image, var(--dream-skin-art, none)) !important;
  background-position:
    0 0,
    var(--skin-background-position-x, 50%) var(--skin-background-position-y, 50%) !important;
  background-repeat: no-repeat !important;
  background-size: auto, var(--skin-background-size, cover) !important;
}

html.codex-dream-skin ${HOST_SELECTOR} :is(
  .loki-metis-workbuddy-sidebar-frame,
  .loki-metis-workbuddy-main-frame,
  .loki-metis-workbuddy-content
) {
  color: var(--skin-text, CanvasText) !important;
  background: transparent !important;
}

html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-sidebar-surface {
  color: var(--skin-text, CanvasText) !important;
  border-color: var(--skin-line, GrayText) !important;
  background: color-mix(
    in srgb,
    var(--skin-panel, Canvas) var(--skin-sidebar-opacity, 92%),
    transparent
  ) !important;
  backdrop-filter: blur(var(--skin-blur, 16px));
}

html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-main-surface {
  color: var(--skin-text, CanvasText) !important;
  background: color-mix(
    in srgb,
    var(--skin-bg, Canvas) var(--skin-main-opacity, 78%),
    transparent
  ) !important;
}

html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-header {
  color: var(--skin-text, CanvasText) !important;
  border-color: var(--skin-line, GrayText) !important;
  background: color-mix(
    in srgb,
    var(--skin-panel, Canvas) var(--skin-header-opacity, 88%),
    transparent
  ) !important;
  backdrop-filter: blur(var(--skin-blur, 16px));
}

html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-composer {
  --cb-content-background: transparent !important;
  --cb-content-border-color: var(--skin-line, GrayText) !important;
  --cb-main-area-background: transparent !important;
  --cb-main-area-border-color: var(--skin-line, GrayText) !important;
  --cb-main-area-box-shadow: none !important;
  color: var(--skin-text, CanvasText) !important;
  border: var(--skin-border-width, 1px) solid var(--skin-line, GrayText) !important;
  border-radius: var(--skin-radius, 12px) !important;
  background: color-mix(
    in srgb,
    var(--skin-panel, Canvas) var(--skin-composer-opacity, 94%),
    transparent
  ) !important;
  box-shadow: 0 16px 42px color-mix(
    in srgb,
    var(--skin-bg, Canvas) var(--skin-shadow-opacity, 42%),
    transparent
  ) !important;
  backdrop-filter: blur(var(--skin-blur, 16px));
}

html.codex-dream-skin ${HOST_SELECTOR} .loki-metis-workbuddy-detail-surface {
  color: var(--skin-text, CanvasText) !important;
  border-color: var(--skin-line, GrayText) !important;
  background: color-mix(
    in srgb,
    var(--skin-panel, Canvas) var(--skin-card-opacity, 90%),
    transparent
  ) !important;
  backdrop-filter: blur(var(--skin-blur, 16px));
}
`;

  const snapshot = (appliedRules, skippedRules) => ({
    version: VERSION,
    appliedRules: [...appliedRules].sort(),
    skippedRules: [...skippedRules].sort(),
  });
  const isWorkBuddyHost = () =>
    Boolean(
      document.querySelector(HOST_SELECTOR) &&
      document.title === "WorkBuddy" &&
      document.querySelector(HOST_SELECTORS.appMount),
    );

  if (!isWorkBuddyHost()) {
    return snapshot([], ["workbuddy-host-marker"]);
  }

  const existing = window[STATE_KEY];
  if (
    existing?.host === HOST &&
    existing?.version === VERSION &&
    typeof existing.ensure === "function"
  ) {
    return existing.ensure();
  }
  if (typeof existing?.cleanup === "function") {
    try {
      existing.cleanup();
    } catch {
      // 旧适配器清理失败不应阻止新版本安装。
    }
  }

  const ownedAliases = [];
  let observer = null;
  let debounceTimer = null;

  const state = {
    host: HOST,
    version: VERSION,
    observer: null,
    styleNode: null,
    ownedAliases,
    appliedRules: [],
    skippedRules: [],
    ensure: null,
    cleanup: null,
  };

  const ownsAlias = (node, className) =>
    ownedAliases.some((entry) => entry.node === node && entry.className === className);

  const pruneOwnedAliases = () => {
    let removed = false;
    for (let index = ownedAliases.length - 1; index >= 0; index -= 1) {
      const entry = ownedAliases[index];
      if (entry.node.isConnected) continue;
      entry.node.classList.remove(entry.className);
      ownedAliases.splice(index, 1);
      removed = true;
    }
    return removed;
  };

  const addAlias = (node, className, ruleId, appliedRules) => {
    if (node.classList.contains(className)) {
      if (ownsAlias(node, className)) appliedRules.add(ruleId);
      return;
    }
    node.classList.add(className);
    if (!ownsAlias(node, className)) ownedAliases.push({ node, className, ruleId });
    appliedRules.add(ruleId);
  };

  const releaseOwnedAliasesOutside = (ruleId, desiredNodes) => {
    for (let index = ownedAliases.length - 1; index >= 0; index -= 1) {
      const entry = ownedAliases[index];
      if (entry.ruleId !== ruleId || desiredNodes.has(entry.node)) continue;
      entry.node.classList.remove(entry.className);
      ownedAliases.splice(index, 1);
    }
  };

  const addAll = (selector, className, ruleId, appliedRules) => {
    const nodes = [...document.querySelectorAll(selector)];
    for (const node of nodes) addAlias(node, className, ruleId, appliedRules);
    return nodes.length;
  };

  const ensureStyle = () => {
    let style = document.getElementById(STYLE_ID);
    if (!style) {
      style = document.createElement("style");
      style.id = STYLE_ID;
      (document.head || document.documentElement).appendChild(style);
    }
    style.textContent = STYLE_TEXT;
    style.dataset.compatibilityVersion = VERSION;
    state.styleNode = style;
  };

  const reconcileComposer = (appliedRules, skippedRules) => {
    const host = document.querySelector(HOST_SELECTOR);
    const chats = host ? [...host.querySelectorAll(HOST_SELECTORS.chatRoots)] : [];
    const seeds = chats.flatMap((chat) => [
      ...chat.querySelectorAll(HOST_SELECTORS.composerSeed),
    ]);
    const candidates = [
      ...new Set(
        seeds
          .map((seed) => seed.closest("section"))
          .filter(
            (section) =>
              section &&
              section.querySelector(HOST_SELECTORS.composerConfirmation) &&
              section.querySelectorAll(HOST_SELECTORS.composerEditor).length === 1,
          ),
      ),
    ];
    const desiredNodes = new Set(candidates.length === 1 ? candidates : []);
    releaseOwnedAliasesOutside(RULES.composer, desiredNodes);
    if (candidates.length === 1) {
      addAlias(
        candidates[0],
        "loki-metis-workbuddy-composer",
        RULES.composer,
        appliedRules,
      );
    } else if (chats.length > 0) {
      skippedRules.add(RULES.composer);
    }
  };

  const ensure = () => {
    pruneOwnedAliases();
    const appliedRules = new Set();
    const skippedRules = new Set();
    if (!isWorkBuddyHost()) {
      skippedRules.add("workbuddy-host-marker");
      return snapshot(appliedRules, skippedRules);
    }
    ensureStyle();

    if (
      addAll(
        HOST_SELECTORS.shell,
        "loki-metis-workbuddy-shell",
        RULES.shell,
        appliedRules,
      ) === 0
    ) {
      skippedRules.add(RULES.shell);
    }

    const sidebarFrames = addAll(
      HOST_SELECTORS.sidebarFrames,
      "loki-metis-workbuddy-sidebar-frame",
      RULES.sidebar,
      appliedRules,
    );
    const sidebarSurfaces = addAll(
      HOST_SELECTORS.sidebarSurfaces,
      "loki-metis-workbuddy-sidebar-surface",
      RULES.sidebar,
      appliedRules,
    );
    if (sidebarFrames === 0 || sidebarSurfaces === 0) skippedRules.add(RULES.sidebar);

    const mainFrames = addAll(
      HOST_SELECTORS.mainFrames,
      "loki-metis-workbuddy-main-frame",
      RULES.main,
      appliedRules,
    );
    const mainSurfaces = addAll(
      HOST_SELECTORS.mainSurfaces,
      "loki-metis-workbuddy-main-surface",
      RULES.main,
      appliedRules,
    );
    addAll(
      HOST_SELECTORS.mainContent,
      "loki-metis-workbuddy-content",
      RULES.main,
      appliedRules,
    );
    if (mainFrames === 0 || mainSurfaces === 0) skippedRules.add(RULES.main);

    if (
      addAll(
        HOST_SELECTORS.headers,
        "loki-metis-workbuddy-header",
        RULES.header,
        appliedRules,
      ) === 0
    ) {
      skippedRules.add(RULES.header);
    }

    addAll(
      HOST_SELECTORS.details,
      "loki-metis-workbuddy-detail-surface",
      RULES.detail,
      appliedRules,
    );
    reconcileComposer(appliedRules, skippedRules);

    state.appliedRules = [...appliedRules].sort();
    state.skippedRules = [...skippedRules].sort();
    document.documentElement.setAttribute(VERSION_ATTRIBUTE, VERSION);
    return snapshot(appliedRules, skippedRules);
  };

  const cleanup = () => {
    if (debounceTimer !== null) window.clearTimeout(debounceTimer);
    debounceTimer = null;
    observer?.disconnect();
    observer = null;
    state.observer = null;
    for (const { node, className } of ownedAliases) node.classList.remove(className);
    ownedAliases.length = 0;
    if (state.styleNode?.isConnected) state.styleNode.remove();
    state.styleNode = null;
    if (document.documentElement.getAttribute(VERSION_ATTRIBUTE) === VERSION) {
      document.documentElement.removeAttribute(VERSION_ATTRIBUTE);
    }
    if (window[STATE_KEY] === state) delete window[STATE_KEY];
    return true;
  };

  const schedule = () => {
    if (debounceTimer !== null) window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(() => {
      debounceTimer = null;
      if (window[STATE_KEY] === state) ensure();
    }, DEBOUNCE_MILLISECONDS);
  };
  const containsCompatibilityCandidate = (node) =>
    node?.nodeType === 1 &&
    (node.matches(COMPATIBILITY_SELECTOR) ||
      Boolean(node.querySelector(COMPATIBILITY_SELECTOR)));

  state.ensure = ensure;
  state.cleanup = cleanup;
  window[STATE_KEY] = state;
  observer = new MutationObserver((records) => {
    const releasedAliases = pruneOwnedAliases();
    const relevantChange = records.some((record) => {
      if (record.type === "attributes")
        return containsCompatibilityCandidate(record.target);
      return (
        [...record.addedNodes].some(containsCompatibilityCandidate) ||
        [...record.removedNodes].some(containsCompatibilityCandidate) ||
        [...record.removedNodes].includes(state.styleNode)
      );
    });
    if (releasedAliases || relevantChange) schedule();
  });
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: [
      "class",
      "data-application-name",
      "data-electron-desktop",
      "data-product-name",
      "data-view-id",
      "data-cb-chat-input-toolbar-selector",
      "data-cb-chat-input-toolbar-right",
      "role",
      "contenteditable",
    ],
    childList: true,
    subtree: true,
  });
  state.observer = observer;
  return ensure();
})();

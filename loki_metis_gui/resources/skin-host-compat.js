(() => {
  "use strict";

  const STATE_KEY = "__BIFANG_CODEX_SKIN_COMPAT__";
  const VERSION = "4";
  const VERSION_ATTRIBUTE = "data-bifang-codex-skin-compat";
  const DEBOUNCE_MILLISECONDS = 160;
  const COMPATIBILITY_SELECTOR = [
    "main[data-app-shell-main-surface]",
    "#root main",
    "main > header[data-app-shell-application-menu-bar]",
    "[data-composer-surface-variant]",
    "[data-codex-composer]",
  ].join(", ");
  const RULES = {
    main: "legacy-main-surface",
    header: "legacy-app-header-tint",
    composer: "legacy-composer-surface-chrome",
  };

  const existing = window[STATE_KEY];
  if (existing?.version === VERSION && typeof existing.ensure === "function") {
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

  const snapshot = (appliedRules, skippedRules) => ({
    version: VERSION,
    appliedRules: [...appliedRules].sort(),
    skippedRules: [...skippedRules].sort(),
  });

  const state = {
    version: VERSION,
    observer: null,
    ownedAliases,
    appliedRules: [],
    skippedRules: [],
    ensure: null,
    cleanup: null,
  };

  const ownsAlias = (node, className) => ownedAliases.some(
    (entry) => entry.node === node && entry.className === className,
  );

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
    if (!ownsAlias(node, className)) {
      ownedAliases.push({ node, className, ruleId });
    }
    appliedRules.add(ruleId);
  };

  const applyUniqueRule = (
    nativeSelector,
    candidates,
    className,
    ruleId,
    appliedRules,
    skippedRules,
    unresolvedSelector = null,
  ) => {
    const nativeNode = document.querySelector(nativeSelector);
    if (nativeNode) {
      if (ownsAlias(nativeNode, className)) appliedRules.add(ruleId);
      return;
    }
    if (candidates.length === 1) {
      addAlias(candidates[0], className, ruleId, appliedRules);
    } else if (candidates.length > 1) {
      skippedRules.add(ruleId);
    } else if (unresolvedSelector && document.querySelector(unresolvedSelector)) {
      skippedRules.add(ruleId);
    }
  };

  const ensure = () => {
    pruneOwnedAliases();
    const appliedRules = new Set();
    const skippedRules = new Set();

    const stableMainCandidates = [...document.querySelectorAll(
      "main[data-app-shell-main-surface]",
    )];
    const legacyMainCandidates = [...document.querySelectorAll("#root main")];

    applyUniqueRule(
      "main.main-surface",
      stableMainCandidates.length > 0 ? stableMainCandidates : legacyMainCandidates,
      "main-surface",
      RULES.main,
      appliedRules,
      skippedRules,
      "main",
    );

    applyUniqueRule(
      "main > header.app-header-tint",
      [...document.querySelectorAll(
        "main > header[data-app-shell-application-menu-bar]",
      )],
      "app-header-tint",
      RULES.header,
      appliedRules,
      skippedRules,
      "main > header",
    );

    const nativeComposerSurface = document.querySelector(
      "[data-composer-surface-variant].composer-surface-chrome",
    );
    if (nativeComposerSurface) {
      if (ownsAlias(nativeComposerSurface, "composer-surface-chrome")) {
        appliedRules.add(RULES.composer);
      }
    } else {
      const composers = [...document.querySelectorAll("[data-codex-composer]")];
      const surfaces = [...new Set(composers.map((composer) => (
        composer.closest("[data-composer-surface-variant]")
      )).filter(Boolean))];
      if (composers.length > 0 && surfaces.length === 1) {
        addAlias(
          surfaces[0],
          "composer-surface-chrome",
          RULES.composer,
          appliedRules,
        );
      } else if (composers.length > 0) {
        skippedRules.add(RULES.composer);
      }
    }

    state.appliedRules = [...appliedRules].sort();
    state.skippedRules = [...skippedRules].sort();
    document.documentElement?.setAttribute(VERSION_ATTRIBUTE, VERSION);
    return snapshot(appliedRules, skippedRules);
  };

  const cleanup = () => {
    if (debounceTimer !== null) window.clearTimeout(debounceTimer);
    debounceTimer = null;
    observer?.disconnect();
    observer = null;
    state.observer = null;
    for (const { node, className } of ownedAliases) {
      node.classList.remove(className);
    }
    ownedAliases.length = 0;
    if (document.documentElement?.getAttribute(VERSION_ATTRIBUTE) === VERSION) {
      document.documentElement.removeAttribute(VERSION_ATTRIBUTE);
    }
    if (window[STATE_KEY] === state) delete window[STATE_KEY];
    return true;
  };

  state.ensure = ensure;
  state.cleanup = cleanup;
  window[STATE_KEY] = state;
  const isCompatibilityCandidate = (target) => target?.nodeType === 1 && (
    target.matches("main[data-app-shell-main-surface], #root main")
    || target.matches("main > header[data-app-shell-application-menu-bar]")
    || target.matches("[data-composer-surface-variant]")
  );
  const containsCompatibilityCandidate = (node) => node?.nodeType === 1 && (
    node.matches(COMPATIBILITY_SELECTOR)
    || Boolean(node.querySelector(COMPATIBILITY_SELECTOR))
  );
  observer = new MutationObserver((records) => {
    const compatibilityClassChanged = records.some((record) => (
      record.type === "attributes" && isCompatibilityCandidate(record.target)
    ));
    if (compatibilityClassChanged) {
      if (window[STATE_KEY] === state) ensure();
      return;
    }
    const releasedAliases = pruneOwnedAliases();
    const compatibilityTreeChanged = records.some((record) => (
      record.type === "childList" && (
        [...record.addedNodes].some(containsCompatibilityCandidate)
        || [...record.removedNodes].some(containsCompatibilityCandidate)
      )
    ));
    if (!releasedAliases && !compatibilityTreeChanged) return;
    if (debounceTimer !== null) window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(() => {
      debounceTimer = null;
      if (window[STATE_KEY] === state) ensure();
    }, DEBOUNCE_MILLISECONDS);
  });
  observer.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["class", "data-app-shell-main-surface"],
    childList: true,
    subtree: true,
  });
  state.observer = observer;
  return ensure();
})()

const DEFAULT_CDP_PORT: u16 = 9341;
const CODEX_PAGE_READY_TIMEOUT: Duration = Duration::from_secs(15);
const EXISTING_CODEX_PAGE_READY_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_PAGE_POLL_INTERVAL: Duration = Duration::from_millis(350);
const CODEX_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(750);
const CODEX_LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
const CODEX_FORCE_CLOSE_TIMEOUT: Duration = Duration::from_secs(15);
const CDP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CDP_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CDP_TARGET_SETTLE_DELAY: Duration = Duration::from_millis(200);
const ACCOUNT_PROFILE_READY_TIMEOUT: Duration = Duration::from_secs(2);
const ACCOUNT_PROFILE_RETRY_INTERVAL: Duration = Duration::from_millis(150);
const MAX_ACCOUNT_AVATAR_BYTES: usize = 256 * 1024;
const MAX_RUNTIME_STYLE_PROBE_CHARS: usize = 256 * 1024;
const SKIN_WATCH_INTERVAL: Duration = Duration::from_secs(5);
const CDP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
const WATCH_STOP_TIMEOUT: Duration = Duration::from_secs(12);
const SKIN_VERSION: &str = "1.7.0";
const HOST_COMPATIBILITY_VERSION: &str = "4";
const HOST_COMPATIBILITY_SCRIPT: &str = include_str!("../../../resources/skin-host-compat.js");
const THEME_RUNTIME_CSS: &str = include_str!("../../../resources/theme-runtime/theme.css");
const THEME_RUNTIME_SCRIPT: &str = include_str!("../../../resources/theme-runtime/renderer-inject.js");
const THEME_TEMPLATE_MANIFEST: &str = include_str!("../../../resources/theme-template/theme.json");
const THEME_TEMPLATE_CSS: &str = include_str!("../../../resources/theme-template/theme.css");
const THEME_TEMPLATE_IMAGES: [(&str, &[u8]); 2] = [
    (
        "preview.png",
        include_bytes!("../../../resources/theme-template/preview.png"),
    ),
    (
        "background.png",
        include_bytes!("../../../resources/theme-template/background.png"),
    ),
];
const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_IMPORT_ENTRIES: usize = 256;
const MAX_IMPORT_BATCH_BYTES: u64 = MAX_IMPORT_BYTES * MAX_IMPORT_BATCH_FILES as u64;
static IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
const LEGACY_REQUIRED_FILES: [&str; 6] = [
    "dream-skin.css",
    "renderer-inject.js",
    "theme.json",
    "qq2007-sky.png",
    "avatar.png",
    "qqshow.jpg",
];
const LEGACY_RUNTIME_FILES: [&str; 5] = [
    "dream-skin.css",
    "renderer-inject.js",
    "qq2007-sky.png",
    "avatar.png",
    "qqshow.jpg",
];

const PROBE_SCRIPT: &str = r#"(() => {
  const mainCandidates = document.querySelectorAll('#root main');
  const stableMainCandidates = document.querySelectorAll('main[data-app-shell-main-surface]');
  const composer = document.querySelector('[data-codex-composer]');
  const markers = {
    shell: Boolean(document.querySelector('main.main-surface')) ||
      stableMainCandidates.length === 1 || mainCandidates.length === 1,
    header: Boolean(document.querySelector('main > header[data-app-shell-application-menu-bar]')),
    sidebar: Boolean(document.querySelector('aside.app-shell-left-panel')),
    composer: Boolean(document.querySelector('.composer-surface-chrome')) ||
      Boolean(composer?.closest('[data-composer-surface-variant]')),
    main: Boolean(document.querySelector('[role="main"]')),
  };
  const primaryPage = location.protocol === 'app:' && location.hostname === '-' &&
    location.pathname === '/index.html' && location.search === '';
  const codexDocument = ['Codex', 'ChatGPT'].includes(document.title) &&
    Boolean(document.querySelector('#root'));
  return {
    url: location.href,
    codex: primaryPage && codexDocument && Object.values(markers).some(Boolean),
  };
})()"#;

const ACCOUNT_PROFILE_PROBE_SCRIPT: &str = r#"(() => {
  const normalize = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();
  const runtime = window.__CODEX_DREAM_SKIN_STATE__;
  const marker = runtime?.skin;
  const styleText = document.getElementById('codex-dream-skin-style')?.textContent || '';
  const activeSkin = marker && typeof marker === 'object'
    ? {
        version: typeof runtime.version === 'string' ? runtime.version : null,
        source: marker.source === 'builtin' || marker.source === 'user' ? marker.source : null,
        id: typeof marker.id === 'string' ? marker.id : null,
        legacyThemeId: null,
        legacyStyleText: null,
      }
    : {
        version: typeof runtime?.version === 'string' ? runtime.version : null,
        source: null,
        id: null,
        legacyThemeId: typeof runtime?.themeId === 'string' ? runtime.themeId : null,
        legacyStyleText: !runtime?.themeId && styleText.length > 0 && styleText.length <= 262144
          ? styleText : null,
      };
  const buttons = [...document.querySelectorAll('button[aria-haspopup="menu"]')];
  const semanticLabels = new Set(['打开个人资料菜单', 'Open profile menu']);
  const semantic = buttons.filter((button) => semanticLabels.has(normalize(button.getAttribute('aria-label'))));
  const candidates = semantic.length > 0 ? semantic : buttons.filter((button) => {
    const label = normalize(button.innerText);
    return button.querySelector('img') && label.length > 0 && label.length <= 80 &&
      !/[\u0000-\u001f\u007f]/.test(label);
  });
  if (candidates.length !== 1) return { label: null, avatarDataUrl: null, activeSkin };
  const button = candidates[0];
  const label = normalize(button.innerText);
  const image = button.querySelector('img');
  const source = String(image?.currentSrc || image?.src || '');
  const avatarDataUrl = /^data:image\/(?:png|jpeg|webp);base64,[A-Za-z0-9+/=]+$/.test(source)
    ? source : null;
  return {
    label: label.length > 0 && label.length <= 80 && !/[\u0000-\u001f\u007f]/.test(label)
      ? label : null,
    avatarDataUrl,
    activeSkin,
  };
})()"#;

const APPEARANCE_PROBE_SCRIPT: &str = r#"(async () => {
  const root = document.documentElement;
  const marker = [root?.className, root?.dataset?.theme, root?.dataset?.appearance,
    document.body?.className, document.body?.dataset?.theme].filter(Boolean).join(' ').toLowerCase();
  const effectiveMode = /\belectron-dark\b|\b(?:theme|appearance)-dark\b|\bdark\b/.test(marker)
    ? 'dark'
    : (/\belectron-light\b|\b(?:theme|appearance)-light\b|\blight\b/.test(marker)
      ? 'light'
      : (matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));
  const computed = getComputedStyle(root);
  const effective = {
    accent: computed.getPropertyValue('--codex-base-accent').trim() || null,
    contrast: computed.getPropertyValue('--codex-base-contrast').trim() || null,
    surface: computed.getPropertyValue('--codex-base-surface').trim() || null,
    ink: computed.getPropertyValue('--codex-base-ink').trim() || null,
  };
  let appearance = null;
  let appearanceReadable = false;
  try {
    const entry = [...document.scripts].map((script) => script.src).find(Boolean);
    if (!entry) throw new Error('entry-missing');
    const entryUrl = new URL(entry, location.href);
    if (entryUrl.protocol !== location.protocol || entryUrl.hostname !== location.hostname) throw new Error('entry-origin');
    const entryController = new AbortController();
    const entryTimer = setTimeout(() => entryController.abort(), 2000);
    const entryText = await fetch(entryUrl.href, { signal: entryController.signal }).then((response) => {
      if (!response.ok) throw new Error('entry-fetch');
      const length = Number(response.headers.get('content-length') || 0);
      if (length > 2 * 1024 * 1024) throw new Error('entry-size');
      return response.text();
    }).finally(() => clearTimeout(entryTimer));
    if (entryText.length > 2 * 1024 * 1024) throw new Error('entry-size');
    const initialMatch = entryText.match(/["']\.\/assets\/(app-initial-[A-Za-z0-9_-]+\.js)["']/);
    if (!initialMatch) throw new Error('initial-module');
    const initialUrl = new URL(`./assets/${initialMatch[1]}`, entryUrl);
    const initialController = new AbortController();
    const initialTimer = setTimeout(() => initialController.abort(), 2000);
    const initialText = await fetch(initialUrl.href, { signal: initialController.signal }).then((response) => {
      if (!response.ok) throw new Error('initial-fetch');
      const length = Number(response.headers.get('content-length') || 0);
      if (length > 20 * 1024 * 1024) throw new Error('initial-size');
      return response.text();
    }).finally(() => clearTimeout(initialTimer));
    if (initialText.length > 20 * 1024 * 1024) throw new Error('initial-size');
    const actionsMatch = initialText.match(/["']\.\/register-app-actions-([A-Za-z0-9_-]+)\.js["']/);
    if (!actionsMatch) throw new Error('actions-module');
    const actionsUrl = new URL(`./register-app-actions-${actionsMatch[1]}.js`, initialUrl);
    if (actionsUrl.protocol !== location.protocol || actionsUrl.hostname !== location.hostname) throw new Error('actions-origin');
    const module = await import(actionsUrl.href);
    const registry = module.appActionRegistry;
    const action = registry?.get?.('app.appearance.get');
    if (typeof action !== 'function') throw new Error('action-missing');
    appearance = await action({ type: 'app.appearance.get' }, undefined);
    appearanceReadable = Boolean(appearance && typeof appearance === 'object');
  } catch {}
  return { effectiveMode, effective, appearanceReadable, appearance };
})()"#;

const APPEARANCE_MODE_SCRIPT: &str = r#"(() => {
  const root = document.documentElement;
  const marker = [root?.className, root?.dataset?.theme, root?.dataset?.appearance,
    document.body?.className, document.body?.dataset?.theme].filter(Boolean).join(' ').toLowerCase();
  const effectiveMode = /\belectron-dark\b|\b(?:theme|appearance)-dark\b|\bdark\b/.test(marker)
    ? 'dark'
    : (/\belectron-light\b|\b(?:theme|appearance)-light\b|\blight\b/.test(marker)
      ? 'light'
      : (matchMedia?.('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'));
  return { effectiveMode, effective: {}, appearanceReadable: false, appearance: null };
})()"#;

const REMOVE_SCRIPT: &str = r#"(() => {
  window.__CODEX_DREAM_SKIN_DISABLED__ = true;
  const state = window.__CODEX_DREAM_SKIN_STATE__;
  if (state?.cleanup) {
    try { state.cleanup(); } catch {}
  } else {
    document.documentElement?.classList.remove('codex-dream-skin');
    document.documentElement?.style.removeProperty('--dream-skin-art');
    document.documentElement?.style.removeProperty('--skin-background-image');
    document.documentElement?.style.removeProperty('--dream-skin-avatar');
    document.documentElement?.style.removeProperty('--dream-skin-friends');
    document.getElementById('codex-dream-skin-style')?.remove();
    document.getElementById('codex-dream-skin-chrome')?.remove();
    delete window.__CODEX_DREAM_SKIN_STATE__;
  }
  const compatibility = window.__BIFANG_CODEX_SKIN_COMPAT__;
  if (compatibility?.cleanup) {
    try { compatibility.cleanup(); } catch {}
  }
  return true;
})()"#;

//! 换肤模块共享的调试端口、超时、容量与资源上限常量。

use super::*;

/// 新启动 Codex 的固定调试端口；定向重启已选实例时可在受限区间另选端口。
pub(super) const DEFAULT_CDP_PORT: u16 = 9341;
/// WorkBuddy 调试端口的默认起点，与 Codex 错开，避免两个宿主抢同一个端口。
pub(super) const WORKBUDDY_DEFAULT_CDP_PORT: u16 = 9441;
/// 新启动 Codex 后，等主页面就绪的最长时间。
pub(super) const CODEX_PAGE_READY_TIMEOUT: Duration = Duration::from_secs(15);
/// 连接已在运行的 Codex 时，等主页面就绪的最长时间。
pub(super) const EXISTING_CODEX_PAGE_READY_TIMEOUT: Duration = Duration::from_secs(15);
/// 等待页面就绪时每隔多久看一次。
pub(super) const CODEX_PAGE_POLL_INTERVAL: Duration = Duration::from_millis(350);
/// 等待进程出现或退出时每隔多久看一次。
pub(super) const CODEX_PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(750);
/// 启动 Codex 并等它开放调试端口的最长时间。
pub(super) const CODEX_LAUNCH_TIMEOUT: Duration = Duration::from_secs(15);
/// 强制关闭 Codex 后，等进程真正退出的最长时间。
pub(super) const CODEX_FORCE_CLOSE_TIMEOUT: Duration = Duration::from_secs(15);
/// 建立调试连接的最长时间。
pub(super) const CDP_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// 单个调试请求等待回复的最长时间。
pub(super) const CDP_REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
/// 页面目标出现后再等一小会儿，让它稳定下来再操作。
pub(super) const CDP_TARGET_SETTLE_DELAY: Duration = Duration::from_millis(200);
/// 读取账号头像与名称时，等界面准备好的最长时间。
pub(super) const ACCOUNT_PROFILE_READY_TIMEOUT: Duration = Duration::from_secs(2);
/// 账号信息还没准备好时的重试间隔。
pub(super) const ACCOUNT_PROFILE_RETRY_INTERVAL: Duration = Duration::from_millis(150);
/// 接受的账号头像最大字节数，防止异常数据撑爆内存。
pub(super) const MAX_ACCOUNT_AVATAR_BYTES: usize = 256 * 1024;
/// 从页面读取运行中样式文本的最大字符数。
pub(super) const MAX_RUNTIME_STYLE_PROBE_CHARS: usize = 256 * 1024;
/// 皮肤守护检查页面是否还在、皮肤是否还生效的周期。
pub(super) const SKIN_WATCH_INTERVAL: Duration = Duration::from_secs(5);
/// 清理页面里已注入皮肤的最长时间。
pub(super) const CDP_CLEANUP_TIMEOUT: Duration = Duration::from_secs(10);
/// 停止皮肤守护任务时等它收尾的最长时间。
pub(super) const WATCH_STOP_TIMEOUT: Duration = Duration::from_secs(12);
/// 注入到页面里的皮肤运行时版本，页面里带着它便于判断是不是自己注入的。
pub(super) const SKIN_VERSION: &str = "1.7.0";
/// 宿主兼容脚本的版本号，脚本有变化时要一起升级。
pub(super) const HOST_COMPATIBILITY_VERSION: &str = "5";
/// Codex 宿主兼容脚本，随程序内置。
pub(super) const HOST_COMPATIBILITY_SCRIPT: &str = include_str!("../../resources/skin-host-compat.js");
/// WorkBuddy 宿主兼容脚本，随程序内置。
pub(super) const WORKBUDDY_HOST_COMPATIBILITY_SCRIPT: &str =
    include_str!("../../resources/workbuddy-skin-host-compat.js");
/// 主题皮肤运行时样式表，随程序内置。
pub(super) const THEME_RUNTIME_CSS: &str = include_str!("../../resources/theme-runtime/theme.css");
/// 主题皮肤运行时注入脚本，随程序内置。
pub(super) const THEME_RUNTIME_SCRIPT: &str = include_str!("../../resources/theme-runtime/renderer-inject.js");
/// “新建主题”使用的默认清单模板。
pub(super) const THEME_TEMPLATE_MANIFEST: &str = include_str!("../../resources/theme-template/theme.json");
/// “新建主题”使用的默认样式模板。
pub(super) const THEME_TEMPLATE_CSS: &str = include_str!("../../resources/theme-template/theme.css");
/// “新建主题”附带的默认预览图与背景图。
pub(super) const THEME_TEMPLATE_IMAGES: [(&str, &[u8]); 2] = [
    (
        "preview.png",
        include_bytes!("../../resources/theme-template/preview.png"),
    ),
    (
        "background.png",
        include_bytes!("../../resources/theme-template/background.png"),
    ),
];
/// 皮肤包里单个文本文件的最大字节数。
pub(super) const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
/// 导入单个皮肤包允许的最大字节数。
pub(super) const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
/// 一个皮肤压缩包里最多允许的条目数，防止压缩炸弹。
pub(super) const MAX_IMPORT_ENTRIES: usize = 256;
/// 一次批量导入全部皮肤包的字节数上限。
pub(super) const MAX_IMPORT_BATCH_BYTES: u64 = MAX_IMPORT_BYTES * MAX_IMPORT_BATCH_FILES as u64;
/// 导入临时目录的递增序号，保证并发导入各用各的目录。
pub(super) static IMPORT_SEQUENCE: AtomicU64 = AtomicU64::new(0);
/// 旧版皮肤包必须包含的文件。
pub(super) const LEGACY_REQUIRED_FILES: [&str; 6] = [
    "dream-skin.css",
    "renderer-inject.js",
    "theme.json",
    "qq2007-sky.png",
    "avatar.png",
    "qqshow.jpg",
];
/// 旧版皮肤包里真正会注入页面运行的文件。
pub(super) const LEGACY_RUNTIME_FILES: [&str; 5] = [
    "dream-skin.css",
    "renderer-inject.js",
    "qq2007-sky.png",
    "avatar.png",
    "qqshow.jpg",
];

/// 在 Codex 页面里运行的探测脚本：确认当前页面确实是 Codex 主界面，而不是别的网页。
pub(super) const PROBE_SCRIPT: &str = r#"(() => {
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

/// 在 WorkBuddy 页面里运行的探测脚本：确认当前页面确实是 WorkBuddy 主界面。
pub(super) const WORKBUDDY_PROBE_SCRIPT: &str = r#"(() => {
  const body = document.body;
  return {
    url: location.href,
    workBuddy: location.protocol === 'file:' && document.title === 'WorkBuddy' &&
      Boolean(document.querySelector('#root')) &&
      body?.dataset.applicationName === 'workbuddy' &&
      body?.dataset.electronDesktop === 'true' &&
      body?.dataset.productName === 'WorkBuddy',
  };
})()"#;

/// 读取页面里账号头像与名称的探测脚本；只在能唯一确定按钮时才返回结果。
pub(super) const ACCOUNT_PROFILE_PROBE_SCRIPT: &str = r#"(() => {
  const normalize = (value) => String(value ?? '').replace(/\s+/g, ' ').trim();
  const buttons = [...document.querySelectorAll('button[aria-haspopup="menu"]')];
  const semanticLabels = new Set(['打开个人资料菜单', 'Open profile menu']);
  const semantic = buttons.filter((button) => semanticLabels.has(normalize(button.getAttribute('aria-label'))));
  const candidates = semantic.length > 0 ? semantic : buttons.filter((button) => {
    const label = normalize(button.innerText);
    return button.querySelector('img') && label.length > 0 && label.length <= 80 &&
      !/[\u0000-\u001f\u007f]/.test(label);
  });
  if (candidates.length !== 1) return { label: null, avatarDataUrl: null };
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
  };
})()"#;

/// 判断页面里当前生效的是哪套皮肤的探测脚本（同时兼容旧版运行时留下的标记）。
pub(super) const ACTIVE_SKIN_PROBE_SCRIPT: &str = r#"(() => {
  if (window.__CODEX_DREAM_SKIN_STATE__ || window.__BIFANG_CODEX_SKIN_COMPAT__) return { activeSkin: null };
  const runtime = window.__LOKI_METRIS_DREAM_SKIN_STATE__;
  if (runtime?.quiesced === true) return { activeSkin: null };
  const marker = runtime?.skin;
  const root = document.documentElement;
  const style = document.getElementById('loki-metris-dream-skin-style');
  const styleText = style?.textContent || '';
  const styleApplied = window.__LOKI_METRIS_DREAM_SKIN_DISABLED__ !== true &&
    root?.classList.contains('codex-dream-skin') === true &&
    style?.tagName === 'STYLE' && style.isConnected === true &&
    Boolean(styleText.trim()) && Boolean(style.sheet?.cssRules.length);
  if (!styleApplied) return { activeSkin: null };
  if (marker && typeof marker === 'object' && typeof runtime.cleanup !== 'function') {
    return { activeSkin: null };
  }
  return { activeSkin: marker && typeof marker === 'object'
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
      }
  };
})()"#;

/// 读取 Codex 当前外观（明暗、强调色等）的探测脚本，读不到时只返回有效模式。
pub(super) const APPEARANCE_PROBE_SCRIPT: &str = r#"(async () => {
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

/// 只读取明暗模式的轻量探测脚本，用于外观读取失败时兜底。
pub(super) const APPEARANCE_MODE_SCRIPT: &str = r#"(() => {
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

/// 从页面里移除已注入皮肤的清理脚本；清理失败要如实返回 false，不能假装成功。
pub(super) const REMOVE_SCRIPT: &str = r#"(() => {
  const ownState = window.__LOKI_METRIS_DREAM_SKIN_STATE__;
  const ownCompat = window.__LOKI_METRIS_CODEX_SKIN_COMPAT__;
  if (!ownState && !ownCompat && !window.__LOKI_METRIS_SKIN_TRANSACTION__) return true;
  if (window.__CODEX_DREAM_SKIN_STATE__ || window.__BIFANG_CODEX_SKIN_COMPAT__) {
    let stopped = true;
    for (const state of [ownState, ownCompat]) {
      if (!state) continue;
      if (typeof state.quiesce !== 'function') { stopped = false; continue; }
      try { if (state.quiesce() !== true) stopped = false; } catch { stopped = false; }
    }
    if (stopped) delete window.__LOKI_METRIS_SKIN_TRANSACTION__;
    return stopped;
  }
  window.__LOKI_METRIS_DREAM_SKIN_DISABLED__ = true;
  let cleaned = true;
  const state = window.__LOKI_METRIS_DREAM_SKIN_STATE__;
  if (state?.cleanup) {
    try { state.cleanup(); } catch { cleaned = false; }
  } else {
    try {
      document.documentElement?.classList.remove('codex-dream-skin');
      document.documentElement?.style.removeProperty('--dream-skin-art');
      document.documentElement?.style.removeProperty('--skin-background-image');
      document.documentElement?.style.removeProperty('--dream-skin-avatar');
      document.documentElement?.style.removeProperty('--dream-skin-friends');
      document.getElementById('loki-metris-dream-skin-style')?.remove();
      document.getElementById('loki-metris-dream-skin-chrome')?.remove();
      delete window.__LOKI_METRIS_DREAM_SKIN_STATE__;
    } catch { cleaned = false; }
  }
  const compatibility = window.__LOKI_METRIS_CODEX_SKIN_COMPAT__;
  if (compatibility?.cleanup) {
    try { compatibility.cleanup(); } catch { cleaned = false; }
  }
  if (cleaned) delete window.__LOKI_METRIS_SKIN_TRANSACTION__;
  return cleaned;
})()"#;

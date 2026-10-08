#!/usr/bin/env node
/** 仅渲染原生 GUI/profile 与独立打包配置，不具有创建 workspace、core 或 Git 的写入能力。 */
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath } from 'node:url';
import { renderDefaultConfig } from '../../desktop-build-gpui-release/scripts/gpui_config.mjs';
import { renderPlatformIcons, renderDmgBackground } from '../../desktop-build-gpui-release/scripts/gpui_icons.mjs';
import { renderGpuiPackageJson } from './gpui_node_tooling.mjs';
import { normalizeCapabilities, nativeTemplate, nativeMemberDependencies, renderNativeProfile, trayPixels } from './gpui_native_capabilities.mjs';
import { parseGpuiInitializationProfile } from './gpui_profile.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const assetsRoot = path.resolve(here, '../assets');
const text = {
  'zh-CN': {
    navigation: { settings: '设置', about: '关于', sponsor: '赞助' },
    sidebar: { toggle: '展开或收起侧栏' },
    settings: { application: '应用信息', local_preferences: '外观和语言偏好仅保存在此设备。', appearance: '外观', appearance_detail: '选择浅色、深色，或实时跟随操作系统外观。', light: '浅色', dark: '深色', system: '跟随系统', language: '界面语言', language_detail: '默认探测系统语言，可在此随时切换。', chinese: '中文', english: 'English' },
    about: { application: '关于应用', description: '这是中性桌面脚手架，产品功能尚未定义。', author: '项目负责人', check_updates: '检查更新', release_notes: '更新日志', release_features: '功能优化', release_fixes: '问题修复', no_changes: '无', updater_unavailable: '当前 GPUI 基线未提供更新服务，不会发起网络请求。', no_releases: '当前构建未嵌入发布记录。', local_first: '本地运行', local_first_detail: '设置、语言和静态支持页可离线使用。' },
  },
  'en-US': {
    navigation: { settings: 'Settings', about: 'About', sponsor: 'Sponsor' },
    sidebar: { toggle: 'Expand or collapse sidebar' },
    settings: { application: 'Application', local_preferences: 'Appearance and language preferences are stored on this device.', appearance: 'Appearance', appearance_detail: 'Choose light, dark, or follow changes to the operating system appearance.', light: 'Light', dark: 'Dark', system: 'Follow system', language: 'Interface language', language_detail: 'Detect the system language by default, or choose a language here.', chinese: '中文', english: 'English' },
    about: { application: 'About this application', description: 'This is a neutral desktop scaffold. Product features have not been defined.', author: 'Project owner', check_updates: 'Check for updates', release_notes: 'Release notes', release_features: 'Feature improvements', release_fixes: 'Bug fixes', no_changes: 'None', updater_unavailable: 'The current GPUI baseline has no update service and makes no network request.', no_releases: 'This build has no embedded release notes.', local_first: 'Runs locally', local_first_detail: 'Settings, languages and static support pages work offline.' },
  },
};

/** 所有路径组件必须是普通目录或文件，拒绝符号链接穿透。 */
export function assertRealPath(value, allowMissing = false) {
  const absolute = path.resolve(value);
  const parsed = path.parse(absolute);
  let current = parsed.root;
  for (const segment of absolute.slice(parsed.root.length).split(path.sep).filter(Boolean)) {
    current = path.join(current, segment);
    let metadata;
    try { metadata = fs.lstatSync(current); } catch (error) {
      if (allowMissing && error.code === 'ENOENT') return;
      throw error;
    }
    if (metadata.isSymbolicLink()) throw new Error(`symbolic links are not allowed: ${current}`);
    if (current !== absolute && !metadata.isDirectory()) throw new Error(`ancestor must be a directory: ${current}`);
  }
}

/** 输入严格绑定双语身份、九字段与条件原生能力平台边界。 */
export function normalizeOptions(options) {
  const result = { aboutPage: 'enabled', sponsorPage: 'enabled', sidebarMode: 'detailed', targetPlatforms: ['macos'], ...options };
  if (!/^[a-z][a-z0-9]*(?:_[a-z0-9]+)*$/.test(result.projectId ?? '') || result.projectId.length > 64) {
    throw new Error('projectId must be an ASCII snake_case Rust identifier of at most 64 characters');
  }
  for (const key of ['nameZh', 'nameEn', 'owner']) {
    if (typeof result[key] !== 'string' || !result[key].isWellFormed() || !result[key].trim() || result[key] !== result[key].trim() || /[\x00-\x1f\x7f]/.test(result[key]) || result[key].length > 200) {
      throw new Error(`${key} must be nonempty single-line text of at most 200 characters`);
    }
  }
  for (const key of ['aboutPage', 'sponsorPage']) {
    if (!['enabled', 'disabled'].includes(result[key])) throw new Error(`${key} must be enabled or disabled`);
  }
  if (!['compact', 'detailed'].includes(result.sidebarMode)) throw new Error('sidebarMode must be compact or detailed');
  if (!Array.isArray(result.targetPlatforms) || !result.targetPlatforms.length || result.targetPlatforms.some(value => !['macos', 'windows', 'linux'].includes(value)) || new Set(result.targetPlatforms).size !== result.targetPlatforms.length) {
    throw new Error('targetPlatforms must contain distinct macos, windows or linux values');
  }
  normalizeCapabilities(result);
  if (result.logo !== undefined) {
    if (!path.isAbsolute(result.logo)) throw new Error('logo must be an absolute PNG path');
    assertRealPath(result.logo);
    const metadata = fs.lstatSync(result.logo);
    if (!metadata.isFile() || metadata.size < 33 || metadata.size > 8 * 1024 * 1024) throw new Error('logo must be a bounded regular PNG file');
    const logo = fs.readFileSync(result.logo);
    if (!logo.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10]))) throw new Error('logo must have a PNG signature');
    result.logoBytes = logo;
  }
  return result;
}

/** JSON 与 Rust 的基本字符串转义在已排除控制字符后保持相同。 */
function rustText(value) { return JSON.stringify(value).slice(1, -1); }

/** 读取模板时同样拒绝损坏链接，不允许插值遗漏进入生成源码。 */
function template(file, tokens) {
  const source = path.join(assetsRoot, file);
  assertRealPath(source);
  const value = fs.readFileSync(source, 'utf8');
  return value.replace(/@@([A-Z_]+)@@/g, (_, key) => {
    if (!Object.hasOwn(tokens, key)) throw new Error(`unresolved template token ${key} in ${file}`);
    return tokens[key];
  });
}

/** 生成全部文件供初始化器组合使用；不读取或修改目标工程。 */
export function renderGpuiAdapterFiles(input) {
  const options = normalizeOptions(input);
  const { projectId, nameZh, nameEn, owner } = options;
  const about = options.aboutPage === 'enabled';
  const sponsor = options.sponsorPage === 'enabled';
  const compact = options.sidebarMode === 'compact';
  const notification = options.systemNotification === 'enabled';
  const native = nativeTemplate(options);
  const nativeDeps = nativeMemberDependencies(options);
  const profile = JSON.parse(fs.readFileSync(path.join(assetsRoot, 'brand-support/brand-support-profile.json'), 'utf8'));
  const contact = profile.contacts.windowTitle;
  const logoExtension = options.logoBytes ? 'png' : 'svg';
  const manifest = JSON.parse(fs.readFileSync(path.join(assetsRoot, 'brand-support/media-manifest.json'), 'utf8'));
  const media = sponsor ? manifest.assets : [];
  const tokens = {
    ...native.tokens,
    CORE_PROBE: options.neutralCoreProbe ? `        let _definition = ${projectId}_core::product_definition();` : `        use ${projectId}_core as _;`,
    PROJECT_ID: projectId, KEBAB_ID: projectId.replaceAll('_', '-'), NAME_ZH: rustText(nameZh), NAME_EN: rustText(nameEn), OWNER: rustText(owner),
    APP_IDENTIFIER: `com.${projectId.replaceAll('_', '-')}.app`,
    NOTIFICATION_MODULE: notification ? 'mod notifications;' : '',
    SETTINGS_TEST_MODULE: notification ? '#[cfg(test)]\nmod settings_test;' : '',
    NOTIFICATION_FIELD: notification ? '    pub system_notification: bool,' : '',
    NOTIFICATION_TEST_VALUE: notification ? 'system_notification: true,' : '',
    NOTIFICATION_DECODE: notification ? '                "system_notification" => value.system_notification = field == "true",' : '',
    NOTIFICATION_ENCODE: notification ? '        text.push_str(&format!("system_notification={}\\n", self.system_notification));' : '',
    NOTIFICATION_MUT: notification ? 'mut ' : '',
    NOTIFICATION_SETTING: notification ? `            .child(self.card("settings.system_notification", "settings.system_notification_detail", cx)
                .child(gpui_kit::component::switch::Switch::new("system-notification-switch")
                    .label(self.t("settings.system_notification")).checked(self.preferences.system_notification)
                    .on_click(cx.listener(|this, enabled, _, cx| {
                        this.preferences.system_notification = *enabled;
                        this.writer.save(&this.preferences);
                        cx.notify();
                    }))))` : '',
    LOGO_EXT: logoExtension, COMPACT: String(compact), CONTACT_TITLE: ` ${rustText(contact.channel)}:${rustText(contact.value)}`, BRAND_CONTACT: rustText(profile.contacts.support.value),
    ABOUT_MODULE: about ? 'mod about;' : '', SPONSOR_MODULE: sponsor ? 'mod sponsor;' : '',
    ABOUT_VARIANT: about ? '    About,' : '', SPONSOR_VARIANT: sponsor ? '    Sponsor,' : '',
    ABOUT_FIELDS: about ? '    show_update_status: bool,\n    show_release_notes: bool,' : '', ABOUT_DEFAULTS: about ? '            show_update_status: false, show_release_notes: false,' : '',
    ABOUT_TITLE: about ? '            Page::About => "navigation.about",' : '', SPONSOR_TITLE: sponsor ? '            Page::Sponsor => "navigation.sponsor",' : '',
    ABOUT_RENDER: about ? '            Page::About => self.about_page(cx),' : '', SPONSOR_RENDER: sponsor ? '            Page::Sponsor => self.sponsor_page(window, cx),' : '',
    SPONSOR_ACTIVE: sponsor ? 'self.page == Page::Sponsor' : 'false',
    ABOUT_NAV: about ? '        sidebar = sidebar.child(self.navigation_item(Page::About, "nav-about", "navigation.about", IconName::Info, cx));' : '',
    SPONSOR_NAV: sponsor ? '        sidebar = sidebar.child(self.navigation_item(Page::Sponsor, "nav-sponsor", "navigation.sponsor", IconName::Heart, cx));' : '',
    COLLAPSE_CONTROL: compact ? '' : `            .child(Button::new("sidebar-toggle").ghost().icon(IconName::PanelLeft).absolute()
                .left(px(self.sidebar_width() - 12.)).top(px(SIDEBAR_INSET + self.sidebar_logo_size() / 2. - 12.) + gpui_kit::rems(SIDEBAR_LOGO_TOP).to_pixels(window.rem_size()))
                .size(px(24.)).tooltip(self.t("sidebar.toggle")).accessibility_label(self.t("sidebar.toggle"))
                .on_click(cx.listener(|this, _, _, cx| { this.preferences.collapsed = !this.preferences.collapsed; this.writer.save(&this.preferences); cx.notify(); })))`,
    SPONSOR_ASSETS_LOAD: media.map(asset => `        if path == ${JSON.stringify(asset.bundlePath)} { return Ok(Some(Cow::Borrowed(include_bytes!(${JSON.stringify('../assets/' + asset.bundlePath)})))); }`).join('\n'),
    SPONSOR_ASSETS_LIST: media.length ? `        paths.extend([${media.map(asset => JSON.stringify(asset.bundlePath)).join(', ')}].into_iter().filter(|path| path.starts_with(prefix)).map(SharedString::from));` : '',
    SPONSOR_TIERS: profile.sponsor.tiers.map((tier, index) => `                            .child(self.sponsor_tier(${index}, ${JSON.stringify(tier.nameKey)}, ${tier.price}, ${JSON.stringify(tier.image)}, ${JSON.stringify(tier.imageAltKey)}, &[${tier.benefits.map(benefit => `(${JSON.stringify(benefit.mainKey)}, ${benefit.noteKey ? `Some(${JSON.stringify(benefit.noteKey)})` : 'None'})`).join(', ')}], cx))`).join('\n'),
  };
  const files = new Map();
  files.set('package.json', renderGpuiPackageJson());
  files.set(`${projectId}_gui/Cargo.toml`, `[package]\nname = "${projectId}_gui"\nversion.workspace = true\nedition.workspace = true\nrust-version.workspace = true\n\n[dependencies]\n${projectId}_core.workspace = true\ngpui-kit.workspace = true\nrust-i18n.workspace = true\nsys-locale.workspace = true\ntracing.workspace = true\ntracing-subscriber.workspace = true\ntracing-appender.workspace = true\n\n[dev-dependencies]\ngpui-kit = { workspace = true, features = ["test-support"] }\n`);
  files.set(`${projectId}_gui/build.rs`, template('gui/build.rs', tokens));
  const guiCargo = files.get(`${projectId}_gui/Cargo.toml`);
  files.set(`${projectId}_gui/Cargo.toml`, guiCargo.replace('\n[dev-dependencies]', `\n${nativeDeps.normal}${nativeDeps.target}\n[dev-dependencies]`));
  for (const file of ['main.rs', 'assets.rs', 'preferences.rs', 'preferences_test.rs', 'logging.rs', 'logging_test.rs', 'lifecycle.rs', 'lifecycle_test.rs', 'release_notes.rs', 'app/mod.rs', 'app/mod_test.rs', 'app/settings.rs', ...native.files, ...(notification ? ['notifications.rs', 'notifications_test.rs', 'app/settings_test.rs'] : []), ...(about ? ['app/about.rs'] : []), ...(sponsor ? ['app/sponsor.rs', 'app/sponsor_test.rs'] : [])]) {
    files.set(`${projectId}_gui/src/${file}`, template(`gui/src/${file}`, tokens));
  }
  if (options.systemTray === 'enabled') files.set(`${projectId}_gui/assets/tray.rgba`, trayPixels(options.logoBytes));
  files.set(`${projectId}_gui/assets/logo.${logoExtension}`, options.logoBytes ?? fs.readFileSync(path.join(assetsRoot, 'gui/assets/logo.svg')));
  files.set('packaging/gpui.json', JSON.stringify(renderDefaultConfig({ projectId, nameZh, nameEn, owner, logoPath: `${projectId}_gui/assets/logo.${logoExtension}` }), null, 2) + '\n');
  files.set('packaging/macos/background.png', renderDmgBackground());
  if (options.logoBytes) {
    const icons = renderPlatformIcons(options.logoBytes);
    files.set('packaging/icons/app.icns', icons.macos);
    files.set('packaging/icons/app.ico', icons.windows);
  }
  for (const locale of ['zh-CN', 'en-US']) {
    const localized = structuredClone(text[locale]);
    localized.application = { name: locale === 'zh-CN' ? nameZh : nameEn };
    const brand = JSON.parse(fs.readFileSync(path.join(assetsRoot, `brand-support/i18n/${locale}.json`), 'utf8'));
    if (about) localized.about = { ...brand.about, ...localized.about };
    else { delete localized.about; delete localized.navigation.about; }
    if (sponsor) localized.sponsor = brand.sponsor;
    else delete localized.navigation.sponsor;
    if (compact) delete localized.sidebar;
    if (notification) Object.assign(localized.settings, locale === 'zh-CN'
      ? { system_notification: '应用通知', system_notification_detail: '允许应用提交系统通知；系统权限与送达由操作系统决定。开启此项不会发送测试通知。' }
      : { system_notification: 'Application notifications', system_notification_detail: 'Allow this app to submit system notifications. Permission and delivery are controlled by the operating system. This switch sends no test message.' });
    if (options.systemTray === 'enabled') localized.tray = locale === 'zh-CN' ? { show_window: '显示窗口', quit: '退出' } : { show_window: 'Show Window', quit: 'Quit' };
    if (options.autostart === 'enabled') Object.assign(localized.settings, locale === 'zh-CN'
      ? { autostart: '开机自启', autostart_detail: '登录系统后正常显示应用，实际状态以操作系统登录项为准。', autostart_error: '操作失败；可读取时已显示系统实际状态。', autostart_unknown: '当前无法读取系统登录项，请重试。', reload: '重新读取' }
      : { autostart: 'Start at login', autostart_detail: 'Show this app normally after sign-in. The operating system registration is authoritative.', autostart_error: 'The operation failed. The actual system state is shown when readable.', autostart_unknown: 'The system login item is unavailable. Retry reading it.', reload: 'Reload' });
    files.set(`${projectId}_gui/locales/${locale}.json`, JSON.stringify(localized, null, 2) + '\n');
  }
  for (const asset of media) {
    const file = path.join(assetsRoot, 'brand-support', asset.sourcePath);
    assertRealPath(file);
    const bytes = fs.readFileSync(file);
    if (bytes.length !== asset.sizeBytes || crypto.createHash('sha256').update(bytes).digest('hex') !== asset.sha256) throw new Error(`managed sponsor asset changed: ${asset.sourcePath}`);
    files.set(`${projectId}_gui/assets/${asset.bundlePath}`, bytes);
  }
  files.set('docs/GUI_APP_PROFILE.md', `# GUI 应用资料\n\n- 中文名称：${nameZh}\n- English name: ${nameEn}\n- GUI framework: gpui\n- owner: ${owner}\n- Logo: ${options.logoBytes ? 'caller-selected PNG; upstream identity evidence remains required' : 'neutral engineering fixture; not a confirmed product identity'}\n- unavailable native capabilities: deep_link\n${renderNativeProfile(options)}`);
  parseGpuiInitializationProfile(files.get('docs/GUI_APP_PROFILE.md'));
  files.set(`${projectId}_gui/assets/SOURCE.md`, `# 本地资源来源\n\n应用 Logo 来自${options.logoBytes ? '调用方明确选择的 PNG；正式身份核对由初始化器负责' : '本 Skill 自绘中性工程夹具，不代表正式产品身份'}。\n\n${sponsor ? 'sponsor/* 逐字节复用 Harness desktop-prepare-gui-support-surfaces 的受管品牌资产；批准范围、尺寸、SHA-256、敏感支付材料分类及许可责任见 sponsor-media-manifest.json。支付二维码仅静态展示，不授权支付自动化或公开再许可。' : '赞助页未选择，没有赞助媒体或支付材料。'}\n`);
  if (sponsor) files.set(`${projectId}_gui/assets/sponsor-media-manifest.json`, JSON.stringify(manifest, null, 2) + '\n');
  return files;
}

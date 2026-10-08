/** GPUI 原生能力的条件依赖、模板接线与中性 profile；helper 只用 Node 标准库。 */
import { decodePng } from '../../desktop-build-gpui-release/scripts/gpui_icons.mjs';

export const capabilityKeys = ['systemTray', 'systemNotification', 'autostart', 'singleInstance', 'globalShortcut'];

/** 缺省不启用；显式非法选择与不支持的平台组合在目标写入前拒绝。 */
export function normalizeCapabilities(options) {
  for (const key of capabilityKeys) {
    options[key] ??= 'disabled';
    if (!['enabled', 'disabled'].includes(options[key])) throw new Error(`${key} must be enabled or disabled`);
  }
  if (options.deepLink !== undefined && options.deepLink !== 'disabled') throw new Error('deepLink is unavailable in the GPUI baseline; only disabled is supported');
  options.deepLink = 'disabled';
  if (options.systemTray === 'enabled' && options.targetPlatforms.includes('linux')) throw new Error('systemTray currently supports macos/windows only; Linux must use disabled');
  if (options.globalShortcut === 'enabled' && options.targetPlatforms.includes('linux')) throw new Error('globalShortcut currently supports macos/windows only; Linux backend cannot verify registration');
  return options;
}

/** 只声明真正被当前能力引用的工作区依赖，固定壳层依赖仍由原 renderer 拥有。 */
export function nativeDependencies(options) {
  const result = [];
  if (options.systemTray === 'enabled') result.push(['tray-icon', '{ version = "0.26.1", default-features = false }']);
  if (options.autostart === 'enabled') result.push(['auto-launch', '"0.6.0"']);
  if (options.singleInstance === 'enabled') result.push(['interprocess', '"2.4.4"']);
  if (options.globalShortcut === 'enabled') result.push(['global-hotkey', '"0.8.0"']);
  if (['systemTray', 'autostart', 'singleInstance', 'globalShortcut'].some(key => options[key] === 'enabled')) result.push(['raw-window-handle', '"0.6.2"']);
  return result;
}

/** 根与成员使用同一条件表；窗口句柄依赖仅由 Windows 宿主继承。 */
export function nativeMemberDependencies(options) {
  const deps = nativeDependencies(options);
  const normal = deps.filter(([key]) => !['tray-icon', 'raw-window-handle'].includes(key)).map(([key]) => `${key}.workspace = true`).join('\n');
  const tray = options.systemTray === 'enabled' ? '\n[target.\'cfg(any(windows, target_os = "macos"))\'.dependencies]\ntray-icon.workspace = true\n' : '';
  const windows = deps.some(([key]) => key === 'raw-window-handle') ? '\n[target.\'cfg(windows)\'.dependencies]\nraw-window-handle.workspace = true\n' : '';
  return { normal: normal ? normal + '\n' : '', target: tray + windows };
}

/** 在工程阶段解码所选 PNG，运行时托盘不新增 PNG 解码 crate。 */
export function trayPixels(logoBytes) {
  const result = Buffer.alloc(32 * 32 * 4);
  if (!logoBytes) {
    for (let y = 4; y < 28; y++) for (let x = 4; x < 28; x++) result.set([30, 110, 220, 255], (y * 32 + x) * 4);
    return result;
  }
  const image = decodePng(logoBytes);
  for (let y = 0; y < 32; y++) for (let x = 0; x < 32; x++) {
    const offset = (Math.floor((y + 0.5) * image.height / 32) * image.width + Math.floor((x + 0.5) * image.width / 32)) * image.channels;
    result.set([image.pixels[offset], image.pixels[offset + 1], image.pixels[offset + 2], image.channels === 4 ? image.pixels[offset + 3] : 255], (y * 32 + x) * 4);
  }
  if (!result.some((byte, index) => index % 4 === 3 && byte > 0)) throw new Error('system tray icon must have visible nontransparent pixels');
  return result;
}

/** 生成条件 token，禁用能力的模块、控件和宿主调用完全缺席。 */
export function nativeTemplate(options) {
  const tray = options.systemTray === 'enabled';
  const auto = options.autostart === 'enabled';
  const single = options.singleInstance === 'enabled';
  const shortcut = options.globalShortcut === 'enabled';
  const enabled = tray || auto || single || shortcut;
  const files = enabled ? ['native/mod.rs', 'native/windows.rs'] : [];
  if (tray) files.push('native/tray.rs');
  if (auto) files.push('native/autostart.rs', 'native/autostart_windows.rs');
  if (single) files.push('native/instance.rs', 'native/instance_test.rs');
  if (shortcut) files.push('native/shortcuts.rs');
  return { enabled, files, tokens: {
    NATIVE_MODULE: enabled ? 'mod native;' : '',
    NATIVE_LOCALE: enabled ? '    let locale = preferences.locale();' : '',
    NATIVE_HANDLE: enabled ? 'handle' : '_handle',
    NATIVE_INSTALL: enabled ? `        native::install(handle, locale, ${single ? 'instance, instance_receiver, ' : ''}cx);` : '',
    NATIVE_LANGUAGE: tray ? '        crate::native::set_language(self.locale, cx);' : '',
    NATIVE_CLOSE: tray ? '            !crate::native::hide_if_available(cx)' : '            true',
    INSTANCE_START: single ? `    let (instance_sender, instance_receiver) = std::sync::mpsc::sync_channel(1);
    let directory = match preferences::instance_directory() { Ok(path) => path, Err(_) => return std::process::ExitCode::FAILURE };
    let instance = match native::instance::Instance::acquire(&directory, instance_sender) {
        Ok(native::instance::Launch::Primary(instance)) => instance,
        Ok(native::instance::Launch::Activated) => return std::process::ExitCode::SUCCESS,
        Err(_) => { eprintln!("single_instance_unavailable"); return std::process::ExitCode::FAILURE; }
    };` : '',
    INSTANCE_DIRECTORY: single ? `/// 单实例使用独立私有子目录；不改变已有偏好目录的权限。
pub(crate) fn instance_directory() -> std::io::Result<PathBuf> {
    path().and_then(|path| path.parent().map(|parent| parent.join("instance"))).ok_or(std::io::ErrorKind::NotFound.into())
}` : '',
    NATIVE_SUBMODULES: [tray ? 'mod tray;' : '', auto ? 'pub mod autostart;' : '', single ? 'pub mod instance;' : '', shortcut ? 'pub mod shortcuts;' : ''].filter(Boolean).join('\n'),
    NATIVE_FIELDS: [tray ? '    tray: Option<tray::Tray>,' : '', auto ? '    pub autostart: autostart::Service,' : '', single ? '    _instance: instance::Instance,' : '', shortcut ? '    pub shortcuts: shortcuts::Service,' : ''].filter(Boolean).join('\n'),
    NATIVE_INSTANCE_ARGUMENTS: single ? 'instance: instance::Instance, receiver: std::sync::mpsc::Receiver<()>, ' : '',
    NATIVE_CREATE: `    let _ = locale;\n${tray ? `    #[cfg(windows)]
    let allows_tray = native_window.is_some();
    #[cfg(not(windows))]
    let allows_tray = true;
    let tray = if allows_tray { tray::Tray::new(locale).ok() } else { None };
    if tray.is_none() { tracing::warn!(event = "tray_unavailable"); }` : ''}
${auto ? '    let autostart = autostart::Service::new();\n    let mut revision = 0;' : ''}`,
    NATIVE_DEFAULTS: [tray ? '        tray,' : '', auto ? '        autostart,' : '', single ? '        _instance: instance,' : '', shortcut ? '        shortcuts: shortcuts::Service::new(),' : ''].filter(Boolean).join('\n'),
    NATIVE_POLL: [single ? '                if receiver.try_iter().take(64).count() > 0 { show(handle, cx); }' : '', tray ? `                let events = cx.global::<Services>().tray.as_ref().map(|tray| tray.events()).unwrap_or_default();
                for event in events { match event { tray::Event::Show => show(handle, cx), tray::Event::Quit => cx.quit() } }` : '',
      auto ? '                let current = cx.global::<Services>().autostart.state().revision;\n                if current != revision { revision = current; cx.refresh_windows(); }' : '',
      shortcut ? '                let handlers = cx.global::<Services>().shortcuts.pending();\n                for dispatch in handlers { dispatch(cx); }' : ''].filter(Boolean).join('\n'),
    NATIVE_TRAY_METHODS: tray ? `/// 托盘实际成功才隐藏，失败允许正常关闭，不留下无法恢复的进程。
pub fn hide_if_available(cx: &mut App) -> bool {
    if !cx.try_global::<Services>().is_some_and(|services| services.tray.is_some()) { return false }
    #[cfg(windows)]
    let native = cx.global::<Services>().window;
    cx.defer(move |cx| {
        #[cfg(windows)]
        if let Some(window) = native { window.hide(); }
        #[cfg(not(windows))]
        cx.hide();
    });
    true
}
/// 语言切换更新已创建的双语托盘菜单。
pub fn set_language(locale: &str, cx: &mut App) {
    if cx.has_global::<Services>() { if let Some(tray) = &mut cx.global_mut::<Services>().tray { tray.set_language(locale); } }
}` : '',
    NATIVE_AUTOSTART_METHODS: auto ? `/// 设置视图仅读取内存快照，实际 OS 状态由后台 worker 更新。
pub fn autostart_state(cx: &App) -> autostart::State {
    cx.try_global::<Services>().map(|services| services.autostart.state()).unwrap_or_default()
}
/// 事件直接来自自启控件，初始化不会调用此 mutation。
pub fn set_autostart(enabled: bool, cx: &App) {
    if let Some(services) = cx.try_global::<Services>() { services.autostart.set(enabled); }
}
/// 读取失败时提供显式重试，不能乐观假定系统状态。
pub fn reload_autostart(cx: &App) {
    if let Some(services) = cx.try_global::<Services>() { services.autostart.reload(); }
}` : '',
    AUTOSTART_SETTING: auto ? `            .child(self.card("settings.autostart", "settings.autostart_detail", cx)
                .child(gpui_kit::component::switch::Switch::new("autostart-switch")
                    .label(self.t("settings.autostart")).checked(crate::native::autostart_state(cx).enabled.unwrap_or(false))
                    .map(|switch| gpui_kit::base::Disableable::disabled(switch, crate::native::autostart_state(cx).busy || crate::native::autostart_state(cx).enabled.is_none()))
                    .on_click(cx.listener(|_, enabled, _, cx| { crate::native::set_autostart(*enabled, cx); cx.notify(); })))
                .when(crate::native::autostart_state(cx).error, |card| card.child(self.t("settings.autostart_error")))
                .when(crate::native::autostart_state(cx).enabled.is_none(), |card| card.child(self.t("settings.autostart_unknown")))
                .child(Button::new("autostart-reload").outline().label(self.t("settings.reload"))
                    .on_click(cx.listener(|_, _, _, cx| { crate::native::reload_autostart(cx); cx.notify(); }))))` : '',
  } };
}

/** 中性九字段仍保持固定顺序；快捷键独立空合同没有产品默认动作。 */
export function renderNativeProfile(options) {
  const key = name => options[name.replace(/_([a-z])/g, (_, letter) => letter.toUpperCase())];
  const fields = ['system_tray', 'system_notification', 'autostart', 'about_page', 'sponsor_page', 'single_instance', 'deep_link', 'global_shortcut', 'sidebar_mode'];
  let result = '\n```gui-initialization-config\n' + fields.map(name => `${name}: ${key(name)}`).join('\n') + '\n```\n';
  if (options.globalShortcut === 'enabled') result += '\n```gui-global-shortcut-contract\n{"schemaVersion":1,"actions":[]}\n```\n';
  return result;
}

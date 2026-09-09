# LokiMetis Rust Core 与 Tauri GUI 基线

本文件描述当前终端项目的 Rust/Tauri 技术基线。产品业务范围由最新 Approved Product Spec 定义；这里的页面、命令和宿主能力本身不代表发布就绪。

## 固定架构

- workspace 成员固定为 `loki_metis_core` 与 `loki_metis_gui/src-tauri`，接口固定为 GUI。
- `loki_metis_core` 拥有平台无关的领域类型、规则、校验、用例、状态转换、持久化策略和稳定错误。
- `loki_metis_gui` 是唯一薄 adapter：React 负责展示与纯交互状态，Tauri 负责 IPC、运行时装配和桌面宿主机制。GUI 直接依赖 core，core 不得反向依赖 GUI/Tauri/WebView。
- 根 `Cargo.toml` 保存目标平台 `windows`、`macos`、`linux`、接口 `gui`、Rust 1.95 MSRV 和当前版本；锁文件保存真实解析结果。

## 技术栈

- Rust 2024 edition、Tauri 2、Tokio、serde、tracing、rust-i18n。
- React 19、TypeScript、Vite、Mantine、TanStack Router、TanStack Query、Jotai、i18next/react-i18next、Tabler Icons。
- 系统通知启用时，Windows/Linux 使用 `tauri-plugin-notification` Rust API；macOS target 另使用 `mac-usernotifications = "0.3.1"` 的现代 User Notifications API。WebView 不安装 notification JavaScript 包或获得 `notification:*` capability。
- 前端生产代码不得依赖 Node.js API。依赖版本、Node/pnpm engines 与脚本以当前清单为准，不要求全局第三方包。

## 当前 GUI 能力

唯一配置见 `docs/GUI_APP_PROFILE.md`。当前启用系统托盘、系统通知、开机自启、设置页赞助支持、单实例和受限深链接，侧栏为 compact；独立赞助路由与全局快捷键禁用。

Tauri Builder 顺序固定为：

1. `tauri-plugin-single-instance`，回调只恢复并聚焦主窗口；
2. `tauri-plugin-deep-link`，只接受 `app-loki-metis://restore` 的冷/热启动事件；
3. `tauri-plugin-os`，仅由 Rust 读取系统 locale；
4. `tauri-plugin-window-state`，只保存/恢复尺寸、位置和最大化；
5. `tauri-plugin-dialog`，WebView capability 精确为 `dialog:default`；
6. `tauri-plugin-opener`，只允许系统浏览器打开固定 GitHub 仓库地址；
7. `tauri-plugin-notification`，由 Rust-only worker 串行处理；
8. `tauri-plugin-autostart`，以 OS 注册状态为权威。

每个插件恰好注册一次。托盘由同一 Builder 的 `.setup(...)` 与 `.on_window_event(...)` 接线；关闭主窗口隐藏，托盘“显示窗口”/“Show Window”和左键恢复，浮窗显隐项按真实可见性切换为“显示浮窗”/“Show Floating Window”或“隐藏浮窗”/“Hide Floating Window”且冷启动默认显示，托盘“退出程序”/“Exit Program”真正终止应用。托盘安装后与既有扫描/来源/Agent 变更路径复用 core 联合窗口聚合刷新当天 Token 总数，并在 adapter 展示层按十进制 `K`/`M`/`B`、两位小数和逗号千分位格式化；无当天记录或联合读取失败时清空标题，不新增独立轮询或统计存储。Tauri 原生标题在 Windows 不受支持，Windows 保持仅图标降级。

桌宠使用独立的 `pet` WebView 窗口，右键设置按需创建独立的 `pet-settings` WebView 窗口。两者都不挂主壳；`pet-settings` 关闭时销毁，下一次右键再创建，避免隐藏 WebView 常驻。桌宠布局、分页、缩放、锁定、置顶、位置和尺寸由 Rust 权威状态驱动，React 只发送交互意图并渲染一致快照。12 个展示位置及同位置最近迁移裁决属于 core 规则，不得在 Tauri 或 React 中按 Agent 枚举重新绑定。

## IPC 与状态

- 只有一个 `invoke_handler`，固定包含 `get_app_metadata`、`get_system_locale`、`set_interface_language`、`load_release_notes`，并包含通知与自启各自的窄 get/set 命令。
- `get_app_metadata` 从打包名称、Cargo 版本和产品定义状态返回类型化元数据，标题固定为 `LokiMetis` 且不带版本号；不得返回联系人或浏览器猜测值。
- 系统语言由 OS locale 与已保存语言偏好共同解析；设置语言时同步 React i18next、Rust `rust_i18n` 与托盘文案。
- 通知应用偏好默认关闭并由 Rust 持有；macOS 使用现代 `UNUserNotificationCenter` 异步 API，先读取真实授权状态，只有 `NotDetermined` 才请求权限并复读；`Authorized`、`Provisional`、`Ephemeral` 才允许持久化启用。`Denied`、`Restricted`/`Unknown` 或请求后仍未授权时保持偏好为 `false`，并由 Rust-only 受控 opener 使用固定 `x-apple.systempreferences:com.apple.Notifications-Settings.extension?id=` 前缀与当前 `AppHandle` identifier 打开本应用的系统通知设置；它不得接受 WebView URL、任意 bundle identifier 或 shell 字符串，打开失败必须返回独立、可观察的稳定错误。该恢复入口不扩大只允许固定 GitHub 地址的 WebView opener capability。
- 自启默认不注册且始终回读 OS 状态。通知或自启异步切换失败时，界面重读并恢复真实状态，显示可操作错误；通知权限、排队和投递不得只写日志或伪装成功。
- 页面会话状态保存在应用根 Jotai store，进程内跨路由保持；TanStack Query 拥有异步数据和缓存。领域状态始终以 core 为权威。

## 固定壳层

- 首次窗口 1440×900，最小 960×640，居中；持久状态无效或不再与当前显示器工作区相交时回退该基线。
- 使用 `tauri-gui-sidebar-compact-80-v1`：侧栏宽 80px、内容内边距 6px、Logo 36px、图标 22px、名称全宽居中，不提供折叠动作。
- 固定 `/settings` 展示应用、版本、本地更新日志、固定 GitHub 仓库入口、语言、浅色/深色/跟随系统主题、通知开关、自启开关、唯一 Agent 复选面板与底部双收款码赞助区。用量看板的物理 Agent 与 WorkBuddy 视图在「数据源」后显示 `/dashboard/settings` 「设置」，只承载全局扫描间隔与自动清理；「全部」视图不显示该子页。Hooks 目录与写入表单位于 `/monitor/settings` 并只消费统一选择。
- `/about`、独立 `/sponsor`、`/test` 必须缺席；设置与侧栏不显示联系人、隐私、统计、遥测或应用更新入口。收款码只从本地 bundle 展示，GitHub opener capability 只允许精确仓库 URL。
- 所有可见文案提供 `zh-CN` 与 `en-US`；系统不支持的语言回退英文。交互控件必须有可访问名称、焦点和键盘路径。

## 应用身份

- 应用显示名为 LokiMetis，中文名为诡秘神谕；bundle identifier 与 deep-link scheme 必须由项目身份一致派生。
- Windows NSIS 同时提供 English 与简体中文并显示语言选择器；用户可见安装名称与快捷方式随所选语言变化，稳定安装身份仍为 LokiMetis。macOS 通过 `InfoPlist.strings` 按系统首选语言本地化 Finder 显示名；物理安装包、DMG 与 `.app` 名保持 LokiMetis。
- 母版位于 `loki_metis_gui/src-tauri/icons/app-icon-master.png`，前端副本位于 `loki_metis_gui/public/app-identity/logo.png`，两者逐字节一致。
- 平台图标由项目本地 Tauri `icon` 命令从母版生成。`bundle.icon` 必须列出完整平台图标集（含 `.ico` 与 `.icns`）：Windows 主程序与 MSI 取第一个 `.ico`，macOS `.app` 取 `.icns`，`default_window_icon()` 在 Unix 上取第一个 `.png`，因此托盘来源必须是可见的 32×32、8-bit RGBA、非交错 PNG。NSIS `installerIcon` 与 `uninstallerIcon` 不会回落到 `bundle.icon`，必须显式指定，否则安装器使用 NSIS 默认图标。具体路径见 `docs/GUI_APP_PROFILE.md`。
- macOS DMG 背景是独立的 660×400 打包资产，不得与运行时窗口状态或应用 Logo 混用。

## 本地发布说明

- 所有 GUI 保留发布专用 `src-tauri/tauri.release.conf.json`，只在正式候选构建时把项目根 `release-notes.json` 映射为 bundle 根的同名资源；调试构建不传该配置。
- `load_release_notes` 不接受路径参数，通过 `BaseDirectory::Resource` 与 `tokio::fs` 异步读取固定资源。Rust 与 React 都验证 schema v2、1 MiB 上限、最多五版、每类十条、完整中英文对、最新在前和单个小写 `v`。
- 首次正式发布前资源缺失时，设置页显示可重试错误，不使用编译期假数据，也不授予通用文件系统权限。
- 本地发布说明不构成 updater；应用不得建立联网版本服务、下载或安装路径。

## 异步与日志

- 后台任务必须有 owner、取消、超时/重试上限和关闭回收；不得 detached，锁不得跨越不受控 `.await`。
- tracing 使用有界滚动本地文件，包含时间、级别、target 与消息，并对路径和载荷脱敏；不配置网络 exporter。
- 只有真实业务需要 Tokio I/O、同步、时间或任务原语时才增加 feature。

## 开发验证

- 日常开发只运行本次变化需要的相关非空 core、Rust adapter 与 React 回归测试。
- 业务行为必须先有 core 测试，再验证 IPC/视图映射；宿主能力由结构测试和适用的真实宿主场景覆盖。
- 侧栏、主题、i18n、设置、通知、自启、托盘、单实例、深链接、窗口状态和 dialog 的实现必须与 profile 一致，并扫描禁用能力、updater、统计和远程遥测残留。
- macOS 通知结构回归必须覆盖授权状态先于请求、只在 `NotDetermined` 请求、拒绝/受限/请求后仍未授权时精确打开当前应用通知设置，以及打开失败可观察；真实权限、设置恢复与投递只由已签名、公证并 stapled 的安装候选 E2E 证明，未签名 debug 应用不能代证。
- 格式、lint、全仓测试、真实 Tauri 构建、性能和最终产物 E2E 只在本次变化或专用流程要求时运行。实际命令以当前清单与对应 Skill 为准。

## 构建与交付

- Windows 原生本地安装试包只生成 x64 NSIS 开发制品，不升级为候选。
- 正式候选支持 macOS 原生 DMG、Windows 原生 x64 NSIS，以及 macOS xwin 的 Windows x64 NSIS；xwin 不能证明 Windows 原生安装、运行或性能。
- Linux 是源码目标平台，但在建立真实渠道合同前正式产物保持 `Unverified`。
- `$desktop-prepare-release` 在任何发布提交或候选构建前锁定本次发布的 `reviewSelection`、`performanceSelection` 与 macOS `macosSigningSelection`/`macosSigningSource`。三项选择不写入通用持久策略，同一发布的修复或中断重跑复用原选择，新发布重新解析；`system_notification = enabled` 的 macOS 候选必须启用签名，冲突在提交前停止。E2E 不由发布准备锁定，构建只为本次运行另行解析当前 E2E 选择与渠道硬要求。
- 候选必须来自具名分支的 clean 40 位 source commit。构建只读消费已经封存的审查、性能和签名信封，不得重新询问、翻转选择或制造证据；先运行完整非空 Rust/前端测试。审查启用时，结构化证据必须绑定被审查的同一 source commit；性能启用或渠道强制时，在打包前以同一 clean HEAD 的 `gui-release-v2` release-profile no-bundle 探针验证冷启动、交互/Long Task、整进程树 CPU/RSS、内存增长和进程回收。纯指标失败只有在原始证据允许时才可由用户明确 waiver，结构、绑定、窗口状态恢复或进程回收失败不可豁免。
- macOS 签名关闭时使用 `--no-sign`，记录原因与剩余风险且不探测本机签名身份、证书、公证凭据或 profile；启用时 Developer ID 签名、公证、stapling、Gatekeeper 与最终验证缺一不可，任一步失败都阻断且不得回退 unsigned。xwin runtime 和 Windows 原生安装/性能继续精确标记 `Unverified`，不能由交叉构建成功代证。
- 所有布局、签名、公证、stapling 和包内资源变化完成后才计算最终安装包、release notes 与适用审查/性能证据的摘要。候选先在隔离 staging 中写入 `milestoneAcceptance: pending` 的 manifest，再按 manifest 重新枚举并复算精确文件集、大小、SHA-256、选择与证据；全部成功后才以不跟随链接的目录级原子替换提交到根 `release/`，提交后只读重验，不得遗留历史、额外、空或未声明文件。
- manifest 至少绑定项目/版本、source commit 与 clean 状态、平台/架构、bundle format、native/xwin 模式、安装包路径/大小/SHA-256、release-notes 版本/路径/摘要、Rust/前端测试数、`runtimeVerification`、审查/性能/签名及适用公证证据、当次 E2E 选择和 `milestoneAcceptance`。最终字节形成后，`$desktop-verify-delivery` 按持久策略要求的冒烟、构建当次 E2E、渠道硬要求以及同一候选的审查、性能、发布说明、DMG 布局、签名和 manifest 完整性决定 `accepted` 或退回开发循环；E2E 不得翻转已锁定选择或把失败、`Not run`、`Unverified`、unsigned 通知候选改判为通过。
- 发布说明、签名、公证、stapling、重打包或渠道处理只要改变运行字节、启动器、依赖或行为，就形成新候选并重新进入构建与验收。构建和发布准备不自动授权 tag、push、上传、商店提交或正式渠道发布。

## 按需产品能力

保留最新 Approved Product Spec 已明确批准的本地文件能力、经官方身份与安装路径验证的 Codex/WorkBuddy GUI 进程操作、仅回环地址的 CDP，以及只允许 `https://github.com/ManonLoki/LokiMetis` 的 WebView opener；macOS 通知恢复另仅允许上述 Rust-only 固定系统设置入口。这些都是有界产品/宿主能力，不构成通用文件系统、任意进程或 shell、非回环网络、任意 URL 或远程数据管线授权。

新增或扩大文件、外部程序、网络与系统入口必须先由产品规格明确批准，再由 `$desktop-implement-change` 读取对应能力参考并保持 core-first、最小权限、可取消、可观察、可测试和失败可恢复。普通网络能力不得用于重新引入 updater、产品统计或远程遥测。

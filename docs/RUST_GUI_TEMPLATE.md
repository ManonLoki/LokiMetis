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
- 前端生产代码不得依赖 Node.js API。依赖版本、Node/pnpm engines 与脚本以当前清单为准，不要求全局第三方包。

## 当前 GUI 能力

唯一配置见 `docs/GUI_APP_PROFILE.md`。当前启用系统托盘、系统通知、开机自启、单实例和受限深链接，侧栏为 compact；赞助页与全局快捷键禁用且必须零残留。

Tauri Builder 顺序固定为：

1. `tauri-plugin-single-instance`，回调只恢复并聚焦主窗口；
2. `tauri-plugin-deep-link`，只接受 `app-loki-metis://restore` 的冷/热启动事件；
3. `tauri-plugin-os`，仅由 Rust 读取系统 locale；
4. `tauri-plugin-window-state`，只保存/恢复尺寸、位置和最大化；
5. `tauri-plugin-dialog`，WebView capability 精确为 `dialog:default`；
6. `tauri-plugin-notification`，由 Rust-only worker 串行处理；
7. `tauri-plugin-autostart`，以 OS 注册状态为权威。

每个插件恰好注册一次。托盘由同一 Builder 的 `.setup(...)` 与 `.on_window_event(...)` 接线；关闭主窗口隐藏，托盘“显示窗口”和左键恢复，桌宠显隐项按真实可见性切换浮窗且冷启动默认显示，托盘“退出”真正终止应用。

桌宠使用独立的 `pet` WebView 窗口，右键设置按需创建独立的 `pet-settings` WebView 窗口。两者都不挂主壳；`pet-settings` 关闭时销毁，下一次右键再创建，避免隐藏 WebView 常驻。桌宠布局、分页、缩放、锁定、置顶、位置和尺寸由 Rust 权威状态驱动，React 只发送交互意图并渲染一致快照。12 个展示位置及同位置最近迁移裁决属于 core 规则，不得在 Tauri 或 React 中按 Agent 枚举重新绑定。

## IPC 与状态

- 只有一个 `invoke_handler`，固定包含 `get_app_metadata`、`get_system_locale`、`set_interface_language`、`load_release_notes`，并包含通知与自启各自的窄 get/set 命令。
- `get_app_metadata` 从打包名称、Cargo 版本和产品定义状态返回类型化元数据，标题固定为 `LokiMetis` 且不带版本号；不得返回联系人或浏览器猜测值。
- 系统语言由 OS locale 与已保存语言偏好共同解析；设置语言时同步 React i18next、Rust `rust_i18n` 与托盘文案。
- 通知应用偏好默认关闭并由 Rust 持有；自启默认不注册且始终回读 OS 状态。异步切换失败时界面恢复真实状态并显示可操作错误。
- 页面会话状态保存在应用根 Jotai store，进程内跨路由保持；TanStack Query 拥有异步数据和缓存。领域状态始终以 core 为权威。

## 固定壳层

- 首次窗口 1440×900，最小 960×640，居中；持久状态无效或不再与当前显示器工作区相交时回退该基线。
- 使用 `tauri-gui-sidebar-compact-80-v1`：侧栏宽 80px、内容内边距 6px、Logo 36px、图标 22px、名称全宽居中，不提供折叠动作。
- 固定 `/settings` 展示应用、版本、本地更新日志、语言、浅色/深色/跟随系统主题、通知开关、自启开关与唯一 Agent 复选面板。用量看板的物理 Agent 与 WorkBuddy 视图在「数据源」后显示 `/dashboard/settings` 「设置」，只承载全局扫描间隔与自动清理；「全部」视图不显示该子页。Hooks 目录与写入表单位于 `/monitor/settings` 并只消费统一选择。
- `/about`、`/sponsor`、`/test` 必须缺席；设置与侧栏不显示联系人、隐私、统计、遥测或应用更新入口。
- 所有可见文案提供 `zh-CN` 与 `en-US`；系统不支持的语言回退英文。交互控件必须有可访问名称、焦点和键盘路径。

## 应用身份

- 应用显示名为 LokiMetis，中文名为诡秘神谕；bundle identifier 与 deep-link scheme 必须由项目身份一致派生。
- 母版位于 `loki_metis_gui/src-tauri/icons/app-icon-master.png`，前端副本位于 `loki_metis_gui/public/app-identity/logo.png`，两者逐字节一致。
- 平台图标由项目本地 Tauri `icon` 命令从母版生成。托盘与 bundle 引用普通文件 `src-tauri/icons/32x32.png`，它必须是可见的 32×32、8-bit RGBA、非交错 PNG。
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
- 格式、lint、全仓测试、真实 Tauri 构建、性能和最终产物 E2E 只在本次变化或专用流程要求时运行。实际命令以当前清单与对应 Skill 为准。

## 构建与交付

- Windows 原生本地安装试包只生成 x64 NSIS 开发制品，不升级为候选。
- 正式候选支持 macOS 原生 DMG、Windows 原生 x64 NSIS，以及 macOS xwin 的 Windows x64 NSIS；xwin 不能证明 Windows 原生安装、运行或性能。
- Linux 是源码目标平台，但在建立真实渠道合同前正式产物保持 `Unverified`。
- 候选必须从 clean HEAD 运行完整非空 Rust/前端测试，按当次选择执行性能与最终产物 E2E，并遵守 `docs/RELEASE.md` 的 manifest、摘要、签名和原子收集规则。

## 按需产品能力

当前不预置产品文件操作、外部进程或网络能力。产品规格明确需求后，由 `$desktop-implement-change` 使用其对应参考并保持 core-first、最小权限、可取消、可观察和可测试。普通网络能力不得用于重新引入 updater、产品统计或远程遥测。

# Rust core 与 GPUI 工程基线

当前只选择 GPUI GUI，接口和平台取根 Cargo metadata。没有 CLI/TUI/MCP 或前端运行时，不因文档文件名附加接口。文件、中文注释、测试、记忆事件和例外详见 ENGINEERING_RULES。

## 工作区

- resolver 3、edition 2024、MSRV 1.98.1，根 workspace.package.version 是当前版本事实；core/gui 的 version/edition/rust-version 继承 workspace。
- loki_metris_core 已按批准复刻规格迁入历史纯 Rust 领域模型、SQLite 索引、解析与聚合；声明的项目内依赖见根 Cargo.toml。产品定义已完成，具体复刻进度见自身 Work Plan。core 不依赖 GPUI、Tauri 或 WebView。
- loki_metris_gui 使用 gpui-kit 0.7.3、rust-i18n 4.2.4 兼容范围、sys-locale 0.3.2、tracing 0.1.44、tracing-subscriber 0.3.23、tracing-appender 0.2.5；实际解析结果以 Cargo 为准。
- 已选原生机制使用 tray-icon 0.26.1、auto-launch 0.6.0、interprocess 2.4.5、global-hotkey 0.8.0；Windows 条件 raw-window-handle，Kit test-support 仅 dev-dependencies。依赖统一 workspace，成员不另定版本。
- 优先标准库/现有依赖，不预建运行时/驱动接口；真正必要新依赖先说明确认，显式声明在项目内隔离，不安装全局包。
- macOS标题辅助功能通过项目内`accesskit_macos 0.26.3`路径补丁修正原生Heading角色与层级，保持原公开接口和依赖。官方归档原样保留，标准库生成器只在OUT_DIR应用严格补丁；来源、完整许可、原生selector回归及移除条件见[vendor说明](../vendor/accesskit_macos/README.md)。本地与正式包装均携带三份原样许可及修改声明。

## Core-first 和运行时

GUI 只解析语法、持有展示/交互状态、调用 core 并映射错误。值域、跨字段关系、业务默认值、权限/幂等、权威状态、迁移/持久化策略、工作流和稳定领域错误属于 core。多个 core 调用间的业务编排应提升为一个用例 API。输入/输出使用 core 自有类型，不泄漏 GPUI/宿主类型。

GPUI loop 运行同步主线程，不使用 Tokio 根任务包裹或嵌套 runtime。未来业务异步复用 adapter 拥有的 runtime；render/handler 零阻塞 I/O，长任务有 owner、取消、有界输入/输出与退出回收。偏好仅启动前有界读取，后台合并/同目录临时文件原子替换；正常退出等待最后快照。业务持久化不落 GUI 偏好。

端侧唯一 tracing subscriber，INFO 以上结构化事件经有界队列同时写 stderr 和本地按日滚动日志，最多七份，退出刷新。core 只发事件不装 subscriber；不记录秘密、PII、原始业务内容或任意用户路径，错误用稳定类别。普通页面会话由根原生视图持有，同进程跨页保留、真正退出重置。

## GUI 与宿主

九字段与空快捷键 action contract 取 GUI_APP_PROFILE；中性初始化不得注册默认 chord、登录项或业务动作。托盘左键/显示菜单恢复聚焦，关闭隐藏，退出回收 owned 服务；创建失败不隐藏无法恢复的窗口。单实例私有目录锁/有界本地 IPC，第二进程只恢复窗口后成功退出，不转发业务载荷。自启以 OS 实际状态为准，后台串行写后复读，unknown/error 可重试，Windows 仅当前用户。平台内部线程限制按 native-capabilities 引用如实报告。

真实系统 locale 解析语言，未知回退英文；显式语言覆盖，主题支持浅/深/随系统。窗口首次/无效回退 1440×900 居中、最小 960×640，只恢复与显示器相交的尺寸/位置/最大化。compact 为固定 80px 窄栏不折叠，Logo 后当前版本，功能项向下，底部为公共设置。像素以设计索引的 GPUI 命中标准及已批准的历史复刻偏离为准。

GPUI 当前 updater、通用原生 dialog、深链接 unavailable；通知、原生目录/文件选择及支持入口按已批准复刻规格接入，具体出站和媒体所有权见 [GUI 支持界面](GUI_SUPPORT_SURFACES.md)。公共设置承载应用信息、更新日志和页内赞助区，无独立关于导航。普通构建无发布日志；正式候选通过 GPUI 构建注入及资源 map 携带已提交 schema v2 双语日志，包内字节/DMG 资源/manifest 必须一致。`packaging/gpui.json` 的 `releaseNotesUi` 声明设置页日志入口，关闭关于页不应跳过嵌入校验。

## 真实命令

在项目根执行 pnpm run test（cargo test --workspace --all-targets --all-features）、cargo build --workspace、cargo run -p loki_metris_gui、pnpm run validate。根 package.json 仅 Node 标准库工程入口，无 JS 前端或 npm 运行依赖。

需要 macOS 通知身份或包内皮肤资源的开发对照使用 `pnpm gpui:preview`，完成后通过 Computer Use 打开 `target/gpui-preview/LokiMetris.app`。此命令复用 Cargo 已配置的 host debug 缓存，核对源码／资源、完整 ad-hoc 签名与严格身份验证后才替换旧预览；旧包保留为同目录 `previous-*.app`。运行中的同路径预览必须先正常退出。工具不启动应用、不使用证书、不公证、不生成分发候选；候选继续使用下述独立打包入口。

Cargo.lock / pnpm 锁默认 ignored，测试构建不用 --locked/--frozen-lockfile；显式切换 tracked 后才按受管锁策略冻结解析。vendor原样归档中的Cargo.lock仅为来源证据，逐字节保留并跟踪，不是当前工作区的解析锁。日常只运行本次必要相关非空测试；显式候选全 workspace 测试，零测试/失败阻断。

普通本地试包通过 $desktop-build-gpui-release 的 local 模式；未来正式 Git 发布后，local_package 从 clean 默认主分支/tag 进入 candidate 模式，当前构建单独解析 E2E/签名。原生 macOS app/DMG 与原生 Windows x64 NSIS 分宿主生成，当前 Windows 尚未运行，不把 macOS 构建推断为 Windows 验证。

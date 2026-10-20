# Rust core 与 GPUI 工程基线

GUI 版本/兼容、性能和打包方案按 [GUI 工程索引](GUI_ENGINEERING.md) 分派；只在当前任务需要时读取对应 Skill。

当前只选择 GPUI GUI，接口和平台取根 Cargo metadata。没有 CLI/TUI/MCP 或前端运行时，不因文档文件名附加接口。文件、中文注释、测试、记忆事件和例外详见 ENGINEERING_RULES。

当前依赖声明、features、平台继承、本地兼容层和 Harness 工具版本统一见[技术选型与依赖清单](TECHNOLOGY.md)。它按根 Cargo 与 package.json 整理，不把保留初始化模板或本机忽略锁文件当作当前版本合同。

## 工作区

- resolver 3、edition 2024、MSRV 1.98.1，根 workspace.package.version 是当前版本事实；pairing/core/gui/mobile_bridge 的 version/edition/rust-version 继承 workspace。
- loki_metris_pairing 提供桌面与移动桥共用的SPAKE2 A/B、恢复通道、配对窗口与资源校验，不依赖GUI、数据库、网络或运行时；core依赖它进行配对验证并拥有设备保存规则。独立loki_metris_mobile_bridge拥有C ABI及缓冲映射unsafe，手机应用及JNI/Swift实际互通仍在实施。
- loki_metris_core 已按批准复刻规格迁入历史纯 Rust 领域模型、SQLite 索引、解析与聚合；声明的项目内依赖见根 Cargo.toml。产品定义已完成，具体复刻进度见自身 Work Plan。core 不依赖 GPUI、Tauri 或 WebView。
- loki_metris_gui 使用 gpui-kit 0.7.3、rust-i18n 4.2.4 兼容范围、sys-locale 0.3.2、tracing 0.1.44、tracing-subscriber 0.3.23、tracing-appender 0.2.5；实际解析结果以 Cargo 为准。
- 已选原生机制使用精确约束 tray-icon =0.26.1 及本地 HRESULT 补丁、auto-launch 0.6.0、interprocess 2.4.5、global-hotkey 0.8.0；三平台按 target 条件继承 raw-window-handle，Kit test-support 仅 dev-dependencies。依赖统一 workspace，成员不另定版本。
- Linux已接通同一tray-icon的`ksni`特性与恢复入口探测，通过会话D-Bus承载托盘，不启用GTK/AppIndicator后端；Linux GUI另显式继承ashpd 0.13.13（async-io/global_shortcuts）和x11rb 0.13.2（根xkb，成员另启用shape/randr）。Linux双架构构建与打包合同见[Linux工程知识库](LINUX_GPUI_TOOLCHAIN.md)。Portal/X11热键服务尚未接线，不把依赖编译当作完整Linux能力验收，也不改变空热键合同。
- 优先标准库/现有依赖，不预建运行时/驱动接口；真正必要新依赖先说明确认，显式声明在项目内隔离，不安装全局包。
- macOS标题辅助功能通过项目内`accesskit_macos 0.26.3`路径补丁修正原生Heading角色与层级，保持原公开接口和依赖。官方归档原样保留，标准库生成器只在OUT_DIR应用严格补丁；来源、完整许可、原生selector回归及移除条件见[vendor说明](../vendor/accesskit_macos/README.md)。本地与正式包装均携带三份原样许可及修改声明。

## Core-first 和运行时

GUI 只解析语法、持有展示/交互状态、调用 core 并映射错误。值域、跨字段关系、业务默认值、权限/幂等、权威状态、迁移/持久化策略、工作流和稳定领域错误属于 core。多个 core 调用间的业务编排应提升为一个用例 API。输入/输出使用 core 自有类型，不泄漏 GPUI/宿主类型。

GPUI loop 运行同步主线程，不使用 Tokio 根任务包裹或嵌套 runtime。当前 `UsageService` 在事件循环前建立并拥有双 worker Tokio runtime，扫描、查询、通知、Hooks 与后台任务复用其 handle；core 不建立根 runtime。render/handler 零阻塞 I/O，长任务有 owner、取消、有界输入/输出与退出回收。偏好仅启动前有界读取，后台合并/同目录临时文件原子替换；正常退出等待最后快照。业务持久化不落 GUI 偏好。

端侧唯一 tracing subscriber，INFO 以上结构化事件经有界队列同时写 stderr 和本地按日滚动日志，最多七份，退出刷新。core 只发事件不装 subscriber；不记录秘密、PII、原始业务内容或任意用户路径，错误用稳定类别。普通页面会话由根原生视图持有，同进程跨页保留、真正退出重置。

## GUI 与宿主

九字段与空快捷键 action contract 取 GUI_APP_PROFILE；中性初始化不得注册默认 chord、登录项或业务动作。托盘左键/显示菜单恢复聚焦，关闭隐藏，退出回收 owned 服务；创建失败不隐藏无法恢复的窗口。单实例私有目录锁/有界本地 IPC，第二进程只恢复窗口后成功退出，不转发业务载荷。自启以 OS 实际状态为准，后台串行写后复读，unknown/error 可重试，Windows 仅当前用户。平台内部线程限制按 native-capabilities 引用如实报告。

真实系统 locale 解析语言，未知回退英文；显式语言覆盖，主题支持浅/深/随系统。窗口首次/无效回退 1440×900 居中、最小 960×640，只恢复与显示器相交的尺寸/位置/最大化。compact 为固定 80px 窄栏不折叠，Logo 后当前版本，功能项向下，底部为公共设置。像素以设计索引的 GPUI 命中标准及已批准的历史复刻偏离为准。

GPUI 当前 updater、通用原生 dialog、深链接 unavailable；通知、原生目录/文件选择及支持入口按已批准复刻规格接入，具体出站和媒体所有权见 [GUI 支持界面](GUI_SUPPORT_SURFACES.md)。公共设置承载应用信息、更新日志和页内赞助区，无独立关于导航。普通构建无发布日志；正式候选通过 GPUI 构建注入及资源 map 携带已提交 schema v2 双语日志，包内字节/DMG 资源/manifest 必须一致。`packaging/gpui.json` 的 `releaseNotesUi` 声明设置页日志入口，关闭关于页不应跳过嵌入校验。

## 真实命令

源码启动使用 `pnpm run dev`，它由 Node 标准库设置 debug 皮肤资源路径再调用 `cargo run -p loki_metris_gui`；直接 cargo run 不会自动设置该资源路径。相关单元测试按本次修改选取；显式全量测试入口为 `pnpm run test`（cargo test --workspace --all-targets --all-features），编译入口为 `cargo build --workspace`，治理检查入口为 `pnpm run validate`。根 package.json 无 JS 前端或 npm 运行依赖，不因列出命令就自动执行全部步骤。

验证入口 `scripts/validate.mjs` 复用受管 GPUI runner，将行数检查定点接到项目封存物/vendor 摘要登记层；公共检查器保持上游原样，其余两项门禁和失败传播不变。保留的通用 `gpui_dev.mjs` 提供保存后编译/重启及POSIX owned进程组回收知识，Windows后代回收限制见[技术债](TECH_DEBT.md)，当前产品 `dev` 继续使用已有资源装配，不在升级中切换开发行为。

需要 macOS 通知身份或包内皮肤资源的开发对照使用 `pnpm gpui:preview`，完成后通过 Computer Use 打开 `target/gpui-preview/LokiMetris.app`。此命令复用 Cargo 已配置的 host debug 缓存，核对源码／资源、完整 ad-hoc 签名与严格身份验证后才替换旧预览；旧包保留为同目录 `previous-*.app`。运行中的同路径预览必须先正常退出。工具不启动应用、不使用证书、不公证、不生成分发候选；候选继续使用下述独立打包入口。

Cargo.lock / pnpm 锁默认 ignored，测试构建不用 --locked/--frozen-lockfile；显式切换 tracked 后才按受管锁策略冻结解析。vendor原样归档中的Cargo.lock仅为来源证据，逐字节保留并跟踪，不是当前工作区的解析锁。日常只运行本次必要相关非空测试；显式候选全 workspace 测试，零测试/失败阻断。

普通本地试包通过 $desktop-build-gpui-release 的 local 模式；正式 Git 发布后，local_package 从 clean 默认主分支/tag 进入 candidate 模式，当前构建单独解析 E2E/签名。macOS app/DMG 与 Windows x64 NSIS 分原生宿主生成；Linux GNU x64/ARM64 的 deb/tar.gz 及同系统跨架构合同见[Linux工程知识库](LINUX_GPUI_TOOLCHAIN.md)。各次构建和目标运行证据分别核对，不把一个宿主的构建、单元测试或实物结果推断为其它平台验收。

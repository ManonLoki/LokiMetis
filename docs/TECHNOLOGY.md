# 技术选型与依赖清单

本清单于 2026-10-22 按仓库已采用的配置和源码核对，不表示软件包仓库最新版本或本轮兼容性测试结果。版本、features、平台条件的事实源仍是[根 Cargo.toml](../Cargo.toml)、[配对库清单](../loki_metris_pairing/Cargo.toml)、[core 清单](../loki_metris_core/Cargo.toml)、[GUI 清单](../loki_metris_gui/Cargo.toml)、[移动桥清单](../loki_metris_mobile_bridge/Cargo.toml)及[根 package.json](../package.json)；变更这些配置时同步本清单。

## 工程与运行时

| 层次 | 当前选择 | 事实与边界 |
| --- | --- | --- |
| 产品接口 | GPUI 原生 Rust GUI | 根 metadata：`interfaces = ["gui"]`、`gui-framework = "gpui"`；没有产品 CLI/TUI/MCP 接口或 WebView 前端 |
| Rust 工程 | edition `2024`、resolver `3`、MSRV `1.98.1` | 四个成员继承 workspace 的版本、edition 和 rust-version；产品版本只取根清单 |
| 共享配对层 | `loki_metris_pairing` | 桌面与移动桥直接调用；共享配对/恢复会话、窗口与图片校验；不依赖GUI、SQLite、网络或异步运行时 |
| 移动协议桥 | `loki_metris_mobile_bridge` | 独立 C ABI 句柄/缓冲适配器，复用 pairing；本 crate 不含 JNI 导出。JNI/Swift 调用封装、手机网络及系统存储驱动由外部移动项目维护，Watch 经 iPhone 系统配套通道同步；平台接入与运行证据分别核对 |
| 领域层 | `loki_metris_core` | 解析、核算、查询、SQLite 索引、工作流和稳定领域错误；不依赖 GPUI |
| 桌面层 | `loki_metris_gui` | 视图、原生服务、I/O 驱动与运行时装配；GPUI 事件循环位于同步主线程 |
| 异步执行 | Tokio，由 GUI 的 `UsageService` 拥有 | [服务初始化](../loki_metris_service/src/usage_service/mod.rs)建立 2-worker runtime，承载扫描、查询、后台服务和通知；不把 GPUI loop 包进 Tokio 根任务 |
| 本地数据 | SeaORM + SQLite | core 定义索引与迁移，适配器驱动具体读写；原始 Agent 会话与本产品派生库分离 |
| 图像与字体 | `image` / `png` / `image-webp`，内嵌 Geist / Geist Mono | 静态图、GIF/APNG/WebP 与受控解码工作集；字体不安装到系统，许可见[字体目录](../loki_metris_gui/assets/fonts/) |
| 换肤 | `chromiumoxide`、ZIP 与本地皮肤资源 | 已批准的宿主 CDP/文件编排；不是应用自身的 WebView 运行时 |
| 工程脚本 | Node.js `>=24.21.0`、pnpm `>=12.4.1` | Node 标准库工具，无 npm 第三方依赖，无需先执行 `pnpm install` |
| Git 工具门槛 | `>=2.36.0` | 取[环境门禁](../.agents/skills/desktop-check-development-environment/references/development-environment-gates.md)；只在真实环境失败或明确配置工具链时进入恢复流程 |

架构与退出所有权见 [Rust/GPUI 基线](RUST_CLI_TEMPLATE.md)，产品范围与未完成项分别见[最新规格](product_spec/README.md)和[持续计划](work_plan/README.md)。

## 局域网配对技术路线

已确认采用共享Rust配对核心与移动端薄封装；BoringSSL各端SDK路线本次不采用。各平台接入、Watch配套路线、备选方案和实际证据边界统一见[设备配对技术选型](DEVICE_PAIRING_TECHNOLOGY.md)，精确套件及四帧合同见[设备配对协议](DEVICE_PAIRING_PROTOCOL.md)。移动端原生驱动与业务通信职责见[终端联动协议](DEVICE_LINK_PROTOCOL.md)；本选型不改变桌面目标平台，各宿主运行证据分别核对。

## 版本口径

下表的“声明”逐项对应根 `[workspace.dependencies]`，不是完整 resolved 清单。没有 `=` 的版本遵循 Cargo 兼容范围；`serde = "1"`、`serde_json = "1"` 也不代表固定补丁版。`gpui_apple` 是 `gpui-pre-apple` 的依赖别名。

根 `Cargo.lock` 与 pnpm 锁文件按当前默认 `ignored` 策略不跟踪；本机已有锁只能说明该次解析，不能作为跨机器固定版本合同。构建证据绑定当次源码和工具身份，不提供完整resolved依赖快照；此表也不能代替当次解析记录。vendor 原样归档中的锁文件属于第三方来源证据，继续完整保留。

### 领域、数据与异步

| 库 | 声明 | 当前用途 / 特征 |
| --- | --- | --- |
| `base64` | `0.23.1` | 协议与资源编码，core / GUI 继承 |
| `encoding_rs` | `0.8.42` | 本地输入文本编码 |
| `jiff` | `0.2.38` | 日期、时间与时区 |
| `sea-orm` | `2.0.4` | 关闭默认特征，`runtime-tokio-rustls,sqlx-sqlite`；host复用SQLite驱动只读Cursor固定状态键，系统无sqlite3时仍可读取 |
| `sea-orm-migration` | `2.0.4` | 同上，SQLite 迁移 |
| `serde` | `1` | `derive`，结构化类型序列化 |
| `serde_json` | `1` | JSON / JSONL 协议与持久化 |
| `mdns-sd` | `0.21.6` | GUI局域网发现；关闭默认特征，仅启用`async` |
| `spake2` | `=0.4.0` | 共享配对库密码交换；关闭默认特征，仅启用`getrandom` |
| `hkdf` | `0.13.0` | 共享配对库方向确认密钥与持久密钥派生；关闭默认特征 |
| `sha2` | `0.11.1` | 共享配对库transcript与内容寻址图片摘要；关闭默认特征 |
| `chacha20poly1305` | `0.11.0` | 共享配对库双向认证确认；关闭默认特征，仅启用`alloc` |
| `thiserror` | `2.0.21` | 稳定错误类型 |
| `toml` | `1.1.8` | TOML 配置 |
| `tokio` | `1.53.3` | 关闭默认特征，`fs,io-util,macros,net,process,rt,rt-multi-thread,sync,time` |
| `tracing` | `0.1.44` | 关闭默认特征，`std`；core 只发事件 |
| `loki_metris_pairing` | 本地路径 `loki_metris_pairing` | core继承同一轻量A/B协议库；精确SPAKE2变体，不宣称任意RFC9382实现兼容 |
| `loki_metris_core` | 本地路径 `loki_metris_core` | GUI 继承同一领域库；`test-support` 仅开发测试启用 |

### GUI、资源与诊断

| 库 | 声明 | 当前用途 / 特征 |
| --- | --- | --- |
| `gpui-kit` | `0.8.1` | 原生组件与图标；`test-support` 仅 dev-dependencies |
| `gpui_apple` | `=0.3.10` | 包名 `gpui-pre-apple`，关闭默认特征；仅 macOS 启用 `parent-rounded-clip`，统一已有传递依赖特征 |
| `rust-i18n` | `4.2.4` | 编译期中英词典 |
| `sys-locale` | `0.3.2` | 真实系统语言探测 |
| `tracing-subscriber` | `0.3.23` | 关闭默认特征，`fmt,registry,std`；端侧唯一订阅器 |
| `tracing-appender` | `0.2.5` | 关闭默认特征；有界后台日志与滚动文件 |
| `chromiumoxide` | `0.9.1` | 已批准的换肤宿主 CDP 连接 |
| `futures` | `0.3.34` | 异步流与 future 组合 |
| `zip` | `9.0.3` | 关闭默认特征，`deflate`；主题 ZIP 导入导出 |
| `anyhow` | `1.0.104` | 宿主调用错误上下文 |
| `uuid` | `1.28.0` | `v4,v7`；资源与事务标识 |
| `image` | `0.25.10` | 关闭默认特征，`jpeg,png,gif,bmp,webp`；WebP 复用已解析的 `image-webp` |
| `png` | `0.18.1` | PNG/APNG 资源处理 |
| `image-webp` | `0.2.4` | 与 `image` 共用同一锁定版本；仅读取 WebP 容器头，在解码像素前完成帧数与内存预算 |
| `tempfile` | `3.27.0` | GUI 临时资源与 core 测试夹具；core 仅 dev-dependency |
| `same-file` | `1.0.6` | 文件身份核对 |
| `whichdisk` | `0.7.0` | 关闭默认特征，`list`；卷能力识别 |

### 原生宿主

| 库 | 声明 | 当前平台条件 / 用途 |
| --- | --- | --- |
| `tray-icon` | `=0.26.1` | 关闭默认特征；macOS/Windows 普通继承，Linux 增加 `ksni`，路径补丁见下节 |
| `auto-launch` | `0.6.0` | 自启状态与注册；启动不自动注册 |
| `dirs` | `6.0.0` | 仅 Linux GUI；XDG 登录项读取与 auto-launch 写入共用 home 解析 |
| `interprocess` | `2.4.5` | 单实例本地 IPC |
| `global-hotkey` | `0.8.0` | 已选快捷键能力；当前 action 合同为空，无键位注册 |
| `ashpd` | `0.13.13` | 仅 Linux，关闭默认特征，`async-io,global_shortcuts`；Portal 热键依赖准备，后台服务尚未接线 |
| `x11rb` | `0.13.2` | 仅 Linux；根 `xkb`，GUI 另启用 `shape,randr`，窗口/显示器/输入区域 |
| `raw-window-handle` | `0.6.2` | 三平台分别按 target 条件继承，原生窗口桥接 |
| `windows` | `0.62.2` | 仅 Windows，关闭默认特征；Win32 / WinRT 窗口、文件、通知、注册表等 API，精确 feature 列表取根清单 |
| `objc2-foundation` | `0.3.2` | 仅 macOS，关闭默认特征，Foundation 类型与 block 桥接 |
| `objc2-app-kit` | `0.3.2` | 仅 macOS，关闭默认特征，AppKit 窗口/屏幕/应用 API |
| `objc2` | `0.6.5` | 仅 macOS，Objective-C 绑定 |
| `block2` | `0.6.2` | 仅 macOS，原生 block 回调 |
| `libc` | `0.2.190` | 仅 macOS GUI 直接继承 |
| `objc2-user-notifications` | `0.3.2` | 仅 macOS，关闭默认特征；权限、提交、响应及代理桥接 |

Linux 托盘采用 StatusNotifierItem/会话 D-Bus，不引入 GTK/AppIndicator 循环。通知使用本产品的原生 owner：macOS UserNotifications、Windows WinRT Toast、Linux freedesktop D-Bus；不再把通用 Kit 中性通知模板当作本产品完整实现。点击恢复、权限和取消边界见 [GUI 支持界面](GUI_SUPPORT_SURFACES.md)。

X11/XWayland 的桌宠、托盘恢复探测和 Linux 通知已接线；纯 Wayland 的桌宠全局定位/置顶与 Portal/X11 热键后台服务仍有明确边界。依赖声明、源码接入、构建成功、目标运行验收分别取证，详见 [Linux 工程知识库](LINUX_GPUI_TOOLCHAIN.md)。

## 本地第三方兼容层

根 `[patch.crates-io]` 指向以下七个包装包；`upstream/` 原样内容、来源摘要与许可证保持完整，生成器只在 Cargo `OUT_DIR` 应用严格补丁。

| 包 / 路径 | 包版本 | 维护职责 |
| --- | --- | --- |
| [accesskit_macos](../vendor/accesskit_macos/README.md) | `0.26.3` | 原生 Heading 角色与层级 |
| [gpui-pre](../vendor/gpui-pre/README.md) | `0.3.10` | 圆角视口、AX/延迟绘制、焦点代数、hitbox 排除与 Linux 隐藏创建首次映射 |
| [gpui-pre-apple](../vendor/gpui-pre-apple/README.md) | `0.3.10` | Apple/Metal 图元与可变字体；`parent-rounded-clip` 为 macOS 显式 opt-in |
| [gpui-pre-macos](../vendor/gpui-pre-macos/README.md) | `0.3.10` | macOS 窗口恢复、缩放/全屏失败与 reopen 桥接 |
| [gpui-pre-windows](../vendor/gpui-pre-windows/README.md) | `0.3.10` | Windows 渲染 ABI 与窗口兼容层 |
| [gpui-pre-wgpu](../vendor/gpui-pre-wgpu/README.md) | `0.3.10` | WGPU 图元/着色器 ABI |
| [tray-icon](../vendor/tray-icon/README.md) | `0.26.1` | Windows 托盘查询接受全部非负成功 HRESULT |

GPUI 五包同为 `0.3.10`，不要只升级其中一个包或把字体补丁仍定位到旧 `gpui-pre-macos`。来源文件数量/摘要的机械登记见 [ENGINEERING_RULES](ENGINEERING_RULES.md#23-分层代码行数门禁与通用文本上限)及相应 vendor 清单；补丁生成验证不代表 GPU 像素或候选 E2E 已通过。

## Harness 工程层

本仓库是已实例化的终端下游，保留 Harness 工程规则与 Skills 用于自身维护，不是可继续派生产品的 Harness 源根。没有本项目的 Harness 时间版本；产品版本仍由根 Cargo 与版本门禁管理。工程所有权与升级保护见 [desktop-upgrade-harness](../.agents/skills/desktop-upgrade-harness/SKILL.md)。

- `AGENTS.md` 负责启动与路由；[AGENT_POLICY](AGENT_POLICY.md)保存已确认能力开关、Task 与发布生命周期；[ENGINEERING_RULES](ENGINEERING_RULES.md)保存架构、测试和文档事件规则。
- GUI 方案按 [工程索引](GUI_ENGINEERING.md) 分派；相关 Skill 的依赖说明引用本清单；保留的 add-only renderer 仍是中性初始化快照，具体差异见[依赖参考](../.agents/skills/desktop-add-gpui-adapter/references/dependency-baseline.md)。不要据旧模板降级当前 Cargo 或覆盖产品代码。
- Node 仅运行 `.agents/skills/`、`scripts/` 中的工程工具；`validate` 是显式治理检查，`test` 是全 workspace 测试，日常文档维护只做必要解析/完整性检查。
- macOS/Windows 打包核对全局 PATH 的 `cargo-packager 0.11.8`；其内部辅助工具使用 OS 用户缓存，不能声称全部缓存均项目隔离。Linux 直接使用 GNU `tar`、`dpkg-deb`、`readelf`，不安装 cargo-packager。
- 打包选择与产物边界见 [RELEASE](RELEASE.md)和[构建 Skill](../.agents/skills/desktop-build-gpui-release/SKILL.md)；普通试包写 `target/`，正式候选写忽略的 `release/`，不据文档更新自动构建、发布或升级依赖。

受管 Tauri/Mantine 设计资料完整保留，但本项目运行栈只读取 GPUI 命中规则；本产品扩展以 [GUI profile](GUI_APP_PROFILE.md)与已批准 ADR 为准。

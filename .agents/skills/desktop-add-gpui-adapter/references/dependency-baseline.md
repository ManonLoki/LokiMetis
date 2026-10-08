# GPUI 初始化基础技术选型

初始化必须读取本表，按用途选择依赖，并在完成报告中列出实际声明项。来源是用户指定的 gpui-demo 示例工程直接 Cargo 依赖及其 2026-10-09 锁文件快照；版本是已知兼容参考，不代表 registry 最新版本，也不把示例业务变成中性脚手架要求。表中全部库纳入基础选型标准，具体安装仍遵守标准库优先、项目本地显式声明、按能力选择的已批准边界。不得全局安装。

## 固定壳层与条件宿主能力

| 库 | 版本基线 | 用途与启用条件 |
|---|---|---|
| `gpui-kit` | `0.7.1` | 固定原生 GUI、组件、图标及系统通知；不另装 GPUI 的不兼容副本 |
| `rust-i18n` | `4.2.0`（示例解析 `4.2.4`） | 固定编译期中英文词典，保留已验证兼容下界 |
| `sys-locale` | `0.3.2` | 固定真实系统语言探测 |
| `tracing` | `0.1.44`，关闭默认 features，`std` | Harness 固定端侧结构化日志，示例外的既有诊断标准 |
| `tracing-subscriber` | `0.3.23`，关闭默认 features，`fmt,registry,std` | 仅端侧安装一次日志订阅器 |
| `tracing-appender` | `0.2.5`，关闭默认 features | 仅端侧有界队列与按日滚动文件 |
| `tray-icon` | `0.26.1`，关闭默认 features | 只在托盘启用时加入；当前模板限 macOS/Windows，成员使用 target 条件继承 |
| `auto-launch` | `0.6.0` | 只在自启启用时加入；状态以 OS 为准，创建句柄不注册登录项 |
| `interprocess` | `2.4.4` | 只在单实例启用时加入；标准库文件锁配合本地 IPC 唤醒 |
| `raw-window-handle` | `0.6.2` | 已选原生服务的 Windows 窗口恢复/托盘隐藏桥；只由 Windows member 继承 |
| `global-hotkey` | `0.8.0` | 新增的真实后台热键选型；示例只有窗口内 KeyBinding。仅快捷键启用时加入；当前模板限 macOS/Windows，Linux 上游不能可靠报告注册失败，因此失败关闭 |

根 `[workspace.dependencies]` 声明实际使用的库，GUI member 使用 `workspace = true`；禁用能力的直接依赖、源码、设置、资源和专属测试全部缺席。系统通知复用 Kit，不增加通知 crate。托盘图标在工程阶段由 Node 标准库转换为 RGBA，不为运行时解码再加入 `png`。

## 示例中按实际用途采用的基础库

以下库是下游遇到对应需求时的首选基线。中性初始化不需要这些业务能力，所以不自动加入 Cargo、不生成占位 service；在终端下游先确认用途和依赖必要性，再按已批准范围声明最小 features。core 可使用与业务有关且不绑定 GUI 的库；runtime 装配、文件存储、网络客户端和资源加载由适配器拥有。

| 库 | 示例锁文件版本与 features | 基础用途 |
|---|---|---|
| `tokio` | `1.53.2`；`macros,rt-multi-thread,time,io-util,sync` | 实际异步 I/O 需要时，由端侧拥有一个 runtime；GPUI loop 始终在同步主线程，壳层不需要 Tokio |
| `serde` | `1.0.229`；`derive` | 结构化数据序列化；纯简单偏好仍可使用标准库 |
| `serde_json` | `1.0.151` | JSON 协议/持久化；不在 UI 渲染中进行解析或 I/O |
| `jiff` | `0.2.38` | 日期、时间和时区计算 |
| `num-bigint` | `0.5.1` | 确有任意精度整数需求时采用 |
| `num-traits` | `0.2.19` | 数值转换与泛型运算，按算法需求采用 |
| `dirs` | `7.0.0` | 需要超出现有标准库目录解析的跨平台用户目录时采用 |
| `futures-util` | `0.3.34`；关闭默认 features，`sink` | 实际 stream/sink 异步组合所需的最小工具集 |
| `rust-embed` | `8.13.0`；`debug-embed` | 多文件静态资源嵌入；少量固定字节优先 `include_bytes!` |
| `png` | `0.18.1`；示例仅 macOS | 确有运行时 PNG 解码需求时采用；当前托盘模板不需要 |
| `tempfile` | `3.27.0` | 需要自动回收且安全创建的临时文件时采用，测试用途放 dev-dependencies |
| `tokio-tungstenite` | `0.30.0`；关闭默认 features，`handshake` | 明确 WebSocket 需求时采用；示例此配置不提供 TLS，`wss` 需另核对 TLS features |
| `reqwest` | `0.13.5`；关闭默认 features，`json,rustls,system-proxy` | 明确 HTTP/JSON 需求时采用；不凭初始化产生网络请求 |

新增或提高直接版本下界必须按工程规则在最低工具链完成最低直接版本解析与非空测试。不要把示例完整锁文件、未使用的传递依赖或业务配置复制进下游。后续升级先核对官方文档与真实兼容结果，再调整本表及对应 renderer；不能只改文案。

原生 API 来源：[GPUI Kit 通知](https://gpui-kit.com/docs/system-notification/)、[tray-icon](https://docs.rs/tray-icon/0.26.1/tray_icon/)、[auto-launch](https://docs.rs/auto-launch/0.6.0/auto_launch/)、[interprocess](https://docs.rs/interprocess/2.4.4/interprocess/)、[global-hotkey](https://docs.rs/global-hotkey/0.8.0/global_hotkey/)。

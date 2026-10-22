# 诡秘神谕 · LokiMetris

**移动展示终端：[LokiMetisMobileDisplay](https://github.com/ManonLoki/LokiMetisMobileDisplay)** —— 在 Android、iPhone、iPad、Apple Watch 和小组件上查看电脑数据。

LokiMetris 是面向 Windows、macOS 与 Linux 的原生桌面应用，用来查看 AI 编程工具的 Token 用量、账户额度与 Agent 活动。它把用量看板、Hooks 监控、桌宠、应用换肤和移动设备联动放在一起，使用 Rust 与 GPUI 构建，无需 WebView。

[下载桌面版](https://github.com/ManonLoki/LokiMetis/releases/latest) · [查看版本更新](https://github.com/ManonLoki/LokiMetis/releases)

## 功能

### 用量看板与账户额度

| 工具 | 用量来源 | 账户额度 | 应用换肤 |
| --- | --- | --- | --- |
| Codex | 本机会话记录 | 支持 | macOS / Windows |
| Claude Code | 本机会话记录 | 读取本地 statusline 额度记录 | — |
| Cursor | 官方账户用量接口 | 支持 | — |
| Grok | 本机会话记录 | 支持 | — |
| WorkBuddy | 本机项目会话与 Trace 记录 | — | macOS / Windows |

- **六种时间范围**：今日、昨日、本周、上周、本月、上月。
- **Token 明细**：总量、输入、输出、缓存读写、推理输出与缓存比例；可复制精确数值。上游没有提供的分项显示为“未提供”。
- **统计与图表**：查看每日趋势，按模型、项目、会话、数据根分组；Codex、Claude Code 还可按推理强度查看。
- **调用记录**：在“全部”中按模型、推理强度、项目、会话、数据根筛选，支持排序和分页加载。
- **来源管理**：自动发现本地来源，也可手动添加目录、启停来源或设置别名；同时显示数据新鲜度、完整性与可信度。WorkBuddy 读取固定的 `~/.workbuddy`、`~/.workbuddy-ai` 项目目录。
- **WorkBuddy 专项统计**：会话与请求、Token 与积分、子代理请求，以及 Trace 错误率、取消数和平均时长。
- **账户额度**：按来源提供的周期显示剩余比例、套餐与重置时间，包括可用的五小时、周或账单月额度。

本机 Token 总量汇总 Codex、Claude Code、Grok 与 WorkBuddy。**Cursor 使用独立的账户用量视图，不并入本机总量**；查询使用电脑上已有的 Cursor 登录态，需要本机可用的 `curl` 与 `sqlite3`。

额度信息依赖对应工具的本地记录或现有登录态。Codex、Cursor、Grok 在需要时查询官方接口；Claude Code 读取本地 statusline 的 `rate_limits`。来源没有提供额度时不显示对应卡片。

### Hooks 监控与桌宠

支持 **Codex、Claude Code、Cursor、Grok、WorkBuddy** 的 Hooks 接入，展示空闲、运行中、询问、出错四种状态。

- 为每个 Agent 选择一个展示位（1～12 号），分别配置四种状态的文字和图片。
- 图库支持 JPEG、PNG、GIF、BMP、WebP 的导入、预览、筛选与删除，支持 GIF、APNG、动态 WebP 动画。
- 独立桌宠浮窗显示 Hooks 图文和今日 Token 摘要，支持翻页，以及 1×1、1×2、2×1、1×3、3×1、2×2 六种布局。
- 桌宠可调整尺寸、置顶、锁定位置与尺寸、开启鼠标穿透，并保存布局与位置。
- 系统通知可提示用量里程碑、Agent 询问、错误、完成，以及换肤结果。

### 应用换肤

在 macOS、Windows 上为 **Codex 与 WorkBuddy** 应用皮肤。

- 六套内置皮肤：我的世界、雾野晨光、森灵晨辉、Pikachu、田园风光、银心远航。
- 浏览、搜索与预览皮肤，创建用户主题。
- 导入前检查 ZIP 内容，导入皮肤或导出用户资源包。
- 选择宿主应用实例，应用或停用皮肤；支持将旧皮肤转换为纯主题。

内置皮肤保留在应用中，不能删除或导出。Linux 当前不支持 Codex 与 WorkBuddy 换肤。

### 移动设备联动

配合 [LokiMetisMobileDisplay](https://github.com/ManonLoki/LokiMetisMobileDisplay)，在手机、平板、Apple Watch 和小组件上查看当日 Token、账户额度与 Hooks 状态。手机和平板还可同步状态文字与图片。

- 同一局域网内自动发现电脑，首次使用四位码配对，之后自动认证重连。
- 已配对设备页显示设备名称、平台、版本、在线状态与最近通信时间；移除设备会撤销本机配对并断开连接。
- Apple Watch 通过已配对的 iPhone 完成联动，桌面可查看对应 Watch 的在线与同步状态。
- 小组件使用独立只读权限获取用量、额度与活动摘要，后台更新受移动系统调度影响。

移动端的安装、设备要求、小组件与 Android 悬浮窗说明见[移动展示终端 README](https://github.com/ManonLoki/LokiMetisMobileDisplay#readme)。

### 日常设置

支持简体中文与 English、浅色/深色/跟随系统主题、Agent 显示开关、系统通知、开机自启，以及托盘中的主窗口和桌宠控制。用量扫描间隔可设为 1～1440 分钟，派生统计保留时间可设为 1～3650 天，默认分别为 5 分钟和 90 天。

## 下载与安装

从 [GitHub Releases](https://github.com/ManonLoki/LokiMetis/releases/latest) 下载对应平台的安装包：

| 平台 | 架构 | 格式 |
| --- | --- | --- |
| Windows | x64 | `.exe` 安装程序 |
| macOS | Apple Silicon（ARM64） | `.dmg` |
| Linux | AMD64 / ARM64（GNU） | `.deb`、`.tar.gz` |

Linux 的运行库要求以安装包声明为准；桌宠需要 X11 / XWayland，纯 Wayland 当前不支持桌宠所需的全局定位与置顶。托盘显示取决于桌面环境是否提供 StatusNotifierItem 支持。

## 开始使用

1. 打开应用，在设置中选择要显示的 Agent，查看自动发现的用量来源；需要时添加对应会话目录。
2. 在看板切换工具和时间范围，查看 Token、调用记录、统计与图表。账户额度需要对应工具已有可用的本地记录或登录态。
3. 需要活动状态时，在监控页接入对应工具的 Hooks，并为状态配置图文；随后可从托盘打开桌宠。
4. 需要换肤时，在皮肤页选择皮肤与宿主实例，再执行应用。

连接移动设备：

1. 让电脑与手机或平板连接同一局域网，保持桌面应用运行。
2. 在移动端设置中选择自动发现的电脑并发起配对，允许系统请求的本地网络访问。
3. 在电脑自动弹出的配对窗口中，输入移动端显示的四位码。
4. 连接成功后回到移动端展示台；以后打开移动端会自动重连。Watch 沿用 iPhone 的配对。

## 数据存放

Codex、Claude Code 与 Grok 的本机会话统计写入 LokiMetris 自己的派生索引；WorkBuddy 读取本机项目记录，Cursor 查询独立账户用量。统计保留时间只影响派生数据，不删除原始 Agent 会话文件。Hooks 接入会修改对应工具的配置；换肤会改变所选宿主实例的运行时外观。

业务数据默认目录：

| 平台 | 路径 |
| --- | --- |
| macOS | `~/Library/Application Support/app-loki-metris/data` |
| Windows | `%LOCALAPPDATA%/app-loki-metris/data` |
| Linux | `~/.local/share/app-loki-metris` |

账户用量与额度可能访问对应提供商的官方接口；移动展示通过配对后的局域网加密连接同步。应用字体随包内嵌。

## 从源码运行

需要 Rust ≥ 1.98.1、Node.js ≥ 24.21.0、pnpm ≥ 12.4.1，以及当前平台的原生编译工具。Linux 依赖与双架构构建说明见 [Linux GPUI 工程知识库](docs/LINUX_GPUI_TOOLCHAIN.md)。

```sh
git clone https://github.com/ManonLoki/LokiMetis.git
cd LokiMetis
pnpm run dev
```

`pnpm run dev` 会通过 Cargo 启动 GUI，并设置源码中的皮肤资源目录。Node.js/pnpm 仅用于工程脚本，根工程没有 npm 第三方依赖。

macOS 需要完整 App bundle 身份时，可运行 `pnpm run gpui:preview` 准备本地预览 App；该命令不会自动启动 App。

调试时可用绝对路径环境变量 `LOKI_METRIS_DATA_DIR` 指定业务数据目录。它不改变原始会话来源、GUI 偏好目录或系统集成设置。Bash 示例：

```sh
LOKI_METRIS_DATA_DIR="$PWD/target/dev-data" pnpm run dev
```

## 技术与目录

Rust 2024、GPUI / gpui-kit 原生界面，SeaORM + SQLite 存储派生数据，Tokio 运行后台服务，rust-i18n 提供中英界面。

```text
.
├── loki_metris_core/           # 业务规则、用量统计、监控与设备领域逻辑
├── loki_metris_gui/            # GPUI 界面、桌宠、原生系统集成与数据读取
├── loki_metris_pairing/        # 共享配对与加密协议
├── loki_metris_mobile_bridge/  # 移动端 JNI / C ABI 桥接
├── vendor/                    # 原生依赖兼容补丁
├── packaging/                 # 平台打包配置
├── scripts/                   # 开发与打包脚本
├── docs/                      # 技术与使用协议
└── Cargo.toml                 # 工作区、依赖与当前版本
```

进一步阅读：[技术选型与依赖](docs/TECHNOLOGY.md)、[Rust / GPUI 运行时与命令](docs/RUST_CLI_TEMPLATE.md)、[Hooks 协议](docs/HOOKS_PROTOCOLS.md)、[设备联动协议](docs/DEVICE_LINK_PROTOCOL.md)、[已知问题](docs/TECH_DEBT.md)。

## 许可

本项目使用企业专有商业许可证：[`LICENSE.zh-CN.md`](LICENSE.zh-CN.md) 与 [`LICENSE.en.md`](LICENSE.en.md)。字体许可与来源见 [`loki_metris_gui/assets/fonts/`](loki_metris_gui/assets/fonts/)，第三方许可不被本项目许可覆盖。

## 赞助

如果 LokiMetris 对你有帮助，可以使用微信支付或支付宝扫码赞助。

| 微信支付 | 支付宝 |
| --- | --- |
| <img src="loki_metris_gui/assets/sponsor/wechat-pay.png" alt="LokiMetris 微信支付赞助收款码" width="300"> | <img src="loki_metris_gui/assets/sponsor/alipay.jpg" alt="LokiMetris 支付宝赞助收款码" width="300"> |

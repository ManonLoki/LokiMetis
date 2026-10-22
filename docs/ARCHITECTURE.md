# 当前桌面架构

[打开 Archify 交互图](../.archify/architecture-desktop-20261022-144530/desktop.html) · [查看图稿](../.archify/architecture-desktop-20261022-144530/candidate.json)

![当前桌面架构预览](../.archify/architecture-desktop-20261022-144530/visual-check/desktop.visual-check.2048x1320.light.png)

HTML 为独立页面，可下载后在浏览器打开。图展示主要运行关系，节点附有固定提交的源码位置；箭头标注操作方向与协议，不表示全部模块 import 依赖。

源码基线：`950d9eb0cf39b9f57f08a98cdd45c81dd8effac1`；梳理日期：2026-10-22。

桌面进程拥有 GPUI 主线程、应用级 Tokio 运行时及受限后台 owner；隐藏主窗口不销毁这些服务。core 不依赖 GUI，但其 SQLite 持久层确实执行 I/O，不能把整个 crate 描述为纯函数。

## 主要链路

**用量主链**：GPUI 经 UsageService 邮箱发起查询或扫描；后台适配器发现并读取会话文件，core 持久层负责 SQLite 连接、事务和查询，再由纯业务函数生成六种时间窗口的页面模型。

**独立来源**：WorkBuddy 直接读取固定项目 JSONL / Trace，不进入三类会话索引。Cursor 账户用量单独查询官方接口并保存在内存，不并入本机 Token 总量。

**活动与桌宠**：Agent 的 Hook 经本程序 relay 发到环回 HTTP listener；core 推进状态机，监控页、桌宠、通知和设备发布器各自消费结果。桌宠按页布局投影，手机发布器不读取桌宠当前页。

**换肤**：皮肤服务在后台选择并验证 Codex / WorkBuddy 宿主实例，借助 CDP 注入和监视运行中的页面；此路径支持 macOS / Windows。

**移动联动**：设备服务随应用启动，mDNS 只提供发现信息；四位码配对保存信任后，每次 Resume 再认证。TCP 负责图文与资源块；独立只读 HTTP 只返回 Token、额度和状态摘要。

## 模块与源码

| 子系统 | 直接证据 |
| --- | --- |
| GPUI 界面与原生宿主 | [服务装配与窗口](../loki_metris_gui/src/main.rs#L80) · [桌宠读取投影](../loki_metris_gui/src/pet/mod.rs#L201) · [原生通知消费](../loki_metris_gui/src/native/notifications/activity.rs#L10) |
| UsageService | [后台 owner 与任务](../loki_metris_gui/src/usage_service/mod.rs#L153) · [串行 writer 扫描](../loki_metris_gui/src/usage_service/scan.rs#L27) · [请求与异步回执](../loki_metris_gui/src/app/dashboard/mod.rs#L188) |
| 本机来源适配器 | [索引与来源发现](../loki_metris_gui/src/backend/scan_orchestration/run.rs#L67) · [解析与批次写入](../loki_metris_gui/src/backend/local_index/rollout_ingest.rs#L201) · [独立读取与核算](../loki_metris_gui/src/backend/workbuddy/mod.rs#L125) |
| AI 工具本机记录 | [实际打开会话文件](../loki_metris_gui/src/backend/local_index/file_source.rs#L139) · [项目 JSONL 文件](../loki_metris_gui/src/backend/workbuddy/jsonl/mod.rs#L282) |
| core SQLite 持久层 | [数据库连接及迁移](../loki_metris_core/src/local_index/mod.rs#L59) · [批次事务提交](../loki_metris_core/src/local_index/call_store/mod.rs#L61) · [查询与标准化](../loki_metris_core/src/local_index/snapshot_usage_reads.rs#L157) |
| 应用私有 SQLite | [真实文件与连接 I/O](../loki_metris_core/src/private_sqlite.rs#L33) · [三类数据库槽位](../loki_metris_core/src/client_ports.rs#L13) |
| core 业务规则 | [逻辑调用归并](../loki_metris_core/src/aggregate/canonicalization.rs#L51) · [时间窗口与核算](../loki_metris_core/src/local_view.rs#L113) · [Hook 状态机](../loki_metris_core/src/agent_hooks/state_machine/mod.rs#L109) |
| 账户用量与额度驱动 | [Cursor 分页与缓存](../loki_metris_gui/src/usage_service/cursor_usage/mod.rs#L75) · [本地优先与纯解析](../loki_metris_gui/src/usage_service/provider_quota/fetch.rs#L36) · [受限 HTTPS 与只读 sqlite3](../loki_metris_gui/src/usage_service/provider_quota/transport.rs#L73) |
| 提供商官方接口 | [固定官方端点](../loki_metris_gui/src/usage_service/provider_quota/fetch.rs#L112) · [curl 调用与超时取消](../loki_metris_gui/src/usage_service/provider_quota/transport.rs#L118) |
| Hooks 与换肤服务 | [状态推进与活动通知](../loki_metris_gui/src/monitor/listener/lifecycle.rs#L101) · [配置 writer owner](../loki_metris_gui/src/usage_service/hooks.rs#L98) · [宿主页注入](../loki_metris_gui/src/skins/injection.rs#L69) |
| Agent 与宿主应用 | [环回 HTTP 中继](../loki_metris_gui/src/monitor/relay.rs#L440) · [CDP 连接与宿主](../loki_metris_gui/src/skins/cdp_connect.rs#L17) · [换肤平台边界](../loki_metris_gui/src/skins/platform/mod.rs#L348) |
| 局域网设备服务 | [启动、发现与连接分流](../loki_metris_gui/src/usage_service/devices/server.rs#L114) · [用量、额度与全槽 Hooks](../loki_metris_gui/src/usage_service/devices/publication.rs#L76) · [配对账本落盘](../loki_metris_gui/src/usage_service/devices/store.rs#L6) |
| 移动展示与只读组件 | [TCP 图文与只读授权](../loki_metris_gui/src/usage_service/devices/server.rs#L280) · [认证只读 HTTP 摘要](../loki_metris_gui/src/usage_service/devices/widget_http.rs#L23) · [调用共享配对协议](../loki_metris_core/src/devices/pairing.rs#L5) |

## 图稿与检查记录

- [最终生成回执](../.archify/architecture-desktop-20261022-144530/review-2/desktop.finalize.json)与[浏览器检查](../.archify/architecture-desktop-20261022-144530/review-2/desktop.browser-check.json)：Showcase 9/9，零错误、零警告。
- 已查看 1440×900、2048×1320 的浅色和深色截图；小窗口采用页面纵向滚动。
- [截图与感知复核记录](../.archify/architecture-desktop-20261022-144530/REPORT.md)。
- 图稿 SHA-256：`15e3d34aca6ffd1e97ba5a4a6bece5c526a808a85e1692783c594f0a5d01593c`。
- HTML SHA-256：`4663994d0a059b0a60d564a1199f9db4b774e51dea439651d8f6569cfcc41c84`。

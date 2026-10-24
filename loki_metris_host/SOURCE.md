# 本机扫描宿主来源

`local_index/` 与 `workbuddy/` 于 2026-10-09 按用户明确请求从同一负责人 ManonLoki 的历史 LokiMetis `loki_metis_gui/src-tauri/src/backend/` 迁入。保留解析、文件身份校验、发现预算及回归，crate 标识改为当前 core，Tauri 的 blocking/runtime 入口替换为项目拥有的 Tokio。

测试使用临时目录或隔离 runtime。来源不包含用户会话、凭据、数据库或旧 App 配置；旧源码路径不是运行时依赖。Native 服务只读原始 Agent 数据，并写入 LokiMetris 独立派生目录。新增依赖由当前会话的用户明确授权。

`scan_orchestration/` 于同日从历史 `src-tauri/src/scan_orchestration/` 提取已登记来源扫描、后台候选激活与原子重新索引流程。保留文件身份、预算、禁用根过滤、增量/重建与错误语义；移除 Tauri 与 AppRuntimeState 耦合，以当前产品拥有的 Tokio、单 writer 和显式 retention 注入替代。

2026-10-24 起上述 `local_index/`、`workbuddy/`、`scan_orchestration/` 及同目录的文件戳、来源目录与通知状态驱动从 GUI crate 的 `backend` 模块整体迁入独立 crate `loki_metris_host`，内容与行为不变；GUI 经本 crate 公开 API 调用，依赖方向固定为 gui → host → core。

2026-10-25 起 Codex、Claude、Grok 与 WorkBuddy 的逐行解析器（只接收任意 `Read` 字节流）迁入 core 的 `session_logs` 模块，本 crate 保留发现、文件打开与身份校验、字节预算、解析缓存和扫描编排；依赖方向改为 service → host → core。

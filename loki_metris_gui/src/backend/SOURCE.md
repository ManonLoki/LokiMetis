# 本机扫描宿主来源

`local_index/` 与 `workbuddy/` 于 2026-10-09 按用户明确请求从同一负责人 ManonLoki 的历史 LokiMetis `loki_metis_gui/src-tauri/src/backend/` 迁入。保留解析、文件身份校验、发现预算及回归，crate 标识改为当前 core，Tauri 的 blocking/runtime 入口替换为项目拥有的 Tokio。

测试使用临时目录或隔离 runtime。来源不包含用户会话、凭据、数据库或旧 App 配置；旧源码路径不是运行时依赖。Native 服务只读原始 Agent 数据，并写入 LokiMetris 独立派生目录。新增依赖由当前会话的用户明确授权。

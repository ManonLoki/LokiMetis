# 诡秘神谕 · LokiMetris

ManonLoki 的 Windows / macOS 原生 Rust 桌面项目，使用 GPUI。当前目标是参照历史 LokiMetis 源码和已安装 App 复刻用量看板、AI 监控/桌宠、应用换肤与原生交互；当前进度和未完成范围见 [持续计划](docs/work_plan/20261009_work_plan.md)。整体体验复刻尚未完成。

## 当前实现

- `loki_metris_core`：纯 Rust 领域核心，提供 Token/时间口径、SQLite 索引和迁移、调用/统计/图表查询、来源策略、Hooks/监控/桌宠及皮肤校验。
- `loki_metris_gui`：GPUI 原生主壳、真实本机用量概览、六个自然日窗口、客户端切换、来源列表、扫描进度/取消、精确值复制；现有设置和关于页仍在向参考产品迁移。
- Codex、Claude Code、Grok 读取本机会话并写当前 App 自己的派生索引；WorkBuddy 只读其两个固定根的 project JSONL，联合视图遵循 core 核算口径。
- 所有新增依赖在当前 Cargo 工作区声明；字体随 App 内嵌，不进行全局包或字体安装。历史源码目录和已安装参考 App 都不是运行时依赖。

监控、桌宠、完整统计/图表/调用交互、来源管理、换肤和公共设置仍按阶段实现。core API 存在不代表这些原生界面已完成。当前保留已选金色神谕之眼 Logo 和独立 `com.loki-metris.app` 身份；不会覆盖参考 App。

## 开发

Rust >= 1.98.1、Node.js >= 24.21.0、pnpm >= 12.4.1。根工程入口无 npm 第三方依赖。版本以根 Cargo 为准；当前忽略本地生成锁文件。

```sh
cargo run -p loki_metris_gui
cargo test -p loki_metris_core --lib
cargo test -p loki_metris_gui --bin loki_metris_gui
```

macOS 默认派生数据位于 `~/Library/Application Support/app-loki-metris/data`。验证时可用绝对路径环境变量 `LOKI_METRIS_DATA_DIR` 隔离派生库；它不会改变原始 Agent 目录，也不会复制历史 App 数据库。初次扫描较大的会话库需要时间，进度与取消由应用拥有的有界 worker 提供。

```sh
LOKI_METRIS_DATA_DIR="$PWD/target/parity-data" cargo run -p loki_metris_gui
```

## 工程边界

本仓库是独立终端下游，不再派生项目。[批准规格](docs/product_spec/20261009_product_spec.md)、[GUI 资料](docs/GUI_APP_PROFILE.md)、[Agent Policy](docs/AGENT_POLICY.md) 为当前事实入口。工程由 Harness 提交 `970448cb6e5da05e6ece9c9317f8c951f57b5911` 的选择性文件快照初始化，未迁入上游 Git 历史；首次升级需按既有规则审计基线，不能伪造溯源锁。

阶段 Computer Use 对照记录保存在忽略的 `release/gpui-parity/`。调试预览不构成安装包或最终候选验收；Windows 运行与安装包尚无本次证据。发布、推送和分发遵守独立流程。

完整商业许可证：[中文](LICENSE.zh-CN.md)、[English](LICENSE.en.md)。字体许可与来源在 `loki_metris_gui/assets/fonts/`；第三方许可不被本项目许可覆盖。

# GPUI 工程候选

仅根 Cargo 已选 GUI 且框架为 gpui 时完整读取；共同保护、身份渲染和机器匹配仍执行 [所有权策略](ownership-policy.md)。

`$desktop-add-gpui-adapter` 与 `$desktop-build-gpui-release` 只适用于 `gui-framework = "gpui"` 的 GUI 下游，分别传播原生 Rust 模板和独立本机 macOS `.app`/DMG、Windows NSIS 打包管线，不注入 Tauri/WebView、Mantine、React 或 Tauri 插件；两种 GUI 都使用 Node.js/pnpm 工程工具。`$desktop-prepare-gui-app-identity` 同样适用。Tauri 与非 GUI 下游不得接收 GPUI 打包 Skill。

## 保留与删除

GPUI 目录内的 `scripts/create_gpui_workspace.mjs`、`scripts/create_gpui_workspace.test.mjs` 与 `assets/core/**` 同为初始化专用 `tombstone`，必须先于该目录的 `conditional` 规则。终端下游保留受限 `scripts/merge_gpui_node_tooling.mjs`、纯渲染库 `scripts/gpui_node_tooling.mjs`、`scripts/gpui_validate.mjs` 及其测试、纯渲染库 `scripts/gpui_adapter_files.mjs`、既有 core 的 GUI 增量入口 `scripts/add_gpui_adapter.mjs` 及其测试和 GUI/品牌资产；升级不恢复首次创建工作区或派生 core 的入口。

GPUI 项目本地 `.agents/skills/gpui-kit/**` 与 `.agents/skills/gpui-kit-design-guides/**` 是 protected 第三方文档，不进入候选或来源锁，不由升级覆盖/安装；仅新建 GPUI 下游在初始化阶段安装固定来源快照。

GPUI 专属的 `$desktop-manage-gpui-dependencies`、`$desktop-optimize-gpui-performance` 与中性 `docs/GUI_ENGINEERING.md` 为 conditional，仅向根 Cargo 已选 gpui 的 GUI 下游传播；Tauri/非 GUI 候选出现时阻断。依赖治理不默认升级；性能知识不附带产品数据/预算。GPUI 打包额外包含 Linux GNU x64/ARM64 deb/tar.gz，是否接线桌面能力仍由产品事实决定。

## 项目根工程脚本

根 `package.json` 继续是 protected 产品文件，不进入升级候选，也不由 `plan|apply|record` 写入。GPUI 工程入口迁移属于本次已授权范围时，在工程候选收敛后运行目标项目保留的 `node .agents/skills/desktop-add-gpui-adapter/scripts/merge_gpui_node_tooling.mjs merge --root .`；该专用工具仅补齐缺失的工程 scripts/engines，保留其他字段与已有值，同名冲突、不同兼容要求或危险文件节点在任何写入前失败。它不降低已有工具要求、不新增 npm 依赖、不运行安装或发布；随后实际运行 `pnpm run validate` 与 `pnpm run release:inspect` 核对入口。未在本次范围迁移时报告旧入口状态，不能把升级器同步 Skill 说成已接入项目根脚本。

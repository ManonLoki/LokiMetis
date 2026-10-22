# 文档导航

使用入口见[根 README](../README.md)。本目录按用户操作、当前实现、产品要求和历史证据组织；版本、依赖、平台与工程命令分别回到对应清单核对。

## 使用与当前实现

| 内容 | 入口 | 核对来源 |
| --- | --- | --- |
| 功能、安装、真实截图与操作教程 | [README](../README.md) | GPUI 实际导航、界面文案及对应操作 |
| 运行链路、模块分工与交互图 | [桌面架构](ARCHITECTURE.md) | 四个 workspace 成员及 GUI 后台服务；图内源码链接保留其标注的提交基线 |
| 依赖、features、平台条件与兼容包 | [技术清单](TECHNOLOGY.md) | 根和各成员 Cargo.toml、package.json、vendor 维护入口 |
| 运行时、退出所有权与开发命令 | [Rust / GPUI 工程基线](RUST_CLI_TEMPLATE.md) | main、UsageService、原生 owner 与根 scripts |
| 界面约定与原生能力 | [GUI profile](GUI_APP_PROFILE.md)、[支持界面与出站入口](GUI_SUPPORT_SURFACES.md) | app、pet、monitor、native 及对应服务 |
| Hooks 接入与状态解释 | [Hooks 协议](HOOKS_PROTOCOLS.md) | core 的协议/状态机与 GUI 配置写入、relay、监听器 |
| 配对、加密会话、图文与小组件 | [配对协议](DEVICE_PAIRING_PROTOCOL.md)、[联动协议](DEVICE_LINK_PROTOCOL.md)、[技术选型](DEVICE_PAIRING_TECHNOLOGY.md) | pairing、core/devices、GUI 设备驱动与移动桥 |
| Linux 工具链和宿主限制 | [Linux 知识库](LINUX_GPUI_TOOLCHAIN.md) | 平台代码、打包脚本与标明版本/架构的历史证据 |
| 本地打包与正式渠道 | [发布规则](RELEASE.md)、[GitHub Actions](GITHUB_RELEASE.md) | 打包入口、release 工作流与冻结的发布上下文 |
| 已知缺口与解除条件 | [技术债](TECH_DEBT.md) | 仍未解决的实现或实物验证边界 |

`loki_metris_mobile_bridge` 导出共享 C ABI；JNI/Swift 调用侧和手机原生驱动位于外部移动项目。本仓库源码、单元回归与移动端实物互通证据分别核对。

## 产品要求与工程规则

- [产品规格索引](product_spec/README.md)：最新日期文件保存当前批准的目标、边界与成功标准，不能把目标全部视为已经实现。
- [当前开发计划](work_plan/README.md)：持续体验收敛的未完成范围；源码存在或局部回归通过不自动完成阶段。
- [Agent 入口](../AGENTS.md)、[持久策略](AGENT_POLICY.md)、[工程规则](ENGINEERING_RULES.md)：维护路由、已确认选择及约束。
- [GUI 工程索引](GUI_ENGINEERING.md)、[设计标准索引](design_standards/README.md)：按实际框架选择职责；本产品使用 GPUI。
- [兼容层总览](../vendor/docs/VENDOR_OVERVIEW.md)：区分原样 upstream、人工补丁、生成树与包内回归。

## 历史与证据

- [ADR](adr/README.md)保存决定及其当时依据；后续决定以原文中的明确承接关系解释。
- [Changelog](changelog/README.md)保存变化发生时的记录；历史 `required_version` 是最低所需版本。
- [渠道验证索引](VERIFICATION.md)记录对应发布的实际范围；构建、签名、运行和完整验收各有边界。
- [性能手册](performance/README.md)与[完整参考](PERFORMANCE_OPTIMIZATION.md)保留历史实验条件和失败结果，不能作为当前版本的性能承诺。
- `release/`、`target/` 中的本地证据不随源码分发。文档中的历史路径用于定位原始保留集合；缺失时不能补造通过记录。

当前指南与实现发生漂移时，应追到具体代码、配置和测试一起校准；原始历史测量、失败与人工复核事实继续保留。

# GPUI 工程方案索引

本文按任务组织 GUI 依赖版本、实现方案、性能和打包知识。根 Cargo 决定实际框架/接口/目标/版本；中性 renderer、已有产品接线与目标运行证据分别判断。普通实现只读命中主题，不自动初始化、升级依赖、跑基准或构建。

| 任务 | 入口与事实 | 完成边界 |
| --- | --- | --- |
| 依赖清单、版本或 features | [$desktop-manage-gpui-dependencies](../.agents/skills/desktop-manage-gpui-dependencies/SKILL.md)；其中依赖矩阵记录中性声明下界和已有采用参考 | 分清声明/resolved/vendor，实际升级才改清单与回归 |
| 后端兼容、字体/AX/窗口/托盘问题 | [兼容补丁合同](../.agents/skills/desktop-manage-gpui-dependencies/references/compatibility.md) | 原样来源 + 严格生成 + 按后端证据，不默认复制补丁 |
| 已有 core 增加 GUI、原生接线 | [$desktop-add-gpui-adapter](../.agents/skills/desktop-add-gpui-adapter/SKILL.md) | 按 profile 装配，禁用能力无残留；不复制业务权威状态 |
| 异步/队列/重绘/缓存/图片性能 | [$desktop-optimize-gpui-performance](../.agents/skills/desktop-optimize-gpui-performance/SKILL.md) | 定场景、测成本、最小修改、行为回归、同条件复测 |
| 本地试包与发布候选 | [$desktop-build-gpui-release](../.agents/skills/desktop-build-gpui-release/SKILL.md) | 公共生命周期 + 按平台引用，产物与运行验收分开 |
| 最终候选验收 | 保留的 desktop-verify-delivery / desktop-test-final-artifact-e2e | 真实最终字节和目标宿主，构建通过不代替验收 |

## 方案选择

原生 GPUI/Kit 负责视图与图标，rust-i18n/sys-locale 负责语言，端侧日志使用有界后台 sink。宿主依赖按 target/profile 裁剪。业务确需才加 Tokio、图片解码、数据库或网络；Tokio 服务拥有 runtime，GPUI loop 仍在同步主线程。renderer 未接线的 Linux 能力保持限制，产品可以通过独立实现扩展，不能反向宣称模板已支持。

性能优化优先减少无消费者的工作、合并最新意图、限制并发/驻留和完善失效。CPU像素、GPU纹理、Scene及任务分别拥有寿命；API/微基准、整应用短窗、长期及跨平台结论分层，不以固定产品阈值替代测量。

macOS 使用 app.zip/DMG，Windows x64 使用 NSIS；两者核对全局 cargo-packager 0.11.8。Linux GNU x64/ARM64 使用系统 tar/dpkg-deb/readelf，可同系统交叉架构。独立 CLI 全局安装，产品 Rust/Node 依赖仍按项目清单；本索引不授权安装。local 不发布，candidate 只消费已发布源码与当次选择；签名、解包、运行各自取证。

## Skills 维护与传播

adapter 只负责装配，依赖 Skill 管版本/补丁，性能 Skill 管定位/测量，build Skill 管产物。平台是 build 的按需 reference，不另建重复生命周期的 Skill。新增两个 GPUI Skill 仅向 Cargo 已选 gpui 的下游传播；Tauri 与非 GUI 不接收。共享规则通过索引引用，产品参数和历史性能证据留在产品文档。回流 Harness 定点合并中性工程，不覆盖产品源码/身份/策略或其他新工具，也不伪造完整升级来源锁。

## 本项目事实与边界

当前依赖全表见 [TECHNOLOGY](TECHNOLOGY.md)，Linux 产品接线/工具链见 [LINUX_GPUI_TOOLCHAIN](LINUX_GPUI_TOOLCHAIN.md)。性能现状、历史测量与被否定的尝试见 [PERFORMANCE_OPTIMIZATION](PERFORMANCE_OPTIMIZATION.md) 和 [性能知识库](performance/README.md)；历史 macOS 合成短窗 A/B 不代表当前版本或 Windows/Linux 复测。具体候选阻断取 [RELEASE](RELEASE.md)。本次只调整工程知识与打包工具，不改产品 Rust 依赖声明。

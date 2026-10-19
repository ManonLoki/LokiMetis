---
name: desktop-manage-gpui-dependencies
description: 核查或调整 GPUI Rust GUI 的依赖声明、平台 features 与本地兼容补丁；用于依赖整理、升级或版本冲突，不因普通界面开发自动升级。
---

# 管理 GPUI 依赖与兼容方案

先确认根 Cargo 的 GUI 框架为 `gpui`。本 Skill 管依赖与补丁决策；已有 core 的 GUI 装配交 `$desktop-add-gpui-adapter`，成本问题交 `$desktop-optimize-gpui-performance`，安装包交 `$desktop-build-gpui-release`。沿当前开发分支与版本流程完成，不另建 Task 或重复提交周期。

1. 读取根与受影响成员 Cargo、锁策略、存在时的 `docs/TECHNOLOGY.md`，再读 [版本与能力矩阵](references/dependency-matrix.md)。分别记录声明范围、当次 resolved 版本和 path/git/vendor 来源；缺少解析证据就保持未知。Cargo 裸版本通常为兼容范围，只有精确要求才称 pin。
2. 纯整理只修事实和路由，不升级、安装或重生成 GUI。实际新增或提高下界时，按已授权用途及工程规则核对官方版本文档、MSRV、平台 features、最低直接版本解析和本次非空回归；上游存在通用 `$desktop-manage-dependencies` 时复用其流程。
3. core 保持框架无关。GUI member 继承 workspace 依赖，宿主库用 target 条件，`test-support` 留在 dev-dependencies。组件库、异步 runtime、图片、数据库和网络分别有真实消费者才加入；示例完整依赖表不是安装清单。
4. 涉及 GPUI 后端、字体、可访问性、窗口或托盘的本地兼容层时，按 [兼容补丁合同](references/compatibility.md)核对整条版本/ABI/来源链。升级成功不能仅凭根 crate 编译或一个后端的测试推断。
5. 报告实际改变、未改变的产品能力、版本范围与补丁适用条件、本次测试及未验证平台。只读整理不声称已完成依赖升级。

用户要求回流上游 Harness 时，先确认该源的唯一 Git 根、规则、分支和已有修改，再定点写中性知识及可复用工具；保留两边独有机制。`desktop-upgrade-harness` 是下游升级器，不是反向覆盖器；没有全工程来源审计就不创建或刷新来源锁。产品源代码、业务参数和验收记录不搬入模板源。

Rust 运行与开发依赖属于项目清单。Node.js、pnpm、cargo-packager、7-Zip 等独立 CLI 按用户的全局工具策略安装；安装授权与安装位置分开判断，安装后核对真实路径与版本。本 Skill 本身不安装工具。

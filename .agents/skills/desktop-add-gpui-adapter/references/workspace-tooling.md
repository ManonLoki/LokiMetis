## 根 pnpm 工程入口

[gpui_node_tooling.mjs](../scripts/gpui_node_tooling.mjs) 的 `renderGpuiPackageJson(existingBytes = null)` 是标准库纯函数。新 `package.json` 为 `private: true`，仅声明 `engines.node = ">=24.21.0"`、`engines.pnpm = ">=12.4.1"` 和以下脚本；无 npm 依赖，无需先执行安装。已有 package 的公开性、`packageManager`、用户脚本、依赖和其他字段原样保留，只添加缺失的工程脚本与 engine。非等值同名脚本或 engine（包括更严格范围）、重复 JSON 键、非法内容及链接都在任何写入前失败；不得降低已有要求或新增精确 `packageManager` pin。add-only 在安装前复核 package/Cargo/core 快照并在失败时撤销本次文件。

保留的通用 `gpui_dev.mjs` 监听工作区变更、增量编译并重启 owned 调试进程，包含取消与POSIX进程组回收；相关回归为 `gpui_dev.test.mjs`。Windows leader先退出时的后代回收仍有[已知缺口](../../../../docs/TECH_DEBT.md)，对应真实回归跳过该平台。当前产品 `pnpm dev` 仍使用 `scripts/dev.mjs` 装配实际资源；`pnpm run validate` 通过 `scripts/validate.mjs` 复用下表的 GPUI runner，并定点接入本地封存物/vendor 摘要校验。工程升级不调用受限合并器覆盖这两个已有同名入口。

| 命令 | 实际行为 |
| --- | --- |
| `pnpm run validate` | [gpui_validate.mjs](../scripts/gpui_validate.mjs) 串行汇总保留的文件行数、Rust 中文声明注释和 `check_core_first.mjs --workspace-root .` 工程门禁，返回首个非零状态；不依赖源专属 runner。 |
| `pnpm run test` | `cargo test --workspace --all-targets --all-features`。 |
| `pnpm run release:inspect` | `release_git.mjs inspect --project-root .`，只读工作区快照。 |
| `pnpm run release:notes check --file release-notes.json` | 原更新日志 helper；`upsert`、`render` 及其参数仍遵守原发布 Skill。 |
| `pnpm run release:context check --project-root .` | 原发布上下文 helper；`write`、`verify` 及其必填参数保持不变。 |
| `pnpm run release:git --version <version> --release-context-sha256 <sha256>` | 原 `git_lifecycle.mjs release --project-root .`；只在明确发布授权后调用，版本与上下文摘要门禁不可省略。 |
| `pnpm run gpui:package preview --root .` | 原独立 GPUI 构建 helper；`setup`、`icons`、`build` 与 E2E/签名/候选边界仍由构建 Skill 管理。 |

升级继续把根 `package.json` 视为 protected，不由通用升级器覆盖。确认需要迁移入口后，显式运行 `node .agents/skills/desktop-add-gpui-adapter/scripts/merge_gpui_node_tooling.mjs merge --root .`；该受限迁移仅接受已有 GPUI workspace 和实际保留工具，只写 package，冲突零写入，重复调用保留文件原字节。pnpm 参数直接跟在脚本后，不添加额外 `--` 分隔。

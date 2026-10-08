# 诡秘神谕 · LokiMetris

由 ManonLoki 负责的 Windows / macOS 原生 Rust 桌面项目，标识 `loki_metris`，初始版本 `0.1.0`。当前是中性 GPUI 工程，产品功能尚未定义。

## 当前工程

- `loki_metris_core`：无第三方依赖的共享核心，明确返回产品未定义状态。
- `loki_metris_gui`：GPUI Kit 原生界面，精简侧栏、设置、关于页、中英文、浅色/深色/跟随系统。
- 已选托盘、开机自启能力、单实例、全局快捷键能力。自启默认不注册，热键合同为空，无默认动作/系统键位。
- 未启用通知、赞助页、深链接。GPUI 当前无 updater 服务及通用原生 dialog；更新检查保持零出站。
- 用户选择 candidate-1 金色神谕之眼 Logo，完整事实见 [GUI 应用资料](docs/GUI_APP_PROFILE.md)。

## 开发

需要 Rust >= 1.98.1、Node.js >= 24.21.0、pnpm >= 12.4.1。根工程入口无 npm 第三方依赖。当前忽略本地生成锁文件，不使用 `--locked` 或 `--frozen-lockfile`。

```sh
pnpm run test
cargo build --workspace
cargo run -p loki_metris_gui
pnpm run validate
```

`test` 运行全 Rust workspace / targets / features。`validate` 检查文件硬行数、Rust 中文注释与 core-first；日常只按 [Agent 路由](AGENTS.md) 运行必要验证。

## 策略与验证边界

推荐预设已确认：`superpowers: disabled`、`parallel_worktree_subagents: disabled`、`acceptance_smoke: enabled`、`e2e_hint: disabled`。

`user_owned_tasks: disabled`：不自动创建左侧 Task。`post_release_action: local_package`：未来 Git 发布后本地打包，每次候选仍独立解析 E2E；完整语义见 [Agent Policy](docs/AGENT_POLICY.md)。

macOS Apple Silicon 初始化已通过 26 项 Rust 测试和实际 debug 构建。Computer Use 检查了 Logo/双语/三态主题/侧栏/设置/关于页、窗口偏好有效恢复及损坏回退；自启实测开启、复读、关闭；二次启动恢复已有窗口且第二进程退出。菜单栏操作由用户本机确认托盘可见、恢复和退出正常，正常退出后的偏好/日志刷新已检查。测试创建的登录项、偏好和日志已恢复至执行前状态。Windows 编译、运行与安装包尚未验证。这些是中性初始化检查，不构成产品或发布候选验收。

本次未创建安装包、发布 tag、远端或渠道分发；后续试包/候选按 [发布规则](docs/RELEASE.md) 与 `$desktop-build-gpui-release` 执行。

## 工程来源与下一步

工程由 Harness 源提交 `970448cb6e5da05e6ece9c9317f8c951f57b5911` 的选定文件快照初始化，再按 GPUI 配置裁剪并替换项目身份。412 份原始快照文件已逐项与该提交的 Git blob 核对一致；其中选择性复制 214 份、未复制 198 份，另有 24 项在快照前排除。排除项包括源 Git/构建输出、Version.md、发布上下文/记录、日期产品记忆、历史验证及上游专用 Python 禁令检查器；实例化/初始化入口在收尾裁掉。源 Git 历史没有迁移，许可证仅替换项目名称。`design-taste-frontend` 及其原始 MIT 许可/来源独立保留。

| 初始化字段 | 已确认事实 |
| --- | --- |
| 中英文名称来源 | 诡秘神谕 / LokiMetris，均由用户直接提供 |
| 标识 / 负责人 / 版本 | loki_metris / ManonLoki / 0.1.0 |
| 输入父目录 | /Users/manonloki/Documents/my-work/ai |
| 唯一项目根 / Git 顶层 | /Users/manonloki/Documents/my-work/ai/loki_metris |
| Harness 源根 | /Users/manonloki/Documents/ym-work/harness/agent-first-harness-template |
| 平台 / 接口 / 框架 | Windows、macOS / GUI / GPUI |
| 桌面能力 / Logo | [GUI_APP_PROFILE.md](docs/GUI_APP_PROFILE.md)，九项配置和 candidate-1 选择均已确认，保留中性行为 |
| 写入前 Git 环境门禁 | status=passed，version=2.56.0，change=unchanged；未安装或升级工具 |
| 源 Harness 验证 | node scripts/validate_harness.mjs 通过；未在下游调用源验证器 |
| 超范围产品输入 | 本次未收到，也未记录或实施产品业务需求 |

基线使用独立 `main`，提交消息为 `chore: initialize project`；最终 Git 版本 2.56.0。有效作者 `user.name=罗超然`、`user.email=manonloki@163.com` 均来自既有 global 配置，origin/source 为 `file:/Users/manonloki/.gitconfig`，原样复用，无推导及 repo-local 身份覆盖，未修改 global/system 配置。受管模板检查通过，仅写当前仓库四项 local 配置：`commit.template=.git/harness/meaningful-commit-template.txt`（配置值为绝对路径）、`commit.cleanup=strip`、`commit.verbose=true`、`core.commentChar=#`，未替换冲突配置。基线提交与干净状态以本仓库 Git 复读为准。

未建立 `.harness/upstream-lock.json`；首次 `$desktop-upgrade-harness` 必须先初始基线审计，不得伪造受管清单。此项目为终端仓库，实例化/初始化入口已裁掉。

下一步在此根目录使用 `$desktop-define-product` 定义目标和 MVP。反馈渠道与分发渠道尚未确定；关于页 QQ 来自受管品牌资产，不代表已建立产品反馈流程。

完整商业许可证：[中文](LICENSE.zh-CN.md)、[English](LICENSE.en.md)。第三方许可不被本项目许可覆盖；分发时另核对实际依赖 NOTICE。

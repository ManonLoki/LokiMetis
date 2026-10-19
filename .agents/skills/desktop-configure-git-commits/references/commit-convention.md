# Git 提交消息规范

本规范采用 Conventional Commits 主题，并在非简单变化中保留修改意图、行为变化、影响和验证事实。编辑器实际显示的注释模板以 [`../assets/commit-template.txt`](../assets/commit-template.txt) 为准。

主题摘要和正文说明使用中文；Conventional Commit 的 type、scope、技术标识符、命令、Why/Changes/Impact/Test 标签和真实测试状态可保留。Git 必要语义不代替中文说明，普通合并显式写“合并开发分支 …”或“合并远端默认分支 …”。

## 格式

```text
<type>(<scope>): <summary>

Why:
- 为什么要做这次修改
- 缺陷修复说明导致问题的根因

Changes:
- 具体完成了什么行为或契约变化
- 不要只罗列文件名

Impact:
- 对功能、API、数据、性能或兼容性的影响
- 没有明显影响时写 无

Test:
- 实际执行的命令、场景和结果
- 未执行时写 Not run: <原因>
```

`scope` 可省略。已经批准的破坏性变化使用 `<type>(<scope>)!: <summary>`，并在正文或 footer 清楚说明迁移影响；不得用 `!` 代替产品范围、兼容契约或版本批准。

## Type

| Type | 用途 | 规范示例 |
|---|---|---|
| `feat` | 新增用户或维护者可用能力 | `feat(queue): 支持可配置的并发数` |
| `fix` | 修复可复现缺陷 | `fix(sync): 防止重复上传` |
| `refactor` | 不改变可观察行为的结构调整 | `refactor(storage): 提取仓库访问层` |
| `perf` | 可测量的性能优化 | `perf(image): 并发处理独立图片` |
| `ui` | UI 或交互调整 | `ui(settings): 简化压缩选项` |
| `style` | 不影响语义的格式或代码风格变化 | `style: 应用 rustfmt 格式` |
| `docs` | 文档变化 | `docs(api): 补充上传示例` |
| `test` | 测试或测试基础设施变化 | `test(queue): 覆盖重试耗尽场景` |
| `build` | 构建系统或依赖变化 | `build: 更新 Tauri 兼容范围` |
| `ci` | CI/CD 工作流变化 | `ci: 增加发布候选工作流` |
| `chore` | 其他明确的工程维护 | `chore: 删除未使用资产` |
| `revert` | 明确回滚既有提交 | `revert: 回滚图片队列重构` |

工具生成的 merge、revert、fixup 或 squash 消息应保留 Git 的必要语义并使用中文说明；不要为了套模板破坏自动合并、定向回滚或交互式 rebase 流程。

## Summary

Summary 回答“这次提交完成了什么有意义的变化”，而不是“改了哪些文件”。使用中文摘要和清晰动词，保持单行、具体、可在日志中独立理解；建议简短，不以 `update`、`fix bug`、`modify file` 等模糊措辞代替结果。

推荐：

```text
feat(queue): 支持可配置的并发数
```

不推荐：

```text
update queue
modify task.rs
fix bug
update
```

## 正文

- `Why` 是最重要的长期信息。diff 通常能说明改了什么，但不能恢复当时的动机、约束或根因。
- `Changes` 描述得到的行为、状态转换或契约，不复制文件清单。
- `Impact` 明确兼容性、数据、性能、API 和用户体验；没有明显影响时写 `无`。
- `Test` 只陈述真实执行。写明关键命令/场景和结果；未执行必须说明原因，不能把计划中的测试写成已通过。
- 关联问题、迁移提示或 `BREAKING CHANGE:` 可作为 footer；正文中不得出现凭据、个人数据或敏感载荷。

## 完整示例

```text
feat(queue): 增加图片并发处理

Why:
- 批量压缩图片时串行处理速度较慢
- 需要支持并发执行，同时限制同时运行的任务数

Changes:
- 新增有界任务队列和最大并发数
- 增加 pending/running/success/failed 状态流转
- 等待中的任务会在执行槽可用后自动启动

Impact:
- 提升批量处理吞吐量
- 单张图片压缩结果保持不变

Test:
- 通过 1、10、100 张图片的队列回归
- 验证最大并发限制和失败任务隔离
```

```text
fix(queue): 防止任务重复执行

Why:
- 队列刷新时，同一个 pending 任务可能被多个 worker 同时获取
- 根因是执行前没有原子取得任务所有权

Changes:
- 只有 pending -> running 原子状态切换成功的 worker 才能执行任务
- 删除重复的任务启动路径

Impact:
- 消除重复处理，不改变正常任务顺序

Test:
- 高并发执行 100 个任务，每个 task id 恰好执行一次
```

```text
refactor(compression): 分离编码器与任务调度器

Why:
- 编码逻辑与任务生命周期耦合，难以独立演进和测试

Changes:
- 提取 CompressionEngine
- TaskScheduler 只负责任务生命周期并通过统一接口调用编码器

Impact:
- 无用户可见行为变化

Test:
- PNG/JPEG 结果与重构前一致
- 批量任务状态流转回归通过
```

## 简单提交

非常简单且意图明确的变化可以只写主题，不要为了形式制造空洞正文：

```text
fix(ui): 防止进度文本溢出
docs(readme): 澄清安装步骤
chore: 删除未使用依赖
```

## 原子性

一个提交对应一个完整、独立、可解释的变化。队列、并发限制、重试和 UI 状态如果能分别保持仓库处于合理状态，可以形成多个逻辑提交；同一功能中仅新增结构、函数、导入和修复编译错误则应合并，不能按文件或代码动作机械切碎。

提交前确认：

1. 提交只表达一个主要目的，checkout 后仓库处于合理状态。
2. Summary 在半年后仍能说明结果。
3. `Why` 解释动机；缺陷修复解释根因。
4. `Changes` 描述行为，不是文件清单。
5. `Impact` 与真实兼容/数据/性能/API 影响一致。
6. `Test` 只记录实际执行，没有把未运行写成通过。
7. 没有混入无关格式化、调试代码、秘密、缓存或临时文件。

## 消息检查边界

提交前运行只读 `configure_git_commit.mjs message-check --message-file <file>`，拒绝空消息、非法 UTF-8/控制字符、首行空白及纯英文摘要。Conventional Commit 的 scope 中出现汉字不能替代中文摘要。检查不访问仓库配置或安装模板；通过只表示确定格式满足要求，中文准确性、正文语言和差异一致性仍逐项复核。提交后复读实际消息，不把 hook 改写前文本当最终消息。

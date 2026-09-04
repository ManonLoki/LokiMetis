---
name: desktop-configure-git-commits
description: 仅在用户明确要求配置 Git，或下一步将实际创建提交时，为独立仓库即时检查并补齐仓库级身份、安装并检查提交消息模板；不得因打开、检查或编辑仓库而提前配置。
---

# 配置有意义的 Git 提交

让每个提交表达一个完整、独立、可解释的变化，并为半年后的排查保留修改原因、行为影响和真实验证结果。

## 适用模式

- **安装**：用户明确要求配置提交模板，或已授权流程的下一步将实际创建提交且当前仓库尚未安装受管模板。
- **检查/修复**：下一步将实际创建提交时发现模板没有出现、本地配置漂移，或用户明确要求修复受管模板。
- **编写/复核**：准备提交、拆分逻辑提交，或检查现有提交消息是否准确。

进入仓库、读取状态、诊断、编辑、运行测试、生成脚手架、仅编写/复核提交消息或“以后可能会提交”都不触发安装、检查或修复。没有即将发生的实际提交，也没有用户明确配置请求时，不得运行本 Skill 的脚本或改写任何 Git 配置。项目初始化只有在全部脚手架与一次性裁剪完成、下一步就是创建唯一基线提交时，才满足即时触发条件。安装请求或该次提交授权当前仓库的本地配置与 Git 元数据变更，但不授权全局配置、推送、标签或历史改写。提交本身只由调用本 Skill 的已授权初始化或发布流程执行。

## 工作流程

1. 先确认用户已明确要求配置 Git，或已授权流程的下一步将实际运行 `git commit`；否则停止且不读取或设置提交模板配置。满足触发条件后，读取项目 `AGENTS.md`、当前 Git 状态和本次变更事实，确认传入目录的规范化路径恰好等于 `git rev-parse --show-toplevel`。父仓库、裸仓库和非独立目录都停止处理。
2. 实际提交前先运行身份只读报告。若 `user.name` 与 `user.email` 已经有效生效，无论来自 local/global/conditional 配置都原样保留；不得为了统一格式创建 local 覆盖。任一字段缺失时，由 Agent 从宿主账户读取设备用户名（POSIX 使用 `id -un`，Windows PowerShell 使用 `[Environment]::UserName`）：安全 ASCII 英文用户名直接使用，非英文值先由 Agent 翻译并归一化为单一 ASCII username/slug；本 Skill 不实现、猜测或调用固定翻译算法。只把该单一值传给 bootstrap，脚本从同一输入确定派生 `user.name = <asciiDeviceUsername>` 与 `user.email = <asciiDeviceUsername>@gmail.com`，并只为缺失字段执行 `git config --local`，然后独立检查并返回 scope/origin/derivation：

   ```text
   python3 .agents/skills/desktop-configure-git-commits/scripts/configure_git_commit.py identity-report --project-root .
   python3 .agents/skills/desktop-configure-git-commits/scripts/configure_git_commit.py identity-bootstrap --project-root . --fallback-username "<Agent-provided ASCII English device username>"
   python3 .agents/skills/desktop-configure-git-commits/scripts/configure_git_commit.py identity-check --project-root .
   ```

   `identity-report` 与 `identity-check` 只读；`identity-bootstrap` 在身份完整时幂等且不写配置。已有字段无效、Agent 未提供安全 ASCII username/slug、仓库边界异常或 local 写入/复探失败时必须停止，绝不得静默改写已有身份、接受独立且可能不一致的邮箱输入或退回全局配置。
3. 安装或修复模板时，使用同一标准库脚本。它把受管模板逐字节安装到当前仓库的 Git common dir，并只通过 `git config --local` 设置 `commit.template`、`commit.cleanup=strip`、`commit.verbose=true` 与 `core.commentChar=#`：

   ```text
   python3 .agents/skills/desktop-configure-git-commits/scripts/configure_git_commit.py install --project-root .
   python3 .agents/skills/desktop-configure-git-commits/scripts/configure_git_commit.py check --project-root .
   ```

4. 若任一受管模板键已有不同值，脚本必须在写入前失败并报告冲突。只有用户明确批准替换当前仓库模板配置时才重跑 `install --replace`；不得把 `--replace` 当作默认恢复手段。任何模式都不得运行 `git config --global`、`git config --system`，不得修改签名、凭据、远端或 hooks；身份 bootstrap 的唯一写入是缺失 `user.name`/`user.email` 的当前仓库 local 值。
5. 编写或复核提交信息时读取 [提交消息规范](references/commit-convention.md)。先检查暂存差异是否只包含一个逻辑结果；无关变化保持未暂存或拆成后续提交，不得擅自重写已有历史。
6. 简单且意图明确的维护变化可以只使用一行 Conventional Commit 主题。非简单变化必须填写 `Why`、`Changes`、`Impact`、`Test`；`Why` 解释动机或根因，`Changes` 描述行为变化，`Impact` 说明兼容/数据/性能/API 影响，`Test` 只记录实际执行的验证。未运行时写 `Not run: <原因>`，不得伪造通过。
7. 创建提交前复核主题和正文与实际 diff 一致，不包含秘密、令牌、个人数据、临时调试内容或未跟踪生成物。用户没有要求创建提交时，若当前工作流也不强制产生提交，只返回建议消息，不运行身份 bootstrap、`install`/`check`，也不执行 `git commit`。

## 初始化接入

新下游开始初始化时由环境门禁检查/安装 Git 可执行文件，但那一阶段还没有仓库 local 配置；不得在初始化表单、复制、身份改写、环境门禁、脚手架编写或测试阶段提前运行身份或模板写入。全部脚手架检查和一次性能力裁剪完成、下一步就是创建唯一基线提交时，才即时建立或确认独立 Git 边界、保留本 Skill，按上文运行 `identity-report` → 必要时 `identity-bootstrap` → `identity-check`，再运行模板 `install` 与 `check` 并创建 `chore: initialize project`。已有有效身份保持不变；缺失身份从 Agent 已解析的单一 ASCII 英文设备 username 确定派生名称及同名 Gmail，只写仓库 local。该基线属于规范允许的简单单行提交。身份/模板冲突、字节不一致、复探或检查失败都阻断初始化；不得通过修改全局 Git 配置绕过。初始化完成必须向用户返回最终 Git 版本、身份值及各自 scope/source/derivation、仓库根、分支与基线 commit。

## 完成输出

报告模式、规范 Git 根、有效 `user.name`/`user.email`、各字段 scope/origin/source、是否写入 local、受管模板状态、本地四项模板配置、是否发生替换、建议或实际提交消息，以及未执行的推送/标签/历史操作。身份与模板检查通过只证明提交前置配置正确，不证明提交内容本身有意义。

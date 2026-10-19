---
name: desktop-inspect-release-notes
description: 只读校验或按中文、英文预览现有 release-notes.json；不生成条目、不写文件、不改变版本，不执行构建或发布。
---

# 检查与预览更新日志

用户要求检查/预览，或已授权发布/候选流程需要复核现有日志时使用。读取 [发布规范](../../../docs/RELEASE.md) 的用户可见版本与更新日志章节；schema、十版窗口、翻译对、顺序与资源上限继续以原规则为准。文件不存在时报告缺席，不初始化空日志或补造历史。

## 执行

1. 确认指定或项目根日志路径；期望版本复用当前明确版本事实，不通过本入口改变版本。
2. 使用只读 wrapper：

   ```text
   node .agents/skills/desktop-inspect-release-notes/scripts/inspect_release_notes.mjs check --file <path> [--expected-version <version>]
   node .agents/skills/desktop-inspect-release-notes/scripts/inspect_release_notes.mjs render --file <path> --locale <zh-CN|en-US>
   ```

3. wrapper 只复用 `$desktop-prepare-release` 的读取、校验与渲染函数，不透传拥有写入能力的原 `main`。两种语言需要语义核对时分别渲染并比较。
4. 失败报告真实原因并返回调用方；只有明确进入发布准备后，原发布 Skill 才能整理和写入日志。

## 边界与输出

不接受 `upsert`、输出文件或任意透传参数，不写项目记忆、版本和候选证据，不提交、构建、发布或联网。报告实际校验、期望/实际版本或所选语言文本；结构有效不证明描述已实现或候选已发布。此目录依赖保留的 [原日志库](../desktop-prepare-release/scripts/release_notes.mjs)，单独复用时必须一并带上该标准库依赖。

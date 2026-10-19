# Hooks协议依据

核对日期：2026-10-16。这是维护事件映射的依据；原生Agent与操作系统通知的完整端到端验收另行执行。

| Agent | 已核实的映射依据 | 第一方来源 |
| --- | --- | --- |
| Codex | 公开支持12项事件，包含PostCompact与Interrupt；Interrupt静默取消，超时3秒。Stop可被其它Hook阻止后续跑，仅显示空闲，不据此发送确定成功通知。未虚构失败或提问事件。 | [Hooks](https://learn.chatgpt.com/docs/hooks) |
| Claude Code | AskUserQuestion/ExitPlanMode进入交互等待；PermissionDenied与工具失败可恢复；主Agent的StopFailure为父轮次失败。子Agent的失败按agent_id隔离，只解除其自身交互，不冒充父任务错误或结束；MCP澄清回执按交互/服务器关联。 | [Hooks reference](https://code.claude.com/docs/en/hooks) |
| Cursor | 执行前事件不是权限等待；afterAgentResponse仅消息完成；stop的completed/error/aborted分别为完成/错误/取消；sessionEnd仅有明确最终结果才报告完成或错误。 | [Hooks](https://cursor.com/docs/hooks) |
| Grok Build | Notification使用notificationType；permission_prompt实际等待，agent_error为轮次失败；工具失败/权限拒绝可恢复；StopCancelled不报告完成。Stop可同promptId续跑，故不报告确定成功；task_complete只是后台作业完成，不能替代Agent成功。 | [用户指南](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-pager/docs/user-guide/10-hooks.md)、[事件定义](https://github.com/xai-org/grok-build/blob/main/crates/codegen/xai-grok-hooks/src/event.rs) |
| WorkBuddy | Stop可继续，StopFailure是停止Hook异常；FinalStop的final_stop_reason才是completed/failed/cancelled/interrupted最终结果。三类阻塞工具及澄清通知进入等待。 | [官方插件说明](https://www.workbuddy.cn/docs/workbuddy/Plugins)、下述本机第一方引擎 |
| OpenCode | 稳定v1.19.0的tool.execute.before/after是独立具名回调；权限/问题的requestID关联asked.id，工具callID不能作为turn ID。 | [插件接口](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/plugin/src/index.ts)、[权限协议](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/schema/src/v1/permission.ts)、[问题协议](https://github.com/anomalyco/opencode/blob/v1.19.0/packages/schema/src/v1/question.ts) |
| Hermes | smart审批不等待人类；API错误可重试；post_llm_call也可能是失败解释，最终结果以on_session_end的completed/failed/interrupted为准。 | [observer契约](https://github.com/NousResearch/hermes-agent/blob/main/website/docs/developer-guide/observer-hooks.md)、[真实终态调用](https://github.com/NousResearch/hermes-agent/blob/main/agent/turn_finalizer.py) |
| Gemini CLI | AfterTool的tool_response.error保留错误语义但不转发正文；ToolPermission通知才表示权限/提问等待。 | [Hooks reference](https://geminicli.com/docs/hooks/reference/) |
| GitHub Copilot CLI | permissionRequest先于自动规则判定，不能证明人类等待；notification的permission_prompt/elicitation_dialog才进入询问。 | [Hooks reference](https://docs.github.com/en/copilot/reference/hooks-reference) |
| Qwen Code | permission_prompt是权限等待；工具错误/权限拒绝不终止父轮次。未采用尚未实现的elicitation_dialog。 | [Hooks](https://qwenlm.github.io/qwen-code-docs/en/users/features/hooks/) |
| Kimi Code | Interrupt表示用户主动中断并替代Stop；不报告成功。 | [Hooks](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/hooks.html) |

WorkBuddy终态证据来自已安装5.7.7的`/Applications/WorkBuddy.app/Contents/Resources/app.asar.unpacked/cli/dist/codebuddy-headless.js`，SHA-256为`76b75e8513c70f696148278cf5621cd0a9bc9a8105318a451fbed7e63bae38c5`。核对executeStopHooks、executeFinalStopHooks与completed/failed/cancelled/interrupted调用点；该结论不自动推广为独立CodeBuddy CLI的版本保证。

Claude子Agent归属字段于2026-10-19补核：[公共输入字段](https://code.claude.com/docs/en/hooks#common-input-fields)的`agent_id`标识子Agent或进程内teammate，`agent_type`也可由主会话的`--agent`提供，不能单独据此判为子Agent。[SubagentStop](https://code.claude.com/docs/en/hooks#subagentstop)沿用父会话的`session_id`。据此，本实现将明确带`agent_id`的工具/权限失败及API终止限制在该子Agent，不以其失败推断父轮次失败；未带子Agent身份的主会话错误保持原有语义。

公共归约规则：

- 会话ID、轮次ID、子Agent ID和工具/澄清ID分别处理；只转发有限状态、标识和工具公开名称，不转发提示、参数、答案或错误正文。
- 只有匹配的交互回复/工具完成可以解除对应等待；子Agent结束、压缩和非最终Stop不充当回复。
- 无调用ID的权限请求仅在同Agent的活动工具唯一时关联；MCP表单至少按服务器隔离。无法区分的并行请求保守保留询问，不猜测全部已回答。
- 新增订阅或生成插件变更后，需要在应用内重新保存对应Hooks配置，并按原有宿主要求审核/重启/启用；此次修复不自动改写用户的Agent配置。
- 未提供轮次/序号的上游无法证明任意跨轮次乱序事件的归属。没有最终结果信号的可继续Stop，也无法在第一条Stop到达时预知宿主是否继续；不声称拥有上游未提供的证据。
- Codex/Grok当前公开协议缺少“所有Stop门禁已通过”的权威信号；本实现保守抑制其Stop完成通知，保留询问、错误以及空闲/续跑展示。后续上游补足契约再恢复可靠完成提示。Claude等无轮次ID兼容协议仍按其Stop/idle事件展示与通知，Stop门禁后的绝对最终性同样不能由观察者保证。

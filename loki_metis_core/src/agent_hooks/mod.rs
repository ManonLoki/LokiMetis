//! AgentHooks 领域：十四项 AI 工具的 Hook 协议、生成/合并与状态归约。
//!
//! 公共流程只依赖 [`HookProtocol`]；事件名、配置 JSON 结构、命令输出约定和
//! 托管条目布局由各工具的独立实现负责。

mod error;
mod payload;
mod state_machine;
mod types;

// 引入 Path 与 Duration，分别用于生成 CLI relay 插件和声明生命周期交接延迟
use std::{path::Path, time::Duration};

// 以下每个子模块对应一个受支持的 AI 工具协议实现，模块名与工具 slug 对应
mod claude_code;
mod code_buddy;
mod codex;
mod cursor;
mod gemini_cli;
mod generation;
mod github_copilot;
mod grok;
mod hermes;
mod kimi_code;
mod open_claw;
mod open_code;
mod qoder;
mod qwen_code;
mod work_buddy;

// 仅在测试编译时包含跨工具契约测试模块
#[cfg(test)]
mod tests_contract;

// 引入 serde_json 的 Map/Value 与 json! 宏，用于构造和解析 Hook 配置 JSON
use serde_json::{Map, Value, json};

// 引入 generation 子模块内部使用的标记识别函数（本模块内私有复用）
use generation::command_has_marker;
// 对外重新导出配置生成、WSL 配置生成、配置合并三个公开入口
pub use error::HookError;
pub use generation::{
    generate_hook_config, generate_wsl_hook_config, merge_hook_config, remove_managed_hook_entries,
};
pub use payload::{MinimalHookPayload, PreparedNativeHook, prepare_native_hook};
pub use state_machine::{HookEventDecision, HookStateMachine};
pub use types::{
    AiTool, AiToolDescriptor, HOOK_EVENT_TYPE_HEADER, HOOK_RELAY_EPHEMERAL_PORT,
    HOOK_RELAY_INSTANCE_HEADER, HOOK_RELAY_RENDEZVOUS_FILENAME,
    HOOK_RELAY_RENDEZVOUS_SCHEMA_VERSION, HookBehavior, HookConfigDirectories, HookConfigLocation,
    HookConfigPreview, HookConfigWriteResult, HookTransition, HookWriteOutcome,
    MAX_NATIVE_HOOK_INPUT_BYTES, hook_relay_loopback_address, normalize_enabled_ai_tools,
};

// 所有受管 Hook 命令共用的标识前缀，用于在配置文件中识别 LokiMetis 写入的条目
pub(super) const MANAGED_HOOK_PREFIX: &str = "LokiMetis";

// 描述一个 Hook 事件在状态机语义上归属的“种类”，决定它触发什么样的状态迁移
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HookEventKind {
    // 工作区/项目打开，尚未进入具体会话
    WorkspaceStart,
    // 会话开始
    SessionStart,
    // 明确的工作开始（有确定的展示态迁移到 Running）
    WorkStart,
    // 未限定作用域的工作开始，展示态由携带的 HookBehavior 决定
    UnscopedWorkStart(HookBehavior),
    // 未限定作用域的工作完成，固定迁移回 Running 展示
    UnscopedWorkCompletion,
    // 工作过程中的进度事件，展示态由携带的 HookBehavior 决定
    WorkProgress(HookBehavior),
    // 工作完成事件，展示态由携带的 HookBehavior 决定
    WorkCompletion(HookBehavior),
    // 明确的停止事件，固定迁移到 Idle 展示
    Stop,
    // 终止态事件（如成功/失败/中断的最终结果），展示态由携带的 HookBehavior 决定
    TerminalState(HookBehavior),
    // 通用状态事件，展示态由携带的 HookBehavior 决定
    State(HookBehavior),
    // 会话结束，触发展示位释放
    SessionEnd,
}

impl HookEventKind {
    // 把事件种类映射为具体的状态迁移动作（展示某个行为态，或释放展示位）
    pub(super) const fn transition(self) -> HookTransition {
        match self {
            // 会话结束：释放该会话占用的展示位
            Self::SessionEnd => HookTransition::Release,
            // 工作区开始 / 会话开始 / 停止：统一展示为 Idle
            Self::WorkspaceStart | Self::SessionStart | Self::Stop => {
                HookTransition::Display(HookBehavior::Idle)
            }
            // 明确的工作开始 / 未限定作用域的工作完成：统一展示为 Running
            Self::WorkStart | Self::UnscopedWorkCompletion => {
                HookTransition::Display(HookBehavior::Running)
            }
            // 携带具体 HookBehavior 的几类事件：直接透传该行为态作为展示态
            Self::UnscopedWorkStart(behavior)
            | Self::WorkProgress(behavior)
            | Self::WorkCompletion(behavior)
            | Self::TerminalState(behavior)
            | Self::State(behavior) => HookTransition::Display(behavior),
        }
    }
}

// 描述协议声明的一个具体 Hook 事件：事件名、可选的 matcher（子类型过滤器）、
// 以及该事件对应的状态机语义种类
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct HookEvent {
    // 事件在该工具原生配置中的名称（如 "SessionStart"）
    pub name: &'static str,
    // 可选的匹配器，用于进一步区分同一事件名下的子类型（如 Notification 的 idle_prompt）
    pub matcher: Option<&'static str>,
    // 该事件对应的状态机语义种类
    pub kind: HookEventKind,
}

impl HookEvent {
    // 构造一个不带 matcher 的普通事件
    pub const fn new(name: &'static str, kind: HookEventKind) -> Self {
        Self {
            name,
            // 不带 matcher
            matcher: None,
            kind,
        }
    }

    // 构造一个带 matcher 的事件，用于需要按子类型区分状态的场景
    pub const fn with_matcher(
        name: &'static str,
        matcher: &'static str,
        kind: HookEventKind,
    ) -> Self {
        Self {
            name,
            // 携带指定的 matcher
            matcher: Some(matcher),
            kind,
        }
    }
}

// 一个事件在三种目标平台/场景下对应的托管命令字符串集合
pub(super) struct ManagedCommands {
    // POSIX shell（macOS/Linux，以及 WSL）使用的命令
    pub posix: String,
    // WSL 配置必须选择 POSIX 命令；普通 Windows 配置仍保持现有 CMD 分支。
    pub is_wsl: bool,
    // 这两个字段只在 `#[cfg(target_os = "windows")]` 分支中被读取
    // （见 `platform_command` 与 `codex.rs` 的 `handler`），非 Windows 平台
    // 编译时会被 dead_code 检查误判为未使用，因此显式允许。
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    // 原生 Windows CMD 命令
    pub windows: String,
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    // 经 PowerShell 宿主转发到 CMD 的命令变体
    pub windows_powershell_host: String,
}

/// 单个工具必须实现的完整 Hook 协议契约。
pub(super) trait HookProtocol: Sync {
    // 返回该协议对应的工具枚举值
    fn tool(&self) -> AiTool;
    // 返回该工具面向用户展示的名称
    fn name(&self) -> &'static str;
    // 返回该工具在 Hook 请求路径中使用的 slug 标识
    fn slug(&self) -> &'static str;
    // 返回该工具主配置文件相对于其配置根目录的文件名
    fn config_filename(&self) -> &'static str;
    // 返回用于预览展示的完整相对路径（含配置根目录前缀）
    fn preview_filename(&self) -> &'static str;
    // 返回该工具声明的全部 Hook 事件列表
    fn events(&self) -> &'static [HookEvent];

    /// 返回独立配置文件内容时，公共 JSON hooks 生成/合并流程会被跳过。
    /// 插件只获得 LokiMetis CLI 可执行路径，不得自行访问 listener 端口。
    fn standalone_config(&self, _relay_executable: &Path) -> Option<String> {
        // 默认没有独立配置文件，走公共 JSON 生成流程
        None
    }

    /// 是否以独立插件文件接入；与 TOML 等自定义合并协议明确区分。
    fn uses_standalone_plugin(&self) -> bool {
        false
    }

    /// 返回与主配置文件一同写入的受管文件。所有文件都会先完成冲突校验，
    /// 再由 application 层统一落盘，避免只安装半套插件。
    fn auxiliary_configs(&self) -> Vec<HookConfigPreview> {
        // 默认没有附加文件
        Vec::new()
    }

    /// 合并独立文件。默认只覆盖带当前工具 `LokiMetis` 标识的受管文件；需要与
    /// 用户内容共存的独立格式可自行覆盖。
    fn merge_standalone(
        &self,
        // 现有独立文件内容；None 表示文件不存在
        existing_content: Option<&str>,
        // 新生成的配置预览
        generated: &HookConfigPreview,
    ) -> Result<String, HookError> {
        // 若现有内容存在且不含本工具的管理标识，说明这是用户/其他工具的文件，拒绝覆盖
        if existing_content.is_some_and(|content| !contains_managed_marker(content, self.tool())) {
            return Err(HookError::new("error.hooks.foreignFileRejected")
                .param("filename", generated.filename.clone()));
        }
        // 否则直接用新生成的内容整体替换（默认策略：独立文件整体由 LokiMetis 托管）
        Ok(generated.content.clone())
    }

    // 默认直接返回事件本身声明的种类，不做基于状态值的二次判定
    fn event_kind(&self, event: &HookEvent, _status: Option<&str>) -> HookEventKind {
        event.kind
    }

    /// 生命周期交接缓冲；用于消歧无 ID End，也用于延后物理 Release。
    fn release_settle_delay(&self) -> Duration {
        // 默认不延迟
        Duration::ZERO
    }

    /// 是否允许显式 `SessionStart` 覆盖同 ID 墓碑；新的 `WorkStart` 始终可建新 epoch。
    fn session_start_revives_tombstone(&self) -> bool {
        // 默认允许
        true
    }

    /// 默认返回 `Null`：走 `standalone_config` 独立文件路线的工具无需实现。
    fn handler(&self, _event: &HookEvent, _commands: &ManagedCommands) -> Value {
        Value::Null
    }

    // 默认配置根结构为 `{ "hooks": { ...每个事件对应一个条目... } }`
    fn config_root(&self, hooks: Map<String, Value>) -> Value {
        json!({ "hooks": Value::Object(hooks) })
    }

    /// 把逐事件生成的 handler 序列化为最终配置。默认输出 JSON；若未来出现公开
    /// 配置格式不是 JSON 的 command Hook，可覆盖该步骤。
    fn render_config(&self, hooks: Map<String, Value>) -> Result<String, HookError> {
        // 用带缩进的 pretty 格式序列化配置根结构；失败时返回可序列化错误
        serde_json::to_string_pretty(&self.config_root(hooks)).map_err(|error| {
            HookError::new("error.hooks.renderFailed").param("detail", error.to_string())
        })
    }

    /// 是否绕过公共 JSON 合并器。独立插件需显式启用；共享 TOML 等
    /// 非 JSON 配置也可按自身协议启用。
    fn uses_custom_merge(&self) -> bool {
        // 默认与是否走独立插件文件保持一致：独立插件天然需要自定义合并
        self.uses_standalone_plugin()
    }

    /// 从一组 hooks 事件条目中过滤掉本工具的受管处理器；一个条目的处理器
    /// 全部被移除后，连同这个条目本身一起丢弃，避免留下空壳分组。
    fn remove_managed_entries(&self, entries: &mut Vec<Value>) {
        entries.retain_mut(|group| {
            // 取出该分组下的 handlers 数组；取不到则保留该分组原样（结构不符合预期，不做处理）
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else {
                return true;
            };
            // 保留所有不属于本工具受管标识的 handler
            handlers.retain(|handler| !entry_is_managed(handler, self));
            // 若移除后 handlers 非空则保留该分组，否则丢弃这个空壳分组
            !handlers.is_empty()
        });
    }

    /// 配置实际变化后需要执行的激活流程。工具专属流程必须在协议内唯一声明，
    /// application 与前端不得再根据 `AiTool` 组合布尔值或推断指引。
    fn changed_write_outcome(&self) -> HookWriteOutcome {
        // 默认写入后立即生效，无需重启或额外操作
        HookWriteOutcome::Active
    }
}

// 按 AI 工具类型分发到对应的静态协议实现，是本模块内所有分发函数的唯一入口。
pub(super) fn protocol(tool: AiTool) -> &'static dyn HookProtocol {
    match tool {
        AiTool::Codex => &codex::CODEX,
        AiTool::ClaudeCode => &claude_code::CLAUDE_CODE,
        AiTool::Cursor => &cursor::CURSOR,
        AiTool::OpenCode => &open_code::OPEN_CODE,
        AiTool::WorkBuddy => &work_buddy::WORK_BUDDY,
        AiTool::Hermes => &hermes::HERMES,
        AiTool::OpenClaw => &open_claw::OPEN_CLAW,
        AiTool::CodeBuddy => &code_buddy::CODE_BUDDY,
        AiTool::QwenCode => &qwen_code::QWEN_CODE,
        AiTool::KimiCode => &kimi_code::KIMI_CODE,
        AiTool::Qoder => &qoder::QODER,
        AiTool::GeminiCli => &gemini_cli::GEMINI_CLI,
        AiTool::GitHubCopilot => &github_copilot::GITHUB_COPILOT,
        AiTool::Grok => &grok::GROK,
    }
}

/// 返回该工具主配置文件的文件名，供 adapter 拼出完整配置路径。
pub fn hook_config_filename(tool: AiTool) -> &'static str {
    protocol(tool).config_filename()
}

/// WSL 内目前只托管 command Hook；原生插件的安装与可执行文件定位不能复用本分支。
pub fn hook_supports_wsl(tool: AiTool) -> bool {
    // 只有非独立插件（包括自定义 TOML command Hook）才支持 WSL
    !protocol(tool).uses_standalone_plugin()
}

/// 返回该工具的展示名称。
pub fn ai_tool_name(tool: AiTool) -> &'static str {
    protocol(tool).name()
}

/// 返回全部 AI 工具的展示目录。名称来自每个工具的 `HookProtocol`；
/// 顺序按展示名 ASCII 升序（A-Z），供设置页 AI 客户端列表使用。持久化规范化仍走
/// `AiTool::ALL`，不受此展示顺序影响。
pub fn ai_tool_descriptors() -> Vec<AiToolDescriptor> {
    let mut descriptors = AiTool::ALL
        .into_iter()
        // 把每个工具映射为携带其展示名称的描述符
        .map(|tool| AiToolDescriptor {
            tool,
            name: protocol(tool).name().to_owned(),
        })
        .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.name.cmp(&right.name));
    descriptors
}

/// 配置发生变化时，返回该工具协议唯一声明的写入结果。
#[cfg(test)]
pub(super) fn hook_changed_write_outcome(tool: AiTool) -> HookWriteOutcome {
    protocol(tool).changed_write_outcome()
}

/// 根据实际内容变化构造写入结果；未变化时统一返回 `Unchanged`。
/// 根据实际内容变化构造写入结果。
pub fn hook_config_write_result(
    // 目标工具
    tool: AiTool,
    // 写入的配置文件名
    filename: String,
    // 本次写入相较于之前内容是否发生了变化
    config_changed: bool,
) -> HookConfigWriteResult {
    // 委托给公共构造函数，按“是否变化”与协议声明的写入结果组合出最终结果
    HookConfigWriteResult::from_changed_outcome(
        tool,
        filename,
        config_changed,
        protocol(tool).changed_write_outcome(),
    )
}

/// 兼容旧调用方：配置变化后该工具是否需要用户审核/信任。
#[cfg(test)]
pub(super) fn hook_requires_review(tool: AiTool) -> bool {
    protocol(tool).changed_write_outcome().requires_review()
}

/// 兼容旧调用方：配置变化后该工具是否需要重启才能加载。
#[cfg(test)]
pub(super) fn hook_restart_required(tool: AiTool) -> bool {
    protocol(tool).changed_write_outcome().restart_required()
}

/// 按 Hook 请求路径中的 slug 反查对应工具，避免与各协议自身的 `slug()` 重复维护映射表。
pub fn tool_from_slug(slug: &str) -> Option<AiTool> {
    AiTool::ALL
        .into_iter()
        // 遍历全部工具，找到 slug 匹配的第一个（slug 在各协议间应保证唯一）
        .find(|tool| protocol(*tool).slug() == slug)
}

/// 返回该工具需要一并写入的附加受管文件（独立插件的元数据/清单等）。
pub fn generate_hook_auxiliary_configs(tool: AiTool) -> Vec<HookConfigPreview> {
    protocol(tool).auxiliary_configs()
}

// 按事件名在该工具声明的事件表中查找对应的事件定义
pub(super) fn event_definition(tool: AiTool, event: &str) -> Option<HookEvent> {
    protocol(tool)
        .events()
        .iter()
        .copied()
        // 找到名称匹配的事件定义
        .find(|candidate| candidate.name == event)
}

// 结合可选的状态值，解析出事件名对应的状态机语义种类
pub(super) fn event_kind(tool: AiTool, event: &str, status: Option<&str>) -> Option<HookEventKind> {
    // 先取出协议实现，后续同时用于查找事件定义和调用 event_kind
    let protocol = protocol(tool);
    // 找到事件定义后，交给协议按事件与状态值解析出具体种类
    event_definition(tool, event).map(|definition| protocol.event_kind(&definition, status))
}

// 返回该工具协议声明的生命周期交接缓冲时长
pub(crate) fn release_settle_delay(tool: AiTool) -> Duration {
    protocol(tool).release_settle_delay()
}

// 返回该工具是否允许显式 SessionStart 覆盖同 ID 墓碑
pub(crate) fn session_start_revives_tombstone(tool: AiTool) -> bool {
    protocol(tool).session_start_revives_tombstone()
}

/// 查找某工具事件声明的基础状态迁移，供协议契约测试使用。
#[cfg(test)]
pub(super) fn hook_transition(tool: AiTool, event: &str) -> Option<HookTransition> {
    event_definition(tool, event).map(|definition| definition.kind.transition())
}

/// 构造 Claude-Code 兼容协议共用的 `{ hooks: [{ type, command, matcher? }] }` 条目。
pub(super) fn command_group(command: &str, matcher: Option<&str>) -> Value {
    // 构造基础分组：单个 command 类型 handler
    let mut group = json!({
        "hooks": [{
            "type": "command",
            "command": command,
        }]
    });
    // 若传入了 matcher，则补充到分组上，用于按子类型过滤
    if let Some(matcher) = matcher {
        group["matcher"] = Value::String(matcher.to_owned());
    }
    // Claude-Code 兼容协议要求该事件的值是一个分组数组，这里只放入这一个分组
    Value::Array(vec![group])
}

// 按编译目标平台选取应写入配置的命令变体（Windows 用 PowerShell 包装，POSIX 直接执行）。
pub(super) fn platform_command(commands: &ManagedCommands) -> &str {
    if commands.is_wsl {
        // WSL 场景始终使用 POSIX 命令，忽略实际编译目标平台
        return &commands.posix;
    }
    #[cfg(target_os = "windows")]
    {
        // 非 WSL 的 Windows 编译目标使用原生 CMD 命令
        &commands.windows
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        // macOS/Linux 编译目标使用 POSIX 命令
        &commands.posix
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    // 其他编译目标当前不受支持，编译期直接报错
    compile_error!("LokiMetis Hook command generation only supports Windows, macOS, and Linux");
}

// 判断该工具是否需要把每个上游事件都原样转发（不做状态机抑制/合并）
pub(crate) fn forwards_every_event(tool: AiTool) -> bool {
    // 只有具备稳定会话/工作开始语义并经过状态机适配验证的工具执行抑制；
    // 其他协议按事件到达顺序直通，避免公共状态机误丢上游事件。
    !matches!(
        tool,
        AiTool::Codex
            | AiTool::ClaudeCode
            | AiTool::Cursor
            | AiTool::OpenCode
            | AiTool::QwenCode
            | AiTool::KimiCode
            | AiTool::Qoder
            | AiTool::GeminiCli
            | AiTool::GitHubCopilot
            | AiTool::Grok
    )
}

// 判断 hooks 配置条目是否携带该工具的 LokiMetis 管理标识。
fn entry_is_managed<P: HookProtocol + ?Sized>(entry: &Value, protocol: &P) -> bool {
    ["command", "commandWindows"]
        .into_iter()
        // 依次尝试从条目上取出 command / commandWindows 字符串字段
        .filter_map(|key| entry.get(key).and_then(Value::as_str))
        // 只要任一命令字段携带本工具的管理标识，就认为该条目是受管条目
        .any(|command| contains_command_marker(command, protocol.tool()))
}

// 生成 relay 与配置清理共同识别的 `--managed-by` 标识。
/// 生成 relay 与配置清理共同识别的管理标识。
pub fn managed_hook_marker(tool: AiTool) -> String {
    format!("{MANAGED_HOOK_PREFIX}:tool={}", protocol(tool).slug())
}

// 判断配置文件整体内容中是否包含该工具的管理标识（用于独立文件冲突校验）
fn contains_managed_marker(content: &str, tool: AiTool) -> bool {
    content.contains(&managed_hook_marker(tool))
}

// 判断单条命令字符串中是否包含该工具的管理标识（兼容各平台转义形式）
fn contains_command_marker(command: &str, tool: AiTool) -> bool {
    command_has_marker(command, &managed_hook_marker(tool))
}

// 把字符串按单引号包裹，内部已有的单引号转义为 POSIX shell 惯用的 `'"'"'` 形式
pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

// 生成通过 LokiMetis CLI relay 转发单次事件的公共 JS Promise 包装片段
// （`forwardThroughCli`），内建 settle/超时/kill 语义。OpenClaw 与 OpenCode 的
// 独立插件生成共用此片段，仅 relay 子命令不同。
pub(super) fn js_cli_relay_forwarder(relay_subcommand: &str, marker: &str) -> String {
    format!(
        r#"const forwardThroughCli = (hookEvent, body) => new Promise((resolve, reject) => {{
  let settled = false
  let deadline
  const settle = (error) => {{
    if (settled) return
    settled = true
    if (deadline) clearTimeout(deadline)
    if (error) reject(error)
    else resolve()
  }}
  const relay = spawn(relayExecutable, [
    "--loki-metis-hook-relay", "{relay_subcommand}", hookEvent,
    "--managed-by", "{marker}",
  ], {{ stdio: ["pipe", "ignore", "ignore"], windowsHide: true }})
  relay.once("error", settle)
  relay.once("exit", (code) => {{
    if (code === 0) settle()
    else settle(new Error(`LokiMetis CLI relay exited with code ${{code}}`))
  }})
  relay.stdin.once("error", settle)
  deadline = setTimeout(() => {{
    const error = new Error("LokiMetis CLI relay deadline exceeded")
    relay.kill()
    settle(error)
  }}, 4000)
  relay.stdin.end(body, "utf8")
}})"#
    )
}

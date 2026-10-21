// 本文件是 mod.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：构造 Claude Code 兼容协议共用的 `{ hooks: [{ type, command, matcher? }] }` 条目。；按 WSL 状态与编译目标选择应写入配置的命令变体。；判断工具是否应绕过状态机抑制并原样转发每个上游事件。

/// 构造 Claude Code 兼容协议共用的 `{ hooks: [{ type, command, matcher? }] }` 条目。
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
    // Claude Code 兼容协议要求该事件的值是一个分组数组，这里只放入这一个分组
    Value::Array(vec![group])
}

/// 按 WSL 状态与编译目标选择应写入配置的命令变体。
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
    compile_error!("LokiMetris Hook command generation only supports Windows, macOS, and Linux");
}

/// 判断工具是否应绕过状态机抑制并原样转发每个上游事件。
pub(crate) fn forwards_every_event(tool: AiTool) -> bool {
    // 只有具备稳定会话/工作开始语义并经过状态机适配验证的工具执行抑制；
    // 其他协议按事件到达顺序直通，避免公共状态机误丢上游事件。
    !matches!(
        tool,
        AiTool::Codex
            | AiTool::ClaudeCode
            | AiTool::Cursor
            | AiTool::OpenCode
            | AiTool::WorkBuddy
            | AiTool::CodeBuddy
            | AiTool::QwenCode
            | AiTool::KimiCode
            | AiTool::Qoder
            | AiTool::GeminiCli
            | AiTool::GitHubCopilot
            | AiTool::Grok
    )
}

/// 判断单个 Hook 配置条目是否携带指定工具的管理标识。
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

/// 判断配置文件内容是否包含指定工具的管理标识。
fn contains_managed_marker(content: &str, tool: AiTool) -> bool {
    content.contains(&managed_hook_marker(tool))
}

/// 判断单条命令是否包含指定工具的管理标识，并兼容现役平台转义。
fn contains_command_marker(command: &str, tool: AiTool) -> bool {
    command_has_marker(command, &managed_hook_marker(tool))
}

/// 按 POSIX shell 单引号规则安全包裹一个参数。
pub(super) fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// 生成通过 CLI relay 转发单次事件的公共 JavaScript Promise 片段。
///
/// 片段内建单次 settle、超时和进程回收语义，由 OpenClaw 与 OpenCode 共用。
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
    "--loki-metris-hook-relay", "{relay_subcommand}", hookEvent,
    "--managed-by", "{marker}",
  ], {{ stdio: ["pipe", "ignore", "ignore"], windowsHide: true }})
  relay.once("error", settle)
  relay.once("exit", (code) => {{
    if (code === 0) settle()
    else settle(new Error(`LokiMetris CLI relay exited with code ${{code}}`))
  }})
  relay.stdin.once("error", settle)
  deadline = setTimeout(() => {{
    const error = new Error("LokiMetris CLI relay deadline exceeded")
    relay.kill()
    settle(error)
  }}, 4000)
  relay.stdin.end(body, "utf8")
}})"#
    )
}

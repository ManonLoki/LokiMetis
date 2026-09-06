// 引入 Path，用于接收 relay 可执行文件的路径参数
use std::path::Path;

// 引入 serde_json 的 Map/Value/json! 宏，用于拼装和解析 Hook 配置 JSON
use serde_json::{Map, Value, json};

// 引入统一的应用错误类型，供本模块各生成/合并步骤返回可序列化错误
use crate::agent_hooks::HookError;

// 引入协议层的公共类型与工具函数：AiTool 枚举、预览/命令结构体、
// 标识生成函数、按工具分发协议实例的函数，以及 shell 引号转义函数
use super::{
    AiTool, HookConfigPreview, HookProtocol, ManagedCommands, managed_hook_marker, protocol,
    shell_quote,
};

/// 为一个工具生成完整的主配置文件内容。
pub fn generate_hook_config(
    // 目标 AI 工具类型
    tool: AiTool,
    // relay 可执行文件在本机（非 WSL）上的路径
    relay_executable: &Path,
) -> Result<HookConfigPreview, HookError> {
    // 非 WSL 场景：不传入额外的 WSL 可执行路径，直接复用带可执行路径的通用实现
    generate_hook_config_with_executable(tool, relay_executable, None)
}

/// 为运行在 WSL 内的客户端生成 POSIX Hook 命令。relay 本身仍是 Windows
/// LokiMetis GUI 可执行文件，但 executable 已由 application 层通过 wslpath 转换为 Linux 路径。
pub fn generate_wsl_hook_config(
    // 目标 AI 工具类型
    tool: AiTool,
    // Windows 侧 relay 可执行文件路径（用于计算 marker 等，不直接写入命令）
    relay_executable: &Path,
    // 已转换为 WSL/Linux 视角的可执行文件路径字符串，将写入生成的命令
    wsl_executable: &str,
) -> Result<HookConfigPreview, HookError> {
    // 传入 Some(wsl_executable)，触发 WSL 专属的 POSIX 路径处理分支
    generate_hook_config_with_executable(tool, relay_executable, Some(wsl_executable))
}

// 生成主配置文件内容的通用实现，供本机与 WSL 两个公开入口共用
fn generate_hook_config_with_executable(
    // 目标 AI 工具类型
    tool: AiTool,
    // relay 可执行文件路径
    relay_executable: &Path,
    // 可选的 WSL 侧可执行路径；为 None 表示非 WSL 场景
    wsl_executable: Option<&str>,
) -> Result<HookConfigPreview, HookError> {
    // 按工具类型取出对应的协议实现
    let protocol = protocol(tool);
    // 若该工具声明了独立配置文件内容，直接使用它，跳过逐事件生成流程
    if let Some(content) = protocol.standalone_config() {
        return Ok(HookConfigPreview {
            // 独立配置文件的预览路径由协议自身给出
            filename: protocol.preview_filename().to_owned(),
            // 独立配置文件内容原样返回
            content,
        });
    }
    // 用于累积每个事件生成的 handler，键为事件名
    let mut hooks = Map::new();

    // 遍历该工具协议声明的全部事件
    for event in protocol.events() {
        // 为当前事件生成三种平台命令变体
        let commands = managed_commands(protocol, event.name, relay_executable, wsl_executable);
        // 调用协议的 handler 实现，把命令组装成该工具期望的 JSON 结构，并按事件名存入 hooks
        hooks.insert(event.name.to_owned(), protocol.handler(event, &commands));
    }

    Ok(HookConfigPreview {
        // 主配置文件的预览路径由协议给出
        filename: protocol.preview_filename().to_owned(),
        // 交给协议的 render_config 把 hooks 渲染成最终文件内容（默认是 JSON）
        content: protocol.render_config(hooks)?,
    })
}

/// 把新生成的配置与现有文件合并，只替换带管理标识的条目。
pub fn merge_hook_config(
    // 现有配置文件内容；None 表示文件尚不存在
    existing_content: Option<&str>,
    // 本次新生成的配置预览
    generated: &HookConfigPreview,
    // 目标 AI 工具类型
    tool: AiTool,
) -> Result<HookConfigPreview, HookError> {
    // 按工具类型取出对应的协议实现
    let protocol = protocol(tool);
    // 若该工具需要走自定义合并（如独立插件文件、非 JSON 的 TOML），交给协议自身处理
    if protocol.uses_custom_merge() {
        return Ok(HookConfigPreview {
            // 文件名沿用新生成配置的文件名
            filename: generated.filename.clone(),
            // 由协议的 merge_standalone 决定如何与现有内容合并
            content: protocol.merge_standalone(existing_content, generated)?,
        });
    }
    // 走公共 JSON 合并路径：先解析现有内容为 JSON 值
    let mut existing = match existing_content {
        // 已有文件内容：解析失败时返回可序列化错误，附带原始解析错误详情
        Some(content) => serde_json::from_str::<Value>(content).map_err(|error| {
            HookError::new("error.hooks.existingConfigInvalid").param("detail", error.to_string())
        })?,
        // 文件不存在：以空对象作为合并起点
        None => json!({}),
    };
    // 解析新生成的配置内容为 JSON 值，失败时返回可序列化错误
    let generated_value = serde_json::from_str::<Value>(&generated.content).map_err(|error| {
        HookError::new("error.hooks.generatedConfigInvalid").param("detail", error.to_string())
    })?;
    // 取出现有配置的根对象；若根不是对象则返回错误
    let existing_root = existing
        .as_object_mut()
        .ok_or_else(|| HookError::new("error.hooks.existingConfigRootNotObject"))?;
    // 取出新生成配置的根对象；若根不是对象则返回错误
    let generated_root = generated_value
        .as_object()
        .ok_or_else(|| HookError::new("error.hooks.generatedConfigRootNotObject"))?;

    // 遍历新生成配置根对象的每个键值对，用于同步除 hooks 外的根级元数据字段
    for (key, value) in generated_root {
        if key != "hooks" {
            // `version` 等由协议生成的根元数据属于该 Hook 文件契约；每次写入都
            // 恢复为当前受支持值，同时继续保留生成器未声明的用户根字段。
            existing_root.insert(key.clone(), value.clone());
        }
        // 若键是 "hooks"，则跳过，交由后面专门的 hooks 合并逻辑处理
    }

    // 取出（或初始化）现有配置中的 hooks 对象，用于后续按事件合并
    let existing_hooks = existing_root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| HookError::new("error.hooks.existingHooksNotObject"))?;
    // 取出新生成配置中的 hooks 对象；若缺失或类型不对则返回错误
    let generated_hooks = generated_root
        .get("hooks")
        .and_then(Value::as_object)
        .ok_or_else(|| HookError::new("error.hooks.generatedHooksMissing"))?;
    // 先清理现有 hooks 中属于本工具的旧受管条目，避免重复合并后残留过期命令
    for event in existing_hooks.keys().cloned().collect::<Vec<_>>() {
        // 对每个事件的条目数组执行“移除受管条目”，并判断是否应整体移除该事件键
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            // 若该事件的值不是数组，视为不应移除（保留原样，交由后续覆盖逻辑处理）
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            // 委托协议实现按其自身的条目结构移除受管 handler
            protocol.remove_managed_entries(entries);
            // 移除后若数组为空，说明该事件下已无剩余条目，应整体删除该事件键
            entries.is_empty()
        });
        if should_remove {
            // 数组已清空，删除该事件键，避免留下空数组
            existing_hooks.remove(&event);
        }
    }

    // 把新生成的每个事件条目追加进现有 hooks，与用户手动添加的其他条目共存
    for (event, generated_entries) in generated_hooks {
        // 新生成条目必须是数组；理论上由协议保证，此处仍做防御性校验
        let generated_entries = generated_entries.as_array().ok_or_else(|| {
            HookError::new("error.hooks.generatedEventNotArray").param("event", event.clone())
        })?;
        // 取出（或初始化）现有配置中同名事件的条目数组
        let existing_entries = existing_hooks
            .entry(event.clone())
            .or_insert_with(|| Value::Array(Vec::new()))
            .as_array_mut()
            .ok_or_else(|| {
                HookError::new("error.hooks.existingEventNotArray").param("event", event.clone())
            })?;
        // 把新生成的条目追加到现有数组末尾（旧的受管条目已在上一步被清理）
        existing_entries.extend(generated_entries.iter().cloned());
    }

    Ok(HookConfigPreview {
        // 合并结果沿用新生成配置的文件名
        filename: generated.filename.clone(),
        // 把合并后的 JSON 值格式化输出；序列化失败时返回可序列化错误
        content: serde_json::to_string_pretty(&existing).map_err(|error| {
            HookError::new("error.hooks.mergeFailed").param("detail", error.to_string())
        })?,
    })
}

/// 从现有公共 JSON Hook 配置中只移除指定工具的 LokiMetis 受管条目。
///
/// 没有受管条目时逐字返回原文；清理后仍有用户、其它工具或根级内容时返回
/// 清理后的 JSON；只有本次确实移除了条目且整个文档仅剩空 `hooks` 时返回 `None`。
/// 独立插件和 TOML 等自定义合并协议不经过这个 JSON 清理入口。
pub fn remove_managed_hook_entries(
    tool: AiTool,
    existing_content: &str,
) -> Result<Option<String>, HookError> {
    let protocol = protocol(tool);
    if protocol.uses_custom_merge() {
        return Err(HookError::new("error.hooks.cleanupUnsupported"));
    }
    let mut existing = serde_json::from_str::<Value>(existing_content).map_err(|error| {
        HookError::new("error.hooks.existingConfigInvalid").param("detail", error.to_string())
    })?;
    let existing_root = existing
        .as_object_mut()
        .ok_or_else(|| HookError::new("error.hooks.existingConfigRootNotObject"))?;
    let Some(existing_hooks) = existing_root.get_mut("hooks") else {
        return Ok(Some(existing_content.to_owned()));
    };
    let existing_hooks = existing_hooks
        .as_object_mut()
        .ok_or_else(|| HookError::new("error.hooks.existingHooksNotObject"))?;
    let mut changed = false;
    for event in existing_hooks.keys().cloned().collect::<Vec<_>>() {
        let should_remove = existing_hooks.get_mut(&event).is_some_and(|entries| {
            let Some(entries) = entries.as_array_mut() else {
                return false;
            };
            let before = entries.clone();
            protocol.remove_managed_entries(entries);
            changed |= *entries != before;
            entries.is_empty()
        });
        if should_remove {
            existing_hooks.remove(&event);
        }
    }
    if !changed {
        return Ok(Some(existing_content.to_owned()));
    }
    let only_empty_hooks = existing.as_object().is_some_and(|root| {
        root.len() == 1
            && root
                .get("hooks")
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
    });
    if only_empty_hooks {
        return Ok(None);
    }
    serde_json::to_string_pretty(&existing)
        .map(Some)
        .map_err(|error| {
            HookError::new("error.hooks.mergeFailed").param("detail", error.to_string())
        })
}

// 为一个事件生成三种平台变体的托管命令字符串（POSIX shell、Windows CMD、
// 经 PowerShell 转发到 CMD），供各工具的 `handler` 按自身协议组装配置条目。
fn managed_commands(
    // 当前工具的协议实现，用于取 slug 等信息
    protocol: &dyn HookProtocol,
    // 当前事件名，会作为参数写入命令
    event: &str,
    // relay 可执行文件路径（本机视角）
    relay_executable: &Path,
    // 可选的 WSL 侧可执行路径；Some 表示当前是 WSL 场景
    wsl_executable: Option<&str>,
) -> ManagedCommands {
    // 计算该工具的 LokiMetis 管理标识，写入命令末尾的 --managed-by 参数
    let marker = managed_hook_marker(protocol.tool());
    // 把路径转换为字符串（有损转换，处理非法 UTF-8 时用替换字符）
    let executable = relay_executable.to_string_lossy().into_owned();
    // 计算 POSIX 命令要使用的可执行路径：WSL 场景直接用传入的 Linux 路径；
    // 非 WSL 的 Windows 场景把反斜杠替换为正斜杠，兼容 POSIX 风格 shell；
    // 其他平台直接使用原始路径
    let posix_executable = if let Some(wsl_executable) = wsl_executable {
        wsl_executable.to_owned()
    } else if cfg!(windows) {
        executable.replace('\\', "/")
    } else {
        executable.clone()
    };
    // 拼装 POSIX shell 命令：可执行路径、relay 子命令、工具 slug、事件名、管理标识，
    // 每个参数都经过 shell 引号转义
    let posix = format!(
        "{} --loki-metis-hook-relay {} {} --managed-by {}",
        shell_quote(&posix_executable),
        shell_quote(protocol.slug()),
        shell_quote(event),
        shell_quote(&marker),
    );
    // 拼装 Windows CMD 命令：用 cmd.exe /d /s /c 包裹，参数按 CMD 双引号规则转义
    let windows = format!(
        "cmd.exe /d /s /c \"{} --loki-metis-hook-relay {} {} --managed-by {}\"",
        windows_quote(&executable),
        windows_quote(protocol.slug()),
        windows_quote(event),
        windows_quote(&marker),
    );
    // 拼装经 PowerShell 宿主转发到 CMD 的命令：需要额外的反引号转义，
    // 用于同时保住含空格路径和参数边界
    let windows_powershell_host = format!(
        "cmd.exe /d /s /c \"{} --loki-metis-hook-relay {} {} --managed-by {}\"",
        powershell_host_quote(&executable),
        powershell_host_quote(protocol.slug()),
        powershell_host_quote(event),
        powershell_host_quote(&marker),
    );
    ManagedCommands {
        // POSIX/WSL 场景使用的命令
        posix,
        // 标记当前命令是否属于 WSL 场景，供 platform_command 选择命令变体
        is_wsl: wsl_executable.is_some(),
        // 原生 Windows CMD 命令
        windows,
        // 经 PowerShell 宿主转发到 CMD 的命令变体
        windows_powershell_host,
    }
}

// 按 Windows CMD 双引号规则转义一个参数。
fn windows_quote(value: &str) -> String {
    // 用双引号包裹整体，内部已有的双引号按 CMD 规则转义为两个双引号
    format!("\"{}\"", value.replace('"', "\"\""))
}

// PowerShell 会先解析整条 command，再把 `/c` 后的文本交给 CMD。用反引号保护
// 内层双引号，才能同时保住含空格的安装路径和参数边界。
fn powershell_host_quote(value: &str) -> String {
    // 先转义值内部已有的反引号，避免与后续转义规则冲突；
    // 再把双引号转义为反引号+双引号，交由 PowerShell 解析后仍保留给 CMD 的双引号
    format!("`\"{}`\"", value.replace('`', "``").replace('"', "`\""))
}

// 判断一条当前格式的命令字符串是否携带指定管理标识。Windows PowerShell
// 宿主可能保留用于保护双引号的反引号，因此比较前只归一化该现役转义形式。
pub(super) fn command_has_marker(command: &str, marker: &str) -> bool {
    // 闭包：判断给定字符串是否包含 `marker'` 或 `marker"`，
    // 即标识后紧跟命令引号收尾，避免误判成标识的前缀子串
    let contains_marker = |value: &str| {
        value.contains(&format!("{marker}'")) || value.contains(&format!("{marker}\""))
    };
    // 先按原始字符串判断；再把 PowerShell 宿主转义的反引号双引号还原成普通双引号后再判断一次
    contains_marker(command) || contains_marker(&command.replace("`\"", "\""))
}

// 以下四个测试子模块分别覆盖：通用生成/协议契约、Kimi Code 专属场景、
// 合并逻辑、以及第二批新增工具（Qwen/Qoder/Gemini/Copilot 等）
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_kimi;
#[cfg(test)]
mod tests_merge;

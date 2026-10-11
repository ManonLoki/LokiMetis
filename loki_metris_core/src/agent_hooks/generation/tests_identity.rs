//! 新旧产品共享第三方配置时只能认领自身管理标识。

use super::*;

/// 升级时继续认领旧 CMD/PowerShell 宿主格式，只替换本产品而保留用户命令。
#[cfg(windows)]
#[test]
fn legacy_windows_host_commands_upgrade_to_one_encoded_relay() {
    for command in [
        r#"cmd.exe /d /s /c ""C:\Old App\relay.exe" --loki-metris-hook-relay "codex" "SessionStart" --managed-by "LokiMetris:tool=codex"""#,
        r#"cmd.exe /d /s /c "`"C:\Old App\relay.exe`" --loki-metris-hook-relay `"codex`" `"SessionStart`" --managed-by `"LokiMetris:tool=codex`"""#,
    ] {
        let old = json!({"keep":true,"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":command},{"type":"command","command":"user-event"}]}]}}).to_string();
        let generated = generate_hook_config(AiTool::Codex, Path::new(r"C:\New App\relay.exe")).unwrap();
        let merged = merge_hook_config(Some(&old), &generated, AiTool::Codex).unwrap();
        let actual: Value = serde_json::from_str(&merged.content).unwrap();
        let commands: Vec<_> = actual["hooks"]["SessionStart"].as_array().unwrap().iter().flat_map(|group| group["hooks"].as_array().unwrap()).filter_map(|hook| hook["command"].as_str()).collect();
        assert_eq!(commands.iter().filter(|command| command_has_marker(command, "LokiMetris:tool=codex")).count(), 1);
        assert!(commands.contains(&"user-event"));
        assert!(commands.iter().any(|command| command.starts_with(windows_command::PREFIX)));
        assert!(!tests::contract_text(&merged).contains("Old App"));
        let cleaned = remove_managed_hook_entries(AiTool::Codex, &merged.content).unwrap().unwrap();
        assert!(cleaned.contains("user-event"));
        assert!(!cleaned.contains(windows_command::PREFIX));
        assert_eq!(serde_json::from_str::<Value>(&cleaned).unwrap()["keep"], true);
    }
}

/// 每种展开字符都经真实生成入口选用编码命令，更新、幂等合并与删除只处理本应用。
#[cfg(windows)]
#[test]
fn special_windows_installation_paths_generate_replace_and_remove_owned_hooks() {
    let tool = AiTool::ClaudeCode;
    let old = generate_hook_config(tool, Path::new(r"C:\ordinary\relay.exe")).unwrap();
    assert!(old.content.contains(windows_command::PREFIX));
    for character in ['$', '%', '!', '`', '‘', '’', '‚', '‛', '“', '”', '„'] {
        let path = format!(r"C:\special{character}\relay.exe");
        let generated = generate_hook_config(tool, Path::new(&path)).unwrap();
        assert!(generated.content.contains(windows_command::PREFIX), "{character}");
        let merged = merge_hook_config(Some(&old.content), &generated, tool).unwrap();
        let repeated = merge_hook_config(Some(&merged.content), &generated, tool).unwrap();
        assert_eq!(merged.content, repeated.content);
        assert!(!tests::contract_text(&merged).contains("ordinary"));
        let cleaned = remove_managed_hook_entries(tool, &merged.content).unwrap();
        assert!(cleaned.is_none_or(|cleaned| !cleaned.contains(windows_command::PREFIX)));
    }
}

/// 旧 LokiMetis 条目在合并和清理后完整保留，新条目仍可独立移除。
#[test]
fn old_application_json_hooks_are_preserved() {
    let old=r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"'/old/LokiMetis' --loki-metis-hook-relay claude-code SessionStart --managed-by 'LokiMetis:tool=claude-code'"}]}]}}"#;
    let preview=generate_hook_config(AiTool::ClaudeCode,Path::new("/new/LokiMetris")).unwrap();
    let merged=merge_hook_config(Some(old),&preview,AiTool::ClaudeCode).unwrap();
    assert!(merged.content.contains("LokiMetis:tool=claude-code"));assert!(tests::contract_text(&merged).contains("LokiMetris:tool=claude-code"));
    let cleaned=remove_managed_hook_entries(AiTool::ClaudeCode,&merged.content).unwrap().unwrap();
    assert_eq!(serde_json::from_str::<Value>(&cleaned).unwrap(),serde_json::from_str::<Value>(old).unwrap());
    assert_eq!(remove_managed_hook_entries(AiTool::ClaudeCode,old).unwrap().as_deref(),Some(old));
}

/// 独立插件路径及内容管理标识均隔离，不允许覆盖旧产品或第三方文件。
#[test]
fn old_application_plugin_is_foreign_and_new_paths_are_distinct() {
    for tool in [AiTool::OpenCode,AiTool::Hermes,AiTool::OpenClaw,AiTool::GitHubCopilot,AiTool::Grok] {
        let preview=generate_hook_config(tool,Path::new("/new/LokiMetris")).unwrap();
        assert!(preview.filename.contains("lokimetris"));assert!(!preview.filename.contains("lokimetis"));
    }
    let preview=generate_hook_config(AiTool::OpenCode,Path::new("/new/LokiMetris")).unwrap();
    let old="// LokiMetis:tool=opencode\nexport const LokiMetisPlugin = () => ({})";
    assert_eq!(merge_hook_config(Some(old),&preview,AiTool::OpenCode).unwrap_err().code,"error.hooks.foreignFileRejected");
}

/// Kimi 的旧受管边界按外部文本保留，新区块只追加自身身份。
#[test]
fn old_application_kimi_block_remains_untouched() {
    let old="# LokiMetis:tool=kimi-code begin\n[[hooks]]\nevent = \"SessionStart\"\ncommand = \"old command\"\n# LokiMetis:tool=kimi-code end\n";
    let preview=generate_hook_config(AiTool::KimiCode,Path::new("/new/LokiMetris")).unwrap();
    let merged=merge_hook_config(Some(old),&preview,AiTool::KimiCode).unwrap();
    assert!(merged.content.starts_with(old));assert!(merged.content.contains("# LokiMetris:tool=kimi-code begin"));
}

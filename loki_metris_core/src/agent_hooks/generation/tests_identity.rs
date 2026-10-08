//! 新旧产品共享第三方配置时只能认领自身管理标识。

use super::*;

/// 旧 LokiMetis 条目在合并和清理后完整保留，新条目仍可独立移除。
#[test]
fn old_application_json_hooks_are_preserved() {
    let old=r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"'/old/LokiMetis' --loki-metis-hook-relay claude-code SessionStart --managed-by 'LokiMetis:tool=claude-code'"}]}]}}"#;
    let preview=generate_hook_config(AiTool::ClaudeCode,Path::new("/new/LokiMetris")).unwrap();
    let merged=merge_hook_config(Some(old),&preview,AiTool::ClaudeCode).unwrap();
    assert!(merged.content.contains("LokiMetis:tool=claude-code"));assert!(merged.content.contains("LokiMetris:tool=claude-code"));
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

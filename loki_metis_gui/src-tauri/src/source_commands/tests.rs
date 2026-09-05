use crate::backend::local_index::PARSER_VERSION;
use crate::commands::ensure_business_access;
use crate::dto::AgentClientKindDto;
use crate::runtime::AppRuntimeState;
use crate::source_commands::support::{
    acquire_source_root_write_permit, parser_version_for_client, to_source_client_kind,
};
use loki_metis_core::{
    SourceClientKind, ensure_primary_source_root_supported, normalize_source_root_alias,
    validate_source_root_id,
};

/// 看板不接源项目向导；数据源管理在无向导时即可访问。
#[tokio::test]
async fn manual_source_management_does_not_require_wizard() {
    let temp = tempfile::tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());
    assert_eq!(ensure_business_access(&state).await, Ok(()));
}

/// 验证数据根写许可与该客户端扫描 writer 互斥，释放后才能开始扫描。
#[test]
fn source_registration_holds_the_client_scan_writer() {
    let temp = tempfile::tempdir().expect("isolated app-data is available");
    let state = AppRuntimeState::new(temp.path().to_path_buf());

    for client in AgentClientKindDto::ALL {
        let permit = acquire_source_root_write_permit(&state, client)
            .expect("idle client grants root mutation permit");
        assert!(state.local_scan.get(client.into()).try_start().is_err());
        drop(permit);
        let scan_permit = state
            .local_scan
            .get(client.into())
            .try_start()
            .expect("released mutation permit allows the next scan");
        drop(scan_permit);
    }
}

/// 验证根 ID 校验只接受各客户端固定前缀加稳定散列，拒绝路径或跨客户端混用。
#[test]
fn accepts_only_stable_root_ids() {
    let codex_source_client = to_source_client_kind(AgentClientKindDto::Codex);
    let claude_source_client = to_source_client_kind(AgentClientKindDto::ClaudeCode);
    assert!(validate_source_root_id(codex_source_client, "root-0123456789abcdef").is_ok());
    assert!(validate_source_root_id(claude_source_client, "claude-root-0123456789abcdef").is_ok());
    assert!(validate_source_root_id(codex_source_client, "/Users/example/.codex").is_err());
    assert!(validate_source_root_id(codex_source_client, "root-not-a-hash").is_err());
    assert!(validate_source_root_id(claude_source_client, "root-0123456789abcdef").is_err());
}

/// 验证别名校验会裁剪空白、拒绝路径样式与超长/空白输入。
#[test]
fn accepts_only_safe_root_aliases() {
    assert_eq!(
        normalize_source_root_alias("  工作数据根  "),
        Ok("工作数据根".to_owned())
    );
    assert!(normalize_source_root_alias("/Users/example/.codex").is_err());
    assert!(normalize_source_root_alias(" ").is_err());
    assert!(normalize_source_root_alias(&"x".repeat(65)).is_err());
}

/// 验证主数据目录选择只对 Codex 客户端开放，Claude Code 不支持。
#[test]
fn primary_root_is_codex_only() {
    assert!(
        ensure_primary_source_root_supported(to_source_client_kind(AgentClientKindDto::Codex))
            .is_ok()
    );
    assert!(
        ensure_primary_source_root_supported(to_source_client_kind(AgentClientKindDto::ClaudeCode))
            .is_err()
    );
}

/// 验证各客户端返回的 parser version 与 core 定义及 Codex 写入 generation 一致。
#[test]
fn parser_version_matches_expected_client() {
    assert_eq!(SourceClientKind::Codex.parser_version(), PARSER_VERSION);
    assert_eq!(SourceClientKind::Codex.parser_version(), 8);
    assert_eq!(SourceClientKind::ClaudeCode.parser_version(), 4);
    assert_eq!(SourceClientKind::GrokBuildCli.parser_version(), 3);
    assert_eq!(
        parser_version_for_client(AgentClientKindDto::Codex),
        SourceClientKind::Codex.parser_version()
    );
    assert_eq!(
        parser_version_for_client(AgentClientKindDto::ClaudeCode),
        SourceClientKind::ClaudeCode.parser_version()
    );
    assert_eq!(
        parser_version_for_client(AgentClientKindDto::GrokBuildCli),
        SourceClientKind::GrokBuildCli.parser_version()
    );
}

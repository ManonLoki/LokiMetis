use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tauri::{Manager, path::BaseDirectory};

const RELEASE_NOTES_RESOURCE_PATH: &str = "release-notes.json";
const RELEASE_NOTES_SCHEMA_VERSION: u8 = 2;
const MAX_RELEASE_NOTE_VERSIONS: usize = 5;
const MAX_RELEASE_NOTE_ITEMS: usize = 10;
const MAX_RELEASE_NOTES_BYTES: u64 = 1024 * 1024;

/// 表示设置页可读取的整个更新日志资源。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseNotesDocument {
    schema_version: u8,
    releases: Vec<ReleaseNoteEntry>,
}

/// 表示一个正式发布版本的两类双语用户可见更新。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseNoteEntry {
    release_date: String,
    version: String,
    feature_optimizations: Vec<LocalizedReleaseNoteItem>,
    bug_fixes: Vec<LocalizedReleaseNoteItem>,
}

/// 把一个发布事实的中英文文案绑定为不可拆分的翻译对。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LocalizedReleaseNoteItem {
    #[serde(rename = "zh-CN")]
    zh_cn: String,
    #[serde(rename = "en-US")]
    en_us: String,
}

/// 向前端暴露稳定、无本机路径细节的资源读取失败类型。
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseNotesLoadError {
    Unavailable,
    TooLarge,
    Invalid,
}

/// 从 Tauri 候选的只读资源目录异步读取并验证更新日志。
#[tauri::command]
pub async fn load_release_notes(
    app: tauri::AppHandle,
) -> Result<ReleaseNotesDocument, ReleaseNotesLoadError> {
    let resource_path = app
        .path()
        .resolve(RELEASE_NOTES_RESOURCE_PATH, BaseDirectory::Resource)
        .map_err(|_| ReleaseNotesLoadError::Unavailable)?;
    let metadata = tokio::fs::symlink_metadata(&resource_path)
        .await
        .map_err(|_| ReleaseNotesLoadError::Unavailable)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(ReleaseNotesLoadError::Unavailable);
    }
    if metadata.len() > MAX_RELEASE_NOTES_BYTES {
        return Err(ReleaseNotesLoadError::TooLarge);
    }
    let bytes = tokio::fs::read(resource_path)
        .await
        .map_err(|_| ReleaseNotesLoadError::Unavailable)?;
    if bytes.len() as u64 > MAX_RELEASE_NOTES_BYTES {
        return Err(ReleaseNotesLoadError::TooLarge);
    }
    parse_release_notes(&bytes)
}

/// 把 IPC 前的 JSON 字节收窄为固定 schema，并拒绝顺序或上限漂移。
fn parse_release_notes(bytes: &[u8]) -> Result<ReleaseNotesDocument, ReleaseNotesLoadError> {
    let document: ReleaseNotesDocument =
        serde_json::from_slice(bytes).map_err(|_| ReleaseNotesLoadError::Invalid)?;
    validate_release_notes(&document)?;
    Ok(document)
}

/// 校验近五版、十条上限、唯一版本和最新在前的发布事实。
fn validate_release_notes(document: &ReleaseNotesDocument) -> Result<(), ReleaseNotesLoadError> {
    if document.schema_version != RELEASE_NOTES_SCHEMA_VERSION
        || document.releases.is_empty()
        || document.releases.len() > MAX_RELEASE_NOTE_VERSIONS
    {
        return Err(ReleaseNotesLoadError::Invalid);
    }
    let mut versions = HashSet::new();
    let mut previous_date: Option<&str> = None;
    for release in &document.releases {
        if !is_valid_release_date(&release.release_date)
            || !is_valid_display_version(&release.version)
            || !versions.insert(release.version.as_str())
            || release.feature_optimizations.len() > MAX_RELEASE_NOTE_ITEMS
            || release.bug_fixes.len() > MAX_RELEASE_NOTE_ITEMS
            || release.feature_optimizations.is_empty() && release.bug_fixes.is_empty()
            || !has_unique_non_empty_items(&release.feature_optimizations)
            || !has_unique_non_empty_items(&release.bug_fixes)
        {
            return Err(ReleaseNotesLoadError::Invalid);
        }
        if previous_date.is_some_and(|date| date < release.release_date.as_str()) {
            return Err(ReleaseNotesLoadError::Invalid);
        }
        previous_date = Some(&release.release_date);
    }
    Ok(())
}

/// 判断单个语言字段非空、没有空白包围且在该分类下唯一。
fn is_clean_unique_text<'a>(text: &'a str, seen: &mut HashSet<&'a str>) -> bool {
    !text.is_empty() && text.trim() == text && seen.insert(text)
}

/// 要求每个翻译对非空、没有空白包围且同分类逐语言去重。
fn has_unique_non_empty_items(items: &[LocalizedReleaseNoteItem]) -> bool {
    let mut unique_zh_cn = HashSet::new();
    let mut unique_en_us = HashSet::new();
    items.iter().all(|item| {
        is_clean_unique_text(item.zh_cn.as_str(), &mut unique_zh_cn)
            && is_clean_unique_text(item.en_us.as_str(), &mut unique_en_us)
    })
}

/// 接受带且只带一个小写 v 的 SemVer 或 Harness 十二位时间版本。
fn is_valid_display_version(version: &str) -> bool {
    let Some(machine_version) = version.strip_prefix('v') else {
        return false;
    };
    if matches!(machine_version.as_bytes().first(), Some(b'v') | Some(b'V')) {
        return false;
    }
    if machine_version.len() == 12 && machine_version.bytes().all(|byte| byte.is_ascii_digit()) {
        return true;
    }
    let components: Vec<&str> = machine_version.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && component.parse::<u16>().is_ok_and(|value| value <= 100)
        })
}

/// 校验规范 YYYY-MM-DD，包括闰年和每月实际天数。
fn is_valid_release_date(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return false;
    }
    let Ok(year) = value[0..4].parse::<u16>() else {
        return false;
    };
    let Ok(month) = value[5..7].parse::<u8>() else {
        return false;
    };
    let Ok(day) = value[8..10].parse::<u8>() else {
        return false;
    };
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap_year = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        2 if leap_year => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=days_in_month).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真实 schema 能被解析，并保持两类发布内容中的中英文翻译对。
    #[test]
    fn parses_valid_release_notes_resource() {
        let document = parse_release_notes(
            r#"{
                "schemaVersion": 2,
                "releases": [
                    {
                        "releaseDate": "2026-08-27",
                        "version": "v1.2.3",
                        "featureOptimizations": [
                            {"zh-CN": "新增候选内更新日志", "en-US": "Add bundled release notes"}
                        ],
                        "bugFixes": []
                    }
                ]
            }"#
            .as_bytes(),
        )
        .expect("valid release notes should parse");

        assert_eq!(document.releases.len(), 1);
        assert_eq!(document.releases[0].version, "v1.2.3");
    }

    /// 未知字段、重复条目和多重 v 前缀都必须失败关闭。
    #[test]
    fn rejects_invalid_release_notes_resource() {
        for invalid in [
            r#"{"schemaVersion":2,"releases":[],"extra":true}"#.as_bytes(),
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-08-27","version":"vv1.2.3","featureOptimizations":[{"zh-CN":"重复","en-US":"duplicate"},{"zh-CN":"重复","en-US":"duplicate"}],"bugFixes":[]}]}"#.as_bytes(),
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-08-27","version":"v1.2.3","featureOptimizations":[{"zh-CN":"缺少英文"}],"bugFixes":[]}]}"#.as_bytes(),
        ] {
            assert!(matches!(
                parse_release_notes(invalid),
                Err(ReleaseNotesLoadError::Invalid)
            ));
        }
    }

    /// 日期必须有效且按最新在前排列，不能仅满足字符串外形。
    #[test]
    fn rejects_invalid_or_out_of_order_release_dates() {
        assert!(!is_valid_release_date("2026-02-29"));
        assert!(is_valid_release_date("2028-02-29"));
        let invalid = r#"{
            "schemaVersion": 2,
            "releases": [
                {"releaseDate":"2026-08-26","version":"v1.0.1","featureOptimizations":[{"zh-CN":"一","en-US":"one"}],"bugFixes":[]},
                {"releaseDate":"2026-08-27","version":"v1.0.0","featureOptimizations":[{"zh-CN":"二","en-US":"two"}],"bugFixes":[]}
            ]
        }"#
        .as_bytes();
        assert!(matches!(
            parse_release_notes(invalid),
            Err(ReleaseNotesLoadError::Invalid)
        ));
    }
}

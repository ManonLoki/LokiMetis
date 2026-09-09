use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use tauri::{Manager, path::BaseDirectory};

const RELEASE_NOTES_RESOURCE_PATH: &str = "release-notes.json";
const RELEASE_NOTES_SCHEMA_VERSION: u8 = 2;
const MAX_RELEASE_NOTE_VERSIONS: usize = 5;
const MAX_RELEASE_NOTE_ITEMS: usize = 10;
const MAX_RELEASE_NOTES_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 表示随应用打包的版本化更新日志文档。
pub(crate) struct ReleaseNotesDocument {
    schema_version: u8,
    releases: Vec<ReleaseNoteEntry>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// 保存单个版本的日期及双语功能与修复条目。
struct ReleaseNoteEntry {
    release_date: String,
    version: String,
    feature_optimizations: Vec<LocalizedReleaseNoteItem>,
    bug_fixes: Vec<LocalizedReleaseNoteItem>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
/// 保存一条更新说明的简体中文与英文文本。
struct LocalizedReleaseNoteItem {
    #[serde(rename = "zh-CN")]
    zh_cn: String,
    #[serde(rename = "en-US")]
    en_us: String,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
/// 区分更新日志资源不可用、过大或格式无效的失败类型。
pub(crate) enum ReleaseNotesLoadError {
    Unavailable,
    TooLarge,
    Invalid,
}

/// 只读取固定的随包资源，不允许调用方提供任意文件系统路径。
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

/// 解析并验证更新日志资源字节。
fn parse_release_notes(bytes: &[u8]) -> Result<ReleaseNotesDocument, ReleaseNotesLoadError> {
    let document: ReleaseNotesDocument =
        serde_json::from_slice(bytes).map_err(|_| ReleaseNotesLoadError::Invalid)?;
    validate_release_notes(&document)?;
    Ok(document)
}

/// 验证文档版本、条目上限、日期顺序和双语内容唯一性。
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

/// 判断文本非空、无首尾空白且未在同一语言集合中重复。
fn is_clean_unique_text<'a>(text: &'a str, seen: &mut HashSet<&'a str>) -> bool {
    !text.is_empty() && text.trim() == text && seen.insert(text)
}

/// 检查每条双语说明在各自语言中均非空且唯一。
fn has_unique_non_empty_items(items: &[LocalizedReleaseNoteItem]) -> bool {
    let mut unique_zh_cn = HashSet::new();
    let mut unique_en_us = HashSet::new();
    items.iter().all(|item| {
        is_clean_unique_text(item.zh_cn.as_str(), &mut unique_zh_cn)
            && is_clean_unique_text(item.en_us.as_str(), &mut unique_en_us)
    })
}

/// 验证供界面展示的 `vMAJOR.MINOR.PATCH` 版本格式与数值范围。
fn is_valid_display_version(version: &str) -> bool {
    let Some(machine_version) = version.strip_prefix('v') else {
        return false;
    };
    if matches!(machine_version.as_bytes().first(), Some(b'v') | Some(b'V')) {
        return false;
    }
    let components: Vec<&str> = machine_version.split('.').collect();
    components.len() == 3
        && components.iter().all(|component| {
            !component.is_empty()
                && component.bytes().all(|byte| byte.is_ascii_digit())
                && component.parse::<u16>().is_ok_and(|value| value <= 100)
        })
}

/// 验证公历日期采用有效的 `YYYY-MM-DD` 格式。
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

    /// 有效的双语更新日志资源应完成解析。
    #[test]
    fn parses_valid_release_notes_resource() {
        let document = parse_release_notes(
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-09-05","version":"v0.1.0","featureOptimizations":[{"zh-CN":"新增更新日志","en-US":"Add release notes"}],"bugFixes":[]}]}"#.as_bytes(),
        )
        .expect("valid release notes should parse");
        assert_eq!(document.releases[0].version, "v0.1.0");
    }

    /// 未知字段、非法版本、重复条目或缺少翻译的资源应被拒绝。
    #[test]
    fn rejects_invalid_release_notes_resource() {
        for invalid in [
            r#"{"schemaVersion":2,"releases":[],"extra":true}"#.as_bytes(),
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-09-05","version":"v202609050957","featureOptimizations":[{"zh-CN":"时间版本","en-US":"time version"}],"bugFixes":[]}]}"#.as_bytes(),
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-09-05","version":"vv0.1.0","featureOptimizations":[{"zh-CN":"重复","en-US":"duplicate"},{"zh-CN":"重复","en-US":"duplicate"}],"bugFixes":[]}]}"#.as_bytes(),
            r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-09-05","version":"v0.1.0","featureOptimizations":[{"zh-CN":"缺少英文"}],"bugFixes":[]}]}"#.as_bytes(),
        ] {
            assert!(matches!(
                parse_release_notes(invalid),
                Err(ReleaseNotesLoadError::Invalid)
            ));
        }
    }

    /// 无效日期或按时间升序排列的发布记录应被拒绝。
    #[test]
    fn rejects_invalid_or_out_of_order_release_dates() {
        assert!(!is_valid_release_date("2026-02-29"));
        assert!(is_valid_release_date("2028-02-29"));
        let invalid = r#"{"schemaVersion":2,"releases":[{"releaseDate":"2026-09-04","version":"v0.1.1","featureOptimizations":[{"zh-CN":"一","en-US":"one"}],"bugFixes":[]},{"releaseDate":"2026-09-05","version":"v0.1.0","featureOptimizations":[{"zh-CN":"二","en-US":"two"}],"bugFixes":[]}]}"#.as_bytes();
        assert!(matches!(
            parse_release_notes(invalid),
            Err(ReleaseNotesLoadError::Invalid)
        ));
    }
}

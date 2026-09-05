//! Grok home 内 `sessions/<cwd>/<session>/updates.jsonl` 的路径硬规则。

use std::path::{Component, Path};

/// 判定相对路径是否为可索引的 Grok 完成轮次文件。
pub fn is_grok_updates_path(relative: &Path) -> bool {
    let parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    parts.len() == 4
        && parts[0] == "sessions"
        && !parts[1].is_empty()
        && parts[1] != "logs"
        && is_session_dir_name(parts[2])
        && parts[3] == "updates.jsonl"
}

/// 会话目录接受 UUID 或其它不含路径分隔符的有界标识。
fn is_session_dir_name(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 128
        && !trimmed.starts_with('.')
        && !trimmed.contains(['/', '\\'])
}

/// 从相对路径取出 URL 编码工作目录名，供项目展示标签使用。
pub fn grok_project_seed(relative: &Path) -> Option<String> {
    let mut parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        });
    let first = parts.next()?;
    let cwd = parts.next()?;
    (first == "sessions").then(|| cwd.to_owned())
}

/// 从相对路径取出会话目录名，供线程键使用。
pub fn grok_session_seed(relative: &Path) -> Option<String> {
    let parts = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>();
    (parts.len() >= 3 && parts[0] == "sessions").then(|| parts[2].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// 只接受四级 `sessions/<cwd>/<session>/updates.jsonl`，拒绝 logs 与其它文件。
    #[test]
    fn accepts_only_session_updates_jsonl() {
        assert!(is_grok_updates_path(Path::new(
            "sessions/%2Ftmp%2Fapp/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee/updates.jsonl"
        )));
        assert!(!is_grok_updates_path(Path::new("logs/unified.jsonl")));
        assert!(!is_grok_updates_path(Path::new(
            "sessions/cwd/session/summary.json"
        )));
        assert!(!is_grok_updates_path(Path::new("auth.json")));
    }
}

use std::path::Path;

use super::SourceClientKind;

/// 仅用于展示的来源根别名：优先取目录名，缺失时回退客户端默认别名。
pub fn source_root_alias_from_path(path: &Path, source_client: SourceClientKind) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| source_client.default_source_root_alias().to_owned())
}

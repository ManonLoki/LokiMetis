//! 提供来源根标识查找的纯业务辅助，便于不同载体在共享策略层复用。

use thiserror::Error;

/// 表示来源根在持有集合中的引用状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum SourceRootLookupError {
    /// 指定标识的来源根不存在。
    #[error("requested source root was not found")]
    Missing,
    /// 指定来源根存在，但当前处于停用状态。
    #[error("requested source root is not enabled")]
    NotEnabled,
}

/// 按稳定标识在来源根集合中定位一条引用；未命中返回 `Missing`。
pub fn resolve_source_root_by_id<'a, SourceRoot>(
    source_roots: &'a [SourceRoot],
    requested_root_id: &str,
    get_root_id: impl Fn(&SourceRoot) -> Option<&str>,
) -> Result<&'a SourceRoot, SourceRootLookupError> {
    source_roots
        .iter()
        .find(|source_root| get_root_id(source_root) == Some(requested_root_id))
        .ok_or(SourceRootLookupError::Missing)
}

/// 按稳定标识定位来源根并额外校验启用状态；未命中或停用返回对应错误。
pub fn resolve_enabled_source_root_by_id<'a, SourceRoot>(
    source_roots: &'a [SourceRoot],
    requested_root_id: &str,
    get_root_id: impl Fn(&SourceRoot) -> Option<&str>,
    is_enabled: impl Fn(&SourceRoot) -> bool,
) -> Result<&'a SourceRoot, SourceRootLookupError> {
    let source_root = resolve_source_root_by_id(source_roots, requested_root_id, get_root_id)?;
    if is_enabled(source_root) {
        Ok(source_root)
    } else {
        Err(SourceRootLookupError::NotEnabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    /// 表示来源查找测试中的最小数据根记录。
    struct TestRoot {
        id: &'static str,
        enabled: bool,
    }

    #[test]
    /// 验证按客户端和稳定标识查找精确数据根。
    fn finds_root_by_id() {
        let roots = [
            TestRoot {
                id: "root-disabled",
                enabled: false,
            },
            TestRoot {
                id: "root-enabled",
                enabled: true,
            },
        ];

        let found = resolve_source_root_by_id(&roots, "root-enabled", |root| Some(root.id))
            .expect("enabled root should be found");
        assert_eq!(found.id, "root-enabled");
        assert_eq!(
            resolve_source_root_by_id(&roots, "missing", |root| Some(root.id)),
            Err(SourceRootLookupError::Missing)
        );
    }

    #[test]
    /// 验证启用查找会排除已禁用的数据根。
    fn finds_enabled_root_only() {
        let roots = [
            TestRoot {
                id: "root-disabled",
                enabled: false,
            },
            TestRoot {
                id: "root-enabled",
                enabled: true,
            },
        ];

        assert_eq!(
            resolve_enabled_source_root_by_id(
                &roots,
                "root-enabled",
                |root| Some(root.id),
                |root| root.enabled,
            ),
            Ok(&roots[1])
        );
        assert_eq!(
            resolve_enabled_source_root_by_id(
                &roots,
                "root-disabled",
                |root| Some(root.id),
                |root| root.enabled,
            ),
            Err(SourceRootLookupError::NotEnabled)
        );
        assert_eq!(
            resolve_enabled_source_root_by_id(
                &roots,
                "missing",
                |root| Some(root.id),
                |root| root.enabled,
            ),
            Err(SourceRootLookupError::Missing)
        );
    }
}

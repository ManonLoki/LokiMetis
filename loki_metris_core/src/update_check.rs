//! 手动检查更新的纯逻辑：标签解析与版本比较，不做任何网络请求。

/// 仓库的公开 releases 页面；GUI 发现新版本时展示此地址。
pub const RELEASES_PAGE: &str = "https://github.com/ManonLoki/LokiMetis/releases/latest";

/// 解析三段版本及正式发布的八位日期后缀，容忍单个 v/V 前缀。
pub fn normalize_tag(tag: &str) -> Option<(u64, u64, u64)> {
    let tag = tag.trim();
    let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    let digits = if let Some((version, date)) = tag.split_once('-') {
        if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        version
    } else {
        tag
    };
    if !digits.bytes().all(|byte| byte.is_ascii_digit() || byte == b'.') {
        return None;
    }
    let mut parts = digits.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    // 恰好三段：第四段存在即视为非产品标签
    (parts.next().is_none()).then_some((major, minor, patch))
}

/// 比较结果：最新标签不高于当前版本视为已是最新。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateFinding {
    /// 已是最新版本。
    UpToDate,
    /// 发现新版本，携带最新标签原文。
    Available(String),
}

/// 与当前版本比较；最新标签无法解析时返回 None，调用方按检查失败处理。
pub fn compare(current: &str, latest_tag: &str) -> Option<UpdateFinding> {
    let current = normalize_tag(current)?;
    let latest = normalize_tag(latest_tag)?;
    Some(if latest > current {
        UpdateFinding::Available(latest_tag.trim().to_owned())
    } else {
        UpdateFinding::UpToDate
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 三段版本和项目正式日期标签可解析；预发布、多前缀和坏格式拒绝。
    #[test]
    fn normalize_tag_accepts_product_tags_only() {
        assert_eq!(normalize_tag("v0.13.0"), Some((0, 13, 0)));
        assert_eq!(normalize_tag("1.2.3"), Some((1, 2, 3)));
        assert_eq!(normalize_tag("  V10.0.99 "), Some((10, 0, 99)));
        assert_eq!(normalize_tag("v0.12.0-20261103"), Some((0, 12, 0)));
        assert_eq!(normalize_tag("v0.13.0-20261105"), Some((0, 13, 0)));
        assert_eq!(normalize_tag("v0.13.0-preview"), None);
        assert_eq!(normalize_tag("vv0.13.0"), None);
        assert_eq!(normalize_tag("1.2"), None);
        assert_eq!(normalize_tag("1.2.3.4"), None);
        assert_eq!(normalize_tag("x.y.z"), None);
        assert_eq!(normalize_tag(""), None);
    }

    /// 比较产出三态；最新标签畸形时返回 None 交由调用方报失败。
    #[test]
    fn compare_reports_newer_equal_and_malformed() {
        assert_eq!(
            compare("0.12.1", "v0.13.0"),
            Some(UpdateFinding::Available("v0.13.0".into()))
        );
        assert_eq!(compare("0.13.0", "0.13.0"), Some(UpdateFinding::UpToDate));
        assert_eq!(compare("1.0.0", "0.99.99"), Some(UpdateFinding::UpToDate));
        assert_eq!(compare("0.12.1", "november-release"), None);
    }
}

//! 通过跨平台 `machine-uid` 读取本机稳定设备 ID 候选；读不到时返回空，由 core 决定生成并持久化 UUID。
//!
//! 读取顺序由 `load_or_initialize_settings` 拥有：已持久化的设备 ID 优先，缺失时才调用本模块，
//! 本模块返回 None 时 core 生成随机 UUID 并写回设置文件，之后的查询只返回已缓存的值。

/// 在需要补发设备唯一 ID 时读取本机稳定标识；读取失败或规范化后为空时返回 None。
// `machine-uid` 在 Linux 读取 machine-id 文件，macOS 走 gethostuuid 系统调用，
// Windows 直接读注册表 MachineGuid，均不再派生子进程。错误对象可能携带 OS 错误文本，
// 因此这里只记录“不可用”这一事实，不输出错误内容。
pub(crate) fn read_stable_device_unique_id() -> Option<String> {
    let raw = machine_uid::get()
        .inspect_err(|_| {
            tracing::info!("stable machine id unavailable; device id will be generated");
        })
        .ok()?;
    normalize_stable_device_unique_id(&raw)
}

/// 去掉首尾空白与 GUID 形式可能带回的花括号；规范化后为空视为不可用。
fn normalize_stable_device_unique_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim().trim_matches(|c| c == '{' || c == '}').trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use loki_metis_core::DeviceUniqueId;

    use super::*;

    /// 验证规范化会去除空白与花括号，空值和只含花括号的值都视为不可用。
    #[test]
    fn normalizes_raw_machine_id_candidates() {
        assert_eq!(
            normalize_stable_device_unique_id("  {11111111-2222-4333-8444-555555555555}\n"),
            Some("11111111-2222-4333-8444-555555555555".to_owned())
        );
        assert_eq!(
            normalize_stable_device_unique_id("0123456789abcdef0123456789abcdef\n"),
            Some("0123456789abcdef0123456789abcdef".to_owned())
        );
        assert_eq!(normalize_stable_device_unique_id("   "), None);
        assert_eq!(normalize_stable_device_unique_id("{}"), None);
    }

    /// 验证在支持的桌面平台上读到的稳定 ID 必须能被 core 接受为设备唯一 ID，否则会静默退化为随机 UUID。
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    #[test]
    fn platform_stable_id_is_accepted_by_core_when_available() {
        if let Some(candidate) = read_stable_device_unique_id() {
            DeviceUniqueId::parse(&candidate)
                .expect("platform stable machine id must be a parsable UUID");
        }
    }
}

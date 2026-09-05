//! 定义设备用户名、设备名与设备唯一 ID 的 runtime-neutral 契约，不读取任何系统身份。

use thiserror::Error;
use uuid::Uuid;

/// 限制用户可读设备标签的字符数，避免未来 DTO 或设置文件承载无界输入。
pub const DEVICE_USERNAME_MAX_CHARS: usize = 64;

/// OS 主机名快照的最大字符数，覆盖 DNS 主机名上限并防止无界设置字段。
pub const DEVICE_NAME_MAX_CHARS: usize = 255;

/// 表示经过规范化且可安全持久化的可选设备用户名标签。
// “newtype”模式：把 String 包进一个只有私有字段的元组结构体（外部看不到
// `.0`），这样外部代码只能通过 from_setting_input 这个校验过的构造函数
// 拿到实例，就不可能绕过长度/控制字符校验直接塞一个非法字符串进来——
// 编译期就把“未校验”和“已校验”两种字符串区分成了不同类型。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceUsername(String);

impl DeviceUsername {
    /// 规范化设置页输入；空白表示清除，非空值必须满足长度与单行文本边界。
    pub fn from_setting_input(input: &str) -> Result<Option<Self>, DeviceUsernameError> {
        let normalized = input.trim();
        if normalized.is_empty() {
            return Ok(None);
        }
        if normalized.chars().count() > DEVICE_USERNAME_MAX_CHARS {
            return Err(DeviceUsernameError::TooLong);
        }
        if normalized.chars().any(char::is_control) {
            return Err(DeviceUsernameError::ContainsControlCharacter);
        }
        Ok(Some(Self(normalized.to_owned())))
    }

    /// 返回已经规范化的设备用户名文本，供 adapter 映射 DTO 或持久化。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 把已验证标签转为所有权字符串，避免 adapter 再次修改其内容。
    pub fn into_string(self) -> String {
        self.0
    }
}

/// 描述设备用户名被拒绝的稳定原因，不携带原始个人输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeviceUsernameError {
    /// 规范化后的非空标签超过批准的字符上限。
    #[error("device username is too long")]
    TooLong,
    /// 标签包含换行、空字符或其他控制字符，不能作为单行身份标签。
    #[error("device username contains a control character")]
    ContainsControlCharacter,
}

/// 将设备用户名校验失败映射为面向展示的中文文案，避免 GUI 重复维护该语义。
pub const fn device_username_error_message(error: DeviceUsernameError) -> &'static str {
    match error {
        DeviceUsernameError::TooLong => "设备用户名最多 64 个字符。",
        DeviceUsernameError::ContainsControlCharacter => "设备用户名不能包含换行或其他控制字符。",
    }
}

/// 表示经过规范化的只读设备名，通常来自 OS 主机名快照。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceName(String);

impl DeviceName {
    /// 规范化主机名候选；空白表示不可用，非空值必须满足长度与单行文本边界。
    pub fn from_hostname(input: &str) -> Result<Option<Self>, DeviceNameError> {
        let normalized = input.trim();
        if normalized.is_empty() {
            return Ok(None);
        }
        if normalized.chars().count() > DEVICE_NAME_MAX_CHARS {
            return Err(DeviceNameError::TooLong);
        }
        if normalized.chars().any(char::is_control) {
            return Err(DeviceNameError::ContainsControlCharacter);
        }
        Ok(Some(Self(normalized.to_owned())))
    }

    /// 返回已经规范化的设备名文本，供设置快照与 DTO 映射。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 把已验证设备名转为所有权字符串。
    pub fn into_string(self) -> String {
        self.0
    }
}

/// 描述设备名被拒绝的稳定原因，不携带原始主机值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeviceNameError {
    /// 规范化后的非空主机名超过批准的字符上限。
    #[error("device name is too long")]
    TooLong,
    /// 主机名包含换行或其他控制字符。
    #[error("device name contains a control character")]
    ContainsControlCharacter,
}

/// 将设备名校验失败映射为展示文本。
pub const fn device_name_error_message(error: DeviceNameError) -> &'static str {
    match error {
        DeviceNameError::TooLong => "设备名最多 255 个字符。",
        DeviceNameError::ContainsControlCharacter => "设备名不能包含换行或其他控制字符。",
    }
}

/// 表示首次确定并持久化的本机设备唯一 ID（UUID）。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceUniqueId(String);

/// 已有 ID 原样保留；否则采用可解析的稳定候选；再否则生成新 ID。
pub fn resolve_device_unique_id(
    existing: Option<DeviceUniqueId>,
    stable_candidate: Option<&str>,
) -> DeviceUniqueId {
    if let Some(existing) = existing {
        return existing;
    }
    stable_candidate
        .and_then(|candidate| DeviceUniqueId::parse(candidate).ok())
        .unwrap_or_else(DeviceUniqueId::generate)
}

impl DeviceUniqueId {
    /// 生成新的 UUID v4 文本，仅在没有已有 ID 且稳定候选不可用时使用。
    // UUID v4：完全随机生成的 128 位标识符，重复概率可忽略不计，
    // 用作本机安装的匿名唯一 ID，不依赖任何硬件序列号或用户信息。
    pub fn generate() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    /// 解析已持久化的 UUID；非法值拒绝，避免静默换发新身份。
    pub fn parse(input: &str) -> Result<Self, DeviceUniqueIdError> {
        let normalized = input.trim();
        if normalized.is_empty() {
            return Err(DeviceUniqueIdError::Invalid);
        }
        let parsed = Uuid::parse_str(normalized).map_err(|_| DeviceUniqueIdError::Invalid)?;
        Ok(Self(parsed.to_string()))
    }

    /// 返回规范化后的 UUID 文本。
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 把已验证唯一 ID 转为所有权字符串。
    pub fn into_string(self) -> String {
        self.0
    }
}

/// 描述设备唯一 ID 被拒绝的稳定原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DeviceUniqueIdError {
    /// 持久化字符串不是合法 UUID。
    #[error("device unique id is invalid")]
    Invalid,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 验证中英文标签会去除首尾空白，但保留中间用于辨认的普通空格。
    #[test]
    fn normalizes_user_supplied_device_username() {
        let username = DeviceUsername::from_setting_input("  示例用户的工作设备  ")
            .expect("valid label is accepted")
            .expect("non-empty label is retained");

        assert_eq!(username.as_str(), "示例用户的工作设备");
    }

    /// 验证留空代表用户主动清除标签，而不是保存一个不可辨认的空字符串。
    #[test]
    fn treats_blank_input_as_cleared_setting() {
        assert_eq!(DeviceUsername::from_setting_input(" \t "), Ok(None));
    }

    /// 验证批准的 64 字符边界可以保存，避免前后端对上限产生偏差。
    #[test]
    fn accepts_exact_character_limit() {
        let value = "用".repeat(DEVICE_USERNAME_MAX_CHARS);

        assert!(
            DeviceUsername::from_setting_input(&value)
                .expect("exact limit is accepted")
                .is_some()
        );
    }

    /// 验证超过字符上限时返回稳定错误且不截断用户身份标签。
    #[test]
    fn rejects_values_above_character_limit() {
        let value = "x".repeat(DEVICE_USERNAME_MAX_CHARS + 1);

        assert_eq!(
            DeviceUsername::from_setting_input(&value),
            Err(DeviceUsernameError::TooLong)
        );
    }

    /// 验证换行等控制字符不能进入设置文件或未来身份载荷。
    #[test]
    fn rejects_control_characters() {
        assert_eq!(
            DeviceUsername::from_setting_input("Manon\n第二行"),
            Err(DeviceUsernameError::ContainsControlCharacter)
        );
    }

    /// 验证设备名长度和控制字符错误映射到固定中文文案。
    #[test]
    fn reports_device_username_validation_messages() {
        assert_eq!(
            device_username_error_message(DeviceUsernameError::TooLong),
            "设备用户名最多 64 个字符。"
        );
        assert_eq!(
            device_username_error_message(DeviceUsernameError::ContainsControlCharacter),
            "设备用户名不能包含换行或其他控制字符。"
        );
    }

    /// 验证主机名快照会去除空白，并在空白时表示不可用。
    #[test]
    fn normalizes_os_hostname_as_device_name() {
        let name = DeviceName::from_hostname("  MacBook-Pro.local  ")
            .expect("hostname is accepted")
            .expect("non-empty hostname is retained");
        assert_eq!(name.as_str(), "MacBook-Pro.local");
        assert_eq!(DeviceName::from_hostname("  "), Ok(None));
    }

    /// 验证超长或含控制字符的主机名不能进入设置快照。
    #[test]
    fn rejects_invalid_device_names() {
        assert_eq!(
            DeviceName::from_hostname(&"a".repeat(DEVICE_NAME_MAX_CHARS + 1)),
            Err(DeviceNameError::TooLong)
        );
        assert_eq!(
            DeviceName::from_hostname("bad\nhost"),
            Err(DeviceNameError::ContainsControlCharacter)
        );
    }

    /// 验证新生成的唯一 ID 可解析，且非法字符串被拒绝。
    #[test]
    fn generates_and_parses_device_unique_id() {
        let generated = DeviceUniqueId::generate();
        let parsed = DeviceUniqueId::parse(generated.as_str()).expect("generated id parses");
        assert_eq!(parsed, generated);
        assert_eq!(
            DeviceUniqueId::parse("not-a-uuid"),
            Err(DeviceUniqueIdError::Invalid)
        );
        assert_eq!(
            DeviceUniqueId::parse("  "),
            Err(DeviceUniqueIdError::Invalid)
        );
    }

    /// 已有 ID 不得被不同的稳定候选或生成结果替换。
    #[test]
    fn keeps_existing_device_unique_id() {
        let existing =
            DeviceUniqueId::parse("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee").expect("fixture parses");
        let resolved = resolve_device_unique_id(
            Some(existing.clone()),
            Some("11111111-2222-4333-8444-555555555555"),
        );
        assert_eq!(resolved, existing);
    }

    /// 缺失时必须写入可解析的稳定候选，而不是另一次随机生成。
    #[test]
    fn prefers_parseable_stable_candidate_when_missing() {
        let resolved =
            resolve_device_unique_id(None, Some("  11111111-2222-4333-8444-555555555555  "));
        assert_eq!(resolved.as_str(), "11111111-2222-4333-8444-555555555555");
    }

    /// 稳定候选无法解析为唯一 ID 时才生成，生成结果仍须可再解析。
    #[test]
    fn generates_only_when_stable_candidate_is_unavailable() {
        let generated = resolve_device_unique_id(None, Some("not-a-uuid"));
        assert_ne!(generated.as_str(), "not-a-uuid");
        assert_eq!(
            DeviceUniqueId::parse(generated.as_str()).expect("generated id parses"),
            generated
        );
        let blank = resolve_device_unique_id(None, Some("  "));
        assert_eq!(
            DeviceUniqueId::parse(blank.as_str()).expect("blank candidate falls back to generate"),
            blank
        );
        let missing = resolve_device_unique_id(None, None);
        assert_eq!(
            DeviceUniqueId::parse(missing.as_str()).expect("missing candidate generates"),
            missing
        );
    }
}

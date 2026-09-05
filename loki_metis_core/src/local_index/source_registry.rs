//! 定义用户登记数据根与本机索引 registry 的非敏感展示记录。

use std::path::{Path, PathBuf};

use crate::{CoverageState, RegisteredRootIdentity, RootActivationState};

/// 标识数据根是由哪个受控入口发现或登记的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryMethod {
    /// 当前用户主目录下的默认 `.codex`。
    DefaultHome,
    /// 当前进程 `CODEX_HOME` 指向的数据根。
    Environment,
    /// 用户明确登记并可随时禁用的数据根。
    Registered,
    /// 用户主动发起全设备发现得到的候选根。
    FullDevice,
    /// 元数据发现经用户确认得到的候选根。
    MetadataDiscovery,
}

// as_str/from_label 这对函数是本模块反复出现的模式：数据库里存文本标签
// 而不是整数，是为了让数据库文件本身具备可读性（便于人工排查），同时
// `from_label` 对未知字符串返回 None 而不是 panic，让调用方能优雅处理
// “数据库被更新版本写过、出现了本版本不认识的标签”这种前向兼容场景。
// 命名为 `from_label` 而不是 `from_str`：避免和标准库 `FromStr::from_str`
// 混淆（clippy::should_implement_trait），也与本文件里
// `coverage_state_from_label`/`confidence_from_label` 保持一致命名。
impl DiscoveryMethod {
    /// 返回用于数据库存储的稳定短标签。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::DefaultHome => "default_home",
            Self::Environment => "environment",
            Self::Registered => "registered",
            Self::FullDevice => "full_device",
            Self::MetadataDiscovery => "metadata_discovery",
        }
    }

    /// 从当前 schema 的稳定标签恢复发现方式。
    pub fn from_label(value: &str) -> Option<Self> {
        match value {
            "default_home" => Some(Self::DefaultHome),
            "environment" => Some(Self::Environment),
            "registered" => Some(Self::Registered),
            "full_device" => Some(Self::FullDevice),
            "metadata_discovery" => Some(Self::MetadataDiscovery),
            _ => None,
        }
    }

    /// 重复物理目录只允许更高信任入口覆盖展示身份。
    pub const fn priority(self) -> u8 {
        match self {
            Self::Registered => 3,
            Self::Environment => 2,
            Self::DefaultHome => 1,
            Self::FullDevice | Self::MetadataDiscovery => 0,
        }
    }

    /// 缺省 `.codex`/`.claude` 目录本就是可选状态；只有非默认目录方式，
    /// 或该候选对应一个已登记根需要重新校验，才计入覆盖缺口。
    pub const fn counts_as_coverage_gap(self, has_existing_root_id: bool) -> bool {
        !matches!(self, Self::DefaultHome) || has_existing_root_id
    }
}

/// 表示用户明确维护的自定义数据根输入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredRoot {
    /// 已持久化根的精确稳定 ID；尚未登记的新候选为空。
    pub root_id: Option<String>,
    /// 只在 adapter 内用于只读访问和 registry 持久化的绝对或调用方解析路径。
    pub path: PathBuf,
    /// 默认向 GUI 展示的用户别名，不要求暴露完整路径。
    pub alias: String,
    /// 禁用的数据根保留登记信息但不参与快速扫描。
    pub enabled: bool,
}

impl RegisteredRootIdentity for RegisteredRoot {
    /// 返回待登记来源的已验证本机路径。
    fn path(&self) -> &Path {
        &self.path
    }

    /// 返回可复用的稳定数据根标识；首次发现时允许缺失。
    fn root_id(&self) -> Option<&str> {
        self.root_id.as_deref()
    }

    /// 返回待登记来源的安全展示别名。
    fn alias(&self) -> &str {
        &self.alias
    }
}

/// 表示 registry 中一个可展示的数据根，不返回其绝对访问路径。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootRecord {
    /// adapter 根据访问位置生成的稳定根 ID。
    pub root_id: String,
    /// 用户别名或安全默认名称。
    pub alias: String,
    /// 标识该根当前是否参与扫描。
    pub enabled: bool,
    /// 首次确认、索引中、可周期更新或验证失败的独立状态。
    pub activation_state: RootActivationState,
    /// 标识该根是否为 Codex 唯一主数据目录。
    pub is_primary: bool,
    /// 根的发现或登记方式。
    pub discovery_method: DiscoveryMethod,
    /// 最近一次扫描对该根的覆盖结论。
    pub last_coverage: Option<CoverageState>,
    /// 当前索引仍保留 checkpoint 的来源文件数量。
    pub source_file_count: u64,
    /// 当前 generation 中可追溯的调用观察数量，canonical 去重在 core 另行完成。
    pub call_observation_count: u64,
}

/// 把 core 覆盖状态转换为数据库稳定标签。
pub(crate) const fn coverage_state_label(state: CoverageState) -> &'static str {
    match state {
        CoverageState::Complete => "complete",
        CoverageState::Partial => "partial",
        CoverageState::Cancelled => "cancelled",
        CoverageState::Failed => "failed",
    }
}

/// 从当前 schema 的稳定标签恢复 core 覆盖状态。
pub(crate) fn coverage_state_from_label(value: &str) -> Option<CoverageState> {
    match value {
        "complete" => Some(CoverageState::Complete),
        "partial" => Some(CoverageState::Partial),
        "cancelled" => Some(CoverageState::Cancelled),
        "failed" => Some(CoverageState::Failed),
        _ => None,
    }
}

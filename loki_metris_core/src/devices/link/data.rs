//! 局域网展示协议只公开聚合与展示内容，不传递路径、原始Hook或凭据。

use crate::{AiTool, Completeness, Confidence, HookBehavior, TodayAvailability, TodayUsageSnapshot, UsageDay};
use serde::{Deserialize, Serialize};
use super::{LinkControl, LinkError, assets::{AssetChunk, AssetDescriptor}};

/// 终端发往桌面的有界请求；图片由终端逐块拉取，无待发无限队列。
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum DeviceMessage {
    /// 只能请求当前清单中的摘要与对齐偏移。
    AssetRequest { /// SHA-256小写十六进制摘要。
        hash: String, /// 从零开始的字节偏移。
        offset: u64 },
    /// 已有心跳及后台暂停控制。
    #[serde(untagged)]
    Control(LinkControl),
}

/// 桌面主动推送最新完整快照，图片正文仅响应明确请求。
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum HostMessage {
    /// 新连接及每次更新都携带完整投影，重连无需回放旧事件。
    Snapshot { /// 本连接当前展示事实。
        snapshot: DisplaySnapshot },
    /// 当前清单资源的一块正文。
    AssetChunk { /// 有界且经过摘要绑定的块。
        chunk: AssetChunk },
    /// 配置修改后旧摘要失效，终端等待下一快照，不删除已校验缓存。
    AssetUnavailable { /// 被请求的摘要。
        hash: String },
}

/// 质量与数值共同传输，未扫描、读取失败和精确零值不能混用。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenAmount {
    /// 十进制字符串保留完整u64，避免跨端JSON浮点损失。
    pub total: Option<String>,
    /// available/noRecords/unavailable。
    pub availability: String,
    /// 已启用数据源的完整度。
    pub completeness: Completeness,
    /// 数值的直接/推算/冲突质量。
    pub confidence: Confidence,
}
impl From<&TodayUsageSnapshot> for TokenAmount {
    /// 读取失败隐藏旧数字；部分可用值保留并标记质量。
    fn from(value: &TodayUsageSnapshot) -> Self {
        Self { total: (value.availability == TodayAvailability::Available).then(|| value.total_tokens.to_string()),
            availability: match value.availability { TodayAvailability::Available => "available", TodayAvailability::NoRecords => "noRecords", TodayAvailability::Unavailable => "unavailable" }.into(),
            completeness: value.completeness, confidence: value.confidence }
    }
}
/// 单个支持统计的Agent；Cursor等仅支持Hooks的工具不伪造零用量。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentTokens { /// 公开工具身份。
    pub agent: AiTool, /// 同轮观测的金额与质量。
    pub amount: TokenAmount }
/// 一轮一致的总量与分量，只表示桌面当前民用日。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TokenSnapshot {
    /// 日期、时区与实际日界。
    pub day: UsageDay,
    /// 所有数字共同的观测时间。
    pub observed_ms: i64,
    /// 联合口径总量。
    pub all: TokenAmount,
    /// 同一快照读取构造的各Agent分量。
    pub agents: Vec<AgentTokens>,
}
impl TokenSnapshot {
    /// 不同日期/时区/观测时间及重复Agent的分量禁止混入总量。
    pub fn from_observation(all: &TodayUsageSnapshot, agents: &[(AiTool, TodayUsageSnapshot)]) -> Result<Self, LinkError> {
        let mut seen = std::collections::HashSet::new();
        if agents.iter().any(|(agent, value)| !seen.insert(*agent) || value.day != all.day || value.observed_ms != all.observed_ms) { return Err(LinkError::Invalid); }
        Ok(Self { day: all.day.clone(), observed_ms: all.observed_ms, all: all.into(),
            agents: agents.iter().map(|(agent, value)| AgentTokens { agent: *agent, amount: value.into() }).collect() })
    }
}
/// 每个槽位只传最终展示结果，释放状态由完整快照中缺项表达。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HookDisplay {
    /// 公开Agent。
    pub agent: AiTool,
    /// 一到十二的展示位置。
    pub slot: u8,
    /// 当前四态行为。
    pub behavior: HookBehavior,
    /// 用户配置文案，最长512字节，截断时有显式标志。
    pub content: String,
    /// 文案是否因协议上限被截断。
    pub content_truncated: bool,
    /// 当前图片摘要，缺失/未选图为空。
    pub image: Option<String>,
}
/// 有界完整快照；读取失败必须清除对应投影并明确错误。
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DisplaySnapshot {
    /// 协议版本。
    pub version: u8,
    /// Token读取成功时存在。
    pub tokens: Option<TokenSnapshot>,
    /// Token读取失败的稳定类别。
    pub token_error: Option<String>,
    /// Hook监听尚未启动或读取失败时显式false。
    pub hooks_available: bool,
    /// 每槽位最新有效状态。
    pub hooks: Vec<HookDisplay>,
    /// 所有公开档案引用的可用图片，后续Hooks只引用摘要。
    pub assets: Vec<AssetDescriptor>,
    /// 配置/图库读取失败的稳定类别。
    pub catalog_error: Option<String>,
}

/// 复用本机十二槽位覆盖/释放规则，仅输出公开且启用Agent的最终展示。
pub fn project_hooks(drafts: &[crate::AiProfileDraft], states: &[crate::PetOverlayToolState],
    enabled: &[AiTool], hashes: &std::collections::BTreeMap<String, String>) -> Vec<HookDisplay> {
    let drafts: Vec<_> = drafts.iter().filter(|draft| enabled.contains(&draft.tool) && crate::is_public_monitor_tool(draft.tool)).cloned().collect();
    let view = crate::project_pet_overlay_from_drafts(&drafts, states);
    view.slots.into_iter().filter_map(|slot| {
        let tile = slot.tile?;
        if !enabled.contains(&tile.tool) || !crate::is_public_monitor_tool(tile.tool) { return None; }
        let behavior = states.iter().filter(|s| s.tool == tile.tool && s.slot_index == slot.slot_index).max_by_key(|s| s.revision)?.behavior?;
        let mut content = tile.content;
        // 每槽位512字节使最坏JSON转义与完整清单合计仍低于64KiB。
        let content_truncated = content.len() > 512;
        if content_truncated { let mut end = 512; while !content.is_char_boundary(end) { end -= 1; } content.truncate(end); }
        Some(HookDisplay { agent: tile.tool, slot: slot.slot_index + 1, behavior, content, content_truncated,
            image: tile.image_key.and_then(|id| hashes.get(&id).cloned()) })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    /// 控制包与图片请求保持严格字段边界，不能用多余字段混淆解析。
    #[test]
    fn device_messages_accept_control_and_reject_unknown_fields() {
        assert!(matches!(serde_json::from_str::<DeviceMessage>(r#"{"type":"pong","nonce":7}"#).unwrap(), DeviceMessage::Control(LinkControl::Pong { nonce:7 })));
        assert!(matches!(serde_json::from_str::<DeviceMessage>(r#"{"type":"assetRequest","hash":"a","offset":0}"#).unwrap(), DeviceMessage::AssetRequest { .. }));
        assert!(serde_json::from_str::<DeviceMessage>(r#"{"type":"pause","ip":"192.168.1.1"}"#).is_err());
        assert!(serde_json::from_str::<DeviceMessage>(r#"{"type":"assetRequest","hash":"a","offset":0,"path":"secret"}"#).is_err());
    }
    /// u64整数完整保留，跨轮分量与重复Agent不能混入总量。
    #[test]
    fn token_observation_preserves_u64_and_rejects_mixed_windows() {
        let value = TodayUsageSnapshot { day: UsageDay::capture(1_700_000_000_000, &jiff::tz::TimeZone::UTC).unwrap(), source:"isolated".into(),
            total_tokens:u64::MAX, observed_ms:1_700_000_000_000, availability:TodayAvailability::Available,
            completeness:Completeness::Complete, confidence:Confidence::Exact };
        let snapshot = TokenSnapshot::from_observation(&value, &[(AiTool::Codex,value.clone())]).unwrap();
        assert_eq!(snapshot.all.total.as_deref(), Some("18446744073709551615"));
        let mut stale = value.clone(); stale.observed_ms -= 1;
        assert!(TokenSnapshot::from_observation(&value, &[(AiTool::Codex,stale)]).is_err());
        assert!(TokenSnapshot::from_observation(&value, &[(AiTool::Codex,value.clone()),(AiTool::Codex,value.clone())]).is_err());
        let mut unavailable = value; unavailable.availability = TodayAvailability::Unavailable;
        assert_eq!(TokenAmount::from(&unavailable).total, None);
    }
    /// 槽位覆盖与释放沿用桌宠规则，长中文截断保持UTF-8，协议不暴露图库路径。
    #[test]
    fn hook_projection_resolves_slots_release_and_bounded_unicode() {
        let mut draft = crate::AiProfileDraft::default_for(AiTool::Codex);
        draft.hooks[1].content = "长文".repeat(500); draft.hooks[1].image = "local-image-id".into();
        let hashes = [("local-image-id".into(), "a".repeat(64))].into();
        let mut states = vec![crate::PetOverlayToolState { tool:AiTool::Codex, slot_index:0, behavior:Some(HookBehavior::Running), revision:1 }];
        let view = project_hooks(&[draft.clone()], &states, &[AiTool::Codex], &hashes);
        assert_eq!(view[0].slot, 1); assert!(view[0].content_truncated); assert!(view[0].content.len() <= 512);
        assert_eq!(view[0].image.as_ref().unwrap(), &"a".repeat(64));
        states[0].behavior = None;
        assert!(project_hooks(&[draft.clone()], &states, &[AiTool::Codex], &hashes).is_empty());
        states[0].behavior = Some(HookBehavior::Running);
        assert!(project_hooks(&[draft], &states, &[], &hashes).is_empty());
    }
}

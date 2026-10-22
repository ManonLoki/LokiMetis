//! 已认证终端共享一个后台快照发布器；无终端时停止周期统计与图库读取。

use super::Devices;
use std::{collections::BTreeMap, sync::{Arc, atomic::Ordering}, time::Duration};
use loki_metris_core::{ScanCancellation, devices::link::{data::{DisplaySnapshot, HostMessage, TokenSnapshot, project_hooks}, assets::AssetDescriptor}};
use tokio::sync::watch;

/// 最新投影与不可变正文共享；慢终端最多保留当前与正在读的一份Arc。
pub(super) struct Publication {
    /// 完成真实读取的单调时间；默认等待态不能完成HTTP刷新。
    pub(super) refreshed_at: Option<std::time::Instant>,
    pub(super) snapshot: DisplaySnapshot,
    pub(super) bytes: BTreeMap<String, Arc<[u8]>>,
}
impl Default for Publication {
    /// 尚未读取时明确等待刷新，不能发送虚构零统计。
    fn default() -> Self { Self { refreshed_at: None, snapshot: DisplaySnapshot { version: 1, token_error: Some("device_data_refreshing".into()), ..Default::default() }, bytes: BTreeMap::new() } }
}
impl Publication {
    /// 快照只发送当前日且仍新鲜的数字，长时间无终端后的首包不展示旧数值。
    pub(super) fn message(&self, now: i64) -> HostMessage {
        let mut snapshot = self.snapshot.clone();
        if snapshot.tokens.as_ref().is_some_and(|t| !t.day.contains(now) || !(0..=90_000).contains(&now.saturating_sub(t.observed_ms))) {
            snapshot.tokens = None; snapshot.token_error = Some("device_data_refreshing".into());
        }
        HostMessage::Snapshot { snapshot, capabilities: vec![loki_metris_core::devices::companion::WATCH_COMPANION_CAPABILITY.into(),
            loki_metris_core::devices::link::DEVICE_FORM_FACTOR_CAPABILITY.into(), "snapshotQueryV1".into()], widget_endpoint: None }
    }
    /// 不接受路径/URL，且只从当前完整清单查找内容；下载期间修改清单返回失效。
    pub(super) fn asset(&self, hash: &str, offset: u64) -> Result<HostMessage, &'static str> {
        if hash.len() != 64 || !hash.bytes().all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v)) { return Err("device_protocol_invalid"); }
        let Some(descriptor) = self.snapshot.assets.iter().find(|a| a.hash == hash) else { return Ok(HostMessage::AssetUnavailable { hash: hash.into() }); };
        let bytes = self.bytes.get(hash).ok_or("device_protocol_invalid")?;
        Ok(HostMessage::AssetChunk { chunk: descriptor.chunk(bytes, offset).map_err(|_| "device_protocol_invalid")? })
    }
}

/// 同一次目录事务及哈希结果只在配置/图库revision变化时重建，不随Hook变更重读图片。
#[derive(Default)]
struct Catalog {
    drafts: Vec<loki_metris_core::AiProfileDraft>,
    hashes: BTreeMap<String, String>,
    assets: Vec<AssetDescriptor>,
    bytes: BTreeMap<String, Arc<[u8]>>,
}
/// 阻塞岛读取与哈希有界图库，manifest仅包含公开档案真实引用，最多四图乘公开Agent数。
fn catalog(directory: &std::path::Path) -> Result<Catalog, &'static str> {
    let (profiles, gallery) = crate::monitor::images::link_catalog(directory).map_err(|_| "device_catalog_unavailable")?;
    let drafts: Vec<_> = profiles.drafts.into_iter().filter(|d| loki_metris_core::is_public_monitor_tool(d.tool)).collect();
    let mut result = Catalog { drafts, ..Default::default() };
    for hook in result.drafts.iter().flat_map(|d| &d.hooks) {
        if result.hashes.contains_key(&hook.image) { continue; }
        if let Some(bytes) = gallery.bytes.get(&hook.image) {
            let descriptor = AssetDescriptor::from_bytes(bytes).map_err(|_| "device_catalog_unavailable")?;
            result.hashes.insert(hook.image.clone(), descriptor.hash.clone());
            if result.bytes.insert(descriptor.hash.clone(), bytes.clone()).is_none() { result.assets.push(descriptor); }
        }
    }
    if result.assets.len() > 64 { return Err("device_catalog_unavailable"); }
    result.assets.sort_by(|a,b| a.hash.cmp(&b.hash)); Ok(result)
}

impl Devices {
    /// 阻塞读取登记应用owner并持跨代许可；外层future取消后owner仍会回收已开始的读取。
    async fn read_catalog(&self, directory: std::path::PathBuf) -> Result<Catalog, &'static str> {
        let permit = self.catalog_gate.clone().acquire_owned().await.map_err(|_| "device_catalog_unavailable")?;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.owner.spawn_blocking("device-catalog", move |shutdown| {
            let _permit = permit;
            if shutdown.is_cancelled() { return; }
            let _ = tx.send(catalog(&directory));
        })?;
        rx.await.map_err(|_| "device_catalog_unavailable")?
    }

    /// 本任务由数据服务JoinSet拥有，连接归零休眠；快速Hook轮询只读内存revision。
    pub(super) async fn publish_loop(self: Arc<Self>, mut cancelled: watch::Receiver<bool>) {
        let Some(weak) = self.context.get().cloned() else { return; };
        let mut catalog_value = Catalog::default(); let mut catalog_revision = None;
        let mut hook_key = None; let mut token_due = 0; let mut source_epoch = None;
        let mut value = DisplaySnapshot { version: 1, ..Default::default() };
        loop {
            if *cancelled.borrow() { return; }
            if { let state = self.state(); state.server.sessions.is_empty() && state.server.widget_readers == 0 } {
                catalog_value = Catalog::default(); value = DisplaySnapshot { version: 1, ..Default::default() };
                self.publications.send_replace(Arc::new(Publication::default()));
                catalog_revision = None; hook_key = None; token_due = 0;
                tokio::select! { biased; _ = cancelled.changed() => return, _ = self.publication_refresh.notified() => {} }
                continue;
            }
            let Some(context) = weak.upgrade() else { return; };
            let now = super::super::current_epoch_ms();
            let epoch = context.notification_source_epoch.load(Ordering::Acquire);
            let enabled = context.agents.snapshot().await.ok().map(|settings| settings.enabled_ai_tools);
            // 终端需求独立于桌面页面可见性，复用同一后台额度板及五分钟/进行中合并门禁。
            context.quota.kick(false, &context.background);
            let mut changed = value.update_agent_visibility(enabled, context.quota.snapshot().for_terminal());
            let enabled = value.enabled_agents.clone().unwrap_or_default();
            if now >= token_due || source_epoch != Some(epoch) || value.tokens.as_ref().is_some_and(|t| !t.day.contains(now)) {
                let cancellation = ScanCancellation::default();
                let loaded = tokio::select! {
                    biased; _ = cancelled.changed() => { cancellation.cancel(); return; },
                    loaded = super::super::today::load_with_components(context.clone(), now, jiff::tz::TimeZone::system(), cancellation.clone()) => loaded,
                };
                match loaded {
                    Ok((_, all, agents)) => match TokenSnapshot::from_observation(&all, &agents) {
                        Ok(tokens) => { value.tokens = Some(tokens); value.token_error = None; },
                        Err(_) => { value.tokens = None; value.token_error = Some("device_data_unavailable".into()); },
                    },
                    Err(error) => { value.tokens = None; value.token_error = Some(error.into()); },
                }
                source_epoch = Some(epoch); token_due = now.saturating_add(30_000); changed = true;
            }
            let revision = context.monitor_revision.load(Ordering::Acquire);
            if catalog_revision != Some(revision) {
                let directory = context.directory.clone();
                match self.read_catalog(directory).await {
                    Ok(catalog) => { catalog_value = catalog; value.catalog_error = None; },
                    _ => { catalog_value = Catalog::default(); value.catalog_error = Some("device_catalog_unavailable".into()); },
                }
                value.assets = catalog_value.assets.clone(); catalog_revision = Some(revision); hook_key = None; changed = true;
            }
            let status = context.agents.listener.get().map(|l| l.snapshot());
            let key = status.as_ref().map(|s| (s.listening, s.revision));
            let current_key = (key, enabled.clone());
            if hook_key.as_ref() != Some(&current_key) {
                value.hooks_available = status.as_ref().is_some_and(|s| s.listening);
                value.hooks = status.as_ref().filter(|s| s.listening).map(|s| project_hooks(&catalog_value.drafts, &s.pet_states, &enabled, &catalog_value.hashes)).unwrap_or_default();
                hook_key = Some(current_key); changed = true;
            }
            drop(context);
            if changed {
                self.publications.send_replace(Arc::new(Publication { refreshed_at: Some(std::time::Instant::now()), snapshot: value.clone(), bytes: catalog_value.bytes.clone() }));
            }
            tokio::select! { biased; _ = cancelled.changed() => return, _ = self.publication_refresh.notified() => { token_due = 0; },
                _ = tokio::time::sleep(Duration::from_millis(500)) => {} }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loki_metris_core::AiTool;

    /// 等待实际后台发布指定名单，跳过其它真实刷新，不依赖固定睡眠或改写发布状态。
    async fn next_enabled(updates: &mut watch::Receiver<Arc<Publication>>, enabled: &[AiTool]) -> Arc<Publication> {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                updates.changed().await.unwrap();
                let publication = updates.borrow_and_update().clone();
                if publication.snapshot.enabled_agents.as_deref() == Some(enabled) { return publication; }
            }
        }).await.expect("Agent设置应由现有500ms轮询发布")
    }

    /// Cursor没有Token来源且启停不改变统计epoch，真实发布器仍独立发送名单及明确全关闭。
    #[test]
    fn cursor_visibility_publishes_without_tokens_or_source_epoch_change() {
        let temp = tempfile::tempdir().unwrap();
        let service = crate::usage_service::UsageService::in_test_directory(temp.path().join("data")).unwrap();
        service.save_enabled_ai_tools(vec![AiTool::Cursor]).unwrap().blocking_recv().unwrap().unwrap();
        let devices = service.context.devices.clone();
        devices.state().server.widget_readers = 1;
        let runtime = service.runtime.lock().unwrap().as_ref().unwrap().handle().clone();
        runtime.block_on(async {
            let epoch = service.context.notification_source_epoch.load(Ordering::Acquire);
            let (cancel, cancelled) = watch::channel(false);
            let mut updates = devices.publications.subscribe();
            let publisher = tokio::spawn(devices.clone().publish_loop(cancelled));
            let initial = next_enabled(&mut updates, &[AiTool::Cursor]).await;
            assert!(initial.snapshot.tokens.is_none());
            assert_eq!(initial.snapshot.token_error.as_deref(), Some("usage_agents_disabled"));
            for enabled in [vec![], vec![AiTool::Cursor]] {
                service.save_enabled_ai_tools(enabled.clone()).unwrap().await.unwrap().unwrap();
                assert_eq!(service.context.notification_source_epoch.load(Ordering::Acquire), epoch);
                let publication = next_enabled(&mut updates, &enabled).await;
                assert!(publication.snapshot.tokens.is_none());
                assert_eq!(publication.snapshot.token_error.as_deref(), Some("usage_agents_disabled"));
                let HostMessage::Snapshot { snapshot, .. } = publication.message(super::super::super::current_epoch_ms()) else { panic!("snapshot"); };
                assert_eq!(snapshot.enabled_agents, Some(enabled));
            }
            cancel.send(true).unwrap();
            publisher.await.unwrap();
        });
        devices.state().server.widget_readers = 0;
        service.finish();
    }
}

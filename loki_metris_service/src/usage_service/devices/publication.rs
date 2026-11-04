//! 已认证终端共享一个后台快照发布器；无终端时停止周期统计与图库读取。

use super::Devices;
use std::{collections::BTreeMap, sync::{Arc, atomic::Ordering}, time::Duration};
use loki_metris_core::{PetLayout, ScanCancellation, devices::link::{data::{DisplaySnapshot, HostMessage, TokenSnapshot, project_hooks_page}, assets::AssetDescriptor}};
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
        // 移动端Hooks跟随桌宠当前页；未读到设置时按默认2×2首页投影，加载后键变化触发重投影。
        let mut pet_page_key: Option<(PetLayout, u8)> = None;
        let mut value = DisplaySnapshot { version: 1, ..Default::default() };
        loop {
            if *cancelled.borrow() { return; }
            if { let state = self.state(); state.server.sessions.is_empty() && state.server.widget_readers == 0 } {
                catalog_value = Catalog::default(); value = DisplaySnapshot { version: 1, ..Default::default() };
                self.publications.send_replace(Arc::new(Publication::default()));
                catalog_revision = None; hook_key = None; token_due = 0; pet_page_key = None;
                tokio::select! { biased; _ = cancelled.changed() => return, _ = self.publication_refresh.notified() => {} }
                continue;
            }
            let Some(context) = weak.upgrade() else { return; };
            let now = super::super::current_epoch_ms();
            let epoch = context.notification_source_epoch.load(Ordering::Acquire);
            let settings = context.agents.snapshot().await.ok();
            let enabled = settings.as_ref().map(|settings| settings.enabled_ai_tools.clone());
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
            // 桌宠当前页（布局+焦点位置）是移动端Hooks唯一发送范围；翻页与布局切换都必须重投影。
            let current_pet_page = settings.as_ref().map(|settings| {
                (settings.pet_window.layout, settings.pet_window.focused_slot)
            });
            if hook_key.as_ref() != Some(&current_key) || pet_page_key != current_pet_page {
                value.hooks_available = status.as_ref().is_some_and(|s| s.listening);
                let pet_window = settings.as_ref().map(|settings| settings.pet_window).unwrap_or_default();
                value.hooks = status.as_ref().filter(|s| s.listening).map(|s| project_hooks_page(&catalog_value.drafts, &s.pet_states, &enabled, &catalog_value.hashes, pet_window.layout, pet_window.focused_slot)).unwrap_or_default();
                hook_key = Some(current_key); pet_page_key = current_pet_page; changed = true;
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

    /// Cursor 不进入本机 Token 汇总且启停不改变统计 epoch；发布器仍独立发送启用名单及明确全关闭。
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

    /// 等待实际后台发布满足条件的快照，跳过无关刷新，不依赖固定睡眠或改写发布状态。
    async fn next_publication(updates: &mut watch::Receiver<Arc<Publication>>,
        predicate: impl Fn(&DisplaySnapshot) -> bool) -> Arc<Publication> {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                updates.changed().await.unwrap();
                let publication = updates.borrow_and_update().clone();
                if predicate(&publication.snapshot) { return publication; }
            }
        }).await.expect("桌宠当前页变化应由现有500ms轮询发布")
    }

    /// 测试真实中继边界；事件只含固定fixture标识，不携带本机Agent内容。
    fn post_running_event(endpoint_path: &std::path::Path) {
        let endpoint: serde_json::Value = serde_json::from_slice(&std::fs::read(endpoint_path).unwrap()).unwrap();
        let instance = endpoint["instanceId"].as_str().unwrap();
        let port = endpoint["port"].as_u64().unwrap() as u16;
        let body = "{\"hook_event_name\":\"UserPromptSubmit\",\"session_id\":\"fixture-session\",\"turn_id\":\"fixture-turn\"}";
        let request = format!("POST /api/hooks/{instance}/codex HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n{}: {instance}\r\n{}: UserPromptSubmit\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            loki_metris_core::HOOK_RELAY_INSTANCE_HEADER, loki_metris_core::HOOK_EVENT_TYPE_HEADER, body.len());
        let mut stream = std::net::TcpStream::connect_timeout(&([127,0,0,1],port).into(), Duration::from_secs(2)).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        std::io::Write::write_all(&mut stream, request.as_bytes()).unwrap();
        let mut response = String::new();
        std::io::Read::read_to_string(&mut stream, &mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 202"), "{response}");
    }

    /// 发布器只发送桌宠当前页：真实事件占据1号位后，翻页撤下旧页发送空页，
    /// 翻回后恢复同页投影，槽位保持绝对编号；翻页无需额外配置即纳入快照变化。
    #[test]
    fn publisher_follows_pet_current_page_for_terminal_hooks() {
        let temp = tempfile::tempdir().unwrap();
        let service = crate::usage_service::UsageService::in_test_directory(temp.path().join("data")).unwrap();
        service.monitor_settings().unwrap().blocking_recv().unwrap().unwrap();
        // 档案把Codex配在1号位（默认2×2首页），文案挂在Running行为上。
        let mut draft = loki_metris_core::AiProfileDraft::default_for(AiTool::Codex);
        draft.slot = 1; draft.hooks[1].content = "首页运行".into();
        service.save_monitor_profile_draft(draft).unwrap().blocking_recv().unwrap().unwrap();
        service.save_enabled_ai_tools(vec![AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
        let endpoint = temp.path().join("cache/loki-metris-hook-relay.json");
        service.start_test_monitoring_at(endpoint.clone()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !service.monitor_status().unwrap().listening {
            assert!(std::time::Instant::now() < deadline, "relay not listening");
            std::thread::sleep(Duration::from_millis(5));
        }
        post_running_event(&endpoint);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !service.monitor_status().unwrap().pet_states.iter().any(|state| state.behavior == Some(loki_metris_core::HookBehavior::Running)) {
            assert!(std::time::Instant::now() < deadline, "running state: {:?}", service.monitor_status().unwrap());
            std::thread::sleep(Duration::from_millis(5));
        }
        let devices = service.context.devices.clone();
        devices.state().server.widget_readers = 1;
        let runtime = service.runtime.lock().unwrap().as_ref().unwrap().handle().clone();
        runtime.block_on(async {
            let (cancel, cancelled) = watch::channel(false);
            let mut updates = devices.publications.subscribe();
            let publisher = tokio::spawn(devices.clone().publish_loop(cancelled));
            let hooks_on_page = |snapshot: &DisplaySnapshot| {
                snapshot.hooks.len() == 1 && snapshot.hooks[0].slot == 1
                    && snapshot.hooks[0].agent == AiTool::Codex
                    && snapshot.hooks[0].content == "首页运行"
            };
            let shown = next_publication(&mut updates, hooks_on_page).await;
            assert_eq!(shown.snapshot.hooks[0].behavior, loki_metris_core::HookBehavior::Running);
            // 翻到第二页：新快照撤下旧页，1号位不再发送，第二页空槽发送空集合。
            let range = loki_metris_core::PetSizeRange { min: 32, max: 2880 };
            service.change_pet_preferences(loki_metris_core::PetPreferenceChange::TurnPage(1), range).unwrap()
                .await.unwrap().unwrap();
            let cleared = next_publication(&mut updates, |snapshot| snapshot.hooks.is_empty()).await;
            assert!(cleared.snapshot.hooks_available, "翻页不改变监听可用性");
            // 翻回首页：同一当前页投影恢复，槽位仍是绝对1号位。
            service.change_pet_preferences(loki_metris_core::PetPreferenceChange::TurnPage(-1), range).unwrap()
                .await.unwrap().unwrap();
            let restored = next_publication(&mut updates, hooks_on_page).await;
            assert_eq!(restored.snapshot.hooks[0].slot, 1);
            cancel.send(true).unwrap();
            publisher.await.unwrap();
        });
        devices.state().server.widget_readers = 0;
        service.finish();
    }
}

//! 小组件短时HTTP读取：固定路径、严格长度、专用AEAD权限与当前账本复核。

use super::Devices;
use std::{sync::Arc, time::Instant};
use tokio::{net::TcpStream, io::{AsyncReadExt, AsyncWriteExt}};
use serde::Deserialize;

/// 请求只携带身份、随机nonce和固定读取操作密文。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Request { client_id: String, device_id: String, nonce: String, ciphertext: String }

/// future被取消或超时也立即释放发布器需求，不遗留永久后台轮询。
struct Demand(Arc<Devices>);
impl Drop for Demand {
    /// 计数仅由已经登记的请求持有，析构时唤醒重新检查休眠条件。
    fn drop(&mut self) {
        let mut state = self.0.state(); state.server.widget_readers = state.server.widget_readers.saturating_sub(1);
        drop(state); self.0.publication_refresh.notify_one();
    }
}

impl Devices {
    /// 只有通过专用读取权限认证的请求才启动一次实际统计读取。
    pub(super) async fn serve_widget_http(self: &Arc<Self>, stream: &mut TcpStream) {
        let result = self.widget_http_response(stream).await;
        let (status, body) = match result { Ok(bytes) => ("200 OK", bytes), Err(()) => ("403 Forbidden", Vec::new()) };
        let header = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nCache-Control: no-store\r\nConnection: close\r\nContent-Length: {}\r\n\r\n", body.len());
        let _ = stream.write_all(header.as_bytes()).await;
        let _ = stream.write_all(&body).await;
        let _ = stream.shutdown().await;
    }

    /// 头部与正文独立有界；拒绝chunked、重复长度和非约定方法/路径。
    async fn widget_http_response(self: &Arc<Self>, stream: &mut TcpStream) -> Result<Vec<u8>, ()> {
        let mut header = Vec::new(); let mut byte = [0];
        while !header.ends_with(b"\r\n\r\n") {
            if header.len() >= 4096 { return Err(()); }
            stream.read_exact(&mut byte).await.map_err(|_| ())?; header.push(byte[0]);
        }
        let length = request_length(&header)?;
        let mut body = vec![0; length]; stream.read_exact(&mut body).await.map_err(|_| ())?;
        let request: Request = serde_json::from_slice(&body).map_err(|_| ())?;
        let auth = {
            let mut state = self.state(); let book = state.book.as_ref().ok_or(())?;
            let auth = book.widget_request(&request.client_id, &request.device_id, &request.nonce, &request.ciphertext).map_err(|_| ())?;
            let saved: Vec<_> = book.summaries().into_iter().map(|device| device.id).collect();
            state.server.widget_nonces.retain(|id, _| saved.contains(id));
            let nonces = state.server.widget_nonces.entry(request.device_id.clone()).or_default();
            if nonces.contains(&request.nonce) { return Err(()); }
            if nonces.len() >= 32 { nonces.pop_front(); }
            nonces.push_back(request.nonce.clone());
            auth
        };
        let started = Instant::now();
        let mut updates = self.publications.subscribe();
        updates.borrow_and_update();
        { self.state().server.widget_readers += 1; }
        let _demand = Demand(self.clone());
        self.publication_refresh.notify_one();
        let publication = loop {
            updates.changed().await.map_err(|_| ())?;
            let value = updates.borrow_and_update().clone();
            if value.refreshed_at.is_some_and(|time| time >= started) { break value; }
        };
        // 读取中解除配对立即撤销权限，迟到读取不能返回旧身份的数据。
        {
            let state = self.state(); let book = state.book.as_ref().ok_or(())?;
            book.widget_request(&request.client_id, &request.device_id, &request.nonce, &request.ciphertext).map_err(|_| ())?;
        }
        let plaintext = widget_summary(&request.client_id, &publication.snapshot)?;
        let random = uuid::Uuid::new_v4(); let nonce: [u8; 12] = random.as_bytes()[..12].try_into().map_err(|_| ())?;
        let (nonce, ciphertext) = auth.seal(&nonce, &plaintext).map_err(|_| ())?;
        serde_json::to_vec(&serde_json::json!({"nonce":nonce,"ciphertext":ciphertext})).map_err(|_| ())
    }
}

/// HTTP摘要保留独立启用名单、额度与聚合值，主动裁掉Hook正文、图片和宿主资源元信息。
fn widget_summary(source: &str, snapshot: &loki_metris_core::devices::link::data::DisplaySnapshot) -> Result<Vec<u8>, ()> {
    let hooks: Vec<_> = snapshot.hooks.iter().map(|hook| serde_json::json!({"agent":hook.agent,"slot":hook.slot,"behavior":hook.behavior})).collect();
    serde_json::to_vec(&serde_json::json!({"type":"snapshot","sourceId":source,
        "snapshot":{"version":1,"enabledAgents":snapshot.enabled_agents,"tokens":snapshot.tokens,"tokenError":snapshot.token_error,"hooksAvailable":snapshot.hooks_available,"hooks":hooks,"providerQuota":snapshot.provider_quota}})).map_err(|_| ())
}

/// 长度解析为纯适配器语法，不接收任意资源或代理请求。
fn request_length(header: &[u8]) -> Result<usize, ()> {
    let text = std::str::from_utf8(header).map_err(|_| ())?;
    let mut lines = text.split("\r\n");
    if lines.next() != Some("POST /api/widget/v1/snapshot HTTP/1.1") { return Err(()); }
    let mut length = None; let mut json = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(())?;
        match name.to_ascii_lowercase().as_str() {
            "content-length" => {
                if length.is_some() || !value.trim().bytes().all(|b| b.is_ascii_digit()) { return Err(()); }
                length = Some(value.trim().parse::<usize>().map_err(|_| ())?);
            },
            "transfer-encoding" | "expect" => return Err(()),
            "content-type" => { json = value.trim().eq_ignore_ascii_case("application/json"); },
            _ => {},
        }
    }
    let length = length.ok_or(())?;
    if !json || !(1..=1024).contains(&length) { return Err(()); }
    Ok(length)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{CLIENT, DEVICE, link_book};
    /// HTTP手动裁剪不能丢额度，也不能把当前Hook正文夹带给独立读取者。
    #[test]
    fn widget_summary_preserves_quota_and_discards_private_hook_fields() {
        let quota = loki_metris_core::parse_provider_quota(loki_metris_core::QuotaProvider::Codex,
            r#"{"rate_limits":{"primary":{"used_percent":97,"window_minutes":10080}}}"#).unwrap();
        let mut snapshot = loki_metris_core::devices::link::data::DisplaySnapshot {
            version: 1, enabled_agents: Some(vec![loki_metris_core::AiTool::Codex, loki_metris_core::AiTool::Cursor]),
            token_error: Some("usage_sources_not_configured".into()),
            provider_quota: loki_metris_core::ProviderQuotaSnapshot::project(Some(123), &[quota]), ..Default::default()
        };
        snapshot.hooks.push(loki_metris_core::devices::link::data::HookDisplay {
            agent: loki_metris_core::AiTool::Codex, slot: 1, behavior: loki_metris_core::HookBehavior::Running,
            content: "private-hook-body".into(), content_truncated: false, image: Some("a".repeat(64)),
        });
        let body = widget_summary(CLIENT, &snapshot).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["snapshot"]["providerQuota"]["providers"][0]["weekly"]["usedTenths"], 970);
        assert_eq!(value["snapshot"]["enabledAgents"], serde_json::json!(["codex", "cursor"]));
        assert!(value["snapshot"]["tokens"].is_null());
        assert_eq!(value["snapshot"]["tokenError"], "usage_sources_not_configured");
        assert!(value["snapshot"]["hooks"][0].get("content").is_none());
        assert!(value["snapshot"].get("assets").is_none());
        assert!(!String::from_utf8_lossy(&body).contains("private-hook-body"));
        for enabled in [None, Some(vec![])] {
            snapshot.enabled_agents = enabled.clone();
            let body = widget_summary(CLIENT, &snapshot).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(value["snapshot"]["enabledAgents"], serde_json::to_value(enabled).unwrap());
        }
    }
    /// 固定读取入口拒绝代理路径、无界正文和含糊的HTTP framing。
    #[test]
    fn widget_http_rejects_ambiguous_or_unbounded_requests() {
        let valid = "POST /api/widget/v1/snapshot HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n";
        assert_eq!(request_length(valid.as_bytes()), Ok(100));
        for bad in [valid.replace("POST", "GET"), valid.replace("100", "1025"), valid.replace("100", "-1"),
            valid.replace("Content-Length:", "Transfer-Encoding:"), valid.replace("\r\n\r\n", "\r\nContent-Length: 1\r\n\r\n"), valid.replace("/api/widget/v1/snapshot", "http://remote/")] {
            assert!(request_length(bad.as_bytes()).is_err());
        }
    }

    /// 无TCP会话时HTTP仍唤醒新读取；共享环回socket路径验证协议分流和需求回收。
    #[test]
    fn idle_widget_http_requests_fresh_summary_without_claiming_tcp_presence() {
        let temp = tempfile::tempdir().unwrap();
        let service = crate::usage_service::UsageService::in_test_directory(temp.path().join("data")).unwrap();
        service.save_enabled_ai_tools(vec![loki_metris_core::AiTool::Codex]).unwrap().blocking_recv().unwrap().unwrap();
        let devices = service.context.devices.clone();
        devices.finish(Ok(link_book()), None);
        {
            let runtime = service.runtime.lock().unwrap();
            runtime.as_ref().unwrap().block_on(async {
                let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                let address = listener.local_addr().unwrap();
                let permits = Arc::new(tokio::sync::Semaphore::new(1));
                let (cancel, cancelled) = tokio::sync::watch::channel(false);
                let mut updates = devices.publications.subscribe();
                let publisher = tokio::spawn(devices.clone().publish_loop(cancelled.clone()));
                tokio::time::timeout(std::time::Duration::from_secs(2), updates.changed()).await.unwrap().unwrap();
                assert!(updates.borrow_and_update().refreshed_at.is_none());
                let server = devices.clone();
                let peer = async move {
                    for _ in 0..2 {
                        let (stream, _) = listener.accept().await.unwrap();
                        server.clone().serve_peer(stream, "192.168.1.20:51000".parse().unwrap(), permits.clone().acquire_owned().await.unwrap(), cancelled.clone()).await;
                    }
                };
                let client = async {
                    let body = serde_json::to_vec(&serde_json::json!({"clientId":CLIENT,"deviceId":DEVICE,
                        "nonce":"AwMDAwMDAwMDAwMD","ciphertext":"Xb0+fryh6yKmj9ynUJN1ibves2wo6z40FJxtUvoAbZtNdXluBiI="})).unwrap();
                    for status in ["200 OK", "403 Forbidden"] {
                        let mut stream = TcpStream::connect(address).await.unwrap();
                        stream.write_all(format!("POST /api/widget/v1/snapshot HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes()).await.unwrap();
                        stream.write_all(&body).await.unwrap();
                        let mut response = Vec::new(); stream.read_to_end(&mut response).await.unwrap();
                        assert!(response.starts_with(format!("HTTP/1.1 {status}\r\n").as_bytes()));
                        if status == "200 OK" {
                            let offset = response.windows(4).position(|bytes| bytes == b"\r\n\r\n").unwrap() + 4;
                            let envelope: serde_json::Value = serde_json::from_slice(&response[offset..]).unwrap();
                            assert!(envelope["ciphertext"].as_str().unwrap().len() > 32);
                            assert!(!String::from_utf8_lossy(&response).contains("hooksAvailable"));
                        }
                    }
                };
                tokio::time::timeout(std::time::Duration::from_secs(4), async { tokio::join!(peer, client); }).await.unwrap();
                assert_eq!(devices.state().server.widget_readers, 0);
                assert!(devices.state().server.sessions.is_empty());
                // 实际发布器完成了一次临时目录读取，不把初始等待态冒充新摘要。
                assert!(updates.has_changed().unwrap());
                let refreshed = updates.borrow_and_update().clone();
                if refreshed.refreshed_at.is_some() {
                    assert_ne!(refreshed.snapshot.token_error.as_deref(), Some("device_data_refreshing"));
                }
                cancel.send(true).unwrap(); publisher.await.unwrap();
            });
        }
        service.finish();
    }
}

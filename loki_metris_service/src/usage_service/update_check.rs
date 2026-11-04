//! 手动检查更新的出站读取：只回应用户点击，固定地址、有界传输、无凭据。

use super::UsageService;
use loki_metris_core::{QuotaFailure, update_check};
use tokio::sync::{oneshot, watch};

/// GitHub API 的固定查询地址；只读、无查询参数、无凭据。
const RELEASES_API: &str = "https://api.github.com/repos/ManonLoki/LokiMetis/releases/latest";

/// 响应正文上限；releases/latest 的 JSON 远小于此值，超限视为异常。
const MAX_BODY_BYTES: usize = 64 * 1024;

impl UsageService {
    /// 一次手动检查：返回最新发布标签原文；只响应用户点击，无后台轮询。
    pub fn check_latest_release(
        &self,
    ) -> Result<oneshot::Receiver<Result<String, &'static str>>, &'static str> {
        let (sender, receiver) = oneshot::channel();
        // 取消通道与退出联动：应用关闭时在途读取自行结束
        let (_shutdown, mut cancel) = watch::channel(false);
        // 有界读取：8 秒预算与 64 KiB 上限；无凭据、无正文、不接受重定向
        self.background
            .spawn("update-check", move |shutdown| async move {
                // 关闭即放弃结果
                if shutdown.is_cancelled() {
                    return;
                }
                let result = super::provider_quota::transport::run_command(
                    std::path::Path::new("/usr/bin/curl"),
                    &[
                        "-fsSL".to_owned(),
                        "--max-time".to_owned(),
                        "8".to_owned(),
                        "--proto".to_owned(),
                        "=https".to_owned(),
                        "-H".to_owned(),
                        "User-Agent: LokiMetris".to_owned(),
                        "-H".to_owned(),
                        "Accept: application/vnd.github+json".to_owned(),
                        RELEASES_API.to_owned(),
                    ],
                    None,
                    super::provider_quota::transport::CurlLimits {
                        timeout: std::time::Duration::from_secs(8),
                        max_body: MAX_BODY_BYTES,
                    },
                    &mut cancel,
                )
                .await;
                // 交给等待方；退出后发送失败可以忽略
                let body = result;
                let _ = sender.send(decode_latest(&body));
            })?;
        Ok(receiver)
    }
}

/// 解析 GitHub releases/latest 响应：JSON 中的 tag_name 必须是产品三段标签。
fn decode_latest(result: &Result<Vec<u8>, QuotaFailure>) -> Result<String, &'static str> {
    let Ok(body) = result else {
        return Err("update_check_unavailable");
    };
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "update_check_malformed")?;
    let tag = value["tag_name"].as_str().ok_or("update_check_malformed")?;
    if update_check::normalize_tag(tag).is_none() {
        return Err("update_check_malformed");
    }
    Ok(tag.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 固定 JSON 解出 tag_name；缺字段、坏标签与非 JSON 失败关闭。
    #[test]
    fn release_response_decodes_only_product_tags() {
        let ok = Ok(b"{\"tag_name\":\"v0.13.0\",\"name\":\"x\"}".to_vec());
        assert_eq!(decode_latest(&ok).unwrap(), "v0.13.0");
        let missing = Ok(b"{\"name\":\"x\"}".to_vec());
        assert_eq!(decode_latest(&missing), Err("update_check_malformed"));
        let bad = Ok(b"{\"tag_name\":\"autumn\"}".to_vec());
        assert_eq!(decode_latest(&bad), Err("update_check_malformed"));
        let broken = Err(QuotaFailure::Transport);
        assert_eq!(decode_latest(&broken), Err("update_check_unavailable"));
    }
}

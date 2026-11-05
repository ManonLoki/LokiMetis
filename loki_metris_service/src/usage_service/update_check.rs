//! 手动检查更新的出站读取：固定地址、无凭据，并由应用后台 owner 取消和回收。

use std::{path::PathBuf, time::Duration};
use tokio::sync::oneshot;
use loki_metris_core::{QuotaFailure, update_check};

use super::{UsageService, background::BackgroundTaskOwner, provider_quota::transport};

/// GitHub API 的唯一查询地址；不接受调用方指定地址或重定向。
const RELEASES_API: &str = "https://api.github.com/repos/ManonLoki/LokiMetis/releases/latest";
/// 响应正文上限，超过即终止本次子进程。
const MAX_BODY_BYTES: usize = 64 * 1024;

impl UsageService {
    /// 用户手动检查一次；只读现有 PATH 中的 curl，不轮询、下载或安装。
    pub fn check_latest_release(
        &self,
    ) -> Result<oneshot::Receiver<Result<String, &'static str>>, &'static str> {
        start_check(
            &self.background,
            transport::find_program(if cfg!(windows) { "curl.exe" } else { "curl" }),
            transport::CurlLimits { timeout: Duration::from_secs(8), max_body: MAX_BODY_BYTES },
        )
    }
}

/// 让 owner 的真实退出通道贯穿传输；注入程序和预算供隔离进程回归复用。
fn start_check(
    background: &BackgroundTaskOwner,
    program: Option<PathBuf>,
    limits: transport::CurlLimits,
) -> Result<oneshot::Receiver<Result<String, &'static str>>, &'static str> {
    let (sender, receiver) = oneshot::channel();
    background.spawn("update-check", move |shutdown| async move {
        let mut cancel = shutdown.receiver();
        if shutdown.is_cancelled() {
            return;
        }
        let body = if let Some(program) = program {
            transport::run_command(&program, &request_args(), None, limits, &mut cancel).await
        } else {
            Err(QuotaFailure::Transport)
        };
        let _ = sender.send(decode_latest(&body));
    })?;
    Ok(receiver)
}

/// 禁用 curl 默认配置，不跟随跳转，并附带状态码以拒绝所有非 200 响应。
fn request_args() -> Vec<String> {
    [
        "--disable", "--silent", "--show-error", "--fail", "--max-time", "8",
        "--proto", "=https", "--proto-redir", "=https", "--max-redirs", "0",
        "--header", "User-Agent: LokiMetris", "--header", "Accept: application/vnd.github+json",
        "--write-out", "\n__LM_HTTP_%{http_code}", "--url", RELEASES_API,
    ].into_iter().map(str::to_owned).collect()
}

/// 只有成功的 HTTP 响应和可解析的正式版本标签才能交给 core 比较。
fn decode_latest(result: &Result<Vec<u8>, QuotaFailure>) -> Result<String, &'static str> {
    let bytes = result.as_ref().map_err(|_| "update_check_unavailable")?;
    let response = transport::split_http(bytes).map_err(|_| "update_check_malformed")?;
    if response.status != 200 {
        return Err("update_check_unavailable");
    }
    let value: serde_json::Value = serde_json::from_str(&response.body)
        .map_err(|_| "update_check_malformed")?;
    let tag = value["tag_name"].as_str().ok_or("update_check_malformed")?;
    update_check::normalize_tag(tag).ok_or("update_check_malformed")?;
    Ok(tag.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 正式日期标签可以读取；跳转、坏标签和传输失败不能成为可用更新。
    #[test]
    fn release_response_decodes_only_successful_product_tags() {
        let ok = Ok(b"{\"tag_name\":\"v0.13.0-20261105\"}\n__LM_HTTP_200".to_vec());
        assert_eq!(decode_latest(&ok), Ok("v0.13.0-20261105".to_owned()));
        let redirect = Ok(b"{\"tag_name\":\"v0.13.0\"}\n__LM_HTTP_302".to_vec());
        assert_eq!(decode_latest(&redirect), Err("update_check_unavailable"));
        for bytes in [b"{}\n__LM_HTTP_200".as_slice(), b"{\"tag_name\":\"autumn\"}\n__LM_HTTP_200", b"{}"] {
            assert_eq!(decode_latest(&Ok(bytes.to_vec())), Err("update_check_malformed"));
        }
        assert_eq!(decode_latest(&Err(QuotaFailure::Transport)), Err("update_check_unavailable"));
    }

    /// 真实夹具进程经过生产 owner、传输和解析，覆盖过去会立即取消的调用链。
    #[cfg(unix)]
    #[tokio::test]
    async fn owned_update_check_runs_to_completion_and_rejects_failures() {
        let root = tempfile::tempdir().expect("fixture directory");
        let program = fixture(root.path());
        let owner = BackgroundTaskOwner::new(std::sync::Arc::default());
        for (mode, expected) in [
            ("ok", Ok("v0.13.0-20261105".to_owned())),
            ("redirect", Err("update_check_unavailable")),
            ("exit", Err("update_check_unavailable")),
            ("huge", Err("update_check_unavailable")),
            ("sleep", Err("update_check_unavailable")),
        ] {
            std::fs::write(root.path().join("mode"), mode).expect("fixture mode");
            let receipt = start_check(&owner, Some(program.clone()), transport::CurlLimits {
                timeout: if mode == "sleep" { Duration::from_millis(300) } else { Duration::from_secs(8) },
                max_body: MAX_BODY_BYTES,
            }).expect("registered check");
            assert_eq!(receipt.await.expect("receipt"), expected, "{mode}");
        }
        let argv = std::fs::read_to_string(root.path().join("argv")).expect("fixture argv");
        assert!(argv.starts_with("--disable\n"));
        assert!(!argv.lines().any(|arg| arg == "--location" || arg == "-L"));
        assert!(argv.contains(RELEASES_API));
        let missing = start_check(&owner, None, transport::CurlLimits::production()).expect("registered check");
        assert_eq!(missing.await.expect("receipt"), Err("update_check_unavailable"));
        owner.shutdown().await;
    }

    /// 应用退出通过 owner 取消真实在途子进程，回收完成后不再登记新请求。
    #[cfg(unix)]
    #[tokio::test]
    async fn update_check_shutdown_cancels_and_reaps_the_process() {
        let root = tempfile::tempdir().expect("fixture directory");
        let program = fixture(root.path());
        std::fs::write(root.path().join("mode"), "sleep").expect("fixture mode");
        let owner = BackgroundTaskOwner::new(std::sync::Arc::default());
        let receipt = start_check(&owner, Some(program), transport::CurlLimits::production()).expect("registered check");
        tokio::time::timeout(Duration::from_secs(3), async {
            while !root.path().join("argv").exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("child started");
        owner.shutdown().await;
        assert_eq!(receipt.await.expect("cancel receipt"), Err("update_check_unavailable"));
        assert_eq!(owner.owned_task_count(), 0);
        assert!(start_check(&owner, None, transport::CurlLimits::production()).is_err());
    }

    /// 标准库 Python 夹具只输出固定响应并记录 argv，完全不访问网络或用户数据。
    #[cfg(unix)]
    fn fixture(root: &std::path::Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let program = root.join("curl-fixture.py");
        std::fs::write(&program, r#"#!/usr/bin/env python3
import pathlib, sys, time
root = pathlib.Path(__file__).parent
(root / 'argv').write_text('\n'.join(sys.argv[1:]) + '\n')
mode = (root / 'mode').read_text()
if mode == 'sleep': time.sleep(10)
if mode == 'exit': sys.exit(1)
if mode == 'huge': print('x' * 70000)
else: print('{"tag_name":"v0.13.0-20261105"}\n__LM_HTTP_' + ('302' if mode == 'redirect' else '200'), end='')
"#).expect("fixture script");
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700)).expect("executable fixture");
        program
    }
}

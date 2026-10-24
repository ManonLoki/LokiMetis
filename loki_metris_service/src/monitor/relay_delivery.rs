// 本文件是 relay.rs 的分片（第 2/2 部分），通过 include! 并入同一模块，
// 与主文件共用作用域，因此无需额外导入。收纳内容：可注入文件与等待策略的可靠投递实现，确保测试不触碰真实 temp 文件。；向一个已验证描述执行单次 HTTP 投递并校验响应实例身份。；使用单一 wall-clock 截止时间执行连接、写入与完整响应读取。

/// 可注入文件与等待策略的可靠投递实现，确保测试不触碰真实 temp 文件。
fn post_local_hook_at(
    rendezvous_path: &Path,
    slug: &str,
    hook_type: &str,
    body: &[u8],
    retry_count: u8,
    retry_delay: Duration,
) -> anyhow::Result<()> {
    let mut last_retryable = None;
    for attempt in 0..=retry_count {
        let outcome = match load_hook_relay_rendezvous_at(rendezvous_path) {
            Ok(rendezvous) => post_local_hook_once(slug, hook_type, body, &rendezvous),
            Err(error) => Err(PostAttemptError::Retryable(error)),
        };
        match outcome {
            Ok(()) => return Ok(()),
            Err(PostAttemptError::Fatal(error)) => return Err(error),
            Err(PostAttemptError::Retryable(error)) => last_retryable = Some(error),
        }
        if attempt < retry_count {
            thread::sleep(retry_delay);
        }
    }
    Err(last_retryable.unwrap_or_else(|| anyhow!("Hook relay 重试耗尽"))).context(format!(
        "无法连接 LokiMetris Hook listener（共尝试 {} 次）",
        retry_count + 1
    ))
}

/// 向一个已验证描述执行单次 HTTP 投递并校验响应实例身份。
fn post_local_hook_once(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    rendezvous: &HookRelayRendezvous,
) -> Result<(), PostAttemptError> {
    post_local_hook_once_with_timeout(
        slug,
        hook_type,
        body,
        rendezvous,
        LOCAL_RELAY_REQUEST_TIMEOUT,
    )
}

/// 使用单一 wall-clock 截止时间执行连接、写入与完整响应读取。
fn post_local_hook_once_with_timeout(
    slug: &str,
    hook_type: &str,
    body: &[u8],
    rendezvous: &HookRelayRendezvous,
    request_timeout: Duration,
) -> Result<(), PostAttemptError> {
    let started_at = Instant::now();
    let connect_timeout = LOCAL_RELAY_CONNECT_TIMEOUT.min(request_timeout);
    let mut stream =
        TcpStream::connect_timeout(&hook_relay_connect_addr(rendezvous.port), connect_timeout)
            .map_err(|error| {
                PostAttemptError::Retryable(anyhow!(error).context("无法连接 Hook listener"))
            })?;
    stream.set_nodelay(true).map_err(|error| {
        PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook listener 连接"))
    })?;
    let request = encode_local_hook_request(slug, hook_type, body, rendezvous);
    write_hook_request_before_deadline(&mut stream, &request, started_at, request_timeout)?;
    let response = read_hook_response_before_deadline(&mut stream, started_at, request_timeout)?;
    validate_hook_response(&response, &rendezvous.instance_id)
}

/// 返回本次请求的剩余预算；耗尽后不再执行任何阻塞 I/O。
fn remaining_request_budget(
    started_at: Instant,
    request_timeout: Duration,
) -> Result<Duration, PostAttemptError> {
    request_timeout
        .checked_sub(started_at.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 请求超过总时限")))
}

/// 手动推进写入，每次系统调用都只获得整个请求剩余的 wall-clock 预算。
fn write_hook_request_before_deadline(
    stream: &mut TcpStream,
    mut request: &[u8],
    started_at: Instant,
    request_timeout: Duration,
) -> Result<(), PostAttemptError> {
    while !request.is_empty() {
        let remaining = remaining_request_budget(started_at, request_timeout)?;
        stream.set_write_timeout(Some(remaining)).map_err(|error| {
            PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook 请求写入截止时间"))
        })?;
        match stream.write(request) {
            Ok(0) => {
                return Err(PostAttemptError::Fatal(anyhow!(
                    "Hook listener 在请求写完前关闭连接"
                )));
            }
            Ok(written) => request = &request[written..],
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(PostAttemptError::Fatal(
                    anyhow!(error).context("无法写入 Hook 请求"),
                ));
            }
        }
    }
    Ok(())
}

/// 读取到连接结束，同时让持续小流量也不能刷新整次请求的总时限。
fn read_hook_response_before_deadline(
    stream: &mut TcpStream,
    started_at: Instant,
    request_timeout: Duration,
) -> Result<Vec<u8>, PostAttemptError> {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        let remaining = remaining_request_budget(started_at, request_timeout)?;
        stream.set_read_timeout(Some(remaining)).map_err(|error| {
            PostAttemptError::Fatal(anyhow!(error).context("无法配置 Hook 响应读取截止时间"))
        })?;
        match stream.read(&mut chunk) {
            Ok(0) => return Ok(response),
            Ok(read) => {
                response.extend_from_slice(&chunk[..read]);
                if response.len() as u64 > MAX_HOOK_HTTP_RESPONSE_BYTES {
                    return Err(PostAttemptError::Fatal(anyhow!("Hook listener 响应过大")));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                return Err(PostAttemptError::Fatal(
                    anyhow!(error).context("无法读取 Hook listener 响应"),
                ));
            }
        }
    }
}

/// 校验 2xx 状态与响应实例头，避免陈旧端口误把事件提交给 AIMonitor。
fn validate_hook_response(
    response: &[u8],
    expected_instance_id: &str,
) -> Result<(), PostAttemptError> {
    let response = std::str::from_utf8(response)
        .map_err(|_| PostAttemptError::Fatal(anyhow!("Hook listener 响应不是 UTF-8")))?;
    let headers = response
        .split_once("\r\n\r\n")
        .map(|(headers, _)| headers)
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 响应不完整")))?;
    let mut lines = headers.lines();
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| PostAttemptError::Fatal(anyhow!("Hook listener 响应状态无效")))?;
    let response_instance_id = lines.find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.eq_ignore_ascii_case(HOOK_RELAY_INSTANCE_HEADER)
            .then(|| value.trim())
    });
    if response_instance_id != Some(expected_instance_id) {
        return Err(PostAttemptError::Retryable(anyhow!(
            "Hook listener 响应实例身份不匹配"
        )));
    }
    match status {
        200..=299 => Ok(()),
        503 => Err(PostAttemptError::Retryable(anyhow!(
            "Hook listener 暂时不可用：HTTP 503"
        ))),
        _ => Err(PostAttemptError::Fatal(anyhow!(
            "Hook listener 拒绝事件：HTTP {status}"
        ))),
    }
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;

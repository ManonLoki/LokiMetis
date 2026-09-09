//! 在 macOS 上把回环 CDP 监听端口绑定到已验证宿主进程树。

use std::collections::{BTreeSet, HashMap, HashSet};

use super::AppError;

const LSOF_PATH: &str = "/usr/sbin/lsof";
const PS_PATH: &str = "/bin/ps";

/// 验证指定端口只有一个回环 listener，且 owner 是已验证 WorkBuddy 根进程或其后代。
pub(crate) async fn workbuddy_endpoint_owned_by_root(
    port: u16,
    root_pid: u32,
) -> Result<bool, AppError> {
    let port_selector = format!("-iTCP:{port}");
    let listener_output = tokio::process::Command::new(LSOF_PATH)
        .args(["-nP", "-a"])
        .arg(port_selector)
        .args(["-sTCP:LISTEN", "-Fpn"])
        .output()
        .await
        .map_err(|_| owner_inspection_failed())?;
    if !listener_output.status.success() {
        return if listener_output.stdout.is_empty() {
            Ok(false)
        } else {
            Err(owner_inspection_failed())
        };
    }
    let Some(owner_pid) =
        unique_loopback_listener_owner(&String::from_utf8_lossy(&listener_output.stdout), port)
    else {
        return Ok(false);
    };

    let process_output = tokio::process::Command::new(PS_PATH)
        .args(["-axo", "pid=,ppid="])
        .output()
        .await
        .map_err(|_| owner_inspection_failed())?;
    if !process_output.status.success() {
        return Err(owner_inspection_failed());
    }
    let Some(parents) = parse_process_parents(&String::from_utf8_lossy(&process_output.stdout))
    else {
        return Err(owner_inspection_failed());
    };
    Ok(process_descends_from(owner_pid, root_pid, &parents))
}

/// 从 `lsof -Fpn` 输出中提取唯一的回环监听进程；任何通配或歧义绑定均拒绝。
fn unique_loopback_listener_owner(output: &str, port: u16) -> Option<u32> {
    let ipv4 = format!("127.0.0.1:{port}");
    let ipv6 = format!("[::1]:{port}");
    let mut current_pid = None;
    let mut owners = BTreeSet::new();
    let mut saw_listener = false;
    for line in output.lines().filter(|line| !line.is_empty()) {
        match line.as_bytes()[0] {
            b'p' => current_pid = line[1..].parse::<u32>().ok(),
            b'n' => {
                let address = &line[1..];
                if address != ipv4 && address != ipv6 {
                    return None;
                }
                owners.insert(current_pid?);
                saw_listener = true;
            }
            _ => {}
        }
    }
    (saw_listener && owners.len() == 1).then(|| owners.into_iter().next())?
}

/// 解析 `ps` 的 PID/PPID 快照；重复 PID 或畸形行使整份快照失效。
fn parse_process_parents(output: &str) -> Option<HashMap<u32, u32>> {
    let mut parents = HashMap::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut fields = line.split_whitespace();
        let pid = fields.next()?.parse::<u32>().ok()?;
        let parent_pid = fields.next()?.parse::<u32>().ok()?;
        if fields.next().is_some() || parents.insert(pid, parent_pid).is_some() {
            return None;
        }
    }
    (!parents.is_empty()).then_some(parents)
}

/// 沿不可循环的父进程链确认 listener owner 归属于指定已验证根进程。
fn process_descends_from(owner_pid: u32, root_pid: u32, parents: &HashMap<u32, u32>) -> bool {
    if !parents.contains_key(&owner_pid) || !parents.contains_key(&root_pid) {
        return false;
    }
    let mut current = owner_pid;
    let mut visited = HashSet::new();
    loop {
        if current == root_pid {
            return true;
        }
        if current == 0 || !visited.insert(current) {
            return false;
        }
        let Some(parent) = parents.get(&current) else {
            return false;
        };
        current = *parent;
    }
}

/// 返回不包含端口、PID、路径或系统输出的稳定端点归属检查错误。
fn owner_inspection_failed() -> AppError {
    AppError::new(
        "skin.workbuddy_cdp_owner_inspection_failed",
        "无法验证 WorkBuddy 调试端口所属进程，未应用皮肤。",
    )
}

#[cfg(test)]
mod tests {
    use super::{parse_process_parents, process_descends_from, unique_loopback_listener_owner};

    /// 唯一 owner 可同时监听 IPv4 与 IPv6 回环地址。
    #[test]
    fn listener_owner_accepts_one_process_on_loopback_only() {
        let output = "p42\nn127.0.0.1:9442\nn[::1]:9442\n";
        assert_eq!(unique_loopback_listener_owner(output, 9442), Some(42));
    }

    /// 通配地址、多个 owner 或缺失 owner 的监听记录必须保守拒绝。
    #[test]
    fn listener_owner_rejects_wildcard_ambiguous_and_orphan_rows() {
        assert_eq!(unique_loopback_listener_owner("p42\nn*:9442\n", 9442), None);
        assert_eq!(
            unique_loopback_listener_owner("p42\nn127.0.0.1:9442\np43\nn[::1]:9442\n", 9442,),
            None
        );
        assert_eq!(
            unique_loopback_listener_owner("n127.0.0.1:9442\n", 9442),
            None
        );
    }

    /// listener owner 可以经过多个中间进程归属于已验证 WorkBuddy 根。
    #[test]
    fn process_owner_must_descend_from_verified_root() {
        let parents = parse_process_parents("10 1\n20 10\n30 20\n").expect("valid snapshot");
        assert!(process_descends_from(30, 10, &parents));
        assert!(process_descends_from(10, 10, &parents));
        assert!(!process_descends_from(30, 11, &parents));
        assert!(!process_descends_from(99, 99, &parents));
    }

    /// 缺失父进程与循环链不能伪装成已验证根的后代。
    #[test]
    fn process_owner_rejects_missing_and_cyclic_ancestry() {
        let missing = parse_process_parents("20 10\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 9, &missing));
        let cyclic = parse_process_parents("20 30\n30 20\n").expect("valid snapshot");
        assert!(!process_descends_from(30, 10, &cyclic));
    }

    /// 畸形或重复 PID 会使整个进程快照失效，避免部分解析后误接受。
    #[test]
    fn process_parent_parser_rejects_malformed_or_duplicate_rows() {
        assert!(parse_process_parents("10 1 extra\n").is_none());
        assert!(parse_process_parents("10 1\n10 2\n").is_none());
        assert!(parse_process_parents("not-a-pid 1\n").is_none());
    }
}

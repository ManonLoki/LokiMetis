//! 通过 Windows TCP 表验证 CDP 回环监听端口的唯一 owner PID。

use std::collections::HashSet;
use std::ffi::c_void;
use std::mem::size_of;
use std::net::{Ipv4Addr, Ipv6Addr};

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, NO_ERROR};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::Networking::WinSock::{AF_INET, AF_INET6};

/// 返回仅绑定回环地址且所有记录归属同一 PID 的监听端口 owner。
pub(super) fn loopback_listener_owner(port: u16) -> Result<Option<u32>, ()> {
    let ipv4_rows = tcp_listener_rows_for::<MIB_TCPROW_OWNER_PID>(AF_INET.0 as u32)?;
    let ipv6_rows = tcp_listener_rows_for::<MIB_TCP6ROW_OWNER_PID>(AF_INET6.0 as u32)?;
    Ok(unique_loopback_listener_owner(&ipv4_rows, &ipv6_rows, port))
}

/// 仅当目标端口全部监听于回环地址且归属唯一 PID 时返回其 owner。
pub(super) fn unique_loopback_listener_owner(
    ipv4_rows: &[MIB_TCPROW_OWNER_PID],
    ipv6_rows: &[MIB_TCP6ROW_OWNER_PID],
    port: u16,
) -> Option<u32> {
    let matching_ipv4 = ipv4_rows
        .iter()
        .filter(|row| u16::from_be(row.dwLocalPort as u16) == port)
        .collect::<Vec<_>>();
    let matching_ipv6 = ipv6_rows
        .iter()
        .filter(|row| u16::from_be(row.dwLocalPort as u16) == port)
        .collect::<Vec<_>>();
    if matching_ipv4.is_empty() && matching_ipv6.is_empty() {
        return None;
    }
    if matching_ipv4
        .iter()
        .any(|row| !Ipv4Addr::from(row.dwLocalAddr.to_ne_bytes()).is_loopback())
        || matching_ipv6
            .iter()
            .any(|row| !Ipv6Addr::from(row.ucLocalAddr).is_loopback())
    {
        return None;
    }
    let owners = matching_ipv4
        .into_iter()
        .map(|row| row.dwOwningPid)
        .chain(matching_ipv6.into_iter().map(|row| row.dwOwningPid))
        .collect::<HashSet<_>>();
    (owners.len() == 1)
        .then(|| owners.into_iter().next())
        .flatten()
}

/// 读取指定地址族的 owner-PID listener 表，并对系统返回长度做边界复核。
fn tcp_listener_rows_for<Row: Copy>(address_family: u32) -> Result<Vec<Row>, ()> {
    let mut required_bytes = 0_u32;
    let status = unsafe {
        GetExtendedTcpTable(
            None,
            &mut required_bytes,
            false,
            address_family,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if status != ERROR_INSUFFICIENT_BUFFER.0 && status != NO_ERROR.0 {
        return Err(());
    }
    if required_bytes < size_of::<u32>() as u32 {
        return Ok(Vec::new());
    }

    for _ in 0..3 {
        let word_count = (required_bytes as usize).div_ceil(size_of::<u32>());
        let mut buffer = vec![0_u32; word_count];
        let mut returned_bytes = (buffer.len() * size_of::<u32>()) as u32;
        let status = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast::<c_void>()),
                &mut returned_bytes,
                false,
                address_family,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if status == ERROR_INSUFFICIENT_BUFFER.0 {
            required_bytes = returned_bytes;
            continue;
        }
        if status != NO_ERROR.0 || returned_bytes < size_of::<u32>() as u32 {
            return Err(());
        }
        let count = buffer[0] as usize;
        let rows_bytes = count
            .checked_mul(size_of::<Row>())
            .and_then(|value| value.checked_add(size_of::<u32>()))
            .ok_or(())?;
        if rows_bytes > returned_bytes as usize || rows_bytes > buffer.len() * size_of::<u32>() {
            return Err(());
        }
        let rows =
            unsafe { std::slice::from_raw_parts(buffer.as_ptr().add(1).cast::<Row>(), count) };
        return Ok(rows.to_vec());
    }
    Err(())
}

//! KSNI 对象存在不等于桌面显示恢复入口；独立有界 worker 只发布实际注册事实。

use ashpd::zbus::{Connection, Proxy, connection::Builder};
use futures::{FutureExt as _, future::{Either, select}};
use gpui_kit::{BackgroundExecutor, Task};
use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, time::Duration};

/// 唯一异步查询由托盘持有；主线程只读内存，退出取消轮询与在途连接。
pub(super) struct RestoreEntry {
    /// worker 最近一次确认的托盘恢复入口可用性；未知或失败均为 false。
    available: Arc<AtomicBool>,
    /// 周期查询 StatusNotifierWatcher 的后台任务，随托盘释放而取消。
    _worker: Task<()>,
}

impl RestoreEntry {
    /// 初始未知视为不可恢复；只有活宿主与本进程精确托盘注册均确认才允许隐藏。
    pub fn new(executor: BackgroundExecutor) -> Self {
        let available = Arc::new(AtomicBool::new(false));
        let published = available.clone();
        let timer = executor.clone();
        let worker = executor.spawn(async move {
            let mut connection = None;
            loop {
                let probe = async {
                    if connection.is_none() {
                        connection = Some(Builder::session()?.method_timeout(Duration::from_millis(500)).build().await?);
                    }
                    query(connection.as_ref().unwrap()).await
                };
                let observed = select(probe.boxed(), timer.timer(Duration::from_secs(2)).boxed()).await;
                let available = matches!(observed, Either::Left((Ok(true), _)));
                let failed = !matches!(observed, Either::Left((Ok(_), _)));
                // 先释放可能借用连接的未完成 future，再决定下轮是否重建连接。
                drop(observed);
                published.store(available, Ordering::Release);
                if failed { connection = None; }
                timer.timer(Duration::from_secs(1)).await;
            }
        });
        Self { available, _worker: worker }
    }

    /// UI 关闭边界不查询 D-Bus，不把未知或失败当作已经显示的入口。
    pub fn available(&self) -> bool { self.available.load(Ordering::Acquire) }
}

/// 兼容 KDE 的 service/path 与 GNOME 的 service@/path 表示；不接受非绝对对象路径。
fn item_address(item: &str) -> Option<(&str, &str)> {
    let (service, path) = if let Some((service, path)) = item.split_once('@') {
        (service, path)
    } else if let Some(index) = item.find('/') {
        (&item[..index], &item[index..])
    } else { (item, "/StatusNotifierItem") };
    (!service.is_empty() && path.starts_with('/') && !path.contains('@')).then_some((service, path))
}

/// watcher 必须声明真实宿主，且注册列表中须有本进程 main_tray；其它应用不能代替恢复入口。
async fn query(connection: &Connection) -> ashpd::zbus::Result<bool> {
    let watcher = Proxy::new(connection, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher", "org.kde.StatusNotifierWatcher").await?;
    if !watcher.get_property::<bool>("IsStatusNotifierHostRegistered").await? { return Ok(false); }
    let items: Vec<String> = watcher.get_property("RegisteredStatusNotifierItems").await?;
    let bus = ashpd::zbus::fdo::DBusProxy::new(connection).await?;
    let deadline = std::time::Instant::now() + Duration::from_secs(1);
    for item in items.iter().take(128) {
        if std::time::Instant::now() >= deadline { break; }
        let Some((service, path)) = item_address(item) else { continue; };
        let Ok(name) = service.try_into() else { continue; };
        if bus.get_connection_unix_process_id(name).await.ok() != Some(std::process::id()) { continue; }
        let entry = Proxy::new(connection, service, path, "org.kde.StatusNotifierItem").await?;
        if entry.get_property::<String>("Id").await? == "main_tray" { return Ok(true); }
    }
    Ok(false)
}

#[cfg(test)]
#[path = "tray_linux_tests.rs"]
mod tests;

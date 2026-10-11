//! 关窗时与延迟隐藏前均确认托盘恢复入口，失去 Explorer 时保留可见窗口。

/// 仅已确认初始恢复入口的关闭请求可以进入延迟隐藏阶段。
pub(super) struct Request;
impl Request {
    /// 初始失败允许正常关闭，不能仅凭托盘 Rust 对象存在阻止关闭。
    pub fn begin(available: impl FnOnce() -> bool) -> Option<Self> {
        available().then_some(Self)
    }
    /// 后来的显示/退出或托盘失效都取消旧隐藏；检查与调用在同一 UI 回合完成。
    pub fn finish(self, current: bool, available: impl FnOnce() -> bool, hide: impl FnOnce()) -> bool {
        if !current || !available() { return false; }
        hide();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::Request;
    use std::cell::Cell;
    /// 真实关闭请求使用的两阶段门禁覆盖初始不可用、排队后失效与后来的显示撤销。
    #[test]
    fn close_requires_a_live_restore_entry_at_both_boundaries() {
        assert!(Request::begin(|| false).is_none());
        let available = Cell::new(true);
        let hidden = Cell::new(0);
        let request = Request::begin(|| available.get()).unwrap();
        available.set(false);
        assert!(!request.finish(true, || available.get(), || hidden.set(hidden.get() + 1)));
        assert_eq!(hidden.get(), 0);
        available.set(true);
        let request = Request::begin(|| available.get()).unwrap();
        assert!(request.finish(true, || available.get(), || hidden.set(hidden.get() + 1)));
        assert_eq!(hidden.get(), 1);
        let request = Request::begin(|| available.get()).unwrap();
        assert!(!request.finish(false, || panic!("较新的显示或退出已撤销隐藏"), || hidden.set(hidden.get() + 1)));
        assert_eq!(hidden.get(), 1);
    }
}

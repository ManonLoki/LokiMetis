//! 跟踪尚未完成或取消的编码，避免 CPU 提前覆盖 GPU 正在读取的图集区域。

use std::collections::BTreeSet;

/// 不保存已结束的历史；退休资源只等待退休时已经存在的编码。
pub(crate) struct RetirementQueue<T> {
    next_epoch: u64,
    active: BTreeSet<u64>,
    retired: Vec<(u64, T)>,
}

impl<T> Default for RetirementQueue<T> {
    fn default() -> Self {
        Self {
            next_epoch: 0,
            active: BTreeSet::new(),
            retired: Vec::new(),
        }
    }
}

impl<T> RetirementQueue<T> {
    /// 在第一次纹理查询前保留编码；未提交路径也需要取消此 epoch。
    pub(crate) fn begin(&mut self) -> u64 {
        self.next_epoch = self
            .next_epoch
            .checked_add(1)
            .expect("Metal atlas epoch exhausted");
        self.active.insert(self.next_epoch);
        self.next_epoch
    }

    /// 没有尚存编码时立即归还；否则保留完整物理分配。
    pub(crate) fn retire(&mut self, value: T) -> Option<T> {
        if let Some(&epoch) = self.active.last() {
            self.retired.push((epoch, value));
            None
        } else {
            Some(value)
        }
    }

    /// GPU 完成和未提交取消都结束一个确切 epoch，不能跳过较早的活跃编码。
    pub(crate) fn finish(&mut self, epoch: u64) -> Vec<T> {
        assert!(
            self.active.remove(&epoch),
            "Metal atlas frame finished twice"
        );
        let first_active = self.active.first().copied();
        let mut ready = Vec::new();
        let mut pending = Vec::new();
        for (target, value) in self.retired.drain(..) {
            if first_active.is_none_or(|first| first > target) {
                ready.push(value);
            } else {
                pending.push((target, value));
            }
        }
        self.retired = pending;
        ready
    }
}

#[cfg(test)]
mod tests {
    use super::RetirementQueue;

    /// 空闲状态保持原有即时释放行为。
    #[test]
    fn atlas_retirement_idle_is_immediate() {
        let mut q = RetirementQueue::default();
        assert_eq!(q.retire("idle"), Some("idle"));
        assert!(q.retired.is_empty());
    }

    /// 完成前仍保有待释放资源，完成后只归还一次。
    #[test]
    fn atlas_retirement_waits_for_active_encoding() {
        let mut q = RetirementQueue::default();
        let epoch = q.begin();
        assert_eq!(q.retire("texture"), None);
        assert_eq!(q.retired.len(), 1);
        assert_eq!(q.finish(epoch), vec!["texture"]);
        assert!(q.active.is_empty() && q.retired.is_empty());
    }

    /// 新编码不阻止其开始之前退休的区域被安全回收。
    #[test]
    fn atlas_retirement_future_encoding_does_not_hold_old_tile() {
        let mut q = RetirementQueue::default();
        let first = q.begin();
        q.retire("old");
        let future = q.begin();
        assert_eq!(q.finish(first), vec!["old"]);
        assert!(q.active.contains(&future));
        assert!(q.finish(future).is_empty());
    }

    /// 乱序完成或取消不能跨过仍可能提交的较早编码。
    #[test]
    fn atlas_retirement_out_of_order_finish_keeps_prepared_frame() {
        let mut q = RetirementQueue::default();
        let prepared = q.begin();
        let later = q.begin();
        q.retire("shared region");
        assert!(q.finish(later).is_empty());
        assert_eq!(q.finish(prepared), vec!["shared region"]);
    }

    /// 不同退休批次按各自的已有编码释放。
    #[test]
    fn atlas_retirement_releases_precise_waves() {
        let mut q = RetirementQueue::default();
        let first = q.begin();
        q.retire("A");
        let second = q.begin();
        q.retire("B");
        assert_eq!(q.finish(first), vec!["A"]);
        assert_eq!(q.finish(second), vec!["B"]);
    }

    /// 长时间运行不积累已结束的 epoch 或退休记录。
    #[test]
    fn atlas_retirement_does_not_retain_epoch_history() {
        let mut q = RetirementQueue::default();
        for i in 0..10_000 {
            let epoch = q.begin();
            q.retire(i);
            assert_eq!(q.finish(epoch), vec![i]);
            assert!(q.active.is_empty() && q.retired.is_empty());
        }
    }
}

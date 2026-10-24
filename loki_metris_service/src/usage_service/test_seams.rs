//! 仅测试编译的 UsageService 接缝：生产退出路径使用 finish_with_pet。

use super::*;

impl UsageService {
    /// 停止新 writer 并有界等待合作收尾；不能强停的blocking仍由原owner保留。
    pub fn finish(&self) { self.finish_with_pet(PetShutdownPlan::default()); }
}

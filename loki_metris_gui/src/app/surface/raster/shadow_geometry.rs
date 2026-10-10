//! 圆角挖空与切片只依赖几何；按行压缩相同覆盖率，避免常驻完整 f64 像素表。

use super::{Pixels, ShadowSample, Slice, cdf, rounded_coverage, slices};

/// 连段终点为开区间，覆盖率保留原算法的完整 f64 位值。
struct MaskRun {
    end: u32,
    outside: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::shadow_geometry_tests::oracle_rounded_coverage;

    /// 检查每个展开覆盖率的 f64 位值，阻止最终字节偶合掩盖量化错误。
    #[test]
    fn compressed_runs_preserve_unquantized_coverage_bits() {
        for (width, height, scale, radius) in [
            (0, 0, 1., -0.), (1, 50, 1.125, 20.), (50, 1, 1.5, 20.),
            (50, 50, 1.25, 0.25), (1000, 50, 2., 12.),
            (20000, 10000, 1.125, 19.999), (20000, 10000, 2., 20.),
        ] {
            let geometry = ShadowGeometry::new(width, height, scale, radius);
            let radius = (radius * scale)
                .min(f64::from(geometry.card_width.min(geometry.card_height)) / 2.);
            assert_eq!(geometry.row_offsets.len(), (geometry.height - 1) as usize);
            for (row, offsets) in (1..geometry.height - 1).zip(geometry.row_offsets.windows(2)) {
                let mut start = 1;
                for run in &geometry.runs[offsets[0]..offsets[1]] {
                    assert!(run.end > start && run.end < geometry.width);
                    for column in start..run.end {
                        let outside = 1. - oracle_rounded_coverage(
                            f64::from(column) - f64::from(geometry.near_x) - 0.5,
                            f64::from(row) - f64::from(geometry.near_y) - 0.5,
                            f64::from(geometry.card_width), f64::from(geometry.card_height), radius,
                        );
                        assert_eq!(run.outside.to_bits(), outside.to_bits());
                    }
                    start = run.end;
                }
                assert_eq!(start, geometry.width - 1);
            }
        }
    }
}

/// 每个 owner 仅保留一个计划；动态 Gaussian 参数不进入几何缓存。
pub(in crate::app::surface) struct ShadowGeometry {
    width: u32,
    height: u32,
    card_width: u32,
    card_height: u32,
    near_x: u32,
    near_y: u32,
    pub(super) x: Vec<Slice>,
    pub(super) y: Vec<Slice>,
    row_offsets: Vec<usize>,
    runs: Vec<MaskRun>,
}

impl ShadowGeometry {
    /// 首次或尺寸、比例、半径变化时计算原始覆盖；颜色与动画不触发重建。
    pub fn new(width: u32, height: u32, scale: f64, radius: f64) -> Self {
        let cap = |value: f64| (value * scale).ceil() as u32;
        let (near_x, near_y) = (cap(76.), cap(52.));
        let (card_width, x) = slices(width, cap(126.), cap(126.), near_x, cap(76.));
        let (card_height, y) = slices(height, cap(146.), cap(106.), near_y, cap(100.));
        let width = card_width + near_x + cap(76.) + 2;
        let height = card_height + near_y + cap(100.) + 2;
        assert!(radius >= 0. && radius <= 20., "品牌材质半径必须处于当前产品的0..20px范围");
        let radius = (radius * scale).min(f64::from(card_width.min(card_height)) / 2.);
        let mut runs: Vec<MaskRun> = Vec::new();
        let mut row_offsets = vec![0];
        for row in 1..height - 1 {
            let start = runs.len();
            for column in 1..width - 1 {
                let px = f64::from(column) - f64::from(near_x) - 0.5;
                let py = f64::from(row) - f64::from(near_y) - 0.5;
                let outside = 1. - rounded_coverage(px, py, f64::from(card_width), f64::from(card_height), radius);
                if runs.len() > start && runs.last().unwrap().outside.to_bits() == outside.to_bits() {
                    runs.last_mut().unwrap().end = column + 1;
                } else {
                    runs.push(MaskRun { end: column + 1, outside });
                }
            }
            row_offsets.push(runs.len());
        }
        Self { width, height, card_width, card_height, near_x, near_y, x, y, row_offsets, runs }
    }

    /// 按原乘法顺序重着色；透明内像素仍保留直通 BGRA，外围保护圈仍全零。
    pub fn render(&self, sample: ShadowSample) -> Pixels {
        let cast = [-sample.spread, sample.dy - sample.spread,
            f64::from(self.card_width) + sample.spread,
            f64::from(self.card_height) + sample.dy + sample.spread];
        let profile = |position: f64, lo: f64, hi: f64| {
            if hi <= lo { 0. } else {
                cdf((position - lo) / sample.sigma) - cdf((position - hi) / sample.sigma)
            }
        };
        let ax: Vec<_> = (0..self.width).map(|i| profile(f64::from(i) - f64::from(self.near_x) - 0.5, cast[0], cast[2])).collect();
        let ay: Vec<_> = (0..self.height).map(|i| profile(f64::from(i) - f64::from(self.near_y) - 0.5, cast[1], cast[3])).collect();
        let mut bytes = vec![0; self.width as usize * self.height as usize * 4];
        for (row, offsets) in (1..self.height - 1).zip(self.row_offsets.windows(2)) {
            let mut start = 1;
            for run in &self.runs[offsets[0]..offsets[1]] {
                for column in start..run.end {
                    let alpha = sample.alpha * ax[column as usize] * ay[row as usize] * run.outside;
                    let index = (row as usize * self.width as usize + column as usize) * 4;
                    bytes[index..index + 3].copy_from_slice(&sample.bgra);
                    bytes[index + 3] = (alpha * 255.).round().clamp(0., 255.) as u8;
                }
                start = run.end;
            }
        }
        Pixels { width: self.width, height: self.height, bytes }
    }

    /// 切片仍携带实际长轴目标，绘制不复制或累计历史映射。
    pub fn slices(&self) -> (&[Slice], &[Slice]) { (&self.x, &self.y) }

    /// 回归读取实际计划占用，包含 Vec 容量而非仅有效元素数。
    #[cfg(test)]
    pub(super) fn allocated_bytes(&self) -> usize {
        self.row_offsets.capacity() * std::mem::size_of::<usize>()
            + self.runs.capacity() * std::mem::size_of::<MaskRun>()
            + (self.x.capacity() + self.y.capacity()) * std::mem::size_of::<Slice>()
    }
}
